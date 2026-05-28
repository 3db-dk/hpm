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

use super::bundled::{ensure_managed_python, run_uv_command};
use super::types::{PythonVersion, ResolvedDependencySet};
use super::venv::VenvManager;
use anyhow::{Context, Result};
use hpm_package::ScriptEntry;
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use tempfile::NamedTempFile;
use tracing::{debug, info};

/// Default Python version when a script entry omits `python`.
///
/// Picked to match Houdini 21.x's bundled interpreter — the typical context
/// for out-of-process hooks like `tt_setup` that this feature targets.
pub const DEFAULT_SCRIPT_PYTHON: &str = "3.11";

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
    python_version: Option<&str>,
    requirements: &[String],
) -> Result<PathBuf> {
    let py_str = python_version.unwrap_or(DEFAULT_SCRIPT_PYTHON);
    let parsed = PythonVersion::from_str(py_str)
        .with_context(|| format!("Invalid python version '{}' in script entry", py_str))?;

    let resolved = if requirements.is_empty() {
        info!("Preparing script venv (python {}, no requirements)", py_str);
        ResolvedDependencySet::new(parsed)
    } else {
        info!(
            "Resolving {} requirement(s) for script venv (python {})",
            requirements.len(),
            py_str
        );
        resolve_raw_requirements(py_str, requirements, parsed).await?
    };

    let manager = VenvManager::new()?;
    manager.ensure_virtual_environment(&resolved).await
}

/// Path to the directory inside a venv that holds executables (`bin` on Unix,
/// `Scripts` on Windows). Callers prepend this to `PATH` before spawning the
/// script so `python` resolves to the venv interpreter.
pub fn venv_bin_dir(venv_path: &Path) -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        venv_path.join("Scripts")
    }
    #[cfg(not(target_os = "windows"))]
    {
        venv_path.join("bin")
    }
}

/// Env-var mutations a caller should apply to a script subprocess before
/// spawning. Produced by [`prepare_script_env`].
///
/// Designed to be spawn-strategy agnostic: `hpm run` shells out via
/// `cmd /C` / `sh -c`, the tumbletrove-desktop hook runner direct-spawns
/// via `CreateProcessW` / `execvp`, and both consume this handle the same
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
    /// process::Command::envs`, the desktop's `run_capturing_shell` /
    /// `spawn_detached_shell`, etc.
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
            const PATH_KEY: &str = "PATH";
            let existing = env_vars
                .get(PATH_KEY)
                .cloned()
                .or_else(|| std::env::var(PATH_KEY).ok())
                .unwrap_or_default();
            env_vars.insert(PATH_KEY.to_string(), compose_path(prefix, &existing));
        }
    }
}

/// Prepend `prefix` to `existing` using the platform path separator.
/// Pure helper extracted so unit tests can exercise the composition
/// without mutating the process environment.
fn compose_path(prefix: &Path, existing: &str) -> String {
    let separator = if cfg!(target_os = "windows") {
        ";"
    } else {
        ":"
    };
    let prefix_str = prefix.to_string_lossy();
    if existing.is_empty() {
        prefix_str.into_owned()
    } else {
        format!("{}{}{}", prefix_str, separator, existing)
    }
}

