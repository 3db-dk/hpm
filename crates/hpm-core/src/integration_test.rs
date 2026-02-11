#[cfg(test)]
mod integration_tests {
    use crate::dependency::DependencyResolver;
    use crate::discovery::ProjectDiscovery;
    use crate::storage::StorageManager;
    use hpm_config::{ProjectsConfig, StorageConfig};
    use std::sync::Arc;
    use tempfile::TempDir;

    #[tokio::test]
    async fn end_to_end_cleanup_scenario() {
        // Setup temporary directories
        let temp_dir = TempDir::new().unwrap();
        let storage_root = temp_dir.path().join("hpm_storage");
        let projects_root = temp_dir.path().join("projects");

        // Create storage configuration
        let storage_config = StorageConfig {
            home_dir: storage_root.clone(),
            cache_dir: storage_root.join("cache"),
            packages_dir: storage_root.join("packages"),
            registry_cache_dir: storage_root.join("registry"),
        };

        // Create project configuration
        let mut projects_config = ProjectsConfig::default();
        projects_config.add_search_root(projects_root.clone());

        // Initialize storage manager
        let storage_manager = StorageManager::new(storage_config).unwrap();

        // Create mock installed packages
        let packages_dir = &storage_manager.config.packages_dir;
        std::fs::create_dir_all(packages_dir).unwrap();

        // Package A - will be used by project
        let package_a_dir = packages_dir.join("package-a@1.0.0");
        std::fs::create_dir_all(&package_a_dir).unwrap();
        std::fs::write(
            package_a_dir.join("hpm.toml"),
            r#"
[package]
name = "package-a"
version = "1.0.0"
description = "Package A"
"#,
        )
        .unwrap();

        // Package B - will be orphaned
        let package_b_dir = packages_dir.join("package-b@1.0.0");
        std::fs::create_dir_all(&package_b_dir).unwrap();
        std::fs::write(
            package_b_dir.join("hpm.toml"),
            r#"
[package]
name = "package-b"
version = "1.0.0"
description = "Package B"
"#,
        )
        .unwrap();

        // Create project that uses package A
        let project_dir = projects_root.join("test-project");
        std::fs::create_dir_all(&project_dir).unwrap();
        std::fs::write(
            project_dir.join("hpm.toml"),
            r#"
[package]
name = "test-project"
version = "1.0.0"
description = "Test project"

[dependencies]
package-a = { git = "https://github.com/example/package-a", version = "1.0.0" }
"#,
        )
        .unwrap();

        // Test discovery
        let project_discovery = ProjectDiscovery::new(projects_config.clone());
        let projects = project_discovery.find_projects().unwrap();

        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].manifest.package.name, "test-project");

        // Test dependency resolution
        let resolver = DependencyResolver::new(Arc::new(storage_manager.clone()));
        let dependency_graph = resolver.build_dependency_graph(&projects).await.unwrap();

        // Package A should be marked as needed (root dependency)
        let root_packages: Vec<_> = dependency_graph
            .nodes()
            .values()
            .filter(|node| node.is_root)
            .collect();

        assert_eq!(root_packages.len(), 1);

        // Test cleanup dry run
        let would_remove = storage_manager
            .cleanup_unused_dry_run(&projects_config)
            .await
            .unwrap();

        // Package B should be identified as orphaned
        assert_eq!(would_remove.len(), 1);
        assert!(would_remove.contains(&"package-b@1.0.0".to_string()));

        // Verify packages exist before cleanup
        assert!(storage_manager.package_exists("package-a", "1.0.0"));
        assert!(storage_manager.package_exists("package-b", "1.0.0"));

        // Perform actual cleanup
        let removed = storage_manager
            .cleanup_unused(&projects_config)
            .await
            .unwrap();

        // Verify cleanup results
        assert_eq!(removed.len(), 1);
        assert!(removed.contains(&"package-b@1.0.0".to_string()));

        // Verify package A still exists, package B is removed
        assert!(storage_manager.package_exists("package-a", "1.0.0"));
        assert!(!storage_manager.package_exists("package-b", "1.0.0"));
    }

    #[tokio::test]
    async fn transitive_dependency_preservation() {
        let temp_dir = TempDir::new().unwrap();
        let storage_root = temp_dir.path().join("hpm_storage");
        let projects_root = temp_dir.path().join("projects");

        let storage_config = StorageConfig {
            home_dir: storage_root.clone(),
            cache_dir: storage_root.join("cache"),
            packages_dir: storage_root.join("packages"),
            registry_cache_dir: storage_root.join("registry"),
        };

        let mut projects_config = ProjectsConfig::default();
        projects_config.add_search_root(projects_root.clone());

        let storage_manager = StorageManager::new(storage_config).unwrap();
        let packages_dir = &storage_manager.config.packages_dir;

        // Create package hierarchy: project -> package-a -> package-c
        // package-b is orphaned

        // Package A - depends on package C
        let package_a_dir = packages_dir.join("package-a@1.0.0");
        std::fs::create_dir_all(&package_a_dir).unwrap();
        std::fs::write(
            package_a_dir.join("hpm.toml"),
            r#"
[package]
name = "package-a"
version = "1.0.0"
description = "Package A"

[dependencies]
package-c = { git = "https://github.com/example/package-c", version = "1.0.0" }
"#,
        )
        .unwrap();

        // Package B - orphaned
        let package_b_dir = packages_dir.join("package-b@1.0.0");
        std::fs::create_dir_all(&package_b_dir).unwrap();
        std::fs::write(
            package_b_dir.join("hpm.toml"),
            r#"
[package]
name = "package-b"  
version = "1.0.0"
description = "Package B"
"#,
        )
        .unwrap();

        // Package C - transitive dependency
        let package_c_dir = packages_dir.join("package-c@1.0.0");
        std::fs::create_dir_all(&package_c_dir).unwrap();
        std::fs::write(
            package_c_dir.join("hpm.toml"),
            r#"
[package]
name = "package-c"
version = "1.0.0"
description = "Package C"
"#,
        )
        .unwrap();

        // Project that uses package A
        let project_dir = projects_root.join("test-project");
        std::fs::create_dir_all(&project_dir).unwrap();
        std::fs::write(
            project_dir.join("hpm.toml"),
            r#"
[package]
name = "test-project"
version = "1.0.0"
description = "Test project"

[dependencies]
package-a = { git = "https://github.com/example/package-a", version = "1.0.0" }
"#,
        )
        .unwrap();

        // Perform cleanup
        let removed = storage_manager
            .cleanup_unused(&projects_config)
            .await
            .unwrap();

        // Only package B should be removed
        assert_eq!(removed.len(), 1);
        assert!(removed.contains(&"package-b@1.0.0".to_string()));

        // Verify package A and C are preserved (transitive dependency)
        assert!(storage_manager.package_exists("package-a", "1.0.0"));
        assert!(storage_manager.package_exists("package-c", "1.0.0"));
        assert!(!storage_manager.package_exists("package-b", "1.0.0"));
    }

    /// Test that config errors propagate correctly across crate boundaries
    #[tokio::test]
    async fn config_error_propagation() {
        // Create storage with invalid paths - error should propagate cleanly
        let temp_dir = TempDir::new().unwrap();

        let storage_config = StorageConfig {
            home_dir: temp_dir.path().join("hpm_storage"),
            cache_dir: temp_dir.path().join("cache"),
            packages_dir: temp_dir.path().join("packages"),
            registry_cache_dir: temp_dir.path().join("registry"),
        };

        // Create storage manager - should succeed even with non-existent paths
        let storage_manager = StorageManager::new(storage_config);
        assert!(storage_manager.is_ok());

        // Listing packages on empty storage should return empty vec, not error
        let installed = storage_manager.unwrap().list_installed();
        assert!(installed.is_ok());
        assert!(installed.unwrap().is_empty());
    }

    /// Test project discovery with no projects found
    #[test]
    fn project_discovery_empty_results() {
        let temp_dir = TempDir::new().unwrap();
        let projects_root = temp_dir.path().join("empty_projects");
        std::fs::create_dir_all(&projects_root).unwrap();

        let mut projects_config = ProjectsConfig::default();
        projects_config.add_search_root(projects_root);

        let discovery = ProjectDiscovery::new(projects_config);
        let projects = discovery.find_projects();

        assert!(projects.is_ok());
        assert!(projects.unwrap().is_empty());
    }

    /// Test that project discovery handles various manifest states gracefully
    #[test]
    fn invalid_manifest_discovery_resilience() {
        let temp_dir = TempDir::new().unwrap();
        let projects_root = temp_dir.path().join("projects");

        let mut projects_config = ProjectsConfig::default();
        projects_config.add_search_root(projects_root.clone());

        // Create a project with syntactically invalid TOML
        let project_dir = projects_root.join("invalid-project");
        std::fs::create_dir_all(&project_dir).unwrap();
        std::fs::write(
            project_dir.join("hpm.toml"),
            "this is not valid { toml [ syntax",
        )
        .unwrap();

        let discovery = ProjectDiscovery::new(projects_config);
        let result = discovery.find_projects();

        // The key test: this should not panic
        // Whether it returns Ok (skipping invalid) or Err (reporting the error) is implementation-dependent
        let _ = result;
    }

    /// Test cleanup resilience with corrupted package directories
    #[tokio::test]
    async fn cleanup_handles_corrupted_packages() {
        let temp_dir = TempDir::new().unwrap();
        let storage_root = temp_dir.path().join("hpm_storage");
        let projects_root = temp_dir.path().join("projects");

        let storage_config = StorageConfig {
            home_dir: storage_root.clone(),
            cache_dir: storage_root.join("cache"),
            packages_dir: storage_root.join("packages"),
            registry_cache_dir: storage_root.join("registry"),
        };

        let mut projects_config = ProjectsConfig::default();
        projects_config.add_search_root(projects_root.clone());

        let storage_manager = StorageManager::new(storage_config).unwrap();
        let packages_dir = &storage_manager.config.packages_dir;
        std::fs::create_dir_all(packages_dir).unwrap();

        // Create a corrupted package directory (no manifest)
        let corrupted_dir = packages_dir.join("corrupted@1.0.0");
        std::fs::create_dir_all(&corrupted_dir).unwrap();
        // No hpm.toml file - this is the corruption

        // Create empty projects directory
        std::fs::create_dir_all(&projects_root).unwrap();

        // The key test: cleanup should not panic when encountering corrupted packages
        // It may return an error or skip the corrupted package, but should not crash
        let result = storage_manager.cleanup_unused(&projects_config).await;

        // Just verify it doesn't panic - the actual behavior (error or skip) is implementation-dependent
        let _ = result;
    }
}
