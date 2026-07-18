//! Project-level orchestration: install, sync, manifest editing, and Houdini
//! package.json emission for one `hpm.toml` project.
//!
//! [`ProjectManager`] is the entry point. Supporting types live next door:
//!
//! - [`error`] — [`ProjectError`]
//! - [`types`] — [`ProjectDependency`], [`InstallOutcome`]
//! - `houdini_emit` — Houdini `packages/` manifest emission
//!   ([`PROJECT_OVERRIDES_FILE`] and the `<creator>.<slug>.json` generators)

use crate::archive_fetcher::ArchiveFetcher;
use crate::lock::{LockFile, LockedSource};
use crate::package_source::PackageSource;
use crate::python::resolver::resolve_combined;
use crate::python::{VenvManager, collect_python_dependencies, resolve_dependencies, venv_bin_dir};
use crate::storage::{InstalledPackage, PackageSpec, StorageManager};
use hpm_config::{Config, ProjectPaths};
use hpm_package::{IoOp, ManifestLoadError, PackageManifest};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::task::JoinSet;
use tracing::{debug, info};

pub mod error;
pub(crate) mod houdini_emit;
pub mod manifest_edit;
pub mod types;

pub use error::ProjectError;
pub use houdini_emit::PROJECT_OVERRIDES_FILE;
pub use types::{InstallOutcome, ProjectDependency};

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
    /// Callers load `Config` once at their top level and thread it down — the
    /// embedded callers (the desktop client) used to trigger 3+ `Config::load`
    /// disk reads per user operation, all of which now collapse into the
    /// shared `Arc<Config>` here and on `StorageManager`.
    ///
    /// All internally-built `RegistrySet`s are anonymous. For caller-driven
    /// auth (e.g. a desktop client passing a bearer token for visibility-gated
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
    /// `sync_dependencies` and `add_dependency`'s registry-resolved path both
    /// construct their own `RegistrySet` from the supplied [`Config`] (or the
    /// project manifest's `[[registries]]` override). With `auth_token =
    /// Some(...)`, those internal sets are built via
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

        // Fetcher staging lives next to the global CAS under ~/.hpm/.
        // Drive both directories off `storage.home_dir` directly — using
        // `packages_dir.parent()` was wrong when the user overrode
        // `packages_dir` to a path outside `home_dir` (the cache then
        // landed at the wrong place; the cwd fallback to "." was worse).
        let storage_root = &config.storage.home_dir;
        let cache_dir = storage_root.join("cache");
        let fetch_packages_dir = storage_root.join("fetch");
        let fetcher = ArchiveFetcher::new(cache_dir, fetch_packages_dir)?;

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

    pub async fn add_dependency(&self, spec: &PackageSpec) -> Result<(), ProjectError> {
        info!("Adding dependency: {} {}", spec.name, spec.version_req);

        let installed_packages = self.storage_manager.list_installed()?;

        let installed_package = if let Some(pkg) = installed_packages.iter().find(|p| {
            Self::matches_spec_name(p, &spec.name) && p.is_compatible_with(&spec.version_req)
        }) {
            info!(
                "Package {} already installed with compatible version {}",
                spec.name, pkg.version
            );
            pkg.clone()
        } else {
            self.resolve_and_install_from_registry(spec).await?
        };

        // Respect the project's [runtime] overrides like sync does, so a
        // freshly added dep doesn't emit un-reconciled entries until the
        // next full sync.
        let project_env_overrides = self
            .load_project_manifest()?
            .map(|m| m.runtime)
            .unwrap_or_default();
        self.generate_houdini_manifest_with_python(
            &installed_package,
            None,
            &project_env_overrides,
        )?;
        self.write_project_overrides_manifest(&project_env_overrides)?;
        self.update_project_manifest(spec)?;

        info!("Successfully added dependency: {}", spec.name);
        Ok(())
    }

    /// Match an installed package against a spec name, handling both scoped
    /// (`creator/slug`) and bare (`slug`) forms. The canonical identifier
    /// is `manifest.package.path`; the slug is the kebab segment after `/`.
    fn matches_spec_name(pkg: &InstalledPackage, spec_name: &str) -> bool {
        let path = &pkg.manifest.package.path;
        path.as_str() == spec_name || path.slug() == spec_name
    }

    /// Resolve a package spec against configured registries and install it.
    async fn resolve_and_install_from_registry(
        &self,
        spec: &PackageSpec,
    ) -> Result<InstalledPackage, ProjectError> {
        let registry_set = crate::registry::RegistrySet::from_configs_with_auth(
            &self.config.registries,
            &self.config.storage.registry_cache_dir,
            self.auth_token.as_deref(),
        )
        .map_err(|e| ProjectError::RegistryConfiguration(Box::new(e)))?;

        if registry_set.is_empty() {
            return Err(ProjectError::NoRegistriesConfigured {
                name: spec.name.clone(),
                version_req: spec.version_req.as_str().to_string(),
            });
        }

        let entry = self.resolve_registry_entry(&registry_set, spec).await?;
        let version = entry.version.clone();
        let source = PackageSource::url(entry.dl.clone(), entry.version.clone())?
            .with_registry_checksum(entry.cksum.as_deref())?;
        self.fetch_and_install_pkg(&spec.name, &version, source)
            .await
    }

    /// Resolve a `PackageSpec` to a concrete `RegistryEntry` via
    /// [`crate::registry::RegistrySet::resolve`].
    async fn resolve_registry_entry(
        &self,
        registry_set: &crate::registry::RegistrySet,
        spec: &PackageSpec,
    ) -> Result<crate::registry::RegistryEntry, ProjectError> {
        let req_str = spec.version_req.as_str();
        registry_set
            .resolve(&spec.name, req_str)
            .await
            .map_err(|source| match source {
                crate::registry::RegistryError::VersionNotFound { .. } => {
                    ProjectError::NoMatchingVersion {
                        name: spec.name.clone(),
                        version_req: req_str.to_string(),
                    }
                }
                other => ProjectError::RegistryResolution {
                    name: spec.name.clone(),
                    version_req: req_str.to_string(),
                    source: Box::new(other),
                },
            })
    }

    pub async fn remove_dependency(&self, name: &str) -> Result<(), ProjectError> {
        info!("Removing dependency: {}", name);

        // 1. Remove from project manifest (hpm.toml)
        self.remove_from_project_manifest(name)?;

        // 2. Remove Houdini package manifest from project. `name` is the
        //    dependency key, i.e. a scoped `creator/slug`; a key that isn't a
        //    well-formed package path never had a manifest emitted for it.
        if let Ok(path) = hpm_package::PackagePath::new(name) {
            let manifest_path = self.project_paths.package_manifest_path(&path);
            if manifest_path.exists() {
                std::fs::remove_file(&manifest_path)
                    .map_err(|e| IoOp::wrap("remove Houdini manifest at", &manifest_path, e))?;
                debug!("Removed Houdini manifest: {:?}", manifest_path);
            }
        }

        info!("Successfully removed dependency: {}", name);
        Ok(())
    }

    pub async fn sync_dependencies(&self) -> Result<Vec<(String, InstallOutcome)>, ProjectError> {
        info!("Syncing project dependencies");

        let project_manifest = match self.load_project_manifest()? {
            Some(manifest) => manifest,
            None => {
                info!("No project manifest found, nothing to sync");
                return Ok(Vec::new());
            }
        };

        let project_env_overrides = project_manifest.runtime;
        let manifest_registries = project_manifest.registries;
        let dependencies = project_manifest.dependencies;

        // Build registry set once for any registry-resolved deps. A manifest
        // [[registries]] override beats the user's [registries] from config.
        // Wrapped in Arc so each spawned task can hold a cheap clone.
        let registry_set: Option<Arc<crate::registry::RegistrySet>> =
            if dependencies.values().any(|spec| spec.is_registry()) {
                let registry_configs: Vec<hpm_config::RegistrySourceConfig> =
                    if !manifest_registries.is_empty() {
                        manifest_registries
                            .iter()
                            .map(|r| hpm_config::RegistrySourceConfig {
                                name: r.name.clone(),
                                url: r.url.clone(),
                                registry_type: r.registry_type.clone(),
                            })
                            .collect()
                    } else {
                        self.config.registries.clone()
                    };
                let set = crate::registry::RegistrySet::from_configs_with_auth(
                    &registry_configs,
                    &self.config.storage.registry_cache_dir,
                    self.auth_token.as_deref(),
                )
                .map_err(|e| ProjectError::RegistryConfiguration(Box::new(e)))?;
                Some(Arc::new(set))
            } else {
                None
            };

        // Fetch list of globally installed packages once for short-circuit lookups
        let all_installed = Arc::new(self.storage_manager.list_installed()?);

        // Phase 1: spawn all installs in parallel via a JoinSet. Each task owns
        // a clone of (StorageManager, ArchiveFetcher, RegistrySet) and its dep
        // spec, so resolve→fetch→copy-into-CAS chains overlap across deps.
        let mut tasks: JoinSet<(String, Result<InstallOutcome, ProjectError>)> = JoinSet::new();
        for (name, spec) in dependencies {
            let storage = self.storage_manager.clone();
            let fetcher = self.fetcher.clone();
            let registry_set = registry_set.clone();
            let all_installed = all_installed.clone();

            tasks.spawn(async move {
                let result = install_one_dep(
                    &storage,
                    &fetcher,
                    registry_set.as_deref(),
                    &all_installed,
                    &name,
                    &spec,
                )
                .await;
                (name, result)
            });
        }

        let mut outcomes: Vec<(String, InstallOutcome)> = Vec::new();
        while let Some(joined) = tasks.join_next().await {
            // A spawned-task panic leaks structural confusion; let the runtime
            // surface it rather than synthesising a typed ProjectError.
            let (name, result) = joined.expect("dependency install task panicked");
            outcomes.push((name, result?));
        }

        // Snapshot the installed-package list once for downstream steps; the
        // outcomes hang onto richer metadata for the lockfile, but Python
        // resolution / manifest emission only need the InstalledPackage.
        let installed: Vec<InstalledPackage> =
            outcomes.iter().map(|(_, o)| o.package.clone()).collect();

        // Resolve Python pip dependencies and get venv site-packages path (if any)
        let venv_site_packages = self.resolve_python_deps(&installed).await?;

        // Generate Houdini JSON manifests for all packages, plus the
        // project overrides manifest that Houdini processes after them.
        for pkg in &installed {
            self.generate_houdini_manifest_with_python(
                pkg,
                venv_site_packages.as_deref(),
                &project_env_overrides,
            )?;
        }
        self.write_project_overrides_manifest(&project_env_overrides)?;

        // Sweep stale per-package manifests left over from previous syncs.
        // Houdini reads every .json file in `packages_dir` on launch, so a
        // manifest whose slug has dropped out of the dependency set (dev override
        // removed, registry yank, manual edit) keeps loading the package even
        // though hpm.toml no longer asks for it.
        self.sweep_stale_houdini_manifests(&installed)?;

        info!("Successfully synced project dependencies");
        Ok(outcomes)
    }

    /// Fetch a remote package and install it to global storage, returning the InstalledPackage.
    /// Used by single-package paths like `add_dependency` that don't need the checksum.
    async fn fetch_and_install_pkg(
        &self,
        name: &str,
        version: &str,
        source: PackageSource,
    ) -> Result<InstalledPackage, ProjectError> {
        let (package, _checksum) =
            fetch_and_install_pkg(&self.storage_manager, &self.fetcher, name, version, source)
                .await?;
        Ok(package)
    }

    /// Collect and resolve Python pip dependencies from installed packages.
    /// Returns the venv site-packages path if any Python deps exist, None otherwise.
    async fn resolve_python_deps(
        &self,
        installed_packages: &[InstalledPackage],
    ) -> Result<Option<PathBuf>, ProjectError> {
        let has_python_deps = installed_packages
            .iter()
            .any(|p| !p.manifest.python_dependencies.is_empty());
        if !has_python_deps {
            return Ok(None);
        }

        info!("Resolving Python pip dependencies");

        // Initialize UV binary (downloads on first use)
        crate::python::initialize()
            .await
            .map_err(|e| ProjectError::PythonResolution(e.into()))?;

        // Collect python dependencies from all package manifests. The project
        // manifest's own Houdini version is the source of truth for which
        // CPython we target — Houdini ships a fixed embedded interpreter
        // (20.5→3.11, 21→3.11, 22→3.13), and per-package `[compat].houdini`
        // declarations only describe compatibility floors, not the runtime
        // ABI. Without this override the venv could end up pinned to a
        // package's older Python and crash on import inside the launched
        // Houdini.
        let manifests: Vec<PackageManifest> = installed_packages
            .iter()
            .map(|p| p.manifest.clone())
            .collect();
        let project_houdini_version = self
            .load_project_manifest()?
            .and_then(|m| m.compat.houdini_min());
        let collected = collect_python_dependencies(project_houdini_version.as_deref(), &manifests)
            .await
            .map_err(|e| ProjectError::PythonResolution(e.into()))?;

        if collected.dependencies.is_empty() {
            return Ok(None);
        }

        info!(
            "Collected {} Python dependencies, resolving...",
            collected.dependencies.len()
        );

        // Resolve to exact versions via UV pip compile
        let resolved = resolve_dependencies(&collected)
            .await
            .map_err(|e| ProjectError::PythonResolution(e.into()))?;

        info!(
            "Resolved {} Python packages (hash: {})",
            resolved.packages.len(),
            resolved.hash()
        );

        // Ensure venv exists (content-addressable, shared across identical dep
        // sets). Record the packages that contributed Python dependencies —
        // the venv hash is over the *resolved* set, which cleanup cannot
        // recompute, so this is its only handle on what the venv belongs to.
        let venv_manager =
            VenvManager::new().map_err(|e| ProjectError::PythonResolution(e.into()))?;
        let used_by: Vec<String> = installed_packages
            .iter()
            .filter(|p| !p.manifest.python_dependencies.is_empty())
            .map(InstalledPackage::venv_ref)
            .collect();
        let venv_path = venv_manager
            .ensure_virtual_environment_for(&resolved, &used_by)
            .await
            .map_err(|e| ProjectError::PythonResolution(e.into()))?;

        let site_packages =
            venv_manager.get_python_site_packages_path(&venv_path, &resolved.python_version);
        info!("Python venv site-packages: {}", site_packages.display());
        Ok(Some(site_packages))
    }

    /// Resolve the combined runtime environment for a `package-env` script.
    ///
    /// Builds the same environment `install` materialises for Houdini, but for
    /// an out-of-Houdini process: the merged uv venv resolved from
    /// `[python_dependencies]` across the project and its installed hpm
    /// dependencies (plus `extra_requirements` — the script's own
    /// `requirements`), and every involved package's `python/` directory on
    /// `PYTHONPATH`. The project's own Houdini version drives the interpreter
    /// ABI, exactly as in [`Self::resolve_python_deps`].
    ///
    /// Read-only: the dependency set is taken from the existing `hpm.lock` +
    /// global package store, so this never fetches packages or rewrites
    /// generated Houdini manifests. The venv itself is content-addressable, so
    /// when `install` already built it this returns the same path without
    /// re-resolving wheels.
    ///
    /// # Errors
    ///
    /// [`ProjectError::PackageEnvNotReady`] when the project declares hpm
    /// dependencies but `hpm.lock` is missing or a locked package isn't in the
    /// store — i.e. `hpm install` hasn't been run. Python resolution / venv
    /// failures surface as [`ProjectError::PythonResolution`].
    pub async fn resolve_package_env(
        &self,
        extra_requirements: &[String],
    ) -> Result<PackageRunEnv, ProjectError> {
        let project_manifest = self.load_project_manifest()?.ok_or_else(|| {
            ProjectError::PackageEnvNotReady(format!(
                "No hpm.toml found at {} — a package environment needs a project manifest.",
                self.project_paths.manifest_file.display()
            ))
        })?;

        // The manifest path always has a parent (it is `<root>/hpm.toml`);
        // never fall back to cwd, which would silently resolve python/ and
        // dependency paths against wherever the process happens to run.
        let project_root = self
            .project_paths
            .manifest_file
            .parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| {
                ProjectError::PackageEnvNotReady(format!(
                    "Manifest path {} has no parent directory",
                    self.project_paths.manifest_file.display()
                ))
            })?;

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

        let project_houdini_version = project_manifest.compat.houdini_min();

        crate::python::initialize()
            .await
            .map_err(|e| ProjectError::PythonResolution(e.into()))?;
        let collected = collect_python_dependencies(project_houdini_version.as_deref(), &manifests)
            .await
            .map_err(|e| ProjectError::PythonResolution(e.into()))?;
        let resolved = resolve_combined(&collected, extra_requirements)
            .await
            .map_err(|e| ProjectError::PythonResolution(e.into()))?;

        let mut run_env = PackageRunEnv {
            python_paths,
            ..Default::default()
        };

        if !resolved.packages.is_empty() {
            let venv_manager =
                VenvManager::new().map_err(|e| ProjectError::PythonResolution(e.into()))?;
            // Record the same owners the install path records, so a venv
            // built by `hpm run` is reclaimable once its packages are gone
            // instead of falling into the unowned-and-aged-out bucket.
            let used_by: Vec<String> = dep_packages
                .iter()
                .filter(|p| !p.manifest.python_dependencies.is_empty())
                .map(InstalledPackage::venv_ref)
                .collect();
            let venv_path = venv_manager
                .ensure_virtual_environment_for(&resolved, &used_by)
                .await
                .map_err(|e| ProjectError::PythonResolution(e.into()))?;
            let site_packages =
                venv_manager.get_python_site_packages_path(&venv_path, &resolved.python_version);
            run_env.python_paths.push(site_packages);
            run_env.venv_bin = Some(venv_bin_dir(&venv_path));
            run_env.virtual_env = Some(venv_path);
        }

        Ok(run_env)
    }

    /// The project's locked, already-installed hpm dependencies.
    ///
    /// Resolves each `[dependencies]` entry through `hpm.lock` (for the exact
    /// version) and the global package store (for the on-disk install path).
    /// Returns an empty list when the project has no dependencies. Errors with
    /// [`ProjectError::PackageEnvNotReady`] when the lockfile is missing or a
    /// locked package isn't installed.
    fn installed_dependency_closure(
        &self,
        project_manifest: &PackageManifest,
    ) -> Result<Vec<InstalledPackage>, ProjectError> {
        if project_manifest.dependencies.is_empty() {
            return Ok(Vec::new());
        }

        let lock_path = &self.project_paths.lock_file;
        let lock = LockFile::load(lock_path).map_err(|e| {
            ProjectError::PackageEnvNotReady(format!(
                "Could not read {} ({e}). Run 'hpm install' to resolve and lock \
                 this project's dependencies before running a package-env script.",
                lock_path.display()
            ))
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
                    return Err(ProjectError::PackageEnvNotReady(format!(
                        "Dependency {name}@{} is locked but not installed. Run 'hpm install' \
                         to populate the package environment.",
                        locked.version
                    )));
                }
            }
        }

        Ok(packages)
    }

    fn update_project_manifest(&self, spec: &PackageSpec) -> Result<(), ProjectError> {
        let manifest_path = &self.project_paths.manifest_file;
        if !manifest_path.exists() {
            return Err(ProjectError::Manifest(ManifestLoadError::NotFound {
                path: manifest_path.clone(),
            }));
        }
        manifest_edit::upsert_dependency(
            manifest_path,
            &spec.name,
            &hpm_package::DependencySpec::registry(spec.version_req.as_str(), None),
        )?;
        Ok(())
    }

    fn remove_from_project_manifest(&self, name: &str) -> Result<(), ProjectError> {
        manifest_edit::remove_dependency(&self.project_paths.manifest_file, name)?;
        Ok(())
    }

    pub fn list_dependencies(&self) -> Result<Vec<ProjectDependency>, ProjectError> {
        let mut dependencies = vec![];

        if !self.project_paths.packages_dir.exists() {
            return Ok(dependencies);
        }

        let entries = std::fs::read_dir(&self.project_paths.packages_dir).map_err(|e| {
            IoOp::wrap(
                "read project packages directory",
                &self.project_paths.packages_dir,
                e,
            )
        })?;

        let installed_packages = self.storage_manager.list_installed()?;

        for entry in entries.flatten() {
            let path = entry.path();
            let Some(file_name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            // The overrides manifest is not a per-package file.
            if file_name == houdini_emit::PROJECT_OVERRIDES_FILE {
                continue;
            }
            let Some(stem) = file_name.strip_suffix(".json") else {
                continue;
            };
            // Manifests are named `<creator>.<slug>.json`. Anything else is a
            // legacy bare-slug file or someone else's — the stale sweep deals
            // with those; reporting them as dependencies would attach a name
            // that matches no installed package.
            let Some(package_path) = hpm_package::PackagePath::from_file_stem(stem) else {
                continue;
            };

            let installed_package = installed_packages
                .iter()
                .find(|p| p.manifest.package.path == package_path)
                .cloned();

            dependencies.push(ProjectDependency {
                name: package_path.as_str().to_string(),
                installed_package,
            });
        }

        Ok(dependencies)
    }
}

