//! Console utilities for HPM CLI
//!
//! This module provides consistent terminal output styling, colors, and user interaction
//! patterns across all HPM commands. It follows UV's approach to professional CLI design
//! with support for:
//!
//! - **Styled Output**: Color-coded messages with semantic meaning (success, error, warning, info)
//! - **Verbosity Control**: Multiple output levels from silent to verbose
//! - **Color Management**: Automatic detection with manual override options
//! - **User Interaction**: Prompts for confirmation and input (foundation for future features)
//!
//! # Examples
//!
//! ```rust
//! use hpm_cli::console::{Console, Verbosity, ColorChoice};
//!
//! // Create a console with specific settings
//! let mut console = Console::with_settings(Verbosity::Normal, ColorChoice::Auto);
//!
//! // Print styled messages
//! console.success("Package installed successfully");
//! console.warn("This feature is experimental");
//! console.info("Processing dependencies...");
//! ```
//!
//! # Design Principles
//!
//! - **Semantic Colors**: Green for success, red for errors, yellow for warnings, blue for info
//! - **Accessibility**: Uses symbols (checkmark, x-mark, warning, info) alongside colors for color-blind users
//! - **Terminal Agnostic**: Works across Windows, macOS, and Linux terminals
//! - **Performance**: Minimal overhead for quiet/silent modes

use anstream::{stderr, stdout, AutoStream};
use owo_colors::{OwoColorize, Style};
use std::fmt::Display;
use std::io::Write;

/// Console printer that controls terminal output with styling and verbosity
#[derive(Debug)]
pub struct Console {
    verbosity: Verbosity,
    color: ColorChoice,
    stderr: AutoStream<std::io::Stderr>,
    stdout: AutoStream<std::io::Stdout>,
}

/// Verbosity levels for console output
///
/// Controls how much information is displayed to the user. Higher levels
/// include all output from lower levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Verbosity {
    /// Minimal output (warnings and essential information)
    ///
    /// Shows important information that users should be aware of,
    /// including successful operations and warnings about potential issues.
    Quiet,

    /// Standard output level (default)
    ///
    /// Provides a good balance of information for interactive use.
    /// Shows success, info, and warning messages.
    Normal,

    /// Verbose output (includes additional details)
    ///
    /// Shows detailed information useful for troubleshooting.
    Verbose,
}

/// Color output preference
///
/// Controls when colored output is used. This allows users to override
/// automatic terminal detection when needed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorChoice {
    /// Automatically detect terminal color support (default)
    ///
    /// Uses heuristics to determine if the terminal supports colors
    /// and if the user wants colored output (respects NO_COLOR, etc.)
    Auto,

    /// Always use colored output
    ///
    /// Forces color output regardless of terminal detection.
    /// Useful when piping through tools that preserve colors.
    Always,

    /// Never use colored output
    ///
    /// Disables all colors and styling. Useful for logging,
    /// scripting, or accessibility needs.
    Never,
}

/// Message levels for styled output
///
/// Internal enum for categorizing different types of messages.
/// Each level has associated colors and symbols.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Level {
    /// Success messages (green, checkmark)
    Success,
    /// Informational messages (blue, info symbol)
    Info,
    /// Warning messages (yellow, warning symbol)
    Warning,
}

impl Console {
    /// Create a new console with default settings
    ///
    /// Uses normal verbosity and automatic color detection.
    /// This is suitable for most interactive use cases.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use hpm_cli::console::Console;
    ///
    /// let mut console = Console::new();
    /// console.success("Operation completed successfully");
    /// ```
    pub fn new() -> Self {
        Self {
            verbosity: Verbosity::Normal,
            color: ColorChoice::Auto,
            stderr: stderr(),
            stdout: stdout(),
        }
    }

    /// Create console with specific verbosity and color settings
    ///
    /// Allows full control over console behavior, useful for testing
    /// or when specific output behavior is required.
    ///
    /// # Arguments
    ///
    /// * `verbosity` - Controls how much output is shown
    /// * `color` - Controls when colors are used
    ///
    /// # Examples
    ///
    /// ```rust
    /// use hpm_cli::console::{Console, Verbosity, ColorChoice};
    ///
    /// // Create a quiet console with no colors for scripting
    /// let mut console = Console::with_settings(Verbosity::Quiet, ColorChoice::Never);
    ///
    /// // Create a verbose console with forced colors for debugging
    /// let mut console = Console::with_settings(Verbosity::Verbose, ColorChoice::Always);
    /// ```
    pub fn with_settings(verbosity: Verbosity, color: ColorChoice) -> Self {
        Self {
            verbosity,
            color,
            stderr: stderr(),
            stdout: stdout(),
        }
    }

    /// Print a success message
    ///
    /// Displays a green checkmark with the message, indicating successful completion.
    /// Only shown at Quiet verbosity level and above.
    ///
    /// # Arguments
    ///
    /// * `message` - The success message to display
    ///
    /// # Examples
    ///
    /// ```rust
    /// console.success("Package installed successfully");
    /// // Output: [checkmark] Package installed successfully (in green)
    /// ```
    pub fn success(&mut self, message: impl Display) {
        if self.should_show(Level::Success) {
            self.print_styled(Level::Success, message);
        }
    }

