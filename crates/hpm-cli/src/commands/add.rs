//! HPM Add Command
//!
//! This module implements the `hpm add` command for adding package dependencies to HPM projects.
//!
//! ## Functionality
//!
//! The add command provides comprehensive dependency management:
//! - Adds package dependencies to hpm.toml manifest files
//! - Resolves packages through configured registries
//! - Supports local path dependencies with `--path` flag
//! - Handles optional dependencies with `--optional` flag
//! - Flexible manifest targeting via `--package` flag
//! - Automatic dependency resolution and installation
//! - Lock file generation and updates
//!
//! ## Usage Examples
//!
//! ```bash
//! # Add a package from the registry (latest version)
//! hpm add geometry-tools
//!
//! # Add a specific version
//! hpm add geometry-tools@1.0.0
//!
//! # Add a local path dependency (for development)
//! hpm add local-tools --path ../local-tools
//!
//! # Add optional dependency
//! hpm add material-library --optional
//!
//! # Target specific manifest file
//! hpm add geometry-tools --package /path/to/project/
//! ```

use super::manifest_utils::{determine_manifest_path, load_manifest, save_manifest};
use super::registry::build_registry_set;
use anyhow::{bail, Context, Result};
use hpm_config::Config;
use hpm_package::DependencySpec;
use indexmap::IndexMap;
use std::path::PathBuf;
use tracing::{info, warn};

/// Parse `name@version` syntax. Returns (name, optional_version).
fn parse_name_version(input: &str) -> (String, Option<String>) {
    if let Some(at_pos) = input.rfind('@') {
        let name = &input[..at_pos];
        let version = &input[at_pos + 1..];
        if !name.is_empty() && !version.is_empty() {
            return (name.to_string(), Some(version.to_string()));
        }
    }
    (input.to_string(), None)
}

