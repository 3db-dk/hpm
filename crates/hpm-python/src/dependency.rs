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

/// Map Houdini version to appropriate Python version
fn map_houdini_to_python_version(houdini_version: &str) -> Result<PythonVersion> {
    // Parse Houdini version (e.g., "19.5", "20.0", "20.5")
    let version_parts: Vec<&str> = houdini_version.split('.').collect();
    if version_parts.len() < 2 {
        return Ok(PythonVersion::new(3, 9, None)); // Default fallback
    }

    let major: u32 = version_parts[0].parse().unwrap_or(19);
    let minor: u32 = version_parts[1].parse().unwrap_or(5);

    // Map Houdini versions to Python versions based on typical distributions
    let python_version = match (major, minor) {
        (19, 0..=5) => PythonVersion::new(3, 7, None),
        (20, 0) => PythonVersion::new(3, 9, None),
        (20, 5) => PythonVersion::new(3, 10, None),
        (21, _) => PythonVersion::new(3, 11, None),
        _ => PythonVersion::new(3, 9, None), // Default
    };

    Ok(python_version)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hpm_package::{HoudiniConfig, PackageInfo};
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
                name: "test-package".to_string(),
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
                min_version: Some("20.0".to_string()),
                max_version: None,
            }),
            dependencies: None,
            python_dependencies: Some(python_deps),
            scripts: None,
        };

        let packages = vec![manifest];
        let result = collect_python_dependencies(&packages).await.unwrap();

        assert_eq!(result.dependencies.len(), 2);
        assert!(result.dependencies.contains_key("numpy"));
        assert!(result.dependencies.contains_key("requests"));

        // Check that Python version was mapped correctly (Houdini 20.0 -> Python 3.9)
        assert!(result.python_version.is_some());
        let py_version = result.python_version.unwrap();
        assert_eq!(py_version.major, 3);
        assert_eq!(py_version.minor, 9);
    }

    #[test]
    fn test_houdini_to_python_version_mapping() {
        assert_eq!(
            map_houdini_to_python_version("19.5").unwrap(),
            PythonVersion::new(3, 7, None)
        );
        assert_eq!(
            map_houdini_to_python_version("20.0").unwrap(),
            PythonVersion::new(3, 9, None)
        );
        assert_eq!(
            map_houdini_to_python_version("20.5").unwrap(),
            PythonVersion::new(3, 10, None)
        );
        assert_eq!(
            map_houdini_to_python_version("21.0").unwrap(),
            PythonVersion::new(3, 11, None)
        );
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
