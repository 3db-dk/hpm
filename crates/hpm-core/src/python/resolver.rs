//! Python dependency resolution using UV

use super::bundled::{ensure_managed_python, run_uv_command};
use super::error::PythonError;
use super::types::{PythonDependencies, PythonVersion, ResolvedDependencySet};
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
/// use hpm_core::python::{resolve_dependencies, PythonDependencies, PythonDependency, VersionSpec};
///
/// # async fn example() -> Result<(), hpm_core::python::PythonError> {
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
) -> Result<ResolvedDependencySet, PythonError> {
    info!(
        "Resolving {} Python dependencies",
        dependencies.dependencies.len()
    );

    let python_version = dependencies
        .python_version
        .clone()
        .unwrap_or_else(super::default_python_version);

    let resolved = compile_requirements(&requirement_lines(dependencies), python_version)
        .await
        .map_err(|e| e.uv_context("Failed to run UV dependency resolution"))?;

    info!("Resolved {} Python packages", resolved.packages.len());
    Ok(resolved)
}

/// Resolve `collected` manifest dependencies together with raw PEP-508
/// `extra_requirements` (e.g. a script's own `requirements`) into a single
/// content-addressable [`ResolvedDependencySet`].
///
/// Both inputs are written into one requirements file and compiled in a single
/// `uv pip compile` pass, so the result is one coherent resolution rather than
/// two separately-pinned sets that could disagree on a shared transitive dep.
///
/// Returns an empty set (carrying just the Python version) when there is
/// nothing to resolve — no non-optional manifest deps and no non-blank
/// requirement strings. Callers can skip venv creation in that case and rely
/// on `python/` directories alone.
///
/// The Python version comes from `collected` (the project's Houdini-mapped
/// CPython); absent that, it falls back to
/// [`crate::python::DEFAULT_PYTHON_VERSION`].
pub async fn resolve_combined(
    collected: &PythonDependencies,
    extra_requirements: &[String],
) -> Result<ResolvedDependencySet, PythonError> {
    let python_version = collected
        .python_version
        .clone()
        .unwrap_or_else(super::default_python_version);

    let has_manifest_deps = collected.dependencies.values().any(|d| !d.optional);
    let has_extra = extra_requirements.iter().any(|r| !r.trim().is_empty());
    if !has_manifest_deps && !has_extra {
        debug!("Package environment has no Python deps to resolve");
        return Ok(ResolvedDependencySet::new(python_version));
    }

    info!(
        "Resolving package environment ({} manifest dep(s) + {} extra requirement(s))",
        collected.dependencies.len(),
        extra_requirements
            .iter()
            .filter(|r| !r.trim().is_empty())
            .count()
    );

    let mut lines = requirement_lines(collected);
    lines.extend(extra_requirements.iter().map(|r| r.to_string()));

    let resolved = compile_requirements(&lines, python_version)
        .await
        .map_err(|e| e.uv_context("Failed to resolve package environment dependencies"))?;
    info!(
        "Resolved {} packages for package environment",
        resolved.packages.len()
    );
    Ok(resolved)
}

/// Render the non-optional entries of `dependencies` as PEP-508 requirement
/// lines (`name[extras]spec`).
fn requirement_lines(dependencies: &PythonDependencies) -> Vec<String> {
    dependencies
        .dependencies
        .iter()
        .filter(|(_, dep)| !dep.optional)
        .map(|(name, dep)| {
            // Handle "*" version (any version) by omitting the version specifier
            let version_part = if dep.version.spec == "*" || dep.version.spec.is_empty() {
                ""
            } else {
                dep.version.spec.as_str()
            };
            if dep.extras.is_empty() {
                format!("{}{}", name, version_part)
            } else {
                format!("{}[{}]{}", name, dep.extras.join(","), version_part)
            }
        })
        .collect()
}

