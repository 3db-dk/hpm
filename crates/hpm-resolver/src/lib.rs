//! HPM Dependency Resolver
//!
//! This crate provides a high-performance dependency resolution algorithm for HPM (Houdini Package Manager).
//! The resolver is inspired by PubGrub, the same algorithm used by UV for Python packages, ensuring
//! optimal performance and correctness in complex dependency scenarios.
//!
//! # Overview
//!
//! The dependency resolver takes a set of root requirements and produces a complete, consistent
//! set of package versions that satisfy all constraints. It handles version specifications,
//! transitive dependencies, and conflict resolution automatically.
//!
//! # Algorithm Details
//!
//! ## PubGrub-Inspired Approach
//!
//! The resolver uses an incremental approach with the following key components:
//!
//! 1. **Partial Solutions**: Maintains packages in three states: decided, undecided, and conflicted
//! 2. **Priority-Based Selection**: Processes packages with stricter constraints first for optimal performance
//! 3. **Conflict Learning**: Remembers incompatible package combinations to avoid repeated failures
//! 4. **Backtracking**: Intelligently backtracks when conflicts are discovered
//! 5. **Incompatibility Tracking**: Records why certain package combinations cannot coexist
//!
//! ## Performance Optimizations
//!
//! - **Incremental Solving**: Only recalculates affected parts of the solution tree
//! - **Smart Prioritization**: Exact versions (==) → Strict constraints (^, ~) → Loose constraints (>=)
//! - **Early Termination**: Stops as soon as a complete solution is found
//! - **Conflict Caching**: Avoids re-exploring known incompatible states
//! - **Lazy Evaluation**: Only fetches package metadata when needed
//!
//! # Basic Usage
//!
//! ```rust,ignore
//! use hpm_resolver::{
//!     DependencyResolver, RegistryProvider, ResolverConfig,
//!     Requirement, Priority, Version, VersionConstraint
//! };
//!
//! # async fn example() -> anyhow::Result<()> {
//! // Create a provider that can fetch package information
//! # let my_registry_client = todo!();
//! let provider = RegistryProvider::new(Box::new(my_registry_client));
//!
//! // Configure the resolver
//! let config = ResolverConfig {
//!     prefer_latest: true,
//!     allow_prereleases: false,
//!     max_backtrack_iterations: 1000,
//!     resolution_timeout_secs: 300,
//! };
//!
//! // Create the resolver
//! let mut resolver = DependencyResolver::new(provider, config);
//!
//! // Define root requirements
//! let requirements = vec![
//!     Requirement {
//!         name: "geometry-tools".to_string(),
//!         constraint: VersionConstraint::Compatible(Version::new(2, 1, 0)),
//!         source_package: None,
//!         priority: Priority::Root,
//!     },
//!     Requirement {
//!         name: "mesh-utilities".to_string(),
//!         constraint: VersionConstraint::GreaterThanOrEqual(Version::new(1, 5, 0)),
//!         source_package: None,
//!         priority: Priority::Root,
//!     },
//! ];
//!
//! // Resolve dependencies
//! let resolution = resolver.resolve(requirements).await?;
//!
//! // Access resolved packages
//! for (name, package_id) in &resolution.packages {
//!     println!("{} resolved to {}", name, package_id.version);
//! }
//!
//! // Check resolution metadata
//! println!("Resolved {} packages in {}ms",
//!     resolution.metadata.total_packages,
//!     resolution.metadata.resolution_time_ms
//! );
//! # Ok(())
//! # }
//! ```
//!
//! # Advanced Features
//!
//! ## Custom Package Providers
//!
//! Implement the `PackageProvider` trait to integrate with different package sources:
//!
//! ```rust,ignore
//! use hpm_resolver::{PackageProvider, PackageInfo, Version};
//! use anyhow::Result;
//!
//! struct MyCustomProvider {
//!     // Your package source implementation
//! }
//!
//! #[async_trait::async_trait]
//! impl PackageProvider for MyCustomProvider {
//!     async fn get_package_info(&mut self, name: &str, version: &Version) -> Result<PackageInfo> {
//!         // Fetch package information from your source
//!         todo!()
//!     }
//!
//!     async fn list_versions(&mut self, name: &str) -> Result<Vec<Version>> {
//!         // List all available versions for a package
//!         todo!()
//!     }
//!
//!     async fn get_latest_version(&mut self, name: &str) -> Result<Option<Version>> {
//!         // Get the latest version of a package
//!         todo!()
//!     }
//! }
//! ```
//!
//! ## Version Constraints
//!
//! The resolver supports comprehensive version constraint specifications:
//!
//! - `VersionConstraint::Exact(v)` - Exact version match (==1.2.3)
//! - `VersionConstraint::Compatible(v)` - Compatible version range (^1.2.3)
//! - `VersionConstraint::Tilde(v)` - Tilde version range (~1.2.3)
//! - `VersionConstraint::GreaterThan(v)` - Greater than (>1.2.3)
//! - `VersionConstraint::GreaterThanOrEqual(v)` - Greater than or equal (>=1.2.3)
//! - `VersionConstraint::LessThan(v)` - Less than (<1.2.3)
//! - `VersionConstraint::LessThanOrEqual(v)` - Less than or equal (<=1.2.3)
//! - `VersionConstraint::Range(range)` - Custom range
//! - `VersionConstraint::Any` - Any version (*)
//!
//! ## Error Handling
//!
//! The resolver provides detailed error information for debugging:
//!
//! ```rust,ignore
//! use hpm_resolver::{ResolverError};
//!
//! # async fn example() -> anyhow::Result<()> {
//! # let resolver = todo!();
//! # let requirements = vec![];
//! match resolver.resolve(requirements).await {
//!     Ok(resolution) => {
//!         // Success - use the resolution
//!     },
//!     Err(e) => match e.downcast_ref::<ResolverError>() {
//!         Some(ResolverError::NoSolution) => {
//!             println!("No solution found - constraints may be incompatible");
//!         },
//!         Some(ResolverError::PackageNotFound { name }) => {
//!             println!("Package '{}' not found in registry", name);
//!         },
//!         Some(ResolverError::VersionConflict { package, constraint1, constraint2 }) => {
//!             println!("Version conflict for {}: {} vs {}", package, constraint1, constraint2);
//!         },
//!         _ => {
//!             println!("Other error: {}", e);
//!         }
//!     }
//! }
//! # Ok(())
//! # }
//! ```
//!
//! # Performance Characteristics
//!
//! The resolver is optimized for real-world package management scenarios:
//!
//! - **Time Complexity**: O(packages × versions × constraints) in the worst case, but typically much better due to pruning
//! - **Space Complexity**: O(packages × incompatibilities) for conflict learning
//! - **Typical Performance**: Resolves hundreds of packages with complex constraints in milliseconds
//! - **Scaling**: Handles large dependency graphs (1000+ packages) efficiently
//!
//! # Integration with HPM
//!
//! This resolver is designed specifically for HPM but can be adapted for other package managers.
//! It integrates seamlessly with:
//!
//! - HPM registry protocol via `RegistryProvider`
//! - HPM package manifests via `PackageInfo` structures  
//! - HPM update commands for dependency graph updates
//! - HPM conflict resolution for user-friendly error reporting

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use thiserror::Error;

