//! Virtual environment management

use crate::bundled::{ensure_managed_python, run_uv_command};
use crate::get_venvs_dir;
use crate::types::{OrphanedVenv, PythonVersion, ResolvedDependencySet, VenvMetadata};
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
/// let manager = VenvManager::new()?;
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
    /// Construct a manager rooted at the default `~/.hpm/venvs/`.
    ///
    /// Errors when the user's home directory cannot be resolved (no
    /// `$HOME` / `%USERPROFILE%`). Tests that want a controlled location
    /// should use [`with_venvs_dir`](Self::with_venvs_dir) instead.
    pub fn new() -> Result<Self> {
        Ok(Self {
            venvs_dir: get_venvs_dir()?,
        })
    }

    /// Create a `VenvManager` rooted at an explicit venvs directory.
    ///
    /// Intended for tests and any caller that needs to isolate venv state from
    /// the default `~/.hpm/venvs/` location.
    pub fn with_venvs_dir(venvs_dir: PathBuf) -> Self {
        Self { venvs_dir }
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
    /// let manager = VenvManager::new()?;
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

        // Self-heal venvs left half-installed by earlier hpm versions: the
        // pre-0.7.1 `--target` bug could populate metadata.json with a
        // successful-looking dep set while leaving `site-packages` empty.
        // Now that the install path is fixed, trusting `venv_path.exists()`
        // alone still wedges users on the stale dir. Verify the expected
        // packages are present; if not, drop the directory and rebuild.
        // Also rebuild when metadata.json is from an incompatible schema —
        // otherwise launch fails hard on the parse error with no way to
        // clear the cache from the UI.
        if venv_path.exists() {
            if let Some(reason) = self.venv_staleness_reason(&venv_path, resolved_deps).await {
                warn!("Rebuilding venv {}: {}", hash, reason);
                fs::remove_dir_all(&venv_path)
                    .await
                    .context("Failed to remove stale virtual environment")?;
            }
        }

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

    /// Returns a human-readable reason if the venv at `venv_path` should be
    /// rebuilt, or `None` if it's reusable.
    ///
    /// Staleness sources:
    /// - `site-packages` is missing or doesn't contain the resolved packages
    ///   (e.g. the pre-0.7.1 `--target` bug).
    /// - `metadata.json` exists but can't be deserialized, which happens when
    ///   the schema changed across hpm versions. Without this check, launch
    ///   fails hard on the parse error with no way to clear the cache from
    ///   the UI.
    async fn venv_staleness_reason(
        &self,
        venv_path: &Path,
        resolved_deps: &ResolvedDependencySet,
    ) -> Option<&'static str> {
        if !resolved_deps.packages.is_empty() {
            let site_packages =
                self.get_python_site_packages_path(venv_path, &resolved_deps.python_version);
            if fs::metadata(&site_packages).await.is_err() {
                return Some("site-packages missing");
            }
            if !any_package_present(&site_packages, resolved_deps).await {
                return Some("site-packages missing expected packages");
            }
        }

        let metadata_path = venv_path.join("metadata.json");
        if metadata_path.exists() {
            match fs::read_to_string(&metadata_path).await {
                Ok(content) => {
                    if serde_json::from_str::<VenvMetadata>(&content).is_err() {
                        return Some("metadata.json is from an incompatible schema");
                    }
                }
                Err(_) => return Some("metadata.json is unreadable"),
            }
        }

        None
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
        // `uv venv --python <ver>` historically auto-downloads managed
        // CPython, but make it explicit so a clean Windows box (no system
        // Python, no managed install) doesn't trip the same "No interpreter
        // found" failure mode that `pip compile` does.
        ensure_managed_python(&python_version).await?;
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

        // Install into the venv's own Python. `--target` would drop files at an
        // arbitrary path that the venv interpreter doesn't import from — that
        // left ~/.hpm/venvs/<hash>/Lib/site-packages empty on Windows even
        // though metadata claimed the packages were installed.
        let python_exe = venv_python_executable(venv_path);
        let python_str = python_exe
            .to_str()
            .context("Venv Python path is not UTF-8")?;
        run_uv_command(&[
            "pip",
            "install",
            "-r",
            req_file.path().to_str().unwrap(),
            "--python",
            python_str,
        ])
        .await
        .context("Failed to install packages into virtual environment")?;

        // Confirm at least one requested package actually landed in the venv
        // before we write metadata claiming success. This would have caught
        // the `--target` regression loudly instead of silently.
        let site_packages =
            self.get_python_site_packages_path(venv_path, &resolved_deps.python_version);
        if !resolved_deps.packages.is_empty() {
            let populated = fs::metadata(&site_packages).await.is_ok()
                && any_package_present(&site_packages, resolved_deps).await;
            if !populated {
                return Err(anyhow::anyhow!(
                    "uv reported success but {} is missing the installed packages",
                    site_packages.display()
                ));
            }
        }

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

        metadata.last_used = Some(std::time::SystemTime::now());

        // Atomic write: stage to <path>.tmp then rename. The self-heal
        // path rebuilds the entire venv on a truncated metadata.json,
        // which is expensive — avoid triggering it on a crash mid-write.
        let metadata_json =
            serde_json::to_string_pretty(&metadata).context("Failed to serialize metadata")?;
        let mut tmp_path = metadata_path.as_os_str().to_os_string();
        tmp_path.push(".tmp");
        let tmp_path = PathBuf::from(tmp_path);
        fs::write(&tmp_path, metadata_json)
            .await
            .context("Failed to write metadata file")?;
        fs::rename(&tmp_path, &metadata_path)
            .await
            .context("Failed to commit metadata file")?;

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

    /// Get the Python site-packages path for a virtual environment.
    ///
    /// The Unix layout is `lib/pythonX.Y/site-packages` — the previous
    /// hardcoded `lib/python/site-packages` didn't exist in any real venv,
    /// so PYTHONPATH pointed at a directory Python never populated. The
    /// caller supplies the Python version (already known from the resolved
    /// dependency set) so we don't have to parse `pyvenv.cfg` here.
    pub fn get_python_site_packages_path(
        &self,
        venv_path: &Path,
        python_version: &PythonVersion,
    ) -> PathBuf {
        #[cfg(target_os = "windows")]
        {
            let _ = python_version; // Windows venvs share one Lib/site-packages
            venv_path.join("Lib").join("site-packages")
        }
        #[cfg(not(target_os = "windows"))]
        {
            venv_path
                .join("lib")
                .join(format!(
                    "python{}.{}",
                    python_version.major, python_version.minor
                ))
                .join("site-packages")
        }
    }
}

