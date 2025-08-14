//! Package storage backend for the registry

use crate::types::{Package, PackageVersion, RegistryError};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[async_trait]
pub trait PackageStorage: Send + Sync {
    async fn store_package(
        &self,
        package: PackageVersion,
        data: Vec<u8>,
    ) -> Result<String, RegistryError>;

    async fn get_package_data(&self, name: &str, version: &str) -> Result<Vec<u8>, RegistryError>;

    async fn get_package_info(
        &self,
        name: &str,
        version: Option<&str>,
    ) -> Result<PackageVersion, RegistryError>;

    async fn search_packages(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<(Vec<PackageVersion>, usize), RegistryError>;

    async fn list_versions(&self, name: &str) -> Result<Vec<String>, RegistryError>;

    async fn package_exists(&self, name: &str, version: &str) -> Result<bool, RegistryError>;
}

/// In-memory storage implementation for development and testing
pub struct MemoryStorage {
    packages: Arc<RwLock<HashMap<String, Package>>>,
    package_data: Arc<RwLock<HashMap<String, Vec<u8>>>>,
}

impl MemoryStorage {
    pub fn new() -> Self {
        Self {
            packages: Arc::new(RwLock::new(HashMap::new())),
            package_data: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    fn make_data_key(name: &str, version: &str) -> String {
        format!("{}@{}", name, version)
    }
}

impl Default for MemoryStorage {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl PackageStorage for MemoryStorage {
    async fn store_package(
        &self,
        package_version: PackageVersion,
        data: Vec<u8>,
    ) -> Result<String, RegistryError> {
        let name = package_version.metadata.name.clone();
        let version = package_version.version.clone();

        // Check if package version already exists
        if self.package_exists(&name, &version).await? {
            return Err(RegistryError::PackageAlreadyExists {
                name: name.clone(),
                version: version.clone(),
            });
        }

        let data_key = Self::make_data_key(&name, &version);

        // Store package data
        {
            let mut package_data = self.package_data.write().await;
            package_data.insert(data_key.clone(), data);
        }

        // Store package metadata
        {
            let mut packages = self.packages.write().await;
            let package = packages.entry(name.clone()).or_insert_with(|| Package {
                name: name.clone(),
                versions: HashMap::new(),
            });
            package.versions.insert(version.clone(), package_version);
        }

        Ok(format!("{}/{}@{}", "packages", name, version))
    }

    async fn get_package_data(&self, name: &str, version: &str) -> Result<Vec<u8>, RegistryError> {
        let data_key = Self::make_data_key(name, version);
        let package_data = self.package_data.read().await;

        package_data
            .get(&data_key)
            .cloned()
            .ok_or_else(|| RegistryError::VersionNotFound {
                name: name.to_string(),
                version: version.to_string(),
            })
    }

    async fn get_package_info(
        &self,
        name: &str,
        version: Option<&str>,
    ) -> Result<PackageVersion, RegistryError> {
        let packages = self.packages.read().await;
        let package = packages
            .get(name)
            .ok_or_else(|| RegistryError::PackageNotFound {
                name: name.to_string(),
            })?;

        let version = if let Some(v) = version {
            v
        } else {
            // Find the latest version (simplified - should use proper semver comparison)
            package
                .versions
                .keys()
                .max()
                .ok_or_else(|| RegistryError::PackageNotFound {
                    name: name.to_string(),
                })?
        };

        package
            .versions
            .get(version)
            .cloned()
            .ok_or_else(|| RegistryError::VersionNotFound {
                name: name.to_string(),
                version: version.to_string(),
            })
    }

    async fn search_packages(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<(Vec<PackageVersion>, usize), RegistryError> {
        let packages = self.packages.read().await;
        let query_lower = query.to_lowercase();

        let mut matching_packages = Vec::new();

        for package in packages.values() {
            // Simple search by name, description, and keywords
            for version in package.versions.values() {
                let matches = version.metadata.name.to_lowercase().contains(&query_lower)
                    || version
                        .metadata
                        .description
                        .to_lowercase()
                        .contains(&query_lower)
                    || version
                        .metadata
                        .keywords
                        .iter()
                        .any(|k| k.to_lowercase().contains(&query_lower));

                if matches {
                    matching_packages.push(version.clone());
                }
            }
        }

        let total_count = matching_packages.len();

        // Apply pagination
        let start = offset.min(matching_packages.len());
        let end = (start + limit).min(matching_packages.len());
        matching_packages = matching_packages[start..end].to_vec();

        Ok((matching_packages, total_count))
    }

    async fn list_versions(&self, name: &str) -> Result<Vec<String>, RegistryError> {
        let packages = self.packages.read().await;
        let package = packages
            .get(name)
            .ok_or_else(|| RegistryError::PackageNotFound {
                name: name.to_string(),
            })?;

        let mut versions: Vec<String> = package.versions.keys().cloned().collect();
        versions.sort(); // In production, use proper semver sorting
        Ok(versions)
    }

    async fn package_exists(&self, name: &str, version: &str) -> Result<bool, RegistryError> {
        let packages = self.packages.read().await;

        Ok(packages
            .get(name)
            .map(|pkg| pkg.versions.contains_key(version))
            .unwrap_or(false))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{HoudiniRequirements, PackageMetadata};
    use chrono::Utc;

    fn create_test_package_version(name: &str, version: &str) -> PackageVersion {
        PackageVersion {
            version: version.to_string(),
            metadata: PackageMetadata {
                name: name.to_string(),
                version: version.to_string(),
                description: "Test package".to_string(),
                authors: vec!["Test Author".to_string()],
                license: Some("MIT".to_string()),
                dependencies: std::collections::HashMap::new(),
                houdini: HoudiniRequirements {
                    min_version: Some("19.0".to_string()),
                    max_version: Some("20.0".to_string()),
                    platforms: vec!["linux".to_string()],
                },
                keywords: vec!["test".to_string()],
                readme: Some("Test readme".to_string()),
                repository: Some("https://github.com/test/test".to_string()),
                homepage: Some("https://test.dev".to_string()),
            },
            published_at: Utc::now(),
            published_by: "test_user".to_string(),
            checksum: "abc123".to_string(),
            size_bytes: 1024,
        }
    }

    #[tokio::test]
    async fn test_store_and_retrieve_package() {
        let storage = MemoryStorage::new();
        let package = create_test_package_version("test-package", "1.0.0");
        let test_data = b"test package data".to_vec();

        // Store package
        let package_id = storage
            .store_package(package.clone(), test_data.clone())
            .await
            .unwrap();
        assert_eq!(package_id, "packages/test-package@1.0.0");

        // Retrieve package data
        let retrieved_data = storage
            .get_package_data("test-package", "1.0.0")
            .await
            .unwrap();
        assert_eq!(retrieved_data, test_data);

        // Retrieve package info
        let retrieved_info = storage
            .get_package_info("test-package", Some("1.0.0"))
            .await
            .unwrap();
        assert_eq!(retrieved_info.metadata.name, "test-package");
        assert_eq!(retrieved_info.version, "1.0.0");
    }

    #[tokio::test]
    async fn test_search_packages() {
        let storage = MemoryStorage::new();

        let package1 = create_test_package_version("web-utils", "1.0.0");
        let package2 = create_test_package_version("geometry-tools", "2.0.0");

        storage
            .store_package(package1, b"data1".to_vec())
            .await
            .unwrap();
        storage
            .store_package(package2, b"data2".to_vec())
            .await
            .unwrap();

        // Search for "web" should find web-utils
        let (results, total) = storage.search_packages("web", 10, 0).await.unwrap();
        assert_eq!(total, 1);
        assert_eq!(results[0].metadata.name, "web-utils");

        // Search for "tools" should find geometry-tools
        let (results, total) = storage.search_packages("tools", 10, 0).await.unwrap();
        assert_eq!(total, 1);
        assert_eq!(results[0].metadata.name, "geometry-tools");

        // Search for "test" (in description) should find both
        let (_results, total) = storage.search_packages("test", 10, 0).await.unwrap();
        assert_eq!(total, 2);
    }
}
