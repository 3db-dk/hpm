//! HPM List Command
//!
//! This module implements the `hpm list` command for displaying comprehensive package information
//! and dependencies from HPM projects. This command serves as the primary way to view package
//! details and dependency information in HPM.
//!
//! ## Functionality
//!
//! The list command provides comprehensive package and dependency visibility:
//! - Displays package metadata (name, version, description, Houdini compatibility)
//! - Lists HPM package dependencies from hpm.toml manifest files
//! - Lists Python package dependencies from hpm.toml manifest files
//! - Supports flexible manifest targeting via `--package` flag
//! - Clear categorization between HPM and Python dependencies
//! - Version specifications displayed with dependency names
//!
//! ## Usage Examples
//!
//! ```bash
//! # List dependencies from current directory's hpm.toml
//! hpm list
//!
//! # List dependencies from specific manifest file
//! hpm list --package /path/to/project/hpm.toml
//!
//! # List dependencies from directory containing hpm.toml
//! hpm list --package /path/to/project/
//! ```
//!
//! ## Output Format
//!
//! The command displays information in organized sections:
//! - Package information (name, version, description, Houdini compatibility)
//! - HPM Dependencies section with version specs, git sources, optional markers
//! - Python Dependencies section with version specs, extras, optional markers
//! - Clear indication when no dependencies are found
//!
//! ## Example Output
//!
//! ```text
//! Package: geometry-tools v1.2.0
//! Description: Advanced geometry manipulation tools for Houdini
//! Houdini compatibility: min: 20.0, max: 21.0
//!
//! HPM Dependencies:
//!   utility-nodes ^2.1.0
//!   material-library 1.5 (optional)
//!   mesh-utils git: https://github.com/example/mesh-utils (tag: v1.0)
//!
//! Python Dependencies:
//!   numpy >=1.20.0
//!   matplotlib ^3.5.0 (optional)
//!   requests >=2.25.0 [security,socks]
//! ```
//!
//! ## Implementation Details
//!
//! The list command follows HPM's established patterns:
//! - Uses the same manifest path resolution as other commands
//! - Integrates with existing PackageManifest parsing
//! - Provides comprehensive error handling and user feedback
//! - Consistent with HPM's professional, concise output style

use anyhow::{Context, Result};
use hpm_package::{DependencySpec, PackageManifest, PythonDependencySpec};
use std::path::{Path, PathBuf};
use tracing::info;

/// Display comprehensive package information and dependencies from hpm.toml manifest
///
/// This is the primary command for viewing package details and dependency information in HPM.
/// It provides a complete overview of the package including metadata, HPM dependencies, and
/// Python dependencies.
///
/// # Arguments
///
/// * `manifest_path` - Optional path to hpm.toml file or directory containing one
///   - If `None`, searches for hpm.toml in current directory
///   - If `Some(path)` and path is a file, uses that file directly
///   - If `Some(path)` and path is a directory, looks for hpm.toml in that directory
///
/// # Returns
///
/// * `Ok(())` - Successfully displayed dependencies
/// * `Err(anyhow::Error)` - Failed to find, parse, or display manifest
///
/// # Examples
///
/// ```no_run
/// use std::path::PathBuf;
/// # async fn example() -> anyhow::Result<()> {
/// // List from current directory
/// list_dependencies(None).await?;
///
/// // List from specific manifest
/// list_dependencies(Some(PathBuf::from("/path/to/hpm.toml"))).await?;
/// # Ok(())
/// # }
/// ```
pub async fn list_dependencies(manifest_path: Option<PathBuf>) -> Result<()> {
    info!("Listing package dependencies");

    // Determine manifest path
    let manifest_path = determine_manifest_path(manifest_path)?;
    info!("Using manifest: {}", manifest_path.display());

    // Load and validate manifest
    let manifest = load_manifest(&manifest_path)
        .with_context(|| format!("Failed to load manifest from {}", manifest_path.display()))?;

    // Display package information
    display_package_info(&manifest);

    // Display HPM dependencies
    display_hpm_dependencies(&manifest);

    // Display Python dependencies
    display_python_dependencies(&manifest);

    Ok(())
}

