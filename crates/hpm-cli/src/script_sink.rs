//! [`ScriptSink`] implementation for the CLI.
//!
//! Streams script diagnostics to the [`Console`] and spawns the prepared
//! command through the host shell (`sh -c` / `cmd /S /C`), letting the child
//! inherit stdio so its output reaches the terminal directly. The shared
//! runner in [`hpm_core::script_run`] owns env composition and the inner
//! command line; this sink owns the spawn.

use anyhow::{Context, Result};
use async_trait::async_trait;
use hpm_core::script_run::{PreparedScript, ScriptSink};
use std::process::Command;

use crate::console::Console;

/// A [`ScriptSink`] that reports through the CLI's [`Console`] and spawns
/// scripts via the host shell with inherited stdio.
pub struct ConsoleSink<'a> {
    console: &'a mut Console,
}

impl<'a> ConsoleSink<'a> {
    pub fn new(console: &'a mut Console) -> Self {
        Self { console }
    }
}

#[async_trait]
impl ScriptSink for ConsoleSink<'_> {
    fn info(&mut self, message: &str) {
        self.console.info(message);
    }

    fn warn(&mut self, message: &str) {
        self.console.warn(message);
    }

    async fn run(&mut self, script: &PreparedScript) -> Result<i32> {
        let mut command = shell_command(&script.command_line);
        command.current_dir(&script.working_dir).envs(&script.env);
        let status = command
            .status()
            .with_context(|| format!("Failed to spawn script '{}'", script.name))?;
        Ok(status.code().unwrap_or(1))
    }
}

/// Wrap `cmd` in the host shell so it runs as a single command line.
fn shell_command(cmd: &str) -> Command {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        let mut c = Command::new("cmd");
        // Build the command line verbatim with `raw_arg`. If we used the
        // normal `.arg(cmd)`, Rust would re-escape our already-`"`-quoted
        // forwarded args (turning `"1001"` into `\"1001\"`); cmd.exe passes
        // the backslashes through literally and the child's CRT then parses
        // them as literal quote characters in argv. `/S` makes cmd strip
        // exactly the outer quote pair and run the rest of the line intact.
        c.raw_arg("/S /C");
        c.raw_arg(windows_cmd_quote(cmd));
        c
    }
    #[cfg(not(target_os = "windows"))]
    {
        let mut c = Command::new("sh");
        c.arg("-c").arg(cmd);
        c
    }
}

/// Wrap a full command line in the outer quote pair that `cmd /S /C` expects.
///
/// `/S` removes the first and last `"` of the argument and treats everything
/// in between as the command, so a single enclosing pair lets the inner
/// per-arg quoting reach the child process unchanged.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn windows_cmd_quote(cmd: &str) -> String {
    format!("\"{cmd}\"")
}

#[cfg(test)]
mod tests {
    use super::windows_cmd_quote;

    #[test]
    fn windows_cmd_quote_wraps_in_single_outer_pair() {
        // `cmd /S /C` strips exactly the outer pair, so the inner command —
        // including any per-arg `"` quoting — must survive verbatim and must
        // NOT be re-escaped to `\"`. The `\"` rewrite is what produced literal
        // `"1001"` args in the child's argv on Windows.
        let cmd = r#"python probe.py "foo" "1001""#;
        let wrapped = windows_cmd_quote(cmd);
        assert!(wrapped.starts_with('"') && wrapped.ends_with('"'));
        assert!(!wrapped.contains("\\\""));
        // Strip the outer pair the way `/S` does: the inner text must be the
        // original command string, byte-for-byte.
        let inner = &wrapped[1..wrapped.len() - 1];
        assert_eq!(inner, cmd);
    }
}
