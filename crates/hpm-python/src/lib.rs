//! HPM Python Dependency Management
//!
//! This crate provides comprehensive Python dependency management for HPM packages,
//! including virtual environment isolation, dependency resolution, and Houdini integration.
//!
//! ## Features
//!
//! - **Virtual Environment Isolation**: Content-addressable virtual environments based on dependency hashes
//! - **Dependency Resolution**: UV-powered dependency resolution for optimal performance
//! - **Conflict Detection**: Automatic detection and reporting of dependency conflicts
//! - **Houdini Integration**: Seamless integration with Houdini's package system via PYTHONPATH injection
//! - **Complete UV Isolation**: Bundled UV binary with isolated cache and configuration
//! - **Cleanup Management**: Intelligent cleanup of orphaned virtual environments
//!
//! ## Architecture
//!
//! The system uses hash-based virtual environment sharing where packages with identical
//! resolved dependencies share the same virtual environment, optimizing disk usage and
//! installation time.
//!
//! ## Example Usage
//!
//! ```rust,no_run
//! use hpm_python::{initialize, collect_python_dependencies, resolve_dependencies, VenvManager};
//! use hpm_package::PackageManifest;
//!
//! # async fn example() -> anyhow::Result<()> {
//! // Initialize the Python dependency system
//! initialize().await?;
//!
//! // Collect dependencies from package manifests
//! let packages: Vec<PackageManifest> = vec![];
//! let deps = collect_python_dependencies(&packages).await?;
//!
//! // Resolve to exact versions
//! let resolved = resolve_dependencies(&deps).await?;
//!
//! // Ensure virtual environment exists
//! let venv_manager = VenvManager::new();
//! let venv_path = venv_manager.ensure_virtual_environment(&resolved).await?;
//! # Ok(())
//! # }
//! ```

pub mod bundled;
pub mod cleanup;
pub mod dependency;
pub mod integration;
pub mod resolver;
pub mod types;
pub mod venv;

#[cfg(test)]
pub mod integration_tests;

pub use dependency::*;
pub use integration::*;
pub use resolver::*;
pub use types::*;
pub use venv::*;

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
