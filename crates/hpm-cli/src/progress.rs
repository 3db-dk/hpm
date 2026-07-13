//! Progress indicators for CLI operations.
//!
//! This module provides styled progress bars and spinners for long-running
//! operations, using indicatif for terminal-friendly output.

use console::style;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use std::time::Duration;

/// Progress bar style presets
pub struct ProgressStyles;

impl ProgressStyles {
    /// Style for indeterminate spinner
    pub fn spinner() -> ProgressStyle {
        ProgressStyle::with_template("{spinner:.cyan} {msg} [{elapsed_precise}]")
            .expect("valid template")
    }

    /// Style for a completed task
    pub fn finished() -> ProgressStyle {
        ProgressStyle::with_template("{msg}").expect("valid template")
    }
}

/// A progress tracker for multi-step operations
pub struct OperationProgress {
    multi: MultiProgress,
    main_bar: Option<ProgressBar>,
}

impl OperationProgress {
    /// Create a new operation progress tracker
    pub fn new() -> Self {
        Self {
            multi: MultiProgress::new(),
            main_bar: None,
        }
    }

    /// Start tracking a main operation with a message
    pub fn start(&mut self, message: impl Into<String>) {
        let pb = self.multi.add(ProgressBar::new_spinner());
        pb.set_style(ProgressStyles::spinner());
        pb.set_message(message.into());
        pb.enable_steady_tick(Duration::from_millis(100));
        self.main_bar = Some(pb);
    }

    /// Update the main operation message
    pub fn set_message(&self, message: impl Into<String>) {
        if let Some(ref pb) = self.main_bar {
            pb.set_message(message.into());
        }
    }

    /// Mark operation as completed with success
    pub fn finish_success(&self, message: impl Into<String>) {
        if let Some(ref pb) = self.main_bar {
            pb.set_style(ProgressStyles::finished());
            pb.finish_with_message(format!("{} {}", style("[ok]").green(), message.into()));
        }
    }
}

impl Default for OperationProgress {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_progress_styles_valid() {
        // Just ensure styles can be created without panicking
        let _ = ProgressStyles::spinner();
        let _ = ProgressStyles::finished();
    }

    #[test]
    fn test_operation_progress_lifecycle() {
        let mut op = OperationProgress::new();
        op.start("Starting operation");
        op.set_message("Processing...");
        op.finish_success("Done");
    }
}