/// Install a single dependency, short-circuiting if it's already in the
/// global CAS. Spawnable from the JoinSet in `sync_dependencies` — takes
/// shared state by `&` (cloned into the task by the caller).
///
/// `all_installed` is the snapshot of `StorageManager::list_installed()`
/// captured before installs began; comparing against it avoids re-fetching
/// packages that another concurrent task may also be racing to install
/// (the CAS is idempotent under `install_into_cas`, but skipping the
/// network round-trip and the remove-and-recopy that `install_into_cas`
/// performs is worth the shared snapshot — that recopy is the
/// well-known Windows `os error 5` trigger when Houdini holds files open).
///
/// Returns `InstallOutcome` with `checksum` / `source` populated only when
/// they're known: fresh fetches get both, `Url`-spec short-circuits get the
/// URL only, `Registry` short-circuits get neither (the lockfile builder
/// can backfill those from the prior lockfile).
async fn install_one_dep(
    storage: &StorageManager,
    fetcher: &ArchiveFetcher,
    registry_set: Option<&crate::registry::RegistrySet>,
    all_installed: &[InstalledPackage],
    name: &str,
    spec: &hpm_package::DependencySpec,
) -> Result<InstallOutcome, ProjectError> {
    use hpm_package::DependencySpec;
    match spec {
        DependencySpec::Registry {
            version, registry, ..
        } => {
            if let Some(pkg) = all_installed
                .iter()
                .find(|p| ProjectManager::matches_spec_name(p, name) && p.version == *version)
            {
                info!("Package {}@{} already installed", name, version);
                return Ok(InstallOutcome {
                    package: pkg.clone(),
                    checksum: None,
                    source: None,
                });
            }
            let rs = registry_set.expect("registry set built when registry deps present");
            // A registry-resolved dep with no registries configured is its own
            // failure, distinct from "package not found": resolving against an
            // empty set would otherwise surface a misleading VersionNotFound.
            // Mirror the single-package `resolve_and_install_from_registry`
            // path so `hpm install` points the user at `hpm registry add`.
            if rs.is_empty() {
                return Err(ProjectError::NoRegistriesConfigured {
                    name: name.to_string(),
                    version_req: version.clone(),
                });
            }
            let entry = rs
                .get_version_in(name, version, registry.as_deref())
                .await
                .map_err(|source| ProjectError::RegistryResolution {
                    name: name.to_string(),
                    version_req: version.clone(),
                    source: Box::new(source),
                })?;
            let url = entry.dl.clone();
            let source = PackageSource::url(url.clone(), version)?
                .with_registry_checksum(entry.cksum.as_deref())?;
            let (package, checksum) =
                fetch_and_install_pkg(storage, fetcher, name, version, source).await?;
            Ok(InstallOutcome {
                package,
                checksum: Some(checksum),
                source: Some(LockedSource::url(url, version.clone())),
            })
        }
        DependencySpec::Url { url, version, .. } => {
            if let Some(pkg) = all_installed
                .iter()
                .find(|p| ProjectManager::matches_spec_name(p, name) && p.version == *version)
            {
                info!("Package {}@{} already installed", name, version);
                return Ok(InstallOutcome {
                    package: pkg.clone(),
                    checksum: None,
                    source: Some(LockedSource::url(url.clone(), version.clone())),
                });
            }
            let source = PackageSource::url(url, version)?;
            let (package, checksum) =
                fetch_and_install_pkg(storage, fetcher, name, version, source).await?;
            Ok(InstallOutcome {
                package,
                checksum: Some(checksum),
                source: Some(LockedSource::url(url.clone(), version.clone())),
            })
        }
        DependencySpec::Path { path, link, .. } => {
            // Unlike the Registry/Url arms there's no `all_installed`
            // skip here: a path dep's workspace can change between syncs, so we
            // always re-enter the installer. `install_inner` content-addresses
            // dev copies (`_dev/<slug>@<version>/<source-hash>/`), so an
            // unchanged workspace resolves to the same hash directory and is
            // reused untouched, while a rebuild lands in a fresh directory —
            // never removing a copy a running Houdini may have mapped.
            let source_path = std::path::Path::new(path);
            let package = if *link {
                storage.install_as_dev_link(source_path).await?
            } else {
                storage.install_as_dev_copy(source_path).await?
            };
            Ok(InstallOutcome {
                package,
                checksum: None,
                source: Some(LockedSource::path(source_path)),
            })
        }
    }
}

/// Fetch a remote package and copy it into the global CAS. Returns the
/// installed package alongside the fetcher's SHA-256 of the archive.
///
/// `pub(crate)` so `hpm global` installs go through the identical
/// fetch-and-CAS path as project installs rather than reimplementing it.
pub(crate) async fn fetch_and_install_pkg(
    storage: &StorageManager,
    fetcher: &ArchiveFetcher,
    name: &str,
    version: &str,
    source: PackageSource,
) -> Result<(InstalledPackage, String), ProjectError> {
    let fetch_result = fetcher.fetch(&source, name).await?;
    let checksum = fetch_result.checksum.clone();
    let installed = storage.install_into_cas(&fetch_result.package_path).await?;
    info!("Successfully fetched and installed {}@{}", name, version);
    Ok((installed, checksum))
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