pub mod solver;
pub mod version;

pub use solver::DependencyResolver;
pub use version::{Version, VersionConstraint, VersionRange};

/// A package identifier with name and version
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PackageId {
    pub name: String,
    pub version: Version,
}

impl PackageId {
    pub fn new(name: String, version: Version) -> Self {
        Self { name, version }
    }

    pub fn identifier(&self) -> String {
        format!("{}@{}", self.name, self.version)
    }
}

impl fmt::Display for PackageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}@{}", self.name, self.version)
    }
}

/// Package dependency specification
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dependency {
    pub name: String,
    pub constraint: VersionConstraint,
    pub optional: bool,
    pub source: DependencySource,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DependencySource {
    Registry,
    Git { url: String, branch: Option<String> },
    Path { path: String },
}

/// Package information available in the registry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageInfo {
    pub id: PackageId,
    pub dependencies: Vec<Dependency>,
    pub description: Option<String>,
    pub houdini: Option<HoudiniRequirements>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HoudiniRequirements {
    pub min_version: Option<Version>,
    pub max_version: Option<Version>,
    pub platforms: Vec<String>,
}

/// Priority levels for package resolution
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    Root = 0,       // Root/direct dependencies
    Exact = 1,      // Exact version constraints (==)
    Strict = 2,     // Strict constraints (~, ^)
    Loose = 3,      // Loose constraints (>=, *)
    Transitive = 4, // Transitive dependencies
}

/// A requirement for a package with constraints
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Requirement {
    pub name: String,
    pub constraint: VersionConstraint,
    pub source_package: Option<PackageId>,
    pub priority: Priority,
}

