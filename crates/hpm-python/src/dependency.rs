//! Dependency collection and management

use crate::types::{PythonDependencies, PythonDependency, PythonVersion, VersionSpec};
use anyhow::Result;
use hpm_package::{PackageManifest, PythonDependencySpec};

/// Collect Python dependencies from HPM package manifests
///
/// Aggregates Python dependencies from multiple HPM package manifests, resolving
/// conflicts and extracting Python version requirements based on Houdini version constraints.
///
/// This function performs the following operations:
/// 1. Extracts Python dependencies from each package manifest
/// 2. Maps Houdini version requirements to appropriate Python versions
/// 3. Merges dependencies across packages, detecting conflicts
/// 4. Returns a unified dependency specification
///
/// # Arguments
///
/// * `packages` - Slice of package manifests to process
///
/// # Returns
///
/// Returns a `PythonDependencies` structure containing all Python dependencies
/// and the required Python version.
///
/// # Errors
///
/// Returns an error if:
/// - Conflicting Python versions are found across packages
/// - Conflicting dependency versions are specified for the same package
/// - Invalid version specifications are encountered
///
/// # Example
///
/// ```rust,no_run
/// use hpm_python::collect_python_dependencies;
/// use hpm_package::PackageManifest;
///
/// # async fn example() -> anyhow::Result<()> {
/// let packages: Vec<PackageManifest> = vec![]; // Load your package manifests
/// let dependencies = collect_python_dependencies(&packages).await?;
///
/// println!("Found {} Python dependencies", dependencies.dependencies.len());
/// if let Some(py_version) = dependencies.python_version {
///     println!("Required Python version: {}", py_version);
/// }
/// # Ok(())
/// # }
/// ```
pub async fn collect_python_dependencies(
    packages: &[PackageManifest],
) -> Result<PythonDependencies> {
    let mut all_deps = PythonDependencies::new();

    for package in packages {
        if let Some(python_deps) = extract_python_dependencies(package)? {
            all_deps.merge(&python_deps)?;
        }
    }

    Ok(all_deps)
}

/// Extract Python dependencies from a single package manifest
fn extract_python_dependencies(manifest: &PackageManifest) -> Result<Option<PythonDependencies>> {
    if let Some(python_deps_spec) = &manifest.python_dependencies {
        let mut deps = PythonDependencies::new();

        // Extract Python version requirement from Houdini config
        if let Some(houdini_config) = &manifest.houdini {
            if let Some(min_version) = &houdini_config.min_version {
                // Map Houdini version to typical Python version
                let python_version = map_houdini_to_python_version(min_version)?;
                deps.set_python_version(python_version);
            }
        }

        // Process Python dependencies
        for (name, spec) in python_deps_spec {
            let dependency = convert_spec_to_dependency(name, spec)?;
            deps.add_dependency(dependency);
        }

        Ok(Some(deps))
    } else {
        Ok(None)
    }
}

/// Convert PythonDependencySpec to PythonDependency
fn convert_spec_to_dependency(name: &str, spec: &PythonDependencySpec) -> Result<PythonDependency> {
    match spec {
        PythonDependencySpec::Simple(version_spec) => {
            Ok(PythonDependency::new(name, VersionSpec::new(version_spec)))
        }
        PythonDependencySpec::Detailed {
            version,
            optional,
            extras,
        } => {
            let version_spec = version.as_deref().unwrap_or("*");
            let mut dep = PythonDependency::new(name, VersionSpec::new(version_spec));

            if let Some(true) = optional {
                dep = dep.optional();
            }

            if let Some(extras_list) = extras {
                dep = dep.with_extras(extras_list.clone());
            }

            Ok(dep)
        }
    }
}

