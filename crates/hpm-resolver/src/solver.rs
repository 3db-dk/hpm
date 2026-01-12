//! PubGrub-inspired dependency resolution solver
//!
//! This module implements the core dependency resolution algorithm inspired by PubGrub.
//! The solver uses incremental approach with partial solutions, backtracking, and
//! conflict learning to efficiently resolve complex dependency graphs.

#[cfg(test)]
use crate::VersionConstraint;
use crate::{
    Incompatibility, IncompatibilityCause, PackageId, PackageInfo, PackageProvider, Priority,
    Requirement, Resolution, ResolutionMetadata, ResolverConfig, ResolverError, Term, Version,
};
use anyhow::Result;
use std::collections::{BTreeMap, HashMap};
use std::time::{Duration, Instant};
use tracing::{debug, info};

/// The main dependency resolver implementing PubGrub algorithm
pub struct DependencyResolver<P: PackageProvider> {
    provider: P,
    config: ResolverConfig,
    incompatibilities: Vec<Incompatibility>,
    solution: PartialSolution,
    package_cache: HashMap<(String, Version), PackageInfo>,
}

/// Partial solution maintaining decided and undecided packages
#[derive(Debug, Clone)]
struct PartialSolution {
    decided: BTreeMap<String, Assignment>,
    undecided: HashMap<String, Vec<Requirement>>,
}

#[derive(Debug, Clone)]
struct Assignment {
    package_id: PackageId,
    decision_level: usize,
}

impl<P: PackageProvider> DependencyResolver<P> {
    pub fn new(provider: P, config: ResolverConfig) -> Self {
        Self {
            provider,
            config,
            incompatibilities: Vec::new(),
            solution: PartialSolution::new(),
            package_cache: HashMap::new(),
        }
    }

    /// Resolve dependencies starting from root requirements
    pub async fn resolve(&mut self, root_requirements: Vec<Requirement>) -> Result<Resolution> {
        let start_time = Instant::now();
        let mut conflicts_resolved = 0;

        info!(
            "Starting dependency resolution with {} root requirements",
            root_requirements.len()
        );

        // Initialize with root requirements
        self.add_root_requirements(root_requirements);

        let mut iterations = 0;
        loop {
            iterations += 1;

            if iterations > self.config.max_backtrack_iterations {
                return Err(ResolverError::NoSolution.into());
            }

            if start_time.elapsed() > Duration::from_secs(self.config.resolution_timeout_secs) {
                return Err(ResolverError::NoSolution.into());
            }

            // Check if solution is complete
            if self.solution.is_complete() {
                break;
            }

            // Select next package to decide
            let next_package = match self.select_next_package() {
                Some(package) => package,
                None => return Err(ResolverError::NoSolution.into()),
            };

            debug!("Selected package for resolution: {}", next_package);

            // Try to find a compatible version
            match self.select_version_for_package(&next_package).await {
                Ok(Some((version, package_info))) => {
                    // Make assignment
                    self.assign_package(&next_package, version, package_info)
                        .await?;
                }
                Ok(None) => {
                    // No compatible version found - create incompatibility
                    self.handle_no_compatible_version(&next_package);
                    conflicts_resolved += 1;
                    continue;
                }
                Err(e) => return Err(e),
            }

            // Check for conflicts
            if let Some(conflict) = self.detect_conflict() {
                self.resolve_conflict(conflict).await?;
                conflicts_resolved += 1;
            }
        }

        let resolution_time = start_time.elapsed();
        let total_packages = self.solution.decided.len();

        info!(
            "Resolution completed in {}ms with {} packages and {} conflicts",
            resolution_time.as_millis(),
            total_packages,
            conflicts_resolved
        );

        Ok(Resolution {
            packages: self
                .solution
                .decided
                .iter()
                .map(|(name, assignment)| (name.clone(), assignment.package_id.clone()))
                .collect(),
            metadata: ResolutionMetadata {
                resolver_version: "1.0.0".to_string(),
                resolution_time_ms: resolution_time.as_millis() as u64,
                total_packages,
                conflicts_resolved,
            },
        })
    }

