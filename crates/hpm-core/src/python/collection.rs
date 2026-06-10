//! Dependency collection and management

use super::types::{PythonDependencies, PythonDependency, PythonVersion, VersionSpec};
use anyhow::Result;
use hpm_package::{PackageManifest, PythonDependencySpec};

/// Collect Python dependencies from HPM package manifests.
///
/// Aggregates Python dependencies from multiple package manifests and resolves
/// the target Python version against the project's Houdini build.
///
/// # Arguments
///
/// * `project_houdini_version` — The project's `[compat].houdini` lower bound. When
///   provided, it is the **authoritative** source for Python version
///   selection: Houdini ships a fixed embedded CPython per major release
///   (20.5→3.11, 21→3.11, 22→3.13), and the venv must match that ABI or
///   wheels will fail to import inside Houdini. Per-package
///   `[compat].houdini` declarations describe compatibility floors and are
///   intentionally ignored for Python version mapping in this case.
///   When `None`, falls back to per-package mapping (used by callers without
///   a project context, e.g. standalone tests).
/// * `packages` — Slice of package manifests whose `[python_dependencies]`
///   should be aggregated.
///
/// # Errors
///
/// Returns an error if:
/// - `project_houdini_version` maps to an unsupported Houdini version
/// - Conflicting dependency versions are specified for the same package
/// - With `project_houdini_version = None`: conflicting per-package Houdini
///   versions imply different Python ABIs
///
/// # Example
///
/// ```rust,no_run
/// use hpm_core::python::collect_python_dependencies;
/// use hpm_package::PackageManifest;
///
/// # async fn example() -> anyhow::Result<()> {
/// let packages: Vec<PackageManifest> = vec![]; // Load your package manifests
/// let dependencies = collect_python_dependencies(Some("22.0.307"), &packages).await?;
///
/// println!("Found {} Python dependencies", dependencies.dependencies.len());
/// if let Some(py_version) = dependencies.python_version {
///     println!("Required Python version: {}", py_version);
/// }
/// # Ok(())
/// # }
/// ```
pub async fn collect_python_dependencies(
    project_houdini_version: Option<&str>,
    packages: &[PackageManifest],
) -> Result<PythonDependencies> {
    let mut all_deps = PythonDependencies::new();

    // The project's own Houdini version is authoritative — Houdini ships a
    // specific embedded CPython, and any package wheels we install need to
    // match that ABI. A package declaring `[compat].houdini = ">=21.0"` only
    // means "I support 21+", not "the venv must target 21's Python 3.11", so
    // per-package mapping would silently produce a venv that crashes at
    // import when Houdini 22 (Python 3.13) loads it.
    let project_python_version = match project_houdini_version {
        Some(v) => Some(map_houdini_to_python_version(v)?),
        None => None,
    };

    for package in packages {
        if let Some(python_deps) =
            extract_python_dependencies(package, project_python_version.is_some())?
        {
            all_deps.merge(&python_deps)?;
        }
    }

    if let Some(version) = project_python_version {
        all_deps.set_python_version(version);
    }

    Ok(all_deps)
}

