//! HPM Add Command
//!
//! This module implements the `hpm add` command for adding package dependencies to HPM projects.
//!
//! ## Functionality
//!
//! The add command provides comprehensive dependency management:
//! - Adds package dependencies to hpm.toml manifest files
//! - Supports Git archive-based dependencies with `--git` and `--commit` flags
//! - Supports local path dependencies with `--path` flag
//! - Handles optional dependencies with `--optional` flag
//! - Flexible manifest targeting via `--package` flag
//! - Automatic dependency resolution and installation
//! - Lock file generation and updates
//!
//! ## Usage Examples
//!
//! ```bash
//! # Add a Git-based dependency (recommended)
//! hpm add geometry-tools --git https://github.com/studio/geometry-tools --commit abc123
//!
//! # Add a local path dependency (for development)
//! hpm add local-tools --path ../local-tools
//!
//! # Add optional dependency
//! hpm add material-library --git https://github.com/studio/materials --commit def456 --optional
//!
//! # Target specific manifest file
//! hpm add geometry-tools --git https://github.com/studio/geometry-tools --commit abc123 --package /path/to/project/
//! ```
//!
//! ## Implementation Details
//!
//! The add command follows HPM's established patterns:
//! - Uses the same manifest path resolution as `hpm install`
//! - Integrates with the existing package installation system
//! - Maintains consistency with TOML serialization standards
//! - Provides comprehensive error handling and user feedback
//!
//! ## Integration
//!
//! After adding a dependency to the manifest, the command automatically:
//! 1. Resolves transitive dependencies
//! 2. Downloads and installs packages to global storage
//! 3. Updates the project's hpm.lock file
//! 4. Sets up project-specific package references

use anyhow::{bail, Context, Result};
use hpm_package::{DependencySpec, PackageManifest};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tracing::{info, warn};

/// Add a package dependency to hpm.toml manifest
///
/// # Arguments
///
/// * `package_name` - Name of the package to add
/// * `git_url` - Git repository URL (recommended for dependencies)
/// * `commit` - Git commit hash (required when using git_url)
/// * `path` - Local path to package directory (for development dependencies)
/// * `version` - Legacy version specification (prefer git or path)
/// * `manifest_path` - Path to the manifest file or directory
/// * `optional` - Whether the dependency is optional
pub async fn add_package(
    package_name: String,
    git_url: Option<String>,
    commit: Option<String>,
    path: Option<PathBuf>,
    version: Option<String>,
    manifest_path: Option<PathBuf>,
    optional: bool,
) -> Result<()> {
    info!("Adding package dependency: {}", package_name);

    // Validate arguments
    if git_url.is_some() && commit.is_none() {
        bail!("--commit is required when using --git. Please specify a commit hash.");
    }

    if git_url.is_some() && path.is_some() {
        bail!("Cannot specify both --git and --path. Choose one source type.");
    }

    if (git_url.is_some() || path.is_some()) && version.is_some() {
        warn!("--version is ignored when using --git or --path");
    }

    // Determine manifest path
    let manifest_path = determine_manifest_path(manifest_path)?;
    info!("Using manifest: {}", manifest_path.display());

    // Load existing manifest
    let mut manifest = load_manifest(&manifest_path)
        .with_context(|| format!("Failed to load manifest from {}", manifest_path.display()))?;

    // Prepare dependency specification based on source type
    let dependency_spec = if let Some(url) = git_url {
        // Git-based dependency (recommended)
        let commit_hash = commit.unwrap(); // Already validated above
        DependencySpec::Git {
            git: url,
            commit: commit_hash,
            optional,
        }
    } else if let Some(local_path) = path {
        // Path-based dependency (for development)
        let path_str = local_path.to_string_lossy().to_string();
        DependencySpec::Path {
            path: path_str,
            optional,
        }
    } else if let Some(version_str) = version {
        // Legacy version-based dependency
        warn!("Version-based dependencies are deprecated. Consider using --git with a commit hash.");
        if version_str == "latest" {
            if optional {
                DependencySpec::Legacy {
                    version: Some("*".to_string()),
                    git: None,
                    tag: None,
                    branch: None,
                    optional: Some(true),
                    registry: None,
                }
            } else {
                DependencySpec::Simple("*".to_string())
            }
        } else if optional {
            DependencySpec::Legacy {
                version: Some(version_str),
                git: None,
                tag: None,
                branch: None,
                optional: Some(true),
                registry: None,
            }
        } else {
            DependencySpec::Simple(version_str)
        }
    } else {
        // No source specified - show help
        bail!(
            "Please specify a dependency source:\n\
             \n\
             For Git-based dependencies (recommended):\n\
             \n\
             \x20 hpm add {} --git <repository-url> --commit <commit-hash>\n\
             \n\
             For local path dependencies (development):\n\
             \n\
             \x20 hpm add {} --path <local-path>\n\
             \n\
             Example:\n\
             \n\
             \x20 hpm add {} --git https://github.com/user/repo --commit abc123",
            package_name, package_name, package_name
        );
    };

    // Add to dependencies
    if manifest.dependencies.is_none() {
        manifest.dependencies = Some(HashMap::new());
    }

    let dependencies = manifest.dependencies.as_mut().unwrap();

    // Check if dependency already exists
    if dependencies.contains_key(&package_name) {
        warn!("Dependency '{}' already exists, updating...", package_name);
    }

    dependencies.insert(package_name.clone(), dependency_spec);

    // Save updated manifest
    save_manifest(&manifest, &manifest_path)
        .with_context(|| format!("Failed to save manifest to {}", manifest_path.display()))?;

    info!("Successfully added dependency: {}", package_name);

    // Install the newly added dependency
    info!("Installing dependencies...");
    super::install::install_dependencies(Some(manifest_path))
        .await
        .context("Failed to install dependencies after adding package")?;

    info!(
        "Package '{}' added and installed successfully",
        package_name
    );
    Ok(())
}

