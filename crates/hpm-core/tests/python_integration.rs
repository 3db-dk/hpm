//! Integration tests for Python dependency management system

use hpm_core::python::{cleanup, collection, types, venv};
use hpm_package::{CompatConfig, PackageInfo, PackageManifest, PackagePath, PythonDependencySpec};
use indexmap::IndexMap;

#[tokio::test]
async fn test_end_to_end_python_workflow() {
    // This test demonstrates the complete Python dependency workflow:
    // 1. Package manifests with Python dependencies
    // 2. Dependency collection from manifests
    // 3. Dependency resolution using UV
    // 4. Virtual environment creation
    // 5. Houdini package.json generation with PYTHONPATH

    // Create test package manifests with Python dependencies
    let mut python_deps_a = IndexMap::new();
    python_deps_a.insert(
        "numpy".to_string(),
        PythonDependencySpec::Simple(">=1.20.0".to_string()),
    );
    python_deps_a.insert(
        "scipy".to_string(),
        PythonDependencySpec::Detailed {
            version: Some(">=1.7.0".to_string()),
            optional: Some(false),
            extras: None,
        },
    );

    let manifest_a = PackageManifest {
        package: PackageInfo {
            path: PackagePath::new("studio/package-a").unwrap(),
            name: "Package A".to_string(),
            version: "1.0.0".to_string(),
            description: Some("Package A with Python deps".to_string()),
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
        python_dependencies: python_deps_a,
        runtime: indexmap::IndexMap::new(),
        scripts: Default::default(),
        operators: Vec::new(),
    };

    let mut python_deps_b = IndexMap::new();
    python_deps_b.insert(
        "numpy".to_string(),
        PythonDependencySpec::Simple(">=1.20.0".to_string()),
    );
    python_deps_b.insert(
        "matplotlib".to_string(),
        PythonDependencySpec::Detailed {
            version: Some(">=3.5.0".to_string()),
            optional: Some(false),
            extras: Some(vec!["qt5".to_string()]),
        },
    );

    let manifest_b = PackageManifest {
        package: PackageInfo {
            path: PackagePath::new("studio/package-b").unwrap(),
            name: "Package B".to_string(),
            version: "2.0.0".to_string(),
            description: Some("Package B with Python deps".to_string()),
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
            houdini: Some(hpm_package::HoudiniRange::parse(">=20.5").unwrap()), // Same as A
            platforms: Vec::new(),
        },
        stage: Default::default(),
        registries: Vec::new(),
        dependencies: indexmap::IndexMap::new(),
        python_dependencies: python_deps_b,
        runtime: indexmap::IndexMap::new(),
        scripts: Default::default(),
        operators: Vec::new(),
    };

    let manifests = vec![manifest_a, manifest_b];

    // Step 1: Collect Python dependencies. `None` exercises the legacy
    // per-package houdini→python mapping path.
    let collected_deps = collection::collect_python_dependencies(None, &manifests)
        .await
        .expect("Failed to collect Python dependencies");

    // Verify dependency collection
    assert!(collected_deps.dependencies.contains_key("numpy"));
    assert!(collected_deps.dependencies.contains_key("scipy"));
    assert!(collected_deps.dependencies.contains_key("matplotlib"));

    // Python version should be mapped from Houdini version (20.5 -> Python 3.11)
    assert!(collected_deps.python_version.is_some());
    let py_version = collected_deps.python_version.unwrap();
    assert_eq!(py_version.major, 3);
    assert_eq!(py_version.minor, 11);

    // Step 2: Test dependency resolution (mock since UV may not be available)
    // In a real scenario, this would resolve to exact versions
    let mut resolved_deps = types::ResolvedDependencySet::new(py_version);
    resolved_deps.add_package("numpy", "1.24.3");
    resolved_deps.add_package("scipy", "1.10.1");
    resolved_deps.add_package("matplotlib", "3.7.1");

    // Step 3: Test virtual environment hash generation
    let venv_hash = resolved_deps.hash();
    assert_eq!(venv_hash.len(), 16); // SHA-256 truncated to 16 chars

    // Step 4: Test VenvManager operations against an isolated tempdir so the
    // test doesn't read the developer's real `~/.hpm/venvs/` contents.
    let venvs_tmp = tempfile::TempDir::new().expect("Failed to create tempdir");
    let venv_manager = venv::VenvManager::with_venvs_dir(venvs_tmp.path().to_path_buf());
    let venv_path = venvs_tmp.path().join(&venv_hash);

    // Verify Python site packages path generation. The Unix layout must
    // match uv's real venv structure (`lib/pythonX.Y/site-packages`) — the
    // previous `lib/python/site-packages` string was fictional and pointed
    // PYTHONPATH at an empty directory.
    let site_packages_path =
        venv_manager.get_python_site_packages_path(&venv_path, &resolved_deps.python_version);
    #[cfg(target_os = "windows")]
    assert!(
        site_packages_path
            .to_string_lossy()
            .contains("Lib\\site-packages")
    );
    #[cfg(not(target_os = "windows"))]
    assert!(
        site_packages_path.to_string_lossy().contains(&format!(
            "lib/python{}.{}/site-packages",
            resolved_deps.python_version.major, resolved_deps.python_version.minor
        )),
        "unexpected site-packages path: {}",
        site_packages_path.display()
    );

    // Step 5: Test cleanup analyzer against the same isolated tempdir so any
    // real venvs on the developer's machine don't leak into the assertions.
    let cleanup_analyzer = cleanup::PythonCleanupAnalyzer::with_venv_manager(
        venv::VenvManager::with_venvs_dir(venvs_tmp.path().to_path_buf()),
    );
    let venv_stats = cleanup_analyzer
        .get_venv_stats()
        .await
        .expect("Failed to get venv stats");

    // Since no actual venvs exist in test, stats should be empty
    assert_eq!(venv_stats.total_count, 0);
    assert_eq!(venv_stats.active_count, 0);
    assert_eq!(venv_stats.orphaned_count, 0);
}

#[tokio::test]
async fn test_python_dependency_merging() {
    // Test dependency merging across multiple packages
    let mut deps_a = types::PythonDependencies::new();
    deps_a.add_dependency(types::PythonDependency::new(
        "numpy",
        types::VersionSpec::new(">=1.20"),
    ));
    deps_a.add_dependency(types::PythonDependency::new(
        "scipy",
        types::VersionSpec::new(">=1.7"),
    ));
    deps_a.set_python_version(types::PythonVersion::new(3, 9, None));

    let mut deps_b = types::PythonDependencies::new();
    deps_b.add_dependency(types::PythonDependency::new(
        "numpy",
        types::VersionSpec::new(">=1.20"),
    ));
    deps_b.add_dependency(types::PythonDependency::new(
        "matplotlib",
        types::VersionSpec::new(">=3.5"),
    ));
    deps_b.set_python_version(types::PythonVersion::new(3, 9, None));

    // Merge should succeed (compatible versions)
    let result = deps_a.merge(&deps_b);
    assert!(result.is_ok());

    // Should have all three packages
    assert_eq!(deps_a.dependencies.len(), 3);
    assert!(deps_a.dependencies.contains_key("numpy"));
    assert!(deps_a.dependencies.contains_key("scipy"));
    assert!(deps_a.dependencies.contains_key("matplotlib"));
}

#[tokio::test]
async fn test_python_dependency_conflict_detection() {
    // Test conflict detection during merging
    let mut deps_a = types::PythonDependencies::new();
    deps_a.add_dependency(types::PythonDependency::new(
        "numpy",
        types::VersionSpec::new(">=1.20"),
    ));
    deps_a.set_python_version(types::PythonVersion::new(3, 9, None));

    let mut deps_b = types::PythonDependencies::new();
    deps_b.add_dependency(types::PythonDependency::new(
        "numpy",
        types::VersionSpec::new(">=1.25"),
    )); // Different version
    deps_b.set_python_version(types::PythonVersion::new(3, 9, None));

    // Merge should detect conflict
    let result = deps_a.merge(&deps_b);
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("Conflicting versions")
    );
}

