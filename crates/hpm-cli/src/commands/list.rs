//! HPM List Command
//!
//! Displays package metadata plus HPM and Python dependencies from an
//! `hpm.toml` manifest, either as flat sections, as a tree (`--tree`), or as
//! a single JSON document (`--output json|json-lines|json-compact`).
//!
//! ```bash
//! hpm list                                # current directory's hpm.toml
//! hpm list --manifest /path/to/project/   # explicit directory or file
//! hpm list --tree
//! hpm list --output json
//! ```

use super::manifest_utils::{determine_manifest_path, load_manifest};
use crate::console::Console;
use crate::output::OutputFormat;
use anyhow::{Context, Result};
use console::style;
use hpm_package::{DependencySpec, PackageManifest, PythonDependencySpec};
use std::path::PathBuf;
use tracing::info;

/// Display package information and dependencies from an hpm.toml manifest.
///
/// `manifest_path` may be a direct path to `hpm.toml`, a directory containing
/// one, or `None` for the current directory. `tree` selects the tree view for
/// human output; JSON output ignores it (same data either way).
pub async fn list_dependencies(
    manifest_path: Option<PathBuf>,
    tree: bool,
    console: &mut Console,
    output: OutputFormat,
) -> Result<()> {
    info!("Listing package dependencies");

    let manifest_path = determine_manifest_path(manifest_path)?;
    info!("Using manifest: {}", manifest_path.display());

    let manifest = load_manifest(&manifest_path)
        .with_context(|| format!("Failed to load manifest from {}", manifest_path.display()))?;

    if output.is_json() {
        console.stdout(output.render_json(&manifest_json(&manifest)));
    } else if tree {
        display_tree(&manifest, console);
    } else {
        display_package_info(&manifest, console);
        display_hpm_dependencies(&manifest, console);
        display_python_dependencies(&manifest, console);
    }

    Ok(())
}

/// Build the `--output json*` document: package metadata plus structured
/// dependency entries (the manifest's own `DependencySpec` serialization).
fn manifest_json(manifest: &PackageManifest) -> serde_json::Value {
    let dependencies: Vec<_> = manifest
        .dependencies
        .iter()
        .map(|(name, spec)| {
            serde_json::json!({
                "name": name,
                "spec": spec,
                "optional": is_optional_dependency(spec),
            })
        })
        .collect();

    let python_dependencies: Vec<_> = manifest
        .python_dependencies
        .iter()
        .map(|(name, spec)| {
            let extras = match spec {
                PythonDependencySpec::Detailed {
                    extras: Some(extras),
                    ..
                } => extras.clone(),
                _ => Vec::new(),
            };
            serde_json::json!({
                "name": name,
                "version": format_python_dependency_spec(spec),
                "extras": extras,
                "optional": is_optional_python_dependency(spec),
            })
        })
        .collect();

    serde_json::json!({
        "package": {
            "name": manifest.package.name,
            "version": manifest.package.version,
            "description": manifest.package.description,
            "houdini": manifest.compat.houdini.as_ref().map(|r| r.to_string()),
        },
        "dependencies": dependencies,
        "python_dependencies": python_dependencies,
    })
}

/// Render dependencies as a tree with box-drawing characters.
fn display_tree(manifest: &PackageManifest, console: &mut Console) {
    // Package header
    console.stdout(format!(
        "{} {}",
        style(&manifest.package.name).cyan().bold(),
        style(format!("v{}", manifest.package.version)).dim()
    ));

    // HPM dependencies
    if !manifest.dependencies.is_empty() {
        let count = manifest.dependencies.len();
        for (idx, (name, spec)) in manifest.dependencies.iter().enumerate() {
            let is_last = idx == count - 1;
            let prefix = if is_last { "└── " } else { "├── " };

            let source_info = format_tree_source_info(spec);
            let optional_marker = if is_optional_dependency(spec) {
                style(" [optional]").dim().to_string()
            } else {
                String::new()
            };

            console.stdout(format!(
                "{}{}{}{}",
                style(prefix).dim(),
                style(name).green(),
                style(format!(" {}", source_info)).dim(),
                optional_marker
            ));
        }
    }

    // Python dependencies
    if !manifest.python_dependencies.is_empty() {
        console.stdout("");
        console.stdout(style("Python dependencies:").yellow().bold().to_string());

        let count = manifest.python_dependencies.len();
        for (idx, (name, spec)) in manifest.python_dependencies.iter().enumerate() {
            let is_last = idx == count - 1;
            let prefix = if is_last { "└── " } else { "├── " };

            let version_info = format_python_dependency_spec(spec);
            let extras_info = format_python_extras(spec);
            let optional_marker = if is_optional_python_dependency(spec) {
                style(" [optional]").dim().to_string()
            } else {
                String::new()
            };

            console.stdout(format!(
                "{}{}{}{}{}",
                style(prefix).dim(),
                style(name).green(),
                style(format!(" {}", version_info)).dim(),
                extras_info,
                optional_marker
            ));
        }
    }

    if manifest.dependencies.is_empty() && manifest.python_dependencies.is_empty() {
        console.stdout(style("  (no dependencies)").dim().to_string());
    }
}

