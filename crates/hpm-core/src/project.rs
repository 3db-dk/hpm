//! Project-level orchestration: install, sync, manifest editing, and Houdini
//! package.json emission for one `hpm.toml` project.
//!
//! [`ProjectManager`] is the entry point. Supporting types live next door:
//!
//! - [`error`] — [`ProjectError`]
//! - [`types`] — [`ProjectDependency`], [`InstallOutcome`]

use crate::archive_fetcher::ArchiveFetcher;
use crate::lock::LockedSource;
use crate::package_source::PackageSource;
use crate::python::{VenvManager, collect_python_dependencies, resolve_dependencies};
use crate::storage::{InstalledPackage, PackageSpec, StorageManager};
use hpm_config::{Config, ProjectPaths};
use hpm_package::{
    EnvMethod, HoudiniPackage, IoOp, ManifestEnvEntry, ManifestLoadError, PackageManifest,
};
use indexmap::IndexMap;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::task::JoinSet;
use tracing::{debug, info, warn};

pub mod error;
pub mod types;

pub use error::ProjectError;
pub use types::{InstallOutcome, ProjectDependency};

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

        self.generate_houdini_manifest(&installed_package)?;
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
        );

        if registry_set.is_empty() {
            return Err(ProjectError::NoRegistriesConfigured {
                name: spec.name.clone(),
                version_req: spec.version_req.as_str().to_string(),
            });
        }

        let entry = self.resolve_registry_entry(&registry_set, spec).await?;
        let version = entry.version.clone();
        let source = PackageSource::url(entry.dl, entry.version)?;
        self.fetch_and_install_pkg(&spec.name, &version, source)
            .await
    }

    /// Resolve a `PackageSpec` to a concrete `RegistryEntry`. If the spec's
    /// requirement parses as an exact semver version, look it up directly;
    /// otherwise list all versions and pick the highest matching one.
    async fn resolve_registry_entry(
        &self,
        registry_set: &crate::registry::RegistrySet,
        spec: &PackageSpec,
    ) -> Result<crate::registry::RegistryEntry, ProjectError> {
        let req_str = spec.version_req.as_str();

        if semver::Version::parse(req_str).is_ok() {
            return registry_set
                .get_version(&spec.name, req_str)
                .await
                .map_err(|source| ProjectError::RegistryResolution {
                    name: spec.name.clone(),
                    version_req: req_str.to_string(),
                    source: Box::new(source),
                });
        }

        let mut versions = registry_set
            .get_versions(&spec.name)
            .await
            .map_err(|source| ProjectError::RegistryResolution {
                name: spec.name.clone(),
                version_req: req_str.to_string(),
                source: Box::new(source),
            })?;

        versions.retain(|v| !v.yanked && spec.version_req.matches(&v.version));
        versions.sort_by(|a, b| {
            match (
                semver::Version::parse(&a.version),
                semver::Version::parse(&b.version),
            ) {
                (Ok(va), Ok(vb)) => vb.cmp(&va),
                _ => b.version.cmp(&a.version),
            }
        });

        versions
            .into_iter()
            .next()
            .ok_or_else(|| ProjectError::NoMatchingVersion {
                name: spec.name.clone(),
                version_req: req_str.to_string(),
            })
    }

    pub async fn remove_dependency(&self, name: &str) -> Result<(), ProjectError> {
        info!("Removing dependency: {}", name);

        // 1. Remove from project manifest (hpm.toml)
        self.remove_from_project_manifest(name)?;

        // 2. Remove Houdini package manifest from project
        let manifest_path = self.project_paths.package_manifest_path(name);
        if manifest_path.exists() {
            std::fs::remove_file(&manifest_path)
                .map_err(|e| IoOp::wrap("remove Houdini manifest at", &manifest_path, e))?;
            debug!("Removed Houdini manifest: {:?}", manifest_path);
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
                                registry_type: match r.registry_type {
                                    hpm_package::RegistryType::Api => hpm_config::RegistryType::Api,
                                    hpm_package::RegistryType::Git => hpm_config::RegistryType::Git,
                                },
                            })
                            .collect()
                    } else {
                        self.config.registries.clone()
                    };
                Some(Arc::new(
                    crate::registry::RegistrySet::from_configs_with_auth(
                        &registry_configs,
                        &self.config.storage.registry_cache_dir,
                        self.auth_token.as_deref(),
                    ),
                ))
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

        // Generate Houdini JSON manifests for all packages
        for pkg in &installed {
            self.generate_houdini_manifest_with_python(
                pkg,
                venv_site_packages.as_deref(),
                &project_env_overrides,
            )?;
        }

        // Sweep stale per-package manifests left over from previous syncs.
        // Houdini reads every <slug>.json file in `packages_dir` on launch, so a
        // manifest whose slug has dropped out of the dependency set (dev override
        // removed, registry yank, manual edit) keeps loading the package even
        // though hpm.toml no longer asks for it.
        self.sweep_stale_houdini_manifests(&installed)?;

        info!("Successfully synced project dependencies");
        Ok(outcomes)
    }

    /// Remove `<slug>.json` files in the project's packages dir whose slug is
    /// not in `installed_packages`. Only the per-package manifests we own are
    /// considered — non-`.json` entries and any unknown files are left alone.
    fn sweep_stale_houdini_manifests(
        &self,
        installed_packages: &[InstalledPackage],
    ) -> Result<(), ProjectError> {
        let packages_dir = &self.project_paths.packages_dir;
        if !packages_dir.exists() {
            return Ok(());
        }

        let valid_slugs: std::collections::HashSet<&str> = installed_packages
            .iter()
            .map(|pkg| pkg.manifest.package.slug())
            .collect();

        let entries = std::fs::read_dir(packages_dir)
            .map_err(|e| IoOp::wrap("read project packages directory", packages_dir, e))?;

        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let file_name = match path.file_name().and_then(|n| n.to_str()) {
                Some(name) => name,
                None => continue,
            };
            let slug = match file_name.strip_suffix(".json") {
                Some(slug) => slug,
                None => continue,
            };
            if valid_slugs.contains(slug) {
                continue;
            }

            match std::fs::remove_file(&path) {
                Ok(()) => debug!("Removed stale Houdini manifest: {}", path.display()),
                Err(e) => {
                    // Don't fail the whole sync if one stale manifest can't be
                    // removed (e.g. Houdini holds it open on Windows). Surface
                    // it so the user can act.
                    warn!(
                        "Failed to remove stale Houdini manifest {}: {}",
                        path.display(),
                        e
                    );
                }
            }
        }

        Ok(())
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

    fn generate_houdini_manifest(
        &self,
        installed_package: &InstalledPackage,
    ) -> Result<(), ProjectError> {
        self.generate_houdini_manifest_with_python(installed_package, None, &IndexMap::new())
    }

    fn generate_houdini_manifest_with_python(
        &self,
        installed_package: &InstalledPackage,
        venv_site_packages: Option<&Path>,
        project_env_overrides: &IndexMap<String, ManifestEnvEntry>,
    ) -> Result<(), ProjectError> {
        let houdini_package = self.create_houdini_package_with_python(
            installed_package,
            venv_site_packages,
            project_env_overrides,
        )?;
        let manifest_path = self
            .project_paths
            .package_manifest_path(installed_package.manifest.package.slug());

        let content = serde_json::to_vec_pretty(&houdini_package).map_err(|source| {
            ProjectError::HoudiniManifestSerialize {
                path: manifest_path.clone(),
                source,
            }
        })?;
        hpm_package::atomic_write(&manifest_path, content)?;

        debug!(
            "Generated Houdini manifest for {}",
            installed_package.manifest.package.slug()
        );
        Ok(())
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
            .load_project_manifest()
            .ok()
            .flatten()
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

        // Ensure venv exists (content-addressable, shared across identical dep sets)
        let venv_manager =
            VenvManager::new().map_err(|e| ProjectError::PythonResolution(e.into()))?;
        let venv_path = venv_manager
            .ensure_virtual_environment(&resolved)
            .await
            .map_err(|e| ProjectError::PythonResolution(e.into()))?;

        let site_packages =
            venv_manager.get_python_site_packages_path(&venv_path, &resolved.python_version);
        info!("Python venv site-packages: {}", site_packages.display());
        Ok(Some(site_packages))
    }

    #[cfg(test)]
    fn create_houdini_package(
        &self,
        installed_package: &InstalledPackage,
    ) -> Result<HoudiniPackage, ProjectError> {
        self.create_houdini_package_with_python(installed_package, None, &IndexMap::new())
    }

    fn create_houdini_package_with_python(
        &self,
        installed_package: &InstalledPackage,
        venv_site_packages: Option<&Path>,
        project_env_overrides: &IndexMap<String, ManifestEnvEntry>,
    ) -> Result<HoudiniPackage, ProjectError> {
        let package_path = &installed_package.install_path;

        // Point hpath at the package root so Houdini auto-discovers convention
        // subdirs (otls/, desktop/, toolbar/, python_panels/, viewer_states/,
        // python3.11libs/, etc.). See sidefx.com/docs/houdini/ref/plugins.html.
        let hpath = vec![package_path.to_string_lossy().to_string()];

        // Build environment variables
        let mut env = vec![];

        // Inject venv site-packages for packages that declare python_dependencies
        if let Some(site_packages) = venv_site_packages
            && !installed_package.manifest.python_dependencies.is_empty()
        {
            let mut python_env = HashMap::new();
            python_env.insert(
                "PYTHONPATH".to_string(),
                hpm_package::HoudiniEnvValue::Detailed {
                    method: "prepend".to_string(),
                    value: site_packages.to_string_lossy().to_string(),
                },
            );
            env.push(python_env);
        }

        // Package's own python/ directory
        if package_path.join("python").exists() {
            let mut python_env = HashMap::new();
            python_env.insert(
                "PYTHONPATH".to_string(),
                hpm_package::HoudiniEnvValue::Detailed {
                    method: "prepend".to_string(),
                    value: package_path.join("python").to_string_lossy().to_string(),
                },
            );
            env.push(python_env);
        }

        // Scripts path
        if package_path.join("scripts").exists() {
            let mut scripts_env = HashMap::new();
            scripts_env.insert(
                "HOUDINI_SCRIPT_PATH".to_string(),
                hpm_package::HoudiniEnvValue::Detailed {
                    method: "prepend".to_string(),
                    value: package_path.join("scripts").to_string_lossy().to_string(),
                },
            );
            env.push(scripts_env);
        }

        // Append user-defined env vars from [runtime], reconciling each
        // package entry with any project-level [runtime] override of the
        // same key:
        //
        // * `set` (or no override) — the effective entry replaces the
        //   package's contribution wholesale.
        // * `append` / `prepend` — the package's own entry is emitted
        //   first, then the project's override, so Houdini's native
        //   package system merges them in load order with the requested
        //   method. This lets a project *extend* a package-provided value
        //   rather than clobber it.
        //
        // Each entry's conditional variants are filtered by `is_dev` —
        // branches gated to a non-matching `install_source` drop out, so
        // dev-only contributions never reach a registry consumer's Houdini
        // manifest and registry-only contributions never reach a dev
        // install. A required-but-unsupplied placeholder (no value from the
        // package and none from the project) surfaces as `MissingRequiredEnv`.
        if !installed_package.manifest.runtime.is_empty() {
            let pkg_root = package_path.to_string_lossy().into_owned();
            let user_runtime = &installed_package.manifest.runtime;
            let slug = installed_package.manifest.package.slug().to_string();
            let is_dev = installed_package.is_dev;

            // Lower one entry and, unless it is inert in this install
            // context (every variant install-source-filtered out), push it
            // onto `env` under `key`.
            let emit = |key: &str,
                        entry: &ManifestEnvEntry,
                        env: &mut Vec<HashMap<String, hpm_package::HoudiniEnvValue>>|
             -> Result<(), ProjectError> {
                let lowered = entry
                    .lower(&[("$HPM_PACKAGE_ROOT", &pkg_root)], is_dev)
                    .map_err(|e| ProjectError::InvalidEnvExpression {
                        var: key.to_string(),
                        package: slug.clone(),
                        message: e.to_string(),
                    })?;
                if let Some(houdini_value) = lowered {
                    let mut env_map = HashMap::new();
                    env_map.insert(key.to_string(), houdini_value);
                    env.push(env_map);
                }
                Ok(())
            };

            for key in user_runtime.keys() {
                let pkg_entry = user_runtime
                    .get(key)
                    .expect("key originates from package's [runtime]");
                let project_override = project_env_overrides.get(key);

                match project_override {
                    // No project override: emit the package's own entry. A
                    // valueless entry here is an unsatisfied required
                    // placeholder.
                    None => {
                        if pkg_entry.value.is_none() {
                            return Err(ProjectError::MissingRequiredEnv {
                                var: key.clone(),
                                package: slug.clone(),
                            });
                        }
                        emit(key, pkg_entry, &mut env)?;
                    }
                    // `set` replaces the package's contribution wholesale.
                    Some(over) if over.method == EnvMethod::Set => {
                        if over.value.is_none() {
                            return Err(ProjectError::MissingRequiredEnv {
                                var: key.clone(),
                                package: slug.clone(),
                            });
                        }
                        emit(key, over, &mut env)?;
                    }
                    // `append` / `prepend` combine with the package value:
                    // emit the package's entry first (so Houdini merges in
                    // load order), then the project's override. A valueless
                    // package entry (required placeholder) contributes
                    // nothing and is satisfied by the project's value.
                    Some(over) => {
                        if pkg_entry.value.is_none() && over.value.is_none() {
                            return Err(ProjectError::MissingRequiredEnv {
                                var: key.clone(),
                                package: slug.clone(),
                            });
                        }
                        if pkg_entry.value.is_some() {
                            emit(key, pkg_entry, &mut env)?;
                        }
                        if over.value.is_some() {
                            emit(key, over, &mut env)?;
                        }
                    }
                }
            }
        }

        // Generate enable condition from [compat].houdini. The range is a
        // `HoudiniRange` newtype that validated at parse time, so emitting
        // the expression is infallible here.
        let enable = installed_package
            .manifest
            .compat
            .houdini
            .as_ref()
            .map(hpm_package::HoudiniRange::to_enable_expression);

        Ok(HoudiniPackage {
            hpath: if hpath.is_empty() { None } else { Some(hpath) },
            env: if env.is_empty() { None } else { Some(env) },
            enable,
            requires: None,
            recommends: None,
        })
    }

    /// Read hpm.toml, parse as a `toml_edit::DocumentMut`, hand it to `f`,
    /// then write back. The caller is responsible for any pre-condition
    /// check (e.g. existence) — this helper assumes the manifest is there.
    fn with_manifest_edit<F>(&self, f: F) -> Result<(), ProjectError>
    where
        F: FnOnce(&mut toml_edit::DocumentMut) -> Result<(), ProjectError>,
    {
        let path = self.project_paths.manifest_file.clone();

        let content = std::fs::read_to_string(&path).map_err(|source| {
            if source.kind() == std::io::ErrorKind::NotFound {
                ProjectError::Manifest(ManifestLoadError::NotFound { path: path.clone() })
            } else {
                ProjectError::Io(IoOp::wrap("read project manifest", &path, source))
            }
        })?;

        let mut doc: toml_edit::DocumentMut =
            content
                .parse()
                .map_err(|source: toml_edit::TomlError| ProjectError::ManifestEdit {
                    path: path.clone(),
                    source,
                })?;

        f(&mut doc)?;

        std::fs::write(&path, doc.to_string())
            .map_err(|e| IoOp::wrap("write project manifest", &path, e).into())
    }

    fn update_project_manifest(&self, spec: &PackageSpec) -> Result<(), ProjectError> {
        let manifest_path = self.project_paths.manifest_file.clone();
        if !manifest_path.exists() {
            return Err(ProjectError::Manifest(ManifestLoadError::NotFound {
                path: manifest_path,
            }));
        }

        let version_str = spec.version_req.as_str().to_string();
        let name = spec.name.clone();

        self.with_manifest_edit(|doc| {
            if !doc.contains_key("dependencies") {
                doc["dependencies"] = toml_edit::Item::Table(toml_edit::Table::new());
            }

            let deps_table = doc["dependencies"].as_table_mut().ok_or_else(|| {
                ProjectError::ManifestStructure {
                    path: manifest_path.clone(),
                    message: "[dependencies] is not a table".to_string(),
                }
            })?;

            // Simple string form for ^/~/>/< prefixes, exact, and "*";
            // anything else (e.g. registry-named specs) goes through an
            // inline table so toml_edit picks the right shape.
            let bare_form = version_str == "*"
                || version_str.starts_with(['^', '~', '>', '<'])
                || version_str
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_digit());

            if bare_form {
                deps_table[&name] = toml_edit::value(&version_str);
            } else {
                let mut inline = toml_edit::InlineTable::new();
                inline.insert("version", version_str.as_str().into());
                deps_table[&name] = toml_edit::Item::Value(toml_edit::Value::InlineTable(inline));
            }

            Ok(())
        })?;

        info!(
            "Updated hpm.toml with dependency: {} = \"{}\"",
            spec.name, version_str
        );
        Ok(())
    }

    fn remove_from_project_manifest(&self, name: &str) -> Result<(), ProjectError> {
        if !self.project_paths.manifest_file.exists() {
            return Ok(()); // Nothing to remove
        }

        let dep_name = name.to_string();
        self.with_manifest_edit(|doc| {
            for section in ["dependencies", "dev-dependencies"] {
                if let Some(deps) = doc.get_mut(section)
                    && let Some(table) = deps.as_table_mut()
                    && table.contains_key(&dep_name)
                {
                    table.remove(&dep_name);
                    info!("Removed {} from [{}]", dep_name, section);
                }
            }
            Ok(())
        })
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
            if let Some(file_name) = entry.path().file_name() {
                if let Some(name_str) = file_name.to_str() {
                    if name_str.ends_with(".json") {
                        let package_name = name_str.trim_end_matches(".json");

                        // Find corresponding installed package
                        let installed_package = installed_packages
                            .iter()
                            .find(|p| p.manifest.package.slug() == package_name)
                            .cloned();

                        let version = installed_package
                            .as_ref()
                            .map(|p| p.version.clone())
                            .unwrap_or_else(|| "unknown".to_string());

                        dependencies.push(ProjectDependency {
                            name: package_name.to_string(),
                            version,
                            installed_package,
                        });
                    }
                }
            }
        }

        Ok(dependencies)
    }

    pub fn generate_houdini_manifests(&self) -> Result<(), ProjectError> {
        info!("Regenerating all Houdini manifests");

        let dependencies = self.list_dependencies()?;

        for dep in dependencies {
            if let Some(installed_package) = dep.installed_package {
                self.generate_houdini_manifest(&installed_package)?;
            }
        }

        info!("Successfully regenerated all Houdini manifests");
        Ok(())
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
/// URL only, `Simple`/`Registry` short-circuits get neither (the lockfile
/// builder can backfill those from the prior lockfile).
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
        DependencySpec::Simple(version) | DependencySpec::Registry { version, .. } => {
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
            let entry = rs.get_version(name, version).await.map_err(|source| {
                ProjectError::RegistryResolution {
                    name: name.to_string(),
                    version_req: version.clone(),
                    source: Box::new(source),
                }
            })?;
            let url = entry.dl.clone();
            let source = PackageSource::url(url.clone(), version)?;
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
async fn fetch_and_install_pkg(
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
