use crate::dependency::{DependencyResolver, PackageId};
use crate::discovery::ProjectDiscovery;
use hpm_config::{ProjectsConfig, StorageConfig};
use hpm_package::{ManifestLoadError, PackageManifest};
use hpm_python::cleanup::{CleanupResult, PythonCleanupAnalyzer};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;
use tracing::{debug, info, warn};

pub mod types;
pub use types::{InstalledPackage, PackageSpec, VersionReq};

/// Result of comprehensive cleanup including both packages and Python environments
#[derive(Debug)]
pub struct ComprehensiveCleanupResult {
    pub removed_packages: Vec<String>,
    pub python_cleanup: CleanupResult,
}

impl ComprehensiveCleanupResult {
    /// Total number of items cleaned (packages + venvs)
    pub fn total_items_cleaned(&self) -> usize {
        self.removed_packages.len() + self.python_cleanup.items_cleaned()
    }

    /// Total number of items that would be cleaned (packages + venvs)
    pub fn total_items_that_would_be_cleaned(&self) -> usize {
        self.removed_packages.len() + self.python_cleanup.items_that_would_be_cleaned()
    }

    /// Format the total space freed
    pub fn format_total_space_freed(&self) -> String {
        let package_space_estimate = self.removed_packages.len() as u64 * 10 * 1024 * 1024; // 10MB per package
        let total_space = package_space_estimate + self.python_cleanup.space_freed;
        format_bytes(total_space)
    }

    /// Format the total space that would be freed
    pub fn format_total_space_that_would_be_freed(&self) -> String {
        let package_space_estimate = self.removed_packages.len() as u64 * 10 * 1024 * 1024; // 10MB per package
        let total_space = package_space_estimate + self.python_cleanup.space_that_would_be_freed;
        format_bytes(total_space)
    }
}