/// Display package information header
///
/// Shows package name, version, description, and Houdini compatibility information.
/// This provides context for the dependencies that follow.
///
/// # Arguments
///
/// * `manifest` - The parsed package manifest containing package metadata
fn display_package_info(manifest: &PackageManifest) {
    println!(
        "Package: {} v{}",
        manifest.package.name, manifest.package.version
    );

    if let Some(description) = &manifest.package.description {
        println!("Description: {}", description);
    }

    if let Some(houdini_config) = &manifest.houdini {
        let mut houdini_info = Vec::new();

        if let Some(min_version) = &houdini_config.min_version {
            houdini_info.push(format!("min: {}", min_version));
        }

        if let Some(max_version) = &houdini_config.max_version {
            houdini_info.push(format!("max: {}", max_version));
        }

        if !houdini_info.is_empty() {
            println!("Houdini compatibility: {}", houdini_info.join(", "));
        }
    }

    println!(); // Empty line for readability
}

/// Display HPM package dependencies
///
/// Shows all HPM package dependencies with their version specifications.
/// Handles both simple version strings and detailed dependency specifications
/// including git sources, tags, branches, and optional flags.
///
/// # Arguments
///
/// * `manifest` - The parsed package manifest containing dependency information
fn display_hpm_dependencies(manifest: &PackageManifest) {
    println!("HPM Dependencies:");

    match &manifest.dependencies {
        Some(dependencies) if !dependencies.is_empty() => {
            for (name, spec) in dependencies {
                let version_info = format_dependency_spec(spec);
                let optional_marker = if is_optional_dependency(spec) {
                    " (optional)"
                } else {
                    ""
                };
                println!("  {} {}{}", name, version_info, optional_marker);
            }
        }
        _ => {
            println!("  (none)");
        }
    }

    println!(); // Empty line for readability
}

/// Display Python package dependencies
///
/// Shows all Python package dependencies with their version specifications.
/// Includes support for extras (e.g., requests\[security\]) and optional dependencies.
///
/// # Arguments
///
/// * `manifest` - The parsed package manifest containing Python dependency information
fn display_python_dependencies(manifest: &PackageManifest) {
    println!("Python Dependencies:");

    match &manifest.python_dependencies {
        Some(dependencies) if !dependencies.is_empty() => {
            for (name, spec) in dependencies {
                let version_info = format_python_dependency_spec(spec);
                let optional_marker = if is_optional_python_dependency(spec) {
                    " (optional)"
                } else {
                    ""
                };
                let extras_info = format_python_extras(spec);
                println!(
                    "  {} {}{}{}",
                    name, version_info, extras_info, optional_marker
                );
            }
        }
        _ => {
            println!("  (none)");
        }
    }
}

/// Format HPM dependency specification for display
///
/// Converts a dependency specification into a human-readable string.
/// Handles both simple version strings and detailed specifications with git sources.
///
/// # Arguments
///
/// * `spec` - The dependency specification to format
///
/// # Returns
///
/// A formatted string representing the dependency specification
///
/// # Examples
///
/// * Simple: `"^1.0.0"` → `"^1.0.0"`
/// * Git with tag: `{git: "...", tag: "v1.0"}` → `"git: ... (tag: v1.0)"`
fn format_dependency_spec(spec: &DependencySpec) -> String {
    match spec {
        DependencySpec::Simple(version) => version.clone(),
        DependencySpec::Git { git, commit, .. } => {
            format!("git: {} (commit: {})", git, &commit[..commit.len().min(12)])
        }
        DependencySpec::Path { path, .. } => {
            format!("path: {}", path)
        }
        DependencySpec::Legacy {
            version,
            git,
            tag,
            branch,
            ..
        } => {
            if let Some(git_url) = git {
                let mut git_info = format!("git: {}", git_url);
                if let Some(tag_name) = tag {
                    git_info.push_str(&format!(" (tag: {})", tag_name));
                } else if let Some(branch_name) = branch {
                    git_info.push_str(&format!(" (branch: {})", branch_name));
                }
                git_info
            } else if let Some(version_str) = version {
                version_str.clone()
            } else {
                "*".to_string()
            }
        }
    }
}

