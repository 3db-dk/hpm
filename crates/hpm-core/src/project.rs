use crate::archive_fetcher::{ArchiveFetcher, FetchError};
use crate::lock::LockedSource;
use crate::package_source::{PackageSource, PackageSourceError};
use crate::registry::RegistryError;
use crate::storage::{InstalledPackage, PackageSpec, StorageError, StorageManager};
use hpm_config::{Config, ProjectConfig};
use hpm_package::{
    HoudiniPackage, ManifestEnvEntry, ManifestLoadError, PackageManifest, compile_houdini_req,
};
use hpm_python::{VenvManager, collect_python_dependencies, resolve_dependencies};
use indexmap::IndexMap;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::task::JoinSet;
use tracing::{debug, info, warn};

#[derive(Debug)]
pub struct ProjectManager {
    config: Arc<Config>,
    project_config: ProjectConfig,
    storage_manager: Arc<StorageManager>,
    fetcher: ArchiveFetcher,
    auth_token: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ProjectDependency {
    pub name: String,
    pub version: String,
    pub installed_package: Option<InstalledPackage>,
}

/// Per-dependency record returned from `sync_dependencies`, carrying the
/// install path plus the metadata a lockfile needs.
///
/// `checksum` and `source` are `Option` because a sync that short-circuits
/// on the CAS (already-installed package) doesn't re-fetch from the
/// registry, so it has no fresh SHA-256 and (for `Simple`/`Registry` specs)
/// no fresh URL to record. Callers wanting lockfile fidelity can backfill
/// those `None` fields from a prior lockfile entry.
#[derive(Debug, Clone)]
pub struct InstallOutcome {
    pub package: InstalledPackage,
    /// SHA-256 of the archive — `Some` when the dep was freshly fetched,
    /// `None` for path deps and short-circuited CAS hits.
    pub checksum: Option<String>,
    /// Lockfile source — `Some` when we know the URL (fresh fetch or `Url`
    /// spec) or for path deps, `None` for `Simple`/`Registry` short-circuits.
    pub source: Option<LockedSource>,
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
        let project_config = hpm_config::Config::load_project_config(&project_root);

        // Fetcher staging lives next to the global CAS, not inside it.
        let storage_root = config
            .storage
            .packages_dir
            .parent()
            .unwrap_or(std::path::Path::new("."));
        let cache_dir = storage_root.join("cache");
        let fetch_packages_dir = storage_root.join("fetch");
        let fetcher = ArchiveFetcher::new(cache_dir, fetch_packages_dir)?;

        let manager = Self {
            config,
            project_config,
            storage_manager,
            fetcher,
            auth_token,
        };

        manager.ensure_directories()?;
        Ok(manager)
    }

    fn ensure_directories(&self) -> Result<(), ProjectError> {
        // ProjectConfig::ensure_directories does mkdir -p on the well-known
        // dirs; bubble io::Error along with the project root so the failure
        // names a path the user can reason about.
        self.project_config.ensure_directories().map_err(|source| {
            ProjectError::DirectoryCreation {
                path: self.project_config.packages_dir.clone(),
                source,
            }
        })?;
        info!("Ensured project directories exist");
        Ok(())
    }

