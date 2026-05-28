//! `hpm run <script>` — execute a `[scripts]` entry from `hpm.toml`.
//!
//! Plain string entries shell out directly. Table-form entries with `python`
//! or `requirements` (see [`hpm_package::ScriptEntry`]) get a uv-managed
//! venv on demand and run with that interpreter on PATH.

use anyhow::{Context, Result};
use hpm_package::Platform;
use hpm_python::prepare_script_env;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;
use tracing::debug;

use crate::commands::manifest_utils::{determine_manifest_path, load_manifest};
use crate::console::Console;

/// Run the named `[scripts]` entry in `hpm.toml`, forwarding `extra_args`.
///
/// Returns the script's exit code so the caller can propagate it as the
/// `hpm` exit status.
pub async fn run_script(
    script: &str,
    extra_args: &[String],
    directory: Option<PathBuf>,
    console: &mut Console,
) -> Result<i32> {
    let manifest_path = determine_manifest_path(directory)?;
    let manifest = load_manifest(&manifest_path)?;
    let package_root = manifest_path
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));

    let entry = manifest
        .script_for(script, Platform::current())
        .with_context(|| {
            format!(
                "No script '{}' defined in {}",
                script,
                manifest_path.display()
            )
        })?;

    let host_os = Platform::current().and_then(|p| p.os_key().map(str::to_string));
    let resolved_cmd = entry.resolve_cmd(host_os.as_deref()).with_context(|| {
        format!(
            "Script '{}' has no command for host OS {} — its conditional cmd only matches other platforms",
            script,
            host_os.as_deref().unwrap_or("<unknown>")
        )
    })?;
    let cmd_string = build_command_string(&resolved_cmd, extra_args);
    debug!("hpm run {}: {}", script, cmd_string);

    if entry.needs_venv() && !entry.requirements().is_empty() {
        console.info(format!(
            "Preparing script venv ({} requirement(s))",
            entry.requirements().len()
        ));
    }
    let env_handle = prepare_script_env(&entry)
        .await
        .with_context(|| format!("Preparing environment for script '{}'", script))?;
    if let Some(venv_bin) = &env_handle.path_prepend {
        debug!("hpm run {}: using venv bin {}", script, venv_bin.display());
    }

    let mut env_vars: HashMap<String, String> = HashMap::new();
    env_vars.insert(
        "HPM_PACKAGE_ROOT".to_string(),
        package_root.to_string_lossy().into_owned(),
    );
    env_handle.apply_to(&mut env_vars);

    let mut command = shell_command(&cmd_string);
    command.current_dir(&package_root).envs(&env_vars);

    let status = command
        .status()
        .with_context(|| format!("Failed to spawn script '{}'", script))?;

    let exit_code = status.code().unwrap_or(1);
    if exit_code != 0 {
        console.warn(format!(
            "Script '{}' exited with status {}",
            script, exit_code
        ));
    }

    Ok(exit_code)
}

fn build_command_string(cmd: &str, extra_args: &[String]) -> String {
    if extra_args.is_empty() {
        cmd.to_string()
    } else {
        let mut out = cmd.to_string();
        for arg in extra_args {
            out.push(' ');
            out.push_str(&shell_quote(arg));
        }
        out
    }
}

fn shell_command(cmd: &str) -> Command {
    #[cfg(target_os = "windows")]
    {
        let mut c = Command::new("cmd");
        c.arg("/C").arg(cmd);
        c
    }
    #[cfg(not(target_os = "windows"))]
    {
        let mut c = Command::new("sh");
        c.arg("-c").arg(cmd);
        c
    }
}

/// Minimal POSIX/cmd shell quoting for trailing-arg pass-through.
///
/// Not a general-purpose shell-quoter — `hpm run` forwards CLI args, which
/// don't contain newlines or NULs in practice. POSIX path: single-quote and
/// escape embedded single quotes via `'\''`. Windows path: double-quote and
/// escape embedded double quotes — good enough for the values cmd.exe will
/// accept.
fn shell_quote(arg: &str) -> String {
    #[cfg(target_os = "windows")]
    {
        let escaped = arg.replace('"', "\\\"");
        format!("\"{}\"", escaped)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let escaped = arg.replace('\'', "'\\''");
        format!("'{}'", escaped)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_command_appends_args() {
        let out = build_command_string("python scripts/x.py", &["--foo".into(), "bar".into()]);
        // Each arg is quoted; both should be present after the base cmd.
        assert!(out.starts_with("python scripts/x.py "));
        assert!(out.contains("--foo"));
        assert!(out.contains("bar"));
    }

    #[test]
    fn build_command_no_args_is_passthrough() {
        let out = build_command_string("ruff .", &[]);
        assert_eq!(out, "ruff .");
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn posix_quote_handles_single_quotes() {
        // `hpm run x -- "it's"` should survive the trip through `sh -c`.
        let q = shell_quote("it's");
        assert_eq!(q, "'it'\\''s'");
    }
}
