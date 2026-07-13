//! Output format options for HPM CLI.
//!
//! Selected by the global `--output` flag. Commands either honor the chosen
//! format or reject it up front in the dispatcher — no command silently
//! ignores it. `hpm update` genuinely distinguishes `json-lines` (one update
//! per line) from `json`/`json-compact`; single-document commands render one
//! object per [`OutputFormat::render_json`].

use std::fmt::{self, Display};

/// Output format options for HPM commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum OutputFormat {
    /// Human-readable output (default)
    Human,

    /// Pretty-printed JSON
    Json,

    /// One JSON object per line, for streaming consumers
    JsonLines,

    /// Minified JSON
    JsonCompact,
}

impl OutputFormat {
    /// True for every machine-readable variant.
    pub fn is_json(self) -> bool {
        self != Self::Human
    }

    /// Render a single JSON document in this format. For `json-lines` the
    /// document is emitted as one line (a one-document stream); commands with
    /// a natural per-item stream handle `json-lines` themselves.
    ///
    /// Callers must gate on [`Self::is_json`]; `Human` renders pretty JSON as
    /// a fallback but is not a supported input.
    pub fn render_json(self, value: &serde_json::Value) -> String {
        match self {
            Self::Human | Self::Json => {
                serde_json::to_string_pretty(value).expect("JSON value serializes")
            }
            Self::JsonLines | Self::JsonCompact => value.to_string(),
        }
    }
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
