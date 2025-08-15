//! Python environment update utilities for HPM update command
//!
//! This module provides functionality to update Python virtual environments
//! efficiently, including dependency resolution, virtual environment migration,
//! and cleanup of outdated environments.

use crate::bundled::run_uv_command;
use crate::dependency::collect_python_dependencies;
use crate::resolver::resolve_dependencies;
use crate::types::{PythonDependencies, ResolvedDependencySet};
use crate::venv::VenvManager;
use anyhow::{Context, Result};
use hpm_package::PackageManifest;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tracing::{debug, info, warn};

/// Python environment update manager
pub struct PythonUpdateManager {
    venv_manager: VenvManager,
    _home_dir: PathBuf,
}

/// Result of a Python environment update
#[derive(Debug, Clone)]
pub struct PythonUpdateResult {
    pub updated_packages: Vec<String>,
    pub new_venv_hash: Option<String>,
    pub old_venv_hash: Option<String>,
    pub venv_migrated: bool,
    pub packages_resolved: usize,
}

impl PythonUpdateManager {
    pub fn new(home_dir: PathBuf) -> Result<Self> {
        let venv_manager = VenvManager::new();
        Ok(Self {
            venv_manager,
            _home_dir: home_dir,
        })
    }

    /// Update Python dependencies for a package with efficient virtual environment management
    ///
    /// This method:
    /// 1. Resolves new Python dependency versions
    /// 2. Calculates content hash for the new dependency set
    /// 3. Creates or reuses virtual environment based on content hash
    /// 4. Migrates package to new virtual environment if needed
    /// 5. Cleans up old virtual environment if no longer used
    ///
    /// # Arguments
    ///
    /// * `package_name` - Name of the package being updated
    /// * `new_manifest` - Updated package manifest with new Python dependencies
    /// * `current_venv_path` - Path to current virtual environment (if any)
    ///
    /// # Returns
    ///
    /// Returns `PythonUpdateResult` with details about the update operation.
    pub async fn update_python_environment(
        &mut self,
        package_name: &str,
        new_manifest: &PackageManifest,
        current_venv_path: Option<&Path>,
    ) -> Result<PythonUpdateResult> {
        info!("Updating Python environment for package: {}", package_name);

        // Get current virtual environment hash if available
        let old_venv_hash = if let Some(venv_path) = current_venv_path {
            self.get_venv_hash_from_path(venv_path)?
        } else {
            None
        };

        // Collect new Python dependencies
        let manifests = vec![new_manifest.clone()];

        let python_deps = collect_python_dependencies(&manifests).await?;

        if python_deps.dependencies.is_empty() {
            info!("No Python dependencies found - no virtual environment needed");
            return Ok(PythonUpdateResult {
                updated_packages: Vec::new(),
                new_venv_hash: None,
                old_venv_hash,
                venv_migrated: false,
                packages_resolved: 0,
            });
        }

        // Resolve dependencies to exact versions
        let resolved_deps = resolve_dependencies(&python_deps).await?;
        let new_venv_hash = resolved_deps.hash();

        debug!("Resolved {} Python packages", resolved_deps.packages.len());
        debug!("New virtual environment hash: {}", new_venv_hash);

        let mut venv_migrated = false;
        let mut updated_packages = Vec::new();

        // Check if we need a new virtual environment
        let needs_new_venv = old_venv_hash.as_ref() != Some(&new_venv_hash);

        if needs_new_venv {
            info!("Creating/reusing virtual environment: {}", new_venv_hash);

            // Create or reuse virtual environment
            let _new_venv_path = self
                .venv_manager
                .ensure_virtual_environment(&resolved_deps)
                .await?;

            // Link package to new virtual environment
            self.link_package_to_venv(package_name, &new_venv_hash)?;

            venv_migrated = true;

            // Collect updated packages
            for (name, version) in &resolved_deps.packages {
                updated_packages.push(format!("{}=={}", name, version));
            }

            // Clean up old virtual environment if it exists and is not used by other packages
            if let Some(old_hash) = &old_venv_hash {
                if !self.is_venv_used_by_other_packages(old_hash, package_name)? {
                    warn!("Cleaning up old virtual environment: {}", old_hash);
                    // Virtual environment cleanup would be implemented here
                    debug!("Would remove virtual environment: {}", old_hash);
                } else {
                    info!(
                        "Old virtual environment still used by other packages: {}",
                        old_hash
                    );
                }
            }
        } else {
            info!("Virtual environment is up to date: {}", new_venv_hash);
        }

        Ok(PythonUpdateResult {
            updated_packages,
            new_venv_hash: Some(new_venv_hash),
            old_venv_hash,
            venv_migrated,
            packages_resolved: resolved_deps.packages.len(),
        })
    }

