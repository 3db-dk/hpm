//! HPM Python Dependency Management
//!
//! This crate provides comprehensive Python dependency management for HPM packages,
//! solving the fundamental challenge of conflicting Python dependencies across multiple
//! packages through advanced virtual environment isolation and content-addressable sharing.
//!
//! ## Core Features
//!
//! - **Content-Addressable Virtual Environments**: Packages with identical resolved dependencies share virtual environments, optimizing disk usage and installation time
//! - **UV-Powered Resolution**: High-performance dependency resolution using bundled UV binary with complete isolation
//! - **Conflict Detection**: Automatic detection and reporting of dependency conflicts with detailed resolution suggestions
//! - **Houdini Integration**: Seamless integration with Houdini's package system via automated PYTHONPATH injection
//! - **Intelligent Cleanup**: Orphaned virtual environment detection and removal with safety guarantees
//! - **Houdini Version Mapping**: Automatic mapping of Houdini versions to compatible Python versions
//!
//! ## System Architecture
//!
//! The Python dependency system implements a sophisticated content-addressable architecture:
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────────────┐
//! │                            HPM Python Architecture                              │
//! ├─────────────────────────────────────────────────────────────────────────────────┤
//! │                                                                                 │
//! │  Package Manifests (hpm.toml)                                                  │
//! │  ┌─────────────────┐  ┌─────────────────┐  ┌─────────────────┐               │
//! │  │   Package A     │  │   Package B     │  │   Package C     │               │
//! │  │ numpy>=1.20.0   │  │ numpy>=1.20.0   │  │ scipy>=1.7.0    │               │
//! │  │ requests^2.28   │  │ requests^2.28   │  │ matplotlib^3.5  │               │
//! │  └─────────────────┘  └─────────────────┘  └─────────────────┘               │
//! │           │                      │                      │                     │
//! │           ▼                      ▼                      ▼                     │
//! │                                                                                 │
//! │  Dependency Collection & Resolution (via UV)                                   │
//! │  ┌─────────────────────────────────────────────────────────────────┐          │
//! │  │  Resolved Sets:                                                 │          │
//! │  │  • Set 1: numpy==1.24.0, requests==2.28.0 → Hash: a1b2c3d4    │          │
//! │  │  • Set 2: scipy==1.9.0, matplotlib==3.6.0 → Hash: e5f6g7h8    │          │
//! │  └─────────────────────────────────────────────────────────────────┘          │
//! │                           │                      │                             │
//! │                           ▼                      ▼                             │
//! │                                                                                 │
//! │  Content-Addressable Virtual Environments (~/.hpm/venvs/)                     │
//! │  ┌─────────────────┐                    ┌─────────────────┐                   │
//! │  │  VEnv a1b2c3d4  │                    │  VEnv e5f6g7h8  │                   │
//! │  │ ├─ numpy 1.24.0 │  ◄─── Shared ───► │ ├─ scipy 1.9.0  │                   │
//! │  │ ├─ requests 2.28│       by A & B     │ ├─ matplotlib   │                   │
//! │  │ └─ metadata.json│                    │ └─ metadata.json│                   │
//! │  └─────────────────┘                    └─────────────────┘                   │
//! │                                                                                 │
//! │  Houdini Integration (Generated package.json files)                           │
//! │  ┌─────────────────────────────────────────────────────────────────┐          │
//! │  │  env: [                                                         │          │
//! │  │    "PYTHONPATH": "/path/to/venv/a1b2c3d4/lib/python/site-packages"        │
//! │  │  ]                                                              │          │
//! │  └─────────────────────────────────────────────────────────────────┘          │
//! └─────────────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Key Benefits
//!
//! ### 1. Content-Addressable Sharing
//! Multiple packages with identical resolved dependencies share the same virtual environment,
//! dramatically reducing disk usage and installation time. The system calculates a content
//! hash from the exact resolved package versions and Python version.
//!
//! ### 2. Complete UV Isolation
//! HPM bundles its own UV binary and maintains complete isolation from any system UV
//! installation, preventing conflicts and ensuring reproducible behavior across environments.
//!
//! ### 3. Houdini Version Compatibility
//! Automatic mapping between Houdini versions and Python versions ensures packages work
//! correctly with their target Houdini installations:
//!
//! | Houdini Version | Python Version | Notes                    |
//! |----------------|----------------|--------------------------|
//! | 19.0 - 19.5    | Python 3.7     | Legacy support           |
//! | 20.0           | Python 3.9     | Current stable           |
//! | 20.5           | Python 3.10    | Enhanced performance     |
//! | 21.x           | Python 3.11    | Latest features          |
//!
//! ## Module Organization
//!
//! - [`bundled`] - UV binary management and isolated execution
//! - [`venv`] - Virtual environment creation, management, and sharing
//! - [`dependency`] - Dependency collection and conflict detection
//! - [`resolver`] - UV-powered dependency resolution
//! - [`integration`] - Houdini package.json generation and PYTHONPATH setup
//! - [`cleanup`] - Orphaned virtual environment detection and cleanup
//! - [`types`] - Core types for Python versions, dependencies, and metadata
//! - [`update`] - Update management for Python dependencies
//!
//! ## Quick Start Example
//!
//! ```rust,no_run
//! use hpm_python::{initialize, collect_python_dependencies, resolve_dependencies, VenvManager};
//! use hpm_package::PackageManifest;
//!
//! # async fn example() -> anyhow::Result<()> {
//! // 1. Initialize the Python dependency system
//! initialize().await?;
//!
//! // 2. Collect dependencies from package manifests
//! let packages: Vec<PackageManifest> = vec![]; // Your package manifests
//! let collected_deps = collect_python_dependencies(&packages).await?;
//!
//! // 3. Resolve to exact versions with conflict detection
//! let resolved_sets = resolve_dependencies(&collected_deps).await?;
//!
//! // 4. Create virtual environment for the resolved dependencies
//! let venv_manager = VenvManager::new();
//! let venv_path = venv_manager.ensure_virtual_environment(&resolved_sets).await?;
//! println!("Virtual environment ready at: {:?}", venv_path);
//! # Ok(())
//! # }
//! ```
//!
//! ## Advanced Usage Patterns
//!
//! ### Cleanup Management
//! ```rust,no_run
//! use hpm_python::cleanup::PythonCleanupAnalyzer;
//!
//! # async fn cleanup_example() -> anyhow::Result<()> {
//! let analyzer = PythonCleanupAnalyzer::new();
//! let active_packages = vec!["package-a@1.0.0".to_string()];
//!
//! // Find orphaned virtual environments
//! let orphaned = analyzer.analyze_orphaned_venvs(&active_packages).await?;
//!
//! // Preview cleanup (dry run)
//! let result = analyzer.cleanup_orphaned_venvs(&orphaned, true).await?;
//! println!("Would clean {} venvs, freeing {}",
//!     result.items_that_would_be_cleaned(),
//!     result.format_space_that_would_be_freed()
//! );
//! # Ok(())
//! # }
//! ```
//!
//! ### Conflict Resolution
//! ```rust,no_run
//! use hpm_python::resolve_dependencies;
//!
//! # async fn conflict_example() -> anyhow::Result<()> {
//! # use hpm_python::types::PythonDependencies;
//! # let collected_deps = PythonDependencies::default();
//! match resolve_dependencies(&collected_deps).await {
//!     Ok(resolved) => println!("Resolution successful: {} packages", resolved.packages.len()),
//!     Err(e) => {
//!         println!("Dependency conflict detected: {}", e);
//!     }
//! }
//! # Ok(())
//! # }
//! ```