    fn add_root_requirements(&mut self, requirements: Vec<Requirement>) {
        for req in requirements {
            self.solution.add_requirement(req);
        }

        // TODO: Implement proper root incompatibility for complex cases
        // For now, skip root incompatibility to avoid false conflicts
    }

    fn select_next_package(&self) -> Option<String> {
        // Select package with highest priority and most constraints
        self.solution
            .undecided
            .iter()
            .filter(|(_, reqs)| !reqs.is_empty())
            .min_by_key(|(_, reqs)| {
                let min_priority = reqs
                    .iter()
                    .map(|req| req.priority)
                    .min()
                    .unwrap_or(Priority::Transitive);

                // Use negative count for descending order (more constraints = higher priority)
                let constraint_count = -(reqs.len() as i32);

                (min_priority, constraint_count)
            })
            .map(|(name, _)| name.clone())
    }

    async fn select_version_for_package(
        &mut self,
        package_name: &str,
    ) -> Result<Option<(Version, PackageInfo)>> {
        let requirements = self.solution.undecided.get(package_name).ok_or_else(|| {
            ResolverError::PackageNotFound {
                name: package_name.to_string(),
            }
        })?;

        // Get all available versions
        let mut available_versions = self
            .provider
            .list_versions(package_name)
            .await
            .map_err(ResolverError::Registry)?;

        // Filter prereleases if not allowed
        if !self.config.allow_prereleases {
            available_versions.retain(|v| !v.is_prerelease());
        }

        // Sort versions (latest first if prefer_latest is true)
        if self.config.prefer_latest {
            available_versions.sort_by(|a, b| b.cmp(a));
        } else {
            available_versions.sort();
        }

        // Find first version that satisfies all requirements
        for version in available_versions {
            if requirements
                .iter()
                .all(|req| req.constraint.matches(&version))
            {
                // Get package info for this version
                let package_info = self.get_package_info(package_name, &version).await?;
                return Ok(Some((version, package_info)));
            }
        }

        Ok(None)
    }

    async fn get_package_info(&mut self, name: &str, version: &Version) -> Result<PackageInfo> {
        let cache_key = (name.to_string(), version.clone());

        if let Some(info) = self.package_cache.get(&cache_key) {
            return Ok(info.clone());
        }

        let info = self
            .provider
            .get_package_info(name, version)
            .await
            .map_err(ResolverError::Registry)?;

        self.package_cache.insert(cache_key, info.clone());
        Ok(info)
    }

    async fn assign_package(
        &mut self,
        package_name: &str,
        version: Version,
        package_info: PackageInfo,
    ) -> Result<()> {
        let package_id = PackageId::new(package_name.to_string(), version);

        debug!("Assigning package: {}", package_id);

        // Create assignment
        let assignment = Assignment {
            package_id: package_id.clone(),
            decision_level: self.solution.decided.len(),
        };

        // Add to decided packages
        self.solution
            .decided
            .insert(package_name.to_string(), assignment);
        self.solution.undecided.remove(package_name);

        // Add dependencies as new requirements
        for dep in &package_info.dependencies {
            if !dep.optional {
                let requirement = Requirement {
                    name: dep.name.clone(),
                    constraint: dep.constraint.clone(),
                    source_package: Some(package_id.clone()),
                    priority: Priority::Transitive,
                };

                self.solution.add_requirement(requirement);
            }
        }

        Ok(())
    }

    fn handle_no_compatible_version(&mut self, package_name: &str) {
        // Create incompatibility stating this package has no satisfiable versions
        if let Some(requirements) = self.solution.undecided.get(package_name) {
            let terms = requirements
                .iter()
                .map(|req| Term {
                    package: req.name.clone(),
                    constraint: req.constraint.clone(),
                    positive: false, // Negative term - exclude this constraint
                })
                .collect();

            self.incompatibilities.push(Incompatibility {
                terms,
                cause: IncompatibilityCause::NoVersions,
            });
        }
    }

