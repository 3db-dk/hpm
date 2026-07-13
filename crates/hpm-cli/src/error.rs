//! CLI error types with categorized exit codes and help text.
//!
//! - `Config`, `Package`, `Network`, `Io` — user errors, exit code 1.
//! - [`ExitStatus::External`] — preserves a child process exit code.

use console::style;
use std::process::ExitCode;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum CliError {
    #[error("Configuration error")]
    Config {
        #[source]
        source: anyhow::Error,
        help: Option<String>,
    },

    #[error("Package error")]
    Package {
        #[source]
        source: anyhow::Error,
        help: Option<String>,
    },

    #[error("Network error")]
    Network {
        #[source]
        source: anyhow::Error,
        help: Option<String>,
    },

    #[error("I/O error")]
    Io {
        #[source]
        source: anyhow::Error,
        help: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitStatus {
    Success,
    Failure,
    External(u8),
}

impl From<ExitStatus> for ExitCode {
    fn from(status: ExitStatus) -> Self {
        match status {
            ExitStatus::Success => ExitCode::SUCCESS,
            ExitStatus::Failure => ExitCode::from(1),
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
        }
    }
}

impl CliError {
    pub fn config(err: impl Into<anyhow::Error>, help: Option<String>) -> Self {
        Self::Config {
            source: err.into(),
            help,
        }
    }

    pub fn package(err: impl Into<anyhow::Error>, help: Option<String>) -> Self {
        Self::Package {
            source: err.into(),
            help,
        }
    }

    pub fn network(err: impl Into<anyhow::Error>, help: Option<String>) -> Self {
        Self::Network {
            source: err.into(),
            help,
        }
    }

    fn help(&self) -> Option<&str> {
        match self {
            CliError::Config { help, .. }
            | CliError::Package { help, .. }
            | CliError::Network { help, .. }
            | CliError::Io { help, .. } => help.as_deref(),
        }
    }

    /// Render the error's one-line detail, including the full `anyhow` cause
    /// chain. The `{:#}` (alternate) format walks `source()` recursively, so a
    /// failure several layers deep (e.g. install → sync → registry resolution →
    /// HTTP 404) surfaces every link instead of just the outermost context. The
    /// plain `{}` form we used before printed only the top frame, which is what
    /// made distinct failures (no registries / package not found / bad version)
    /// all collapse to one generic line.
    pub(crate) fn detail(&self) -> String {
        match self {
            CliError::Config { source, .. } => format!("Configuration error: {source:#}"),
            CliError::Package { source, .. } => format!("Package error: {source:#}"),
            CliError::Network { source, .. } => format!("Network error: {source:#}"),
            CliError::Io { source, .. } => format!("I/O error: {source:#}"),
        }
    }

    /// Render the error with its detail message and any help hint.
    pub fn print_error(&self) {
        eprintln!("{}", self.detail());
        if let Some(help_text) = self.help() {
            eprintln!("  {}: {}", style("help").cyan(), help_text);
        }
    }

    /// Render a single-line error without the help hint.
    pub fn print_simple(&self) {
        eprintln!("{} {}", style("error:").red().bold(), self.detail());
    }
}

pub type CliResult<T = ()> = Result<T, CliError>;

/// Build the standard `"Use 'hpm <cmd> --help' for usage information"`
/// help string. Exposed so test fixtures and ad-hoc call sites can produce
/// identical text without re-typing it.
pub fn help_for(command: &str) -> String {
    format!("Use 'hpm {command} --help' for usage information")
}

/// Extension trait that promotes a `Result<T, E: Into<anyhow::Error>>` into
/// a [`CliResult`] with the standard `Use 'hpm <cmd> --help' …` hint.
///
/// Replaces the repeated map_err boilerplate that lived in [`crate::run`]
/// for every subcommand:
///
/// ```ignore
/// commands::add::add(...)
///     .await
///     .map_err(|e| CliError::package(e, Some("Use 'hpm add --help' …".into())))?;
/// ```
///
/// becomes
///
/// ```ignore
/// commands::add::add(...).await.cli_package("add")?;
/// ```
pub trait CliResultExt<T> {
    /// Lift the error into [`CliError::Package`] with the standard
    /// `"Use 'hpm <cmd> --help' …"` hint.
    fn cli_package(self, command: &str) -> CliResult<T>;
    /// Lift the error into [`CliError::Network`] with the standard hint.
    fn cli_network(self, command: &str) -> CliResult<T>;
    /// Lift the error into [`CliError::Config`] with the standard hint.
    fn cli_config(self, command: &str) -> CliResult<T>;
}

impl<T, E: Into<anyhow::Error>> CliResultExt<T> for Result<T, E> {
    fn cli_package(self, command: &str) -> CliResult<T> {
        self.map_err(|e| CliError::package(e, Some(help_for(command))))
    }
    fn cli_network(self, command: &str) -> CliResult<T> {
        self.map_err(|e| CliError::network(e, Some(help_for(command))))
    }
    fn cli_config(self, command: &str) -> CliResult<T> {
        self.map_err(|e| CliError::config(e, Some(help_for(command))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exit_status_conversion() {
        assert_eq!(ExitCode::from(ExitStatus::Success), ExitCode::SUCCESS);
        assert_eq!(ExitCode::from(ExitStatus::Failure), ExitCode::from(1));
        assert_eq!(ExitCode::from(ExitStatus::External(42)), ExitCode::from(42));
    }

    #[test]
    fn detail_includes_full_cause_chain() {
        // Regression: a wrapped error must surface its underlying cause, not
        // just the outermost context. Previously `detail()` used `{source}`,
        // which printed only "Failed to sync project dependencies" and hid the
        // real reason (e.g. "no registries configured").
        let root = anyhow::anyhow!("no package registries are configured")
            .context("Failed to sync project dependencies");
        let err = CliError::package(root, None);
        let detail = err.detail();
        assert!(
            detail.contains("Failed to sync project dependencies"),
            "outer context should remain, got: {detail}"
        );
        assert!(
            detail.contains("no package registries are configured"),
            "underlying cause should be surfaced, got: {detail}"
        );
    }

    #[test]
    fn test_cli_error_to_exit_status() {
        let config_error = CliError::config(anyhow::anyhow!("test"), None);
        assert_eq!(ExitStatus::from(&config_error), ExitStatus::Failure);
    }
}
