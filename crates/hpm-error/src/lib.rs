//! HPM Error Handling
//!
//! This crate provides a comprehensive error handling system for HPM, implementing
//! structured error types with rich context information and user-friendly error
//! messages. The error system is designed to provide maximum information for
//! debugging while maintaining clarity for end users.
//!
//! ## Error Architecture
//!
//! HPM uses a hierarchical error system where different modules define their own
//! error types, which are then aggregated into a central [`HpmError`] enum:
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────────────┐
//! │                           HPM Error Hierarchy                                   │
//! ├─────────────────────────────────────────────────────────────────────────────────┤
//! │                                                                                 │
//! │  Application Layer Errors                                                      │
//! │  ┌─────────────────────────────────────────────────────────────────────────┐   │
//! │  │                         HpmError                                        │   │
//! │  │  • PackageNotFound - Requested package doesn't exist                   │   │
//! │  │  • Config - Configuration file issues                                  │   │
//! │  │  • Resolver - Dependency resolution failures                           │   │
//! │  │  • Install - Package installation problems                             │   │
//! │  │  • Network - Registry connectivity issues                              │   │
//! │  │  • Io - File system operation failures                                 │   │
//! │  └─────────────────────────────────────────────────────────────────────────┘   │
//! │                                    │                                           │
//! │                                    ▼ (composed from)                          │
//! │  Domain-Specific Errors                                                        │
//! │  ┌─────────────────┐  ┌─────────────────┐  ┌─────────────────────────────┐     │
//! │  │ StorageError    │  │ RegistryError   │  │   ResolverError             │     │
//! │  │ • DirectoryRead │  │ • PackageNotFound│  │   • NoSolution              │     │
//! │  │ • ManifestParse │  │ • NetworkError  │  │   • VersionConflict         │     │
//! │  │ • PackageNotFound│  │ • AuthFailed    │  │   • CircularDependency     │     │
//! │  └─────────────────┘  └─────────────────┘  └─────────────────────────────┘     │
//! │                                    │                                           │
//! │                                    ▼ (built on)                               │
//! │  System Errors                                                                 │
//! │  ┌─────────────────────────────────────────────────────────────────────────┐   │
//! │  │  • std::io::Error - File system operations                             │   │
//! │  │  • reqwest::Error - HTTP requests and network operations               │   │
//! │  │  • toml::de::Error - Configuration file parsing                        │   │
//! │  │  • serde_json::Error - JSON serialization/deserialization            │   │
//! │  └─────────────────────────────────────────────────────────────────────────┘   │
//! └─────────────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Core Error Types
//!
//! ### HpmError - Central Error Enum
//! The main error type that aggregates all possible HPM errors:
//!
//! ```rust
//! use hpm_error::HpmError;
//!
//! // Example error creation
//! let error = HpmError::PackageNotFound {
//!     name: "nonexistent-package".to_string()
//! };
//! ```
//!
//! ### Error Categories
//!
//! #### Package Errors
//! Errors related to package operations:
//! - **PackageNotFound**: Requested package doesn't exist in registry or storage
//! - **Install**: Package installation failures (extraction, validation, etc.)
//!
//! #### Configuration Errors  
//! Errors in configuration files or settings:
//! - **Config**: Invalid configuration values, missing required settings, malformed TOML
//!
//! #### Network Errors
//! Issues with network operations:
//! - **Network**: Registry connectivity, timeout, authentication failures
//!
//! #### Dependency Resolution Errors
//! Problems with dependency resolution:
//! - **Resolver**: Version conflicts, circular dependencies, unsatisfiable constraints
//!
//! #### System Errors
//! Low-level system operation failures:
//! - **Io**: File system permissions, disk space, path issues
//!
//! ## Error Context and Debugging
//!
//! All HPM errors provide rich context information for debugging:
//!
//! ```rust
//! use hpm_error::HpmError;
//!
//! fn handle_error(error: &HpmError) {
//!     match error {
//!         HpmError::PackageNotFound { name } => {
//!             eprintln!("Package '{}' not found. Try 'hpm search {}' to find similar packages.", name, name);
//!         }
//!         HpmError::Config { message } => {
//!             eprintln!("Configuration error: {}", message);
//!             eprintln!("Check your ~/.hpm/config.toml file for syntax errors.");
//!         }
//!         HpmError::Network(err) => {
//!             eprintln!("Network error: {}", err);
//!             eprintln!("Check your internet connection and registry URL.");
//!         }
//!         HpmError::Resolver { message } => {
//!             eprintln!("Dependency resolution failed: {}", message);
//!             eprintln!("Try updating your dependencies or resolving version conflicts.");
//!         }
//!         _ => {
//!             eprintln!("Error: {}", error);
//!         }
//!     }
//! }
//! ```
//!
//! ## Integration with thiserror
//!
//! HPM uses the [`thiserror`] crate to provide ergonomic error handling with
//! automatic implementations of standard traits:
//!
//! - **Display**: Human-readable error messages
//! - **Error**: Standard error trait implementation
//! - **From**: Automatic conversions from underlying error types
//! - **Source**: Error chain traversal for debugging
//!
//! ## Error Propagation Patterns
//!
//! ### Result Types
//! All HPM operations return `Result<T, HpmError>` for consistent error handling:
//!
//! ```rust
//! use hpm_error::HpmError;
//! use std::result::Result as StdResult;
//!
//! type Result<T> = StdResult<T, HpmError>;
//!
//! fn example_operation() -> Result<()> {
//!     // Operation that might fail
//!     if some_condition() {
//!         return Err(HpmError::Config {
//!             message: "Invalid configuration detected".to_string(),
//!         });
//!     }
//!     Ok(())
//! }
//! # fn some_condition() -> bool { false }
//! ```
//!
//! ### Error Conversion
//! Automatic conversion from underlying error types:
//!
//! ```rust
//! use hpm_error::HpmError;
//! use std::fs;
//!
//! fn read_config() -> Result<String, HpmError> {
//!     // std::io::Error automatically converts to HpmError::Io
//!     let content = fs::read_to_string("config.toml")?;
//!     Ok(content)
//! }
//! ```
//!
//! ### Error Chaining
//! Preserve error context through the error chain:
//!
//! ```rust
//! use hpm_error::HpmError;
//!
//! fn process_package(name: &str) -> Result<(), HpmError> {
//!     download_package(name)
//!         .map_err(|e| HpmError::Install {
//!             message: format!("Failed to install package '{}': {}", name, e),
//!         })?;
//!     Ok(())
//! }
//!
//! # fn download_package(_name: &str) -> Result<(), Box<dyn std::error::Error>> {
//! #     Ok(())
//! # }
//! ```
//!
//! ## Best Practices
//!
//! ### Error Message Guidelines
//! - **Be Specific**: Include relevant details like package names, file paths, or version numbers
//! - **Be Actionable**: Suggest possible solutions or next steps where applicable
//! - **Be Consistent**: Use consistent terminology and formatting across error types
//!
//! ### Context Preservation
//! - **Chain Errors**: Use `map_err` to add context while preserving the original error
//! - **Include Details**: Add relevant information like operation context or user input
//! - **Maintain Source**: Keep the error source chain intact for debugging
//!
//! ### User Experience
//! - **Progressive Detail**: Provide brief messages for users, detailed information for developers
//! - **Helpful Suggestions**: Include suggestions for resolving common issues
//! - **Clear Categories**: Use distinct error types for different failure scenarios
//!
//! ## Integration Examples
//!
//! ### CLI Error Handling
//! ```rust
//! use hpm_error::HpmError;
//!
//! fn main() {
//!     if let Err(error) = run_cli() {
//!         match error {
//!             HpmError::PackageNotFound { name } => {
//!                 eprintln!("Error: Package '{}' not found", name);
//!                 std::process::exit(1);
//!             }
//!             HpmError::Config { message } => {
//!                 eprintln!("Configuration error: {}", message);
//!                 std::process::exit(1);
//!             }
//!             _ => {
//!                 eprintln!("Unexpected error: {}", error);
//!                 std::process::exit(2);
//!             }
//!         }
//!     }
//! }
//!
//! # fn run_cli() -> Result<(), HpmError> { Ok(()) }
//! ```
//!
//! ### Library Integration
//! ```rust
//! use hpm_error::HpmError;
//!
//! pub struct PackageManager {
//!     // ... fields
//! }
//!
//! impl PackageManager {
//!     pub fn install(&mut self, package: &str) -> Result<(), HpmError> {
//!         // Validate package name
//!         if package.is_empty() {
//!             return Err(HpmError::Config {
//!                 message: "Package name cannot be empty".to_string(),
//!             });
//!         }
//!
//!         // Attempt installation
//!         self.download_and_install(package)
//!             .map_err(|e| HpmError::Install {
//!                 message: format!("Installation of '{}' failed: {}", package, e),
//!             })
//!     }
//!
//!     fn download_and_install(&mut self, package: &str) -> Result<(), Box<dyn std::error::Error>> {
//!         // Implementation details...
//!         Ok(())
//!     }
//! }
//! ```

