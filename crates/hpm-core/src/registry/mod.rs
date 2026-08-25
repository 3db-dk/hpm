//! Package registry support for HPM.
//!
//! This module provides a trait-based registry abstraction with two implementations:
//! - [`ApiRegistry`]: HTTP-based registry (e.g., `https://api.tumbletrove.com/v1/registry`)
//! - [`GitRegistry`]: Git-hosted index (Cargo-style, one JSON-lines file per package)
//!
//! Registries allow HPM to resolve package names to download URLs, checksums,
//! and dependency information without requiring users to specify Git URLs manually.

pub mod api;
pub mod git;
pub mod types;

use async_trait::async_trait;
use hpm_package::IoOp;
use std::collections::HashMap;
use thiserror::Error;

pub use api::ApiRegistry;
pub use git::GitRegistry;
pub use types::{PlatformTag, RegistryEntry, SearchResults};

/// Errors that can occur during registry operations.
#[derive(Error, Debug)]
pub enum RegistryError {
    #[error("Package '{name}' not found in registry")]
    PackageNotFound { name: String },

    #[error("Version '{version}' of package '{name}' not found in registry")]
    VersionNotFound { name: String, version: String },

    #[error(
        "Version '{version}' of package '{name}' has no build compatible with host platform '{host}'"
    )]
    NoCompatibleBuild {
        name: String,
        version: String,
        host: String,
    },

    #[error("Failed to connect to registry: {0}")]
    NetworkError(#[from] reqwest::Error),

    #[error("Failed to parse registry data: {0}")]
    ParseError(String),

    #[error(transparent)]
    Io(#[from] IoOp),

    #[error("Git operation failed: {0}")]
    GitError(String),

    #[error("Checksum mismatch for {name}@{version}: expected {expected}, got {actual}")]
    ChecksumMismatch {
        name: String,
        version: String,
        expected: String,
        actual: String,
    },

    #[error(
        "Dependency pins registry '{name}', which is not configured. \
         Add it with `hpm registry add <url> --name {name}`, or drop the \
         `registry` key to resolve across all configured registries."
    )]
    UnknownRegistry { name: String },
}

/// Trait for package registries.
///
/// Both API-based and Git-index-based registries implement this trait,
/// providing a unified interface for package discovery and resolution.
#[async_trait]
pub trait Registry: Send + Sync {
    /// Search the registry for packages matching a query string.
    async fn search(&self, query: &str) -> Result<SearchResults, RegistryError>;

    /// Get all versions of a package.
    async fn get_versions(&self, name: &str) -> Result<Vec<RegistryEntry>, RegistryError>;

    /// Get a specific version of a package.
    async fn get_version(&self, name: &str, version: &str) -> Result<RegistryEntry, RegistryError>;

    /// Refresh the local registry cache (fetch latest index).
    async fn refresh(&self) -> Result<(), RegistryError>;

    /// Get the registry display name.
    fn name(&self) -> &str;
}

/// A collection of registries that can be searched in order.
pub struct RegistrySet {
    registries: Vec<Box<dyn Registry>>,
}

/// Merged search results across a [`RegistrySet`], with the registries that
/// could not be reached listed explicitly.
pub struct SetSearchResults {
    /// Matching entries from every reachable registry.
    pub packages: Vec<RegistryEntry>,
    /// Registries skipped because they were unreachable, with the error.
    pub unavailable: Vec<(String, RegistryError)>,
}

impl RegistrySet {
    pub fn new() -> Self {
        Self {
            registries: Vec::new(),
        }
    }

    /// Build a `RegistrySet` from a full `Config`. Convenience wrapper around
    /// [`Self::from_configs`] for the common case where the caller just wants
    /// the set defined by the user's global config.
    pub fn from_config(config: &hpm_config::Config) -> Result<Self, RegistryError> {
        Self::from_configs(&config.registries, &config.storage.registry_cache_dir)
    }

