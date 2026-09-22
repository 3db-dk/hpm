//! Shared `[scripts]` runner.
//!
//! Composes the full environment and command line for a `[scripts]` entry —
//! the per-script venv (`python` / `requirements`), the package environment
//! (`package-env = true`), and the `HPM_PACKAGE_ROOT` / caller-supplied
//! context vars — then hands a [`PreparedScript`] to an embedder-supplied
//! [`ScriptSink`] for spawning.
//!
//! This is the single place the script-env contract lives. `hpm run`, `hpm
//! build`'s prepack loop, and out-of-process embedders (GUI hook/build
//! runners) all route through here, so a manifest feature picked up by one is
//! picked up by all of them — no per-embedder drift.
//!
//! The split of responsibilities is deliberate: this crate owns *what env a
//! script needs* and *what command line to run* (including per-arg quoting of
//! forwarded args), while the [`ScriptSink`] owns *how to spawn* — `hpm run`
//! shells out via `sh -c` / `cmd /S /C` and streams to the terminal, while an
//! embedder may stream to its own terminal widget and log instead.
//!
//! A sink runs [`PreparedScript::command_line`] through a shell. That is the
//! contract `[scripts]` publishes (see `docs/user-guide.md`, `hpm run`), so
//! every `cmd` in every released package was written expecting shell
//! semantics — globbing, `~`, word splitting, `$VAR` expanding to empty when
//! unset. A sink that tokenizes the string and spawns argv directly is not
//! implementing the same contract: it has to reimplement a shell, and every
//! detail its dialect gets differently is a package that behaves one way
//! under `hpm run` and another under that embedder. There is no structured
//! form to hand out instead — `cmd` is a single string from the manifest down
//! — so a sink that needs one is asking for a second dialect, not for one
//! that already exists. Wrap the string in a shell, and suppress the console
//! window on Windows with `CREATE_NO_WINDOW` if that is the concern.

use crate::project::{PackageRunEnv, ProjectError};
use crate::python::PythonError;
use crate::storage::StorageError;
use async_trait::async_trait;
use hpm_package::{PackageManifest, Platform, ScriptEntry};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::debug;

/// Failures raised by the shared `[scripts]` runner.
///
/// The [`ScriptSink`] trait itself stays on `anyhow::Result` — out-of-repo
/// embedders implement it — so sink failures are erased into
/// [`Sink`](Self::Sink) at the call sites instead of leaking `anyhow` into
/// this enum.
#[derive(Debug, thiserror::Error)]
pub enum ScriptRunError {
    #[error("No script '{name}' defined in package manifest")]
    ScriptNotFound { name: String },

    #[error("[stage].prepack references '{name}' but no such [scripts] entry exists")]
    PrepackScriptNotFound { name: String },