/// Extract Python dependencies from a single package manifest.
///
/// `project_overrides_python_version` is set when the caller has a project-level
/// Houdini version that will be applied at the end. In that case we skip the
/// per-package houdini→python mapping entirely so a 21.x package mixed with a
/// 22.x project doesn't trigger the conflict guard in `PythonDependencies::merge`.
fn extract_python_dependencies(
    manifest: &PackageManifest,
    project_overrides_python_version: bool,
) -> Result<Option<PythonDependencies>> {
    if manifest.python_dependencies.is_empty() {
        return Ok(None);
    }
    let mut deps = PythonDependencies::new();

    if !project_overrides_python_version && let Some(min_version) = manifest.compat.houdini_min() {
        let python_version = map_houdini_to_python_version(&min_version)?;
        deps.set_python_version(python_version);
    }

    for (name, spec) in &manifest.python_dependencies {
        deps.add_dependency(convert_spec_to_dependency(name, spec)?);
    }

    Ok(Some(deps))
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
        // Houdini 20.5's main builds are built against Python 3.11; a 3.10
        // build exists only as a separate download. We map to the default.
        // https://www.sidefx.com/docs/houdini/news/20_5/platforms.html
        (20, 5..) => Ok(PythonVersion::new(3, 11, None)),
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
    use hpm_package::{CompatConfig, PackageInfo, PackagePath};
    use indexmap::IndexMap;

    #[tokio::test]
    async fn test_empty_dependency_collection() {
        let packages = vec![];
        let result = collect_python_dependencies(None, &packages).await.unwrap();
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
                authors: Vec::new(),
                license: None,
                readme: None,
                homepage: None,
                repository: None,
                documentation: None,
                keywords: Vec::new(),
                categories: Vec::new(),
            },
            compat: CompatConfig {
                houdini: Some(hpm_package::HoudiniRange::parse(">=20.5").unwrap()),
                platforms: Vec::new(),
            },
            stage: Default::default(),
            registries: Vec::new(),
            dependencies: indexmap::IndexMap::new(),
            python_dependencies: python_deps,
            runtime: indexmap::IndexMap::new(),
            scripts: Default::default(),
        };

        let packages = vec![manifest];
        let result = collect_python_dependencies(None, &packages).await.unwrap();

        assert_eq!(result.dependencies.len(), 2);
        assert!(result.dependencies.contains_key("numpy"));
        assert!(result.dependencies.contains_key("requests"));

        // Check that Python version was mapped correctly (Houdini 20.5 -> Python 3.11)
        assert!(result.python_version.is_some());
        let py_version = result.python_version.unwrap();
        assert_eq!(py_version.major, 3);
        assert_eq!(py_version.minor, 11);
    }

    /// When the project pins Houdini 22 but a package only declares
    /// `[compat].houdini = ">=20.5"`, the resolver must target Python 3.13
    /// (Houdini 22's embedded interpreter) — not 3.11. Without this override
    /// the venv would carry 3.11 wheels that crash at import inside Houdini 22.
    #[tokio::test]
    async fn test_project_houdini_version_overrides_per_package_mapping() {
        let mut python_deps = IndexMap::new();
        python_deps.insert(
            "numpy".to_string(),
            PythonDependencySpec::Simple(">=1.20.0".to_string()),
        );
        let manifest = PackageManifest {
            package: PackageInfo {
                path: PackagePath::new("studio/test-package").unwrap(),
                name: "Test Package".to_string(),
                version: "1.0.0".to_string(),
                description: None,
                authors: Vec::new(),
                license: None,
                readme: None,
                homepage: None,
                repository: None,
                documentation: None,
                keywords: Vec::new(),
                categories: Vec::new(),
            },
            compat: CompatConfig {
                houdini: Some(hpm_package::HoudiniRange::parse(">=20.5").unwrap()),
                platforms: Vec::new(),
            },
            stage: Default::default(),
            registries: Vec::new(),
            dependencies: indexmap::IndexMap::new(),
            python_dependencies: python_deps,
            runtime: indexmap::IndexMap::new(),
            scripts: Default::default(),
        };

        let result = collect_python_dependencies(Some("22.0.307"), &[manifest])
            .await
            .unwrap();

        let py = result.python_version.expect("project version applied");
        assert_eq!((py.major, py.minor), (3, 13));
    }

    /// Packages with conflicting `[compat].houdini` lower bounds (21 → 3.11 vs 22 → 3.13)
    /// would otherwise hard-fail in `PythonDependencies::merge`. The project
    /// override must short-circuit that check so a 21+22 mix resolves cleanly
    /// against the project's actual Houdini build.
    #[tokio::test]
    async fn test_project_override_resolves_per_package_conflict() {
        let make_manifest = |slug: &str, min_houdini: &str| {
            let mut python_deps = IndexMap::new();
            python_deps.insert(
                "numpy".to_string(),
                PythonDependencySpec::Simple(">=1.20.0".to_string()),
            );
            PackageManifest {
                package: PackageInfo {
                    path: PackagePath::new(slug).unwrap(),
                    name: slug.to_string(),
                    version: "1.0.0".to_string(),
                    description: None,
                    authors: Vec::new(),
                    license: None,
                    readme: None,
                    homepage: None,
                    repository: None,
                    documentation: None,
                    keywords: Vec::new(),
                    categories: Vec::new(),
                },
                compat: CompatConfig {
                    houdini: Some(
                        hpm_package::HoudiniRange::parse(format!(">={}", min_houdini))
                            .expect("test fixture range is valid"),
                    ),
                    platforms: Vec::new(),
                },
                stage: Default::default(),
                registries: Vec::new(),
                dependencies: indexmap::IndexMap::new(),
                python_dependencies: python_deps,
                runtime: indexmap::IndexMap::new(),
                scripts: Default::default(),
            }
        };

        let packages = vec![
            make_manifest("studio/pkg-a", "21.0"),
            make_manifest("studio/pkg-b", "22.0"),
        ];

        // Without override: per-package mapping conflicts (3.11 vs 3.13).
        assert!(collect_python_dependencies(None, &packages).await.is_err());

        // With override: project version is authoritative.
        let result = collect_python_dependencies(Some("22.0"), &packages)
            .await
            .unwrap();
        let py = result.python_version.unwrap();
        assert_eq!((py.major, py.minor), (3, 13));
    }

    #[test]
    fn test_houdini_to_python_version_mapping() {
        assert_eq!(
            map_houdini_to_python_version("20.5").unwrap(),
            PythonVersion::new(3, 11, None)
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
