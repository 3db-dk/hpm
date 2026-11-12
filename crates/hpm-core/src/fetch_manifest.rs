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
use thiserror::Error;
use tracing::{debug, info};

impl From<StorageError> for FetchManifestError {
    fn from(e: StorageError) -> Self {
        Self::Storage(Box::new(e))
    }
}

/// Errors that can occur while fetching a package manifest.
#[derive(Debug, Error)]
pub enum FetchManifestError {
    /// Boxed: `StorageError` is large enough on Windows (its paths are
    /// wider) to trip clippy's `result_large_err` on every function here.
    #[error("storage error: {0}")]
    Storage(Box<StorageError>),

    #[error("registry error: {0}")]
    Registry(#[from] RegistryError),

    #[error("fetch error: {0}")]
    Fetch(#[from] FetchError),

    #[error(transparent)]
    Manifest(#[from] ManifestLoadError),
}

/// Fetch the [`PackageManifest`] for `(name, version)`.
///
/// Pass an empty string or `"latest"` as `version` to resolve the highest
/// non-yanked release in the registries.
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
    // Empty / "latest" resolves to the highest non-yanked release; an exact
    // version is looked up only on a store miss.
    let latest = if version.is_empty() || version == "latest" {
        Some(registry_set.resolve(name, "*").await?)
    } else {
        None
    };
    let resolved_version = latest
        .as_ref()
        .map_or(version, |entry| entry.version.as_str())
        .to_string();

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

    let entry = match latest {
        Some(entry) => entry,
        None => registry_set.get_version(name, &resolved_version).await?,
    };
    let source = PackageSource::url(entry.dl.clone(), resolved_version.clone())
        .and_then(|s| s.with_registry_checksum(entry.cksum.as_deref()))
        .map_err(|e| FetchManifestError::Fetch(crate::archive_fetcher::FetchError::from(e)))?;

    let fetcher = ArchiveFetcher::for_storage(&storage.config)?;
    let (installed, _checksum) = fetcher
        .fetch_into_store::<FetchManifestError>(storage, &source, name)
        .await?;

    info!(
        "fetch_manifest: installed {}@{} into CAS",
        name, resolved_version
    );
    Ok(installed.manifest)
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

    /// An installed package is found by its scoped name, without asking the
    /// registry: the store keys by slug, and `fetch_manifest` used to look
    /// for `packages/creator/slug@version`, miss, and download it again.
    #[tokio::test]
    async fn scoped_name_hits_a_package_installed_by_slug() {
        let temp = TempDir::new().unwrap();
        let storage = make_storage(&temp);
        let source = temp.path().join("src");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(
            source.join("hpm.toml"),
            "[package]\npath = \"acme/tools\"\nname = \"Tools\"\nversion = \"1.0.0\"\n",
        )
        .unwrap();
        storage.install_into_cas(&source).await.unwrap();

        let manifest = fetch_manifest("acme/tools", "1.0.0", &RegistrySet::new(), &storage)
            .await
            .expect("an installed package must not need the registry");
        assert_eq!(manifest.package.path.as_str(), "acme/tools");
    }

    struct FixedRegistry(Vec<crate::registry::RegistryEntry>);

    #[async_trait::async_trait]
    impl crate::registry::Registry for FixedRegistry {
        async fn search(
            &self,
            _query: &str,
        ) -> Result<crate::registry::SearchResults, RegistryError> {
            unreachable!("not searched")
        }
        async fn get_versions(
            &self,
            _name: &str,
        ) -> Result<Vec<crate::registry::RegistryEntry>, RegistryError> {
            Ok(self.0.clone())
        }
        async fn get_version(
            &self,
            name: &str,
            version: &str,
        ) -> Result<crate::registry::RegistryEntry, RegistryError> {
            self.0
                .iter()
                .find(|e| e.version == version)
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
            "fixed"
        }
    }

    fn entry(version: &str, yanked: bool) -> crate::registry::RegistryEntry {
        serde_json::from_value(serde_json::json!({
            "name": "acme/tools",
            "vers": version,
            // Unreachable: a test that downloads fails.
            "dl": "http://127.0.0.1:9/tools.zip",
            "yanked": yanked,
        }))
        .unwrap()
    }

    /// "latest" is the highest non-yanked release. It used to take the
    /// highest version outright, so a yanked 2.0.0 shadowed the installed
    /// 1.0.0 and forced a download of the yanked release.
    #[tokio::test]
    async fn latest_skips_yanked_releases() {
        let temp = TempDir::new().unwrap();
        let storage = make_storage(&temp);
        write_fake_package(
            &storage,
            "acme/tools",
            "1.0.0",
            "[package]\npath = \"acme/tools\"\nname = \"Tools\"\nversion = \"1.0.0\"\n",
        );
        let mut registries = RegistrySet::new();
        registries.add(Box::new(FixedRegistry(vec![
            entry("1.0.0", false),
            entry("2.0.0", true),
        ])));

        for latest in ["latest", ""] {
            let manifest = fetch_manifest("acme/tools", latest, &registries, &storage)
                .await
                .expect("latest resolves to the installed 1.0.0");
            assert_eq!(manifest.package.version, "1.0.0");
        }
    }
}
