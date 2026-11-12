//! The Python environment of a project: one uv venv resolved from the
//! `[python_dependencies]` of a set of manifests, shared by every caller that
//! needs one (a sync, for the Houdini package files; `hpm run`, for a
//! `package-env` script).

use crate::python::resolver::resolve_combined;
use crate::python::{PythonError, PythonHome, VenvManager, collect_python_dependencies};
use crate::storage::InstalledPackage;
use hpm_package::PackageManifest;
use std::path::PathBuf;
use tracing::info;

use super::ProjectError;

/// A venv built for a project.
pub(super) struct ProjectVenv {
    /// The venv root.
    pub(super) path: PathBuf,
    /// Its `site-packages` directory.
    pub(super) site_packages: PathBuf,
}

fn python_error(e: PythonError) -> ProjectError {
    ProjectError::PythonResolution(Box::new(e))
}

/// Resolve the Python dependencies of `manifests`, plus `extra_requirements`
/// (a script's own requirements), into one content-addressed venv, creating
/// it when it does not exist yet. `None` when there is nothing to install,
/// in which case uv is not even fetched.
///
/// `houdini` is the project's own Houdini version (its `[compat].houdini`
/// lower bound): Houdini ships a fixed embedded CPython (20.5→3.11, 21→3.11,
/// 22→3.13), so the project's version decides the interpreter ABI, not a
/// package's compatibility floor. `owners` are recorded as the venv's users,
/// the only handle cleanup has on what a venv belongs to (its hash is over
/// the *resolved* set, which cleanup cannot recompute).
pub(super) async fn ensure_venv(
    home: &PythonHome,
    houdini: Option<&str>,
    manifests: &[PackageManifest],
    owners: &[InstalledPackage],
    extra_requirements: &[String],
) -> Result<Option<ProjectVenv>, ProjectError> {
    let declares_python = manifests.iter().any(|m| !m.python_dependencies.is_empty())
        || extra_requirements.iter().any(|r| !r.trim().is_empty());
    if !declares_python {
        return Ok(None);
    }

    crate::python::initialize(home)
        .await
        .map_err(python_error)?;
    let collected = collect_python_dependencies(houdini, manifests).map_err(python_error)?;
    let resolved = resolve_combined(home, &collected, extra_requirements)
        .await
        .map_err(python_error)?;
    if resolved.packages.is_empty() {
        return Ok(None);
    }
    info!(
        "Resolved {} Python packages (hash: {})",
        resolved.packages.len(),
        resolved.hash()
    );

    let venv_manager = VenvManager::new(home);
    let used_by: Vec<String> = owners
        .iter()
        .filter(|p| !p.manifest.python_dependencies.is_empty())
        .map(InstalledPackage::venv_ref)
        .collect();
    let path = venv_manager
        .ensure_virtual_environment_for(&resolved, &used_by)
        .await
        .map_err(python_error)?;
    let site_packages = venv_manager.get_python_site_packages_path(&path, &resolved.python_version);
    info!("Python venv site-packages: {}", site_packages.display());
    Ok(Some(ProjectVenv {
        path,
        site_packages,
    }))
}
