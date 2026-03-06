//! # HPM Package
//!
//! Comprehensive package manifest processing and Houdini integration for HPM, providing
//! the foundation for understanding, validating, and managing Houdini package metadata
//! and dependencies with full integration support.
//!
//! ## Core Capabilities
//!
//! - **Package Manifest Processing**: Complete `hpm.toml` parsing, validation, and serialization
//! - **Houdini Integration**: Automated generation of `package.json` files from HPM manifests
//! - **Package Templates**: Standardized package structure generation
//! - **Dependency Management**: Support for Git and path dependencies, plus Python dependencies
//! - **Version Constraint Handling**: Full semantic versioning support
//!
//! ## Key Types
//!
//! - [`PackageManifest`] - Primary type representing a complete `hpm.toml` file
//! - [`DependencySpec`] - HPM dependency specifications (Git or Path)
//! - [`PythonDependencySpec`] - Python dependency specifications with extras
//! - [`HoudiniPackage`] - Generated `package.json` structure for Houdini
//! - [`PackageTemplate`] - Template system for generating package directories
//!
//! ## Quick Start
//!
//! ```rust
//! use hpm_package::{PackageManifest, DependencySpec};
//!
//! // Create a new package manifest
//! let manifest = PackageManifest::new(
//!     "geometry-tools".to_string(),
//!     "2.1.0".to_string(),
//!     Some("Advanced geometry tools for Houdini".to_string()),
//!     Some(vec!["Studio Artist <artist@studio.com>".to_string()]),
//!     Some("MIT".to_string()),
//! );
//!
//! // Validate the manifest
//! manifest.validate().expect("Manifest should be valid");
//!
//! // Serialize to TOML
//! let toml_content = toml::to_string(&manifest).expect("Should serialize");
//! ```
//!
//! ## Adding Dependencies
//!
//! ```rust
//! use hpm_package::{PackageManifest, DependencySpec, PythonDependencySpec};
//! use indexmap::IndexMap;
//!
//! let mut manifest = PackageManifest::new(
//!     "my-package".to_string(),
//!     "1.0.0".to_string(),
//!     None, None, None,
//! );
//!
//! // Add HPM dependencies
//! let mut deps = IndexMap::new();
//! deps.insert(
//!     "utility-nodes".to_string(),
//!     DependencySpec::url(
//!         "https://example.com/packages/utility-nodes/1.0.0/utility-nodes-1.0.0.zip",
//!         "1.0.0"
//!     )
//! );
//! manifest.dependencies = Some(deps);
//!
//! // Add Python dependencies
//! let mut py_deps = IndexMap::new();
//! py_deps.insert(
//!     "numpy".to_string(),
//!     PythonDependencySpec::Simple(">=1.20.0".to_string())
//! );
//! manifest.python_dependencies = Some(py_deps);
//! ```
//!
//! ## Generating Houdini package.json
//!
//! ```rust
//! use hpm_package::PackageManifest;
//!
//! let manifest = PackageManifest::new(
//!     "my-package".to_string(),
//!     "1.0.0".to_string(),
//!     None, None, None,
//! );
//!
//! // Generate Houdini-compatible package.json
//! let houdini_package = manifest.generate_houdini_package();
//!
//! let json = serde_json::to_string_pretty(&houdini_package)
//!     .expect("Should serialize to JSON");
//! ```

// Module declarations
pub mod dependency;
pub mod houdini;
pub mod manifest;
pub mod python;
pub mod template;

#[cfg(test)]
mod proptest_helpers;

#[cfg(all(test, feature = "fuzz"))]
mod fuzz_tests;

// Re-exports for convenient access
pub use dependency::DependencySpec;
pub use houdini::{HoudiniEnvValue, HoudiniPackage};
pub use manifest::{HoudiniConfig, PackageInfo, PackageManifest};
pub use python::PythonDependencySpec;
pub use template::PackageTemplate;