/// Map Houdini version to appropriate Python version.
///
/// Accepts both `"21"` and `"21.0"` — bare majors are treated as `major.0`, so
/// Houdini 21 → Python 3.11 matches the documented mapping. Unparseable input
/// or versions outside the supported range return an error rather than a
/// silent fallback, so typos and unrecognised future majors surface at install
/// time instead of producing an ABI-mismatched venv.
///
/// Houdini 19.x (Python 3.7) and 20.0–20.4 (Python 3.9) are intentionally
/// unsupported: their Python interpreters are past upstream EOL.
fn map_houdini_to_python_version(houdini_version: &str) -> Result<PythonVersion> {
    let mut parts = houdini_version.split('.');
    let major: u32 = parts
        .next()
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Could not parse Houdini version '{}': expected a numeric major version (e.g. '21' or '20.5')",
                houdini_version
            )
        })?;
    let minor: u32 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);

    match (major, minor) {
        (20, 5..) => Ok(PythonVersion::new(3, 10, None)),
        (21, _) => Ok(PythonVersion::new(3, 11, None)),
        (22, _) => Ok(PythonVersion::new(3, 13, None)),
        _ => Err(anyhow::anyhow!(
            "No Python version mapping for Houdini {}; supported versions are 20.5+, 21, 22. \
             Houdini 19.x (Python 3.7) and 20.0–20.4 (Python 3.9) are past EOL.",
            houdini_version
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hpm_package::{HoudiniConfig, PackageInfo, PackagePath};
    use indexmap::IndexMap;

    #[tokio::test]
    async fn test_empty_dependency_collection() {
        let packages = vec![];
        let result = collect_python_dependencies(&packages).await.unwrap();
        assert!(result.dependencies.is_empty());
    }

    #[tokio::test]
    async fn test_dependency_collection_with_python_deps() {
        let mut python_deps = IndexMap::new();
        python_deps.insert(
            "numpy".to_string(),
            PythonDependencySpec::Simple(">=1.20.0".to_string()),
        );
        python_deps.insert(
            "requests".to_string(),
            PythonDependencySpec::Detailed {
                version: Some(">=2.25.0".to_string()),
                optional: Some(false),
                extras: Some(vec!["security".to_string()]),
            },
        );

        let manifest = PackageManifest {
            package: PackageInfo {
                path: PackagePath::new("studio/test-package").unwrap(),
                name: "Test Package".to_string(),
                version: "1.0.0".to_string(),
                description: None,
                authors: None,
                license: None,
                readme: None,
                homepage: None,
                repository: None,
                documentation: None,
                keywords: None,
                categories: None,
            },
            houdini: Some(HoudiniConfig {
                min_version: Some("20.5".to_string()),
                max_version: None,
            }),
            native: None,
            registries: None,
            dependencies: None,
            python_dependencies: Some(python_deps),
            env: None,
            scripts: None,
        };

        let packages = vec![manifest];
        let result = collect_python_dependencies(&packages).await.unwrap();

        assert_eq!(result.dependencies.len(), 2);
        assert!(result.dependencies.contains_key("numpy"));
        assert!(result.dependencies.contains_key("requests"));

        // Check that Python version was mapped correctly (Houdini 20.5 -> Python 3.10)
        assert!(result.python_version.is_some());
        let py_version = result.python_version.unwrap();
        assert_eq!(py_version.major, 3);
        assert_eq!(py_version.minor, 10);
    }

    #[test]
    fn test_houdini_to_python_version_mapping() {
        assert_eq!(
            map_houdini_to_python_version("20.5").unwrap(),
            PythonVersion::new(3, 10, None)
        );
        assert_eq!(
            map_houdini_to_python_version("21.0").unwrap(),
            PythonVersion::new(3, 11, None)
        );
        assert_eq!(
            map_houdini_to_python_version("22.0").unwrap(),
            PythonVersion::new(3, 13, None)
        );
    }

    #[test]
    fn test_houdini_to_python_version_bare_major() {
        // Bare major versions (no minor) must resolve the same as "X.0"
        // — which for 20 is 20.0, unsupported.
        assert_eq!(
            map_houdini_to_python_version("21").unwrap(),
            PythonVersion::new(3, 11, None)
        );
        assert_eq!(
            map_houdini_to_python_version("22").unwrap(),
            PythonVersion::new(3, 13, None)
        );
    }

    #[test]
    fn test_houdini_to_python_version_eol_rejected() {
        // Python 3.7 and 3.9 are past upstream EOL; the corresponding Houdini
        // majors must hard-fail rather than silently installing a dead ABI.
        for dead in ["19.0", "19.5", "20.0", "20.4"] {
            let err = map_houdini_to_python_version(dead)
                .expect_err("expected error for EOL-Python Houdini version");
            assert!(
                err.to_string().contains("No Python version mapping"),
                "unexpected error for {dead}: {err}"
            );
        }
    }

    #[test]
    fn test_convert_spec_to_dependency() {
        // Test simple spec
        let simple_spec = PythonDependencySpec::Simple(">=1.0.0".to_string());
        let dep = convert_spec_to_dependency("test-package", &simple_spec).unwrap();
        assert_eq!(dep.name, "test-package");
        assert_eq!(dep.version.spec, ">=1.0.0");
        assert!(!dep.optional);
        assert!(dep.extras.is_empty());

        // Test detailed spec
        let detailed_spec = PythonDependencySpec::Detailed {
            version: Some("^2.0.0".to_string()),
            optional: Some(true),
            extras: Some(vec!["test".to_string(), "dev".to_string()]),
        };
        let dep = convert_spec_to_dependency("detailed-package", &detailed_spec).unwrap();
        assert_eq!(dep.name, "detailed-package");
        assert_eq!(dep.version.spec, "^2.0.0");
        assert!(dep.optional);
        assert_eq!(dep.extras, vec!["test", "dev"]);
    }
}