/// Resolution result containing solved packages
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Resolution {
    pub packages: BTreeMap<String, PackageId>,
    pub metadata: ResolutionMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolutionMetadata {
    pub resolver_version: String,
    pub resolution_time_ms: u64,
    pub total_packages: usize,
    pub conflicts_resolved: usize,
}

/// Incompatibility representing conflicting package combinations
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Incompatibility {
    pub terms: Vec<Term>,
    pub cause: IncompatibilityCause,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Term {
    pub package: String,
    pub constraint: VersionConstraint,
    pub positive: bool, // true = must include, false = must exclude
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IncompatibilityCause {
    Dependency,
    NoVersions,
    Conflict,
    Root,
}

/// Trait for querying package information from a registry
#[async_trait::async_trait]
pub trait PackageProvider {
    async fn get_package_info(&mut self, name: &str, version: &Version) -> Result<PackageInfo>;
    async fn list_versions(&mut self, name: &str) -> Result<Vec<Version>>;
    async fn get_latest_version(&mut self, name: &str) -> Result<Option<Version>>;
}

/// Registry-based package provider
pub struct RegistryProvider {
    client: Box<dyn RegistryClient>,
}

#[async_trait::async_trait]
pub trait RegistryClient: Send + Sync {
    async fn fetch_package_info(&mut self, name: &str, version: &Version) -> Result<PackageInfo>;
    async fn fetch_package_versions(&mut self, name: &str) -> Result<Vec<Version>>;
}

impl RegistryProvider {
    pub fn new(client: Box<dyn RegistryClient>) -> Self {
        Self { client }
    }
}

#[async_trait::async_trait]
impl PackageProvider for RegistryProvider {
    async fn get_package_info(&mut self, name: &str, version: &Version) -> Result<PackageInfo> {
        self.client.fetch_package_info(name, version).await
    }

    async fn list_versions(&mut self, name: &str) -> Result<Vec<Version>> {
        self.client.fetch_package_versions(name).await
    }

    async fn get_latest_version(&mut self, name: &str) -> Result<Option<Version>> {
        let versions = self.list_versions(name).await?;
        Ok(versions.into_iter().max())
    }
}

/// Errors that can occur during resolution
#[derive(Debug, Error)]
pub enum ResolverError {
    #[error("No solution found for dependency constraints")]
    NoSolution,

    #[error("Package not found: {name}")]
    PackageNotFound { name: String },

    #[error("No versions available for package: {name}")]
    NoVersionsAvailable { name: String },

    #[error("Version constraint conflict for {package}: {constraint1} vs {constraint2}")]
    VersionConflict {
        package: String,
        constraint1: String,
        constraint2: String,
    },

    #[error("Circular dependency detected: {cycle}")]
    CircularDependency { cycle: String },

    #[error("Registry error: {0}")]
    Registry(#[from] anyhow::Error),
}

/// Configuration options for the resolver
#[derive(Debug, Clone)]
pub struct ResolverConfig {
    pub prefer_latest: bool,
    pub allow_prereleases: bool,
    pub max_backtrack_iterations: usize,
    pub resolution_timeout_secs: u64,
}

impl Default for ResolverConfig {
    fn default() -> Self {
        Self {
            prefer_latest: true,
            allow_prereleases: false,
            max_backtrack_iterations: 1000,
            resolution_timeout_secs: 300, // 5 minutes
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_package_id_creation() {
        let version = Version::new(1, 2, 3);
        let package_id = PackageId::new("test-package".to_string(), version.clone());

        assert_eq!(package_id.name, "test-package");
        assert_eq!(package_id.version, version);
        assert_eq!(package_id.identifier(), "test-package@1.2.3");
    }

    #[test]
    fn test_package_id_ordering() {
        let pkg1 = PackageId::new("a".to_string(), Version::new(1, 0, 0));
        let pkg2 = PackageId::new("b".to_string(), Version::new(1, 0, 0));
        let pkg3 = PackageId::new("a".to_string(), Version::new(2, 0, 0));

        assert!(pkg1 < pkg2);
        assert!(pkg1 < pkg3);
        assert!(pkg2 > pkg1);
    }

    #[test]
    fn test_priority_ordering() {
        assert!(Priority::Root < Priority::Exact);
        assert!(Priority::Exact < Priority::Strict);
        assert!(Priority::Strict < Priority::Loose);
        assert!(Priority::Loose < Priority::Transitive);
    }

    #[test]
    fn test_resolver_config_default() {
        let config = ResolverConfig::default();
        assert!(config.prefer_latest);
        assert!(!config.allow_prereleases);
        assert_eq!(config.max_backtrack_iterations, 1000);
        assert_eq!(config.resolution_timeout_secs, 300);
    }
}
