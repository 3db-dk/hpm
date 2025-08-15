//! # HPM Core
//!
//! Core functionality for the Houdini Package Manager, providing:
//!
//! - **Package Storage Management**: Global package storage with project-aware cleanup
//! - **Project Discovery**: Filesystem scanning and HPM project detection
//! - **Dependency Resolution**: Dependency graph construction and analysis
//! - **Package Management**: High-level operations for package lifecycle
//!
//! ## Architecture
//!
//! The core module is designed around these key components:
//!
//! - [`StorageManager`] - Manages global package storage in `~/.hpm/`
//! - [`ProjectDiscovery`] - Finds and validates HPM projects across filesystem
//! - [`DependencyGraph`] - Models and analyzes package dependency relationships
//! - [`PackageManager`] - Provides high-level package management operations
//! - [`ProjectManager`] - Handles project-specific operations and Houdini integration
//!
//! ## Key Features
//!
//! - **Project-Aware Cleanup**: Safely removes orphaned packages while preserving dependencies
//! - **Transitive Dependencies**: Follows complete dependency chains for accurate analysis
//! - **Configurable Discovery**: Flexible project search with depth limits and ignore patterns
//! - **Content-Addressable Storage**: Efficient storage with deduplication
//!
//! ## Examples
//!
//! ```rust
//! use hpm_core::{StorageManager, ProjectDiscovery};
//! use hpm_config::Config;
//!
//! # fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Initialize storage and discovery
//! let config = Config::default();
//! let storage = StorageManager::new(config.storage.clone())?;
//! let discovery = ProjectDiscovery::new(config.projects.clone());
//!
//! // Discover all HPM projects
//! let projects = discovery.find_projects()?;
//! println!("Found {} HPM projects", projects.len());
//!
//! // Analyze storage and find orphaned packages
//! let installed = storage.list_installed()?;
//! println!("Installed packages: {}", installed.len());
//! # Ok(())
//! # }
//! ```

pub mod dependency;
pub mod discovery;
pub mod integration_test;
pub mod manager;
pub mod project;
pub mod storage;

pub use dependency::{
    DependencyError, DependencyGraph, DependencyResolver, PackageId, PackageNode,
};
pub use discovery::{DiscoveredProject, DiscoveryError, ProjectDiscovery};
pub use manager::PackageManager;
pub use project::{ProjectDependency, ProjectError, ProjectManager};
pub use storage::{StorageError, StorageManager};