use thiserror::Error;

/// Central error type for all HPM operations
///
/// This enum represents all possible errors that can occur during HPM operations,
/// from high-level package management down to low-level system operations.
/// Each variant includes relevant context information to help with debugging
/// and user-friendly error reporting.
#[derive(Debug, Error)]
pub enum HpmError {
    /// A requested package was not found in the registry or local storage
    ///
    /// This error occurs when attempting to access a package that doesn't exist,
    /// either because the name is incorrect, the package has been removed, or
    /// the registry is not accessible.
    #[error("Package not found: {name}")]
    PackageNotFound {
        /// The name of the package that could not be found
        name: String,
    },

    /// Network-related errors during registry operations
    ///
    /// This includes HTTP request failures, timeout issues, DNS resolution
    /// problems, and other network connectivity issues when communicating
    /// with package registries.
    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),

    /// File system I/O errors
    ///
    /// This includes permission issues, disk space problems, missing files,
    /// and other file system related errors during package operations.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Configuration-related errors
    ///
    /// This covers invalid configuration files, missing required settings,
    /// malformed TOML files, and other configuration-related issues.
    #[error("Configuration error: {message}")]
    Config {
        /// Detailed description of the configuration issue
        message: String,
    },

    /// Dependency resolution errors
    ///
    /// This includes version conflicts, circular dependencies, unsatisfiable
    /// version constraints, and other issues that prevent finding a valid
    /// dependency solution.
    #[error("Dependency resolution error: {message}")]
    Resolver {
        /// Detailed description of the resolution failure
        message: String,
    },

    /// Package installation errors
    ///
    /// This covers failures during package installation including extraction
    /// errors, validation failures, and integration issues with Houdini.
    #[error("Installation error: {message}")]
    Install {
        /// Detailed description of the installation failure
        message: String,
    },
}

