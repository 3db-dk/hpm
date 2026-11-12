//! The runtime environment of a `package-env` script (`hpm run`).

use crate::lock::LockFile;
use crate::storage::InstalledPackage;
use hpm_package::PackageManifest;
use std::path::PathBuf;

use super::{ProjectError, ProjectManager};

/// The resolved runtime environment for a `package-env` script — the merged
/// venv plus every involved package's `python/` directory.
///
/// Produced by [`ProjectManager::resolve_package_env`] and applied by callers
/// (`hpm run`) to a subprocess's environment: `venv_bin` prepended to `PATH`,
/// `virtual_env` exported as `VIRTUAL_ENV`, and `python_paths` prepended to
/// `PYTHONPATH`. All fields may be empty when the package declares no Python
/// dependencies — `python/` directories alone still populate `python_paths`.
#[derive(Debug, Clone, Default)]
pub struct PackageRunEnv {
    /// The venv `bin`/`Scripts` directory to prepend to `PATH`, so `python`
    /// resolves to the resolved interpreter. `None` when no venv was built.
    pub venv_bin: Option<PathBuf>,
    /// The venv root, to export as `VIRTUAL_ENV`. `None` when no venv was built.
    pub virtual_env: Option<PathBuf>,
    /// Directories to prepend to `PYTHONPATH`, in priority order: the running
    /// package's `python/` first, then each dependency's `python/`, then the
    /// venv `site-packages` last.
    pub python_paths: Vec<PathBuf>,
}

impl ProjectManager {
    /// Resolve the combined runtime environment for a `package-env` script.
    ///
    /// Builds the same environment `install` materialises for Houdini, but for
    /// an out-of-Houdini process: the merged uv venv resolved from
    /// `[python_dependencies]` across the project and its installed hpm
    /// dependencies (plus `extra_requirements` — the script's own
    /// `requirements`), and every involved package's `python/` directory on
    /// `PYTHONPATH`. The project's own Houdini version drives the interpreter
    /// ABI, exactly as in a sync (both use `python_env::ensure_venv`).
    ///
    /// Read-only: the dependency set is taken from the existing `hpm.lock` +
    /// global package store, so this never fetches packages or rewrites
    /// generated Houdini manifests. The venv itself is content-addressable, so
    /// when `install` already built it this returns the same path without
    /// re-resolving wheels.
    ///
    /// # Errors
    ///
    /// [`ProjectError::PackageEnvNoManifest`], [`ProjectError::PackageEnvNoLock`] or
    /// [`ProjectError::PackageEnvNotInstalled`] when the project declares hpm
    /// dependencies but `hpm.lock` is missing or a locked package isn't in the
    /// store — i.e. `hpm install` hasn't been run. Python resolution / venv
    /// failures surface as [`ProjectError::PythonResolution`].
    pub async fn resolve_package_env(
        &self,
        extra_requirements: &[String],
    ) -> Result<PackageRunEnv, ProjectError> {
        let project_manifest =
            self.load_project_manifest()?
                .ok_or_else(|| ProjectError::PackageEnvNoManifest {
                    manifest: self.project_paths.manifest_file.clone(),
                })?;
        // `<root>/hpm.toml` by construction (`Config::project_paths`).
        let project_root = self
            .project_paths
            .manifest_file
            .parent()
            .expect("the project manifest path is <root>/hpm.toml")
            .to_path_buf();

        // Locked, already-installed dependency packages (read-only).
        let dep_packages = self.installed_dependency_closure(&project_manifest)?;

        // PYTHONPATH: the running package's python/ wins over its deps'.
        let mut python_paths = Vec::new();
        let project_python = project_root.join("python");
        if project_python.is_dir() {
            python_paths.push(project_python);
        }
        for pkg in &dep_packages {
            let dep_python = pkg.install_path.join("python");
            if dep_python.is_dir() {
                python_paths.push(dep_python);
            }
        }

        // Collect + resolve Python deps across the project itself and its deps,
        // layering the script's own requirements on top, in one pass.
        let mut manifests: Vec<PackageManifest> = Vec::with_capacity(dep_packages.len() + 1);
        manifests.push(project_manifest.clone());
        manifests.extend(dep_packages.iter().map(|p| p.manifest.clone()));

        let venv = super::python_env::ensure_venv(
            project_manifest.compat.houdini_min().as_deref(),
            &manifests,
            &dep_packages,
            extra_requirements,
        )
        .await?;
        let mut run_env = PackageRunEnv {
            python_paths,
            ..Default::default()
        };
        if let Some(venv) = venv {
            run_env.python_paths.push(venv.site_packages);
            run_env.venv_bin = Some(crate::python::venv_bin_dir(&venv.path));
            run_env.virtual_env = Some(venv.path);
        }

        Ok(run_env)
    }

    /// The project's locked, already-installed hpm dependencies.
    ///
    /// Resolves each `[dependencies]` entry through `hpm.lock` (for the exact
    /// version) and the global package store (for the on-disk install path).
    /// Returns an empty list when the project has no dependencies. Errors with
    /// [`ProjectError::PackageEnvNoLock`] or
    /// [`ProjectError::PackageEnvNotInstalled`] when the lockfile is missing or a
    /// locked package isn't installed.
    fn installed_dependency_closure(
        &self,
        project_manifest: &PackageManifest,
    ) -> Result<Vec<InstalledPackage>, ProjectError> {
        if project_manifest.dependencies.is_empty() {
            return Ok(Vec::new());
        }

        let lock_path = &self.project_paths.lock_file;
        let lock = LockFile::load(lock_path).map_err(|e| ProjectError::PackageEnvNoLock {
            lock: lock_path.clone(),
            source: Box::new(e),
        })?;

        let installed = self.storage_manager.list_installed()?;
        let mut packages = Vec::with_capacity(lock.dependencies.len());
        for (name, locked) in &lock.dependencies {
            let found = installed
                .iter()
                .find(|p| Self::matches_spec_name(p, name) && p.version == locked.version)
                .cloned();
            match found {
                Some(pkg) => packages.push(pkg),
                None => {
                    return Err(ProjectError::PackageEnvNotInstalled {
                        name: name.clone(),
                        version: locked.version.clone(),
                    });
                }
            }
        }

        Ok(packages)
    }
}
