// Allow unused_assignments warning because thiserror's #[source] macro
// uses the source fields internally but the compiler doesn't detect this usage.
#![allow(unused_assignments)]

//! Error handling for HPM CLI
//!
//! This module provides a comprehensive error handling system inspired by UV's approach
//! to user-friendly error messages. It includes:
//!
//! - **Structured Error Types**: Categorized errors with contextual information
//! - **Exit Codes**: Unix-standard exit codes for proper shell integration
//! - **Helpful Error Messages**: User-friendly messages with actionable suggestions
//! - **Machine-Readable Errors**: JSON-formatted errors for automation
//!
//! # Design Philosophy
//!
//! The error system follows these principles:
//!
//! 1. **User-Centric**: Error messages should help users understand what went wrong and how to fix it
//! 2. **Categorized**: Errors are grouped by type (Config, Package, Network, etc.) for consistent handling
//! 3. **Contextual**: Each error includes optional help text with specific guidance
//! 4. **Standards Compliant**: Uses Unix exit code conventions for shell script compatibility
//!
//! # Error Categories
//!
//! - **Config**: Configuration file issues, invalid settings
//! - **Package**: Package manifest problems, dependency conflicts
//! - **Network**: Registry connection issues, download failures
//! - **I/O**: File system operations, permission problems
//! - **Internal**: Unexpected errors, bugs in HPM itself
//! - **External**: Failures in external commands (git, etc.)
//!
//! # Examples
//!
//! ```rust
//! use hpm_cli::error::{CliError, CliResult};
//!
//! // Create a package error with helpful context
//! fn validate_package() -> CliResult<()> {
//!     Err(CliError::package(
//!         anyhow::anyhow!("Missing required field 'name'"),
//!         Some("Add a 'name' field to your hpm.toml file".to_string())
//!     ))
//! }
//!
//! // Use the conversion trait for ergonomic error handling
//! use hpm_cli::error::IntoCliError;
//!
//! let result: Result<String, std::io::Error> = std::fs::read_to_string("missing.txt");
//! let cli_result = result.into_io_error(); // Converts to CliError::Io
//! ```

use miette::Diagnostic;
use owo_colors::{OwoColorize, Style};
use std::process::ExitCode;
use thiserror::Error;

/// Main CLI error type that wraps all possible error conditions
///
/// This enum provides a structured approach to error handling across HPM,
/// ensuring consistent error reporting and proper exit codes. Each variant
/// includes contextual information and optional help text.
///
/// # Exit Code Mapping
///
/// - **Config, Package, Network, I/O**: Exit code 1 (user error)
/// - **Internal**: Exit code 2 (internal error)
/// - **External**: Custom exit code from external command
///
/// # Examples
///
/// ```rust
/// // Create a configuration error
/// let error = CliError::config(
///     anyhow::anyhow!("Invalid configuration file"),
///     Some("Check your ~/.hpm/config.toml syntax".to_string())
/// );
///
/// // Create a package error without help
/// let error = CliError::package(
///     anyhow::anyhow!("Package not found"),
///     None
/// );
/// ```
#[derive(Error, Debug, Diagnostic)]
pub enum CliError {
    /// User input or configuration error (exit code 1)
    ///
    /// Used for problems with configuration files, invalid command-line arguments,
    /// or user settings that prevent the operation from proceeding.
    #[error("Configuration error")]
    #[diagnostic(code(hpm::config_error))]
    Config {
        /// The underlying error that caused this problem
        #[source]
        source: anyhow::Error,
        /// Optional help text with suggestions for fixing the issue
        #[help]
        help: Option<String>,
    },

    /// Package-related error (exit code 1)
    ///
    /// Used for problems with package manifests, dependency resolution,
    /// or package validation failures.
    #[error("Package error")]
    #[diagnostic(code(hpm::package_error))]
    Package {
        /// The underlying error that caused this problem
        #[source]
        source: anyhow::Error,
        /// Optional help text with suggestions for fixing the issue
        #[help]
        help: Option<String>,
    },

    /// Network or registry error (exit code 1)
    ///
    /// Used for problems connecting to package registries, downloading packages,
    /// or other network-related operations.
    #[error("Network error")]
    #[diagnostic(code(hpm::network_error))]
    Network {
        /// The underlying error that caused this problem
        #[source]
        source: anyhow::Error,
        /// Optional help text with suggestions for fixing the issue
        #[help]
        help: Option<String>,
    },

