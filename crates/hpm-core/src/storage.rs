use crate::dependency::{DependencyResolver, PackageId};
use crate::discovery::ProjectDiscovery;
use hpm_config::{ProjectsConfig, StorageConfig};
use hpm_package::PackageManifest;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;
use tracing::{debug, error, info, warn};

pub mod types;
pub use types::{InstalledPackage, PackageSpec};

#[derive(Debug, Clone)]
pub struct StorageManager {
    pub config: StorageConfig,
}

impl StorageManager {
    pub fn new(config: StorageConfig) -> Result<Self, StorageError> {
        let manager = Self { config };
        manager.ensure_directories()?;
        Ok(manager)
    }

    fn ensure_directories(&self) -> Result<(), StorageError> {
        self.config
            .ensure_directories()
            .map_err(|e| StorageError::DirectoryCreation(e.to_string()))?;
        info!("Ensured storage directories exist");
        Ok(())
    }

    pub fn package_exists(&self, name: &str, version: &str) -> bool {
        let package_dir = self.config.package_dir(name, version);
        package_dir.exists() && package_dir.join("hpm.toml").exists()
    }

    pub fn get_package_path(&self, name: &str, version: &str) -> PathBuf {
        self.config.package_dir(name, version)
    }

    pub fn list_installed(&self) -> Result<Vec<InstalledPackage>, StorageError> {
        let mut packages = Vec::new();

        if !self.config.packages_dir.exists() {
            return Ok(packages);
        }

        let entries = std::fs::read_dir(&self.config.packages_dir)
            .map_err(|e| StorageError::DirectoryRead(e.to_string()))?;

        for entry in entries.flatten() {
            if let Some(installed_package) = self.parse_installed_package(entry.path())? {
                packages.push(installed_package);
            }
        }

        debug!("Found {} installed packages", packages.len());
        Ok(packages)
    }

    fn parse_installed_package(
        &self,
        package_dir: PathBuf,
    ) -> Result<Option<InstalledPackage>, StorageError> {
        if !package_dir.is_dir() {
            return Ok(None);
        }

        let manifest_path = package_dir.join("hpm.toml");
        if !manifest_path.exists() {
            return Ok(None);
        }

        let manifest_content = std::fs::read_to_string(&manifest_path)
            .map_err(|e| StorageError::ManifestRead(e.to_string()))?;

        let manifest: PackageManifest = toml::from_str(&manifest_content)
            .map_err(|e| StorageError::ManifestParse(e.to_string()))?;

        let metadata = std::fs::metadata(&package_dir)
            .map_err(|e| StorageError::MetadataRead(e.to_string()))?;

        let installed_at = metadata.created().unwrap_or_else(|_| SystemTime::now());

        Ok(Some(InstalledPackage {
            name: manifest.package.name.clone(),
            version: manifest.package.version.clone(),
            manifest,
            install_path: package_dir,
            installed_at,
        }))
    }

    pub async fn install_package(
        &self,
        spec: &PackageSpec,
    ) -> Result<InstalledPackage, StorageError> {
        info!("Installing package: {} {}", spec.name, spec.version_req);

        // TODO: This is a placeholder implementation
        // Real implementation would:
        // 1. Resolve version from registry
        // 2. Download package archive
        // 3. Extract to storage directory
        // 4. Validate package structure

        Err(StorageError::NotImplemented(
            "Package installation not yet implemented".to_string(),
        ))
    }

    pub async fn remove_package(&self, name: &str, version: &str) -> Result<(), StorageError> {
        let package_dir = self.config.package_dir(name, version);

        if !package_dir.exists() {
            return Err(StorageError::PackageNotFound(format!(
                "{}@{}",
                name, version
            )));
        }

        info!("Removing package: {}@{}", name, version);
        std::fs::remove_dir_all(&package_dir)
            .map_err(|e| StorageError::DirectoryRemoval(e.to_string()))?;

        Ok(())
    }