/// Write requirement lines to a temp requirements file, skipping blank lines.
pub(super) fn write_requirements_file(lines: &[String]) -> Result<NamedTempFile, PythonError> {
    let mut temp_file = NamedTempFile::new().map_err(|e| PythonError::RequirementsFile {
        op: "create",
        source: e,
    })?;
    for line in lines {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        writeln!(temp_file, "{}", line).map_err(|e| PythonError::RequirementsFile {
            op: "write",
            source: e,
        })?;
    }
    temp_file
        .flush()
        .map_err(|e| PythonError::RequirementsFile {
            op: "flush",
            source: e,
        })?;
    debug!("Created requirements file: {:?}", temp_file.path());
    Ok(temp_file)
}

/// The one `uv pip compile` invocation: write `lines` to a temp requirements
/// file, make sure the managed CPython exists (on a clean machine UV won't
/// implicitly download one for this command), compile, and parse the pins.
pub(super) async fn compile_requirements(
    lines: &[String],
    python_version: PythonVersion,
) -> Result<ResolvedDependencySet, PythonError> {
    let req_file = write_requirements_file(lines)?;
    let py_str = python_version.to_string();
    ensure_managed_python(&py_str).await?;

    let output = run_uv_command(&[
        "pip",
        "compile",
        req_file.path().to_str().unwrap(),
        "--python-version",
        &py_str,
    ])
    .await?;

    Ok(ResolvedDependencySet::from_pip_compile_output(
        &output.stdout,
        python_version,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::python::types::{PythonDependency, VersionSpec};

    #[test]
    fn test_parse_resolved_dependencies() {
        let output = b"# This file was generated by uv
numpy==1.24.0
requests==2.28.0
certifi==2022.12.7";

        let python_version = PythonVersion::new(3, 9, None);
        let resolved = ResolvedDependencySet::from_pip_compile_output(output, python_version);

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

    #[tokio::test]
    async fn resolve_combined_empty_short_circuits_without_uv() {
        // No manifest deps and only-blank extra requirements: must return an
        // empty set carrying the default Python version, never shelling to uv.
        let collected = PythonDependencies::new();
        let resolved = resolve_combined(&collected, &["   ".to_string()])
            .await
            .expect("empty resolution should succeed offline");
        assert!(resolved.packages.is_empty());
        assert_eq!(resolved.python_version, PythonVersion::new(3, 11, None));
    }

    #[test]
    fn combined_requirements_include_manifest_and_extra() {
        let mut deps = PythonDependencies::new();
        deps.add_dependency(PythonDependency::new("numpy", VersionSpec::new(">=1.20")));
        let mut lines = requirement_lines(&deps);
        lines.extend(["PySide6>=6.6".to_string(), "  ".to_string()]);
        let tmp = write_requirements_file(&lines).unwrap();
        let content = std::fs::read_to_string(tmp.path()).unwrap();
        assert!(content.contains("numpy>=1.20"), "{content}");
        assert!(content.contains("PySide6>=6.6"), "{content}");
        // Blank extra requirement lines are skipped.
        assert_eq!(content.lines().filter(|l| !l.trim().is_empty()).count(), 2);
    }

    #[test]
    fn requirement_lines_render_extras_and_specs() {
        let mut deps = PythonDependencies::new();
        deps.add_dependency(PythonDependency::new("numpy", VersionSpec::new(">=1.20")));
        deps.add_dependency(
            PythonDependency::new("requests", VersionSpec::new(">=2.25"))
                .with_extras(vec!["security".to_string()]),
        );

        let temp_file = write_requirements_file(&requirement_lines(&deps)).unwrap();
        let content = std::fs::read_to_string(temp_file.path()).unwrap();

        assert!(content.contains("numpy>=1.20"));
        assert!(content.contains("requests[security]>=2.25"));
    }

    #[test]
    fn write_requirements_skips_empty_lines() {
        let reqs = vec![
            "  ".to_string(),
            "PySide6>=6.6".to_string(),
            "".to_string(),
            "numpy".to_string(),
        ];
        let tmp = write_requirements_file(&reqs).unwrap();
        let content = std::fs::read_to_string(tmp.path()).unwrap();
        assert_eq!(content, "PySide6>=6.6\nnumpy\n");
    }
}
