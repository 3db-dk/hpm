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
//! - **Dependency Management**: Support for registry, URL, and path dependencies, plus Python dependencies
//! - **Platform-Aware Packaging**: Native platform declarations for multi-architecture archives
//! - **Version Constraint Handling**: Full semantic versioning support
//!
//! ## Key Types
//!
//! - [`PackageManifest`] - Primary type representing a complete `hpm.toml` file
//! - [`DependencySpec`] - HPM dependency specifications (Registry, URL, or Path)
//! - [`PythonDependencySpec`] - Python dependency specifications with extras
//! - [`HoudiniPackage`] - Generated `package.json` structure for HPM runtime
//! - [`HoudiniNativePackage`] - Houdini-native `package.json` for direct Houdini use
//! - [`Platform`] - Canonical platform identifiers for native packaging
//! - [`StageConfig`] - Staging configuration (output_dir, prepack, include/exclude, per-platform place rules)
//! - [`PackageTemplate`] - Template system for generating package directories
//!
//! ## Quick Start
//!
//! ```rust
//! use hpm_package::{PackageManifest, PackagePath, DependencySpec};
//!
//! // Create a new package manifest
//! let manifest = PackageManifest::new(
//!     PackagePath::new("studio/geometry-tools").unwrap(),
//!     "Geometry Tools".to_string(),
//!     "2.1.0".to_string(),
//!     Some("Advanced geometry tools for Houdini".to_string()),
//!     vec!["Studio Artist <artist@studio.com>".to_string()],
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
//! use hpm_package::{PackageManifest, PackagePath, DependencySpec, PythonDependencySpec};
//! use indexmap::IndexMap;
//!
//! let mut manifest = PackageManifest::new(
//!     PackagePath::new("studio/my-package").unwrap(),
//!     "My Package".to_string(),
//!     "1.0.0".to_string(),
//!     None,
//!     Vec::new(),
//!     None,
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
//! manifest.dependencies = deps;
//!
//! // Add Python dependencies
//! let mut py_deps = IndexMap::new();
//! py_deps.insert(
//!     "numpy".to_string(),
//!     PythonDependencySpec::Simple(">=1.20.0".to_string())
//! );
//! manifest.python_dependencies = py_deps;
//! ```
//!
//! ## Generating Houdini package.json
//!
//! ```rust
//! use hpm_package::{PackageManifest, PackagePath};
//!
//! let manifest = PackageManifest::new(
//!     PackagePath::new("studio/my-package").unwrap(),
//!     "My Package".to_string(),
//!     "1.0.0".to_string(),
//!     None,
//!     Vec::new(),
//!     None,
//! );
//!
//! // Generate Houdini-compatible package.json
//! let houdini_package = manifest.generate_houdini_package()
//!     .expect("validated manifest produces a Houdini package");
//!
//! let json = serde_json::to_string_pretty(&houdini_package)
//!     .expect("Should serialize to JSON");
//! ```

// Module declarations
pub mod dependency;
pub mod env_value;
pub mod houdini;
pub mod manifest;
pub mod package_path;
pub mod path_util;
pub mod platform;
pub mod python;
pub mod template;

// Re-exports for convenient access
pub use dependency::DependencySpec;
pub use env_value::{
    Condition, EnvValue, EnvValueBranch, ExpressionError, HoudiniRange, compile_condition,
    compile_houdini_req, houdini_req_has_upper_bound, houdini_req_lower_bound, lower_conditional,
};
pub use houdini::{HoudiniEnvValue, HoudiniNativePackage, HoudiniPackage, HpackageMetadata};
pub use manifest::{
    CompatConfig, EnvMethod, ManifestEnvEntry, ManifestLoadError, PackageInfo, PackageManifest,
    PackageScripts, PlaceRule, PlatformStaging, RegistryConfig, RegistryType, ScriptEntry,
    ScriptEnv, StageConfig, StagePlatformRules,
};
pub use package_path::{PackagePath, PackagePathError};
pub use platform::Platform;
pub use python::PythonDependencySpec;
pub use template::PackageTemplate;