/// Add package dependencies to hpm.toml manifest
///
/// # Arguments
///
/// * `package_names` - Names of the packages to add
/// * `path` - Local path to package directory (for development dependencies, single package only)
/// * `manifest_path` - Path to the manifest file or directory
/// * `optional` - Whether the dependencies are optional
pub async fn add_packages(
    package_names: Vec<String>,
    path: Option<PathBuf>,
    manifest_path: Option<PathBuf>,
    optional: bool,
) -> Result<()> {
    // Validate at least one package specified
    if package_names.is_empty() {
        bail!("Please specify at least one package name");
    }

    info!("Adding package dependencies: {:?}", package_names);

    // Disallow --path with multiple packages
    if path.is_some() && package_names.len() > 1 {
        bail!("Cannot use --path with multiple packages. Add path dependencies one at a time.");
    }

    // Determine manifest path
    let manifest_path = determine_manifest_path(manifest_path)?;
    info!("Using manifest: {}", manifest_path.display());

    // Load existing manifest
    let mut manifest = load_manifest(&manifest_path)
        .with_context(|| format!("Failed to load manifest from {}", manifest_path.display()))?;

    // Add to dependencies
    if manifest.dependencies.is_none() {
        manifest.dependencies = Some(IndexMap::new());
    }

    let dependencies = manifest.dependencies.as_mut().unwrap();

    // Add each package
    for package_name in &package_names {
        let dependency_spec = if let Some(ref local_path) = path {
            // Path-based dependency (for development)
            let path_str = local_path.to_string_lossy().to_string();
            DependencySpec::Path {
                path: path_str,
                optional,
            }
        } else {
            // Resolve from registries
            // Parse name@version syntax
            let (pkg_name, requested_version) = parse_name_version(package_name);

            let config = Config::load().unwrap_or_default();
            let registry_set = build_registry_set(&config);

            if registry_set.is_empty() {
                let example_pkg = package_names.first().unwrap();
                bail!(
                    "No registries configured and no source specified.\n\
                     \n\
                     Add a registry first:\n\
                     \n\
                     \x20 hpm registry add <url> --name <alias>\n\
                     \n\
                     Or use a local path:\n\
                     \n\
                     \x20 hpm add {} --path <local-path>",
                    example_pkg
                );
            }

            // Resolve from registry
            let entry = if let Some(ver) = &requested_version {
                registry_set
                    .get_version(&pkg_name, ver)
                    .await
                    .map_err(|e| anyhow::anyhow!("Failed to resolve {}@{}: {}", pkg_name, ver, e))?
            } else {
                // Get latest non-yanked version
                let versions = registry_set
                    .get_versions(&pkg_name)
                    .await
                    .map_err(|e| anyhow::anyhow!("Failed to resolve {}: {}", pkg_name, e))?;

                versions
                    .into_iter()
                    .rev()
                    .find(|e| !e.yanked)
                    .ok_or_else(|| {
                        anyhow::anyhow!("No non-yanked versions found for '{}'", pkg_name)
                    })?
            };

            info!(
                "Resolved {} -> {} (dl: {})",
                pkg_name, entry.version, entry.dl
            );

            DependencySpec::Url {
                url: entry.dl.clone(),
                version: entry.version.clone(),
                optional,
            }
        };

        // Check if dependency already exists
        if dependencies.contains_key(package_name) {
            warn!("Dependency '{}' already exists, updating...", package_name);
        }

        dependencies.insert(package_name.clone(), dependency_spec);
        info!("Added dependency: {}", package_name);
    }

    // Save updated manifest
    save_manifest(&manifest, &manifest_path)
        .with_context(|| format!("Failed to save manifest to {}", manifest_path.display()))?;

    info!("Successfully added {} dependencies", package_names.len());

    // Install the newly added dependencies
    info!("Installing dependencies...");
    super::install::install_dependencies(Some(manifest_path), false)
        .await
        .context("Failed to install dependencies after adding packages")?;

    info!(
        "{} package(s) added and installed successfully",
        package_names.len()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::test_fixtures::{write_test_manifest, TestManifestOpts};
    use hpm_package::PackageManifest;
    use proptest::prelude::*;
    use tempfile::TempDir;

    /// Strategy to generate valid file path components
    fn path_component_strategy() -> impl Strategy<Value = String> {
        prop_oneof![
            r"[a-zA-Z][a-zA-Z0-9_-]{1,20}",
            Just("src".to_string()),
            Just("test".to_string()),
            Just("project".to_string()),
            Just("package".to_string()),
        ]
    }

    /// Strategy to generate problematic path components
    fn problematic_path_component_strategy() -> impl Strategy<Value = String> {
        prop_oneof![
            Just("".to_string()),        // Empty component
            Just(".".to_string()),       // Current directory
            Just("..".to_string()),      // Parent directory
            Just("...".to_string()),     // Multiple dots
            r"[\s]{1,10}",               // Whitespace only
            r"path[\s]+with[\s]+spaces", // Spaces in path
            r"[^a-zA-Z0-9_/-]{1,10}",    // Special characters
            Just("CON".to_string()),     // Windows reserved
            Just("PRN".to_string()),     // Windows reserved
            Just("AUX".to_string()),     // Windows reserved
            Just("NUL".to_string()),     // Windows reserved
            r"[a-zA-Z0-9_-]{100,200}",   // Extremely long
        ]
    }

    /// Strategy to generate file paths with various structures
    fn file_path_strategy() -> impl Strategy<Value = PathBuf> {
        prop::collection::vec(path_component_strategy(), 1..6).prop_map(|components| {
            let mut path = PathBuf::new();
            for component in components {
                path.push(component);
            }
            path
        })
    }

    /// Strategy to generate problematic file paths
    fn problematic_path_strategy() -> impl Strategy<Value = PathBuf> {
        prop_oneof![
            // Empty path
            Just(PathBuf::new()),
            // Paths with problematic components
            prop::collection::vec(problematic_path_component_strategy(), 1..4).prop_map(
                |components| {
                    let mut path = PathBuf::new();
                    for component in components {
                        path.push(component);
                    }
                    path
                }
            ),
            // Mixed valid/invalid components
            (
                path_component_strategy(),
                problematic_path_component_strategy(),
                path_component_strategy()
            )
                .prop_map(|(start, middle, end)| {
                    let mut path = PathBuf::new();
                    path.push(start);
                    path.push(middle);
                    path.push(end);
                    path
                }),
        ]
    }

    /// Strategy to generate package names that might cause issues
    fn edge_case_package_name_strategy() -> impl Strategy<Value = String> {
        prop_oneof![
            r"[a-z]{1,3}",                        // Very short names
            r"[a-z]{50,100}",                     // Very long names
            r"[a-z]+-+[a-z]+",                    // Multiple hyphens
            Just("a".to_string()),                // Single character
            Just("my-package".to_string()),       // Standard name
            Just("test-123-package".to_string()), // Numbers
            r"[a-z]+[0-9]+[a-z]+",                // Mixed alphanumeric
        ]
    }

    proptest! {
        /// Test that path resolution handles various path structures correctly
        #[test]
        fn prop_path_resolution_robustness(path_parts in prop::collection::vec(path_component_strategy(), 1..5)) {
            let temp_dir = TempDir::new().unwrap();

            // Build nested directory structure
            let mut current_path = temp_dir.path().to_path_buf();
            for part in &path_parts {
                current_path.push(part);
                std::fs::create_dir_all(&current_path).unwrap();
            }

            // Create manifest in the deepest directory
            write_test_manifest(&current_path, TestManifestOpts::default()).unwrap();
            let manifest_file = current_path.join("hpm.toml");

            // Test explicit file path resolution
            let result = determine_manifest_path(Some(manifest_file.clone()));
            prop_assert!(result.is_ok(), "Should resolve explicit file path");
            prop_assert_eq!(result.unwrap(), manifest_file);

            // Test directory path resolution
            let dir_result = determine_manifest_path(Some(current_path.clone()));
            prop_assert!(dir_result.is_ok(), "Should resolve directory containing hpm.toml");
            prop_assert!(dir_result.unwrap().ends_with("hpm.toml"));
        }

        /// Test that problematic paths are handled gracefully
        #[test]
        fn prop_problematic_path_handling(problematic_path in problematic_path_strategy()) {
            let _temp_dir = TempDir::new().unwrap();

            // Try to resolve problematic paths - should fail gracefully
            let result = determine_manifest_path(Some(problematic_path.clone()));

            // Should either succeed (if path accidentally exists) or fail with clear error
            if let Err(error) = result {
                let error_msg = error.to_string();
                prop_assert!(
                    !error_msg.is_empty(),
                    "Error message should not be empty for path: {:?}", problematic_path
                );

                prop_assert!(
                    error_msg.contains("exist") ||
                    error_msg.contains("accessible") ||
                    error_msg.contains("found") ||
                    error_msg.contains("directory") ||
                    error_msg.contains("file"),
                    "Error message should be descriptive for path: {:?}", problematic_path
                );
            }
        }

        /// Test manifest loading with various path structures
        #[test]
        fn prop_manifest_loading_path_independence(
            path_structure in file_path_strategy(),
            package_name in edge_case_package_name_strategy()
        ) {
            let temp_dir = TempDir::new().unwrap();
            let mut test_path = temp_dir.path().to_path_buf();

            // Create nested directory structure
            for component in path_structure.components() {
                if let std::path::Component::Normal(os_str) = component {
                    test_path.push(os_str);
                    let _ = std::fs::create_dir_all(&test_path);
                }
            }

            // Create a valid manifest
            let manifest = PackageManifest::new(
                package_name.clone(),
                "1.0.0".to_string(),
                Some("Test package".to_string()),
                Some(vec!["Author <test@example.com>".to_string()]),
                Some("MIT".to_string()),
            );

            let manifest_path = test_path.join("hpm.toml");

            // Save manifest
            let save_result = save_manifest(&manifest, &manifest_path);
            prop_assert!(save_result.is_ok(), "Should save manifest at any valid path");

            // Load manifest
            let load_result = load_manifest(&manifest_path);
            prop_assert!(load_result.is_ok(), "Should load manifest from any valid path");

            if let Ok(loaded) = load_result {
                prop_assert_eq!(loaded.package.name, package_name);
                prop_assert_eq!(loaded.package.version, "1.0.0");
            }
        }

        /// Test that manifest path determination is consistent
        #[test]
        fn prop_path_determination_consistency(path_parts in prop::collection::vec(path_component_strategy(), 1..4)) {
            let temp_dir = TempDir::new().unwrap();
            let mut test_path = temp_dir.path().to_path_buf();

            for part in path_parts {
                test_path.push(part);
            }
            std::fs::create_dir_all(&test_path).unwrap();
            write_test_manifest(&test_path, TestManifestOpts::default()).unwrap();

            // Multiple calls should give consistent results
            let result1 = determine_manifest_path(Some(test_path.clone()));
            let result2 = determine_manifest_path(Some(test_path.clone()));
            let result3 = determine_manifest_path(Some(test_path.clone()));

            prop_assert_eq!(result1.is_ok(), result2.is_ok(), "Results should be consistent");
            prop_assert_eq!(result2.is_ok(), result3.is_ok(), "Results should be consistent");

            if let (Ok(path1), Ok(path2), Ok(path3)) = (result1, result2, result3) {
                prop_assert_eq!(path1, path2.clone(), "Resolved paths should be identical");
                prop_assert_eq!(path2, path3, "Resolved paths should be identical");
            }
        }

        /// Test dependency spec creation with various inputs
        #[test]
        fn prop_dependency_spec_creation_robustness(
            url in prop_oneof![
                Just("https://example.com/packages/test/1.0.0/test-1.0.0.zip".to_string()),
                Just("https://api.3db.dk/v1/registry/packages/test/1.0.0/download".to_string()),
                r"https://[a-z]+\.[a-z]+/[a-z]+/[a-z]+",
            ],
            version in prop_oneof![
                Just("1.0.0".to_string()),
                Just("2.3.4".to_string()),
                r"[0-9]+\.[0-9]+\.[0-9]+",
            ],
            path in prop_oneof![
                Just("../local-package".to_string()),
                Just("./sibling-package".to_string()),
                r"\.\./[a-z-]+",
            ],
            optional in any::<bool>()
        ) {
            // Test URL dependency spec creation
            let url_spec = DependencySpec::Url {
                url: url.clone(),
                version: version.clone(),
                optional,
            };

            match &url_spec {
                DependencySpec::Url { url: u, version: ver, .. } => {
                    prop_assert_eq!(u, &url, "URL should be preserved");
                    prop_assert_eq!(ver, &version, "Version should be preserved");
                }
                _ => prop_assert!(false, "Should create URL dependency spec"),
            }

            // Test Path dependency spec creation
            let path_spec = DependencySpec::Path {
                path: path.clone(),
                optional,
            };

            match &path_spec {
                DependencySpec::Path { path: p, .. } => {
                    prop_assert_eq!(p, &path, "Path should be preserved");
                }
                _ => prop_assert!(false, "Should create Path dependency spec"),
            }

            // Test JSON serialization (should always work)
            let url_json = serde_json::to_string(&url_spec);
            prop_assert!(url_json.is_ok(), "URL dependency spec should always serialize");

            let path_json = serde_json::to_string(&path_spec);
            prop_assert!(path_json.is_ok(), "Path dependency spec should always serialize");
        }

        /// Test that manifest operations are atomic and don't leave partial files
        #[test]
        fn prop_manifest_operations_atomic(
            package_name in edge_case_package_name_strategy(),
            version in r"[0-9]+\.[0-9]+\.[0-9]+"
        ) {
            let temp_dir = TempDir::new().unwrap();
            let manifest_path = temp_dir.path().join("hpm.toml");

            let manifest = PackageManifest::new(
                package_name.clone(),
                version.clone(),
                Some("Test package".to_string()),
                None,
                None,
            );

            // Save manifest
            let save_result = save_manifest(&manifest, &manifest_path);

            if save_result.is_ok() {
                // File should exist and be complete
                prop_assert!(manifest_path.exists(), "Manifest file should exist after successful save");

                // Should be able to read back immediately
                let content = std::fs::read_to_string(&manifest_path);
                prop_assert!(content.is_ok(), "Should be able to read saved manifest");

                if let Ok(content_str) = content {
                    prop_assert!(!content_str.is_empty(), "Manifest content should not be empty");
                    prop_assert!(content_str.contains(&package_name), "Should contain package name");
                    prop_assert!(content_str.contains(&version), "Should contain version");
                }
            }
        }

        /// Test error handling consistency for file operations
        #[test]
        fn prop_file_operation_error_consistency(
            non_existent_path in problematic_path_strategy(),
            _package_name in edge_case_package_name_strategy()
        ) {
            // Test loading from non-existent paths
            let load_result = load_manifest(&non_existent_path.join("hpm.toml"));

            if let Err(error) = load_result {
                let error_msg = error.to_string();
                prop_assert!(
                    !error_msg.is_empty(),
                    "Error should have message for non-existent path: {:?}", non_existent_path
                );

                // Error should mention file operation failure
                prop_assert!(
                    error_msg.to_lowercase().contains("read") ||
                    error_msg.to_lowercase().contains("file") ||
                    error_msg.to_lowercase().contains("found") ||
                    error_msg.to_lowercase().contains("exist"),
                    "Error should be descriptive for file operation: {:?}", non_existent_path
                );
            }
        }
    }
}