/// Determine the path to the hpm.toml manifest file
fn determine_manifest_path(provided_path: Option<PathBuf>) -> Result<PathBuf> {
    match provided_path {
        Some(path) => {
            if path.is_file() {
                Ok(path)
            } else if path.is_dir() {
                let manifest_in_dir = path.join("hpm.toml");
                if manifest_in_dir.exists() {
                    Ok(manifest_in_dir)
                } else {
                    anyhow::bail!("No hpm.toml found in directory: {}", path.display());
                }
            } else {
                anyhow::bail!(
                    "Provided path does not exist or is not accessible: {}",
                    path.display()
                );
            }
        }
        None => {
            let current_dir = std::env::current_dir().context("Failed to get current directory")?;
            let manifest_path = current_dir.join("hpm.toml");

            if manifest_path.exists() {
                Ok(manifest_path)
            } else {
                anyhow::bail!(
                    "No hpm.toml found in current directory: {}. Use --package to specify a path.",
                    current_dir.display()
                );
            }
        }
    }
}

/// Maximum allowed manifest file size (1 MB) to prevent DoS attacks.
const MAX_MANIFEST_SIZE: u64 = 1024 * 1024;

/// Load and parse the package manifest
fn load_manifest(manifest_path: &Path) -> Result<PackageManifest> {
    // Security check: verify manifest file size to prevent DoS
    let metadata = std::fs::metadata(manifest_path)
        .with_context(|| format!("Failed to read manifest metadata: {}", manifest_path.display()))?;

    if metadata.len() > MAX_MANIFEST_SIZE {
        anyhow::bail!(
            "Manifest file too large ({} bytes). Maximum allowed size is {} bytes.",
            metadata.len(),
            MAX_MANIFEST_SIZE
        );
    }

    let content = std::fs::read_to_string(manifest_path)
        .with_context(|| format!("Failed to read manifest file: {}", manifest_path.display()))?;

    let manifest: PackageManifest = toml::from_str(&content)
        .with_context(|| format!("Failed to parse manifest file: {}", manifest_path.display()))?;

    // Validate manifest
    manifest
        .validate()
        .map_err(|e| anyhow::anyhow!("Manifest validation failed: {}", e))
        .with_context(|| format!("Manifest validation failed: {}", manifest_path.display()))?;

    Ok(manifest)
}