/// Convenience type alias for Results with HpmError
///
/// This provides a shorter way to write `Result<T, HpmError>` throughout
/// the HPM codebase, improving readability and consistency.
pub type Result<T> = std::result::Result<T, HpmError>;

impl HpmError {
    /// Create a new configuration error
    ///
    /// # Example
    /// ```rust
    /// use hpm_error::HpmError;
    ///
    /// let error = HpmError::config("Invalid registry URL in config.toml");
    /// ```
    pub fn config<S: Into<String>>(message: S) -> Self {
        Self::Config {
            message: message.into(),
        }
    }

    /// Create a new resolver error
    ///
    /// # Example
    /// ```rust
    /// use hpm_error::HpmError;
    ///
    /// let error = HpmError::resolver("Package A v1.0 conflicts with package B v2.0");
    /// ```
    pub fn resolver<S: Into<String>>(message: S) -> Self {
        Self::Resolver {
            message: message.into(),
        }
    }

    /// Create a new installation error
    ///
    /// # Example
    /// ```rust
    /// use hpm_error::HpmError;
    ///
    /// let error = HpmError::install("Failed to extract package archive");
    /// ```
    pub fn install<S: Into<String>>(message: S) -> Self {
        Self::Install {
            message: message.into(),
        }
    }

    /// Create a new package not found error
    ///
    /// # Example
    /// ```rust
    /// use hpm_error::HpmError;
    ///
    /// let error = HpmError::package_not_found("nonexistent-package");
    /// ```
    pub fn package_not_found<S: Into<String>>(name: S) -> Self {
        Self::PackageNotFound { name: name.into() }
    }

    /// Check if this error is a network-related error
    ///
    /// # Example
    /// ```rust
    /// use hpm_error::HpmError;
    ///
    /// let error = HpmError::config("Invalid setting");
    /// assert!(!error.is_network_error());
    /// ```
    pub fn is_network_error(&self) -> bool {
        matches!(self, HpmError::Network(_))
    }

    /// Check if this error is a configuration-related error
    ///
    /// # Example  
    /// ```rust
    /// use hpm_error::HpmError;
    ///
    /// let error = HpmError::config("Invalid setting");
    /// assert!(error.is_config_error());
    /// ```
    pub fn is_config_error(&self) -> bool {
        matches!(self, HpmError::Config { .. })
    }

    /// Check if this error represents a missing package
    ///
    /// # Example
    /// ```rust
    /// use hpm_error::HpmError;
    ///
    /// let error = HpmError::package_not_found("missing-package");
    /// assert!(error.is_package_not_found());
    /// ```
    pub fn is_package_not_found(&self) -> bool {
        matches!(self, HpmError::PackageNotFound { .. })
    }
}
