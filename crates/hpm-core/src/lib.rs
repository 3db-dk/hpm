//! # HPM Core
//!
//! The heart of the Houdini Package Manager, providing sophisticated package storage,
//! project discovery, dependency analysis, and cleanup operations with a focus on
//! safety, efficiency, and integration with complex Houdini workflows.
//!
//! ## Core Capabilities
//!
//! - **Project-Aware Storage Management**: Global package storage with intelligent cleanup that preserves dependencies needed by active projects
//! - **Advanced Project Discovery**: Configurable filesystem scanning with depth limits, ignore patterns, and validation
//! - **Dependency Graph Analysis**: Complete dependency resolution including transitive dependencies and cycle detection
//! - **Package Lifecycle Management**: High-level operations for installation, removal, and maintenance
//! - **Python Integration**: Seamless integration with HPM's Python dependency management system
//!
//! ## System Architecture
//!
//! HPM Core implements a layered architecture designed for reliability and performance:
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────────────┐
//! │                               HPM Core Architecture                             │
//! ├─────────────────────────────────────────────────────────────────────────────────┤
//! │                                                                                 │
//! │  Analysis & Discovery Layer                                                    │
//! │  ┌─────────────────────┐              ┌─────────────────────────────────────┐  │
//! │  │   ProjectDiscovery  │              │        DependencyGraph              │  │
//! │  │ • Filesystem Scan   │              │ • Transitive Dependencies           │  │
//! │  │ • Manifest Parse    │ ────────────▶│ • Cycle Detection                   │  │
//! │  │ • Project Validate  │              │ • Root Package Identification       │  │
//! │  └─────────────────────┘              └─────────────────────────────────────┘  │
//! │            │                                          │                         │
//! │            ▼                                          ▼                         │
//! │  Storage & Cleanup Layer                                                       │
//! │  ┌─────────────────────────────────────────────────────────────────────────┐   │
//! │  │                        StorageManager                                   │   │
//! │  │ • Global Package Storage (~/.hpm/packages/)                            │   │
//! │  │ • Project-Aware Cleanup (Orphan Detection)                             │   │
//! │  │ • Python Virtual Environment Integration                               │   │
//! │  │ • Content-Addressable Package Organization                             │   │
//! │  └─────────────────────────────────────────────────────────────────────────┘   │
//! │                                    │                                           │
//! │                                    ▼                                           │
//! │  File System Layer                                                             │
//! │  ┌─────────────────────────────────────────────────────────────────────────┐   │
//! │  │ ~/.hpm/                                                                 │   │
//! │  │ ├── packages/           # Global package storage                        │   │
//! │  │ │   ├── package-a@1.0.0/                                               │   │
//! │  │ │   └── package-b@2.1.0/                                               │   │
//! │  │ ├── cache/              # Registry and dependency cache                │   │
//! │  │ ├── venvs/              # Python virtual environments                  │   │
//! │  │ └── registry/           # Registry index cache                         │   │
//! │  └─────────────────────────────────────────────────────────────────────────┘   │
//! └─────────────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Core Components
//!
//! ### Storage Management ([`StorageManager`])
//! The storage manager implements HPM's dual-storage architecture:
//!
//! - **Global Storage**: All packages installed in `~/.hpm/packages/` with versioned directories
//! - **Project Integration**: Generated Houdini package manifests in `.hmp/packages/` that reference global storage
//! - **Deduplication**: Multiple projects can use the same package version without duplication
//! - **Safety**: Project-aware cleanup prevents removing packages needed by active projects
//!
//! ### Project Discovery ([`ProjectDiscovery`])
//! Advanced filesystem scanning for HPM-managed projects:
//!
//! - **Configurable Scanning**: Explicit paths, search roots, depth limits, ignore patterns
//! - **Manifest Validation**: Ensures discovered projects have valid `hpm.toml` files
//! - **Performance Optimization**: Intelligent traversal with early termination
//! - **Error Resilience**: Continues scanning despite individual project errors
//!
//! ### Dependency Analysis ([`DependencyGraph`])
//! Sophisticated dependency modeling and analysis:
//!
//! - **Transitive Resolution**: Follows complete dependency chains to identify all required packages
//! - **Cycle Detection**: Identifies and warns about circular dependencies
//! - **Root Identification**: Distinguishes between directly required and transitive packages
//! - **Reachability Analysis**: Efficiently determines which packages are needed by active projects
//!
//! ## Safety Guarantees
//!
//! HPM Core provides strong safety guarantees for package operations:
//!
//! ### 1. No Orphan False Positives
//! The cleanup system will never remove a package that is required by an active project,
//! even through complex transitive dependency chains.
//!
//! ### 2. Project Discovery Validation
//! All discovered projects are validated for proper `hmp.toml` structure before being
//! included in dependency analysis.
//!
//! ### 3. Atomic Operations
//! Package installation and removal operations are designed to be atomic - they either
//! complete successfully or leave the system in a consistent state.
//!
//! ### 4. Comprehensive Error Handling
//! All operations provide detailed error information with context for troubleshooting.
//!
//! ## Usage Patterns
//!
//! ### Basic Package Storage Operations
//!
//! ```rust
//! use hpm_core::StorageManager;
//! use hpm_config::Config;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let config = Config::default();
//! let storage = StorageManager::new(config.storage)?;
//!
//! // Check if package exists
//! if storage.package_exists("utility-nodes", "2.1.0") {
//!     println!("Package is already installed");
//! }
//!
//! // List all installed packages
//! let packages = storage.list_installed()?;
//! for package in packages {
//!     println!("Installed: {} v{}", package.name, package.version);
//! }
//! # Ok(())
//! # }
//! ```
//!
//! ### Project Discovery and Analysis
//!
//! ```rust
//! use hpm_core::{ProjectDiscovery, DependencyResolver};
//! use hpm_config::ProjectsConfig;
//! use std::sync::Arc;
//!
//! # async fn discovery_example() -> Result<(), Box<dyn std::error::Error>> {
//! let mut projects_config = ProjectsConfig::default();
//! projects_config.add_search_root("/Users/artist/houdini-projects".into());
//!
//! let discovery = ProjectDiscovery::new(projects_config);
//! let projects = discovery.find_projects()?;
//!
//! println!("Found {} HPM-managed projects", projects.len());
//!
//! // Analyze dependencies across all projects
//! # let storage = todo!(); // StorageManager instance
//! let resolver = DependencyResolver::new(Arc::new(storage));
//! let dependency_graph = resolver.build_dependency_graph(&projects).await?;
//!
//! println!("Dependency graph built successfully");
//! # Ok(())
//! # }
//! ```
//!
//! ### Safe Package Cleanup
//!
//! ```rust
//! use hpm_core::StorageManager;
//! use hpm_config::Config;
//!
//! # async fn cleanup_example() -> Result<(), Box<dyn std::error::Error>> {
//! let config = Config::default();
//! let storage = StorageManager::new(config.storage)?;
//!
//! // Dry run to preview cleanup
//! let would_remove = storage.cleanup_unused_dry_run(&config.projects).await?;
//! println!("Would remove {} orphaned packages:", would_remove.len());
//! for package in &would_remove {
//!     println!("  - {}", package);
//! }
//!
//! // Perform actual cleanup if desired
//! if !would_remove.is_empty() {
//!     let removed = storage.cleanup_unused(&config.projects).await?;
//!     println!("Successfully removed {} packages", removed.len());
//! }
//! # Ok(())
//! # }
//! ```
//!
//! ### Comprehensive Cleanup (Packages + Python)
//!
//! ```rust
//! use hpm_core::StorageManager;
//! use hpm_config::Config;
//!
//! # async fn comprehensive_cleanup() -> Result<(), Box<dyn std::error::Error>> {
//! let config = Config::default();
//! let storage = StorageManager::new(config.storage)?;
//!
//! // Clean both packages and Python virtual environments
//! let result = storage.cleanup_comprehensive(&config.projects).await?;
//!
//! println!("Cleanup completed:");
//! println!("  Packages removed: {}", result.removed_packages.len());
//! println!("  Virtual environments cleaned: {}", result.python_cleanup.items_cleaned());
//! println!("  Total space freed: {}", result.format_total_space_freed());
//! # Ok(())
//! # }
//! ```
//!
//! ## Integration Points
//!
//! ### Python Dependency Management
//! HPM Core integrates seamlessly with the Python dependency system:
//!
//! - Cleanup operations consider Python virtual environment usage
//! - Package removal includes cleanup of associated Python dependencies
//! - Comprehensive cleanup handles both HPM packages and Python virtual environments
//!
//! ### Registry Integration
//! The core storage system is designed to work with the HPM registry:
//!
//! - Package installation interfaces with registry clients
//! - Dependency resolution integrates with registry metadata
//! - Caching systems optimize registry interactions
//!
//! ### Houdini Integration
//! Core operations support Houdini-specific requirements:
//!
//! - Project discovery recognizes Houdini project structures
//! - Package organization supports Houdini's package loading mechanisms
//! - Generated manifests integrate with Houdini's environment system

pub mod archive_fetcher;
pub mod dependency;
pub mod discovery;
pub mod lock;
pub mod package_source;
pub mod project;
pub mod registry;
pub mod storage;
pub mod tag_resolver;

#[cfg(test)]
mod integration_test;

#[cfg(all(test, feature = "fuzz"))]
mod fuzz_tests;

pub use archive_fetcher::{ArchiveFetcher, FetchError, FetchResult};
pub use dependency::{
    DependencyError, DependencyGraph, DependencyResolver, PackageId, PackageNode,
};
pub use discovery::{DiscoveredProject, DiscoveryError, ProjectDiscovery};
pub use lock::{
    LockError, LockFile, LockMetadata, LockPackageInfo, LockedDependency, LockedPythonDependency,
};
pub use package_source::{GitProvider, PackageSource, PackageSourceError};
pub use project::{ProjectDependency, ProjectError, ProjectManager};
pub use registry::{
    ApiRegistry, GitRegistry, Registry, RegistryEntry, RegistryError, RegistrySet, SearchResults,
};
pub use storage::{StorageError, StorageManager};
pub use tag_resolver::{TagResolveError, TagResolver};