/// Absolute path to the Python interpreter inside a venv created by `uv venv`.
fn venv_python_executable(venv_path: &Path) -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        venv_path.join("Scripts").join("python.exe")
    }
    #[cfg(not(target_os = "windows"))]
    {
        venv_path.join("bin").join("python")
    }
}

/// Verify that at least one resolved package has a `dist-info` directory in
/// `site_packages`. PEP 503 canonicalization means the dist-info directory
/// name uses lowercase and underscores (e.g. `foo-bar` installs as
/// `foo_bar-1.0.dist-info`), so we normalize both sides before comparing.
async fn any_package_present(site_packages: &Path, resolved_deps: &ResolvedDependencySet) -> bool {
    let Ok(mut entries) = fs::read_dir(site_packages).await else {
        return false;
    };
    let mut dist_info_prefixes: Vec<String> = Vec::new();
    while let Ok(Some(entry)) = entries.next_entry().await {
        let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
        if let Some(stem) = name.strip_suffix(".dist-info")
            && let Some((pkg_name, _version)) = stem.rsplit_once('-')
        {
            dist_info_prefixes.push(normalize_pep503(pkg_name));
        }
    }
    resolved_deps
        .packages
        .keys()
        .any(|pkg| dist_info_prefixes.contains(&normalize_pep503(pkg)))
}

/// PEP 503 name normalization: lowercase, and collapse `-`/`_`/`.` to `_`.
fn normalize_pep503(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for c in name.chars() {
        if c == '-' || c == '.' {
            out.push('_');
        } else {
            out.push(c.to_ascii_lowercase());
        }
    }
    out
}

