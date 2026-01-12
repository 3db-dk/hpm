//! Output format options for HPM CLI
//!
//! This module defines the output format options for programmatic consumption
//! of HPM command results:
//!
//! - **Human**: Traditional human-readable output (default)
//! - **JSON**: Pretty-printed JSON for human-readable automation
//! - **JSON Lines**: One JSON object per line for streaming processing
//! - **JSON Compact**: Minified JSON for bandwidth-sensitive applications

use std::fmt::{self, Display};

/// Output format options for HPM commands
///
/// Determines how command results are formatted and displayed.
/// Each format serves different use cases and consumption patterns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    /// Human-readable output (default)
    ///
    /// Traditional CLI output with colors, formatting, and user-friendly messages.
    /// Best for interactive terminal use.
    Human,

    /// JSON output for programmatic use
    ///
    /// Pretty-printed JSON with indentation and line breaks.
    /// Good balance between readability and machine processing.
    Json,

    /// JSON Lines output for streaming
    ///
    /// One JSON object per line without pretty printing.
    /// Ideal for streaming, logging, and line-by-line processing.
    JsonLines,

    /// Compact JSON without pretty printing
    ///
    /// Minified JSON without whitespace or line breaks.
    /// Optimal for network transmission and storage efficiency.
    JsonCompact,
}

impl Display for OutputFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Human => write!(f, "human"),
            Self::Json => write!(f, "json"),
            Self::JsonLines => write!(f, "json-lines"),
            Self::JsonCompact => write!(f, "json-compact"),
        }
    }
}
