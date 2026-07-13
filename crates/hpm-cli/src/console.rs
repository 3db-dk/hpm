//! Styled terminal output with verbosity control.
//!
//! One `Console` is constructed in [`crate::run`] and threaded through every
//! command; commands must not construct their own. Two kinds of output:
//!
//! - Result data: [`Console::stdout`] — plain lines on stdout that must
//!   survive `--quiet` (listings, reports, JSON payloads).
//! - Status lines: [`Console::success`] / [`Console::info`] /
//!   [`Console::warn`] / [`Console::error`] (styled, with Unicode markers so
//!   color-blind users still see one) and [`Console::status`] (plain,
//!   supplementary). `info` and `status` are suppressed under `--quiet`;
//!   warnings and errors go to stderr and always print.

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
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
    Error,
}

impl Console {
    pub fn new() -> Self {
        Self::with_settings(Verbosity::Normal, ColorChoice::Auto)
    }

    pub fn with_settings(verbosity: Verbosity, color: ColorChoice) -> Self {
        // Commands embed `console::style(...)` in the strings they print, so
        // an explicit --color choice must flip the crate-global switch too —
        // otherwise only the level markers would obey the flag.
        match color {
            ColorChoice::Always => {
                console::set_colors_enabled(true);
                console::set_colors_enabled_stderr(true);
            }
            ColorChoice::Never => {
                console::set_colors_enabled(false);
                console::set_colors_enabled_stderr(false);
            }
            ColorChoice::Auto => {}
        }
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

    pub fn error(&mut self, message: impl Display) {
        if self.should_show(Level::Error) {
            self.print_styled(Level::Error, message);
        }
    }

    /// Result data: plain line on stdout, printed even under `--quiet`.
    /// Machine-readable payloads (`--output json*`) go through here so they
    /// can never be suppressed.
    pub fn stdout(&mut self, message: impl Display) {
        writeln!(self.stdout, "{message}").ok();
        self.stdout.flush().ok();
    }

    /// Supplementary human-facing line: plain, stdout, suppressed under
    /// `--quiet` (structure trees, follow-up hints, cancellations).
    pub fn status(&mut self, message: impl Display) {
        if self.verbosity > Verbosity::Quiet {
            writeln!(self.stdout, "{message}").ok();
            self.stdout.flush().ok();
        }
    }

    /// Prompt the user with `<label> [y/N]: `. Returns true on `y`/`yes`.
    /// The single interactive-confirmation path for all commands.
    pub fn confirm(&mut self, label: impl Display) -> std::io::Result<bool> {
        use std::io::{BufRead, Write};
        println!();
        print!("{label} [y/N]: ");
        std::io::stdout().flush()?;
        let mut input = String::new();
        std::io::stdin().lock().read_line(&mut input)?;
        let response = input.trim().to_lowercase();
        Ok(response == "y" || response == "yes")
    }

    fn should_show(&self, level: Level) -> bool {
        match (self.verbosity, level) {
            (Verbosity::Quiet, Level::Warning | Level::Success | Level::Error) => true,
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
            Level::Warning | Level::Error => {
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
        // Unicode ballot X (U+2717)
        Level::Error => format!(
            "{} {}",
            style("\u{2717}").red().bold(),
            style(message).red()
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
            Level::Error => "ERROR",
        }
    }
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
        let console = Console::with_settings(Verbosity::Normal, ColorChoice::Auto);
        assert!(console.should_show(Level::Success));
        assert!(console.should_show(Level::Info));
        assert!(console.should_show(Level::Warning));
        assert!(console.should_show(Level::Error));
    }

    #[test]
    fn test_should_show_quiet() {
        let console = Console::with_settings(Verbosity::Quiet, ColorChoice::Auto);
        assert!(console.should_show(Level::Success));
        assert!(!console.should_show(Level::Info));
        assert!(console.should_show(Level::Warning));
        assert!(console.should_show(Level::Error));
    }
}
