//! Where hpm keeps packages and downloads on disk. Every path into the store
//! and the download area is named here, so storage, the fetcher, the
//! lockfile, cleanup and the CLI cannot disagree about it.
//!
//! - `<packages_dir>/<slug>@<version>/` — a registry install (the store).
//! - `<packages_dir>/_dev/<slug>@<version>/` — a path dependency's dev
//!   container (see [`super::dev_install`]).
//! - `<cache_dir>/<download key>` — a downloaded archive (`.part` while it
//!   downloads).
//! - `<home_dir>/fetch/<download key>/` — an archive extracted before it is
//!   copied into the store.
//!
//! The store keys by **bare slug**: a scoped `creator/slug` reduces to
//! `slug`, so `acme/tools@1.0.0` and `other/tools@1.0.0` share a directory.
//! That is a known limitation; changing the key is a store migration.

use hpm_config::StorageConfig;
use std::path::{Path, PathBuf};

use super::dev_install::DEV_INSTALL_DIR;

/// The bare slug of a dependency name: `creator/slug` → `slug`; a bare slug
/// is returned as is.
fn slug_of(name: &str) -> &str {
    name.rsplit('/').next().unwrap_or(name)
}

/// `<slug>@<version>`, the name of a store or dev-container directory.
fn coordinate(name: &str, version: &str) -> String {
    format!("{}@{}", slug_of(name), version)
}

/// The `(slug, version)` a store or dev-container directory name
/// (`<slug>@<version>`) names, or `None` when `name` is not one. Splits on
/// the last `@`; both parts must be non-empty.
pub fn parse_coordinate(name: &str) -> Option<(&str, &str)> {
    name.rsplit_once('@')
        .filter(|(slug, version)| !slug.is_empty() && !version.is_empty())
}

/// A package name and version that cannot name a store, cache or staging
/// path.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("'{name}@{version}' cannot name a package directory: {reason}")]
pub struct InvalidCoordinate {
    pub name: String,
    pub version: String,
    pub reason: &'static str,
}

/// Check that `name@version` can name a directory under a store root: the
/// name is a bare slug or `creator/slug` whose segments hold only ASCII
/// letters, digits, `.`, `_` and `-` and do not start with `.`, and the
/// version is a semantic version. Every path function below goes through
/// here, so a name or version from a manifest, lockfile or registry can only
/// ever select one plain directory entry under its root.
fn check_coordinate(name: &str, version: &str) -> Result<(), InvalidCoordinate> {
    let invalid = |reason| InvalidCoordinate {
        name: name.to_string(),
        version: version.to_string(),
        reason,
    };
    let segment_ok = |segment: &str| {
        !segment.is_empty()
            && !segment.starts_with('.')
            && segment
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
    };
    let segments: Vec<&str> = name.split('/').collect();
    if segments.len() > 2 || !segments.iter().all(|s| segment_ok(s)) {
        return Err(invalid("not a package name"));
    }
    semver::Version::parse(version).map_err(|_| invalid("not a semantic version"))?;
    Ok(())
}

/// The store directory for `name@version` under `packages_dir`. `name` is a
/// dependency key (`creator/slug`) or a bare slug.
pub fn cas_dir(
    packages_dir: &Path,
    name: &str,
    version: &str,
) -> Result<PathBuf, InvalidCoordinate> {
    check_coordinate(name, version)?;
    Ok(packages_dir.join(coordinate(name, version)))
}

/// The file name a download of `name@version` is kept under, in both the
/// archive cache and fetch staging: the name with `/` replaced by `-`, then
/// `-<version>`.
pub fn download_key(name: &str, version: &str) -> Result<String, InvalidCoordinate> {
    check_coordinate(name, version)?;
    Ok(format!("{}-{}", name.replace('/', "-"), version))
}

/// Whether the cache or staging entry `entry` belongs to the download with
/// key `key`: the archive (`<key>`), an extracted or partial copy a fetch
/// made under its own name (`<key>.<pid>-<seq>`, `<key>.<pid>-<seq>.part`),
/// or the shared `<key>.part` older hpm versions used. A neighbour whose key
/// merely starts with `key` (`tool-1.0.01`) does not match.
pub fn is_download_of(entry: &str, key: &str) -> bool {
    let Some(rest) = entry.strip_prefix(key) else {
        return false;
    };
    if rest.is_empty() || rest == ".part" {
        return true;
    }
    let Some(private) = rest.strip_prefix('.') else {
        return false;
    };
    let private = private.strip_suffix(".part").unwrap_or(private);
    let Some((pid, seq)) = private.split_once('-') else {
        return false;
    };
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    digits(pid) && digits(seq)
}