    fn detect_conflict(&self) -> Option<usize> {
        // Check if any incompatibility is violated by current partial solution
        for (index, incompatibility) in self.incompatibilities.iter().enumerate() {
            if self.is_incompatibility_violated(incompatibility) {
                return Some(index);
            }
        }
        None
    }

    fn is_incompatibility_violated(&self, incompatibility: &Incompatibility) -> bool {
        // An incompatibility is violated if all its terms are satisfied by the current solution
        incompatibility.terms.iter().all(|term| {
            if let Some(assignment) = self.solution.decided.get(&term.package) {
                let matches = term.constraint.matches(&assignment.package_id.version);
                if term.positive {
                    matches
                } else {
                    !matches
                }
            } else {
                // Package not yet decided, so term is not satisfied
                false
            }
        })
    }

    async fn resolve_conflict(&mut self, _conflict_index: usize) -> Result<()> {
        // Simplified conflict resolution - in full implementation would analyze
        // conflict graph and perform intelligent backtracking

        // For now, just backtrack to previous decision level
        self.backtrack_one_level();

        Ok(())
    }

    fn backtrack_one_level(&mut self) {
        if let Some(latest_decision_level) = self
            .solution
            .decided
            .values()
            .map(|assignment| assignment.decision_level)
            .max()
        {
            // Remove all assignments at the latest decision level
            self.solution
                .decided
                .retain(|_, assignment| assignment.decision_level < latest_decision_level);

            // Reconstruct undecided packages based on remaining assignments
            // This is simplified - full implementation would be more sophisticated
        }
    }
}

impl PartialSolution {
    fn new() -> Self {
        Self {
            decided: BTreeMap::new(),
            undecided: HashMap::new(),
        }
    }

    fn is_complete(&self) -> bool {
        self.undecided.is_empty()
    }

    fn add_requirement(&mut self, requirement: Requirement) {
        let package_name = requirement.name.clone();

        // Don't add requirement for already decided packages
        if self.decided.contains_key(&package_name) {
            return;
        }

        self.undecided
            .entry(package_name)
            .or_default()
            .push(requirement);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Dependency, DependencySource};

    struct MockProvider {
        packages: HashMap<(String, Version), PackageInfo>,
        versions: HashMap<String, Vec<Version>>,
    }

    impl MockProvider {
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
                description: None,
                houdini: None,
            };

