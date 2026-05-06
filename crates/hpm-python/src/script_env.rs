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

use crate::bundled::{ensure_managed_python, run_uv_command};
use crate::types::{PythonVersion, ResolvedDependencySet};
use crate::venv::VenvManager;
use anyhow::{Context, Result};
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

    let manager = VenvManager::new();
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

/// Resolve raw requirement strings to exact pinned versions via `uv pip compile`.
///
/// Mirrors [`crate::resolver::resolve_dependencies`] but skips the
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

    let mut resolved = ResolvedDependencySet::new(python_version);
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((name, version)) = line.split_once("==") {
            let clean_name = name.split('[').next().unwrap_or(name);
            resolved.add_package(clean_name, version);
        }
    }

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
}
