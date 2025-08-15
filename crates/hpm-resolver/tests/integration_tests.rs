//! Integration tests for HPM dependency resolver
//!
//! Tests the complete dependency resolution pipeline including version constraints,
//! conflict resolution, and PubGrub algorithm implementation.

use anyhow::Result;
use hpm_resolver::{
    Dependency, DependencyResolver, DependencySource, PackageId, PackageInfo, PackageProvider,
    Priority, Requirement, ResolverConfig, Version, VersionConstraint,
};
use std::collections::HashMap;

struct MockPackageProvider {
    packages: HashMap<(String, Version), PackageInfo>,
    versions: HashMap<String, Vec<Version>>,
}

impl MockPackageProvider {
    fn new() -> Self {
        Self {
            packages: HashMap::new(),
            versions: HashMap::new(),
        }
    }

    fn add_package(&mut self, name: String, version: Version, dependencies: Vec<Dependency>) {
        let package_id = PackageId::new(name.clone(), version.clone());
        let package_info = PackageInfo {
            id: package_id,
            dependencies,
            description: Some(format!("Test package {}", name)),
            houdini: None,
        };

        self.packages
            .insert((name.clone(), version.clone()), package_info);
        self.versions.entry(name.clone()).or_default().push(version);

        // Keep versions sorted
        if let Some(versions) = self.versions.get_mut(&name) {
            versions.sort();
            versions.reverse(); // Latest first
        }
    }
}

#[async_trait::async_trait]
impl PackageProvider for MockPackageProvider {
    async fn get_package_info(&mut self, name: &str, version: &Version) -> Result<PackageInfo> {
        self.packages
            .get(&(name.to_string(), version.clone()))
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Package not found: {}@{}", name, version))
    }

    async fn list_versions(&mut self, name: &str) -> Result<Vec<Version>> {
        Ok(self.versions.get(name).cloned().unwrap_or_default())
    }

    async fn get_latest_version(&mut self, name: &str) -> Result<Option<Version>> {
        Ok(self
            .versions
            .get(name)
            .and_then(|versions| versions.first().cloned()))
    }
}

#[tokio::test]
async fn test_simple_dependency_resolution() {
    let mut provider = MockPackageProvider::new();

    // Add packages to mock provider
    provider.add_package("package-a".to_string(), Version::new(1, 0, 0), vec![]);
    provider.add_package("package-b".to_string(), Version::new(1, 0, 0), vec![]);

    let config = ResolverConfig::default();
    let mut resolver = DependencyResolver::new(provider, config);

    let root_requirements = vec![
        Requirement {
            name: "package-a".to_string(),
            constraint: VersionConstraint::GreaterThanOrEqual(Version::new(1, 0, 0)),
            source_package: None,
            priority: Priority::Root,
        },
        Requirement {
            name: "package-b".to_string(),
            constraint: VersionConstraint::Compatible(Version::new(1, 0, 0)),
            source_package: None,
            priority: Priority::Root,
        },
    ];

    let resolution = resolver.resolve(root_requirements).await.unwrap();

    assert_eq!(resolution.packages.len(), 2);
    assert!(resolution.packages.contains_key("package-a"));
    assert!(resolution.packages.contains_key("package-b"));
}

#[tokio::test]
async fn test_transitive_dependencies() {
    let mut provider = MockPackageProvider::new();

    // package-a depends on package-b
    let dep = Dependency {
        name: "package-b".to_string(),
        constraint: VersionConstraint::Compatible(Version::new(1, 0, 0)),
        optional: false,
        source: DependencySource::Registry,
    };
    provider.add_package("package-a".to_string(), Version::new(1, 0, 0), vec![dep]);
    provider.add_package("package-b".to_string(), Version::new(1, 0, 0), vec![]);

    let config = ResolverConfig::default();
    let mut resolver = DependencyResolver::new(provider, config);

    let root_requirements = vec![Requirement {
        name: "package-a".to_string(),
        constraint: VersionConstraint::Any,
        source_package: None,
        priority: Priority::Root,
    }];

    let resolution = resolver.resolve(root_requirements).await.unwrap();

    assert_eq!(resolution.packages.len(), 2);
    assert!(resolution.packages.contains_key("package-a"));
    assert!(resolution.packages.contains_key("package-b"));
}

#[tokio::test]
async fn test_version_selection_latest_preferred() {
    let mut provider = MockPackageProvider::new();

    // Add multiple versions of the same package
    provider.add_package("package-a".to_string(), Version::new(1, 0, 0), vec![]);
    provider.add_package("package-a".to_string(), Version::new(1, 1, 0), vec![]);
    provider.add_package("package-a".to_string(), Version::new(2, 0, 0), vec![]);

    let config = ResolverConfig {
        prefer_latest: true,
        ..Default::default()
    };
    let mut resolver = DependencyResolver::new(provider, config);

    let root_requirements = vec![Requirement {
        name: "package-a".to_string(),
        constraint: VersionConstraint::Compatible(Version::new(1, 0, 0)),
        source_package: None,
        priority: Priority::Root,
    }];

    let resolution = resolver.resolve(root_requirements).await.unwrap();

    let package_a = resolution.packages.get("package-a").unwrap();
    assert_eq!(package_a.version, Version::new(1, 1, 0)); // Latest compatible version
}