    #[error(
        "Script '{name}' has no command for host OS {host_os} — its conditional \
         cmd only matches other platforms"
    )]
    NoCommandForHost { name: String, host_os: String },

    #[error("Prepack script '{name}' exited with status {code} — aborting build")]
    PrepackExit { name: String, code: i32 },

    /// Per-script venv preparation failed (uv bootstrap, interpreter
    /// download, dependency resolve).
    #[error("Preparing environment for script '{name}'")]
    EnvPreparation {
        name: String,
        #[source]
        source: PythonError,
    },

    /// A `package-env` script's project environment could not be resolved.
    /// Boxed: `ProjectError` is large and this is a cold path.
    #[error("Preparing package environment for script '{name}'")]
    PackageEnv {
        name: String,
        #[source]
        source: Box<ProjectError>,
    },

    #[error("Failed to load HPM configuration")]
    Config(#[from] hpm_package::TomlFileError),

    /// Global package storage could not be initialized. Boxed; see
    /// [`PackageEnv`](Self::PackageEnv).
    #[error("Failed to initialize package storage")]
    Storage(#[source] Box<StorageError>),

    /// The embedder's [`ScriptSink::run`] failed. Erased to a trait object
    /// so this enum has no structural dependence on `anyhow`.
    #[error(transparent)]
    Sink(Box<dyn std::error::Error + Send + Sync>),
}

// Hand-written so call sites can `?` from the unboxed error type; see
// `ProjectError`'s From impls for the same pattern.
impl From<StorageError> for ScriptRunError {
    fn from(err: StorageError) -> Self {
        Self::Storage(Box::new(err))
    }
}

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
/// `sh -c` / `cmd /S /C` spawn; a GUI embedder may implement it over terminal
/// events and its own process spawn.
// `Send` so `run_prepack`/`run_script`'s `&mut dyn ScriptSink` futures stay
// `Send` — embedders that drive the runner from a multi-threaded executor
// (awaiting it inside a spawned task that requires `Send + 'static`) can't use
// the trait object otherwise. The CLI's `block_on` doesn't need it, but the
// bound is harmless there: its `Console` sink is already `Send`.
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
    ///
    /// Stays on `anyhow::Result` deliberately — embedders outside this repo
    /// implement the trait; the runner maps failures into
    /// [`ScriptRunError::Sink`].
    async fn run(&mut self, script: &PreparedScript) -> anyhow::Result<i32>;
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
) -> Result<PreparedScript, ScriptRunError> {
    let host_os = Platform::current().and_then(|p| p.os_key().map(str::to_string));
    let resolved_cmd =
        entry
            .resolve_cmd(host_os.as_deref())
            .ok_or_else(|| ScriptRunError::NoCommandForHost {
                name: name.to_string(),
                host_os: host_os.as_deref().unwrap_or("<unknown>").to_string(),
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
        let run_env = resolve_package_env(package_root, entry.requirements(), name).await?;
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
            .map_err(|e| ScriptRunError::EnvPreparation {
                name: name.to_string(),
                source: e,
            })?;
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
) -> Result<i32, ScriptRunError> {
    let entry = manifest
        .script_for(name)
        .ok_or_else(|| ScriptRunError::ScriptNotFound {
            name: name.to_string(),
        })?;
    let prepared = prepare_script(&entry, name, package_root, extra_args, extra_env, sink).await?;
    let code = sink
        .run(&prepared)
        .await
        .map_err(|e| ScriptRunError::Sink(e.into()))?;
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
) -> Result<(), ScriptRunError> {
    for name in names {
        let entry =
            manifest
                .script_for(name)
                .ok_or_else(|| ScriptRunError::PrepackScriptNotFound {
                    name: name.to_string(),
                })?;
        sink.info(&format!("prepack: {}", name));
        let prepared = prepare_script(&entry, name, package_root, &[], extra_env, sink).await?;
        let code = sink
            .run(&prepared)
            .await
            .map_err(|e| ScriptRunError::Sink(e.into()))?;
        if code != 0 {
            return Err(ScriptRunError::PrepackExit {
                name: name.clone(),
                code,
            });
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
    name: &str,
) -> Result<PackageRunEnv, ScriptRunError> {
    let package_env = |e: ProjectError| ScriptRunError::PackageEnv {
        name: name.to_string(),
        source: Box::new(e),
    };
    let config = hpm_config::Config::load()?;
    let storage_manager = Arc::new(crate::StorageManager::new(config.storage.clone())?);
    let project_manager = crate::ProjectManager::new(
        package_root.to_path_buf(),
        storage_manager,
        Arc::new(config),
    )
    .map_err(package_env)?;
    project_manager
        .resolve_package_env(extra_requirements)
        .await
        .map_err(package_env)
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
/// don't contain newlines or NULs in practice. The per-arg quoting targets
/// the shell the sink will spawn into (`sh -c` / `cmd /S /C`); the sink
/// supplies the outer wrapping.
fn shell_quote(arg: &str) -> String {
    #[cfg(target_os = "windows")]
    {
        windows_quote(arg)
    }
    #[cfg(not(target_os = "windows"))]
    {
        posix_quote(arg)
    }
}

/// Single-quote and escape embedded single quotes via `'\''`. Robust for
/// arbitrary content, since nothing inside `'…'` is special to a POSIX shell.
#[cfg_attr(target_os = "windows", allow(dead_code))]
fn posix_quote(arg: &str) -> String {
    format!("'{}'", arg.replace('\'', "'\\''"))
}

/// Double-quote for the Windows command line, following the backslash rule
/// `CommandLineToArgvW` and the MSVC CRT parse argv with: a run of N
/// backslashes is literal on its own, but means N/2 backslashes when it
/// precedes a `"`. So a run that lands before a quote — an embedded one or
/// the closing one — has to be doubled, or the quote it precedes is read as
/// data and the argument never ends.
///
/// The naive `arg.replace('"', "\\\"")` this replaces got both halves wrong.
/// `C:\out\` came out as `"C:\out\"`, whose closing quote the CRT reads as a
/// literal `"` — so a trailing-separator path (which is what `dirname`-style
/// shell plumbing and tab-completion produce) swallowed the rest of the
/// command line. And an argument already containing `\"` was escaped to
/// `\\"`, which terminates the argument instead of quoting it.
///
/// One limitation has no fix at this layer and is not attempted: `cmd.exe`
/// expands `%VAR%` inside double quotes, and its command-line parser (unlike
/// a batch file's) has no escape for `%` — `^` is literal within quotes and
/// `%%` is not collapsed. A forwarded arg containing `%NAME%` where `NAME` is
/// set in the environment therefore reaches the child expanded. An unmatched
/// `%`, or a name that is not set, passes through as written.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn windows_quote(arg: &str) -> String {
    let mut out = String::with_capacity(arg.len() + 2);
    out.push('"');
    let mut backslashes = 0usize;
    for ch in arg.chars() {
        match ch {
            '\\' => {
                backslashes += 1;
                out.push('\\');
            }
            '"' => {
                // Double the run that precedes this quote, then escape the
                // quote itself so the CRT reads it as data.
                out.extend(std::iter::repeat_n('\\', backslashes + 1));
                backslashes = 0;
                out.push('"');
            }
            other => {
                backslashes = 0;
                out.push(other);
            }
        }
    }
    // Same rule for the closing quote: a trailing run would otherwise escape
    // it and leave the argument unterminated.
    out.extend(std::iter::repeat_n('\\', backslashes));
    out.push('"');
    out
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

    #[test]
    fn posix_quote_handles_single_quotes() {
        // `hpm run x -- "it's"` should survive the trip through `sh -c`.
        assert_eq!(posix_quote("it's"), "'it'\\''s'");
        // Nothing else is special inside `'…'`, so a backslash, a double
        // quote and a metacharacter all pass through as written.
        assert_eq!(posix_quote(r"C:\out\"), r"'C:\out\'");
        assert_eq!(posix_quote("a b | c"), "'a b | c'");
    }

    /// Parse one fully-quoted argument the way `CommandLineToArgvW` and the
    /// MSVC CRT do, so the quoting can be checked by round-trip rather than
    /// by asserting a literal spelling. A run of N backslashes before a `"`
    /// yields N/2 backslashes, and the quote is data when N was odd and a
    /// mode toggle when it was even; a run not followed by `"` is literal.
    fn crt_parse_single(command_line: &str) -> String {
        let mut out = String::new();
        let mut in_quotes = false;
        let mut backslashes = 0usize;
        for ch in command_line.chars() {
            match ch {
                '\\' => backslashes += 1,
                '"' => {
                    out.extend(std::iter::repeat_n('\\', backslashes / 2));
                    if backslashes % 2 == 1 {
                        out.push('"');
                    } else {
                        in_quotes = !in_quotes;
                    }
                    backslashes = 0;
                }
                other => {
                    out.extend(std::iter::repeat_n('\\', backslashes));
                    backslashes = 0;
                    assert!(
                        in_quotes || !other.is_whitespace(),
                        "unquoted whitespace splits the argument: {command_line:?}"
                    );
                    out.push(other);
                }
            }
        }
        out.extend(std::iter::repeat_n('\\', backslashes));
        assert!(!in_quotes, "unterminated quote in {command_line:?}");
        out
    }

    /// Every forwarded arg must survive the trip through `cmd /S /C` into the
    /// child's argv unchanged. Checked by round-trip so the property is what
    /// is asserted, not one particular spelling of the escape.
    #[test]
    fn windows_quote_round_trips_through_the_crt_parser() {
        for arg in [
            "plain",
            "with space",
            r"C:\Program Files\Side Effects",
            // Trailing separator: the case the old quoting broke outright,
            // since `"C:\out\"` leaves the closing quote as data and the
            // argument never terminates.
            r"C:\out\",
            r"C:\out\\",
            "say \"hi\"",
            // A backslash already sitting in front of a quote — the other
            // half of the old bug, where `\"` escaped to `\\"` and ended the
            // argument early.
            r#"C:\path\"x"#,
            r#""quoted""#,
            "1001",
            "--flag=value with space",
            "trailing\\",
            "",
        ] {
            let quoted = windows_quote(arg);
            assert_eq!(
                crt_parse_single(&quoted),
                arg,
                "arg {arg:?} quoted as {quoted:?}"
            );
        }
    }

    /// Regression: `C:\out\` used to quote to `"C:\out\"`, whose closing
    /// quote the CRT reads as literal data — so the argument ran on and
    /// swallowed the rest of the command line.
    #[test]
    fn windows_quote_doubles_a_trailing_backslash_run() {
        assert_eq!(windows_quote(r"C:\out\"), r#""C:\out\\""#);
        assert_eq!(windows_quote(r"C:\out\\"), r#""C:\out\\\\""#);
        // A backslash not preceding a quote stays a single backslash.
        assert_eq!(windows_quote(r"C:\out\bin"), r#""C:\out\bin""#);
    }
}
