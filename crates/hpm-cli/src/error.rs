//! CLI error types with categorized exit codes and help text.
//!
//! - `Config`, `Package`, `Network`, `Io` — user errors, exit code 1.
//! - `Internal` — bugs in HPM, exit code 2.
//! - `External` — preserves the child process exit code.

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

    #[error("Internal error")]
    Internal {
        #[source]
        source: anyhow::Error,
        help: Option<String>,
    },

    #[error("External command failed")]
    External {
        command: String,
        exit_code: u8,
        help: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitStatus {
    Success,
    Failure,
    Error,
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

    pub fn io(err: impl Into<anyhow::Error>, help: Option<String>) -> Self {
        Self::Io {
            source: err.into(),
            help,
        }
    }

    pub fn internal(err: impl Into<anyhow::Error>, help: Option<String>) -> Self {
        Self::Internal {
            source: err.into(),
            help,
        }
    }

    pub fn external(cmd: String, code: u8, help: Option<String>) -> Self {
        Self::External {
            command: cmd,
            exit_code: code,
            help,
        }
    }

    fn help(&self) -> Option<&str> {
        match self {
            CliError::Config { help, .. }
            | CliError::Package { help, .. }
            | CliError::Network { help, .. }
            | CliError::Io { help, .. }
            | CliError::Internal { help, .. }
            | CliError::External { help, .. } => help.as_deref(),
        }
    }

    fn detail(&self) -> String {
        match self {
            CliError::Config { source, .. } => format!("Configuration error: {source}"),
            CliError::Package { source, .. } => format!("Package error: {source}"),
            CliError::Network { source, .. } => format!("Network error: {source}"),
            CliError::Io { source, .. } => format!("I/O error: {source}"),
            CliError::Internal { source, .. } => format!("Internal error: {source}"),
            CliError::External {
                command, exit_code, ..
            } => {
                format!("External command '{command}' failed with exit code {exit_code}")
            }
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
}
