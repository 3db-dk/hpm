//! Virtual environment management

use crate::bundled::run_uv_command;
use crate::get_venvs_dir;
use crate::types::{OrphanedVenv, ResolvedDependencySet, VenvMetadata};
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use tokio::fs;
use tracing::{debug, info, warn};

/// Virtual environment manager
///
/// Manages content-addressable virtual environments for Python dependencies.
/// Virtual environments are shared between packages with identical resolved dependencies
/// to optimize disk usage and installation time.
///
/// # Example Usage
///
/// ```rust,no_run
/// use hpm_python::{VenvManager, ResolvedDependencySet, PythonVersion};
///
/// # async fn example() -> anyhow::Result<()> {
/// let manager = VenvManager::new();
///
/// // Create a resolved dependency set
/// let mut resolved = ResolvedDependencySet::new(PythonVersion::new(3, 9, None));
/// resolved.add_package("numpy", "1.24.0");
/// resolved.add_package("requests", "2.28.0");
///
/// // Ensure virtual environment exists (creates if needed)
/// let venv_path = manager.ensure_virtual_environment(&resolved).await?;
/// println!("Virtual environment at: {:?}", venv_path);
/// # Ok(())
/// # }
/// ```
pub struct VenvManager {
    venvs_dir: PathBuf,
}

impl VenvManager {
    pub fn new() -> Self {
        Self {
            venvs_dir: get_venvs_dir(),
        }
    }

    /// Ensure a virtual environment exists for the given dependency set
    ///
    /// This is the primary method for obtaining a virtual environment. It:
    /// 1. Generates a content hash from the resolved dependencies
    /// 2. Checks if a virtual environment with that hash already exists
    /// 3. If not, creates a new virtual environment with the exact dependency versions
    /// 4. Updates metadata to track package usage
    ///
    /// Virtual environments are shared between packages that have identical resolved dependencies,
    /// providing significant space and time savings.
    ///
    /// # Arguments
    ///
    /// * `resolved_deps` - The resolved dependency set containing exact package versions and Python version
    ///
    /// # Returns
    ///
    /// Returns the path to the virtual environment directory.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Virtual environment creation fails
    /// - Package installation fails
    /// - Metadata cannot be written
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use hpm_python::{VenvManager, ResolvedDependencySet, PythonVersion};
    ///
    /// # async fn example() -> anyhow::Result<()> {
    /// let manager = VenvManager::new();
    /// let mut resolved = ResolvedDependencySet::new(PythonVersion::new(3, 9, None));
    /// resolved.add_package("numpy", "1.24.0");
    ///
    /// let venv_path = manager.ensure_virtual_environment(&resolved).await?;
    /// // Virtual environment is now ready for use
    /// # Ok(())
    /// # }
    /// ```
    pub async fn ensure_virtual_environment(
        &self,
        resolved_deps: &ResolvedDependencySet,
    ) -> Result<PathBuf> {
        let hash = resolved_deps.hash();
        let venv_path = self.venvs_dir.join(&hash);

        if !venv_path.exists() {
            info!("Creating virtual environment for dependency set {}", hash);
            self.create_virtual_environment(&venv_path, resolved_deps)
                .await
                .context("Failed to create virtual environment")?;
        } else {
            debug!("Using existing virtual environment: {}", hash);
        }

        self.update_venv_metadata(&venv_path, resolved_deps)
            .await
            .context("Failed to update virtual environment metadata")?;

        Ok(venv_path)
    }

    /// Create a new virtual environment
    async fn create_virtual_environment(
        &self,
        venv_path: &Path,
        resolved_deps: &ResolvedDependencySet,
    ) -> Result<()> {
        // Ensure parent directory exists
        fs::create_dir_all(&self.venvs_dir)
            .await
            .context("Failed to create venvs directory")?;

        // Create virtual environment using UV
        let python_version = resolved_deps.python_version.to_string();
        run_uv_command(&[
            "venv",
            venv_path.to_str().unwrap(),
            "--python",
            &python_version,
        ])
        .await
        .context("Failed to create virtual environment")?;

        // Install resolved dependencies
        if !resolved_deps.packages.is_empty() {
            self.install_packages(venv_path, resolved_deps)
                .await
                .context("Failed to install packages in virtual environment")?;
        }

        info!("Virtual environment created at {:?}", venv_path);
        Ok(())
    }

    /// Install packages in the virtual environment
    async fn install_packages(
        &self,
        venv_path: &Path,
        resolved_deps: &ResolvedDependencySet,
    ) -> Result<()> {
        // Create a requirements file with exact versions
        let req_file = self
            .create_resolved_requirements_file(resolved_deps)
            .await
            .context("Failed to create resolved requirements file")?;

        // Install packages using UV
        let venv_str = venv_path.to_str().unwrap();
        run_uv_command(&[
            "pip",
            "install",
            "-r",
            req_file.path().to_str().unwrap(),
            "--target",
            &format!("{}/lib/python/site-packages", venv_str),
        ])
        .await
        .context("Failed to install packages")?;

        debug!(
            "Installed {} packages in virtual environment",
            resolved_deps.packages.len()
        );
        Ok(())
    }