    pub async fn cleanup_unused(
        &self,
        projects_config: &ProjectsConfig,
    ) -> Result<Vec<String>, StorageError> {
        info!("Starting project-aware package cleanup");

        // 1. Get all installed packages
        let all_installed = self
            .list_installed()
            .map_err(|e| StorageError::DirectoryRead(e.to_string()))?;

        if all_installed.is_empty() {
            info!("No packages installed - cleanup not needed");
            return Ok(vec![]);
        }

        info!(
            "Found {} installed packages to analyze",
            all_installed.len()
        );

        // 2. Discover projects using project configuration
        let project_discovery = ProjectDiscovery::new(projects_config.clone());
        let projects = project_discovery
            .find_projects()
            .map_err(|e| StorageError::ProjectDiscovery(e.to_string()))?;

        if projects.is_empty() {
            warn!(
                "No HPM-managed projects found - skipping cleanup to prevent removing all packages"
            );
            return Ok(vec![]);
        }

        info!(
            "Found {} HPM-managed projects for cleanup analysis",
            projects.len()
        );

        // 3. Build dependency graph from discovered projects
        let resolver = DependencyResolver::new(Arc::new(self.clone()));
        let dependency_graph = resolver
            .build_dependency_graph(&projects)
            .await
            .map_err(|e| StorageError::DependencyResolution(e.to_string()))?;

        // 4. Collect root packages (directly required by projects)
        let root_packages: Vec<PackageId> = dependency_graph
            .nodes()
            .values()
            .filter(|node| node.is_root)
            .map(|node| node.id.clone())
            .collect();

        info!(
            "Found {} root packages required by active projects",
            root_packages.len()
        );

        // 5. Mark all packages reachable from roots
        let needed_packages = dependency_graph.mark_reachable_from_roots(&root_packages);
        info!(
            "Marked {} packages as needed (including transitive dependencies)",
            needed_packages.len()
        );

        // 6. Find orphaned packages by comparing all installed packages to needed packages
        let all_package_ids: HashSet<PackageId> =
            all_installed.iter().map(PackageId::from).collect();

        let orphaned_packages: Vec<PackageId> = all_package_ids
            .difference(&needed_packages)
            .cloned()
            .collect();

        if orphaned_packages.is_empty() {
            info!("No orphaned packages found - cleanup not needed");
            return Ok(vec![]);
        }

        info!(
            "Found {} orphaned packages to remove",
            orphaned_packages.len()
        );

        // 7. Remove orphaned packages
        let mut removed_packages = Vec::new();
        for package_id in orphaned_packages {
            match self
                .remove_package(&package_id.name, &package_id.version)
                .await
            {
                Ok(()) => {
                    removed_packages.push(package_id.identifier());
                    info!("Removed orphaned package: {}", package_id.identifier());
                }
                Err(e) => {
                    warn!(
                        "Failed to remove package {}: {}",
                        package_id.identifier(),
                        e
                    );
                }
            }
        }

        info!(
            "Cleanup completed: removed {} orphaned packages",
            removed_packages.len()
        );
        Ok(removed_packages)
    }

    pub async fn cleanup_unused_dry_run(
        &self,
        projects_config: &ProjectsConfig,
    ) -> Result<Vec<String>, StorageError> {
        info!("Starting dry-run project-aware package cleanup");

        // 1. Get all installed packages
        let all_installed = self
            .list_installed()
            .map_err(|e| StorageError::DirectoryRead(e.to_string()))?;

        if all_installed.is_empty() {
            info!("No packages installed - cleanup not needed");
            return Ok(vec![]);
        }

        info!(
            "Found {} installed packages to analyze",
            all_installed.len()
        );

        let project_discovery = ProjectDiscovery::new(projects_config.clone());
        let projects = project_discovery
            .find_projects()
            .map_err(|e| StorageError::ProjectDiscovery(e.to_string()))?;

        if projects.is_empty() {
            info!("No HPM-managed projects found");
            return Ok(vec![]);
        }

        let resolver = DependencyResolver::new(Arc::new(self.clone()));
        let dependency_graph = resolver
            .build_dependency_graph(&projects)
            .await
            .map_err(|e| StorageError::DependencyResolution(e.to_string()))?;

        let root_packages: Vec<PackageId> = dependency_graph
            .nodes()
            .values()
            .filter(|node| node.is_root)
            .map(|node| node.id.clone())
            .collect();

        let needed_packages = dependency_graph.mark_reachable_from_roots(&root_packages);

        // Find orphaned packages by comparing all installed packages to needed packages
        let all_package_ids: HashSet<PackageId> =
            all_installed.iter().map(PackageId::from).collect();

        let orphaned_packages: Vec<PackageId> = all_package_ids
            .difference(&needed_packages)
            .cloned()
            .collect();

        let orphaned_identifiers: Vec<String> =
            orphaned_packages.iter().map(|id| id.identifier()).collect();

        info!(
            "Dry run: would remove {} orphaned packages",
            orphaned_identifiers.len()
        );
        for identifier in &orphaned_identifiers {
            info!("Would remove: {}", identifier);
        }

        Ok(orphaned_identifiers)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("Directory creation failed: {0}")]
    DirectoryCreation(String),

