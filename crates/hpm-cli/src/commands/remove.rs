//! HPM Remove Command
//!
//! This module implements the `hpm remove` command for removing package dependencies from HPM projects.
//!
//! ## Functionality
//!
//! The remove command provides safe dependency removal:
//! - Removes package dependencies from hpm.toml manifest files
//! - Flexible manifest targeting via `--manifest` flag
//! - Automatic lock file updates to maintain consistency
//! - Preserves downloaded packages for potential reuse by other projects
//! - Comprehensive validation and error handling
//!
//! ## Usage Examples
//!
//! ```bash
//! # Remove a dependency from current project
//! hpm remove utility-nodes
//!
//! # Remove dependency from specific project
//! hpm remove material-library --manifest /path/to/project/
//! hpm remove geometry-tools --manifest /path/to/project/hpm.toml
//! ```
//!
//! ## Design Philosophy
//!
//! The remove command follows a non-destructive approach:
//! - Dependencies are removed from the manifest but packages remain in global storage
//! - This allows other projects to continue using the same packages
//! - Use `hpm clean` to remove orphaned packages that are no longer needed
//! - Lock files are updated to reflect the new dependency state
//!
//! ## Implementation Details
//!
//! The remove command integrates seamlessly with HPM's architecture:
//! - Uses the same manifest path resolution patterns as other commands
//! - Validates dependency existence before attempting removal
//! - Provides clear error messages for missing dependencies or files
//! - Automatically updates lock files by triggering the install process
//!
//! ## Safety Guarantees
//!
//! The command ensures safe operation through:
//! - Validation of manifest file existence and format
//! - Confirmation that the target dependency exists before removal
//! - Graceful handling of edge cases (empty dependencies, missing sections)
//! - Comprehensive error reporting with actionable guidance

use super::manifest_utils::{determine_manifest_path, load_manifest};
use crate::console::Console;
use anyhow::{Context, Result};
use hpm_config::Config;
use hpm_core::project::manifest_edit;
use std::path::PathBuf;
use tracing::info;

/// Remove a package dependency from hpm.toml manifest
pub async fn remove_package(
    config: &Config,
    package_name: &str,
    manifest_path: Option<PathBuf>,
    console: &mut Console,
) -> Result<()> {
    info!("Removing package dependency: {}", package_name);

    // Determine manifest path
    let manifest_path = determine_manifest_path(manifest_path)?;
    info!("Using manifest: {}", manifest_path.display());

    // Validate the manifest before editing it. The edit below goes through
    // toml_edit (formatting-preserving), not a serde round-trip.
    load_manifest(&manifest_path)
        .with_context(|| format!("Failed to load manifest from {}", manifest_path.display()))?;

    let removed = manifest_edit::remove_dependency(&manifest_path, package_name)
        .with_context(|| format!("Failed to update manifest at {}", manifest_path.display()))?;
    if !removed {
        anyhow::bail!(
            "Package '{}' is not a dependency in this manifest.",
            package_name
        );
    }
    info!("Removed dependency: {}", package_name);

    // Update lock file by running install (which regenerates the lock
    // file). A failure here leaves hpm.toml and hpm.lock out of sync, so
    // it fails the command rather than being downgraded to a warning.
    info!("Updating lock file...");
    super::install::install_dependencies(config, Some(manifest_path), false)
        .await
        .context(
            "Dependency removed from hpm.toml, but regenerating the lock file failed; \
             hpm.toml and hpm.lock are now out of sync — run 'hpm install' to reconcile",
        )?;

    console.success(format!("Removed dependency '{}'", package_name));
    console.status(
        "Note: Downloaded packages are not deleted. Run 'hpm clean' to remove orphaned packages.",
    );

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
                ..Default::default()
            },
        )
        .unwrap();

        let manifest_path = temp_dir.path().join("hpm.toml");
        let result = load_manifest(&manifest_path);

        assert!(result.is_ok());
        let manifest = result.unwrap();
        assert_eq!(manifest.package.name, "test-package");
        assert_eq!(manifest.package.version, "1.0.0");
        assert!(!manifest.dependencies.is_empty());
        assert_eq!(manifest.dependencies.len(), 2);
    }

    #[tokio::test]
    async fn test_remove_package_success() {
        let temp_dir = TempDir::new().unwrap();

        write_test_manifest(
            temp_dir.path(),
            TestManifestOpts {
                include_deps: true,
                ..Default::default()
            },
        )
        .unwrap();

        let manifest_path = temp_dir.path().join("hpm.toml");

        let config = Config::default();
        let _result = remove_package(
            &config,
            "utility-nodes",
            Some(manifest_path.clone()),
            &mut Console::new(),
        )
        .await;

        let manifest = load_manifest(&manifest_path).unwrap();

        assert!(!manifest.dependencies.contains_key("utility-nodes"));
        assert!(manifest.dependencies.contains_key("material-library"));
    }

    #[tokio::test]
    async fn test_remove_package_not_found() {
        let temp_dir = TempDir::new().unwrap();

        write_test_manifest(
            temp_dir.path(),
            TestManifestOpts {
                include_deps: true,
                ..Default::default()
            },
        )
        .unwrap();

        let manifest_path = temp_dir.path().join("hpm.toml");

        let config = Config::default();
        let result = remove_package(
            &config,
            "non-existent-package",
            Some(manifest_path),
            &mut Console::new(),
        )
        .await;

        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg.contains("non-existent-package"));
        assert!(error_msg.contains("is not a dependency"));
    }

    #[tokio::test]
    async fn test_remove_package_no_dependencies_section() {
        let temp_dir = TempDir::new().unwrap();

        write_test_manifest(temp_dir.path(), TestManifestOpts::default()).unwrap();

        let manifest_path = temp_dir.path().join("hpm.toml");

        let config = Config::default();
        let result = remove_package(
            &config,
            "some-package",
            Some(manifest_path),
            &mut Console::new(),
        )
        .await;

        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg.contains("is not a dependency"));
    }

    #[test]
    fn test_remove_package_nonexistent_manifest() {
        let temp_dir = TempDir::new().unwrap();
        let _cwd = CwdGuard::enter(temp_dir.path());

        let rt = tokio::runtime::Runtime::new().unwrap();
        let config = Config::default();
        let result = rt.block_on(remove_package(
            &config,
            "some-package",
            None,
            &mut Console::new(),
        ));

        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg.contains("No hpm.toml found"));
    }
}
