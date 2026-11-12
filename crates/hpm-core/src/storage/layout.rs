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

/// The store directory for `name@version` under `packages_dir`. `name` is a
/// dependency key (`creator/slug`) or a bare slug.
pub fn cas_dir(packages_dir: &Path, name: &str, version: &str) -> PathBuf {
    packages_dir.join(coordinate(name, version))
}

/// The file name a download of `name@version` is kept under, in both the
/// archive cache and fetch staging: the name with `/` replaced by `-`, then
/// `-<version>`.
pub fn download_key(name: &str, version: &str) -> String {
    format!("{}-{}", name.replace('/', "-"), version)
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
    pub fn cas_dir(&self, name: &str, version: &str) -> PathBuf {
        cas_dir(&self.packages_dir, name, version)
    }

    /// `<packages_dir>/_dev`, the root of every dev container.
    pub fn dev_root(&self) -> PathBuf {
        self.packages_dir.join(DEV_INSTALL_DIR)
    }

    /// The dev container for `name@version`.
    pub fn dev_container(&self, name: &str, version: &str) -> PathBuf {
        self.dev_root().join(coordinate(name, version))
    }

    /// The fetch-staging directory a download of `name@version` is
    /// extracted into.
    pub fn staging_dir(&self, name: &str, version: &str) -> PathBuf {
        self.fetch_dir.join(download_key(name, version))
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
            l.cas_dir("acme/tools", "1.0.0"),
            PathBuf::from("/h/packages/tools@1.0.0")
        );
        assert_eq!(
            l.cas_dir("tools", "1.0.0"),
            l.cas_dir("acme/tools", "1.0.0")
        );
        assert_eq!(
            l.dev_container("acme/tools", "1.0.0"),
            PathBuf::from("/h/packages/_dev/tools@1.0.0")
        );
    }

    #[test]
    fn downloads_are_keyed_by_the_full_name() {
        let l = layout("/h", "/h/packages", "/h/cache");
        assert_eq!(download_key("acme/tools", "1.0.0"), "acme-tools-1.0.0");
        assert_eq!(
            l.staging_dir("acme/tools", "1.0.0"),
            PathBuf::from("/h/fetch/acme-tools-1.0.0")
        );
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
}