    /// File system or I/O error (exit code 1)
    ///
    /// Used for problems reading or writing files, creating directories,
    /// or other file system operations.
    #[error("I/O error")]
    #[diagnostic(code(hpm::io_error))]
    Io {
        /// The underlying error that caused this problem
        #[source]
        source: anyhow::Error,
        /// Optional help text with suggestions for fixing the issue
        #[help]
        help: Option<String>,
    },

    /// Unexpected internal error (exit code 2)
    ///
    /// Used for bugs in HPM itself, panics that were caught, or other
    /// unexpected conditions that indicate a problem with the software.
    #[error("Internal error")]
    #[diagnostic(code(hpm::internal_error))]
    Internal {
        /// The underlying error that caused this problem
        #[source]
        source: anyhow::Error,
        /// Optional help text with suggestions for fixing the issue
        #[help]
        help: Option<String>,
    },

    /// External command error (custom exit code)
    ///
    /// Used when external commands (like git, python, etc.) fail.
    /// Preserves the original exit code from the external command.
    #[error("External command failed")]
    #[diagnostic(code(hpm::external_error))]
    External {
        /// The command that failed
        command: String,
        /// The exit code returned by the external command
        exit_code: u8,
        /// Optional help text with suggestions for fixing the issue
        #[help]
        help: Option<String>,
    },
}

/// Exit status for CLI commands
///
/// Represents the final outcome of a CLI command execution.
/// Maps to Unix standard exit codes for proper shell integration.
///
/// # Exit Code Conventions
///
/// - **0**: Success - command completed successfully
/// - **1**: Failure - user error (bad input, config, etc.)
/// - **2**: Error - internal error (bugs, unexpected conditions)
/// - **N**: External - exit code from external command
///
/// # Examples
///
/// ```rust
/// use hpm_cli::error::{ExitStatus, CliError};
/// use std::process::ExitCode;
///
/// // Convert to process exit code
/// let status = ExitStatus::Success;
/// let exit_code: ExitCode = status.into();
///
/// // Determine status from error type
/// let error = CliError::config(anyhow::anyhow!("Bad config"), None);
/// let status = ExitStatus::from(&error); // ExitStatus::Failure
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitStatus {
    /// Command succeeded (exit code 0)
    ///
    /// The operation completed successfully without any issues.
    Success,

    /// User error - configuration, input, etc. (exit code 1)
    ///
    /// The operation failed due to user input, configuration problems,
    /// or other issues that the user can fix.
    Failure,

    /// Unexpected error - internal bugs, panics, etc. (exit code 2)
    ///
    /// The operation failed due to an internal error in HPM itself.
    /// These typically indicate bugs that should be reported.
    Error,

    /// External command returned specific exit code
    ///
    /// An external command (git, python, etc.) failed and returned
    /// this specific exit code.
    External(u8),
}

impl From<ExitStatus> for ExitCode {
    fn from(status: ExitStatus) -> Self {
        match status {
            ExitStatus::Success => ExitCode::SUCCESS,
            ExitStatus::Failure => ExitCode::from(1),
            ExitStatus::Error => ExitCode::from(2),
            ExitStatus::External(code) => ExitCode::from(code),
        }
    }
}

impl From<&CliError> for ExitStatus {
    fn from(error: &CliError) -> Self {
        match error {
            CliError::Config { .. }
            | CliError::Package { .. }
            | CliError::Network { .. }
            | CliError::Io { .. } => ExitStatus::Failure,
            CliError::Internal { .. } => ExitStatus::Error,
            CliError::External { exit_code, .. } => ExitStatus::External(*exit_code),
        }
    }
}

impl CliError {
    /// Create a configuration error with optional help text
    ///
    /// # Arguments
    ///
    /// * `source` - The underlying error that caused this problem
    /// * `help` - Optional text with suggestions for fixing the issue
    ///
    /// # Examples
    ///
    /// ```rust
    /// let error = CliError::config(
    ///     std::io::Error::new(std::io::ErrorKind::NotFound, "config file not found"),
    ///     Some("Create ~/.hpm/config.toml or run 'hpm init'".to_string())
    /// );
    /// ```
    pub fn config(err: impl Into<anyhow::Error>, help: Option<String>) -> Self {
        Self::Config {
            source: err.into(),
            help,
        }
    }