    #[error("Directory read failed: {0}")]
    DirectoryRead(String),

    #[error("Directory removal failed: {0}")]
    DirectoryRemoval(String),

    #[error("Manifest read failed: {0}")]
    ManifestRead(String),

    #[error("Manifest parse failed: {0}")]
    ManifestParse(String),

    #[error("Metadata read failed: {0}")]
    MetadataRead(String),

    #[error("Package not found: {0}")]
    PackageNotFound(String),

    #[error("Feature not implemented: {0}")]
    NotImplemented(String),

    #[error("Project discovery failed: {0}")]
    ProjectDiscovery(String),

    #[error("Dependency resolution failed: {0}")]
    DependencyResolution(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn storage_manager_creation() {
        let temp_dir = TempDir::new().unwrap();
        let storage_config = StorageConfig {
            home_dir: temp_dir.path().to_path_buf(),
            cache_dir: temp_dir.path().join("cache"),
            packages_dir: temp_dir.path().join("packages"),
            registry_cache_dir: temp_dir.path().join("registry"),
        };

        let _storage_manager = StorageManager::new(storage_config).unwrap();
        assert!(temp_dir.path().join("packages").exists());
        assert!(temp_dir.path().join("cache").exists());
        assert!(temp_dir.path().join("registry").exists());
    }

    #[test]
    fn package_exists_check() {
        let temp_dir = TempDir::new().unwrap();
        let storage_config = StorageConfig {
            home_dir: temp_dir.path().to_path_buf(),
            cache_dir: temp_dir.path().join("cache"),
            packages_dir: temp_dir.path().join("packages"),
            registry_cache_dir: temp_dir.path().join("registry"),
        };

        let storage_manager = StorageManager::new(storage_config).unwrap();

        assert!(!storage_manager.package_exists("test-package", "1.0.0"));

        // Create a fake package directory
        let package_dir = temp_dir.path().join("packages").join("test-package@1.0.0");
        std::fs::create_dir_all(&package_dir).unwrap();
        std::fs::write(
            package_dir.join("hpm.toml"),
            "[package]\nname = \"test-package\"\nversion = \"1.0.0\"",
        )
        .unwrap();

        assert!(storage_manager.package_exists("test-package", "1.0.0"));
    }

    #[test]
    fn list_installed_packages() {
        let temp_dir = TempDir::new().unwrap();
        let storage_config = StorageConfig {
            home_dir: temp_dir.path().to_path_buf(),
            cache_dir: temp_dir.path().join("cache"),
            packages_dir: temp_dir.path().join("packages"),
            registry_cache_dir: temp_dir.path().join("registry"),
        };

        let storage_manager = StorageManager::new(storage_config).unwrap();

        // Initially no packages
        let packages = storage_manager.list_installed().unwrap();
        assert_eq!(packages.len(), 0);

        // Create a fake package
        let package_dir = temp_dir.path().join("packages").join("test-package@1.0.0");
        std::fs::create_dir_all(&package_dir).unwrap();

        let manifest_content = r#"
[package]
name = "test-package"
version = "1.0.0"
description = "A test package"

[houdini]
min_version = "19.5"
"#;
        std::fs::write(package_dir.join("hpm.toml"), manifest_content).unwrap();

        let packages = storage_manager.list_installed().unwrap();
        assert_eq!(packages.len(), 1);
        assert_eq!(packages[0].name, "test-package");
        assert_eq!(packages[0].version, "1.0.0");
    }
}