// `VenvManager::new()` is fallible (needs `$HOME` / `%USERPROFILE%`),
// so the `Default` impl was removed — there is no sensible default.

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_venv_manager_creation() {
        let manager = VenvManager::new().expect("home dir resolves under test env");
        assert!(manager.venvs_dir.ends_with(".hpm/venvs"));
    }

    #[tokio::test]
    async fn test_python_site_packages_path() {
        let manager = VenvManager::new().expect("home dir resolves under test env");
        let venv_path = PathBuf::from("/test/venv");
        let python = PythonVersion::new(3, 11, None);
        let site_packages = manager.get_python_site_packages_path(&venv_path, &python);

        #[cfg(target_os = "windows")]
        assert!(site_packages.ends_with("Lib/site-packages"));

        #[cfg(not(target_os = "windows"))]
        assert!(
            site_packages.ends_with("lib/python3.11/site-packages"),
            "unexpected site-packages layout: {}",
            site_packages.display()
        );
    }

    /// Ignored by default: creates a real venv via the bundled uv and verifies
    /// that installed packages land in `site-packages` (the Bug A regression
    /// went unnoticed because no existing test actually invoked uv against a
    /// venv). Run with `cargo test --package hpm-python -- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn test_install_populates_real_site_packages() {
        crate::initialize().await.expect("uv init failed");

        let tmp = tempfile::TempDir::new().unwrap();
        let manager = VenvManager::with_venvs_dir(tmp.path().to_path_buf());

        let python = PythonVersion::new(3, 11, None);
        let mut resolved = ResolvedDependencySet::new(python.clone());
        // `packaging` is tiny and pure-Python; enough to prove uv installed
        // into the venv rather than some phantom --target directory.
        resolved.add_package("packaging", "24.1");

        let venv_path = manager
            .ensure_virtual_environment(&resolved)
            .await
            .expect("ensure_virtual_environment failed");

        let site_packages = manager.get_python_site_packages_path(&venv_path, &python);
        assert!(
            site_packages.exists(),
            "site-packages not created at {}",
            site_packages.display()
        );

        let mut found = false;
        let mut rd = std::fs::read_dir(&site_packages).unwrap();
        while let Some(Ok(entry)) = rd.next() {
            let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
            if name.starts_with("packaging-") && name.ends_with(".dist-info") {
                found = true;
                break;
            }
        }
        assert!(
            found,
            "packaging-*.dist-info not found in {}",
            site_packages.display()
        );
    }

    /// Ignored by default: simulates the exact state reported by a user after
    /// upgrading from a buggy hpm — an empty `site-packages/` under a venv
    /// whose metadata already claims the packages are installed.
    /// `ensure_virtual_environment` must detect this and rebuild rather than
    /// silently reusing it.
    #[tokio::test]
    #[ignore]
    async fn test_ensure_heals_half_installed_venv() {
        crate::initialize().await.expect("uv init failed");

        let tmp = tempfile::TempDir::new().unwrap();
        let manager = VenvManager::with_venvs_dir(tmp.path().to_path_buf());

        let python = PythonVersion::new(3, 11, None);
        let mut resolved = ResolvedDependencySet::new(python.clone());
        resolved.add_package("packaging", "24.1");

        // Pre-create the hashed venv dir, mimicking a pre-fix install:
        // empty site-packages + metadata claiming everything landed.
        let hash = resolved.hash();
        let venv_path = tmp.path().join(&hash);
        let site_packages = manager.get_python_site_packages_path(&venv_path, &python);
        fs::create_dir_all(&site_packages).await.unwrap();
        fs::write(site_packages.join("_virtualenv.pth"), b"")
            .await
            .unwrap();
        let metadata = VenvMetadata::new(hash.clone(), resolved.clone(), venv_path.clone());
        fs::write(
            venv_path.join("metadata.json"),
            serde_json::to_string_pretty(&metadata).unwrap(),
        )
        .await
        .unwrap();

        let result = manager
            .ensure_virtual_environment(&resolved)
            .await
            .expect("ensure_virtual_environment should heal and rebuild");

        let mut found = false;
        let mut rd = std::fs::read_dir(manager.get_python_site_packages_path(&result, &python))
            .expect("site-packages missing after heal");
        while let Some(Ok(entry)) = rd.next() {
            let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
            if name.starts_with("packaging-") && name.ends_with(".dist-info") {
                found = true;
                break;
            }
        }
        assert!(
            found,
            "stale venv was reused instead of rebuilt; packaging-*.dist-info missing"
        );
    }

    /// Legacy hpm versions serialized `created_at`/`last_used` as ISO 8601
    /// strings; the current schema expects i64 epoch seconds. A venv written
    /// by the old version must be recognised as stale so the caller rebuilds
    /// rather than propagating the parse error as a hard launch failure.
    #[tokio::test]
    async fn test_unparseable_metadata_flagged_as_stale() {
        let tmp = tempfile::TempDir::new().unwrap();
        let manager = VenvManager::with_venvs_dir(tmp.path().to_path_buf());

        let python = PythonVersion::new(3, 11, None);
        let resolved = ResolvedDependencySet::new(python);
        let venv_path = tmp.path().join("legacy-venv");
        fs::create_dir_all(&venv_path).await.unwrap();

        // Pre-0.8 shape: ISO 8601 strings where the current schema wants i64.
        let legacy = r#"{
            "hash": "deadbeef",
            "dependency_set": {
                "python_version": {"major": 3, "minor": 11, "patch": null},
                "packages": {}
            },
            "created_at": "2026-04-21T11:09:23.090683Z",
            "last_used": "2026-04-22T13:43:18.436012800Z",
            "used_by_packages": [],
            "path": "/tmp/legacy-venv"
        }"#;
        fs::write(venv_path.join("metadata.json"), legacy)
            .await
            .unwrap();

        let reason = manager.venv_staleness_reason(&venv_path, &resolved).await;
        assert!(
            reason.is_some(),
            "legacy metadata.json should be flagged as stale",
        );
    }
}