/// Prepare the per-script environment for `entry`. When the entry's table
/// form declares `python` or `requirements`, this lazily ensures a
/// uv-managed venv (creating it on first call, reusing it on every
/// subsequent call with the same resolved closure) and returns the env
/// mutations the caller must apply before spawning. Plain string entries
/// — and table-form entries with neither field — return an empty handle.
///
/// This is the canonical "what env does this script need?" function shared
/// by every HPM embedder. `hpm run` and the tumbletrove-desktop's tt_*
/// hook runner both route through here, so a manifest change picked up by
/// one is picked up by the other without per-embedder drift.
///
/// # Errors
///
/// Surfaces `ensure_script_venv` failures (uv bootstrap, interpreter
/// download, dependency resolve). Callers typically wrap the error with
/// a `"preparing script venv for <name>"` context.
pub async fn prepare_script_env(entry: &ScriptEntry) -> Result<ScriptEnvHandle> {
    if !entry.needs_venv() {
        return Ok(ScriptEnvHandle::default());
    }
    super::initialize()
        .await
        .context("Failed to initialize bundled uv")?;
    let venv_path = ensure_script_venv(entry.python(), entry.requirements())
        .await
        .context("Failed to prepare script venv")?;
    let bin_dir = venv_bin_dir(&venv_path);
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

/// Resolve raw requirement strings to exact pinned versions via `uv pip compile`.
///
/// Mirrors [`super::resolver::resolve_dependencies`] but skips the
/// `PythonDependencies` shape, which has no syntactic place for arbitrary
/// requirement-string forms (extras, environment markers, ranges).
async fn resolve_raw_requirements(
    python_version_str: &str,
    requirements: &[String],
    python_version: PythonVersion,
) -> Result<ResolvedDependencySet> {
    let req_file = write_requirements_file(requirements)?;

    // Match the resolver path: install the managed CPython up front so a
    // clean machine doesn't trip "No interpreter found" on first run.
    ensure_managed_python(python_version_str).await?;

    let output = run_uv_command(&[
        "pip",
        "compile",
        req_file.path().to_str().unwrap(),
        "--python-version",
        python_version_str,
    ])
    .await
    .context("Failed to resolve script requirements")?;

    let resolved = ResolvedDependencySet::from_pip_compile_output(&output.stdout, python_version);

    debug!(
        "Resolved {} packages for script venv",
        resolved.packages.len()
    );
    Ok(resolved)
}

fn write_requirements_file(requirements: &[String]) -> Result<NamedTempFile> {
    let mut tmp = NamedTempFile::new().context("Failed to create temp requirements file")?;
    for req in requirements {
        let req = req.trim();
        if req.is_empty() {
            continue;
        }
        writeln!(tmp, "{}", req).context("Failed to write requirements file")?;
    }
    tmp.flush().context("Failed to flush requirements file")?;
    Ok(tmp)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_requirements_skips_empty_lines() {
        let reqs = vec![
            "  ".to_string(),
            "PySide6>=6.6".to_string(),
            "".to_string(),
            "numpy".to_string(),
        ];
        let tmp = write_requirements_file(&reqs).unwrap();
        let content = std::fs::read_to_string(tmp.path()).unwrap();
        assert_eq!(content, "PySide6>=6.6\nnumpy\n");
    }

    #[test]
    fn venv_bin_dir_layout_matches_uv() {
        let p = Path::new("/tmp/venv");
        let bin = venv_bin_dir(p);
        #[cfg(target_os = "windows")]
        assert!(bin.ends_with("Scripts"));
        #[cfg(not(target_os = "windows"))]
        assert!(bin.ends_with("bin"));
    }

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

    #[test]
    fn compose_path_prepends_with_platform_separator() {
        let composed = compose_path(&PathBuf::from("/venv/bin"), "/usr/bin");
        let separator = if cfg!(target_os = "windows") {
            ';'
        } else {
            ':'
        };
        assert!(composed.starts_with("/venv/bin"));
        assert!(composed.contains(separator));
        assert!(composed.ends_with("/usr/bin"));
    }

    #[test]
    fn compose_path_omits_separator_when_existing_is_empty() {
        let composed = compose_path(&PathBuf::from("/venv/bin"), "");
        assert_eq!(composed, "/venv/bin");
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
        use hpm_package::{EnvValue, ScriptEnv};
        // Table form with neither python nor requirements behaves like the
        // shorthand per ScriptEntry::needs_venv() — still empty handle.
        let entry = ScriptEntry::WithEnv(ScriptEnv {
            cmd: EnvValue::Flat("python scripts/foo.py".to_string()),
            python: None,
            requirements: vec![],
        });
        let handle = prepare_script_env(&entry).await.unwrap();
        assert!(handle.path_prepend.is_none());
        assert!(handle.env.is_empty());
    }
}