    /// Create a package error with optional help text
    ///
    /// # Arguments
    ///
    /// * `source` - The underlying error that caused this problem  
    /// * `help` - Optional text with suggestions for fixing the issue
    ///
    /// # Examples
    ///
    /// ```rust
    /// let error = CliError::package(
    ///     anyhow::anyhow!("Invalid package manifest"),
    ///     Some("Check your hpm.toml syntax with 'hpm check'".to_string())
    /// );
    /// ```
    pub fn package(err: impl Into<anyhow::Error>, help: Option<String>) -> Self {
        Self::Package {
            source: err.into(),
            help,
        }
    }

    /// Create a network error with optional help text
    ///
    /// # Arguments
    ///
    /// * `source` - The underlying error that caused this problem
    /// * `help` - Optional text with suggestions for fixing the issue
    ///
    /// # Examples
    ///
    /// ```rust
    /// let error = CliError::network(
    ///     reqwest::Error::from(std::io::Error::new(std::io::ErrorKind::TimedOut, "timeout")),
    ///     Some("Check your internet connection and try again".to_string())
    /// );
    /// ```
    pub fn network(err: impl Into<anyhow::Error>, help: Option<String>) -> Self {
        Self::Network {
            source: err.into(),
            help,
        }
    }

    /// Create an I/O error with optional help text
    ///
    /// # Arguments
    ///
    /// * `source` - The underlying error that caused this problem
    /// * `help` - Optional text with suggestions for fixing the issue
    ///
    /// # Examples
    ///
    /// ```rust
    /// let error = CliError::io(
    ///     std::fs::File::open("nonexistent.txt").unwrap_err(),
    ///     Some("Make sure the file exists and you have read permission".to_string())
    /// );
    /// ```
    pub fn io(err: impl Into<anyhow::Error>, help: Option<String>) -> Self {
        Self::Io {
            source: err.into(),
            help,
        }
    }

    /// Create an internal error with optional help text
    ///
    /// # Arguments
    ///
    /// * `source` - The underlying error that caused this problem
    /// * `help` - Optional text with suggestions for fixing the issue
    ///
    /// # Examples
    ///
    /// ```rust
    /// let error = CliError::internal(
    ///     anyhow::anyhow!("Unexpected state in dependency resolver"),
    ///     Some("This is likely a bug in HPM. Please file an issue.".to_string())
    /// );
    /// ```
    pub fn internal(err: impl Into<anyhow::Error>, help: Option<String>) -> Self {
        Self::Internal {
            source: err.into(),
            help,
        }
    }

    /// Create an external command error
    ///
    /// # Arguments
    ///
    /// * `command` - The command that failed
    /// * `exit_code` - The exit code returned by the external command
    /// * `help` - Optional text with suggestions for fixing the issue
    ///
    /// # Examples
    ///
    /// ```rust
    /// let error = CliError::external(
    ///     "git clone https://github.com/user/repo.git".to_string(),
    ///     128,
    ///     Some("Check that the repository URL is correct and you have access".to_string())
    /// );
    /// ```
    pub fn external(cmd: String, code: u8, help: Option<String>) -> Self {
        Self::External {
            command: cmd,
            exit_code: code,
            help,
        }
    }

    /// Print the error with formatted output including help text
    ///
    /// This method provides detailed error information suitable for interactive use.
    /// It includes the full error message and any available help text.
    ///
    /// # Examples
    ///
    /// ```rust
    /// let error = CliError::package(
    ///     anyhow::anyhow!("Missing field 'name'"),
    ///     Some("Add a 'name' field to your hpm.toml".to_string())
    /// );
    /// error.print_error();
    /// // Output: Package error: Missing field 'name'
    /// //         help: Add a 'name' field to your hpm.toml
    /// ```
    pub fn print_error(&self) {
        // Print the main error message
        eprintln!("{}", self);

        // Print help text if available
        let help = match self {
            CliError::Config { help, .. }
            | CliError::Package { help, .. }
            | CliError::Network { help, .. }
            | CliError::Io { help, .. }
            | CliError::Internal { help, .. }
            | CliError::External { help, .. } => help,
        };

        if let Some(help_text) = help {
            eprintln!("  {}: {}", "help".style(Style::new().cyan()), help_text);
        }
    }

