//! `hpm install` — sync the current project against its manifest and lockfile.
//!
//! The install command is a thin shell over
//! [`hpm_core::ProjectManager::sync_dependencies`]: build a manager, call
//! sync, build the lockfile from the returned outcomes, write it. All the
//! mechanics (parallel fetch + install, Python venv, Houdini manifest
//! emission, stale sweep) live in `hpm-core` so every embedder gets exactly
//! the same behaviour.

use super::manifest_utils::{determine_manifest_path, load_manifest};
use crate::console::Console;
use anyhow::{Context, Result, bail};
use hpm_config::Config;
use hpm_core::{LockFile, ProjectManager, StorageManager};
use std::path::PathBuf;
use std::sync::Arc;
use tracing::info;

/// CLI entry for `hpm install`: sync, then report success. Internal callers
/// (`add`/`remove`/`update`) use [`install_dependencies`] directly and own
/// their own success line.
pub async fn execute(
    config: &Config,
    manifest_path: Option<PathBuf>,
    frozen_lockfile: bool,
    console: &mut Console,
) -> Result<()> {
    install_dependencies(config, manifest_path, frozen_lockfile, console).await?;
    console.success("Dependencies installed successfully");
    Ok(())
}

/// Install dependencies from `hpm.toml`.
///
/// * `config` — caller-loaded HPM configuration (shared with this process).
/// * `manifest_path` — explicit `hpm.toml` path, or `None` for cwd lookup.
/// * `frozen_lockfile` — fail instead of regenerating `hpm.lock`.
/// * `console` — where warnings about the existing lockfile go.
pub async fn install_dependencies(
    config: &Config,
    manifest_path: Option<PathBuf>,
    frozen_lockfile: bool,
    console: &mut Console,
) -> Result<()> {
    info!("Starting dependency installation");
    if frozen_lockfile {
        info!("Using frozen lockfile mode - lock file must exist and not change");
    }

    let manifest_path = determine_manifest_path(manifest_path)?;
    info!("Using manifest: {}", manifest_path.display());

    let manifest = load_manifest(&manifest_path)
        .with_context(|| format!("Failed to load manifest from {}", manifest_path.display()))?;
    info!(
        "Installing dependencies for package: {} v{}",
        manifest.package.name, manifest.package.version
    );

    let project_dir = manifest_path
        .parent()
        .context("Manifest file has no parent directory")?
        .to_path_buf();
    let lock_path = project_dir.join("hpm.lock");

    // Pre-sync: load and verify the existing lockfile if present. This is
    // the only place that fails fast under --frozen-lockfile.
    if frozen_lockfile && !lock_path.exists() {
        bail!(
            "--frozen-lockfile requires hpm.lock to exist. \
             Run 'hpm install' first to generate it."
        );
    }
    let existing_lock = if lock_path.exists() {
        match LockFile::load(&lock_path) {
            Ok(lock) => {
                lock.verify_checksums(&config.storage.packages_dir)
                    .context(
                        "Package integrity check failed. \
                     Delete the corrupted package and run 'hpm install' again.",
                    )?;
                info!("Cached packages verified successfully");
                if let Some(ref metadata) = lock.metadata
                    && let Some(days) = metadata.days_since_generated()
                    && days > 90
                {
                    console.warn(format!(
                        "Lock file is {days} days old. \
                         Consider running 'hpm update' to check for newer versions."
                    ));
                }
                Some(lock)
            }
            Err(e) => {
                if frozen_lockfile {
                    bail!(
                        "--frozen-lockfile requires a valid hpm.lock, but loading it failed: \
                         {}. Re-run without --frozen-lockfile to regenerate it.",
                        e
                    );
                }
                console.warn(format!("Failed to load existing lock file: {e}"));
                None
            }
        }
    } else {
        None
    };

    // Sync via ProjectManager. The manager owns all the install mechanics —
    // parallel fetch, CAS install, Houdini manifest emission, stale sweep.
    let storage_manager = Arc::new(
        StorageManager::new(config.storage.clone())
            .context("Failed to initialize storage manager")?,
    );
    let project_manager =
        ProjectManager::new(project_dir, storage_manager, Arc::new(config.clone()))?;
    let outcomes = project_manager
        .sync_dependencies()
        .await
        .context("Failed to sync project dependencies")?;

    // Generate (or refuse to update) the lockfile from sync outcomes.
    let new_lock = project_manager
        .lock_file(&manifest, &outcomes, existing_lock.as_ref())
        .await
        .context("Failed to build the lock file")?;
    if frozen_lockfile {
        // --frozen-lockfile: the new lockfile must equal the existing one.
        // has_changes() compares dependencies + python_dependencies only,
        // ignoring metadata.generated_at which always differs.
        if let Some(ref existing) = existing_lock
            && new_lock.has_changes(existing)
        {
            bail!(
                "--frozen-lockfile is set but the resolved dependency set differs from hpm.lock. \
                     Re-run without --frozen-lockfile to regenerate it."
            );
        }
        info!("Skipping lock file update (--frozen-lockfile)");
    } else {
        new_lock
            .save(&lock_path)
            .with_context(|| format!("Failed to write lock file: {}", lock_path.display()))?;
        info!("Lock file generated: {}", lock_path.display());
    }

    info!("Dependency installation completed successfully");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::test_fixtures::{CwdGuard, TestManifestOpts, write_test_manifest};
    use tempfile::TempDir;

    #[test]
    fn test_load_manifest_valid() {
        let temp_dir = TempDir::new().unwrap();
        write_test_manifest(
            temp_dir.path(),
            TestManifestOpts {
                include_deps: true,
                include_python_deps: true,
                ..Default::default()
            },
        )
        .unwrap();

        let manifest_path = temp_dir.path().join("hpm.toml");
        let manifest = load_manifest(&manifest_path).expect("manifest should load");
        assert_eq!(manifest.package.name, "test-package");
        assert_eq!(manifest.package.version, "1.0.0");
        assert!(!manifest.dependencies.is_empty());
        assert!(!manifest.python_dependencies.is_empty());
    }

    #[test]
    fn test_load_manifest_invalid_toml() {
        let temp_dir = TempDir::new().unwrap();
        let manifest_path = temp_dir.path().join("hpm.toml");
        std::fs::write(&manifest_path, "invalid toml content [[[").unwrap();
        let err = load_manifest(&manifest_path).unwrap_err();
        assert!(
            err.to_string().contains("failed to parse manifest at"),
            "expected ManifestLoadError::Parse text, got: {err}"
        );
    }

    #[tokio::test]
    async fn test_install_dependencies_basic_manifest() {
        // Empty-deps manifest: install should set up the .hpm dir and
        // generate the lockfile without making any network requests.
        let temp_dir = TempDir::new().unwrap();
        let manifest_content = r#"[package]
path = "studio/test-install-package"
name = "test-install-package"
version = "1.0.0"
description = "Test package for install command"
authors = ["Test Author <test@example.com>"]
license = "MIT"

[compat]
houdini = ">=20.5"
"#;
        std::fs::write(temp_dir.path().join("hpm.toml"), manifest_content).unwrap();
        let _cwd = CwdGuard::enter(temp_dir.path());

        let config = Config::default();
        install_dependencies(&config, None, false, &mut Console::new())
            .await
            .unwrap();

        assert!(temp_dir.path().join(".hpm/packages").exists());
        let lock_path = temp_dir.path().join("hpm.lock");
        assert!(lock_path.exists());
        let lock_content = std::fs::read_to_string(&lock_path).unwrap();
        assert!(lock_content.contains("test-install-package"));
        assert!(lock_content.contains("1.0.0"));
    }

    #[tokio::test]
    async fn test_install_dependencies_explicit_manifest_path() {
        let temp_dir = TempDir::new().unwrap();
        let manifest_path = temp_dir.path().join("custom-manifest.toml");
        let manifest_content = r#"[package]
path = "studio/custom-path-package"
name = "custom-path-package"
version = "2.0.0"
description = "Test custom manifest path"
"#;
        std::fs::write(&manifest_path, manifest_content).unwrap();

        let config = Config::default();
        install_dependencies(&config, Some(manifest_path), false, &mut Console::new())
            .await
            .unwrap();

        assert!(temp_dir.path().join(".hpm").exists());
        assert!(temp_dir.path().join("hpm.lock").exists());
    }

    #[test]
    fn test_install_dependencies_nonexistent_manifest() {
        let temp_dir = TempDir::new().unwrap();
        let nonexistent = temp_dir.path().join("nonexistent.toml");
        let rt = tokio::runtime::Runtime::new().unwrap();
        let config = Config::default();
        let err = rt
            .block_on(install_dependencies(
                &config,
                Some(nonexistent),
                false,
                &mut Console::new(),
            ))
            .unwrap_err();
        assert!(err.to_string().contains("does not exist"));
    }
}
