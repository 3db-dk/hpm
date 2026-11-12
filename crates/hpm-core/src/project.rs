//! Project-level orchestration for one `hpm.toml` project: install and
//! sync, the lockfile, the Python environment, Houdini package-file
//! emission, and which installed copy each dependency uses.
//!
//! [`ProjectManager`] is the entry point; its methods live by concern:
//!
//! - `install` — [`ProjectManager::sync_dependencies`] and one dependency's
//!   install
//! - `locking` — [`ProjectManager::lock_file`]
//! - `python_env` — the shared venv a sync or a `package-env` script uses
//! - `run_env` — [`ProjectManager::resolve_package_env`] and [`PackageRunEnv`]
//! - `listing` — [`ProjectManager::list_dependencies`]
//! - `houdini_emit` — Houdini `packages/` manifest emission
//!   ([`PROJECT_OVERRIDES_FILE`] and the `<creator>.<slug>.json` generators)
//! - [`error`] — [`ProjectError`]; [`types`] — [`ProjectDependency`],
//!   [`InstallOutcome`]

use crate::archive_fetcher::ArchiveFetcher;
use crate::storage::{InstalledPackage, StorageManager};
use hpm_config::{Config, ProjectPaths};
use hpm_package::{IoOp, ManifestLoadError, PackageManifest};
use std::path::PathBuf;
use std::sync::Arc;
use tracing::info;

pub mod error;
pub(crate) mod houdini_emit;
mod install;
mod listing;
mod locking;
pub mod manifest_edit;
mod python_env;
mod run_env;
pub mod types;

pub use error::ProjectError;
pub use houdini_emit::PROJECT_OVERRIDES_FILE;
pub use run_env::PackageRunEnv;
pub use types::{InstallOutcome, ProjectDependency};

pub(crate) use install::fetch_and_install_pkg;
#[cfg(test)]
use install::install_one_dep;

#[derive(Debug)]
pub struct ProjectManager {
    config: Arc<Config>,
    project_paths: ProjectPaths,
    storage_manager: Arc<StorageManager>,
    fetcher: ArchiveFetcher,
    auth_token: Option<String>,
}

impl ProjectManager {
    /// Construct a `ProjectManager` for `project_root`, sharing the supplied
    /// `Config` and `StorageManager`.
    ///
    /// Callers load `Config` once at their top level and thread it down —
    /// embedding callers used to trigger 3+ `Config::load` disk reads per user
    /// operation, all of which now collapse into the shared `Arc<Config>` here
    /// and on `StorageManager`.
    ///
    /// All internally-built `RegistrySet`s are anonymous. For caller-driven
    /// auth (e.g. an embedder passing a bearer token for visibility-gated
    /// registries), use [`Self::new_with_auth`].
    pub fn new(
        project_root: PathBuf,
        storage_manager: Arc<StorageManager>,
        config: Arc<Config>,
    ) -> Result<Self, ProjectError> {
        Self::new_with_auth(project_root, storage_manager, config, None)
    }

    /// Like [`Self::new`], but stashes a bearer token that is forwarded to
    /// every `RegistrySet` the manager builds internally.
    ///
    /// `sync_dependencies` constructs its own `RegistrySet` from the supplied
    /// [`Config`] (or the project manifest's `[[registries]]` override). With
    /// `auth_token = Some(...)`, those internal sets are built via
    /// [`crate::registry::RegistrySet::from_configs_with_auth`] so the token
    /// reaches the API-registry HTTP client. `None` is identical to
    /// [`Self::new`].
    ///
    /// Token semantics mirror the registry variant: the token is baked into
    /// the manager and propagates to each `RegistrySet` at the point of
    /// construction. Callers tracking a refreshing token should rebuild the
    /// `ProjectManager` per operation rather than mutating one in place.
    pub fn new_with_auth(
        project_root: PathBuf,
        storage_manager: Arc<StorageManager>,
        config: Arc<Config>,
        auth_token: Option<String>,
    ) -> Result<Self, ProjectError> {
        let project_paths = hpm_config::Config::project_paths(&project_root);

        let fetcher = ArchiveFetcher::for_storage(&config.storage)?;

        let manager = Self {
            config,
            project_paths,
            storage_manager,
            fetcher,
            auth_token,
        };

        manager.ensure_directories()?;
        Ok(manager)
    }

    fn ensure_directories(&self) -> Result<(), ProjectError> {
        // ProjectPaths::ensure_directories does mkdir -p on the well-known
        // dirs; bubble io::Error along with the project root so the failure
        // names a path the user can reason about.
        self.project_paths.ensure_directories().map_err(|source| {
            IoOp::wrap(
                "create project packages directory",
                &self.project_paths.packages_dir,
                source,
            )
        })?;
        info!("Ensured project directories exist");
        Ok(())
    }

    pub fn load_project_manifest(&self) -> Result<Option<PackageManifest>, ProjectError> {
        match PackageManifest::from_path(&self.project_paths.manifest_file) {
            Ok(manifest) => Ok(Some(manifest)),
            Err(ManifestLoadError::NotFound { .. }) => Ok(None),
            Err(e) => Err(ProjectError::Manifest(e)),
        }
    }

    /// Match an installed package against a spec name, handling both scoped
    /// (`creator/slug`) and bare (`slug`) forms. The canonical identifier
    /// is `manifest.package.path`; the slug is the kebab segment after `/`.
    pub(super) fn matches_spec_name(pkg: &InstalledPackage, spec_name: &str) -> bool {
        let path = &pkg.manifest.package.path;
        path.as_str() == spec_name || path.slug() == spec_name
    }
}

#[cfg(test)]
#[path = "project_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "houdini_env_model.rs"]
mod houdini_env_model;

#[cfg(test)]
#[path = "houdini_emission_model_tests.rs"]
mod houdini_emission_model_tests;

#[cfg(test)]
#[path = "houdini_conformance_tests.rs"]
mod houdini_conformance_tests;
