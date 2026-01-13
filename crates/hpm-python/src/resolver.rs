//! Python dependency resolution using UV

use crate::bundled::run_uv_command;
use crate::types::{PythonDependencies, PythonVersion, ResolvedDependencySet};
use anyhow::{Context, Result};
use std::io::Write;
use tempfile::NamedTempFile;
use tracing::{debug, info};

/// Resolve Python dependencies using UV
///
/// Uses UV's dependency resolver to convert version specifications into exact package versions.
/// This ensures reproducible installations and enables content-addressable virtual environment sharing.
///
/// The resolution process:
/// 1. Creates a temporary requirements.txt file from the dependency specifications
/// 2. Runs UV's `pip compile` command to resolve exact versions
/// 3. Parses the resolved output to extract package names and versions
/// 4. Returns a `ResolvedDependencySet` with exact version pins
///
/// # Arguments
///
/// * `dependencies` - Python dependencies with version constraints to resolve
///
/// # Returns
///
/// Returns a `ResolvedDependencySet` containing exact package versions and the Python version.
///
/// # Errors
///
/// Returns an error if:
/// - UV dependency resolution fails (e.g., conflicting constraints)
/// - Temporary requirements file cannot be created
/// - UV output cannot be parsed
/// - Network issues prevent package metadata retrieval
///
/// # Example
///
/// ```rust,no_run
/// use hpm_python::{resolve_dependencies, PythonDependencies, PythonDependency, VersionSpec};
///
/// # async fn example() -> anyhow::Result<()> {
/// let mut deps = PythonDependencies::new();
/// deps.add_dependency(PythonDependency::new("numpy", VersionSpec::new(">=1.20.0")));
/// deps.add_dependency(PythonDependency::new("requests", VersionSpec::new("^2.25.0")));
///
/// let resolved = resolve_dependencies(&deps).await?;
/// println!("Resolved {} packages", resolved.packages.len());
///
/// // Now we have exact versions like "numpy==1.24.0", "requests==2.28.0"
/// for (name, version) in &resolved.packages {
///     println!("{} = {}", name, version);
/// }
/// # Ok(())
/// # }
/// ```
pub async fn resolve_dependencies(
    dependencies: &PythonDependencies,
) -> Result<ResolvedDependencySet> {
    info!(
        "Resolving {} Python dependencies",
        dependencies.dependencies.len()
    );

    // Use default Python version if not specified
    let python_version = dependencies
        .python_version
        .as_ref()
        .unwrap_or(&PythonVersion::new(3, 9, None))
        .clone();

    // Create temporary requirements file
    let req_file =
        create_requirements_file(dependencies).context("Failed to create requirements file")?;

    // Run UV to resolve dependencies
    let output = run_uv_command(&[
        "pip",
        "compile",
        req_file.path().to_str().unwrap(),
        "--python-version",
        &python_version.to_string(),
    ])
    .await
    .context("Failed to run UV dependency resolution")?;

    // Parse resolved dependencies
    let resolved = parse_resolved_dependencies(&output.stdout, python_version)
        .context("Failed to parse resolved dependencies")?;

    info!("Resolved {} Python packages", resolved.packages.len());
    Ok(resolved)
}

/// Create a requirements.txt file from Python dependencies
fn create_requirements_file(dependencies: &PythonDependencies) -> Result<NamedTempFile> {
    let mut temp_file =
        NamedTempFile::new().context("Failed to create temporary requirements file")?;

    for (name, dep) in &dependencies.dependencies {
        if !dep.optional {
            // Handle "*" version (any version) by omitting the version specifier
            let version_part = if dep.version.spec == "*" || dep.version.spec.is_empty() {
                String::new()
            } else {
                dep.version.spec.clone()
            };

            let line = if dep.extras.is_empty() {
                format!("{}{}\n", name, version_part)
            } else {
                format!("{}[{}]{}\n", name, dep.extras.join(","), version_part)
            };
            temp_file
                .write_all(line.as_bytes())
                .context("Failed to write to requirements file")?;
        }
    }

    temp_file
        .flush()
        .context("Failed to flush requirements file")?;
    debug!("Created requirements file: {:?}", temp_file.path());
    Ok(temp_file)
}

/// Parse UV's resolved dependencies output
fn parse_resolved_dependencies(
    output: &[u8],
    python_version: PythonVersion,
) -> Result<ResolvedDependencySet> {
    let output_str = String::from_utf8_lossy(output);
    let mut resolved = ResolvedDependencySet::new(python_version);

    for line in output_str.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // Parse lines like "numpy==1.24.0"
        if let Some((name, version)) = line.split_once("==") {
            // Remove any extras specification like "requests[security]==2.28.0"
            let clean_name = name.split('[').next().unwrap_or(name);
            resolved.add_package(clean_name, version);
        }
    }

    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{PythonDependency, VersionSpec};

    #[test]
    fn test_parse_resolved_dependencies() {
        let output = b"# This file was generated by uv
numpy==1.24.0
requests==2.28.0
certifi==2022.12.7";

        let python_version = PythonVersion::new(3, 9, None);
        let resolved = parse_resolved_dependencies(output, python_version).unwrap();

        assert_eq!(resolved.packages.len(), 3);
        assert_eq!(resolved.packages.get("numpy"), Some(&"1.24.0".to_string()));
        assert_eq!(
            resolved.packages.get("requests"),
            Some(&"2.28.0".to_string())
        );
        assert_eq!(
            resolved.packages.get("certifi"),
            Some(&"2022.12.7".to_string())
        );
    }

    #[test]
    fn test_create_requirements_file() {
        let mut deps = PythonDependencies::new();
        deps.add_dependency(PythonDependency::new("numpy", VersionSpec::new(">=1.20")));
        deps.add_dependency(
            PythonDependency::new("requests", VersionSpec::new(">=2.25"))
                .with_extras(vec!["security".to_string()]),
        );

        let temp_file = create_requirements_file(&deps).unwrap();
        let content = std::fs::read_to_string(temp_file.path()).unwrap();

        assert!(content.contains("numpy>=1.20"));
        assert!(content.contains("requests[security]>=2.25"));
    }
}
