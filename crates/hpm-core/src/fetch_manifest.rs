//! Fetch a package manifest by `(name, version)` without project context.
//!
//! Given a scoped name (e.g. `creator/slug`) and a version, return the parsed
//! [`PackageManifest`]. HPM picks the cheapest path:
//!
//! - already in CAS → read manifest from disk;
//! - otherwise → resolve via [`RegistrySet`], fetch via [`ArchiveFetcher`],
//!   install into CAS, return the manifest.
//!
//! An empty version or the literal string `"latest"` resolves to the highest
//! semver version available across the configured registries.

use crate::archive_fetcher::{ArchiveFetcher, FetchError};
use crate::package_source::PackageSource;
use crate::registry::{RegistryError, RegistrySet};
use crate::storage::{StorageError, StorageManager};
use hpm_package::{ManifestLoadError, PackageManifest};
use std::path::Path;
use thiserror::Error;
use tracing::{debug, info};

/// Errors that can occur while fetching a package manifest.
#[derive(Debug, Error)]
pub enum FetchManifestError {
    #[error("storage error: {0}")]
    Storage(#[from] StorageError),

    #[error("registry error: {0}")]
    Registry(#[from] RegistryError),

    #[error("fetch error: {0}")]
    Fetch(#[from] FetchError),

    #[error(transparent)]
    Manifest(#[from] ManifestLoadError),

    #[error("no versions available for package '{0}'")]
    NoVersionsAvailable(String),
}

/// Fetch the [`PackageManifest`] for `(name, version)`.
///
/// Pass an empty string or `"latest"` as `version` to resolve the highest
/// semver version known to the registries.
///
/// # Side effects
///
/// On a CAS miss this installs the package into HPM's content-addressable
/// store as a side effect. Callers that want to inspect a package without
/// committing it to CAS should treat that as an implementation detail — a
/// future manifest-only fast path can preserve this signature.
pub async fn fetch_manifest(
    name: &str,
    version: &str,
    registry_set: &RegistrySet,
    storage: &StorageManager,
) -> Result<PackageManifest, FetchManifestError> {
    let resolved_version = resolve_version(name, version, registry_set).await?;

    if storage.package_exists(name, &resolved_version) {
        debug!("fetch_manifest: CAS hit for {}@{}", name, resolved_version);
        let manifest_path = storage
            .get_package_path(name, &resolved_version)
            .join("hpm.toml");
        return Ok(PackageManifest::from_path(&manifest_path)?);
    }

    debug!(
        "fetch_manifest: CAS miss for {}@{}, fetching via registry",
        name, resolved_version
    );

    let entry = registry_set.get_version(name, &resolved_version).await?;
    let source = PackageSource::url(entry.dl, resolved_version.clone())
        .map_err(|e| FetchManifestError::Fetch(crate::archive_fetcher::FetchError::from(e)))?;

    let fetcher = build_fetcher(storage)?;
    let fetch_result = fetcher.fetch(&source, name).await?;
    let installed = storage
        .install_into_cas(&fetch_result.package_path)
        .await?;

    info!(
        "fetch_manifest: installed {}@{} into CAS",
        name, resolved_version
    );
    Ok(installed.manifest)
}

/// Resolve `version` to a concrete semver string. Empty / `"latest"` picks
/// the highest semver returned by [`RegistrySet::get_versions`].
async fn resolve_version(
    name: &str,
    version: &str,
    registry_set: &RegistrySet,
) -> Result<String, FetchManifestError> {
    if !version.is_empty() && version != "latest" {
        return Ok(version.to_string());
    }

    let entries = registry_set.get_versions(name).await?;
    if entries.is_empty() {
        return Err(FetchManifestError::NoVersionsAvailable(name.to_string()));
    }

    let best = entries
        .iter()
        .max_by(|a, b| {
            match (
                semver::Version::parse(&a.version),
                semver::Version::parse(&b.version),
            ) {
                (Ok(va), Ok(vb)) => va.cmp(&vb),
                _ => a.version.cmp(&b.version),
            }
        })
        .expect("entries is non-empty");
    Ok(best.version.clone())
}

/// Build a transient [`ArchiveFetcher`] using the same scratch directory
/// layout as [`crate::project::ProjectManager`] so that downloads and
/// extracted package directories are shared across both code paths.
fn build_fetcher(storage: &StorageManager) -> Result<ArchiveFetcher, FetchError> {
    let parent = storage
        .config
        .packages_dir
        .parent()
        .unwrap_or_else(|| Path::new("."));
    let cache_dir = parent.join("cache");
    let fetch_packages_dir = parent.join("fetch");
    ArchiveFetcher::new(cache_dir, fetch_packages_dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hpm_config::StorageConfig;
    use tempfile::TempDir;

    fn make_storage(temp: &TempDir) -> StorageManager {
        let storage_config = StorageConfig {
            home_dir: temp.path().to_path_buf(),
            cache_dir: temp.path().join("cache"),
            packages_dir: temp.path().join("packages"),
            registry_cache_dir: temp.path().join("registry"),
        };
        StorageManager::new(storage_config).unwrap()
    }

    fn write_fake_package(storage: &StorageManager, name: &str, version: &str, manifest: &str) {
        let dir = storage.get_package_path(name, version);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("hpm.toml"), manifest).unwrap();
    }

    #[tokio::test]
    async fn cas_hit_short_circuits_registry() {
        let temp = TempDir::new().unwrap();
        let storage = make_storage(&temp);
        let manifest = r#"
[package]
path = "creator/example"
name = "Example"
version = "1.2.3"
description = "test"
"#;
        write_fake_package(&storage, "creator/example", "1.2.3", manifest);

        // Empty RegistrySet — function must not consult it on CAS hit.
        let registry_set = RegistrySet::new();
        let manifest = fetch_manifest("creator/example", "1.2.3", &registry_set, &storage)
            .await
            .expect("CAS hit should succeed without registries");
        assert_eq!(manifest.package.version, "1.2.3");
    }

    #[tokio::test]
    async fn cas_miss_with_empty_registry_errors() {
        let temp = TempDir::new().unwrap();
        let storage = make_storage(&temp);
        let registry_set = RegistrySet::new();

        let err = fetch_manifest("creator/missing", "0.1.0", &registry_set, &storage)
            .await
            .expect_err("no registries → must fail");
        assert!(matches!(err, FetchManifestError::Registry(_)));
    }

    #[tokio::test]
    async fn latest_with_empty_registry_errors() {
        let temp = TempDir::new().unwrap();
        let storage = make_storage(&temp);
        let registry_set = RegistrySet::new();

        let err = fetch_manifest("creator/missing", "", &registry_set, &storage)
            .await
            .expect_err("no registries → must fail to resolve latest");
        assert!(matches!(err, FetchManifestError::Registry(_)));
    }
}
