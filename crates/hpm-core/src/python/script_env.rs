//! Per-script Python environments.
//!
//! Backs the table form of `[scripts]` entries:
//!
//! ```toml
//! [scripts.tt_setup]
//! cmd = "python scripts/tt_setup.py"
//! python = "3.11"
//! requirements = ["PySide6>=6.6"]
//! ```
//!
//! Resolves the inline `requirements` through `uv pip compile`, then defers
//! to the existing content-addressable [`VenvManager`] so two scripts that
//! ask for the same `python` + `requirements` reuse one venv. The bin
//! directory of the resulting venv is prepended to PATH by the caller so
//! `python` in the script's command resolves to the pinned interpreter.
//!
//! Default Python version is 3.11 when the script omits one — that matches
//! Houdini 21.x's bundled interpreter, which is the most common case for
//! the out-of-process hooks (`tt_setup`, etc.) this feature exists to serve.

use super::PythonHome;
use super::error::PythonError;
use super::types::{PythonVersion, ResolvedDependencySet};
use super::venv::VenvManager;
use hpm_package::ScriptEntry;
use std::collections::HashMap;
use std::path::PathBuf;
use std::str::FromStr;
use tracing::info;

/// Ensure a venv exists with the given Python version and inline requirements,
/// and return its root path.
///
/// `python_version` accepts the same shapes as the manifest: `"3.11"`,
/// `"3.11.9"`, etc. `requirements` are raw PEP-508-ish requirement strings
/// (`"PySide6>=6.6"`, `"numpy"`, ...).
///
/// Two calls with the same resolved set hit the same venv on disk — the hash
/// is computed over the resolved exact versions, so different requirement
/// strings that resolve to the same closure share storage.
pub async fn ensure_script_venv(
    home: &PythonHome,
    python_version: Option<&str>,
    requirements: &[String],
) -> Result<PathBuf, PythonError> {
    let py_str = python_version.unwrap_or(super::DEFAULT_PYTHON_VERSION);
    // The FromStr error already names the offending input, so the prior
    // "in script entry" context adds nothing typed callers can't see.
    let parsed = PythonVersion::from_str(py_str)?;

    let resolved = if requirements.is_empty() {
        info!("Preparing script venv (python {}, no requirements)", py_str);
        ResolvedDependencySet::new(parsed)
    } else {
        info!(
            "Resolving {} requirement(s) for script venv (python {})",
            requirements.len(),
            py_str
        );
        super::resolver::compile_requirements(home, requirements, parsed)
            .await
            .map_err(|e| e.uv_context("Failed to resolve script requirements"))?
    };

    VenvManager::new(home)
        .ensure_virtual_environment(&resolved)
        .await
}

/// Env-var mutations a caller should apply to a script subprocess before
/// spawning. Produced by [`prepare_script_env`].
///
/// Designed to be spawn-strategy agnostic: `hpm run` shells out via
/// `cmd /C` / `sh -c`, while an embedder may direct-spawn via
/// `CreateProcessW` / `execvp`, and both consume this handle the same
/// way. Embedders pass it through their own env-var pipeline rather than
/// surrendering the spawn to this crate.
///
/// All fields are optional / empty when the script's [`ScriptEntry`] doesn't
/// declare `python` or `requirements` (the plain string form).
#[derive(Debug, Clone, Default)]
pub struct ScriptEnvHandle {
    /// Directory to prepend to `PATH` so `python` resolves to the pinned
    /// venv interpreter. `None` when no venv was prepared.
    pub path_prepend: Option<PathBuf>,
    /// Additional env vars to set on the spawn (currently `VIRTUAL_ENV` when
    /// a venv was prepared; future additions slot in here).
    pub env: HashMap<String, String>,
}

impl ScriptEnvHandle {
    /// Fold this handle's mutations into `env_vars`. `env` entries overwrite
    /// any existing keys; `path_prepend` is prepended to whatever `PATH` the
    /// caller has already staged (or the parent process env if `PATH` isn't
    /// in the map yet), joined by the platform's path separator.
    ///
    /// Callers then hand `env_vars` to their spawn primitive — `tokio::
    /// process::Command::envs`, or an embedder's own capturing / detached
    /// shell helpers.
    ///
    /// On Windows the `PATH` env var is case-insensitive, but `HashMap` keys
    /// are not — always stage your own `PATH` under the uppercase key so the
    /// prepend lands on the same entry and the child doesn't see two
    /// conflicting bindings.
    pub fn apply_to(&self, env_vars: &mut HashMap<String, String>) {
        for (k, v) in &self.env {
            env_vars.insert(k.clone(), v.clone());
        }
        if let Some(prefix) = &self.path_prepend {
            prepend_env_paths(env_vars, "PATH", std::slice::from_ref(prefix));
        }
    }
}