    /// Build a `RegistrySet` from registry configurations.
    ///
    /// Use this when the registry list is overridden (e.g. by a project
    /// manifest's `[[registries]]`) rather than coming from `Config`. For the
    /// straight `Config`-driven case, prefer [`Self::from_config`].
    ///
    /// All API registries are built without authentication. For caller-driven
    /// auth (e.g. an embedder passing a bearer token for visibility-gated
    /// registries), use [`Self::from_configs_with_auth`].
    ///
    /// # Arguments
    /// * `registries` - Registry configurations to add
    /// * `registry_cache_dir` - Directory for caching git registry indices
    pub fn from_configs(
        registries: &[hpm_config::RegistrySourceConfig],
        registry_cache_dir: &std::path::Path,
    ) -> Result<Self, RegistryError> {
        Self::from_configs_with_auth(registries, registry_cache_dir, None)
    }

    /// Like [`Self::from_configs`], but attaches a bearer token to every API
    /// registry's HTTP client when `auth_token` is `Some`.
    ///
    /// Git registries ignore the token — there is no auth story for the git
    /// index today. When `auth_token` is `None`, behavior is identical to
    /// [`Self::from_configs`].
    ///
    /// A registry entry that cannot be constructed (bad URL, bad token) is a
    /// hard error: silently dropping it from the set would later surface as
    /// a misleading "package not found".
    pub fn from_configs_with_auth(
        registries: &[hpm_config::RegistrySourceConfig],
        registry_cache_dir: &std::path::Path,
        auth_token: Option<&str>,
    ) -> Result<Self, RegistryError> {
        let mut set = Self::new();

        for reg in registries {
            match reg.registry_type {
                hpm_config::RegistryType::Api => {
                    let api_reg = ApiRegistry::with_auth_token(&reg.name, &reg.url, auth_token)
                        .map_err(|e| {
                            RegistryError::ParseError(format!(
                                "Registry '{}' could not be constructed: {}",
                                reg.name, e
                            ))
                        })?;
                    set.add(Box::new(api_reg));
                }
                hpm_config::RegistryType::Git => {
                    let cache_dir = registry_cache_dir.join(&reg.name);
                    let git_reg = GitRegistry::new(&reg.name, &reg.url, &cache_dir);
                    set.add(Box::new(git_reg));
                }
            }
        }

        Ok(set)
    }

    pub fn add(&mut self, registry: Box<dyn Registry>) {
        self.registries.push(registry);
    }

    /// Search all registries and merge results.
    ///
    /// An unreachable registry does not abort the whole search (the healthy
    /// registries' results are still useful), but it is reported in
    /// [`SetSearchResults::unavailable`] so callers can tell the user the
    /// results may be incomplete instead of silently omitting them.
    pub async fn search(&self, query: &str) -> Result<SetSearchResults, RegistryError> {
        let mut all_packages = Vec::new();
        let mut unavailable = Vec::new();
        for registry in &self.registries {
            match registry.search(query).await {
                // Per-registry, so the same package served by two
                // registries still shows both — only one registry's own
                // per-platform build rows for a version are collapsed.
                Ok(results) => all_packages.extend(dedupe_builds_by_version(
                    results.packages,
                    hpm_package::Platform::current(),
                )),
                Err(e @ RegistryError::NetworkError(_)) => {
                    unavailable.push((registry.name().to_string(), e));
                }
                Err(e) => return Err(e),
            }
        }
        Ok(SetSearchResults {
            packages: all_packages,
            unavailable,
        })
    }

    /// The registries a lookup should consider.
    ///
    /// `None` means the whole set, searched in configured order. `Some(name)`
    /// restricts to the single registry with that name — the dependency pinned
    /// it, so falling back to the rest of the set would resolve the package
    /// from a source the manifest explicitly did not ask for.
    fn selected(&self, registry: Option<&str>) -> Result<Vec<&dyn Registry>, RegistryError> {
        let Some(want) = registry else {
            return Ok(self.registries.iter().map(|r| r.as_ref()).collect());
        };
        let found: Vec<&dyn Registry> = self
            .registries
            .iter()
            .map(|r| r.as_ref())
            .filter(|r| r.name() == want)
            .collect();
        if found.is_empty() {
            return Err(RegistryError::UnknownRegistry {
                name: want.to_string(),
            });
        }
        Ok(found)
    }