    /// Create a requirements file with resolved exact versions
    async fn create_resolved_requirements_file(
        &self,
        resolved_deps: &ResolvedDependencySet,
    ) -> Result<tempfile::NamedTempFile> {
        use std::io::Write;

        let mut temp_file = tempfile::NamedTempFile::new()
            .context("Failed to create temporary requirements file")?;

        for (name, version) in &resolved_deps.packages {
            writeln!(temp_file, "{}=={}", name, version)
                .context("Failed to write to requirements file")?;
        }

        temp_file
            .flush()
            .context("Failed to flush requirements file")?;
        Ok(temp_file)
    }

    /// Update virtual environment metadata
    async fn update_venv_metadata(
        &self,
        venv_path: &Path,
        resolved_deps: &ResolvedDependencySet,
    ) -> Result<()> {
        let hash = resolved_deps.hash();
        let metadata_path = venv_path.join("metadata.json");

        let mut metadata = if metadata_path.exists() {
            // Load existing metadata
            let content = fs::read_to_string(&metadata_path)
                .await
                .context("Failed to read metadata file")?;
            serde_json::from_str::<VenvMetadata>(&content)
                .context("Failed to parse metadata file")?
        } else {
            // Create new metadata
            VenvMetadata::new(hash, resolved_deps.clone(), venv_path.to_path_buf())
        };

        metadata.last_used = Some(chrono::Utc::now());

        // Write updated metadata
        let metadata_json =
            serde_json::to_string_pretty(&metadata).context("Failed to serialize metadata")?;
        fs::write(&metadata_path, metadata_json)
            .await
            .context("Failed to write metadata file")?;

        Ok(())
    }

    /// List all virtual environments
    pub async fn list_all_venvs(&self) -> Result<Vec<VenvMetadata>> {
        let mut venvs = Vec::new();

        if !self.venvs_dir.exists() {
            return Ok(venvs);
        }

        let mut dir_entries = fs::read_dir(&self.venvs_dir)
            .await
            .context("Failed to read venvs directory")?;

        while let Some(entry) = dir_entries.next_entry().await? {
            if entry.file_type().await?.is_dir() {
                let metadata_path = entry.path().join("metadata.json");
                if metadata_path.exists() {
                    match self.load_venv_metadata(&metadata_path).await {
                        Ok(metadata) => venvs.push(metadata),
                        Err(e) => warn!("Failed to load metadata for {:?}: {}", entry.path(), e),
                    }
                }
            }
        }

        Ok(venvs)
    }

    /// Load virtual environment metadata
    async fn load_venv_metadata(&self, metadata_path: &Path) -> Result<VenvMetadata> {
        let content = fs::read_to_string(metadata_path)
            .await
            .context("Failed to read metadata file")?;
        let metadata = serde_json::from_str::<VenvMetadata>(&content)
            .context("Failed to parse metadata file")?;
        Ok(metadata)
    }

    /// Find orphaned virtual environments
    pub async fn find_orphaned_venvs(
        &self,
        active_packages: &[String],
    ) -> Result<Vec<OrphanedVenv>> {
        let all_venvs = self.list_all_venvs().await?;
        let mut orphaned = Vec::new();

        for venv_meta in all_venvs {
            let is_used = venv_meta
                .used_by_packages
                .iter()
                .any(|pkg| active_packages.contains(pkg));

            if !is_used {
                if let Ok(size) = self.calculate_venv_size(&venv_meta.path).await {
                    orphaned.push(OrphanedVenv {
                        hash: venv_meta.hash,
                        path: venv_meta.path,
                        size,
                        created_at: venv_meta.created_at,
                        last_used: venv_meta.last_used,
                    });
                }
            }
        }

        Ok(orphaned)
    }

    /// Calculate the size of a virtual environment
    pub async fn calculate_venv_size(&self, venv_path: &Path) -> Result<u64> {
        let mut total_size = 0u64;
        let mut stack = vec![venv_path.to_path_buf()];

        while let Some(current_path) = stack.pop() {
            if let Ok(mut dir_entries) = fs::read_dir(&current_path).await {
                while let Some(entry) = dir_entries.next_entry().await? {
                    let metadata = entry.metadata().await?;
                    if metadata.is_dir() {
                        stack.push(entry.path());
                    } else {
                        total_size += metadata.len();
                    }
                }
            }
        }

        Ok(total_size)
    }

    /// Remove a virtual environment
    pub async fn remove_venv(&self, venv_path: &Path) -> Result<()> {
        if venv_path.exists() {
            fs::remove_dir_all(venv_path)
                .await
                .context("Failed to remove virtual environment directory")?;
            info!("Removed virtual environment: {:?}", venv_path);
        }
        Ok(())
    }

    /// Get the Python site-packages path for a virtual environment
    pub fn get_python_site_packages_path(&self, venv_path: &Path) -> PathBuf {
        #[cfg(target_os = "windows")]
        {
            venv_path.join("Lib").join("site-packages")
        }
        #[cfg(not(target_os = "windows"))]
        {
            venv_path.join("lib").join("python").join("site-packages")
        }
    }
}

impl Default for VenvManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_venv_manager_creation() {
        let manager = VenvManager::new();
        assert!(manager.venvs_dir.ends_with(".hpm/venvs"));
    }

    #[tokio::test]
    async fn test_python_site_packages_path() {
        let manager = VenvManager::new();
        let venv_path = PathBuf::from("/test/venv");
        let site_packages = manager.get_python_site_packages_path(&venv_path);

        #[cfg(target_os = "windows")]
        assert!(site_packages.ends_with("Lib/site-packages"));

        #[cfg(not(target_os = "windows"))]
        assert!(site_packages.ends_with("lib/python/site-packages"));
    }
}