/// Prepend `prefixes` to the path-list env var `key` (in `env_vars`, falling
/// back to the process env), joined by the platform separator. No-op when
/// `prefixes` is empty; an empty existing value adds no separator.
pub(crate) fn prepend_env_paths(
    env_vars: &mut HashMap<String, String>,
    key: &str,
    prefixes: &[PathBuf],
) {
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

/// Prepare the per-script environment for `entry`. When the entry's table
/// form declares `python` or `requirements`, this lazily ensures a
/// uv-managed venv (creating it on first call, reusing it on every
/// subsequent call with the same resolved closure) and returns the env
/// mutations the caller must apply before spawning. Plain string entries
/// — and table-form entries with neither field — return an empty handle.
///
/// This is the canonical "what env does this script need?" function shared
/// by every HPM embedder. `hpm run` and out-of-process hook runners both
/// route through here, so a manifest change picked up by one is picked up
/// by the other without per-embedder drift.
///
/// # Errors
///
/// Surfaces `ensure_script_venv` failures (uv bootstrap, interpreter
/// download, dependency resolve). Callers typically wrap the error with
/// a `"preparing script venv for <name>"` context.
///
/// The venv lives under the hpm home of the user's configuration
/// (`[storage] home_dir`), loaded only when the entry needs a venv; a
/// caller that already holds the configuration uses
/// [`prepare_script_env_in`].
pub async fn prepare_script_env(entry: &ScriptEntry) -> Result<ScriptEnvHandle, PythonError> {
    if !entry.needs_venv() {
        return Ok(ScriptEnvHandle::default());
    }
    let config = hpm_config::Config::load().map_err(Box::new)?;
    prepare_script_env_in(&PythonHome::for_storage(&config.storage), entry).await
}

/// [`prepare_script_env`] with the venv under `home`.
pub async fn prepare_script_env_in(
    home: &PythonHome,
    entry: &ScriptEntry,
) -> Result<ScriptEnvHandle, PythonError> {
    if !entry.needs_venv() {
        return Ok(ScriptEnvHandle::default());
    }
    super::initialize(home).await?;
    let venv_path = ensure_script_venv(home, entry.python(), entry.requirements()).await?;
    let bin_dir = super::venv_layout::bin_dir(&venv_path);
    let mut env = HashMap::new();
    env.insert(
        "VIRTUAL_ENV".to_string(),
        venv_path.to_string_lossy().into_owned(),
    );
    Ok(ScriptEnvHandle {
        path_prepend: Some(bin_dir),
        env,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_to_sets_env_and_prepends_path_when_present() {
        let handle = ScriptEnvHandle {
            path_prepend: Some(PathBuf::from("/venv/bin")),
            env: {
                let mut m = HashMap::new();
                m.insert("VIRTUAL_ENV".to_string(), "/venv".to_string());
                m
            },
        };
        let mut env = HashMap::new();
        env.insert("PATH".to_string(), "/usr/bin".to_string());
        handle.apply_to(&mut env);

        assert_eq!(env.get("VIRTUAL_ENV").map(String::as_str), Some("/venv"));
        let path = env.get("PATH").cloned().unwrap_or_default();
        let separator = if cfg!(target_os = "windows") {
            ';'
        } else {
            ':'
        };
        assert!(
            path.starts_with("/venv/bin"),
            "expected /venv/bin prefix, got {path}"
        );
        assert!(
            path.contains(separator) && path.ends_with("/usr/bin"),
            "expected existing PATH to be preserved, got {path}"
        );
    }

    /// Prepending yields the prefixes in order, then the existing value,
    /// joined by the platform separator; an empty existing value adds no
    /// trailing separator and no prefixes leave the value alone.
    #[test]
    fn prepend_env_paths_puts_prefixes_before_the_existing_value() {
        let sep = if cfg!(target_os = "windows") {
            ";"
        } else {
            ":"
        };
        for (prefixes, existing, expected) in [
            (
                vec!["/venv/bin"],
                "/usr/bin",
                format!("/venv/bin{sep}/usr/bin"),
            ),
            (vec!["/venv/bin"], "", "/venv/bin".to_string()),
            (
                vec!["/pkg/python", "/dep/python"],
                "/existing",
                format!("/pkg/python{sep}/dep/python{sep}/existing"),
            ),
            (vec![], "/existing", "/existing".to_string()),
        ] {
            let mut env = HashMap::new();
            env.insert("KEY".to_string(), existing.to_string());
            let prefixes: Vec<PathBuf> = prefixes.into_iter().map(PathBuf::from).collect();
            prepend_env_paths(&mut env, "KEY", &prefixes);
            assert_eq!(env["KEY"], expected);
        }
    }

    #[test]
    fn apply_to_is_noop_for_default_handle() {
        let handle = ScriptEnvHandle::default();
        let mut env = HashMap::new();
        env.insert("PATH".to_string(), "/usr/bin".to_string());
        handle.apply_to(&mut env);
        assert_eq!(env.len(), 1);
        assert_eq!(env.get("PATH").map(String::as_str), Some("/usr/bin"));
    }

    #[tokio::test]
    async fn prepare_script_env_returns_empty_handle_for_plain_entry() {
        // Plain string entry → needs_venv() == false → no venv prep, no
        // bundled-uv bootstrap. The function must return the default handle
        // without calling initialize().
        let entry = ScriptEntry::Plain("python scripts/foo.py".to_string());
        let handle = prepare_script_env(&entry).await.unwrap();
        assert!(handle.path_prepend.is_none());
        assert!(handle.env.is_empty());
    }

    #[tokio::test]
    async fn prepare_script_env_returns_empty_handle_for_table_without_venv_fields() {
        use hpm_package::{ScriptCommand, ScriptEnv};
        // Table form with neither python nor requirements behaves like the
        // shorthand per ScriptEntry::needs_venv() — still empty handle.
        let entry = ScriptEntry::WithEnv(ScriptEnv {
            cmd: ScriptCommand::Flat("python scripts/foo.py".to_string()),
            python: None,
            requirements: vec![],
            label: None,
            description: None,
            package_env: false,
        });
        let handle = prepare_script_env(&entry).await.unwrap();
        assert!(handle.path_prepend.is_none());
        assert!(handle.env.is_empty());
    }
}