/// The roots of one store, derived from a [`StorageConfig`].
#[derive(Debug, Clone)]
pub struct StoreLayout {
    packages_dir: PathBuf,
    cache_dir: PathBuf,
    fetch_dir: PathBuf,
    home_dir: PathBuf,
}

impl StoreLayout {
    pub fn new(config: &StorageConfig) -> Self {
        Self {
            packages_dir: config.packages_dir.clone(),
            cache_dir: config.cache_dir.clone(),
            fetch_dir: config.home_dir.join("fetch"),
            home_dir: config.home_dir.clone(),
        }
    }

    pub fn packages_dir(&self) -> &Path {
        &self.packages_dir
    }

    /// Where downloaded archives are cached (`[storage] cache_dir`).
    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }

    /// Where downloads are extracted before they are copied into the store.
    pub fn fetch_dir(&self) -> &Path {
        &self.fetch_dir
    }

    /// The store directory for `name@version`; see [`cas_dir`].
    pub fn cas_dir(&self, name: &str, version: &str) -> Result<PathBuf, InvalidCoordinate> {
        cas_dir(&self.packages_dir, name, version)
    }

    /// `<packages_dir>/_dev`, the root of every dev container.
    pub fn dev_root(&self) -> PathBuf {
        self.packages_dir.join(DEV_INSTALL_DIR)
    }

    /// The dev container for `name@version`.
    pub fn dev_container(&self, name: &str, version: &str) -> Result<PathBuf, InvalidCoordinate> {
        check_coordinate(name, version)?;
        Ok(self.dev_root().join(coordinate(name, version)))
    }

    /// The fetch-staging directory a download of `name@version` is
    /// extracted into.
    pub fn staging_dir(&self, name: &str, version: &str) -> Result<PathBuf, InvalidCoordinate> {
        Ok(self.fetch_dir.join(download_key(name, version)?))
    }

    /// Download locations earlier hpm versions used and this one no longer
    /// writes, still swept so what they left behind is reclaimed: staging at
    /// `<packages_dir>/../fetch` (where `fetch_manifest` extracted) and the
    /// archive cache at `<home_dir>/cache` (where every fetcher cached,
    /// whatever `cache_dir` said). Each is listed only when it differs from
    /// the current location.
    pub(crate) fn legacy_fetch_dir(&self) -> Option<PathBuf> {
        let old = self.packages_dir.parent()?.join("fetch");
        (old != self.fetch_dir).then_some(old)
    }

    pub(crate) fn legacy_cache_dir(&self) -> Option<PathBuf> {
        let old = self.home_dir.join("cache");
        (old != self.cache_dir).then_some(old)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout(home: &str, packages: &str, cache: &str) -> StoreLayout {
        StoreLayout::new(&StorageConfig {
            home_dir: PathBuf::from(home),
            packages_dir: PathBuf::from(packages),
            cache_dir: PathBuf::from(cache),
            registry_cache_dir: PathBuf::from(home).join("registry"),
        })
    }

    #[test]
    fn scoped_and_bare_names_reach_the_same_store_directory() {
        let l = layout("/h", "/h/packages", "/h/cache");
        assert_eq!(
            l.cas_dir("acme/tools", "1.0.0").unwrap(),
            PathBuf::from("/h/packages/tools@1.0.0")
        );
        assert_eq!(
            l.cas_dir("tools", "1.0.0").unwrap(),
            l.cas_dir("acme/tools", "1.0.0").unwrap()
        );
        assert_eq!(
            l.dev_container("acme/tools", "1.0.0").unwrap(),
            PathBuf::from("/h/packages/_dev/tools@1.0.0")
        );
    }

    mod coordinates {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            /// A directory name made from a slug and version parses back to
            /// them, whether the name was given scoped or bare.
            #[test]
            fn coordinate_round_trips(
                creator in prop::option::of("[a-z][a-z0-9-]{0,6}"),
                slug in "[a-z][a-z0-9-]{0,8}",
                version in "[0-9]{1,2}\\.[0-9]{1,2}\\.[0-9]{1,2}(-[a-z0-9.]{1,6})?",
            ) {
                let name = match &creator {
                    Some(c) => format!("{c}/{slug}"),
                    None => slug.clone(),
                };
                let dir = coordinate(&name, &version);
                prop_assert_eq!(parse_coordinate(&dir), Some((slug.as_str(), version.as_str())));
            }
        }

        #[test]
        fn names_without_both_parts_are_not_coordinates() {
            for bad in ["tool", "@1.0.0", "tool@", "_dev", ".stage-1-0"] {
                assert_eq!(parse_coordinate(bad), None, "{bad}");
            }
        }
    }

    #[test]
    fn downloads_are_keyed_by_the_full_name() {
        let l = layout("/h", "/h/packages", "/h/cache");
        assert_eq!(
            download_key("acme/tools", "1.0.0").unwrap(),
            "acme-tools-1.0.0"
        );
        assert_eq!(
            l.staging_dir("acme/tools", "1.0.0").unwrap(),
            PathBuf::from("/h/fetch/acme-tools-1.0.0")
        );
    }

    /// Real names and versions keep naming the same store and download
    /// paths: prerelease and build metadata, digits, underscores, dots and
    /// capitals inside a name.
    #[test]
    fn legitimate_names_and_versions_keep_their_paths() {
        let l = layout("/h", "/h/packages", "/h/cache");
        for (name, version, store, key) in [
            (
                "acme/tools",
                "1.0.0-beta.1+b5",
                "tools@1.0.0-beta.1+b5",
                "acme-tools-1.0.0-beta.1+b5",
            ),
            (
                "studio/fx2",
                "10.20.30",
                "fx2@10.20.30",
                "studio-fx2-10.20.30",
            ),
            (
                "Studio/My_Tool.v2",
                "0.1.0",
                "My_Tool.v2@0.1.0",
                "Studio-My_Tool.v2-0.1.0",
            ),
            (
                "tools",
                "2.0.0-rc.1",
                "tools@2.0.0-rc.1",
                "tools-2.0.0-rc.1",
            ),
        ] {
            assert_eq!(
                l.cas_dir(name, version).unwrap(),
                PathBuf::from("/h/packages").join(store)
            );
            assert_eq!(download_key(name, version).unwrap(), key);
            assert_eq!(
                l.staging_dir(name, version).unwrap(),
                PathBuf::from("/h/fetch").join(key)
            );
        }
    }

    /// A name or version that is not one never names a path outside the
    /// store, cache or staging root.
    #[test]
    fn hostile_names_and_versions_never_leave_their_root() {
        let l = layout("/h", "/h/packages", "/h/cache");
        for (name, version) in [
            ("acme/tools", "1/../../../x"),
            ("acme/tools", "1\\..\\..\\x"),
            ("acme/tools", "C:x"),
            ("acme/..", "1.0.0"),
            ("../..", "1.0.0"),
            ("acme/tools", ""),
            ("acme/tools", "latest"),
            ("a/b/c", "1.0.0"),
            ("acme/.hidden", "1.0.0"),
            ("acme/to:ols", "1.0.0"),
        ] {
            assert!(
                l.cas_dir(name, version).is_err(),
                "cas_dir {name}@{version}"
            );
            assert!(
                l.staging_dir(name, version).is_err(),
                "staging {name}@{version}"
            );
            assert!(
                l.dev_container(name, version).is_err(),
                "dev {name}@{version}"
            );
            assert!(download_key(name, version).is_err(), "key {name}@{version}");
        }
    }

    #[test]
    fn downloads_are_matched_by_key_not_by_prefix() {
        let key = "studio-tool-1.0.0";
        for yes in [
            key,
            "studio-tool-1.0.0.part",
            "studio-tool-1.0.0.42-7",
            "studio-tool-1.0.0.42-7.part",
        ] {
            assert!(is_download_of(yes, key), "{yes}");
        }
        for no in [
            "studio-tool-1.0.01",
            "studio-tool-1.0.0-beta",
            "studio-tool-1.0.0.x-1",
            "studio-tool-1.0.0.1-",
            "studio-tool-2.0.0",
            "studio-tool-1.0.0.42-7.tmp",
        ] {
            assert!(!is_download_of(no, key), "{no}");
        }
    }

    #[test]
    fn legacy_locations_are_listed_only_when_they_differ() {
        let default = layout("/h", "/h/packages", "/h/cache");
        assert_eq!(default.legacy_fetch_dir(), None);
        assert_eq!(default.legacy_cache_dir(), None);

        let moved = layout("/h", "/big/disk/packages", "/fast/cache");
        assert_eq!(moved.fetch_dir(), Path::new("/h/fetch"));
        assert_eq!(
            moved.legacy_fetch_dir(),
            Some(PathBuf::from("/big/disk/fetch"))
        );
        assert_eq!(moved.cache_dir(), Path::new("/fast/cache"));
        assert_eq!(moved.legacy_cache_dir(), Some(PathBuf::from("/h/cache")));
    }

    mod coordinate_properties {
        use super::*;
        use proptest::prelude::*;
        use std::path::{Component, Path};

        /// Names and versions from the pieces that could escape a root:
        /// dots, separators of both kinds, drive and stream syntax, `@`,
        /// NUL, and ordinary name characters.
        fn hostile() -> impl Strategy<Value = String> {
            let piece = prop::sample::select(vec![
                ".", "..", "/", "\\", ":", "@", "\0", "C:", "a", "Z", "1", "0", "-", "_", "+", " ",
            ]);
            prop::collection::vec(piece, 0..7).prop_map(|p| p.concat())
        }

        /// `path` is `root` plus exactly one plain name.
        fn one_name_under(path: &Path, root: &Path) -> bool {
            path.strip_prefix(root).is_ok_and(|rest| {
                let parts: Vec<_> = rest.components().collect();
                matches!(parts.as_slice(), [Component::Normal(_)])
            })
        }

        proptest! {
            #![proptest_config(ProptestConfig::with_cases(1024))]

            /// Whatever a manifest, lockfile or registry names, an accepted
            /// coordinate is one plain name under the store, dev, cache or
            /// staging root, never a path out of it.
            #[test]
            fn accepted_coordinates_are_one_name_under_their_root(
                creator in hostile(),
                slug in hostile(),
                scoped in any::<bool>(),
                version in prop_oneof![hostile(), Just("1.0.0".to_string())],
            ) {
                let name = if scoped { format!("{creator}/{slug}") } else { slug };
                let l = layout("/h", "/h/packages", "/h/cache");
                if let Ok(path) = l.cas_dir(&name, &version) {
                    prop_assert!(one_name_under(&path, Path::new("/h/packages")), "{:?}", path);
                }
                if let Ok(path) = l.dev_container(&name, &version) {
                    prop_assert!(one_name_under(&path, &l.dev_root()), "{:?}", path);
                }
                if let Ok(path) = l.staging_dir(&name, &version) {
                    prop_assert!(one_name_under(&path, Path::new("/h/fetch")), "{:?}", path);
                }
                if let Ok(key) = download_key(&name, &version) {
                    prop_assert!(one_name_under(&Path::new("/h/cache").join(&key), Path::new("/h/cache")), "{:?}", key);
                }
            }

            /// Real coordinates always resolve, so a check that refuses
            /// everything cannot pass the property above alone.
            #[test]
            fn real_coordinates_resolve(
                creator in "[a-z0-9][a-z0-9-]{0,8}",
                slug in "[A-Za-z0-9][A-Za-z0-9._-]{0,8}",
                scoped in any::<bool>(),
                version in (0u32..100, 0u32..100, 0u32..100, prop::option::of("[a-z]{1,5}\\.(0|[1-9][0-9]?)"))
                    .prop_map(|(a, b, c, pre)| match pre {
                        Some(pre) => format!("{a}.{b}.{c}-{pre}"),
                        None => format!("{a}.{b}.{c}"),
                    }),
            ) {
                let name = if scoped { format!("{creator}/{slug}") } else { slug };
                let l = layout("/h", "/h/packages", "/h/cache");
                prop_assert!(l.cas_dir(&name, &version).is_ok(), "{} {}", name, version);
                prop_assert!(l.dev_container(&name, &version).is_ok());
                prop_assert!(l.staging_dir(&name, &version).is_ok());
                prop_assert!(download_key(&name, &version).is_ok());
            }
        }
    }
}