    /// Print a simple error message without help text
    ///
    /// This method provides a concise error message suitable for quiet operation
    /// or when detailed help isn't needed. Used when verbosity is low.
    ///
    /// # Examples
    ///
    /// ```rust
    /// let error = CliError::package(anyhow::anyhow!("Not found"), None);
    /// error.print_simple();
    /// // Output: error: Package error: Not found
    /// ```
    pub fn print_simple(&self) {
        let error_style = Style::new().red().bold();
        let message = match self {
            CliError::Config { source, .. } => {
                format!("Configuration error: {}", source)
            }
            CliError::Package { source, .. } => {
                format!("Package error: {}", source)
            }
            CliError::Network { source, .. } => {
                format!("Network error: {}", source)
            }
            CliError::Io { source, .. } => {
                format!("I/O error: {}", source)
            }
            CliError::Internal { source, .. } => {
                format!("Internal error: {}", source)
            }
            CliError::External {
                command, exit_code, ..
            } => {
                format!(
                    "External command '{}' failed with exit code {}",
                    command, exit_code
                )
            }
        };

        eprintln!("{} {}", "error:".style(error_style), message);
    }
}

/// Convenience type alias for CLI results
///
/// This type alias simplifies function signatures throughout the CLI codebase.
/// It defaults to `()` for the success type, which is common for CLI operations.
///
/// # Examples
///
/// ```rust
/// use hpm_cli::error::CliResult;
///
/// // Function that returns nothing on success
/// fn do_something() -> CliResult {
///     Ok(())
/// }
///
/// // Function that returns a value on success
/// fn get_value() -> CliResult<String> {
///     Ok("success".to_string())
/// }
/// ```
pub type CliResult<T = ()> = Result<T, CliError>;

#[allow(dead_code)]
/// Convert common error types to CLI errors (future feature)
///
/// This trait provides ergonomic conversion from standard error types
/// to categorized CLI errors. Currently unused but kept for future
/// expansion of the error handling system.
///
/// # Examples
///
/// ```rust
/// use hpm_cli::error::IntoCliError;
/// use std::fs;
///
/// let result: Result<String, std::io::Error> = fs::read_to_string("file.txt");
/// let cli_result = result.into_io_error(); // Converts to CliError::Io
/// ```
trait IntoCliError<T> {
    fn into_config_error(self) -> CliResult<T>;
    fn into_package_error(self) -> CliResult<T>;
    fn into_network_error(self) -> CliResult<T>;
    fn into_io_error(self) -> CliResult<T>;
    fn into_internal_error(self) -> CliResult<T>;
}

#[allow(dead_code)]
impl<T, E> IntoCliError<T> for Result<T, E>
where
    E: Into<anyhow::Error>,
{
    fn into_config_error(self) -> CliResult<T> {
        self.map_err(|e| CliError::config(e, None))
    }

    fn into_package_error(self) -> CliResult<T> {
        self.map_err(|e| CliError::package(e, None))
    }

    fn into_network_error(self) -> CliResult<T> {
        self.map_err(|e| CliError::network(e, None))
    }

    fn into_io_error(self) -> CliResult<T> {
        self.map_err(|e| CliError::io(e, None))
    }

    fn into_internal_error(self) -> CliResult<T> {
        self.map_err(|e| CliError::internal(e, None))
    }
}

// Note: Specialized error types were removed as they were unused.
// Error handling is done through the generic CliError variants.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exit_status_conversion() {
        assert_eq!(ExitCode::from(ExitStatus::Success), ExitCode::SUCCESS);
        assert_eq!(ExitCode::from(ExitStatus::Failure), ExitCode::from(1));
        assert_eq!(ExitCode::from(ExitStatus::Error), ExitCode::from(2));
        assert_eq!(ExitCode::from(ExitStatus::External(42)), ExitCode::from(42));
    }

    #[test]
    fn test_cli_error_to_exit_status() {
        let config_error = CliError::config(anyhow::anyhow!("test"), None);
        assert_eq!(ExitStatus::from(&config_error), ExitStatus::Failure);

        let internal_error = CliError::internal(anyhow::anyhow!("test"), None);
        assert_eq!(ExitStatus::from(&internal_error), ExitStatus::Error);

        let external_error = CliError::external("git".to_string(), 128, None);
        assert_eq!(ExitStatus::from(&external_error), ExitStatus::External(128));
    }

    #[test]
    fn test_error_conversion_trait() {
        let result: Result<(), std::io::Error> = Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "file not found",
        ));

        let cli_result = result.into_io_error();
        assert!(matches!(cli_result, Err(CliError::Io { .. })));
    }
}