/// Format source info for tree display (compact format)
fn format_tree_source_info(spec: &DependencySpec) -> String {
    match spec {
        DependencySpec::Simple(version) => {
            format!("(registry@{})", version)
        }
        DependencySpec::Registry {
            version,
            registry: Some(r),
            ..
        } => {
            format!("({}@{})", r, version)
        }
        DependencySpec::Registry { version, .. } => {
            format!("(registry@{})", version)
        }
        DependencySpec::Url { url, version, .. } => {
            format!("({}@{})", url, version)
        }
        DependencySpec::Path { path, .. } => {
            format!("(path: {})", path)
        }
    }
}

/// Display package name, version, description, and Houdini compatibility.
fn display_package_info(manifest: &PackageManifest, console: &mut Console) {
    console.stdout(format!(
        "Package: {} v{}",
        manifest.package.name, manifest.package.version
    ));

    if let Some(description) = &manifest.package.description {
        console.stdout(format!("Description: {}", description));
    }

    if let Some(req) = &manifest.compat.houdini {
        console.stdout(format!("Houdini compatibility: {}", req));
    }

    console.stdout(""); // Empty line for readability
}

/// Display HPM package dependencies with their version specifications.
fn display_hpm_dependencies(manifest: &PackageManifest, console: &mut Console) {
    console.stdout("HPM Dependencies:");

    if manifest.dependencies.is_empty() {
        console.stdout("  (none)");
    } else {
        for (name, spec) in &manifest.dependencies {
            let version_info = format_dependency_spec(spec);
            let optional_marker = if is_optional_dependency(spec) {
                " (optional)"
            } else {
                ""
            };
            console.stdout(format!("  {} {}{}", name, version_info, optional_marker));
        }
    }

    console.stdout(""); // Empty line for readability
}

