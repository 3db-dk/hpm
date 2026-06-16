//! `hpm run <script>` — execute a `[scripts]` entry from `hpm.toml`.
//!
//! Plain string entries shell out directly. Table-form entries with `python`
//! or `requirements` (see [`hpm_package::ScriptEntry`]) get a uv-managed
//! venv on demand and run with that interpreter on PATH.

use anyhow::{Context, Result};
use hpm_core::python::prepare_script_env;
use hpm_core::{PackageRunEnv, ProjectManager, StorageManager};
use hpm_package::Platform;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
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
    extra_env: &HashMap<String, String>,
    console: &mut Console,
) -> Result<i32> {
    let manifest_path = determine_manifest_path(directory)?;
    let manifest = load_manifest(&manifest_path)?;
    let package_root = manifest_path
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));

    let entry = manifest.script_for(script).with_context(|| {
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

    let mut env_vars: HashMap<String, String> = HashMap::new();
    env_vars.insert(
        "HPM_PACKAGE_ROOT".to_string(),
        package_root.to_string_lossy().into_owned(),
    );
    // Caller-supplied context (build profile, target platform). Applied
    // before the managed env so a managed PATH/VIRTUAL_ENV/PYTHONPATH wins.
    for (key, value) in extra_env {
        env_vars.insert(key.clone(), value.clone());
    }

    if entry.uses_package_env() {
        // Run inside the package's full resolved environment: merged venv +
        // every involved package's python/ on PYTHONPATH. Resolved read-only
        // from hpm.lock + the global store via ProjectManager.
        console.info("Preparing package environment");
        let run_env = resolve_package_env(&package_root, entry.requirements())
            .await
            .with_context(|| format!("Preparing package environment for script '{}'", script))?;
        apply_package_env(&run_env, &mut env_vars);
        if let Some(bin) = &run_env.venv_bin {
            debug!(
                "hpm run {}: using package venv bin {}",
                script,
                bin.display()
            );
        }
    } else {
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
        env_handle.apply_to(&mut env_vars);
    }

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

/// Resolve the package environment for a `package-env` script by building a
/// `ProjectManager` rooted at the project and delegating to its read-only
/// resolver. Config is loaded lazily here — only `package-env` scripts pay
/// for it; plain and per-script-venv runs don't touch the project layer.
async fn resolve_package_env(
    package_root: &Path,
    extra_requirements: &[String],
) -> Result<PackageRunEnv> {
    let config = hpm_config::Config::load().context("Failed to load HPM configuration")?;
    let storage_manager = Arc::new(
        StorageManager::new(config.storage.clone())
            .context("Failed to initialize package storage")?,
    );
    let project_manager = ProjectManager::new(
        package_root.to_path_buf(),
        storage_manager,
        Arc::new(config),
    )
    .context("Failed to open project")?;
    project_manager
        .resolve_package_env(extra_requirements)
        .await
        .map_err(Into::into)
}

/// Fold a [`PackageRunEnv`] into the subprocess env map: `VIRTUAL_ENV`,
/// `PATH` (venv bin prepended), and `PYTHONPATH` (package python/ dirs +
/// venv site-packages prepended). Mirrors `ScriptEnvHandle::apply_to`'s
/// prepend semantics so an existing `PATH`/`PYTHONPATH` is preserved.
fn apply_package_env(run_env: &PackageRunEnv, env_vars: &mut HashMap<String, String>) {
    if let Some(virtual_env) = &run_env.virtual_env {
        env_vars.insert(
            "VIRTUAL_ENV".to_string(),
            virtual_env.to_string_lossy().into_owned(),
        );
    }
    if let Some(bin) = &run_env.venv_bin {
        prepend_env_paths(env_vars, "PATH", std::slice::from_ref(bin));
    }
    prepend_env_paths(env_vars, "PYTHONPATH", &run_env.python_paths);
}

/// Prepend `prefixes` to the path-list env var `key` (in `env_vars`, falling
/// back to the process env), joined by the platform separator. No-op when
/// `prefixes` is empty.
fn prepend_env_paths(env_vars: &mut HashMap<String, String>, key: &str, prefixes: &[PathBuf]) {
    if prefixes.is_empty() {
        return;
    }
    let separator = if cfg!(target_os = "windows") {
        ";"
    } else {
        ":"
    };
    let mut parts: Vec<String> = prefixes
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    let existing = env_vars
        .get(key)
        .cloned()
        .or_else(|| std::env::var(key).ok())
        .unwrap_or_default();
    if !existing.is_empty() {
        parts.push(existing);
    }
    env_vars.insert(key.to_string(), parts.join(separator));
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

    #[test]
    fn prepend_env_paths_is_noop_when_empty() {
        let mut env = HashMap::new();
        env.insert("PYTHONPATH".to_string(), "/existing".to_string());
        prepend_env_paths(&mut env, "PYTHONPATH", &[]);
        assert_eq!(env.get("PYTHONPATH").map(String::as_str), Some("/existing"));
    }

    #[test]
    fn prepend_env_paths_prepends_in_order_preserving_existing() {
        let sep = if cfg!(target_os = "windows") {
            ";"
        } else {
            ":"
        };
        let mut env = HashMap::new();
        env.insert("PYTHONPATH".to_string(), "/existing".to_string());
        prepend_env_paths(
            &mut env,
            "PYTHONPATH",
            &[PathBuf::from("/pkg/python"), PathBuf::from("/dep/python")],
        );
        assert_eq!(
            env.get("PYTHONPATH").unwrap(),
            &format!("/pkg/python{sep}/dep/python{sep}/existing")
        );
    }

    #[test]
    fn apply_package_env_sets_virtual_env_path_and_pythonpath() {
        let run_env = PackageRunEnv {
            venv_bin: Some(PathBuf::from("/venv/bin")),
            virtual_env: Some(PathBuf::from("/venv")),
            python_paths: vec![PathBuf::from("/pkg/python"), PathBuf::from("/venv/site")],
        };
        let mut env = HashMap::new();
        env.insert("PATH".to_string(), "/usr/bin".to_string());
        apply_package_env(&run_env, &mut env);

        assert_eq!(env.get("VIRTUAL_ENV").map(String::as_str), Some("/venv"));
        assert!(env.get("PATH").unwrap().starts_with("/venv/bin"));
        assert!(env.get("PATH").unwrap().ends_with("/usr/bin"));
        let pp = env.get("PYTHONPATH").unwrap();
        assert!(pp.starts_with("/pkg/python"));
        assert!(pp.contains("/venv/site"));
    }

    #[test]
    fn apply_package_env_without_venv_only_sets_pythonpath() {
        // A package with python/ dirs but no Python deps: no venv, but the
        // dirs still land on PYTHONPATH.
        let run_env = PackageRunEnv {
            venv_bin: None,
            virtual_env: None,
            python_paths: vec![PathBuf::from("/pkg/python")],
        };
        let mut env = HashMap::new();
        // Stage an empty PYTHONPATH so the prepend doesn't fall back to the
        // test process's own PYTHONPATH (keeps the assertion deterministic).
        env.insert("PYTHONPATH".to_string(), String::new());
        apply_package_env(&run_env, &mut env);
        assert!(!env.contains_key("VIRTUAL_ENV"));
        assert_eq!(
            env.get("PYTHONPATH").map(String::as_str),
            Some("/pkg/python")
        );
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn posix_quote_handles_single_quotes() {
        // `hpm run x -- "it's"` should survive the trip through `sh -c`.
        let q = shell_quote("it's");
        assert_eq!(q, "'it'\\''s'");
    }
}
