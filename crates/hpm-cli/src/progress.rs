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
    /// Style for download progress (shows bytes transferred)
    pub fn download() -> ProgressStyle {
        ProgressStyle::with_template(
            "{spinner:.cyan} {msg}\n  [{elapsed_precise}] [{bar:40.cyan/dim}] {bytes}/{total_bytes} ({bytes_per_sec})"
        )
        .expect("valid template")
        .progress_chars("=> ")
    }

    /// Style for count-based progress (shows items completed)
    pub fn items() -> ProgressStyle {
        ProgressStyle::with_template(
            "{spinner:.cyan} {msg}\n  [{elapsed_precise}] [{bar:40.cyan/dim}] {pos}/{len} ({per_sec})"
        )
        .expect("valid template")
        .progress_chars("=> ")
    }

    /// Style for indeterminate spinner
    pub fn spinner() -> ProgressStyle {
        ProgressStyle::with_template("{spinner:.cyan} {msg} [{elapsed_precise}]")
            .expect("valid template")
    }

    /// Style for simple spinner without elapsed time
    pub fn spinner_simple() -> ProgressStyle {
        ProgressStyle::with_template("{spinner:.cyan} {msg}").expect("valid template")
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

    /// Add a sub-progress bar for a specific task
    pub fn add_task(&self, len: u64, message: impl Into<String>) -> ProgressBar {
        let pb = self.multi.add(ProgressBar::new(len));
        pb.set_style(ProgressStyles::items());
        pb.set_message(message.into());
        pb
    }

    /// Add a download progress bar
    pub fn add_download(&self, total_bytes: u64, message: impl Into<String>) -> ProgressBar {
        let pb = self.multi.add(ProgressBar::new(total_bytes));
        pb.set_style(ProgressStyles::download());
        pb.set_message(message.into());
        pb
    }

    /// Add an indeterminate spinner for a sub-task
    pub fn add_spinner(&self, message: impl Into<String>) -> ProgressBar {
        let pb = self.multi.add(ProgressBar::new_spinner());
        pb.set_style(ProgressStyles::spinner_simple());
        pb.set_message(message.into());
        pb.enable_steady_tick(Duration::from_millis(100));
        pb
    }

    /// Mark operation as completed with success
    pub fn finish_success(&self, message: impl Into<String>) {
        if let Some(ref pb) = self.main_bar {
            pb.set_style(ProgressStyles::finished());
            pb.finish_with_message(format!("{} {}", style("✓").green(), message.into()));
        }
    }

    /// Mark operation as completed with warning
    pub fn finish_warning(&self, message: impl Into<String>) {
        if let Some(ref pb) = self.main_bar {
            pb.set_style(ProgressStyles::finished());
            pb.finish_with_message(format!("{} {}", style("⚠").yellow(), message.into()));
        }
    }

    /// Mark operation as failed
    pub fn finish_error(&self, message: impl Into<String>) {
        if let Some(ref pb) = self.main_bar {
            pb.set_style(ProgressStyles::finished());
            pb.finish_with_message(format!("{} {}", style("✗").red(), message.into()));
        }
    }

    /// Check if progress bars are being hidden (e.g., in CI environments)
    pub fn is_hidden(&self) -> bool {
        self.multi.is_hidden()
    }
}

impl Default for OperationProgress {
    fn default() -> Self {
        Self::new()
    }
}

/// Create a simple spinner for a quick operation
pub fn spinner(message: impl Into<String>) -> ProgressBar {
    let pb = ProgressBar::new_spinner();
    pb.set_style(ProgressStyles::spinner_simple());
    pb.set_message(message.into());
    pb.enable_steady_tick(Duration::from_millis(100));
    pb
}

/// Create a progress bar for counting items
pub fn item_progress(total: u64, message: impl Into<String>) -> ProgressBar {
    let pb = ProgressBar::new(total);
    pb.set_style(ProgressStyles::items());
    pb.set_message(message.into());
    pb
}

/// Create a progress bar for downloads
pub fn download_progress(total_bytes: u64, message: impl Into<String>) -> ProgressBar {
    let pb = ProgressBar::new(total_bytes);
    pb.set_style(ProgressStyles::download());
    pb.set_message(message.into());
    pb
}

/// Finish a progress bar with success styling
pub fn finish_success(pb: &ProgressBar, message: impl Into<String>) {
    pb.set_style(ProgressStyles::finished());
    pb.finish_with_message(format!("{} {}", style("✓").green(), message.into()));
}

/// Finish a progress bar with error styling
pub fn finish_error(pb: &ProgressBar, message: impl Into<String>) {
    pb.set_style(ProgressStyles::finished());
    pb.finish_with_message(format!("{} {}", style("✗").red(), message.into()));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_progress_styles_valid() {
        // Just ensure styles can be created without panicking
        let _ = ProgressStyles::download();
        let _ = ProgressStyles::items();
        let _ = ProgressStyles::spinner();
        let _ = ProgressStyles::spinner_simple();
        let _ = ProgressStyles::finished();
    }

    #[test]
    fn test_operation_progress_lifecycle() {
        let mut op = OperationProgress::new();
        op.start("Starting operation");
        op.set_message("Processing...");
        op.finish_success("Done");
    }

    #[test]
    fn test_simple_spinner() {
        let pb = spinner("Working...");
        pb.finish_with_message("Complete");
    }
}
