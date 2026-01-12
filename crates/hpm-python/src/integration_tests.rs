//! Integration tests for Python dependency management system

use crate::*;
use hpm_package::{HoudiniConfig, PackageInfo, PackageManifest, PythonDependencySpec};
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
            name: "package-a".to_string(),
            version: "1.0.0".to_string(),
            description: Some("Package A with Python deps".to_string()),
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
        python_dependencies: Some(python_deps_a),
        scripts: None,
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
            name: "package-b".to_string(),
            version: "2.0.0".to_string(),
            description: Some("Package B with Python deps".to_string()),
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
            min_version: Some("20.0".to_string()), // Same as package A
            max_version: None,
        }),
        dependencies: None,
        python_dependencies: Some(python_deps_b),
        scripts: None,
    };

    let manifests = vec![manifest_a, manifest_b];

    // Step 1: Collect Python dependencies
    let collected_deps = dependency::collect_python_dependencies(&manifests)
        .await
        .expect("Failed to collect Python dependencies");

    // Verify dependency collection
    assert!(collected_deps.dependencies.contains_key("numpy"));
    assert!(collected_deps.dependencies.contains_key("scipy"));
    assert!(collected_deps.dependencies.contains_key("matplotlib"));

    // Python version should be mapped from Houdini version (20.0 -> Python 3.9)
    assert!(collected_deps.python_version.is_some());
    let py_version = collected_deps.python_version.unwrap();
    assert_eq!(py_version.major, 3);
    assert_eq!(py_version.minor, 9);

    // Step 2: Test dependency resolution (mock since UV may not be available)
    // In a real scenario, this would resolve to exact versions
    let mut resolved_deps = types::ResolvedDependencySet::new(py_version);
    resolved_deps.add_package("numpy", "1.24.3");
    resolved_deps.add_package("scipy", "1.10.1");
    resolved_deps.add_package("matplotlib", "3.7.1");

    // Step 3: Test virtual environment hash generation
    let venv_hash = resolved_deps.hash();
    assert_eq!(venv_hash.len(), 16); // SHA-256 truncated to 16 chars

    // Step 4: Test VenvManager operations
    let venv_manager = venv::VenvManager::new();
    let venv_path = get_venvs_dir().join(&venv_hash);

    // Verify Python site packages path generation
    let site_packages_path = venv_manager.get_python_site_packages_path(&venv_path);
    #[cfg(target_os = "windows")]
    assert!(site_packages_path
        .to_string_lossy()
        .contains("Lib\\site-packages"));
    #[cfg(not(target_os = "windows"))]
    assert!(site_packages_path
        .to_string_lossy()
        .contains("lib/python/site-packages"));

    // Step 5: Test Houdini package.json generation
    let package_json_with_python =
        integration::generate_houdini_package_json("test-package", Some(&venv_path))
            .expect("Failed to generate package.json");

    // Verify package.json contains PYTHONPATH
    assert_eq!(package_json_with_python["path"], "$HPM_PACKAGE_ROOT");
    assert_eq!(package_json_with_python["hpm_managed"], true);
    assert_eq!(package_json_with_python["hpm_package"], "test-package");

    let env_array = package_json_with_python["env"].as_array().unwrap();
    assert_eq!(env_array.len(), 1);
    let pythonpath = env_array[0]["PYTHONPATH"].as_str().unwrap();
    assert!(pythonpath.contains("site-packages"));
    assert!(pythonpath.ends_with("$PYTHONPATH") || pythonpath.ends_with("%PYTHONPATH%"));

    // Step 6: Test cleanup analyzer
    let cleanup_analyzer = cleanup::PythonCleanupAnalyzer::new();
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
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Conflicting versions"));
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
    // Test edge cases in Houdini -> Python version mapping through dependency collection
    use hpm_package::{HoudiniConfig, PackageInfo};
    use indexmap::IndexMap;

    // Test with invalid Houdini version - should fall back to default Python version
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
            min_version: Some("invalid".to_string()),
            max_version: None,
        }),
        dependencies: None,
        python_dependencies: Some(IndexMap::new()),
        scripts: None,
    };

    let manifests = vec![manifest];
    let collected_deps = dependency::collect_python_dependencies(&manifests)
        .await
        .expect("Failed to collect Python dependencies");

    // Should fall back to default Python version
    assert!(collected_deps.python_version.is_some());
    let py_version = collected_deps.python_version.unwrap();
    assert_eq!(py_version.major, 3);
    assert_eq!(py_version.minor, 9); // Default fallback
}

#[tokio::test]
async fn test_cleanup_system_comprehensive() {
    // Test the cleanup system's ability to track virtual environments
    let cleanup_analyzer = cleanup::PythonCleanupAnalyzer::new();

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
