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
    #[allow(dead_code)] // Will be used in future conflict resolution improvements
    reason: AssignmentReason,
}

#[derive(Debug, Clone)]
#[allow(dead_code)] // Will be used in future conflict resolution improvements
enum AssignmentReason {
    Root,
    Dependency { from: PackageId },
    Conflict { incompatibility_index: usize },
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
            reason: AssignmentReason::Root, // Simplified for now
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
}