/// Check if HPM dependency is optional
///
/// Determines whether a dependency specification marks the dependency as optional.
/// Simple dependencies are never optional; detailed dependencies may have an optional flag.
///
/// # Arguments
///
/// * `spec` - The dependency specification to check
///
/// # Returns
///
/// `true` if the dependency is marked as optional, `false` otherwise
fn is_optional_dependency(spec: &DependencySpec) -> bool {
    match spec {
        DependencySpec::Simple(_) => false,
        DependencySpec::Git { optional, .. } => *optional,
        DependencySpec::Path { optional, .. } => *optional,
        DependencySpec::Legacy { optional, .. } => optional.unwrap_or(false),
    }
}

/// Format Python dependency specification for display
///
/// Converts a Python dependency specification into a human-readable version string.
///
/// # Arguments
///
/// * `spec` - The Python dependency specification to format
///
/// # Returns
///
/// A formatted version string, or `"*"` if no version is specified
fn format_python_dependency_spec(spec: &PythonDependencySpec) -> String {
    match spec {
        PythonDependencySpec::Simple(version) => version.clone(),
        PythonDependencySpec::Detailed { version, .. } => {
            version.clone().unwrap_or_else(|| "*".to_string())
        }
    }
}

/// Check if Python dependency is optional
///
/// Determines whether a Python dependency specification marks the dependency as optional.
///
/// # Arguments
///
/// * `spec` - The Python dependency specification to check
///
/// # Returns
///
/// `true` if the dependency is marked as optional, `false` otherwise
fn is_optional_python_dependency(spec: &PythonDependencySpec) -> bool {
    match spec {
        PythonDependencySpec::Simple(_) => false,
        PythonDependencySpec::Detailed { optional, .. } => optional.unwrap_or(false),
    }
}

/// Format Python dependency extras for display
///
/// Formats the extras list for a Python dependency into a display string.
/// Extras are additional optional components of a package (e.g., requests\[security\]).
///
/// # Arguments
///
/// * `spec` - The Python dependency specification containing extras
///
/// # Returns
///
/// A formatted extras string like `" [security,socks]"`, or empty string if no extras
fn format_python_extras(spec: &PythonDependencySpec) -> String {
    match spec {
        PythonDependencySpec::Simple(_) => String::new(),
        PythonDependencySpec::Detailed { extras, .. } => {
            if let Some(extras_list) = extras {
                if !extras_list.is_empty() {
                    format!(" [{}]", extras_list.join(","))
                } else {
                    String::new()
                }
            } else {
                String::new()
            }
        }
    }
}

/// Determine the path to the hpm.toml manifest file
///
/// Resolves the manifest file path using HPM's standard path resolution logic.
/// This function provides consistent behavior across all HPM commands.
///
/// # Arguments
///
/// * `provided_path` - Optional path provided by user
///   - `None`: Search for hpm.toml in current directory
///   - `Some(file_path)`: Use the file directly if it exists
///   - `Some(dir_path)`: Look for hpm.toml in the directory
///
/// # Returns
///
/// * `Ok(PathBuf)` - Resolved path to a valid hpm.toml file
/// * `Err(anyhow::Error)` - Path resolution failed
///
/// # Errors
///
/// This function will return an error if:
/// - No hpm.toml found in current directory (when `provided_path` is `None`)
/// - Provided path does not exist or is not accessible
/// - Directory path provided but contains no hpm.toml file
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
///
/// Reads and parses an hpm.toml manifest file, with validation.
/// Uses the same parsing logic as other HPM commands for consistency.
///
/// # Arguments
///
/// * `manifest_path` - Path to the hpm.toml file to load
///
/// # Returns
///
/// * `Ok(PackageManifest)` - Successfully parsed and validated manifest
/// * `Err(anyhow::Error)` - Failed to read, parse, or validate the manifest
///
/// # Errors
///
/// This function will return an error if:
/// - File cannot be read (permission, not found, etc.)
/// - TOML parsing fails (invalid syntax)
/// - Manifest validation fails (invalid package name, version, etc.)
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

#[cfg(test)]
mod tests {
    use super::*;
    use hpm_package::{DependencySpec, PythonDependencySpec};
    use std::env;
    use tempfile::TempDir;