/// Display Python package dependencies, including extras and optional flags.
fn display_python_dependencies(manifest: &PackageManifest, console: &mut Console) {
    console.stdout("Python Dependencies:");

    if manifest.python_dependencies.is_empty() {
        console.stdout("  (none)");
    } else {
        for (name, spec) in &manifest.python_dependencies {
            let version_info = format_python_dependency_spec(spec);
            let optional_marker = if is_optional_python_dependency(spec) {
                " (optional)"
            } else {
                ""
            };
            let extras_info = format_python_extras(spec);
            console.stdout(format!(
                "  {} {}{}{}",
                name, version_info, extras_info, optional_marker
            ));
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
/// * Git: `{git: "...", version: "1.0.0"}` → `"git: ... (version: 1.0.0)"`
/// * Path: `{path: "../local"}` → `"path: ../local"`
fn format_dependency_spec(spec: &DependencySpec) -> String {
    match spec {
        DependencySpec::Simple(version) => {
            format!("registry (version: {})", version)
        }
        DependencySpec::Registry {
            version,
            registry: Some(r),
            ..
        } => {
            format!("registry: {} (version: {})", r, version)
        }
        DependencySpec::Registry { version, .. } => {
            format!("registry (version: {})", version)
        }
        DependencySpec::Url { url, version, .. } => {
            format!("url: {} (version: {})", url, version)
        }
        DependencySpec::Path { path, .. } => {
            format!("path: {}", path)
        }
    }
}

/// Check if HPM dependency is optional
///
/// Determines whether a dependency specification marks the dependency as optional.
///
/// # Arguments
///
/// * `spec` - The dependency specification to check
///
/// # Returns
///
/// `true` if the dependency is marked as optional, `false` otherwise
fn is_optional_dependency(spec: &DependencySpec) -> bool {
    spec.is_optional()
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::test_fixtures::{CwdGuard, TestManifestOpts, write_test_manifest};
    use hpm_package::{DependencySpec, PythonDependencySpec};
    use tempfile::TempDir;

    #[test]
    fn test_load_manifest_with_dependencies() {
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
        let result = load_manifest(&manifest_path);

        assert!(result.is_ok());
        let manifest = result.unwrap();
        assert_eq!(manifest.package.name, "test-package");
        assert_eq!(manifest.package.version, "1.0.0");
        assert!(!manifest.dependencies.is_empty());
        assert!(!manifest.python_dependencies.is_empty());
        assert_eq!(manifest.dependencies.len(), 2);
        assert_eq!(manifest.python_dependencies.len(), 3);
    }

    #[test]
    fn test_format_dependency_spec_url() {
        let spec = DependencySpec::Url {
            url: "https://example.com/packages/repo/1.0.0/repo-1.0.0.zip".to_string(),
            version: "1.0.0".to_string(),
            optional: false,
        };
        let result = format_dependency_spec(&spec);
        assert_eq!(
            result,
            "url: https://example.com/packages/repo/1.0.0/repo-1.0.0.zip (version: 1.0.0)"
        );
    }

    #[test]
    fn test_format_dependency_spec_path() {
        let spec = DependencySpec::Path {
            path: "../local-package".to_string(),
            optional: false,
            link: false,
        };
        let result = format_dependency_spec(&spec);
        assert_eq!(result, "path: ../local-package");
    }

    #[test]
    fn test_is_optional_dependency() {
        let url_optional = DependencySpec::Url {
            url: "https://example.com/pkg.zip".to_string(),
            version: "1.0.0".to_string(),
            optional: true,
        };
        assert!(is_optional_dependency(&url_optional));

        let url_not_optional = DependencySpec::Url {
            url: "https://example.com/pkg.zip".to_string(),
            version: "1.0.0".to_string(),
            optional: false,
        };
        assert!(!is_optional_dependency(&url_not_optional));

        let path_optional = DependencySpec::Path {
            path: "../local".to_string(),
            optional: true,
            link: false,
        };
        assert!(is_optional_dependency(&path_optional));

        let path_not_optional = DependencySpec::Path {
            path: "../local".to_string(),
            optional: false,
            link: false,
        };
        assert!(!is_optional_dependency(&path_not_optional));
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
        write_test_manifest(
            temp_dir.path(),
            TestManifestOpts {
                include_deps: true,
                include_python_deps: true,
                ..Default::default()
            },
        )
        .unwrap();

        let _cwd = CwdGuard::enter(temp_dir.path());
        let result = list_dependencies(None, false, &mut Console::new(), OutputFormat::Human).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_list_dependencies_no_dependencies() {
        let temp_dir = TempDir::new().unwrap();
        write_test_manifest(temp_dir.path(), TestManifestOpts::default()).unwrap();

        let _cwd = CwdGuard::enter(temp_dir.path());
        let result = list_dependencies(None, false, &mut Console::new(), OutputFormat::Human).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_list_dependencies_explicit_manifest_path() {
        let temp_dir = TempDir::new().unwrap();
        let manifest_path = temp_dir.path().join("custom-manifest.toml");

        let manifest_content = r#"[package]
path = "studio/custom-path-package"
name = "custom-path-package"
version = "2.0.0"
description = "Test custom manifest path"

[dependencies]
test-dep = { url = "https://example.com/packages/test-dep/1.0.0/test-dep-1.0.0.zip", version = "1.0.0" }
"#;
        std::fs::write(&manifest_path, manifest_content).unwrap();

        let result = list_dependencies(
            Some(manifest_path),
            false,
            &mut Console::new(),
            OutputFormat::Human,
        )
        .await;

        assert!(result.is_ok());
    }

    #[test]
    fn test_list_dependencies_nonexistent_manifest() {
        let temp_dir = TempDir::new().unwrap();
        let nonexistent_path = temp_dir.path().join("nonexistent.toml");

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(list_dependencies(
            Some(nonexistent_path),
            false,
            &mut Console::new(),
            OutputFormat::Human,
        ));

        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg.contains("does not exist"));
    }
}
