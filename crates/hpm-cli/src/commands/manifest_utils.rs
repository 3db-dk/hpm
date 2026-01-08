//! Shared manifest utilities for HPM commands
//!
//! This module provides common functions for working with HPM manifest files (hpm.toml)
//! that are used across multiple CLI commands. Centralizing these functions reduces
//! code duplication and ensures consistent behavior across commands.
//!
//! ## Functions
//!
//! - [`determine_manifest_path`]: Resolves the path to the hpm.toml manifest file
//! - [`load_manifest`]: Loads and validates a package manifest from disk
//! - [`save_manifest`]: Serializes and writes a package manifest to disk
//!
//! ## Security
//!
//! The [`load_manifest`] function includes a size limit check to prevent DoS attacks
//! from maliciously large manifest files.

use anyhow::{Context, Result};
use hpm_package::PackageManifest;
use std::path::{Path, PathBuf};

/// Maximum allowed manifest file size (1 MB) to prevent DoS attacks.
pub const MAX_MANIFEST_SIZE: u64 = 1024 * 1024;

/// Determine the path to the hpm.toml manifest file.
///
/// This function resolves the manifest path based on the provided input:
/// - If a file path is provided, it uses that directly
/// - If a directory path is provided, it looks for hpm.toml in that directory
/// - If no path is provided, it looks for hpm.toml in the current directory
///
/// # Arguments
///
/// * `provided_path` - Optional path to the manifest file or containing directory
///
/// # Returns
///
/// The resolved path to the hpm.toml file, or an error if not found.
///
/// # Examples
///
/// ```ignore
/// // Look in current directory
/// let path = determine_manifest_path(None)?;
///
/// // Use explicit file path
/// let path = determine_manifest_path(Some(PathBuf::from("/project/hpm.toml")))?;
///
/// // Look in specific directory
/// let path = determine_manifest_path(Some(PathBuf::from("/project")))?;
/// ```
pub fn determine_manifest_path(provided_path: Option<PathBuf>) -> Result<PathBuf> {
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

/// Load and parse a package manifest from disk.
///
/// This function reads the manifest file, validates its size for security,
/// parses it as TOML, and validates the resulting manifest structure.
///
/// # Arguments
///
/// * `manifest_path` - Path to the hpm.toml file
///
/// # Returns
///
/// The parsed and validated [`PackageManifest`], or an error if loading fails.
///
/// # Security
///
/// This function includes a size check to prevent denial-of-service attacks
/// from extremely large manifest files. The maximum allowed size is 1 MB.
///
/// # Examples
///
/// ```ignore
/// let manifest = load_manifest(Path::new("/project/hpm.toml"))?;
/// println!("Package: {} v{}", manifest.package.name, manifest.package.version);
/// ```
pub fn load_manifest(manifest_path: &Path) -> Result<PackageManifest> {
    // Security check: verify manifest file size to prevent DoS
    let metadata = std::fs::metadata(manifest_path).with_context(|| {
        format!(
            "Failed to read manifest metadata: {}",
            manifest_path.display()
        )
    })?;

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

/// Save a package manifest to disk.
///
/// This function serializes the manifest to TOML format and writes it to the
/// specified path, overwriting any existing file.
///
/// # Arguments
///
/// * `manifest` - The manifest to save
/// * `manifest_path` - Path where the manifest should be written
///
/// # Returns
///
/// `Ok(())` on success, or an error if serialization or writing fails.
///
/// # Examples
///
/// ```ignore
/// let manifest = PackageManifest::new(...);
/// save_manifest(&manifest, Path::new("/project/hpm.toml"))?;
/// ```
pub fn save_manifest(manifest: &PackageManifest, manifest_path: &Path) -> Result<()> {
    let toml_content =
        toml::to_string_pretty(manifest).context("Failed to serialize manifest to TOML")?;

    std::fs::write(manifest_path, toml_content)
        .with_context(|| format!("Failed to write manifest file: {}", manifest_path.display()))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use hpm_package::DependencySpec;
    use std::collections::HashMap;
    use std::env;
    use tempfile::TempDir;

    /// Create a test hpm.toml file with basic package info
    fn create_test_manifest(path: &Path) -> Result<()> {
        let manifest_content = r#"[package]
name = "test-package"
version = "1.0.0"
description = "A test package"
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
    fn test_load_manifest_nonexistent() {
        let temp_dir = TempDir::new().unwrap();
        let manifest_path = temp_dir.path().join("nonexistent.toml");

        let result = load_manifest(&manifest_path);

        assert!(result.is_err());
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
    fn test_max_manifest_size_constant() {
        // Verify the constant is 1 MB
        assert_eq!(MAX_MANIFEST_SIZE, 1024 * 1024);
    }
}