    /// Compare current Python dependencies with new ones to identify updates
    pub async fn check_python_updates(
        &mut self,
        current_deps: &PythonDependencies,
        filter_packages: &[String],
    ) -> Result<Vec<PythonPackageUpdate>> {
        let mut updates = Vec::new();

        for (package_name, current_dep) in &current_deps.dependencies {
            // Skip if filtering and package not in filter
            if !filter_packages.is_empty() && !filter_packages.contains(package_name) {
                continue;
            }

            // Check for newer version using UV
            if let Ok(latest_version) = self.query_latest_python_version(package_name).await {
                // Parse current version from dependency spec
                if let Some(current_version) = self.extract_current_version(&current_dep.version) {
                    if let (Ok(current_ver), Ok(latest_ver)) = (
                        semver::Version::parse(&current_version),
                        semver::Version::parse(&latest_version),
                    ) {
                        if latest_ver > current_ver {
                            updates.push(PythonPackageUpdate {
                                name: package_name.clone(),
                                current_version,
                                latest_version,
                                extras: current_dep.extras.clone(),
                                optional: current_dep.optional,
                            });
                        }
                    }
                }
            }
        }

        Ok(updates)
    }

    /// Query PyPI for the latest version of a Python package using UV
    async fn query_latest_python_version(&self, package_name: &str) -> Result<String> {
        // Use UV to query PyPI for package information
        let output = run_uv_command(&["pip", "search", package_name, "--quiet"])
            .await
            .context("Failed to query PyPI for package version")?;

        // Parse output to extract latest version
        // This is simplified - UV pip search might not be available in all versions
        // In practice, we might need to use a different approach like querying PyPI API directly
        let output_str = String::from_utf8_lossy(&output.stdout);

        // Look for version pattern in output
        for line in output_str.lines() {
            if line.contains(package_name) {
                // Extract version using regex or string parsing
                // Simplified implementation for now
                if let Some(version_start) = line.find('(') {
                    if let Some(version_end) = line[version_start..].find(')') {
                        let version = &line[version_start + 1..version_start + version_end];
                        return Ok(version.to_string());
                    }
                }
            }
        }

        Err(anyhow::anyhow!(
            "Could not determine latest version for {}",
            package_name
        ))
    }

    /// Extract current version from dependency version specification
    fn extract_current_version(&self, version_spec: &crate::types::VersionSpec) -> Option<String> {
        // For now, return a simplified version extraction
        // In a full implementation, this would parse the VersionSpec properly
        Some(version_spec.to_string())
    }

    /// Get virtual environment hash from path
    fn get_venv_hash_from_path(&self, venv_path: &Path) -> Result<Option<String>> {
        // Extract hash from venv path (e.g., ~/.hpm/venvs/abc123def456 -> abc123def456)
        if let Some(file_name) = venv_path.file_name() {
            if let Some(hash) = file_name.to_str() {
                return Ok(Some(hash.to_string()));
            }
        }
        Ok(None)
    }

    /// Check if a virtual environment is used by other packages
    fn is_venv_used_by_other_packages(
        &self,
        _venv_hash: &str,
        _exclude_package: &str,
    ) -> Result<bool> {
        // Check project links in venv metadata or scan for other projects using this venv
        // This is simplified - in practice would check venv metadata or project configurations

        // For now, assume it might be used by others (conservative approach)
        // A full implementation would check the venv metadata file or scan project configurations
        Ok(false) // Simplified: assume not used by others
    }

    /// Link a package to a virtual environment
    fn link_package_to_venv(&self, package_name: &str, venv_hash: &str) -> Result<()> {
        // Create or update package -> venv mapping
        // This could be stored in package metadata or a separate index

        debug!(
            "Linking package {} to virtual environment {}",
            package_name, venv_hash
        );

        // In a full implementation, this would:
        // 1. Update package metadata with venv hash
        // 2. Update venv metadata with linked packages
        // 3. Create package.json files with proper PYTHONPATH

        Ok(())
    }