#[tokio::test]
async fn test_virtual_environment_sharing() {
    // Test that packages with same resolved dependencies share venvs
    let python_version = types::PythonVersion::new(3, 9, None);

    // Create two identical dependency sets
    let mut resolved_a = types::ResolvedDependencySet::new(python_version.clone());
    resolved_a.add_package("numpy", "1.24.0");
    resolved_a.add_package("scipy", "1.10.0");

    let mut resolved_b = types::ResolvedDependencySet::new(python_version);
    resolved_b.add_package("numpy", "1.24.0");
    resolved_b.add_package("scipy", "1.10.0");

    // Should generate same hash (enabling venv sharing)
    assert_eq!(resolved_a.hash(), resolved_b.hash());

    // Different dependency sets should have different hashes
    let mut resolved_c = types::ResolvedDependencySet::new(types::PythonVersion::new(3, 9, None));
    resolved_c.add_package("numpy", "1.25.0"); // Different version
    resolved_c.add_package("scipy", "1.10.0");

    assert_ne!(resolved_a.hash(), resolved_c.hash());
}

#[tokio::test]
async fn test_houdini_python_version_mapping_edge_cases() {
    // Unparseable or unmapped Houdini versions must hard-fail rather than
    // silently install a wrong Python version into the venv.
    use hpm_package::{PackageInfo, PackagePath};
    use indexmap::IndexMap;

    let make_manifest = |version: &str| {
        let mut python_deps = IndexMap::new();
        python_deps.insert(
            "numpy".to_string(),
            PythonDependencySpec::Simple(">=1.20.0".to_string()),
        );
        PackageManifest {
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
                houdini: Some(
                    hpm_package::HoudiniRange::parse(format!(">={}", version))
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
            operators: Vec::new(),
        }
    };

    // A syntactically invalid `[compat].houdini` is caught earlier by
    // `PackageManifest::validate()` (slice 1 of the manifest refactor), so
    // by the time `collect_python_dependencies` sees a manifest, the range
    // either parses or `houdini_min()` returns `None`. We therefore exercise
    // only the unmapped-but-parseable case at the per-package layer; the
    // unparseable case is covered by the `[compat].houdini` validate tests
    // in hpm-package::manifest.

    // Known-but-unmapped future major → error (so we don't silently pick an
    // outdated Python ABI when a new Houdini ships).
    let err = collection::collect_python_dependencies(None, &[make_manifest("23.0")])
        .await
        .expect_err("expected error for unmapped Houdini major");
    assert!(
        err.to_string().contains("No Python version mapping"),
        "unexpected error message: {err}"
    );

    // Same surface checked through the project-houdini-version path: an
    // unparseable or unmapped project Houdini must error before we touch
    // any package, not after.
    let err = collection::collect_python_dependencies(Some("invalid"), &[])
        .await
        .expect_err("expected error for unparseable project Houdini version");
    assert!(
        err.to_string().contains("Could not parse Houdini version"),
        "unexpected error message: {err}"
    );
    let err = collection::collect_python_dependencies(Some("23.0"), &[])
        .await
        .expect_err("expected error for unmapped project Houdini major");
    assert!(
        err.to_string().contains("No Python version mapping"),
        "unexpected error message: {err}"
    );
}

#[tokio::test]
async fn test_cleanup_system_comprehensive() {
    // Test the cleanup system's ability to track virtual environments.
    // Route the analyzer at an empty tempdir so we don't inspect (or delete
    // from) the developer's real `~/.hpm/venvs/`.
    let venvs_tmp = tempfile::TempDir::new().expect("Failed to create tempdir");
    let cleanup_analyzer = cleanup::PythonCleanupAnalyzer::with_venv_manager(
        venv::VenvManager::with_venvs_dir(venvs_tmp.path().to_path_buf()),
    );

    // Test with no active packages (should find no orphans since no venvs exist)
    let active_packages = vec![];
    let orphaned_venvs = cleanup_analyzer
        .analyze_orphaned_venvs(&active_packages)
        .await
        .expect("Failed to analyze orphaned venvs");

    assert!(orphaned_venvs.is_empty()); // No venvs exist in test environment

    // Test cleanup dry run
    let cleanup_result = cleanup_analyzer
        .cleanup_orphaned_venvs(&orphaned_venvs, true) // dry_run = true
        .await
        .expect("Failed to perform cleanup dry run");

    assert_eq!(cleanup_result.items_that_would_be_cleaned(), 0);
    assert_eq!(cleanup_result.space_that_would_be_freed, 0);

    // Test actual cleanup (should be safe since no venvs exist)
    let cleanup_result = cleanup_analyzer
        .cleanup_orphaned_venvs(&orphaned_venvs, false) // dry_run = false
        .await
        .expect("Failed to perform cleanup");

    assert_eq!(cleanup_result.items_cleaned(), 0);
    assert_eq!(cleanup_result.space_freed, 0);
}

#[test]
fn test_python_dependency_types_comprehensive() {
    // Test all Python dependency type functionality

    // Test PythonVersion
    let py_version = types::PythonVersion::new(3, 9, Some(12));
    assert_eq!(py_version.to_string(), "3.9.12");

    let py_version_no_patch = types::PythonVersion::new(3, 10, None);
    assert_eq!(py_version_no_patch.to_string(), "3.10");

    // Test VersionSpec
    let version_spec = types::VersionSpec::new(">=1.20.0");
    assert_eq!(version_spec.to_string(), ">=1.20.0");

    // Test PythonDependency
    let mut dep = types::PythonDependency::new("numpy", types::VersionSpec::new(">=1.20"));
    assert_eq!(dep.name, "numpy");
    assert!(!dep.optional);
    assert!(dep.extras.is_empty());

    dep = dep
        .optional()
        .with_extras(vec!["testing".to_string(), "dev".to_string()]);
    assert!(dep.optional);
    assert_eq!(dep.extras, vec!["testing", "dev"]);

    // Test ResolvedDependencySet
    let mut resolved = types::ResolvedDependencySet::new(py_version);
    assert!(resolved.packages.is_empty());

    resolved.add_package("numpy", "1.24.0");
    resolved.add_package("scipy", "1.10.0");
    assert_eq!(resolved.packages.len(), 2);

    // Hash should be consistent
    let hash1 = resolved.hash();
    let hash2 = resolved.hash();
    assert_eq!(hash1, hash2);
    assert_eq!(hash1.len(), 16);
}
