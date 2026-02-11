//! HPM Error Handling
//!
//! Structured error types for HPM with rich context and user-friendly messages.
//!
//! ## Error Hierarchy
//!
//! [`HpmError`] aggregates all HPM error categories:
//!
//! - **PackageNotFound** - Package doesn't exist in registry or storage
//! - **Config** - Configuration file issues (invalid values, malformed TOML)
//! - **Resolver** - Dependency resolution failures (version conflicts, cycles)
//! - **Install** - Package installation problems (extraction, validation)
//! - **Network** - Registry connectivity issues (wraps `reqwest::Error`)
//! - **Io** - File system operation failures (wraps `std::io::Error`)
//!
//! Domain-specific errors (`StorageError`, `ResolverError`, etc.) are defined
//! in their respective crates and converted to `HpmError` at application boundaries.
//!
//! ## Usage
//!
//! ```rust
//! use hpm_error::HpmError;
//!
//! fn process_package(name: &str) -> Result<(), HpmError> {
//!     download_package(name)
//!         .map_err(|e| HpmError::Install {
//!             message: format!("Failed to install '{}': {}", name, e),
//!         })?;
//!     Ok(())
//! }
//!
//! # fn download_package(_: &str) -> Result<(), Box<dyn std::error::Error>> { Ok(()) }
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