    /// Resolve a package name across all registries (first match wins).
    pub async fn get_versions(&self, name: &str) -> Result<Vec<RegistryEntry>, RegistryError> {
        self.get_versions_in(name, None).await
    }

    /// Like [`Self::get_versions`], but restricted to a pinned registry when
    /// `registry` is `Some`.
    pub async fn get_versions_in(
        &self,
        name: &str,
        registry: Option<&str>,
    ) -> Result<Vec<RegistryEntry>, RegistryError> {
        for registry in self.selected(registry)? {
            match registry.get_versions(name).await {
                // One row per build becomes one row per version here: this is
                // the version-listing API, and its callers (a version picker,
                // `highest_matching`) must not see 1.9.2 twice because it
                // shipped for two platforms.
                Ok(versions) if !versions.is_empty() => {
                    return Ok(dedupe_builds_by_version(
                        versions,
                        hpm_package::Platform::current(),
                    ));
                }
                Ok(_) => continue,
                Err(RegistryError::PackageNotFound { .. }) => continue,
                Err(e) => return Err(e),
            }
        }
        Err(RegistryError::PackageNotFound {
            name: name.to_string(),
        })
    }

    /// Resolve a specific version across all registries (first match wins).
    pub async fn get_version(
        &self,
        name: &str,
        version: &str,
    ) -> Result<RegistryEntry, RegistryError> {
        self.get_version_in(name, version, None).await
    }

    /// Like [`Self::get_version`], but restricted to a pinned registry when
    /// `registry` is `Some`.
    pub async fn get_version_in(
        &self,
        name: &str,
        version: &str,
        registry: Option<&str>,
    ) -> Result<RegistryEntry, RegistryError> {
        for registry in self.selected(registry)? {
            match registry.get_version(name, version).await {
                Ok(entry) => return Ok(entry),
                Err(RegistryError::PackageNotFound { .. })
                | Err(RegistryError::VersionNotFound { .. }) => continue,
                Err(e) => return Err(e),
            }
        }
        Err(RegistryError::VersionNotFound {
            name: name.to_string(),
            version: version.to_string(),
        })
    }

    pub fn is_empty(&self) -> bool {
        self.registries.is_empty()
    }

    /// Resolve a version requirement to a concrete registry entry.
    ///
    /// An exact semver (`"1.2.3"`) is looked up directly — this allows
    /// pinning a yanked version deliberately. Anything else (`"^1"`,
    /// `">=2, <3"`, `"*"`) resolves to the highest non-yanked version
    /// matching the requirement.
    pub async fn resolve(&self, name: &str, req: &str) -> Result<RegistryEntry, RegistryError> {
        self.resolve_in(name, req, None).await
    }

    /// Like [`Self::resolve`], but restricted to a pinned registry when
    /// `registry` is `Some`.
    pub async fn resolve_in(
        &self,
        name: &str,
        req: &str,
        registry: Option<&str>,
    ) -> Result<RegistryEntry, RegistryError> {
        if semver::Version::parse(req).is_ok() {
            return self.get_version_in(name, req, registry).await;
        }
        let parsed = semver::VersionReq::parse(req).map_err(|e| {
            RegistryError::ParseError(format!("Invalid version requirement '{}': {}", req, e))
        })?;
        let versions = self.get_versions_in(name, registry).await?;
        highest_matching(&versions, &parsed)
            .cloned()
            .ok_or_else(|| RegistryError::VersionNotFound {
                name: name.to_string(),
                version: req.to_string(),
            })
    }
}