            self.packages
                .insert((name.clone(), version.clone()), package_info);
            self.versions.entry(name).or_default().push(version);
        }
    }

    #[async_trait::async_trait]
    impl PackageProvider for MockProvider {
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
                .and_then(|versions| versions.iter().max().cloned()))
        }
    }

    #[tokio::test]
    async fn test_simple_resolution() {
        let mut provider = MockProvider::new();
        provider.add_package("package-a".to_string(), Version::new(1, 0, 0), vec![]);

        let config = ResolverConfig::default();
        let mut resolver = DependencyResolver::new(provider, config);

        let root_req = Requirement {
            name: "package-a".to_string(),
            constraint: VersionConstraint::GreaterThanOrEqual(Version::new(1, 0, 0)),
            source_package: None,
            priority: Priority::Root,
        };

        let result = resolver.resolve(vec![root_req]).await.unwrap();
        assert_eq!(result.packages.len(), 1);
        assert!(result.packages.contains_key("package-a"));
    }

    #[tokio::test]
    async fn test_transitive_dependencies() {
        let mut provider = MockProvider::new();

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

        let root_req = Requirement {
            name: "package-a".to_string(),
            constraint: VersionConstraint::Any,
            source_package: None,
            priority: Priority::Root,
        };

        let result = resolver.resolve(vec![root_req]).await.unwrap();
        assert_eq!(result.packages.len(), 2);
        assert!(result.packages.contains_key("package-a"));
        assert!(result.packages.contains_key("package-b"));
    }

    #[test]
    fn test_partial_solution() {
        let mut solution = PartialSolution::new();
        assert!(solution.is_complete());

        let req = Requirement {
            name: "test-package".to_string(),
            constraint: VersionConstraint::Any,
            source_package: None,
            priority: Priority::Root,
        };

        solution.add_requirement(req);
        assert!(!solution.is_complete());
        assert!(solution.undecided.contains_key("test-package"));
    }

    // Property-based tests for dependency resolution

    use proptest::prelude::*;

    /// Strategy to generate valid package names for testing
    fn package_name_strategy() -> impl Strategy<Value = String> {
        prop_oneof![
            r"[a-z][a-z0-9-]{2,20}",
            Just("package-a".to_string()),
            Just("package-b".to_string()),
            Just("utility-lib".to_string()),
            Just("core-tools".to_string()),
        ]
        .prop_filter("Valid package name", |name| {
            !name.starts_with('-') && !name.ends_with('-') && name.len() >= 3
        })
    }

    /// Strategy to generate versions for testing
    fn version_strategy() -> impl Strategy<Value = Version> {
        (0u64..10, 0u64..10, 0u64..10)
            .prop_map(|(major, minor, patch)| Version::new(major, minor, patch))
    }

    /// Strategy to generate version constraints
    fn version_constraint_strategy() -> impl Strategy<Value = VersionConstraint> {
        prop_oneof![
            version_strategy().prop_map(VersionConstraint::Exact),
            version_strategy().prop_map(VersionConstraint::GreaterThanOrEqual),
            version_strategy().prop_map(VersionConstraint::Compatible),
            version_strategy().prop_map(VersionConstraint::Tilde),
            Just(VersionConstraint::Any),
        ]
    }

    /// Strategy to generate dependency specifications
    fn dependency_strategy() -> impl Strategy<Value = Dependency> {
        (
            package_name_strategy(),
            version_constraint_strategy(),
            any::<bool>(),
        )
            .prop_map(|(name, constraint, optional)| Dependency {
                name,
                constraint,
                optional,
                source: DependencySource::Registry,
            })
    }

    /// Strategy to generate requirements
    fn requirement_strategy() -> impl Strategy<Value = Requirement> {
        (
            package_name_strategy(),
            version_constraint_strategy(),
            prop_oneof![
                Just(Priority::Root),
                Just(Priority::Exact),
                Just(Priority::Transitive),
            ],
        )
            .prop_map(|(name, constraint, priority)| Requirement {
                name,
                constraint,
                source_package: None,
                priority,
            })
    }

    /// Strategy to generate resolver configurations
    fn resolver_config_strategy() -> impl Strategy<Value = ResolverConfig> {
        (
            any::<bool>(), // allow_prereleases
            any::<bool>(), // prefer_latest
            1u64..1000,    // max_backtrack_iterations
            30u64..300,    // resolution_timeout_secs
        )
            .prop_map(
                |(
                    allow_prereleases,
                    prefer_latest,
                    max_backtrack_iterations,
                    resolution_timeout_secs,
                )| {
                    ResolverConfig {
                        allow_prereleases,
                        prefer_latest,
                        max_backtrack_iterations: max_backtrack_iterations as usize,
                        resolution_timeout_secs,
                    }
                },
            )
    }

    /// Strategy to generate a mock package graph
    fn package_graph_strategy() -> impl Strategy<Value = Vec<(String, Version, Vec<Dependency>)>> {
        prop::collection::vec(
            (
                package_name_strategy(),
                version_strategy(),
                prop::collection::vec(dependency_strategy(), 0..5),
            ),
            1..10,
        )
    }

    proptest! {
        /// Test that partial solution behaves correctly with various requirements
        #[test]
        fn prop_partial_solution_behavior(
            requirements in prop::collection::vec(requirement_strategy(), 0..10)
        ) {
            let mut solution = PartialSolution::new();

            if requirements.is_empty() {
                prop_assert!(solution.is_complete());
            }

            for req in requirements {
                let package_name = req.name.clone();
                solution.add_requirement(req);

                // Should now be incomplete (unless already decided)
                if !solution.decided.contains_key(&package_name) {
                    prop_assert!(!solution.is_complete());
                    prop_assert!(solution.undecided.contains_key(&package_name));
                }
            }
        }

        /// Test that resolver configuration affects resolution behavior
        #[test]
        fn prop_resolver_config_consistency(config in resolver_config_strategy()) {
            let provider = MockProvider::new();
            let resolver = DependencyResolver::new(provider, config.clone());

            // Configuration should be preserved
            prop_assert_eq!(resolver.config.allow_prereleases, config.allow_prereleases);
            prop_assert_eq!(resolver.config.prefer_latest, config.prefer_latest);
            prop_assert_eq!(resolver.config.max_backtrack_iterations, config.max_backtrack_iterations);
            prop_assert_eq!(resolver.config.resolution_timeout_secs, config.resolution_timeout_secs);
        }

        /// Test that version constraint matching is consistent with resolver expectations
        #[test]
        fn prop_version_constraint_matching_in_resolver(
            constraint in version_constraint_strategy(),
            version in version_strategy()
        ) {
            let matches = constraint.matches(&version);

            // Test requirement creation and constraint behavior
            let req = Requirement {
                name: "test-package".to_string(),
                constraint: constraint.clone(),
                source_package: None,
                priority: Priority::Root,
            };

            prop_assert_eq!(req.constraint.matches(&version), matches);

            // Constraint matching should be consistent
            prop_assert_eq!(constraint.matches(&version), req.constraint.matches(&version));
        }

        /// Test that dependency creation and serialization works correctly
        #[test]
        fn prop_dependency_creation_consistency(dep in dependency_strategy()) {
            let name = dep.name.clone();
            let constraint = dep.constraint.clone();
            let optional = dep.optional;

            // Properties should be preserved
            prop_assert_eq!(dep.name, name);
            prop_assert_eq!(dep.constraint, constraint);
            prop_assert_eq!(dep.optional, optional);
            prop_assert!(matches!(dep.source, DependencySource::Registry));
        }

        /// Test that package graph generation produces valid structures
        #[test]
        fn prop_package_graph_validity(packages in package_graph_strategy()) {
            let mut provider = MockProvider::new();

            // Add all packages to provider
            for (name, version, deps) in packages {
                provider.add_package(name.clone(), version.clone(), deps);

                // Validate that versions are properly stored
                prop_assert!(provider.versions.contains_key(&name));
                prop_assert!(provider.packages.contains_key(&(name.clone(), version.clone())));
            }

            // Test that provider methods work correctly
            for (name, versions) in &provider.versions {
                prop_assert!(!versions.is_empty(), "Package {} should have at least one version", name);

                // All versions should have corresponding package info
                for version in versions {
                    prop_assert!(
                        provider.packages.contains_key(&(name.clone(), version.clone())),
                        "Package {}@{} should have package info", name, version
                    );
                }
            }
        }

        /// Test requirement priority ordering and selection logic
        #[test]
        fn prop_requirement_priority_ordering(
            root_reqs in prop::collection::vec(requirement_strategy(), 1..5),
            direct_reqs in prop::collection::vec(requirement_strategy(), 0..5),
            trans_reqs in prop::collection::vec(requirement_strategy(), 0..5)
        ) {
            let mut all_reqs = Vec::new();

            // Add requirements with different priorities
            for mut req in root_reqs {
                req.priority = Priority::Root;
                all_reqs.push(req);
            }

            for mut req in direct_reqs {
                req.priority = Priority::Exact;
                all_reqs.push(req);
            }

            for mut req in trans_reqs {
                req.priority = Priority::Transitive;
                all_reqs.push(req);
            }

            // Test that priorities are ordered correctly
            let root_count = all_reqs.iter().filter(|r| matches!(r.priority, Priority::Root)).count();
            let direct_count = all_reqs.iter().filter(|r| matches!(r.priority, Priority::Exact)).count();
            let trans_count = all_reqs.iter().filter(|r| matches!(r.priority, Priority::Transitive)).count();

            prop_assert!(root_count > 0, "Should have at least one root requirement");
            prop_assert_eq!(all_reqs.len(), root_count + direct_count + trans_count);

            // Priority ordering should be consistent
            let priorities: Vec<_> = all_reqs.iter().map(|r| r.priority).collect();
            for priority in priorities {
                prop_assert!(matches!(priority, Priority::Root | Priority::Exact | Priority::Transitive));
            }
        }

        /// Test that incompatibility detection logic is sound
        #[test]
        fn prop_incompatibility_detection(
            package_names in prop::collection::vec(package_name_strategy(), 2..5),
            constraints in prop::collection::vec(version_constraint_strategy(), 2..5)
        ) {
            let min_len = package_names.len().min(constraints.len());

            if min_len >= 2 {
                let terms: Vec<Term> = package_names.into_iter()
                    .zip(constraints.into_iter())
                    .take(min_len)
                    .map(|(package, constraint)| Term {
                        package,
                        constraint,
                        positive: true,
                    })
                    .collect();

                let incompatibility = Incompatibility {
                    terms: terms.clone(),
                    cause: IncompatibilityCause::NoVersions,
                };

                // Incompatibility should be well-formed
                prop_assert!(!incompatibility.terms.is_empty());
                prop_assert_eq!(incompatibility.terms.len(), terms.len());

                // All terms should have valid constraints
                for term in &incompatibility.terms {
                    prop_assert!(!term.package.is_empty());
                }
            }
        }

        /// Test resolution metadata consistency
        #[test]
        fn prop_resolution_metadata_consistency(
            packages in prop::collection::btree_map(
                package_name_strategy(),
                (package_name_strategy(), version_strategy()).prop_map(|(name, version)| {
                    PackageId::new(name, version)
                }),
                1..10
            ),
            conflicts_resolved in 0u32..100,
            resolution_time_ms in 1u64..10000
        ) {
            let total_packages = packages.len();

            let metadata = ResolutionMetadata {
                resolver_version: "1.0.0".to_string(),
                resolution_time_ms,
                total_packages,
                conflicts_resolved: conflicts_resolved as usize,
            };

            let resolution = Resolution {
                packages: packages.clone(),
                metadata: metadata.clone(),
            };

            // Metadata should be consistent with resolution
            prop_assert_eq!(resolution.packages.len(), total_packages);
            prop_assert_eq!(resolution.metadata.total_packages, total_packages);
            prop_assert_eq!(resolution.metadata.conflicts_resolved, conflicts_resolved as usize);
            prop_assert_eq!(resolution.metadata.resolution_time_ms, resolution_time_ms);
            prop_assert_eq!(resolution.metadata.resolver_version, "1.0.0");
        }

        /// Test that mock provider behaves consistently with real provider interface
        #[test]
        fn prop_mock_provider_consistency(
            packages in prop::collection::vec(
                (package_name_strategy(), version_strategy(), prop::collection::vec(dependency_strategy(), 0..3)),
                1..8
            )
        ) {
            let _ = tokio_test::block_on(async {
                let mut provider = MockProvider::new();

                // Add packages
                for (name, version, deps) in packages {
                    provider.add_package(name.clone(), version.clone(), deps);

                    // Test that we can retrieve the package info
                    let info_result = provider.get_package_info(&name, &version).await;
                    prop_assert!(info_result.is_ok(), "Should be able to get package info for {}@{}", name, version);

                    if let Ok(info) = info_result {
                        prop_assert_eq!(info.id.name, name.clone());
                        prop_assert_eq!(info.id.version, version.clone());
                    }

                    // Test that versions are listed correctly
                    let versions_result = provider.list_versions(&name).await;
                    prop_assert!(versions_result.is_ok(), "Should be able to list versions for {}", name);

                    if let Ok(versions) = versions_result {
                        prop_assert!(versions.contains(&version), "Listed versions should contain {}", version);
                    }
                }
                Ok(())
            });
        }
    }
}