/// Format byte size in human-readable format
fn format_bytes(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    const THRESHOLD: u64 = 1024;

    if bytes == 0 {
        return "0 B".to_string();
    }

    let mut size = bytes as f64;
    let mut unit_index = 0;

    while size >= THRESHOLD as f64 && unit_index < UNITS.len() - 1 {
        size /= THRESHOLD as f64;
        unit_index += 1;
    }

    if unit_index == 0 {
        format!("{} {}", bytes, UNITS[unit_index])
    } else {
        format!("{:.1} {}", size, UNITS[unit_index])
    }
}

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
            .map_err(StorageError::DirectoryCreation)?;
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

        self.collect_installed_packages(&self.config.packages_dir, &mut packages)?;

        debug!("Found {} installed packages", packages.len());
        Ok(packages)
    }

    /// Recursively collect installed packages from a directory.
    ///
    /// With scoped package paths (e.g. `creator/slug`), packages live at
    /// `~/.hpm/packages/creator/slug@version/`. Directories without `@` in
    /// their name are treated as scope directories and are walked one level
    /// deeper. Directories with `@` are treated as package directories.
    fn collect_installed_packages(
        &self,
        dir: &std::path::Path,
        packages: &mut Vec<InstalledPackage>,
    ) -> Result<(), StorageError> {
        let entries =
            std::fs::read_dir(dir).map_err(|e| StorageError::DirectoryRead(e.to_string()))?;

        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            let dir_name = match path.file_name().and_then(|n| n.to_str()) {
                Some(name) => name.to_string(),
                None => continue,
            };

            if dir_name.contains('@') {
                // This is a package directory (e.g. `slug@1.0.0` or `fire-fx@2.0.0`)
                if let Some(installed_package) = self.parse_installed_package(path)? {
                    packages.push(installed_package);
                }
            } else {
                // This is a scope directory (e.g. `creator`), walk into it
                self.collect_installed_packages(&entry.path(), packages)?;
            }
        }

        Ok(())
    }

    fn parse_installed_package(
        &self,
        package_dir: PathBuf,
    ) -> Result<Option<InstalledPackage>, StorageError> {
        if !package_dir.is_dir() {
            return Ok(None);
        }

        let manifest_path = package_dir.join("hpm.toml");
        let manifest = match PackageManifest::from_path(&manifest_path) {
            Ok(m) => m,
            // Directory without a manifest is not a package — skip silently
            // to keep `list_installed` resilient to stray scaffolding.
            Err(ManifestLoadError::NotFound { .. }) => return Ok(None),
            Err(e) => return Err(StorageError::Manifest(e)),
        };

        let metadata = std::fs::metadata(&package_dir).map_err(StorageError::MetadataRead)?;

        let installed_at = metadata.created().unwrap_or_else(|_| SystemTime::now());

        Ok(Some(InstalledPackage {
            name: manifest
                .package
                .slug()
                .unwrap_or(&manifest.package.path)
                .to_string(),
            version: manifest.package.version.clone(),
            manifest,
            install_path: package_dir,
            installed_at,
        }))
    }

    /// Install a package from a local directory path.
    /// The directory must contain a valid hpm.toml manifest.
    pub async fn install_from_path(
        &self,
        source_path: &std::path::Path,
    ) -> Result<InstalledPackage, StorageError> {
        info!("Installing package from path: {}", source_path.display());

        // Read and parse the manifest
        let manifest_path = source_path.join("hpm.toml");
        let manifest = PackageManifest::from_path(&manifest_path)?;

        let name = manifest
            .package
            .slug()
            .unwrap_or(&manifest.package.path)
            .to_string();
        let name = &name;
        let version = &manifest.package.version;

        info!(
            "Installing {}@{} from {}",
            name,
            version,
            source_path.display()
        );

        // Create the target directory
        let target_dir = self.config.package_dir(name, version);

        // Check if already installed
        if target_dir.exists() {
            warn!(
                "Package {}@{} already exists, removing old version",
                name, version
            );
            std::fs::remove_dir_all(&target_dir).map_err(|e| {
                // On Windows, a running Houdini process holds open handles to
                // files inside the package dir, so removal fails with
                // ERROR_ACCESS_DENIED (os error 5 → PermissionDenied). Map it
                // to an actionable error instead of leaking a raw OS code.
                if e.kind() == std::io::ErrorKind::PermissionDenied {
                    StorageError::PackageInUse {
                        name: name.to_string(),
                        version: version.to_string(),
                        source: e,
                    }
                } else {
                    StorageError::DirectoryRemoval(e)
                }
            })?;
        }

        // Copy the package directory
        self.copy_directory(source_path, &target_dir)?;

        info!("Successfully installed {}@{}", name, version);

        // Return the installed package info
        let metadata = std::fs::metadata(&target_dir).map_err(StorageError::MetadataRead)?;

        Ok(InstalledPackage {
            name: name.clone(),
            version: version.clone(),
            manifest,
            install_path: target_dir,
            installed_at: metadata
                .created()
                .unwrap_or_else(|_| std::time::SystemTime::now()),
        })
    }

    /// Copy a directory recursively
    fn copy_directory(
        &self,
        source: &std::path::Path,
        target: &std::path::Path,
    ) -> Result<(), StorageError> {
        std::fs::create_dir_all(target).map_err(StorageError::DirectoryCreation)?;

        for entry in walkdir::WalkDir::new(source)
            .min_depth(1)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let relative_path = entry
                .path()
                .strip_prefix(source)
                .map_err(|e| StorageError::DirectoryRead(e.to_string()))?;
            let target_path = target.join(relative_path);

            if entry.file_type().is_dir() {
                std::fs::create_dir_all(&target_path).map_err(StorageError::DirectoryCreation)?;
            } else {
                // Ensure parent directory exists
                if let Some(parent) = target_path.parent() {
                    std::fs::create_dir_all(parent).map_err(StorageError::DirectoryCreation)?;
                }
                std::fs::copy(entry.path(), &target_path).map_err(|e| {
                    StorageError::DirectoryRead(format!("Failed to copy file: {}", e))
                })?;
            }
        }

        Ok(())
    }

    /// Find the best installed version matching a requirement
    pub fn find_installed(&self, name: &str, version_req: &VersionReq) -> Option<InstalledPackage> {
        let installed = self.list_installed().ok()?;
        installed
            .into_iter()
            .filter(|pkg| pkg.name == name && pkg.is_compatible_with(version_req))
            .max_by(|a, b| {
                // Compare versions - prefer higher versions
                match (
                    semver::Version::parse(&a.version),
                    semver::Version::parse(&b.version),
                ) {
                    (Ok(va), Ok(vb)) => va.cmp(&vb),
                    _ => a.version.cmp(&b.version),
                }
            })
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
        std::fs::remove_dir_all(&package_dir).map_err(StorageError::DirectoryRemoval)?;

        Ok(())
    }

    /// Find orphaned packages that are not needed by any active project.
    ///
    /// Returns the list of orphaned package IDs along with all installed package identifiers.
    async fn find_orphaned_packages(
        &self,
        projects_config: &ProjectsConfig,
    ) -> Result<Vec<PackageId>, StorageError> {
        // 1. Get all installed packages
        let all_installed = self.list_installed()?;

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

        Ok(orphaned_packages)
    }

    /// Remove orphaned packages. Returns identifiers of the packages actually removed.
    pub async fn cleanup_unused(
        &self,
        projects_config: &ProjectsConfig,
    ) -> Result<Vec<String>, StorageError> {
        info!("Starting project-aware package cleanup");

        let orphaned_packages = self.find_orphaned_packages(projects_config).await?;

        if orphaned_packages.is_empty() {
            info!("No orphaned packages found - cleanup not needed");
            return Ok(vec![]);
        }

        info!(
            "Found {} orphaned packages to remove",
            orphaned_packages.len()
        );

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

    /// Plan — but don't execute — an orphan cleanup.
    ///
    /// Returns the list of package identifiers that `cleanup_unused` *would*
    /// remove if called. Safe to call repeatedly.
    pub async fn cleanup_unused_dry_run(
        &self,
        projects_config: &ProjectsConfig,
    ) -> Result<Vec<String>, StorageError> {
        let orphaned = self.find_orphaned_packages(projects_config).await?;
        let ids: Vec<String> = orphaned.iter().map(|id| id.identifier()).collect();
        info!("Dry run: would remove {} orphaned packages", ids.len());
        for id in &ids {
            info!("Would remove: {id}");
        }
        Ok(ids)
    }

    /// Comprehensive cleanup: orphaned packages + orphaned Python virtual environments.
    ///
    /// When `dry_run` is true, nothing is removed — the result lists what *would*
    /// have been removed.
    pub async fn cleanup_comprehensive(
        &self,
        projects_config: &ProjectsConfig,
        dry_run: bool,
    ) -> Result<ComprehensiveCleanupResult, StorageError> {
        info!(
            "Starting comprehensive cleanup{} (packages + Python environments)",
            if dry_run { " dry run" } else { "" }
        );

        // 1. Package cleanup.
        let removed_packages = if dry_run {
            self.cleanup_unused_dry_run(projects_config).await?
        } else {
            self.cleanup_unused(projects_config).await?
        };

        // 2. Build the set of packages that remain (or would remain) after package cleanup.
        let all_installed = self
            .list_installed()
            .map_err(|e| StorageError::DirectoryRead(e.to_string()))?;
        let remaining_packages: Vec<String> = all_installed
            .into_iter()
            .filter_map(|p| {
                let id = format!("{}@{}", p.name, p.version);
                (!removed_packages.contains(&id)).then_some(id)
            })
            .collect();

        // 3. Python virtual environment cleanup against the remaining set.
        let python_analyzer = PythonCleanupAnalyzer::new();
        let orphaned_venvs = python_analyzer
            .analyze_orphaned_venvs(&remaining_packages)
            .await
            .map_err(|e| StorageError::PythonCleanup(e.to_string()))?;

        let python_cleanup = python_analyzer
            .cleanup_orphaned_venvs(&orphaned_venvs, dry_run)
            .await
            .map_err(|e| StorageError::PythonCleanup(e.to_string()))?;

        let result = ComprehensiveCleanupResult {
            removed_packages,
            python_cleanup,
        };

        if dry_run {
            info!(
                "Comprehensive cleanup dry run: {} packages, {} venvs would be removed",
                result.removed_packages.len(),
                result.python_cleanup.items_that_would_be_cleaned()
            );
        } else {
            info!(
                "Comprehensive cleanup completed: {} packages, {} venvs, {} space freed",
                result.removed_packages.len(),
                result.python_cleanup.items_cleaned(),
                result.python_cleanup.format_space_freed()
            );
        }

        Ok(result)
    }

    /// Clean up only Python virtual environments
    pub async fn cleanup_python_only(&self, dry_run: bool) -> Result<CleanupResult, StorageError> {
        info!("Starting Python-only cleanup (dry_run: {})", dry_run);

        let python_analyzer = PythonCleanupAnalyzer::new();

        // Get list of all active packages
        let active_packages = self
            .list_installed()
            .map_err(|e| StorageError::DirectoryRead(e.to_string()))?;
        let active_package_names: Vec<String> = active_packages
            .into_iter()
            .map(|p| format!("{}@{}", p.name, p.version))
            .collect();

        // Find orphaned virtual environments
        let orphaned_venvs = python_analyzer
            .analyze_orphaned_venvs(&active_package_names)
            .await
            .map_err(|e| StorageError::PythonCleanup(e.to_string()))?;

        // Clean up (or dry run)
        let result = python_analyzer
            .cleanup_orphaned_venvs(&orphaned_venvs, dry_run)
            .await
            .map_err(|e| StorageError::PythonCleanup(e.to_string()))?;

        if dry_run {
            info!(
                "Python cleanup dry run: {} venvs would be cleaned",
                result.items_that_would_be_cleaned()
            );
        } else {
            info!(
                "Python cleanup completed: {} venvs cleaned, {} space freed",
                result.items_cleaned(),
                result.format_space_freed()
            );
        }

        Ok(result)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("Directory creation failed: {0}")]
    DirectoryCreation(#[source] std::io::Error),

    #[error("Directory read failed: {0}")]
    DirectoryRead(String),

    #[error("Directory removal failed: {0}")]
    DirectoryRemoval(#[source] std::io::Error),

    #[error(transparent)]
    Manifest(#[from] ManifestLoadError),

    #[error("Metadata read failed: {0}")]
    MetadataRead(#[source] std::io::Error),

    #[error("Package not found: {0}")]
    PackageNotFound(String),

    #[error("Feature not implemented: {0}")]
    NotImplemented(String),

    #[error("Project discovery failed: {0}")]
    ProjectDiscovery(String),

    #[error("Dependency resolution failed: {0}")]
    DependencyResolution(String),

    #[error("Python cleanup failed: {0}")]
    PythonCleanup(String),

    #[error(
        "Package {name}@{version} is in use by another process; close any \
         running Houdini that depends on it and try again ({source})"
    )]
    PackageInUse {
        name: String,
        version: String,
        #[source]
        source: std::io::Error,
    },
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
            "[package]\npath = \"studio/test-package\"\nname = \"Test Package\"\nversion = \"1.0.0\"",
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
path = "studio/test-package"
name = "Test Package"
version = "1.0.0"
description = "A test package"

[houdini]
min_version = "20.5"
"#;
        std::fs::write(package_dir.join("hpm.toml"), manifest_content).unwrap();

        let packages = storage_manager.list_installed().unwrap();
        assert_eq!(packages.len(), 1);
        assert_eq!(packages[0].name, "test-package");
        assert_eq!(packages[0].version, "1.0.0");
    }

    // Error path tests

    #[tokio::test]
    async fn remove_nonexistent_package_fails() {
        let temp_dir = TempDir::new().unwrap();
        let storage_config = StorageConfig {
            home_dir: temp_dir.path().to_path_buf(),
            packages_dir: temp_dir.path().join("packages"),
            cache_dir: temp_dir.path().join("cache"),
            registry_cache_dir: temp_dir.path().join("registry"),
        };
        let storage_manager = StorageManager::new(storage_config).unwrap();

        let result = storage_manager.remove_package("nonexistent", "1.0.0").await;
        assert!(result.is_err());
        match result {
            Err(StorageError::PackageNotFound(msg)) => {
                assert!(msg.contains("nonexistent"));
            }
            _ => panic!("Expected PackageNotFound error"),
        }
    }

    #[test]
    fn list_packages_with_corrupted_manifest() {
        let temp_dir = TempDir::new().unwrap();
        let storage_config = StorageConfig {
            home_dir: temp_dir.path().to_path_buf(),
            packages_dir: temp_dir.path().join("packages"),
            cache_dir: temp_dir.path().join("cache"),
            registry_cache_dir: temp_dir.path().join("registry"),
        };
        let storage_manager = StorageManager::new(storage_config).unwrap();

        // Create a package directory with a corrupted manifest
        let package_dir = temp_dir.path().join("packages").join("corrupted-pkg@1.0.0");
        std::fs::create_dir_all(&package_dir).unwrap();
        std::fs::write(
            package_dir.join("hpm.toml"),
            "this is not valid toml { [ broken",
        )
        .unwrap();

        let result = storage_manager.list_installed();
        assert!(result.is_err());
        match result {
            Err(StorageError::Manifest(ManifestLoadError::Parse { path, .. })) => {
                assert!(path.ends_with("hpm.toml"));
            }
            other => panic!("Expected Manifest::Parse error, got: {:?}", other),
        }
    }

    #[test]
    fn list_packages_skips_non_directories() {
        let temp_dir = TempDir::new().unwrap();
        let storage_config = StorageConfig {
            home_dir: temp_dir.path().to_path_buf(),
            packages_dir: temp_dir.path().join("packages"),
            cache_dir: temp_dir.path().join("cache"),
            registry_cache_dir: temp_dir.path().join("registry"),
        };
        let storage_manager = StorageManager::new(storage_config).unwrap();

        // Create the packages directory and add a file (not a directory)
        std::fs::create_dir_all(temp_dir.path().join("packages")).unwrap();
        std::fs::write(
            temp_dir.path().join("packages").join("random-file.txt"),
            "not a package",
        )
        .unwrap();

        // Should not error, just skip the file
        let packages = storage_manager.list_installed().unwrap();
        assert!(packages.is_empty());
    }

    #[test]
    fn list_packages_skips_directories_without_manifest() {
        let temp_dir = TempDir::new().unwrap();
        let storage_config = StorageConfig {
            home_dir: temp_dir.path().to_path_buf(),
            packages_dir: temp_dir.path().join("packages"),
            cache_dir: temp_dir.path().join("cache"),
            registry_cache_dir: temp_dir.path().join("registry"),
        };
        let storage_manager = StorageManager::new(storage_config).unwrap();

        // Create a package directory without hpm.toml
        let package_dir = temp_dir.path().join("packages").join("empty-pkg@1.0.0");
        std::fs::create_dir_all(&package_dir).unwrap();
        std::fs::write(package_dir.join("README.md"), "no manifest here").unwrap();

        // Should not error, just skip directories without manifest
        let packages = storage_manager.list_installed().unwrap();
        assert!(packages.is_empty());
    }

    #[test]
    fn list_installed_scoped_packages() {
        let temp_dir = TempDir::new().unwrap();
        let storage_config = StorageConfig {
            home_dir: temp_dir.path().to_path_buf(),
            cache_dir: temp_dir.path().join("cache"),
            packages_dir: temp_dir.path().join("packages"),
            registry_cache_dir: temp_dir.path().join("registry"),
        };

        let storage_manager = StorageManager::new(storage_config).unwrap();

        // Create a scoped package at packages/tumblehead/fire-fx@1.0.0/
        let package_dir = temp_dir
            .path()
            .join("packages")
            .join("tumblehead")
            .join("fire-fx@1.0.0");
        std::fs::create_dir_all(&package_dir).unwrap();

        let manifest_content = r#"
[package]
path = "tumblehead/fire-fx"
name = "Fire FX"
version = "1.0.0"
description = "A fire effects package"

[houdini]
min_version = "20.5"
"#;
        std::fs::write(package_dir.join("hpm.toml"), manifest_content).unwrap();

        // Also create a non-scoped package at packages/old-pkg@2.0.0/
        let old_pkg_dir = temp_dir.path().join("packages").join("old-pkg@2.0.0");
        std::fs::create_dir_all(&old_pkg_dir).unwrap();
        std::fs::write(
            old_pkg_dir.join("hpm.toml"),
            "[package]\npath = \"studio/old-pkg\"\nname = \"Old Package\"\nversion = \"2.0.0\"",
        )
        .unwrap();

        let packages = storage_manager.list_installed().unwrap();
        assert_eq!(packages.len(), 2);

        // Find the scoped package
        let fire_fx = packages.iter().find(|p| p.name == "fire-fx").unwrap();
        assert_eq!(fire_fx.version, "1.0.0");

        // Find the non-scoped package
        let old_pkg = packages.iter().find(|p| p.name == "old-pkg").unwrap();
        assert_eq!(old_pkg.version, "2.0.0");
    }

    #[tokio::test]
    async fn install_from_path_without_manifest_fails() {
        let temp_dir = TempDir::new().unwrap();
        let storage_config = StorageConfig {
            home_dir: temp_dir.path().to_path_buf(),
            packages_dir: temp_dir.path().join("packages"),
            cache_dir: temp_dir.path().join("cache"),
            registry_cache_dir: temp_dir.path().join("registry"),
        };
        let storage_manager = StorageManager::new(storage_config).unwrap();

        // Create a source directory without hpm.toml
        let source_dir = temp_dir.path().join("source-pkg");
        std::fs::create_dir_all(&source_dir).unwrap();

        let result = storage_manager.install_from_path(&source_dir).await;
        assert!(result.is_err());
        match result {
            Err(StorageError::Manifest(ManifestLoadError::NotFound { path })) => {
                assert!(path.ends_with("hpm.toml"));
            }
            other => panic!("Expected Manifest::NotFound error, got: {:?}", other),
        }
    }
}
