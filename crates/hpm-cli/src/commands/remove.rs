//! HPM Remove Command
//!
//! This module implements the `hpm remove` command for removing package dependencies from HPM projects.
//!
//! ## Functionality
//!
//! The remove command provides safe dependency removal:
//! - Removes package dependencies from hmp.toml manifest files
//! - Flexible manifest targeting via `--package` flag
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
//! hpm remove material-library --package /path/to/project/
//! hpm remove geometry-tools --package /path/to/project/hpm.toml
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

use super::manifest_utils::{determine_manifest_path, load_manifest, save_manifest};
use anyhow::{Context, Result};
use hpm_config::Config;
use std::path::PathBuf;
use tracing::info;

/// Remove a package dependency from hpm.toml manifest
pub async fn remove_package(
    config: &Config,
    package_name: String,
    manifest_path: Option<PathBuf>,
) -> Result<()> {
    info!("Removing package dependency: {}", package_name);

    // Determine manifest path
    let manifest_path = determine_manifest_path(manifest_path)?;
    info!("Using manifest: {}", manifest_path.display());

    // Load existing manifest
    let mut manifest = load_manifest(&manifest_path)
        .with_context(|| format!("Failed to load manifest from {}", manifest_path.display()))?;

    if manifest.dependencies.is_empty() {
        anyhow::bail!(
            "No dependencies found in manifest. Package '{}' is not a dependency.",
            package_name
        );
    }

    let dependencies = &mut manifest.dependencies;

    // Check if the dependency exists
    if !dependencies.contains_key(&package_name) {
        anyhow::bail!(
            "Package '{}' is not a dependency in this manifest.",
            package_name
        );
    }

    // Remove the dependency
    dependencies.shift_remove(&package_name);
    info!("Removed dependency: {}", package_name);

    // If dependencies is now empty, we could optionally remove the section
    if dependencies.is_empty() {
        info!("Dependencies section is now empty");
        // Keep the empty dependencies section for consistency
    }

    // Save updated manifest
    save_manifest(&manifest, &manifest_path)
        .with_context(|| format!("Failed to save manifest to {}", manifest_path.display()))?;

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

    info!("Package '{}' removed successfully", package_name);
    info!(
        "Note: Downloaded packages are not deleted. Run 'hpm clean' to remove orphaned packages."
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::test_fixtures::{CwdGuard, TestManifestOpts, write_test_manifest};
    use hpm_package::{DependencySpec, PackagePath};
    use indexmap::IndexMap;
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

    #[test]
    fn test_save_and_load_manifest_after_removal() {
        let temp_dir = TempDir::new().unwrap();

        // Create manifest with dependencies
        let mut manifest = hpm_package::PackageManifest::new(
            PackagePath::new("studio/test-package").unwrap(),
            "Test Package".to_string(),
            "1.0.0".to_string(),
            Some("Test description".to_string()),
            vec!["Author <test@example.com>".to_string()],
            Some("MIT".to_string()),
        );

        // Add dependencies
        let mut dependencies = IndexMap::new();
        dependencies.insert(
            "keep-me".to_string(),
            DependencySpec::Url {
                url: "https://example.com/packages/keep-me/1.0.0/keep-me-1.0.0.zip".to_string(),
                version: "1.0.0".to_string(),
                optional: false,
            },
        );
        dependencies.insert(
            "remove-me".to_string(),
            DependencySpec::Path {
                path: "../remove-me".to_string(),
                optional: false,
                link: false,
            },
        );
        manifest.dependencies = dependencies;

        let manifest_path = temp_dir.path().join("hpm.toml");

        // Save initial manifest
        save_manifest(&manifest, &manifest_path).unwrap();

        // Load, remove dependency, and save again
        let mut loaded_manifest = load_manifest(&manifest_path).unwrap();
        loaded_manifest.dependencies.shift_remove("remove-me");

        save_manifest(&loaded_manifest, &manifest_path).unwrap();

        // Load again and verify
        let final_manifest = load_manifest(&manifest_path).unwrap();
        let final_deps = final_manifest.dependencies;

        assert!(final_deps.contains_key("keep-me"));
        assert!(!final_deps.contains_key("remove-me"));
        assert_eq!(final_deps.len(), 1);
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
            "utility-nodes".to_string(),
            Some(manifest_path.clone()),
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
            "non-existent-package".to_string(),
            Some(manifest_path),
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
        let result = remove_package(&config, "some-package".to_string(), Some(manifest_path)).await;

        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg.contains("No dependencies found"));
    }

    #[test]
    fn test_remove_package_nonexistent_manifest() {
        let temp_dir = TempDir::new().unwrap();
        let _cwd = CwdGuard::enter(temp_dir.path());

        let rt = tokio::runtime::Runtime::new().unwrap();
        let config = Config::default();
        let result = rt.block_on(remove_package(&config, "some-package".to_string(), None));

        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg.contains("No hpm.toml found"));
    }
}