/// Collapse per-platform build rows into one entry per `(name, version)`.
///
/// A registry lists one [`RegistryEntry`] per *build*, not per version: a
/// package publishing a Windows and a Linux archive for 1.9.2 answers
/// `/packages/<name>` with two rows differing only in `dl`, `cksum` and
/// `platform`. That is the right shape for [`Registry::get_versions`] — the
/// trait method feeds [`select_build_for_host`], which needs every build to
/// choose from — but [`RegistrySet::get_versions`] and [`RegistrySet::search`]
/// are the *version* views, and passing builds straight through makes every
/// multi-platform release appear once per platform. Downstream that is not
/// cosmetic: [`highest_matching`] takes the last of the equal-version rows
/// with no host filtering at all, so an unpinned `^1` on a two-platform
/// package can resolve to the other platform's archive.
///
/// The survivor of each group is the host's build, then a universal one, then
/// the first row — so a version whose builds are all for other platforms
/// still *lists*, carrying its own metadata, rather than vanishing from a
/// version picker. Ruling it uninstallable here is
/// [`select_build_for_host`]'s job at install time, not the listing's.
/// Non-yanked builds win over yanked ones within a version, so a yanked
/// host build cannot hide a live one from `highest_matching`; a version whose
/// builds are *all* yanked still lists, and still reads as yanked.
///
/// Groups keep first-appearance order, preserving the registry's own ordering
/// (newest first, for the API registry).
///
/// `host` is a parameter rather than a [`hpm_package::Platform::current`] call
/// inside, so the platform preference can be tested from any host — a test
/// that only ever sees the machine it runs on cannot tell "picked the host's
/// build" apart from "picked the first row".
fn dedupe_builds_by_version(
    entries: Vec<RegistryEntry>,
    host: Option<hpm_package::Platform>,
) -> Vec<RegistryEntry> {
    let mut order: Vec<(String, String)> = Vec::new();
    let mut groups: HashMap<(String, String), Vec<RegistryEntry>> = HashMap::new();
    for entry in entries {
        let key = (entry.name.clone(), entry.version.clone());
        groups
            .entry(key.clone())
            .or_insert_with(|| {
                order.push(key.clone());
                Vec::new()
            })
            .push(entry);
    }
    order
        .into_iter()
        .filter_map(|key| {
            let builds = groups.remove(&key)?;
            let live: Vec<RegistryEntry> = builds.iter().filter(|b| !b.yanked).cloned().collect();
            let pool = if live.is_empty() { &builds } else { &live };
            select_build(pool, host)
                .cloned()
                .or_else(|| pool.first().cloned())
        })
        .collect()
}

/// Pick the best build for the host: exact platform match first, then a
/// universal entry. No silent positional fallback — if the registry
/// annotates every build but none match the host, the caller should error.
///
/// Shared by both registry implementations so a git index serving
/// per-platform builds selects exactly like the API registry does.
pub(crate) fn select_build(
    builds: &[RegistryEntry],
    host: Option<hpm_package::Platform>,
) -> Option<&RegistryEntry> {
    if let Some(host) = host
        && let Some(b) = builds
            .iter()
            .find(|b| b.platform.as_ref().is_some_and(|tag| tag.matches(host)))
    {
        return Some(b);
    }
    builds
        .iter()
        .find(|b| b.platform.as_ref().is_none_or(PlatformTag::is_universal))
}

/// [`select_build`] against the current host, erroring with
/// [`RegistryError::NoCompatibleBuild`] when nothing matches. `builds`
/// must already be filtered to the requested `name`/`version` — they only
/// feed the error message.
pub(crate) fn select_build_for_host<'a>(
    builds: &'a [RegistryEntry],
    name: &str,
    version: &str,
) -> Result<&'a RegistryEntry, RegistryError> {
    let host = hpm_package::Platform::current();
    select_build(builds, host).ok_or_else(|| RegistryError::NoCompatibleBuild {
        name: name.to_string(),
        version: version.to_string(),
        host: host
            .map(|p| p.as_str().to_string())
            .unwrap_or_else(|| format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)),
    })
}

/// The entry with the highest non-yanked semver version matching `req`.
/// Entries whose version does not parse as semver cannot match a semver
/// requirement and are skipped.
pub fn highest_matching<'a>(
    entries: &'a [RegistryEntry],
    req: &semver::VersionReq,
) -> Option<&'a RegistryEntry> {
    entries
        .iter()
        .filter(|e| !e.yanked)
        .filter_map(|e| semver::Version::parse(&e.version).ok().map(|v| (v, e)))
        .filter(|(v, _)| req.matches(v))
        .max_by(|(a, _), (b, _)| a.cmp(b))
        .map(|(_, e)| e)
}

impl Default for RegistrySet {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod dedupe_tests {
    use super::*;
    use hpm_package::Platform;
    use proptest::prelude::*;