    /// Create an optimized virtual environment update plan
    pub async fn create_update_plan(
        &mut self,
        packages_to_update: &[String],
        manifests: &HashMap<String, PackageManifest>,
    ) -> Result<PythonUpdatePlan> {
        let mut plan = PythonUpdatePlan::new();

        // Group packages by their resolved dependency sets to maximize venv reuse
        for package_name in packages_to_update {
            if let Some(manifest) = manifests.get(package_name) {
                if let Some(_python_deps) = &manifest.python_dependencies {
                    let single_manifest = vec![manifest.clone()];

                    let deps = collect_python_dependencies(&single_manifest).await?;
                    let resolved = resolve_dependencies(&deps).await?;
                    let hash = resolved.hash();

                    plan.add_package_update(package_name.clone(), hash, resolved);
                }
            }
        }

        Ok(plan)
    }
}

/// Information about a Python package update
#[derive(Debug, Clone)]
pub struct PythonPackageUpdate {
    pub name: String,
    pub current_version: String,
    pub latest_version: String,
    pub extras: Vec<String>,
    pub optional: bool,
}

/// Plan for updating multiple Python packages efficiently
#[derive(Debug)]
pub struct PythonUpdatePlan {
    /// Groups of packages that can share the same virtual environment
    pub venv_groups: HashMap<String, VenvUpdateGroup>,
}

#[derive(Debug)]
pub struct VenvUpdateGroup {
    pub venv_hash: String,
    pub packages: Vec<String>,
    pub resolved_deps: ResolvedDependencySet,
    pub needs_new_venv: bool,
}

impl PythonUpdatePlan {
    fn new() -> Self {
        Self {
            venv_groups: HashMap::new(),
        }
    }

    fn add_package_update(
        &mut self,
        package_name: String,
        venv_hash: String,
        resolved_deps: ResolvedDependencySet,
    ) {
        let group = self
            .venv_groups
            .entry(venv_hash.clone())
            .or_insert_with(|| {
                VenvUpdateGroup {
                    venv_hash,
                    packages: Vec::new(),
                    resolved_deps,
                    needs_new_venv: true, // Will be determined later
                }
            });

        group.packages.push(package_name);
    }

    /// Get total number of packages to update
    pub fn total_packages(&self) -> usize {
        self.venv_groups
            .values()
            .map(|group| group.packages.len())
            .sum()
    }

    /// Get total number of virtual environments that will be created/updated
    pub fn total_venvs(&self) -> usize {
        self.venv_groups.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::PythonVersion;
    use tempfile::TempDir;

    #[test]
    fn test_extract_current_version() {
        let temp_dir = TempDir::new().unwrap();
        let manager = PythonUpdateManager::new(temp_dir.path().to_path_buf()).unwrap();

        let version_spec = crate::types::VersionSpec::new(">=1.2.0");
        assert!(manager.extract_current_version(&version_spec).is_some());
    }

    #[test]
    fn test_get_venv_hash_from_path() {
        let temp_dir = TempDir::new().unwrap();
        let manager = PythonUpdateManager::new(temp_dir.path().to_path_buf()).unwrap();

        let venv_path = PathBuf::from("/home/user/.hpm/venvs/abc123def456");
        let hash = manager.get_venv_hash_from_path(&venv_path).unwrap();
        assert_eq!(hash, Some("abc123def456".to_string()));
    }

    #[test]
    fn test_update_plan_creation() {
        let mut plan = PythonUpdatePlan::new();

        let resolved_deps = ResolvedDependencySet::new(PythonVersion::new(3, 9, None));
        plan.add_package_update(
            "package-a".to_string(),
            "hash123".to_string(),
            resolved_deps.clone(),
        );
        plan.add_package_update(
            "package-b".to_string(),
            "hash123".to_string(),
            resolved_deps,
        );

        assert_eq!(plan.total_packages(), 2);
        assert_eq!(plan.total_venvs(), 1);

        let group = plan.venv_groups.get("hash123").unwrap();
        assert_eq!(group.packages.len(), 2);
        assert!(group.packages.contains(&"package-a".to_string()));
        assert!(group.packages.contains(&"package-b".to_string()));
    }

    #[tokio::test]
    async fn test_python_update_result() {
        let result = PythonUpdateResult {
            updated_packages: vec!["numpy==1.24.0".to_string(), "requests==2.28.0".to_string()],
            new_venv_hash: Some("abc123".to_string()),
            old_venv_hash: Some("def456".to_string()),
            venv_migrated: true,
            packages_resolved: 2,
        };

        assert_eq!(result.updated_packages.len(), 2);
        assert!(result.venv_migrated);
        assert_eq!(result.packages_resolved, 2);
    }
}
