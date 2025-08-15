//! HPM Add Command
//!
//! This module implements the `hpm add` command for adding package dependencies to HPM projects.
//!
//! ## Functionality
//!
//! The add command provides comprehensive dependency management:
//! - Adds package dependencies to hpm.toml manifest files
//! - Supports version specifications (semantic versioning patterns)
//! - Handles optional dependencies with `--optional` flag
//! - Flexible manifest targeting via `--package` flag
//! - Automatic dependency resolution and installation
//! - Lock file generation and updates
//!
//! ## Usage Examples
//!
//! ```bash
//! # Add latest version of a package
//! hpm add awesome-houdini-tools
//!
//! # Add specific version
//! hpm add utility-nodes --version "^2.1.0"
//!
//! # Add optional dependency
//! hpm add material-library --optional
//!
//! # Target specific manifest file
//! hpm add geometry-tools --package /path/to/project/
//! hpm add mesh-utilities --package /path/to/project/hpm.toml
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

use anyhow::{Context, Result};
use hpm_package::{DependencySpec, PackageManifest};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tracing::{info, warn};

/// Add a package dependency to hpm.toml manifest
pub async fn add_package(
    package_name: String,
    version: Option<String>,
    manifest_path: Option<PathBuf>,
    optional: bool,
) -> Result<()> {
    info!("Adding package dependency: {}", package_name);

    // Determine manifest path
    let manifest_path = determine_manifest_path(manifest_path)?;
    info!("Using manifest: {}", manifest_path.display());

    // Load existing manifest
    let mut manifest = load_manifest(&manifest_path)
        .with_context(|| format!("Failed to load manifest from {}", manifest_path.display()))?;

    // Prepare dependency specification
    let dependency_spec = if let Some(version_str) = version {
        if version_str == "latest" {
            DependencySpec::Simple("*".to_string())
        } else {
            DependencySpec::Simple(version_str)
        }
    } else {
        // Default to latest version if no version specified
        DependencySpec::Simple("*".to_string())
    };

    // Add optional flag if specified
    let dependency_spec = if optional {
        match dependency_spec {
            DependencySpec::Simple(version) => DependencySpec::Detailed {
                version: Some(version),
                git: None,
                tag: None,
                branch: None,
                optional: Some(true),
                registry: None,
            },
            detailed => detailed, // Already detailed, just update optional flag
        }
    } else {
        dependency_spec
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

/// Load and parse the package manifest
fn load_manifest(manifest_path: &Path) -> Result<PackageManifest> {
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
        let spec = DependencySpec::Detailed {
            version: Some("1.0.0".to_string()),
            git: None,
            tag: None,
            branch: None,
            optional: Some(true),
            registry: None,
        };

        match spec {
            DependencySpec::Detailed {
                version, optional, ..
            } => {
                assert_eq!(version, Some("1.0.0".to_string()));
                assert_eq!(optional, Some(true));
            }
            _ => panic!("Expected Detailed dependency spec"),
        }
    }
}