    /// Every host platform, so a test asserting "the host's build won" is
    /// exhaustive over the dimension that matters rather than only exercising
    /// whatever machine happens to run it.
    const HOSTS: [Platform; 6] = [
        Platform::LinuxX86_64,
        Platform::LinuxAarch64,
        Platform::MacosX86_64,
        Platform::MacosAarch64,
        Platform::WindowsX86_64,
        Platform::WindowsAarch64,
    ];

    fn build(version: &str, platform: Option<&str>, yanked: bool) -> RegistryEntry {
        RegistryEntry {
            name: "tumblehead/tumblerig".to_string(),
            version: version.to_string(),
            cksum: None,
            dl: format!(
                "https://pkg.example/TumbleRig-{}-{}.zip",
                version,
                platform.unwrap_or("any")
            ),
            sig: None,
            kid: None,
            houdini_compat: None,
            platform: platform.map(|p| PlatformTag::from(p.to_string())),
            yanked,
            description: None,
            author: None,
            created_at: None,
        }
    }

    /// The reported bug: TumbleRig ships a Windows *and* a Linux archive from
    /// 1.7.9 on, so the registry answers with two rows per version and the
    /// project editor's version select rendered each of them once.
    #[test]
    fn multi_platform_versions_list_once() {
        let entries = vec![
            build("1.9.2", Some("windows-x86_64"), false),
            build("1.9.2", Some("linux-x86_64"), false),
            build("1.7.4", Some("windows-x86_64"), false),
        ];
        for host in HOSTS {
            let versions = dedupe_builds_by_version(entries.clone(), Some(host));
            let listed: Vec<&str> = versions.iter().map(|e| e.version.as_str()).collect();
            assert_eq!(listed, ["1.9.2", "1.7.4"], "host {host:?}");
        }
    }

    /// The survivor is the host's build, not whichever row the registry
    /// happened to serve first — this is what `highest_matching` hands to the
    /// installer for an unpinned range, so picking positionally downloads
    /// another platform's archive.
    #[test]
    fn the_surviving_build_is_the_host_s() {
        let entries = vec![
            build("1.9.2", Some("windows-x86_64"), false),
            build("1.9.2", Some("linux-x86_64"), false),
            build("1.9.2", Some("macos-aarch64"), false),
        ];
        for host in HOSTS {
            let versions = dedupe_builds_by_version(entries.clone(), Some(host));
            assert_eq!(versions.len(), 1, "host {host:?}");
            let dl = &versions[0].dl;
            let has_host_build = entries
                .iter()
                .any(|e| e.platform.as_ref().is_some_and(|t| t.matches(host)));
            if has_host_build {
                assert!(dl.contains(host.as_str()), "host {host:?} got {dl}");
            }
        }
    }

    /// A version built only for other platforms still *lists*. Dropping it
    /// would empty the version picker on a host the package has no build for
    /// (macOS, for TumbleRig) — deciding it can't be installed belongs to
    /// `select_build_for_host` at install time.
    #[test]
    fn a_version_with_no_host_build_still_lists() {
        let entries = vec![build("1.9.2", Some("windows-x86_64"), false)];
        let versions = dedupe_builds_by_version(entries, Some(Platform::MacosAarch64));
        assert_eq!(versions.len(), 1);
        assert_eq!(versions[0].version, "1.9.2");
    }

    /// A yanked build must not shadow a live one at the same version:
    /// `highest_matching` filters on `yanked`, so surfacing the yanked row
    /// would make a perfectly installable version unresolvable.
    #[test]
    fn a_live_build_wins_over_a_yanked_one() {
        let entries = vec![
            build("1.9.2", Some("windows-x86_64"), true),
            build("1.9.2", Some("linux-x86_64"), false),
        ];
        let versions = dedupe_builds_by_version(entries, Some(Platform::WindowsX86_64));
        assert_eq!(versions.len(), 1);
        assert!(!versions[0].yanked);
    }