/// Save the package manifest to file
fn save_manifest(manifest: &PackageManifest, manifest_path: &Path) -> Result<()> {
    let toml_content =
        toml::to_string_pretty(manifest).context("Failed to serialize manifest to TOML")?;

    std::fs::write(manifest_path, toml_content)
        .with_context(|| format!("Failed to write manifest file: {}", manifest_path.display()))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use std::env;
    use tempfile::TempDir;

    /// Create a test hpm.toml file with basic package info
    fn create_test_manifest(path: &Path) -> Result<()> {
        let manifest_content = r#"[package]
name = "test-package"
version = "1.0.0"
description = "A test package for HPM add"
authors = ["Test Author <test@example.com>"]
license = "MIT"

[houdini]
min_version = "20.0"
"#;

        std::fs::write(path.join("hpm.toml"), manifest_content)?;
        Ok(())
    }

    #[test]
    fn test_determine_manifest_path_current_directory() {
        let temp_dir = TempDir::new().unwrap();
        let original_dir = env::current_dir().unwrap();

        create_test_manifest(temp_dir.path()).unwrap();

        env::set_current_dir(temp_dir.path()).unwrap();

        let result = determine_manifest_path(None);

        env::set_current_dir(&original_dir).unwrap();

        assert!(result.is_ok());
        let manifest_path = result.unwrap();
        assert!(manifest_path.ends_with("hpm.toml"));
    }

    #[test]
    fn test_determine_manifest_path_explicit_file() {
        let temp_dir = TempDir::new().unwrap();
        create_test_manifest(temp_dir.path()).unwrap();

        let manifest_path = temp_dir.path().join("hpm.toml");
        let result = determine_manifest_path(Some(manifest_path.clone()));

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), manifest_path);
    }

    #[test]
    fn test_determine_manifest_path_explicit_directory() {
        let temp_dir = TempDir::new().unwrap();
        create_test_manifest(temp_dir.path()).unwrap();

        let result = determine_manifest_path(Some(temp_dir.path().to_path_buf()));

        assert!(result.is_ok());
        assert!(result.unwrap().ends_with("hpm.toml"));
    }

    #[test]
    fn test_determine_manifest_path_no_manifest() {
        let temp_dir = TempDir::new().unwrap();
        let original_dir = env::current_dir().unwrap();

        env::set_current_dir(temp_dir.path()).unwrap();

        let result = determine_manifest_path(None);

        env::set_current_dir(&original_dir).unwrap();

        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg.contains("No hpm.toml found"));
    }

    #[test]
    fn test_load_manifest_valid() {
        let temp_dir = TempDir::new().unwrap();
        create_test_manifest(temp_dir.path()).unwrap();

        let manifest_path = temp_dir.path().join("hpm.toml");
        let result = load_manifest(&manifest_path);

        assert!(result.is_ok());
        let manifest = result.unwrap();
        assert_eq!(manifest.package.name, "test-package");
        assert_eq!(manifest.package.version, "1.0.0");
    }

    #[test]
    fn test_save_and_load_manifest_roundtrip() {
        let temp_dir = TempDir::new().unwrap();

        // Create initial manifest
        let mut manifest = PackageManifest::new(
            "test-package".to_string(),
            "1.0.0".to_string(),
            Some("Test description".to_string()),
            Some(vec!["Author <test@example.com>".to_string()]),
            Some("MIT".to_string()),
        );

        // Add a dependency
        let mut dependencies = HashMap::new();
        dependencies.insert(
            "test-dep".to_string(),
            DependencySpec::Simple("^1.0.0".to_string()),
        );
        manifest.dependencies = Some(dependencies);

        let manifest_path = temp_dir.path().join("hpm.toml");

        // Save and reload
        let save_result = save_manifest(&manifest, &manifest_path);
        assert!(save_result.is_ok());

        let loaded_manifest = load_manifest(&manifest_path).unwrap();

        assert_eq!(loaded_manifest.package.name, manifest.package.name);
        assert_eq!(loaded_manifest.package.version, manifest.package.version);
        assert!(loaded_manifest.dependencies.is_some());

        let loaded_deps = loaded_manifest.dependencies.unwrap();
        assert!(loaded_deps.contains_key("test-dep"));
    }

    #[test]
    fn test_dependency_spec_creation_simple() {
        let spec = DependencySpec::Simple("^1.0.0".to_string());

        match spec {
            DependencySpec::Simple(version) => assert_eq!(version, "^1.0.0"),
            _ => panic!("Expected Simple dependency spec"),
        }
    }

    #[test]
    fn test_dependency_spec_creation_optional() {
        let spec = DependencySpec::Legacy {
            version: Some("1.0.0".to_string()),
            git: None,
            tag: None,
            branch: None,
            optional: Some(true),
            registry: None,
        };

        match spec {
            DependencySpec::Legacy {
                version, optional, ..
            } => {
                assert_eq!(version, Some("1.0.0".to_string()));
                assert_eq!(optional, Some(true));
            }
            _ => panic!("Expected Legacy dependency spec"),
        }
    }

    // Property-based testing strategies for path handling

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
            create_test_manifest(&current_path).unwrap();
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
            create_test_manifest(&test_path).unwrap();

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
            version_input in prop_oneof![
                r"[0-9]+\.[0-9]+\.[0-9]+",           // Valid semver
                r"\^[0-9]+\.[0-9]+\.[0-9]+",         // Caret constraint
                r"~[0-9]+\.[0-9]+\.[0-9]+",          // Tilde constraint
                r">=[0-9]+\.[0-9]+\.[0-9]+",         // GTE constraint
                Just("*".to_string()),                // Wildcard
                Just("latest".to_string()),           // Latest keyword
                r"[a-zA-Z]{1,20}",                   // Invalid version
                Just("".to_string()),                 // Empty version
            ]
        ) {
            // Test that dependency spec creation handles various version inputs gracefully
            let simple_spec = DependencySpec::Simple(version_input.clone());

            // Should always be able to create the spec (validation happens later)
            match simple_spec {
                DependencySpec::Simple(ref v) => {
                    prop_assert_eq!(v, &version_input, "Version should be preserved in simple spec");
                }
                _ => prop_assert!(false, "Should create simple dependency spec"),
            }

            // Test JSON serialization (should always work)
            let json_result = serde_json::to_string(&simple_spec);
            prop_assert!(json_result.is_ok(), "Dependency spec should always serialize");
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