pub mod bundled;
pub mod cleanup;
pub mod dependency;
pub mod integration;
pub mod resolver;
pub mod types;
pub mod update;
pub mod venv;

#[cfg(test)]
pub mod integration_tests;

// Dependency collection
pub use dependency::collect_python_dependencies;

// Houdini integration
pub use integration::{
    extract_python_env_from_package_json, generate_houdini_package_json,
    update_package_json_with_python,
};

// UV resolver
pub use resolver::resolve_dependencies;

// Core types
pub use types::{
    OrphanedVenv, PythonDependencies, PythonDependency, PythonError, PythonResult, PythonVersion,
    ResolvedDependencySet, VenvMetadata, VersionSpec,
};

// Virtual environment management
pub use venv::VenvManager;

use anyhow::Result;
use std::path::PathBuf;

/// Initialize Python dependency management system
///
/// This function must be called before using any Python dependency management features.
/// It ensures the UV binary is available and properly configured for HPM's isolated environment.
///
/// The initialization process:
/// 1. Checks for and extracts the bundled UV binary if needed
/// 2. Sets up UV environment variables for complete isolation
/// 3. Creates necessary directory structure
///
/// # Errors
///
/// Returns an error if UV binary extraction fails or if the directory structure cannot be created.
///
/// # Example
///
/// ```rust,no_run
/// # async fn example() -> anyhow::Result<()> {
/// hpm_python::initialize().await?;
/// // Python dependency management is now ready to use
/// # Ok(())
/// # }
/// ```
pub async fn initialize() -> Result<()> {
    bundled::ensure_uv_binary().await?;
    Ok(())
}

/// Get the HPM Python cache directory
///
/// Returns the directory where UV stores its package cache. This is isolated from
/// any system UV installation to prevent interference.
///
/// Default location: `~/.hpm/uv-cache/`
///
/// # Returns
///
/// PathBuf pointing to the UV cache directory within HPM's managed directory structure.
pub fn get_python_cache_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".hpm")
        .join("uv-cache")
}

/// Get the HPM Python configuration directory
///
/// Returns the directory where UV configuration files are stored. This ensures
/// UV's configuration is completely isolated from any system installation.
///
/// Default location: `~/.hpm/uv-config/`
///
/// # Returns
///
/// PathBuf pointing to the UV configuration directory within HPM's managed directory structure.
pub fn get_python_config_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".hpm")
        .join("uv-config")
}

/// Get the HPM virtual environments directory
///
/// Returns the directory where all Python virtual environments are stored.
/// Virtual environments are organized by content hash for sharing between packages
/// with identical resolved dependencies.
///
/// Default location: `~/.hpm/venvs/`
///
/// # Returns
///
/// PathBuf pointing to the virtual environments directory within HPM's managed directory structure.
///
/// # Directory Structure
///
/// ```text
/// ~/.hpm/venvs/
/// ├── a1b2c3d4/          # Virtual environment with hash a1b2c3d4
/// │   ├── metadata.json  # Environment metadata and package references
/// │   ├── lib/           # Python packages
/// │   └── ...
/// └── e5f6g7h8/          # Another virtual environment
///     └── ...
/// ```
pub fn get_venvs_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".hpm")
        .join("venvs")
}