    /// ...but a version whose every build is yanked still lists, still yanked,
    /// so `VersionYanked` stays distinguishable from `VersionUnavailable`.
    #[test]
    fn an_all_yanked_version_still_lists_as_yanked() {
        let entries = vec![
            build("1.9.2", Some("windows-x86_64"), true),
            build("1.9.2", Some("linux-x86_64"), true),
        ];
        let versions = dedupe_builds_by_version(entries, Some(Platform::WindowsX86_64));
        assert_eq!(versions.len(), 1);
        assert!(versions[0].yanked);
    }

    /// Rows for different packages are never merged — `search` collapses
    /// builds across a whole result set, not just one package's.
    #[test]
    fn distinct_packages_are_not_merged() {
        let mut a = build("1.0.0", Some("linux-x86_64"), false);
        a.name = "acme/one".to_string();
        let mut b = build("1.0.0", Some("linux-x86_64"), false);
        b.name = "acme/two".to_string();
        let versions = dedupe_builds_by_version(vec![a, b], Some(Platform::LinuxX86_64));
        assert_eq!(versions.len(), 2);
    }

    /// Platforms are drawn from a small shared pool so builds actually
    /// collide on `(name, version)` — over free strings every row would be
    /// its own group and the properties below would hold vacuously.
    fn build_rows() -> impl Strategy<Value = Vec<RegistryEntry>> {
        let platform = prop::option::of(prop::sample::select(vec![
            "windows-x86_64",
            "linux-x86_64",
            "macos-aarch64",
            "universal",
        ]));
        prop::collection::vec(
            (
                prop::sample::select(vec!["1.7.4", "1.7.9", "1.9.2", "1.10.1"]),
                platform,
                any::<bool>(),
            ),
            0..12,
        )
        .prop_map(|rows| {
            rows.into_iter()
                .map(|(v, p, yanked)| build(v, p, yanked))
                .collect()
        })
    }

    proptest! {
        /// The listing is exactly a projection onto distinct `(name, version)`
        /// pairs, in first-appearance order: nothing invented, nothing
        /// dropped, nothing reordered.
        #[test]
        fn prop_one_entry_per_version_in_first_appearance_order(entries in build_rows()) {
            let host = Some(Platform::WindowsX86_64);
            let out = dedupe_builds_by_version(entries.clone(), host);

            let mut expected: Vec<(String, String)> = Vec::new();
            for e in &entries {
                let key = (e.name.clone(), e.version.clone());
                if !expected.contains(&key) {
                    expected.push(key);
                }
            }
            let got: Vec<(String, String)> = out
                .iter()
                .map(|e| (e.name.clone(), e.version.clone()))
                .collect();
            prop_assert_eq!(got, expected);

            // Every survivor is one of the rows that went in, unmodified.
            for e in &out {
                prop_assert!(entries.contains(e));
            }
        }

        /// Idempotence: feeding the listing back through changes nothing.
        #[test]
        fn prop_dedupe_is_idempotent(entries in build_rows()) {
            let host = Some(Platform::LinuxX86_64);
            let once = dedupe_builds_by_version(entries, host);
            let twice = dedupe_builds_by_version(once.clone(), host);
            prop_assert_eq!(twice, once);
        }
    }
}

#[cfg(test)]
mod set_tests {
    use super::*;

    /// A registry holding a fixed set of entries, used to observe which
    /// registry a lookup actually reached.
    struct FakeRegistry {
        name: String,
        entries: Vec<RegistryEntry>,
    }

    impl FakeRegistry {
        fn new(name: &str, packages: &[(&str, &str)]) -> Self {
            Self {
                name: name.to_string(),
                entries: packages
                    .iter()
                    .map(|(pkg, version)| RegistryEntry {
                        name: (*pkg).to_string(),
                        version: (*version).to_string(),
                        cksum: None,
                        dl: format!("https://{}/{}-{}.tar.gz", name, pkg, version),
                        sig: None,
                        kid: None,
                        houdini_compat: None,
                        platform: None,
                        yanked: false,
                        description: None,
                        author: None,
                        created_at: None,
                    })
                    .collect(),
            }
        }
    }

    #[async_trait]
    impl Registry for FakeRegistry {
        async fn search(&self, _query: &str) -> Result<SearchResults, RegistryError> {
            Ok(SearchResults {
                packages: self.entries.clone(),
                total: self.entries.len(),
            })
        }

