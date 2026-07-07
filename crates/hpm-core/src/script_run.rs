//! Shared `[scripts]` runner.
//!
//! Composes the full environment and command line for a `[scripts]` entry —
//! the per-script venv (`python` / `requirements`), the package environment
//! (`package-env = true`), and the `HPM_PACKAGE_ROOT` / caller-supplied
//! context vars — then hands a [`PreparedScript`] to an embedder-supplied
//! [`ScriptSink`] for spawning.
//!
//! This is the single place the script-env contract lives. `hpm run`, `hpm
//! build`'s prepack loop, and out-of-process embedders (the tumbletrove
//! desktop hook/build runners) all route through here, so a manifest feature
//! picked up by one is picked up by all of them — no per-embedder drift.
//!
//! The split of responsibilities is deliberate: this crate owns *what env a
//! script needs* and *what command line to run* (including per-arg quoting of
//! forwarded args), while the [`ScriptSink`] owns *how to spawn* — `hpm run`
//! shells out via `sh -c` / `cmd /S /C` and streams to the terminal, the
//! desktop direct-spawns and streams to xterm events plus a build log. Each
//! embedder wraps [`PreparedScript::command_line`] in its own shell.

use crate::project::PackageRunEnv;
use anyhow::{Context, Result, anyhow, bail};
use async_trait::async_trait;
use hpm_package::{PackageManifest, Platform, ScriptEntry};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::debug;

/// A fully-resolved script invocation, ready for an embedder to spawn.
///
/// Produced by [`prepare_script`]. The [`ScriptSink`] wraps
/// [`command_line`](Self::command_line) in its own shell, sets the child's
/// working directory to [`working_dir`](Self::working_dir), and overlays
/// [`env`](Self::env) on the inherited environment.
#[derive(Debug, Clone)]
pub struct PreparedScript {
    /// The `[scripts]` entry name (for diagnostics).
    pub name: String,
    /// The command line: the resolved `cmd` with any forwarded args appended
    /// and shell-quoted. The sink wraps this in `sh -c` / `cmd /S /C` (or its
    /// own spawn primitive) to run it.
    pub command_line: String,
    /// Working directory for the child — the package root.
    pub working_dir: PathBuf,
    /// Environment overlay to set on the child: `HPM_PACKAGE_ROOT`, the
    /// caller's `extra_env` (e.g. `HPM_BUILD_PROFILE` / `HPM_PLATFORM`), and
    /// any `PATH` / `VIRTUAL_ENV` / `PYTHONPATH` from the resolved venv or
    /// package environment.
    pub env: HashMap<String, String>,
}

/// Embedder-supplied diagnostics + spawn surface for the shared runner.
///
/// [`prepare_script`] emits progress through [`info`](Self::info); the runner
/// helpers ([`run_script`], [`run_prepack`]) emit status and spawn through
/// [`run`](Self::run). The CLI implements this over its `Console` and a
/// `sh -c` / `cmd /S /C` spawn; the desktop implements it over xterm events
/// and its own process spawn.
// `Send` so `run_prepack`/`run_script`'s `&mut dyn ScriptSink` futures stay
// `Send` — embedders that drive the runner from a multi-threaded executor
// (the tumbletrove desktop awaits it inside `tauri::async_runtime::spawn` and
// async Tauri commands, which require `Send + 'static`) can't use the trait
// object otherwise. The CLI's `block_on` doesn't need it, but the bound is
// harmless there: its `Console` sink is already `Send`.
#[async_trait]
pub trait ScriptSink: Send {
    /// Emit an informational status line (e.g. `prepack: build-sops`).
    /// Default no-op so terse embedders can ignore it.
    fn info(&mut self, message: &str) {
        let _ = message;
    }

    /// Emit a warning line (e.g. a non-zero script exit). Default no-op.
    fn warn(&mut self, message: &str) {
        let _ = message;
    }

    /// Spawn `script`, stream its output, and return the process exit code.
    async fn run(&mut self, script: &PreparedScript) -> Result<i32>;
}

