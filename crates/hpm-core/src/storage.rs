use crate::dependency::{DependencyResolver, PackageId};
use crate::discovery::ProjectDiscovery;
use hpm_config::{ProjectsConfig, StorageConfig};
use hpm_package::PackageManifest;
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

        // Check if we already have a compatible version installed
        let installed = self.list_installed()?;
        for pkg in &installed {
            if pkg.name == spec.name && pkg.is_compatible_with(&spec.version_req) {
                info!(
                    "Package {} already installed with compatible version {}",
                    spec.name, pkg.version
                );
                return Ok(pkg.clone());
            }
        }

        // For now, return an error indicating the package needs to be installed
        // In a full implementation, this would:
        // 1. Query the registry for available versions matching spec.version_req
        // 2. Download the best matching version
        // 3. Extract and validate the package
        Err(StorageError::PackageNotFound(format!(
            "Package {} {} not found in registry. Use 'hpm install --from-path <path>' to install from a local directory.",
            spec.name, spec.version_req
        )))
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
        if !manifest_path.exists() {
            return Err(StorageError::ManifestRead(format!(
                "No hpm.toml found in {}",
                source_path.display()
            )));
        }

        let manifest_content = std::fs::read_to_string(&manifest_path)
            .map_err(|e| StorageError::ManifestRead(e.to_string()))?;

        let manifest: PackageManifest = toml::from_str(&manifest_content)
            .map_err(|e| StorageError::ManifestParse(e.to_string()))?;

        let name = &manifest.package.name;
        let version = &manifest.package.version;

        info!("Installing {}@{} from {}", name, version, source_path.display());

        // Create the target directory
        let target_dir = self.config.package_dir(name, version);

        // Check if already installed
        if target_dir.exists() {
            warn!("Package {}@{} already exists, removing old version", name, version);
            std::fs::remove_dir_all(&target_dir)
                .map_err(|e| StorageError::DirectoryRemoval(e.to_string()))?;
        }

        // Copy the package directory
        self.copy_directory(source_path, &target_dir)?;

        info!("Successfully installed {}@{}", name, version);

        // Return the installed package info
        let metadata = std::fs::metadata(&target_dir)
            .map_err(|e| StorageError::MetadataRead(e.to_string()))?;

        Ok(InstalledPackage {
            name: name.clone(),
            version: version.clone(),
            manifest,
            install_path: target_dir,
            installed_at: metadata.created().unwrap_or_else(|_| std::time::SystemTime::now()),
        })
    }

    /// Copy a directory recursively
    fn copy_directory(
        &self,
        source: &std::path::Path,
        target: &std::path::Path,
    ) -> Result<(), StorageError> {
        std::fs::create_dir_all(target)
            .map_err(|e| StorageError::DirectoryCreation(e.to_string()))?;

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
                std::fs::create_dir_all(&target_path)
                    .map_err(|e| StorageError::DirectoryCreation(e.to_string()))?;
            } else {
                // Ensure parent directory exists
                if let Some(parent) = target_path.parent() {
                    std::fs::create_dir_all(parent)
                        .map_err(|e| StorageError::DirectoryCreation(e.to_string()))?;
                }
                std::fs::copy(entry.path(), &target_path)
                    .map_err(|e| StorageError::DirectoryRead(format!("Failed to copy file: {}", e)))?;
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
                match (semver::Version::parse(&a.version), semver::Version::parse(&b.version)) {
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

    /// Comprehensive cleanup including both packages and Python virtual environments
    pub async fn cleanup_comprehensive(
        &self,
        projects_config: &ProjectsConfig,
    ) -> Result<ComprehensiveCleanupResult, StorageError> {
        info!("Starting comprehensive cleanup (packages + Python environments)");

        // First, perform package cleanup
        let removed_packages = self.cleanup_unused(projects_config).await?;

        // Then, perform Python virtual environment cleanup
        let python_analyzer = PythonCleanupAnalyzer::new();

        // Get list of active packages after cleanup
        let remaining_packages = self
            .list_installed()
            .map_err(|e| StorageError::DirectoryRead(e.to_string()))?;
        let active_package_names: Vec<String> = remaining_packages
            .into_iter()
            .map(|p| format!("{}@{}", p.name, p.version))
            .collect();

        // Find and clean up orphaned virtual environments
        let orphaned_venvs = python_analyzer
            .analyze_orphaned_venvs(&active_package_names)
            .await
            .map_err(|e| StorageError::PythonCleanup(e.to_string()))?;

        let python_result = python_analyzer
            .cleanup_orphaned_venvs(&orphaned_venvs, false)
            .await
            .map_err(|e| StorageError::PythonCleanup(e.to_string()))?;

        let result = ComprehensiveCleanupResult {
            removed_packages,
            python_cleanup: python_result,
        };

        info!(
            "Comprehensive cleanup completed: {} packages, {} venvs, {} space freed",
            result.removed_packages.len(),
            result.python_cleanup.items_cleaned(),
            result.python_cleanup.format_space_freed()
        );

        Ok(result)
    }

    /// Comprehensive cleanup dry run including both packages and Python virtual environments
    pub async fn cleanup_comprehensive_dry_run(
        &self,
        projects_config: &ProjectsConfig,
    ) -> Result<ComprehensiveCleanupResult, StorageError> {
        info!("Starting comprehensive cleanup dry run");

        // First, get packages that would be removed
        let would_remove_packages = self.cleanup_unused_dry_run(projects_config).await?;

        // Get list of packages that would remain after cleanup
        let all_installed = self
            .list_installed()
            .map_err(|e| StorageError::DirectoryRead(e.to_string()))?;

        // Filter out packages that would be removed
        let remaining_packages: Vec<String> = all_installed
            .into_iter()
            .filter_map(|p| {
                let package_id = format!("{}@{}", p.name, p.version);
                if would_remove_packages.contains(&package_id) {
                    None
                } else {
                    Some(package_id)
                }
            })
            .collect();

        // Analyze Python virtual environments
        let python_analyzer = PythonCleanupAnalyzer::new();
        let orphaned_venvs = python_analyzer
            .analyze_orphaned_venvs(&remaining_packages)
            .await
            .map_err(|e| StorageError::PythonCleanup(e.to_string()))?;

        let python_result = python_analyzer
            .cleanup_orphaned_venvs(&orphaned_venvs, true) // dry_run = true
            .await
            .map_err(|e| StorageError::PythonCleanup(e.to_string()))?;

        let result = ComprehensiveCleanupResult {
            removed_packages: would_remove_packages,
            python_cleanup: python_result,
        };

        info!(
            "Comprehensive cleanup dry run completed: {} packages, {} venvs would be removed",
            result.removed_packages.len(),
            result.python_cleanup.items_that_would_be_cleaned()
        );

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

    #[error("Python cleanup failed: {0}")]
    PythonCleanup(String),
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