        async fn get_versions(&self, name: &str) -> Result<Vec<RegistryEntry>, RegistryError> {
            let found: Vec<_> = self
                .entries
                .iter()
                .filter(|e| e.name == name)
                .cloned()
                .collect();
            if found.is_empty() {
                return Err(RegistryError::PackageNotFound {
                    name: name.to_string(),
                });
            }
            Ok(found)
        }

        async fn get_version(
            &self,
            name: &str,
            version: &str,
        ) -> Result<RegistryEntry, RegistryError> {
            self.entries
                .iter()
                .find(|e| e.name == name && e.version == version)
                .cloned()
                .ok_or_else(|| RegistryError::VersionNotFound {
                    name: name.to_string(),
                    version: version.to_string(),
                })
        }

        async fn refresh(&self) -> Result<(), RegistryError> {
            Ok(())
        }

        fn name(&self) -> &str {
            &self.name
        }
    }

    /// Two registries both carrying the same package at the same version;
    /// only the download URL reveals which one answered.
    fn ambiguous_set() -> RegistrySet {
        let mut set = RegistrySet::new();
        set.add(Box::new(FakeRegistry::new(
            "first",
            &[("acme/tools", "1.0.0")],
        )));
        set.add(Box::new(FakeRegistry::new(
            "second",
            &[("acme/tools", "1.0.0")],
        )));
        set
    }

    #[tokio::test]
    async fn unpinned_lookup_takes_the_first_registry() {
        let entry = ambiguous_set()
            .get_version_in("acme/tools", "1.0.0", None)
            .await
            .unwrap();
        assert!(entry.dl.contains("//first/"), "got {}", entry.dl);
    }

    /// The regression this guards: a pinned registry must win over
    /// configured order, otherwise the manifest asks for one source and
    /// silently gets another.
    #[tokio::test]
    async fn pinned_lookup_uses_the_named_registry_not_the_first() {
        let entry = ambiguous_set()
            .get_version_in("acme/tools", "1.0.0", Some("second"))
            .await
            .unwrap();
        assert!(entry.dl.contains("//second/"), "got {}", entry.dl);
    }

    /// A pin must not silently fall back to the rest of the set.
    #[tokio::test]
    async fn pinned_lookup_does_not_fall_back_to_other_registries() {
        let mut set = RegistrySet::new();
        set.add(Box::new(FakeRegistry::new(
            "first",
            &[("acme/tools", "1.0.0")],
        )));
        set.add(Box::new(FakeRegistry::new(
            "second",
            &[("other/pkg", "1.0.0")],
        )));

        let err = set
            .get_version_in("acme/tools", "1.0.0", Some("second"))
            .await
            .unwrap_err();
        assert!(
            matches!(err, RegistryError::VersionNotFound { .. }),
            "expected VersionNotFound, got {err:?}"
        );
    }

    #[tokio::test]
    async fn pinning_an_unconfigured_registry_is_an_error() {
        let err = ambiguous_set()
            .get_version_in("acme/tools", "1.0.0", Some("typo"))
            .await
            .unwrap_err();
        assert!(
            matches!(&err, RegistryError::UnknownRegistry { name } if name == "typo"),
            "expected UnknownRegistry, got {err:?}"
        );
    }

    #[tokio::test]
    async fn resolve_in_honours_the_pin_for_ranges() {
        let mut set = RegistrySet::new();
        set.add(Box::new(FakeRegistry::new(
            "first",
            &[("acme/tools", "1.0.0")],
        )));
        set.add(Box::new(FakeRegistry::new(
            "second",
            &[("acme/tools", "1.5.0")],
        )));

        // Unpinned: first-match-wins stops at "first" and never sees 1.5.0.
        let unpinned = set.resolve_in("acme/tools", "^1", None).await.unwrap();
        assert_eq!(unpinned.version, "1.0.0");

        let pinned = set
            .resolve_in("acme/tools", "^1", Some("second"))
            .await
            .unwrap();
        assert_eq!(pinned.version, "1.5.0");
    }
}