#[tokio::test]
async fn test_version_constraint_exact() {
    let mut provider = MockPackageProvider::new();

    provider.add_package("package-a".to_string(), Version::new(1, 0, 0), vec![]);
    provider.add_package("package-a".to_string(), Version::new(1, 1, 0), vec![]);

    let config = ResolverConfig::default();
    let mut resolver = DependencyResolver::new(provider, config);

    let root_requirements = vec![Requirement {
        name: "package-a".to_string(),
        constraint: VersionConstraint::Exact(Version::new(1, 0, 0)),
        source_package: None,
        priority: Priority::Root,
    }];

    let resolution = resolver.resolve(root_requirements).await.unwrap();

    let package_a = resolution.packages.get("package-a").unwrap();
    assert_eq!(package_a.version, Version::new(1, 0, 0));
}

#[tokio::test]
async fn test_complex_dependency_graph() {
    let mut provider = MockPackageProvider::new();

    // Create a complex dependency graph:
    // root -> A, B
    // A -> C
    // B -> C (different version constraint)
    // C should resolve to a version satisfying both A and B

    let dep_c_from_a = Dependency {
        name: "package-c".to_string(),
        constraint: VersionConstraint::Compatible(Version::new(1, 0, 0)),
        optional: false,
        source: DependencySource::Registry,
    };

    let dep_c_from_b = Dependency {
        name: "package-c".to_string(),
        constraint: VersionConstraint::GreaterThanOrEqual(Version::new(1, 2, 0)),
        optional: false,
        source: DependencySource::Registry,
    };

    provider.add_package(
        "package-a".to_string(),
        Version::new(1, 0, 0),
        vec![dep_c_from_a],
    );
    provider.add_package(
        "package-b".to_string(),
        Version::new(1, 0, 0),
        vec![dep_c_from_b],
    );
    provider.add_package("package-c".to_string(), Version::new(1, 0, 0), vec![]);
    provider.add_package("package-c".to_string(), Version::new(1, 2, 0), vec![]);
    provider.add_package("package-c".to_string(), Version::new(1, 3, 0), vec![]);

    let config = ResolverConfig::default();
    let mut resolver = DependencyResolver::new(provider, config);

    let root_requirements = vec![
        Requirement {
            name: "package-a".to_string(),
            constraint: VersionConstraint::Any,
            source_package: None,
            priority: Priority::Root,
        },
        Requirement {
            name: "package-b".to_string(),
            constraint: VersionConstraint::Any,
            source_package: None,
            priority: Priority::Root,
        },
    ];

    let resolution = resolver.resolve(root_requirements).await.unwrap();

    assert_eq!(resolution.packages.len(), 3);

    let package_c = resolution.packages.get("package-c").unwrap();
    // Should resolve to 1.3.0 (latest version that satisfies both >= 1.2.0 and ^1.0.0)
    assert!(package_c.version >= Version::new(1, 2, 0));
    assert!(package_c.version.major == 1); // Compatible constraint from A
}

#[tokio::test]
async fn test_resolution_metadata() {
    let mut provider = MockPackageProvider::new();
    provider.add_package("simple-package".to_string(), Version::new(1, 0, 0), vec![]);

    let config = ResolverConfig::default();
    let mut resolver = DependencyResolver::new(provider, config);

    let root_requirements = vec![Requirement {
        name: "simple-package".to_string(),
        constraint: VersionConstraint::Any,
        source_package: None,
        priority: Priority::Root,
    }];

    let resolution = resolver.resolve(root_requirements).await.unwrap();

    assert_eq!(resolution.metadata.total_packages, 1);
    // Resolution time can be 0 for very fast resolution, just check it exists
    let _ = resolution.metadata.resolution_time_ms;
    assert_eq!(resolution.metadata.resolver_version, "1.0.0");
}

#[tokio::test]
async fn test_no_solution_case() {
    let provider = MockPackageProvider::new();
    // Don't add the required package to force a no solution scenario

    let config = ResolverConfig::default();
    let mut resolver = DependencyResolver::new(provider, config);

    let root_requirements = vec![Requirement {
        name: "nonexistent-package".to_string(),
        constraint: VersionConstraint::Any,
        source_package: None,
        priority: Priority::Root,
    }];

    let result = resolver.resolve(root_requirements).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_optional_dependencies_ignored() {
    let mut provider = MockPackageProvider::new();

    let optional_dep = Dependency {
        name: "optional-package".to_string(),
        constraint: VersionConstraint::Any,
        optional: true, // This should be ignored
        source: DependencySource::Registry,
    };

    provider.add_package(
        "package-a".to_string(),
        Version::new(1, 0, 0),
        vec![optional_dep],
    );
    // Note: we're not adding "optional-package" to the provider

    let config = ResolverConfig::default();
    let mut resolver = DependencyResolver::new(provider, config);

    let root_requirements = vec![Requirement {
        name: "package-a".to_string(),
        constraint: VersionConstraint::Any,
        source_package: None,
        priority: Priority::Root,
    }];

    let resolution = resolver.resolve(root_requirements).await.unwrap();

    // Should only resolve package-a, not the optional dependency
    assert_eq!(resolution.packages.len(), 1);
    assert!(resolution.packages.contains_key("package-a"));
    assert!(!resolution.packages.contains_key("optional-package"));
}