    /// Print an informational message
    ///
    /// Displays a blue info symbol with the message, providing helpful information.
    /// Only shown at Normal verbosity level and above.
    ///
    /// # Arguments
    ///
    /// * `message` - The informational message to display
    ///
    /// # Examples
    ///
    /// ```rust
    /// console.info("Processing dependencies...");
    /// // Output: ℹ Processing dependencies... (in blue)
    /// ```
    pub fn info(&mut self, message: impl Display) {
        if self.should_show(Level::Info) {
            self.print_styled(Level::Info, message);
        }
    }

    /// Print a warning message
    ///
    /// Displays a yellow warning symbol with the message, indicating potential issues.
    /// Only shown at Quiet verbosity level and above.
    ///
    /// # Arguments
    ///
    /// * `message` - The warning message to display
    ///
    /// # Examples
    ///
    /// ```rust
    /// console.warn("This feature is experimental");
    /// // Output: [warning] This feature is experimental (in yellow)
    /// ```
    pub fn warn(&mut self, message: impl Display) {
        if self.should_show(Level::Warning) {
            self.print_styled(Level::Warning, message);
        }
    }

    /// Check if we should show output for a given level
    ///
    /// Internal method that implements the verbosity logic for each message level.
    /// This ensures consistent behavior across all message types.
    fn should_show(&self, level: Level) -> bool {
        match (self.verbosity, level) {
            // Quiet: Essential information (success, warnings)
            (Verbosity::Quiet, Level::Warning | Level::Success) => true,
            (Verbosity::Quiet, _) => false,

            // Normal and Verbose: Show all messages
            (Verbosity::Normal | Verbosity::Verbose, _) => true,
        }
    }

    /// Print a styled message with appropriate prefix and colors
    ///
    /// Internal method that handles the actual output formatting and routing.
    /// Routes errors and warnings to stderr, other messages to stdout.
    fn print_styled(&mut self, level: Level, message: impl Display) {
        let should_color = match self.color {
            ColorChoice::Always => true,
            ColorChoice::Never => false,
            ColorChoice::Auto => console::colors_enabled_stderr(),
        };

        let formatted_message = if should_color {
            self.style_message(level, message)
        } else {
            format!("{}: {}", level.prefix(), message)
        };

        // Route warnings to stderr, everything else to stdout
        match level {
            Level::Warning => {
                writeln!(self.stderr, "{}", formatted_message).ok();
                self.stderr.flush().ok();
            }
            _ => {
                writeln!(self.stdout, "{}", formatted_message).ok();
                self.stdout.flush().ok();
            }
        }
    }

    /// Apply styling to a message based on its level
    ///
    /// Internal method that creates the colored and styled output.
    /// Each level has a specific color scheme and symbol for visual clarity.
    fn style_message(&self, level: Level, message: impl Display) -> String {
        match level {
            Level::Success => format!(
                "{} {}",
                // Unicode check mark (U+2713) - standard CLI success symbol
                "\u{2713}".style(Style::new().green().bold()),
                message.to_string().style(Style::new().green())
            ),
            Level::Info => format!(
                "{} {}",
                // Unicode info symbol (U+2139) - standard CLI info symbol
                "\u{2139}".style(Style::new().blue().bold()),
                message
            ),
            Level::Warning => format!(
                "{} {}",
                // Unicode warning sign (U+26A0) - standard CLI warning symbol
                "\u{26A0}".style(Style::new().yellow().bold()),
                message.to_string().style(Style::new().yellow())
            ),
        }
    }
}

impl Default for Console {
    fn default() -> Self {
        Self::new()
    }
}

impl Level {
    /// Get the text prefix for a message level
    ///
    /// Returns the plain text prefix used when colors are disabled.
    /// This ensures consistent output in both colored and non-colored modes.
    fn prefix(&self) -> &'static str {
        match self {
            Level::Success => "SUCCESS",
            Level::Info => "INFO",
            Level::Warning => "WARNING",
        }
    }
}

// Global convenience functions for quick message output

/// Display a success message using default console settings
pub fn success(message: impl Display) {
    let mut console = Console::new();
    console.success(message);
}

/// Display an informational message using default console settings
pub fn info(message: impl Display) {
    let mut console = Console::new();
    console.info(message);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_verbosity_ordering() {
        assert!(Verbosity::Quiet < Verbosity::Normal);
        assert!(Verbosity::Normal < Verbosity::Verbose);
    }

    #[test]
    fn test_should_show_messages() {
        let console = Console::with_settings(Verbosity::Normal, ColorChoice::Never);

        assert!(console.should_show(Level::Success));
        assert!(console.should_show(Level::Info));
        assert!(console.should_show(Level::Warning));
    }

    #[test]
    fn test_should_show_quiet() {
        let console = Console::with_settings(Verbosity::Quiet, ColorChoice::Never);

        assert!(console.should_show(Level::Success));
        assert!(!console.should_show(Level::Info));
        assert!(console.should_show(Level::Warning));
    }
}
