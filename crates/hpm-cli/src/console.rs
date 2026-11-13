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
        // Warnings and errors routinely carry text from registries and
        // manifests; hpm's own styling is added below, after this.
        let message = match level {
            Level::Warning | Level::Error => untrusted(message),
            _ => message.to_string(),
        };
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

/// Text from outside hpm (a registry, a package manifest, an error carrying
/// either), made safe to print: control characters (which start terminal
/// escape sequences that recolour or hide output, set hyperlinks or the
/// window title) and bidirectional overrides (which reorder what is shown)
/// are dropped (Unicode's Bidi_Control set). Everything else, non-ASCII
/// included, is kept.
pub fn untrusted(text: impl Display) -> String {
    text.to_string()
        .chars()
        .filter(|&c| {
            !(c.is_control() && c != '\n' && c != '\t'
                || matches!(c, '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}'))
        })
        .collect()
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

    #[test]
    fn untrusted_text_keeps_text_and_drops_controls() {
        assert_eq!(
            untrusted("Æbleflæsk fx \u{1f525}\nline two\tok"),
            "Æbleflæsk fx \u{1f525}\nline two\tok"
        );
        assert_eq!(
            untrusted("a\u{1b}[2Jb\u{7}c\u{9b}d\u{202e}e\u{2066}f\u{7f}"),
            "a[2Jbcdef"
        );
    }

    mod properties {
        use super::super::untrusted;
        use proptest::prelude::*;

        /// Unicode's Bidi_Control property (PropList.txt).
        fn is_bidi_control(c: char) -> bool {
            matches!(c, '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        }

        /// Text a hostile registry could send: escape sequences (7-bit and
        /// C1), bidi controls, and ordinary text, mixed.
        fn hostile_text() -> impl Strategy<Value = String> {
            let piece = prop_oneof![
                Just("\u{1b}[2J".to_string()),
                Just("\u{1b}]8;;https://x.invalid\u{7}".to_string()),
                Just("\u{9b}31m".to_string()),
                Just("\r".to_string()),
                Just("\u{202e}".to_string()),
                Just("\u{2066}".to_string()),
                Just("\u{061c}".to_string()),
                "[\\x00-\\x1f\\x7f-\\x9f]",
                "\\PC{0,6}",
                any::<char>().prop_map(String::from),
            ];
            prop::collection::vec(piece, 0..10).prop_map(|p| p.concat())
        }

        proptest! {
            #![proptest_config(ProptestConfig::with_cases(1024))]

            /// Cleaned text holds no control character but newline and tab,
            /// and no bidi control; cleaning it again changes nothing; and
            /// what is kept is the input with those characters removed.
            #[test]
            fn cleaned_text_cannot_drive_the_terminal(text in hostile_text()) {
                let clean = untrusted(&text);
                prop_assert!(
                    !clean.chars().any(|c| (c.is_control() && c != '\n' && c != '\t') || is_bidi_control(c)),
                    "{:?}", clean
                );
                prop_assert_eq!(&untrusted(&clean), &clean);
                let expected: String = text
                    .chars()
                    .filter(|&c| !((c.is_control() && c != '\n' && c != '\t') || is_bidi_control(c)))
                    .collect();
                prop_assert_eq!(clean, expected);
            }

            /// Printable text, any script, passes through unchanged.
            #[test]
            fn printable_text_is_kept(text in "[\\PC\n\t]{0,40}") {
                prop_assume!(!text.chars().any(is_bidi_control));
                prop_assert_eq!(untrusted(&text), text);
            }
        }
    }
}
