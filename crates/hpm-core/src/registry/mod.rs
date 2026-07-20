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
    /// auth (e.g. a desktop client passing a bearer token for visibility-gated
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
                Ok(results) => all_packages.extend(results.packages),
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
                Ok(versions) if !versions.is_empty() => return Ok(versions),
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
