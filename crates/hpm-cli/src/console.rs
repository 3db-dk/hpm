//! Styled terminal output with verbosity control.
//!
//! Semantic colors: green for success, yellow for warnings, blue for info.
//! Uses Unicode symbols (checkmark, info, warning) so color-blind users still
//! see a marker. Output is routed to stdout or stderr based on message level.

use console::{Term, style};
use std::fmt::Display;
use std::io::Write;

#[derive(Debug)]
pub struct Console {
    verbosity: Verbosity,
    color: ColorChoice,
    stderr: Term,
    stdout: Term,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Verbosity {
    Quiet,
    Normal,
    Verbose,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorChoice {
    Auto,
    Always,
    Never,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Level {
    Success,
    Info,
    Warning,
}

impl Console {
    pub fn new() -> Self {
        Self::with_settings(Verbosity::Normal, ColorChoice::Auto)
    }

    pub fn with_settings(verbosity: Verbosity, color: ColorChoice) -> Self {
        Self {
            verbosity,
            color,
            stderr: Term::stderr(),
            stdout: Term::stdout(),
        }
    }

    pub fn success(&mut self, message: impl Display) {
        if self.should_show(Level::Success) {
            self.print_styled(Level::Success, message);
        }
    }

    pub fn info(&mut self, message: impl Display) {
        if self.should_show(Level::Info) {
            self.print_styled(Level::Info, message);
        }
    }

    pub fn warn(&mut self, message: impl Display) {
        if self.should_show(Level::Warning) {
            self.print_styled(Level::Warning, message);
        }
    }

    fn should_show(&self, level: Level) -> bool {
        match (self.verbosity, level) {
            (Verbosity::Quiet, Level::Warning | Level::Success) => true,
            (Verbosity::Quiet, _) => false,
            (Verbosity::Normal | Verbosity::Verbose, _) => true,
        }
    }

    fn print_styled(&mut self, level: Level, message: impl Display) {
        let should_color = match self.color {
            ColorChoice::Always => true,
            ColorChoice::Never => false,
            ColorChoice::Auto => console::colors_enabled_stderr(),
        };

        let formatted = if should_color {
            style_message(level, message)
        } else {
            format!("{}: {message}", level.prefix())
        };

        match level {
            Level::Warning => {
                writeln!(self.stderr, "{formatted}").ok();
                self.stderr.flush().ok();
            }
            _ => {
                writeln!(self.stdout, "{formatted}").ok();
                self.stdout.flush().ok();
            }
        }
    }
}

fn style_message(level: Level, message: impl Display) -> String {
    match level {
        // Unicode check mark (U+2713)
        Level::Success => format!(
            "{} {}",
            style("\u{2713}").green().bold(),
            style(message).green()
        ),
        // Unicode info symbol (U+2139)
        Level::Info => format!("{} {message}", style("\u{2139}").blue().bold()),
        // Unicode warning sign (U+26A0)
        Level::Warning => format!(
            "{} {}",
            style("\u{26A0}").yellow().bold(),
            style(message).yellow()
        ),
    }
}

impl Default for Console {
    fn default() -> Self {
        Self::new()
    }
}

impl Level {
    fn prefix(&self) -> &'static str {
        match self {
            Level::Success => "SUCCESS",
            Level::Info => "INFO",
            Level::Warning => "WARNING",
        }
    }
}

/// Convenience: print a success message with default settings.
pub fn success(message: impl Display) {
    Console::new().success(message);
}

/// Convenience: print an info message with default settings.
pub fn info(message: impl Display) {
    Console::new().info(message);
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