    pub fn load_project_manifest(&self) -> Result<Option<PackageManifest>, ProjectError> {
        match PackageManifest::from_path(&self.project_config.manifest_file) {
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
        let manifest_path = self.project_config.package_manifest_path(name);
        if manifest_path.exists() {
            std::fs::remove_file(&manifest_path).map_err(|source| ProjectError::ManifestIo {
                op: "remove",
                path: manifest_path.clone(),
                source,
            })?;
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
        let dependencies = project_manifest.dependencies.unwrap_or_default();

        // Build registry set once for any registry-resolved deps. A manifest
        // [[registries]] override beats the user's [registries] from config.
        // Wrapped in Arc so each spawned task can hold a cheap clone.
        let registry_set: Option<Arc<crate::registry::RegistrySet>> =
            if dependencies.values().any(|spec| spec.is_registry()) {
                let registry_configs: Vec<hpm_config::RegistrySourceConfig> =
                    if let Some(ref regs) = manifest_registries {
                        regs.iter()
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
        let packages_dir = &self.project_config.packages_dir;
        if !packages_dir.exists() {
            return Ok(());
        }

        let valid_slugs: std::collections::HashSet<&str> = installed_packages
            .iter()
            .map(|pkg| pkg.manifest.package.slug())
            .collect();

        let entries =
            std::fs::read_dir(packages_dir).map_err(|source| ProjectError::DirectoryRead {
                path: packages_dir.clone(),
                source,
            })?;

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
        self.generate_houdini_manifest_with_python(installed_package, None, &None)
    }

    fn generate_houdini_manifest_with_python(
        &self,
        installed_package: &InstalledPackage,
        venv_site_packages: Option<&Path>,
        project_env_overrides: &Option<IndexMap<String, ManifestEnvEntry>>,
    ) -> Result<(), ProjectError> {
        let houdini_package = self.create_houdini_package_with_python(
            installed_package,
            venv_site_packages,
            project_env_overrides,
        )?;
        let manifest_path = self
            .project_config
            .package_manifest_path(installed_package.manifest.package.slug());

        // Atomic write: stage to <path>.tmp then rename. Houdini reads
        // these manifests on launch; a crash or interrupt mid-write leaves
        // a truncated JSON that Houdini chokes on and blocks the project.
        let mut tmp_path = manifest_path.as_os_str().to_os_string();
        tmp_path.push(".tmp");
        let tmp_path = PathBuf::from(tmp_path);

        {
            let file =
                std::fs::File::create(&tmp_path).map_err(|source| ProjectError::ManifestIo {
                    op: "create",
                    path: tmp_path.clone(),
                    source,
                })?;
            let mut writer = std::io::BufWriter::new(file);
            serde_json::to_writer_pretty(&mut writer, &houdini_package).map_err(|source| {
                ProjectError::HoudiniManifestSerialize {
                    path: tmp_path.clone(),
                    source,
                }
            })?;
            use std::io::Write;
            writer.flush().map_err(|source| ProjectError::ManifestIo {
                op: "flush",
                path: tmp_path.clone(),
                source,
            })?;
        }

        std::fs::rename(&tmp_path, &manifest_path).map_err(|source| ProjectError::ManifestIo {
            op: "rename",
            path: manifest_path.clone(),
            source,
        })?;

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
            .any(|p| p.manifest.python_dependencies.is_some());
        if !has_python_deps {
            return Ok(None);
        }

        info!("Resolving Python pip dependencies");

        // Initialize UV binary (downloads on first use)
        hpm_python::initialize()
            .await
            .map_err(|e| ProjectError::PythonResolution(e.into()))?;

        // Collect python dependencies from all package manifests. The project
        // manifest's own Houdini version is the source of truth for which
        // CPython we target — Houdini ships a fixed embedded interpreter
        // (20.5→3.10, 21→3.11, 22→3.13), and per-package `[compat].houdini`
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
            .and_then(|m| m.compat.and_then(|c| c.houdini_min()));
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
        let venv_manager = VenvManager::new();
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
        self.create_houdini_package_with_python(installed_package, None, &None)
    }

    fn create_houdini_package_with_python(
        &self,
        installed_package: &InstalledPackage,
        venv_site_packages: Option<&Path>,
        project_env_overrides: &Option<IndexMap<String, ManifestEnvEntry>>,
    ) -> Result<HoudiniPackage, ProjectError> {
        let package_path = &installed_package.install_path;

        // Point hpath at the package root so Houdini auto-discovers convention
        // subdirs (otls/, desktop/, toolbar/, python_panels/, viewer_states/,
        // python3.11libs/, etc.). See sidefx.com/docs/houdini/ref/plugins.html.
        let hpath = vec![package_path.to_string_lossy().to_string()];

        // Build environment variables
        let mut env = vec![];

        // Inject venv site-packages for packages that declare python_dependencies
        if let Some(site_packages) = venv_site_packages {
            if installed_package.manifest.python_dependencies.is_some() {
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

        // Append user-defined env vars from [runtime], with project-level
        // [runtime] overrides winning per-key. Each entry's conditional
        // variants are filtered by `is_dev` — branches gated to a
        // non-matching `install_source` drop out, so dev-only contributions
        // never reach a registry consumer's Houdini manifest and
        // registry-only contributions never reach a dev install. A
        // required-but-unsupplied placeholder (no value, no project
        // override) surfaces as `MissingRequiredEnv`.
        let user_runtime_opt = installed_package.manifest.runtime.as_ref();

        if let Some(user_runtime) = user_runtime_opt {
            let pkg_root = package_path.to_string_lossy().into_owned();

            for key in user_runtime.keys() {
                let project_override = project_env_overrides
                    .as_ref()
                    .and_then(|overrides| overrides.get(key));
                let pkg_entry = user_runtime.get(key);
                let effective_entry = project_override
                    .or(pkg_entry)
                    .expect("key originates from package's [runtime]");

                if effective_entry.value.is_none() {
                    return Err(ProjectError::MissingRequiredEnv {
                        var: key.clone(),
                        package: installed_package.manifest.package.slug().to_string(),
                    });
                }

                let lowered = effective_entry
                    .lower(
                        &[("$HPM_PACKAGE_ROOT", &pkg_root)],
                        installed_package.is_dev,
                    )
                    .map_err(|e| ProjectError::InvalidEnvExpression {
                        var: key.clone(),
                        package: installed_package.manifest.package.slug().to_string(),
                        message: e.to_string(),
                    })?;
                let Some(houdini_value) = lowered else {
                    // Every variant was install-source-filtered out for this
                    // install context — the entry is inert here. Skip silently.
                    continue;
                };
                let mut env_map = HashMap::new();
                env_map.insert(key.clone(), houdini_value);
                env.push(env_map);
            }
        }

        // Generate enable condition from [compat].houdini.
        let enable = installed_package
            .manifest
            .compat
            .as_ref()
            .and_then(|c| c.houdini.as_deref())
            .map(|req| {
                compile_houdini_req(req).map_err(|e| ProjectError::InvalidHoudiniCompat {
                    package: installed_package.manifest.package.slug().to_string(),
                    message: format!("'{}': {}", req, e),
                })
            })
            .transpose()?;

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
        let path = self.project_config.manifest_file.clone();

        let content = std::fs::read_to_string(&path).map_err(|source| {
            if source.kind() == std::io::ErrorKind::NotFound {
                ProjectError::Manifest(ManifestLoadError::NotFound { path: path.clone() })
            } else {
                ProjectError::ManifestIo {
                    op: "read",
                    path: path.clone(),
                    source,
                }
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

        std::fs::write(&path, doc.to_string()).map_err(|source| ProjectError::ManifestIo {
            op: "write",
            path,
            source,
        })
    }

    fn update_project_manifest(&self, spec: &PackageSpec) -> Result<(), ProjectError> {
        let manifest_path = self.project_config.manifest_file.clone();
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
        if !self.project_config.manifest_file.exists() {
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

        if !self.project_config.packages_dir.exists() {
            return Ok(dependencies);
        }

        let entries = std::fs::read_dir(&self.project_config.packages_dir).map_err(|source| {
            ProjectError::DirectoryRead {
                path: self.project_config.packages_dir.clone(),
                source,
            }
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
/// (the CAS is idempotent under `install_from_path`, but skipping the
/// network round-trip and the remove-and-recopy that `install_from_path`
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
                storage.install_from_path_dev_link(source_path).await?
            } else {
                storage.install_from_path_dev(source_path).await?
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
    let installed = storage
        .install_from_path(&fetch_result.package_path)
        .await?;
    info!("Successfully fetched and installed {}@{}", name, version);
    Ok((installed, checksum))
}

#[derive(Debug, thiserror::Error)]
pub enum ProjectError {
    /// Failed to create a directory the project depends on (`.hpm/packages`,
    /// fetcher cache, etc.). Carries the typed `io::Error` so callers can
    /// match on `ErrorKind` (e.g. `PermissionDenied`).
    #[error("Failed to create directory {}", path.display())]
    DirectoryCreation {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// Failed to read a directory the project depends on.
    #[error("Failed to read directory {}", path.display())]
    DirectoryRead {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error(transparent)]
    Manifest(#[from] ManifestLoadError),

    /// I/O failure on a manifest file we own (hpm.toml or a per-package
    /// Houdini JSON). `op` is a verb like "read", "write", or "remove".
    #[error("Failed to {op} {}", path.display())]
    ManifestIo {
        op: &'static str,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// hpm.toml could not be parsed as an editable TOML document. Distinct
    /// from `Manifest(ManifestLoadError::Parse)` because the edit paths
    /// (`update_project_manifest`, `remove_from_project_manifest`) use
    /// `toml_edit::DocumentMut`, which carries its own error type.
    #[error("Failed to parse {} as editable TOML", path.display())]
    ManifestEdit {
        path: PathBuf,
        #[source]
        source: toml_edit::TomlError,
    },

    /// hpm.toml has the wrong structure for the operation (e.g.
    /// `[dependencies]` exists but is not a table).
    #[error("{}: {message}", path.display())]
    ManifestStructure { path: PathBuf, message: String },

    /// Failed to serialise a Houdini package.json.
    #[error("Failed to serialise Houdini manifest at {}", path.display())]
    HoudiniManifestSerialize {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },

    /// Global package storage CAS read/write failure. Boxed because
    /// `StorageError` is large; keeps `ProjectError` itself small enough
    /// that `Result<T, ProjectError>` stays cheap to return on the hot
    /// success path.
    #[error(transparent)]
    Storage(Box<StorageError>),

    /// Archive download / extract failure. Boxed; see `Storage`.
    #[error(transparent)]
    Fetch(Box<FetchError>),

    /// A package source URL could not be parsed.
    #[error(transparent)]
    InvalidPackageSource(#[from] PackageSourceError),

    /// Dependency requested but no registries are configured.
    #[error("Cannot install {name} {version_req}: no registries configured")]
    NoRegistriesConfigured { name: String, version_req: String },

    /// Registry lookup failed for `name@version_req`. Source is boxed;
    /// see `Storage`.
    #[error("Failed to resolve {name} {version_req} from registry")]
    RegistryResolution {
        name: String,
        version_req: String,
        #[source]
        source: Box<RegistryError>,
    },

    /// Registry returned versions, but none satisfied the requirement.
    #[error("No version of {name} matches requirement {version_req}")]
    NoMatchingVersion { name: String, version_req: String },

    /// Python dependency collection / resolution / venv creation failed.
    /// `hpm-python` returns `anyhow::Error`; we box the source rather than
    /// pull in anyhow at this layer.
    #[error("Python dependency resolution failed")]
    PythonResolution(#[source] Box<dyn std::error::Error + Send + Sync>),

    #[error(
        "Required env var '{var}' for package '{package}' has no value. \
         Set it in this project's [runtime] section in hpm.toml."
    )]
    MissingRequiredEnv { var: String, package: String },

    #[error("Invalid conditional value for env var '{var}' in package '{package}': {message}")]
    InvalidEnvExpression {
        var: String,
        package: String,
        message: String,
    },

    /// `[compat].houdini` in a package manifest could not be parsed as a
    /// Cargo-style range. `PackageManifest::validate` catches this at load
    /// time, so reaching this variant means a manifest was constructed
    /// programmatically and never validated.
    #[error("Invalid [compat].houdini in package '{package}': {message}")]
    InvalidHoudiniCompat { package: String, message: String },
}

// Hand-written so call sites can `?` from the unboxed source error types.
// thiserror's `#[from]` would only generate `From<Box<X>>`; we want the
// boxing to be invisible at the use site.
impl From<StorageError> for ProjectError {
    fn from(err: StorageError) -> Self {
        Self::Storage(Box::new(err))
    }
}

impl From<FetchError> for ProjectError {
    fn from(err: FetchError) -> Self {
        Self::Fetch(Box::new(err))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hpm_package::PackagePath;
    use tempfile::TempDir;

    /// Build the `(Config, StorageManager)` pair every `ProjectManager` test
    /// needs, rooted inside `temp_dir`. The CAS lives at `<temp>/.hpm/packages`.
    fn test_setup(temp_dir: &Path) -> (Arc<Config>, Arc<StorageManager>) {
        let storage_config = hpm_config::StorageConfig {
            home_dir: temp_dir.join(".hpm"),
            cache_dir: temp_dir.join(".hpm").join("cache"),
            packages_dir: temp_dir.join(".hpm").join("packages"),
            registry_cache_dir: temp_dir.join(".hpm").join("registry"),
        };
        let config = Arc::new(Config {
            storage: storage_config.clone(),
            ..Default::default()
        });
        let storage_manager = Arc::new(StorageManager::new(storage_config).unwrap());
        (config, storage_manager)
    }

    #[tokio::test]
    async fn project_manager_creation() {
        let temp_dir = TempDir::new().unwrap();
        let (config, storage_manager) = test_setup(temp_dir.path());
        let project_root = temp_dir.path().join("project");
        std::fs::create_dir_all(&project_root).unwrap();

        let _project_manager =
            ProjectManager::new(project_root.clone(), storage_manager, config).unwrap();
        assert!(project_root.join(".hpm").join("packages").exists());
    }

    #[tokio::test]
    async fn new_with_auth_none_matches_new() {
        // Regression: `new(...)` must remain a one-line delegate to
        // `new_with_auth(..., None)`. If the two paths diverge, anonymous
        // callers (the CLI, every existing embedder) would silently drift
        // from the authenticated path's behavior.
        let temp_dir = TempDir::new().unwrap();
        let (config, storage_manager) = test_setup(temp_dir.path());
        let project_root = temp_dir.path().join("project");
        std::fs::create_dir_all(&project_root).unwrap();

        let pm = ProjectManager::new_with_auth(project_root.clone(), storage_manager, config, None)
            .unwrap();
        assert!(pm.auth_token.is_none());
        assert!(project_root.join(".hpm").join("packages").exists());
    }

    #[tokio::test]
    async fn new_with_auth_some_stashes_token() {
        let temp_dir = TempDir::new().unwrap();
        let (config, storage_manager) = test_setup(temp_dir.path());
        let project_root = temp_dir.path().join("project");
        std::fs::create_dir_all(&project_root).unwrap();

        let pm = ProjectManager::new_with_auth(
            project_root,
            storage_manager,
            config,
            Some("supabase-access-token-xyz".to_string()),
        )
        .unwrap();
        assert_eq!(pm.auth_token.as_deref(), Some("supabase-access-token-xyz"));
    }

    #[test]
    fn list_dependencies_empty_project() {
        let temp_dir = TempDir::new().unwrap();
        let (config, storage_manager) = test_setup(temp_dir.path());
        let project_root = temp_dir.path().join("project");
        std::fs::create_dir_all(&project_root).unwrap();

        let project_manager = ProjectManager::new(project_root, storage_manager, config).unwrap();
        let deps = project_manager.list_dependencies().unwrap();
        assert_eq!(deps.len(), 0);
    }

    #[test]
    fn create_houdini_package_basic() {
        let temp_dir = TempDir::new().unwrap();
        let (config, storage_manager) = test_setup(temp_dir.path());
        let project_root = temp_dir.path().join("project");
        std::fs::create_dir_all(&project_root).unwrap();

        let project_manager = ProjectManager::new(project_root, storage_manager, config).unwrap();

        let manifest = hpm_package::PackageManifest::new(
            PackagePath::new("studio/test-package").unwrap(),
            "Test Package".to_string(),
            "1.0.0".to_string(),
            Some("A test package".to_string()),
            None,
            None,
        );

        let package_path = temp_dir.path().join("test-package@1.0.0");
        std::fs::create_dir_all(package_path.join("python")).unwrap();
        std::fs::create_dir_all(package_path.join("otls")).unwrap();

        let installed_package = InstalledPackage {
            version: "1.0.0".to_string(),
            manifest,
            install_path: package_path.clone(),
            is_dev: false,
        };

        let houdini_package = project_manager
            .create_houdini_package(&installed_package)
            .unwrap();
        assert_eq!(
            houdini_package.hpath,
            Some(vec![package_path.to_string_lossy().to_string()])
        );
        assert!(houdini_package.env.is_some());
    }

    #[test]
    fn create_houdini_package_with_project_env_overrides() {
        let temp_dir = TempDir::new().unwrap();
        let (config, storage_manager) = test_setup(temp_dir.path());
        let project_root = temp_dir.path().join("project");
        std::fs::create_dir_all(&project_root).unwrap();

        let project_manager = ProjectManager::new(project_root, storage_manager, config).unwrap();

        // Create a manifest with an env var
        let mut manifest = hpm_package::PackageManifest::new(
            PackagePath::new("studio/test-package").unwrap(),
            "Test Package".to_string(),
            "1.0.0".to_string(),
            Some("A test package".to_string()),
            None,
            None,
        );
        let mut pkg_env = IndexMap::new();
        pkg_env.insert(
            "MY_CONFIG".to_string(),
            ManifestEnvEntry {
                method: hpm_package::EnvMethod::Set,
                value: Some("$HPM_PACKAGE_ROOT/default-config".into()),
                required: false,
            },
        );
        manifest.runtime = Some(pkg_env);

        let package_path = temp_dir.path().join("test-package@1.0.0");
        std::fs::create_dir_all(&package_path).unwrap();

        let installed_package = InstalledPackage {
            version: "1.0.0".to_string(),
            manifest,
            install_path: package_path.clone(),
            is_dev: false,
        };

        // Without override: should use package default
        let houdini_package = project_manager
            .create_houdini_package(&installed_package)
            .unwrap();
        assert_eq!(
            houdini_package.hpath,
            Some(vec![package_path.to_string_lossy().to_string()])
        );
        let env_entries = houdini_package.env.as_ref().unwrap();
        let my_config_entry = env_entries
            .iter()
            .find(|m| m.contains_key("MY_CONFIG"))
            .unwrap();
        match my_config_entry.get("MY_CONFIG").unwrap() {
            hpm_package::HoudiniEnvValue::Detailed { value, .. } => {
                assert!(value.ends_with("/default-config"));
            }
            _ => panic!("Expected Detailed env value"),
        }

        // With project override: should use override value
        let mut project_overrides = IndexMap::new();
        project_overrides.insert(
            "MY_CONFIG".to_string(),
            ManifestEnvEntry {
                method: hpm_package::EnvMethod::Set,
                value: Some("/custom/config/path".into()),
                required: false,
            },
        );

        let houdini_package = project_manager
            .create_houdini_package_with_python(&installed_package, None, &Some(project_overrides))
            .unwrap();
        let env_entries = houdini_package.env.as_ref().unwrap();
        let my_config_entry = env_entries
            .iter()
            .find(|m| m.contains_key("MY_CONFIG"))
            .unwrap();
        match my_config_entry.get("MY_CONFIG").unwrap() {
            hpm_package::HoudiniEnvValue::Detailed { method, value } => {
                assert_eq!(value, "/custom/config/path");
                assert_eq!(method, "set");
            }
            _ => panic!("Expected Detailed env value"),
        }
    }

    #[tokio::test]
    async fn create_houdini_package_required_env_without_override_errors() {
        let temp_dir = TempDir::new().unwrap();
        let (config, storage_manager) = test_setup(temp_dir.path());
        let project_root = temp_dir.path().join("project");
        std::fs::create_dir_all(&project_root).unwrap();
        let project_manager = ProjectManager::new(project_root, storage_manager, config).unwrap();

        let mut manifest = hpm_package::PackageManifest::new(
            PackagePath::new("studio/needs-config").unwrap(),
            "Needs Config".to_string(),
            "1.0.0".to_string(),
            None,
            None,
            None,
        );
        let mut pkg_env = IndexMap::new();
        pkg_env.insert(
            "PROJECT_ROOT".to_string(),
            ManifestEnvEntry {
                method: hpm_package::EnvMethod::Set,
                value: None,
                required: true,
            },
        );
        manifest.runtime = Some(pkg_env);

        let package_path = temp_dir.path().join("needs-config@1.0.0");
        std::fs::create_dir_all(&package_path).unwrap();

        let installed_package = InstalledPackage {
            version: "1.0.0".to_string(),
            manifest,
            install_path: package_path,
            is_dev: false,
        };

        // No project override: required placeholder must trigger MissingRequiredEnv.
        let err = project_manager
            .create_houdini_package(&installed_package)
            .unwrap_err();
        match err {
            ProjectError::MissingRequiredEnv { var, package } => {
                assert_eq!(var, "PROJECT_ROOT");
                assert_eq!(package, "needs-config");
            }
            other => panic!("Expected MissingRequiredEnv, got {:?}", other),
        }

        // Project override supplies a value: should succeed and emit it.
        let mut overrides = IndexMap::new();
        overrides.insert(
            "PROJECT_ROOT".to_string(),
            ManifestEnvEntry {
                method: hpm_package::EnvMethod::Set,
                value: Some("/work/project".into()),
                required: false,
            },
        );
        let pkg = project_manager
            .create_houdini_package_with_python(&installed_package, None, &Some(overrides))
            .unwrap();
        let entry = pkg
            .env
            .as_ref()
            .unwrap()
            .iter()
            .find(|m| m.contains_key("PROJECT_ROOT"))
            .unwrap();
        match entry.get("PROJECT_ROOT").unwrap() {
            hpm_package::HoudiniEnvValue::Detailed { value, method } => {
                assert_eq!(value, "/work/project");
                assert_eq!(method, "set");
            }
            _ => panic!("Expected Detailed env value"),
        }
    }

    /// Helper: locate a single [runtime]-emitted entry by key.
    fn find_env_entry<'a>(
        pkg: &'a hpm_package::HoudiniPackage,
        key: &str,
    ) -> Option<&'a hpm_package::HoudiniEnvValue> {
        pkg.env.as_ref()?.iter().find_map(|m| m.get(key))
    }

    /// Build a `[runtime]` entry with conditional variants gated on
    /// install_source. Mirrors the canonical HDK-plugin use case.
    fn dev_only_runtime_entry(value: &str) -> ManifestEnvEntry {
        use hpm_package::{EnvValueSpec, EnvValueVariant, WhenSelector};
        ManifestEnvEntry {
            method: hpm_package::EnvMethod::Prepend,
            value: Some(EnvValueSpec::Conditional(vec![EnvValueVariant {
                when: WhenSelector {
                    install_source: Some("dev".to_string()),
                    ..Default::default()
                },
                set: value.to_string(),
            }])),
            required: false,
        }
    }

    #[test]
    fn runtime_install_source_dev_gates_emission() {
        let temp_dir = TempDir::new().unwrap();
        let (config, storage_manager) = test_setup(temp_dir.path());
        let project_root = temp_dir.path().join("project");
        std::fs::create_dir_all(&project_root).unwrap();
        let project_manager = ProjectManager::new(project_root, storage_manager, config).unwrap();

        let mut manifest = hpm_package::PackageManifest::new(
            PackagePath::new("studio/hdk-plugin").unwrap(),
            "HDK Plugin".to_string(),
            "1.0.0".to_string(),
            None,
            None,
            None,
        );
        let mut runtime = IndexMap::new();
        runtime.insert(
            "HOUDINI_DSO_PATH".to_string(),
            dev_only_runtime_entry("$HPM_PACKAGE_ROOT/build/Release"),
        );
        manifest.runtime = Some(runtime);

        let package_path = temp_dir.path().join("hdk-plugin@1.0.0");
        std::fs::create_dir_all(&package_path).unwrap();

        // is_dev = false: the dev-only variant filters out, the entry has
        // no surviving branches, so HOUDINI_DSO_PATH is not emitted at all.
        let non_dev = InstalledPackage {
            version: "1.0.0".to_string(),
            manifest: manifest.clone(),
            install_path: package_path.clone(),
            is_dev: false,
        };
        let pkg = project_manager.create_houdini_package(&non_dev).unwrap();
        assert!(
            find_env_entry(&pkg, "HOUDINI_DSO_PATH").is_none(),
            "install_source = 'dev' must not be emitted for non-dev installs"
        );

        // is_dev = true: the dev variant fires, $HPM_PACKAGE_ROOT expands.
        let dev = InstalledPackage {
            version: "1.0.0".to_string(),
            manifest,
            install_path: package_path.clone(),
            is_dev: true,
        };
        let pkg = project_manager.create_houdini_package(&dev).unwrap();
        let entry = find_env_entry(&pkg, "HOUDINI_DSO_PATH")
            .expect("dev-gated variant must be emitted for dev installs");
        // Conditional values lower to DetailedConditional with one entry
        // keyed by the runtime expression ("true" for an install-source-only
        // gate, since install_source is stripped before compile_when).
        match entry {
            hpm_package::HoudiniEnvValue::DetailedConditional { method, value } => {
                assert_eq!(method, "prepend");
                assert_eq!(value.len(), 1);
                let v = value[0].values().next().unwrap();
                let expected = format!("{}/build/Release", package_path.display());
                assert_eq!(v, &expected);
            }
            other => panic!("expected DetailedConditional env value, got {other:?}"),
        }
    }

    #[test]
    fn project_override_wins_over_install_source_dev_variant() {
        let temp_dir = TempDir::new().unwrap();
        let (config, storage_manager) = test_setup(temp_dir.path());
        let project_root = temp_dir.path().join("project");
        std::fs::create_dir_all(&project_root).unwrap();
        let project_manager = ProjectManager::new(project_root, storage_manager, config).unwrap();

        let mut manifest = hpm_package::PackageManifest::new(
            PackagePath::new("studio/overridable").unwrap(),
            "Overridable".to_string(),
            "1.0.0".to_string(),
            None,
            None,
            None,
        );
        let mut runtime = IndexMap::new();
        runtime.insert(
            "HOUDINI_DSO_PATH".to_string(),
            dev_only_runtime_entry("$HPM_PACKAGE_ROOT/build/Release"),
        );
        manifest.runtime = Some(runtime);

        let package_path = temp_dir.path().join("overridable@1.0.0");
        std::fs::create_dir_all(&package_path).unwrap();

        let installed = InstalledPackage {
            version: "1.0.0".to_string(),
            manifest,
            install_path: package_path,
            is_dev: true,
        };

        let mut overrides = IndexMap::new();
        overrides.insert(
            "HOUDINI_DSO_PATH".to_string(),
            ManifestEnvEntry {
                method: hpm_package::EnvMethod::Prepend,
                value: Some("/opt/forced/dso".into()),
                required: false,
            },
        );
        let pkg = project_manager
            .create_houdini_package_with_python(&installed, None, &Some(overrides))
            .unwrap();
        let entry = find_env_entry(&pkg, "HOUDINI_DSO_PATH")
            .expect("dev-gated key must still emit when project overrides it");
        match entry {
            hpm_package::HoudiniEnvValue::Detailed { value, .. } => {
                assert_eq!(value, "/opt/forced/dso");
            }
            other => panic!("expected Detailed env value, got {other:?}"),
        }
    }

    #[test]
    fn matches_spec_name_handles_scoped_and_bare() {
        let manifest = hpm_package::PackageManifest::new(
            PackagePath::new("tumblehead/claudini2").unwrap(),
            "Claudini 2".to_string(),
            "0.4.0".to_string(),
            None,
            None,
            None,
        );
        let pkg = InstalledPackage {
            version: "0.4.0".to_string(),
            manifest,
            install_path: PathBuf::from("/tmp/claudini2@0.4.0"),
            is_dev: false,
        };

        assert!(ProjectManager::matches_spec_name(
            &pkg,
            "tumblehead/claudini2"
        ));
        assert!(ProjectManager::matches_spec_name(&pkg, "claudini2"));
        assert!(!ProjectManager::matches_spec_name(&pkg, "other/claudini2"));
        assert!(!ProjectManager::matches_spec_name(&pkg, "unrelated"));
    }

    /// Regression: when a project's hpm.toml lists a scoped dependency name
    /// (`creator/slug`), but the installed-packages cache stores the bare slug
    /// in `InstalledPackage.name`, the short-circuit must still fire. Otherwise
    /// every `sync_dependencies` re-fetches and re-installs every dep, and on
    /// Windows the remove-and-recopy can fail with os error 5 when another
    /// Houdini holds the package dir open.
    #[tokio::test]
    async fn install_one_dep_short_circuits_on_scoped_name() {
        let temp_dir = TempDir::new().unwrap();
        let (_config, storage_manager) = test_setup(temp_dir.path());
        let fetcher =
            ArchiveFetcher::new(temp_dir.path().join("cache"), temp_dir.path().join("fetch"))
                .unwrap();

        let manifest = hpm_package::PackageManifest::new(
            PackagePath::new("tumblehead/tumblepipe").unwrap(),
            "Tumblepipe".to_string(),
            "1.1.20".to_string(),
            None,
            None,
            None,
        );
        let installed = InstalledPackage {
            version: "1.1.20".to_string(),
            manifest,
            install_path: temp_dir.path().join("tumblepipe@1.1.20"),
            is_dev: false,
        };

        // registry_set: None — if the short-circuit misses, install_one_dep
        // would panic on `expect("registry set built when registry deps
        // present")`. Reaching that panic is exactly the bug.
        let spec = hpm_package::DependencySpec::Simple("1.1.20".to_string());
        let outcome = install_one_dep(
            &storage_manager,
            &fetcher,
            None,
            std::slice::from_ref(&installed),
            "tumblehead/tumblepipe",
            &spec,
        )
        .await
        .expect("scoped lookup must short-circuit on the bare-slug InstalledPackage");

        assert_eq!(outcome.package.slug(), "tumblepipe");
        assert_eq!(outcome.package.version, "1.1.20");
        // Short-circuited Simple/Registry: no fresh fetch -> no checksum / source.
        assert!(outcome.checksum.is_none());
        assert!(outcome.source.is_none());
    }

    /// Regression: a Houdini manifest left over from a previous sync (e.g. a
    /// dev override that has since been removed) must be swept when its slug
    /// no longer appears in the dependency set. Otherwise Houdini keeps
    /// loading the stale package on launch.
    #[test]
    fn sweep_stale_houdini_manifests_removes_orphaned_json() {
        let temp_dir = TempDir::new().unwrap();
        let (config, storage_manager) = test_setup(temp_dir.path());
        let project_root = temp_dir.path().join("project");
        std::fs::create_dir_all(&project_root).unwrap();
        let project_manager = ProjectManager::new(project_root, storage_manager, config).unwrap();

        // Simulate the prior sync's output: foo.json (current dep) and
        // stale.json (dep that left the set).
        let pkg_dir = &project_manager.project_config.packages_dir;
        let foo_json = pkg_dir.join("foo.json");
        let stale_json = pkg_dir.join("stale.json");
        let unrelated = pkg_dir.join("README.md");
        std::fs::write(&foo_json, b"{}").unwrap();
        std::fs::write(&stale_json, b"{}").unwrap();
        std::fs::write(&unrelated, b"hello").unwrap();

        let manifest = hpm_package::PackageManifest::new(
            PackagePath::new("creator/foo").unwrap(),
            "Foo".to_string(),
            "1.0.0".to_string(),
            None,
            None,
            None,
        );
        let installed = InstalledPackage {
            version: "1.0.0".to_string(),
            manifest,
            install_path: temp_dir.path().join("foo@1.0.0"),
            is_dev: false,
        };

        project_manager
            .sweep_stale_houdini_manifests(std::slice::from_ref(&installed))
            .unwrap();

        assert!(foo_json.exists(), "current dep manifest must be kept");
        assert!(!stale_json.exists(), "stale dep manifest must be swept");
        assert!(unrelated.exists(), "non-json files must be left alone");
    }

    /// An empty dependency set must still sweep prior `<slug>.json` files.
    /// This is the dev-override-removed-and-package-disappeared case: the
    /// project resolves zero deps, so nothing iterates the json-write loop,
    /// and only the sweep can clear the stale manifest.
    #[test]
    fn sweep_stale_houdini_manifests_empty_set_clears_all_json() {
        let temp_dir = TempDir::new().unwrap();
        let (config, storage_manager) = test_setup(temp_dir.path());
        let project_root = temp_dir.path().join("project");
        std::fs::create_dir_all(&project_root).unwrap();
        let project_manager = ProjectManager::new(project_root, storage_manager, config).unwrap();

        let pkg_dir = &project_manager.project_config.packages_dir;
        let dev_only = pkg_dir.join("dev-only.json");
        std::fs::write(&dev_only, b"{}").unwrap();

        project_manager.sweep_stale_houdini_manifests(&[]).unwrap();

        assert!(
            !dev_only.exists(),
            "stale manifest must be swept even when the dep set is empty"
        );
    }

    // ---- Property test for install_source filter --------------------------

    use proptest::prelude::*;

    proptest! {
        /// Safety contract: a `[runtime]` entry whose only variant is gated
        /// `install_source = "dev"` never reaches the Houdini manifest for a
        /// non-dev install. The is_dev=false output must match the output
        /// produced from a manifest where the entry is absent.
        #[test]
        fn prop_install_source_dev_inert_for_registry_install(
            value in prop_oneof![
                Just("$HPM_PACKAGE_ROOT/build/Release".to_string()),
                Just("$HPM_PACKAGE_ROOT/dso".to_string()),
                Just("/abs/static".to_string()),
            ],
            key in prop::sample::select(
                vec!["ALPHA_PATH", "BETA_PATH", "GAMMA_PATH"]
            ),
        ) {
            let temp_dir = TempDir::new().unwrap();
            let (config, storage_manager) = test_setup(temp_dir.path());
            let project_root = temp_dir.path().join("project");
            std::fs::create_dir_all(&project_root).unwrap();
            let pm = ProjectManager::new(project_root, storage_manager, config).unwrap();

            let mut with_dev = hpm_package::PackageManifest::new(
                PackagePath::new("studio/inertness").unwrap(),
                "Inertness".to_string(),
                "1.0.0".to_string(),
                None,
                None,
                None,
            );
            let mut runtime = IndexMap::new();
            runtime.insert(key.to_string(), dev_only_runtime_entry(&value));
            with_dev.runtime = Some(runtime);

            let mut without = with_dev.clone();
            without.runtime = None;

            let pkg_path = temp_dir.path().join("inertness@1.0.0");
            std::fs::create_dir_all(&pkg_path).unwrap();

            let a = pm.create_houdini_package(&InstalledPackage {
                version: "1.0.0".to_string(),
                manifest: with_dev,
                install_path: pkg_path.clone(),
                is_dev: false,
            });
            let b = pm.create_houdini_package(&InstalledPackage {
                version: "1.0.0".to_string(),
                manifest: without,
                install_path: pkg_path,
                is_dev: false,
            });

            // Compare via Debug — HoudiniPackage / HoudiniEnvValue don't
            // implement PartialEq, and a regression that fires the dev
            // branch for is_dev=false would diverge here even when both
            // sides succeed.
            prop_assert_eq!(format!("{:?}", a), format!("{:?}", b));
        }
    }
}