    /// Create a test hpm.toml file with dependencies
    ///
    /// Helper function for creating test manifest files with configurable dependency sections.
    /// Used across multiple test cases to ensure consistent test data.
    ///
    /// # Arguments
    ///
    /// * `path` - Directory where hpm.toml should be created
    /// * `include_dependencies` - Whether to include HPM dependencies section
    /// * `include_python_deps` - Whether to include Python dependencies section
    ///
    /// # Returns
    ///
    /// * `Ok(())` - Successfully created manifest file
    /// * `Err(anyhow::Error)` - Failed to write manifest file
    fn create_test_manifest(
        path: &Path,
        include_dependencies: bool,
        include_python_deps: bool,
    ) -> Result<()> {
        let mut manifest_content = String::from(
            r#"[package]
name = "test-package"
version = "1.0.0"
description = "A test package for HPM list"
authors = ["Test Author <test@example.com>"]
license = "MIT"

[houdini]
min_version = "20.0"
max_version = "20.5"
"#,
        );

        if include_dependencies {
            manifest_content.push_str(
                r#"
[dependencies]
utility-nodes = "^2.1.0"
material-library = { version = "1.5", optional = true }
geometry-tools = { git = "https://github.com/example/geometry-tools", tag = "v1.0" }
"#,
            );
        }

        if include_python_deps {
            manifest_content.push_str(
                r#"
[python_dependencies]
numpy = ">=1.20.0"
requests = { version = ">=2.25.0", extras = ["security"] }
matplotlib = { version = "^3.5.0", optional = true }
"#,
            );
        }

        std::fs::write(path.join("hpm.toml"), manifest_content)?;
        Ok(())
    }