/// Compose the environment and command line for `entry`.
///
/// Resolves the `cmd` for the host OS, appends and quotes `extra_args`, and
/// builds the env overlay: `HPM_PACKAGE_ROOT`, the caller's `extra_env`, then
/// either the per-script venv ([`prepare_script_env`](crate::python::prepare_script_env))
/// or — when `package-env = true` — the package's full resolved environment.
/// Caller `extra_env` is applied before the managed env so a managed
/// `PATH` / `VIRTUAL_ENV` / `PYTHONPATH` wins.
///
/// `sink` receives prep progress (`Preparing package environment`, etc.).
pub async fn prepare_script(
    entry: &ScriptEntry,
    name: &str,
    package_root: &Path,
    extra_args: &[String],
    extra_env: &HashMap<String, String>,
    sink: &mut dyn ScriptSink,
) -> Result<PreparedScript> {
    let host_os = Platform::current().and_then(|p| p.os_key().map(str::to_string));
    let resolved_cmd = entry.resolve_cmd(host_os.as_deref()).with_context(|| {
        format!(
            "Script '{}' has no command for host OS {} — its conditional cmd only matches other platforms",
            name,
            host_os.as_deref().unwrap_or("<unknown>")
        )
    })?;
    let command_line = build_command_string(&resolved_cmd, extra_args);
    debug!("hpm script {}: {}", name, command_line);

    let mut env: HashMap<String, String> = HashMap::new();
    env.insert(
        "HPM_PACKAGE_ROOT".to_string(),
        package_root.to_string_lossy().into_owned(),
    );
    // Caller-supplied context (build profile, target platform). Applied
    // before the managed env so a managed PATH/VIRTUAL_ENV/PYTHONPATH wins.
    for (key, value) in extra_env {
        env.insert(key.clone(), value.clone());
    }

    if entry.uses_package_env() {
        // Run inside the package's full resolved environment: merged venv +
        // every involved package's python/ on PYTHONPATH. Resolved read-only
        // from hpm.lock + the global store via ProjectManager.
        sink.info("Preparing package environment");
        let run_env = resolve_package_env(package_root, entry.requirements())
            .await
            .with_context(|| format!("Preparing package environment for script '{}'", name))?;
        apply_package_env(&run_env, &mut env);
        if let Some(bin) = &run_env.venv_bin {
            debug!(
                "hpm script {}: using package venv bin {}",
                name,
                bin.display()
            );
        }
    } else {
        if entry.needs_venv() && !entry.requirements().is_empty() {
            sink.info(&format!(
                "Preparing script venv ({} requirement(s))",
                entry.requirements().len()
            ));
        }
        let env_handle = crate::python::prepare_script_env(entry)
            .await
            .with_context(|| format!("Preparing environment for script '{}'", name))?;
        if let Some(venv_bin) = &env_handle.path_prepend {
            debug!("hpm script {}: using venv bin {}", name, venv_bin.display());
        }
        env_handle.apply_to(&mut env);
    }

    Ok(PreparedScript {
        name: name.to_string(),
        command_line,
        working_dir: package_root.to_path_buf(),
        env,
    })
}

/// Resolve and run a single named `[scripts]` entry from `manifest`, returning
/// its exit code. Diagnostics and spawn go through `sink`.
///
/// `extra_args` are forwarded to the script (quoted into the command line);
/// `extra_env` is overlaid as caller context (build profile, target platform).
pub async fn run_script(
    manifest: &PackageManifest,
    name: &str,
    package_root: &Path,
    extra_args: &[String],
    extra_env: &HashMap<String, String>,
    sink: &mut dyn ScriptSink,
) -> Result<i32> {
    let entry = manifest
        .script_for(name)
        .ok_or_else(|| anyhow!("No script '{}' defined in package manifest", name))?;
    let prepared = prepare_script(&entry, name, package_root, extra_args, extra_env, sink).await?;
    let code = sink.run(&prepared).await?;
    if code != 0 {
        sink.warn(&format!("Script '{}' exited with status {}", name, code));
    }
    Ok(code)
}

/// Run a `[stage].prepack` sequence: each named script in order, aborting on
/// the first non-zero exit. Shared by `hpm build` and out-of-process embedders
/// that materialise install images themselves.
///
/// Each `name` must resolve to a `[scripts]` entry; an unknown name is a hard
/// error before anything spawns. `extra_env` carries the build context
/// (`HPM_BUILD_PROFILE`, `HPM_PLATFORM`, and optionally `HPM_HOUDINI_MAJORS`)
/// onto every prepack script.
pub async fn run_prepack(
    manifest: &PackageManifest,
    names: &[String],
    package_root: &Path,
    extra_env: &HashMap<String, String>,
    sink: &mut dyn ScriptSink,
) -> Result<()> {
    for name in names {
        let entry = manifest.script_for(name).ok_or_else(|| {
            anyhow!(
                "[stage].prepack references '{}' but no such [scripts] entry exists",
                name
            )
        })?;
        sink.info(&format!("prepack: {}", name));
        let prepared = prepare_script(&entry, name, package_root, &[], extra_env, sink).await?;
        let code = sink.run(&prepared).await?;
        if code != 0 {
            bail!(
                "Prepack script '{}' exited with status {} — aborting build",
                name,
                code
            );
        }
    }
    Ok(())
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
        crate::StorageManager::new(config.storage.clone())
            .context("Failed to initialize package storage")?,
    );
    let project_manager = crate::ProjectManager::new(
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

/// Build the command line: the base `cmd` with each forwarded arg quoted and
/// appended. The per-arg quoting targets the shell the sink will spawn into
/// (`sh -c` / `cmd /S /C`); the sink supplies the outer wrapping.
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