    #[test]
    fn test_determine_manifest_path_current_directory() {
        let temp_dir = TempDir::new().unwrap();
        let original_dir = env::current_dir().unwrap();

        create_test_manifest(temp_dir.path(), true, true).unwrap();

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
        create_test_manifest(temp_dir.path(), true, true).unwrap();

        let manifest_path = temp_dir.path().join("hpm.toml");
        let result = determine_manifest_path(Some(manifest_path.clone()));

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), manifest_path);
    }

    #[test]
    fn test_determine_manifest_path_explicit_directory() {
        let temp_dir = TempDir::new().unwrap();
        create_test_manifest(temp_dir.path(), true, true).unwrap();

        let result = determine_manifest_path(Some(temp_dir.path().to_path_buf()));

        assert!(result.is_ok());
        assert!(result.unwrap().ends_with("hpm.toml"));
    }

    #[test]
    fn test_load_manifest_with_dependencies() {
        let temp_dir = TempDir::new().unwrap();
        create_test_manifest(temp_dir.path(), true, true).unwrap();

        let manifest_path = temp_dir.path().join("hpm.toml");
        let result = load_manifest(&manifest_path);

        assert!(result.is_ok());
        let manifest = result.unwrap();
        assert_eq!(manifest.package.name, "test-package");
        assert_eq!(manifest.package.version, "1.0.0");
        assert!(manifest.dependencies.is_some());
        assert!(manifest.python_dependencies.is_some());
        assert_eq!(manifest.dependencies.as_ref().unwrap().len(), 3);
        assert_eq!(manifest.python_dependencies.as_ref().unwrap().len(), 3);
    }

    #[test]
    fn test_format_dependency_spec_simple() {
        let spec = DependencySpec::Simple("^1.0.0".to_string());
        let result = format_dependency_spec(&spec);
        assert_eq!(result, "^1.0.0");
    }

    #[test]
    fn test_format_dependency_spec_legacy_version() {
        let spec = DependencySpec::Legacy {
            version: Some("2.1.0".to_string()),
            git: None,
            tag: None,
            branch: None,
            optional: Some(false),
            registry: None,
        };
        let result = format_dependency_spec(&spec);
        assert_eq!(result, "2.1.0");
    }

    #[test]
    fn test_format_dependency_spec_git() {
        let spec = DependencySpec::Git {
            git: "https://github.com/example/repo".to_string(),
            commit: "abc123def456".to_string(),
            optional: false,
        };
        let result = format_dependency_spec(&spec);
        assert_eq!(result, "git: https://github.com/example/repo (commit: abc123def456)");
    }

    #[test]
    fn test_is_optional_dependency() {
        let simple = DependencySpec::Simple("^1.0.0".to_string());
        assert!(!is_optional_dependency(&simple));

        let optional = DependencySpec::Git {
            git: "https://github.com/example/repo".to_string(),
            commit: "abc123".to_string(),
            optional: true,
        };
        assert!(is_optional_dependency(&optional));

        let not_optional = DependencySpec::Git {
            git: "https://github.com/example/repo".to_string(),
            commit: "abc123".to_string(),
            optional: false,
        };
        assert!(!is_optional_dependency(&not_optional));
    }

    #[test]
    fn test_format_python_dependency_spec() {
        let simple = PythonDependencySpec::Simple(">=1.20.0".to_string());
        let result = format_python_dependency_spec(&simple);
        assert_eq!(result, ">=1.20.0");

        let detailed = PythonDependencySpec::Detailed {
            version: Some("^3.5.0".to_string()),
            optional: None,
            extras: None,
        };
        let result = format_python_dependency_spec(&detailed);
        assert_eq!(result, "^3.5.0");
    }

    #[test]
    fn test_is_optional_python_dependency() {
        let simple = PythonDependencySpec::Simple(">=1.20.0".to_string());
        assert!(!is_optional_python_dependency(&simple));

        let optional = PythonDependencySpec::Detailed {
            version: Some("^3.5.0".to_string()),
            optional: Some(true),
            extras: None,
        };
        assert!(is_optional_python_dependency(&optional));
    }

    #[test]
    fn test_format_python_extras() {
        let no_extras = PythonDependencySpec::Simple(">=1.20.0".to_string());
        let result = format_python_extras(&no_extras);
        assert_eq!(result, "");

        let with_extras = PythonDependencySpec::Detailed {
            version: Some(">=2.25.0".to_string()),
            optional: None,
            extras: Some(vec!["security".to_string(), "socks".to_string()]),
        };
        let result = format_python_extras(&with_extras);
        assert_eq!(result, " [security,socks]");

        let empty_extras = PythonDependencySpec::Detailed {
            version: Some(">=2.25.0".to_string()),
            optional: None,
            extras: Some(vec![]),
        };
        let result = format_python_extras(&empty_extras);
        assert_eq!(result, "");
    }

    #[tokio::test]
    async fn test_list_dependencies_with_full_manifest() {
        let temp_dir = TempDir::new().unwrap();
        let original_dir = env::current_dir().unwrap();

        create_test_manifest(temp_dir.path(), true, true).unwrap();

        env::set_current_dir(temp_dir.path()).unwrap();

        let result = list_dependencies(None).await;

        env::set_current_dir(&original_dir).unwrap();

        // Should complete successfully without errors
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_list_dependencies_no_dependencies() {
        let temp_dir = TempDir::new().unwrap();
        let original_dir = env::current_dir().unwrap();

        create_test_manifest(temp_dir.path(), false, false).unwrap();

        env::set_current_dir(temp_dir.path()).unwrap();

        let result = list_dependencies(None).await;

        env::set_current_dir(&original_dir).unwrap();

        // Should complete successfully even with no dependencies
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_list_dependencies_explicit_manifest_path() {
        let temp_dir = TempDir::new().unwrap();
        let manifest_path = temp_dir.path().join("custom-manifest.toml");

        let manifest_content = r#"[package]
name = "custom-path-package"
version = "2.0.0"
description = "Test custom manifest path"

[dependencies]
test-dep = "1.0.0"
"#;
        std::fs::write(&manifest_path, manifest_content).unwrap();

        let result = list_dependencies(Some(manifest_path)).await;

        assert!(result.is_ok());
    }

    #[test]
    fn test_list_dependencies_nonexistent_manifest() {
        let temp_dir = TempDir::new().unwrap();
        let nonexistent_path = temp_dir.path().join("nonexistent.toml");

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(list_dependencies(Some(nonexistent_path)));

        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg.contains("does not exist"));
    }
}
