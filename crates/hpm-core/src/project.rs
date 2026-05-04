use crate::archive_fetcher::ArchiveFetcher;
use crate::package_source::PackageSource;
use crate::storage::{InstalledPackage, PackageSpec, StorageManager};
use hpm_config::ProjectConfig;
use hpm_package::{HoudiniPackage, ManifestEnvEntry, ManifestLoadError, PackageManifest};
use hpm_python::{VenvManager, collect_python_dependencies, resolve_dependencies};
use indexmap::IndexMap;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::{debug, info, warn};

#[derive(Debug)]
pub struct ProjectManager {
    project_config: ProjectConfig,
    storage_manager: Arc<StorageManager>,
    fetcher: Option<ArchiveFetcher>,
}

#[derive(Debug, Clone)]
pub struct ProjectDependency {
    pub name: String,
    pub version: String,
    pub installed_package: Option<InstalledPackage>,
}

impl ProjectManager {
    pub fn new(
        project_root: PathBuf,
        storage_manager: Arc<StorageManager>,
    ) -> Result<Self, ProjectError> {
        let project_config = hpm_config::Config::load_project_config(&project_root);

        // Create fetcher using HPM's cache and packages directories
        let config =
            hpm_config::Config::load().map_err(|e| ProjectError::ConfigLoad(e.to_string()))?;
        let cache_dir = config
            .storage
            .packages_dir
            .parent()
            .unwrap_or(std::path::Path::new("."))
            .join("cache");
        let fetch_packages_dir = config
            .storage
            .packages_dir
            .parent()
            .unwrap_or(std::path::Path::new("."))
            .join("fetch");
        let fetcher = ArchiveFetcher::new(cache_dir, fetch_packages_dir)
            .map_err(|e| ProjectError::DirectoryCreation(e.to_string()))?;

        let manager = Self {
            project_config,
            storage_manager,
            fetcher: Some(fetcher),
        };

        manager.ensure_directories()?;
        Ok(manager)
    }

    fn ensure_directories(&self) -> Result<(), ProjectError> {
        self.project_config
            .ensure_directories()
            .map_err(|e| ProjectError::DirectoryCreation(e.to_string()))?;
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

        let installed_packages = self
            .storage_manager
            .list_installed()
            .map_err(|e| ProjectError::StorageRead(e.to_string()))?;

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
    /// (`creator/slug`) and bare (`slug`) names. The package's canonical
    /// identifier is `manifest.package.path`; `InstalledPackage.name` only
    /// holds the slug.
    fn matches_spec_name(pkg: &InstalledPackage, spec_name: &str) -> bool {
        pkg.manifest.package.path == spec_name || pkg.name == spec_name
    }

    /// Resolve a package spec against configured registries and install it.
    async fn resolve_and_install_from_registry(
        &self,
        spec: &PackageSpec,
    ) -> Result<InstalledPackage, ProjectError> {
        let config =
            hpm_config::Config::load().map_err(|e| ProjectError::ConfigLoad(e.to_string()))?;
        let registry_set = crate::registry::RegistrySet::from_configs(
            &config.registries,
            &config.storage.registry_cache_dir,
        );

        if registry_set.is_empty() {
            return Err(ProjectError::PackageInstallation(format!(
                "Cannot install {} {}: no registries configured",
                spec.name, spec.version_req
            )));
        }

        let entry = self.resolve_registry_entry(&registry_set, spec).await?;
        self.fetch_and_install_pkg(
            &spec.name,
            &entry.version.clone(),
            PackageSource::Url {
                url: entry.dl,
                version: entry.version,
            },
        )
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
                .map_err(|e| {
                    ProjectError::PackageInstallation(format!(
                        "Failed to resolve {}@{} from registry: {}",
                        spec.name, req_str, e
                    ))
                });
        }

        let mut versions = registry_set.get_versions(&spec.name).await.map_err(|e| {
            ProjectError::PackageInstallation(format!(
                "Failed to list versions for {}: {}",
                spec.name, e
            ))
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

        versions.into_iter().next().ok_or_else(|| {
            ProjectError::PackageInstallation(format!(
                "No version of {} matches requirement {}",
                spec.name, req_str
            ))
        })
    }

    pub async fn remove_dependency(&self, name: &str) -> Result<(), ProjectError> {
        info!("Removing dependency: {}", name);

        // 1. Remove from project manifest (hpm.toml)
        self.remove_from_project_manifest(name)?;

        // 2. Remove Houdini package manifest from project
        let manifest_path = self.project_config.package_manifest_path(name);
        if manifest_path.exists() {
            std::fs::remove_file(&manifest_path)
                .map_err(|e| ProjectError::ManifestRemoval(e.to_string()))?;
            debug!("Removed Houdini manifest: {:?}", manifest_path);
        }

        info!("Successfully removed dependency: {}", name);
        Ok(())
    }

    pub async fn sync_dependencies(&self) -> Result<(), ProjectError> {
        info!("Syncing project dependencies");

        let project_manifest = match self.load_project_manifest()? {
            Some(manifest) => manifest,
            None => {
                info!("No project manifest found, nothing to sync");
                return Ok(());
            }
        };

        let mut installed_packages: Vec<InstalledPackage> = Vec::new();

        let project_env_overrides = project_manifest.env;
        let manifest_registries = project_manifest.registries;
        if let Some(dependencies) = project_manifest.dependencies {
            // Build registry set once (lazily) for any registry-resolved deps
            let registry_set = {
                let has_registry_deps = dependencies.values().any(|spec| spec.is_registry());
                if has_registry_deps {
                    let config = hpm_config::Config::load()
                        .map_err(|e| ProjectError::ConfigLoad(e.to_string()))?;
                    let registry_configs: Vec<hpm_config::RegistrySourceConfig> =
                        if let Some(ref regs) = manifest_registries {
                            regs.iter()
                                .map(|r| hpm_config::RegistrySourceConfig {
                                    name: r.name.clone(),
                                    url: r.url.clone(),
                                    registry_type: match r.registry_type {
                                        hpm_package::RegistryType::Api => {
                                            hpm_config::RegistryType::Api
                                        }
                                        hpm_package::RegistryType::Git => {
                                            hpm_config::RegistryType::Git
                                        }
                                    },
                                })
                                .collect()
                        } else {
                            config.registries.clone()
                        };
                    Some(crate::registry::RegistrySet::from_configs(
                        &registry_configs,
                        &config.storage.registry_cache_dir,
                    ))
                } else {
                    None
                }
            };

            // Fetch list of globally installed packages once for lookups
            let all_installed = self
                .storage_manager
                .list_installed()
                .map_err(|e| ProjectError::StorageRead(e.to_string()))?;

            for (name, dep_spec) in dependencies {
                // Build a PackageSource from the dependency spec and use fetcher for remote deps
                match &dep_spec {
                    hpm_package::DependencySpec::Simple(version)
                    | hpm_package::DependencySpec::Registry { version, .. } => {
                        let pkg = self
                            .ensure_installed(&name, version, &registry_set, &all_installed)
                            .await?;
                        installed_packages.push(pkg);
                    }
                    hpm_package::DependencySpec::Url { url, version, .. } => {
                        let pkg = self
                            .ensure_installed_from_url(&name, version, url, &all_installed)
                            .await?;
                        installed_packages.push(pkg);
                    }
                    hpm_package::DependencySpec::Path { path, .. } => {
                        // Path deps install into the dev-only subtree so they
                        // don't poison the shared registry CAS — see
                        // `install_from_path_dev`.
                        let source_path = std::path::Path::new(path);
                        let installed = self
                            .storage_manager
                            .install_from_path_dev(source_path)
                            .await
                            .map_err(|e| ProjectError::PackageInstallation(e.to_string()))?;
                        installed_packages.push(installed);
                    }
                }
            }
        }

        // Resolve Python pip dependencies and get venv site-packages path (if any)
        let venv_site_packages = self.resolve_python_deps(&installed_packages).await?;

        // Generate Houdini JSON manifests for all packages
        for pkg in &installed_packages {
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
        self.sweep_stale_houdini_manifests(&installed_packages)?;

        info!("Successfully synced project dependencies");
        Ok(())
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
            .map(|pkg| pkg.name.as_str())
            .collect();

        let entries = std::fs::read_dir(packages_dir)
            .map_err(|e| ProjectError::DirectoryRead(e.to_string()))?;

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

    /// Ensure a registry/simple dependency is installed, returning the InstalledPackage.
    async fn ensure_installed(
        &self,
        name: &str,
        version: &str,
        registry_set: &Option<crate::registry::RegistrySet>,
        all_installed: &[InstalledPackage],
    ) -> Result<InstalledPackage, ProjectError> {
        // Check if already in global storage. The dependency `name` from a
        // project manifest is typically scoped (`creator/slug`); the CAS keys
        // installed packages by bare slug. `matches_spec_name` bridges both
        // forms — comparing `p.name == name` directly here would miss every
        // scoped match and trigger a redundant fetch + remove-and-reinstall.
        if let Some(pkg) = all_installed
            .iter()
            .find(|p| Self::matches_spec_name(p, name) && p.version == version)
        {
            info!("Package {}@{} already installed", name, version);
            return Ok(pkg.clone());
        }

        let rs = registry_set.as_ref().expect("registry set built above");
        let entry = rs.get_version(name, version).await.map_err(|e| {
            ProjectError::InvalidDependency(format!(
                "Failed to resolve {}@{} from registry: {}",
                name, version, e
            ))
        })?;
        self.fetch_and_install_pkg(
            name,
            version,
            PackageSource::Url {
                url: entry.dl,
                version: version.to_string(),
            },
        )
        .await
    }

    /// Ensure a URL dependency is installed, returning the InstalledPackage.
    async fn ensure_installed_from_url(
        &self,
        name: &str,
        version: &str,
        url: &str,
        all_installed: &[InstalledPackage],
    ) -> Result<InstalledPackage, ProjectError> {
        if let Some(pkg) = all_installed
            .iter()
            .find(|p| Self::matches_spec_name(p, name) && p.version == version)
        {
            info!("Package {}@{} already installed", name, version);
            return Ok(pkg.clone());
        }

        self.fetch_and_install_pkg(
            name,
            version,
            PackageSource::Url {
                url: url.to_string(),
                version: version.to_string(),
            },
        )
        .await
    }

    /// Fetch a remote package and install it to global storage, returning the InstalledPackage.
    async fn fetch_and_install_pkg(
        &self,
        name: &str,
        version: &str,
        source: PackageSource,
    ) -> Result<InstalledPackage, ProjectError> {
        let fetcher = self
            .fetcher
            .as_ref()
            .ok_or_else(|| ProjectError::PackageInstallation("No fetcher available".to_string()))?;

        // Fetch (download + extract) the package
        let fetch_result = fetcher
            .fetch(&source, name)
            .await
            .map_err(|e| ProjectError::PackageInstallation(e.to_string()))?;

        // Install from the extracted path into global storage
        let installed = self
            .storage_manager
            .install_from_path(&fetch_result.package_path)
            .await
            .map_err(|e| ProjectError::PackageInstallation(e.to_string()))?;

        info!("Successfully fetched and installed {}@{}", name, version);
        Ok(installed)
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
            .package_manifest_path(&installed_package.name);

        // Atomic write: stage to <path>.tmp then rename. Houdini reads
        // these manifests on launch; a crash or interrupt mid-write leaves
        // a truncated JSON that Houdini chokes on and blocks the project.
        let mut tmp_path = manifest_path.as_os_str().to_os_string();
        tmp_path.push(".tmp");
        let tmp_path = PathBuf::from(tmp_path);

        {
            let file = std::fs::File::create(&tmp_path)
                .map_err(|e| ProjectError::ManifestWrite(e.to_string()))?;
            let mut writer = std::io::BufWriter::new(file);
            serde_json::to_writer_pretty(&mut writer, &houdini_package)
                .map_err(|e| ProjectError::JsonSerialization(e.to_string()))?;
            use std::io::Write;
            writer
                .flush()
                .map_err(|e| ProjectError::ManifestWrite(e.to_string()))?;
        }

        std::fs::rename(&tmp_path, &manifest_path)
            .map_err(|e| ProjectError::ManifestWrite(e.to_string()))?;

        debug!("Generated Houdini manifest for {}", installed_package.name);
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
            .map_err(|e| ProjectError::PythonResolution(format!("{:#}", e)))?;

        // Collect python dependencies from all package manifests
        let manifests: Vec<PackageManifest> = installed_packages
            .iter()
            .map(|p| p.manifest.clone())
            .collect();
        let collected = collect_python_dependencies(&manifests)
            .await
            .map_err(|e| ProjectError::PythonResolution(format!("{:#}", e)))?;

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
            .map_err(|e| ProjectError::PythonResolution(format!("{:#}", e)))?;

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
            .map_err(|e| ProjectError::PythonResolution(format!("{:#}", e)))?;

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

        // Append user-defined env vars from [env] section, applying project-level overrides.
        // A package entry with `required = true` and no value is a placeholder
        // that the project's [env] must override; otherwise we hard-error so
        // the package is not silently launched without a value it depends on.
        if let Some(user_env) = &installed_package.manifest.env {
            for (key, entry) in user_env {
                let override_entry = project_env_overrides
                    .as_ref()
                    .and_then(|overrides| overrides.get(key));
                let effective_entry = override_entry.unwrap_or(entry);

                let raw_value = effective_entry.value.as_ref().ok_or_else(|| {
                    ProjectError::MissingRequiredEnv {
                        var: key.clone(),
                        package: installed_package.name.clone(),
                    }
                })?;

                let resolved_value =
                    raw_value.replace("$HPM_PACKAGE_ROOT", &package_path.to_string_lossy());
                let mut env_map = HashMap::new();
                env_map.insert(
                    key.clone(),
                    hpm_package::HoudiniEnvValue::Detailed {
                        method: match effective_entry.method {
                            hpm_package::EnvMethod::Set => "set",
                            hpm_package::EnvMethod::Prepend => "prepend",
                            hpm_package::EnvMethod::Append => "append",
                        }
                        .to_string(),
                        value: resolved_value,
                    },
                );
                env.push(env_map);
            }
        }

        // Generate enable condition from Houdini config
        let enable = if let Some(houdini_config) = &installed_package.manifest.houdini {
            let mut conditions = vec![];

            if let Some(min_version) = &houdini_config.min_version {
                conditions.push(format!("houdini_version >= '{}'", min_version));
            }

            if let Some(max_version) = &houdini_config.max_version {
                conditions.push(format!("houdini_version <= '{}'", max_version));
            }

            if conditions.is_empty() {
                None
            } else {
                Some(conditions.join(" && "))
            }
        } else {
            None
        };

        Ok(HoudiniPackage {
            hpath: if hpath.is_empty() { None } else { Some(hpath) },
            env: if env.is_empty() { None } else { Some(env) },
            enable,
            requires: None,
            recommends: None,
        })
    }

    fn update_project_manifest(&self, spec: &PackageSpec) -> Result<(), ProjectError> {
        // Read existing manifest or create new one
        let manifest_path = &self.project_config.manifest_file;

        let content = if manifest_path.exists() {
            std::fs::read_to_string(manifest_path).map_err(|e| {
                ProjectError::ManifestRead(format!("{}: {}", manifest_path.display(), e))
            })?
        } else {
            // Return error if no manifest exists - user should run `hpm init` first
            return Err(ProjectError::ManifestRead(format!(
                "No hpm.toml found at {}. Run 'hpm init' to create a package first.",
                manifest_path.display()
            )));
        };

        // Parse as editable TOML document
        let mut doc: toml_edit::DocumentMut =
            content.parse().map_err(|e: toml_edit::TomlError| {
                ProjectError::ManifestParse(format!("{}: {}", manifest_path.display(), e))
            })?;

        // Ensure [dependencies] table exists
        if !doc.contains_key("dependencies") {
            doc["dependencies"] = toml_edit::Item::Table(toml_edit::Table::new());
        }

        // Add or update the dependency
        let deps_table = doc["dependencies"].as_table_mut().ok_or_else(|| {
            ProjectError::ManifestParse(format!(
                "{}: [dependencies] is not a table",
                manifest_path.display()
            ))
        })?;

        let version_str = spec.version_req.as_str();

        // Use simple string format for simple version specs, inline table for complex ones
        if version_str == "*"
            || version_str.starts_with('^')
            || version_str.starts_with('~')
            || version_str.starts_with('>')
            || version_str.starts_with('<')
            || version_str
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_digit())
        {
            deps_table[&spec.name] = toml_edit::value(version_str);
        } else {
            // For complex specs, use inline table
            let mut inline = toml_edit::InlineTable::new();
            inline.insert("version", version_str.into());
            deps_table[&spec.name] = toml_edit::Item::Value(toml_edit::Value::InlineTable(inline));
        }

        // Write back to file
        std::fs::write(manifest_path, doc.to_string())
            .map_err(|e| ProjectError::ManifestWrite(e.to_string()))?;

        info!(
            "Updated hpm.toml with dependency: {} = \"{}\"",
            spec.name, version_str
        );
        Ok(())
    }

    fn remove_from_project_manifest(&self, name: &str) -> Result<(), ProjectError> {
        let manifest_path = &self.project_config.manifest_file;

        if !manifest_path.exists() {
            return Ok(()); // Nothing to remove
        }

        let content = std::fs::read_to_string(manifest_path).map_err(|e| {
            ProjectError::ManifestRead(format!("{}: {}", manifest_path.display(), e))
        })?;

        // Parse as editable TOML document
        let mut doc: toml_edit::DocumentMut =
            content.parse().map_err(|e: toml_edit::TomlError| {
                ProjectError::ManifestParse(format!("{}: {}", manifest_path.display(), e))
            })?;

        // Remove from [dependencies] if it exists
        if let Some(deps) = doc.get_mut("dependencies") {
            if let Some(table) = deps.as_table_mut() {
                if table.contains_key(name) {
                    table.remove(name);
                    info!("Removed {} from [dependencies]", name);
                }
            }
        }

        // Also check [dev-dependencies]
        if let Some(deps) = doc.get_mut("dev-dependencies") {
            if let Some(table) = deps.as_table_mut() {
                if table.contains_key(name) {
                    table.remove(name);
                    info!("Removed {} from [dev-dependencies]", name);
                }
            }
        }

        // Write back to file
        std::fs::write(manifest_path, doc.to_string())
            .map_err(|e| ProjectError::ManifestWrite(e.to_string()))?;

        Ok(())
    }

    pub fn list_dependencies(&self) -> Result<Vec<ProjectDependency>, ProjectError> {
        let mut dependencies = vec![];

        if !self.project_config.packages_dir.exists() {
            return Ok(dependencies);
        }

        let entries = std::fs::read_dir(&self.project_config.packages_dir)
            .map_err(|e| ProjectError::DirectoryRead(e.to_string()))?;

        let installed_packages = self
            .storage_manager
            .list_installed()
            .map_err(|e| ProjectError::StorageRead(e.to_string()))?;

        for entry in entries.flatten() {
            if let Some(file_name) = entry.path().file_name() {
                if let Some(name_str) = file_name.to_str() {
                    if name_str.ends_with(".json") {
                        let package_name = name_str.trim_end_matches(".json");

                        // Find corresponding installed package
                        let installed_package = installed_packages
                            .iter()
                            .find(|p| p.name == package_name)
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

#[derive(Debug, thiserror::Error)]
pub enum ProjectError {
    #[error("Directory creation failed: {0}")]
    DirectoryCreation(String),

    #[error("Directory read failed: {0}")]
    DirectoryRead(String),

    #[error(transparent)]
    Manifest(#[from] ManifestLoadError),

    // Used by the toml_edit-based edit paths (`update_project_manifest`,
    // `remove_from_project_manifest`) — those work with `toml_edit::DocumentMut`
    // and can't share `ManifestLoadError`'s `toml::de::Error` variant. The
    // string already includes the manifest path at each call site.
    #[error("Manifest read failed: {0}")]
    ManifestRead(String),

    #[error("Manifest parse failed: {0}")]
    ManifestParse(String),

    #[error("Manifest write failed: {0}")]
    ManifestWrite(String),

    #[error("Manifest removal failed: {0}")]
    ManifestRemoval(String),

    #[error("JSON serialization failed: {0}")]
    JsonSerialization(String),

    #[error("Package installation failed: {0}")]
    PackageInstallation(String),

    #[error("Package not found: {0}")]
    PackageNotFound(String),

    #[error("Storage read failed: {0}")]
    StorageRead(String),

    #[error("Invalid dependency specification: {0}")]
    InvalidDependency(String),

    #[error("Python dependency resolution failed: {0}")]
    PythonResolution(String),

    #[error("Failed to load HPM configuration: {0}")]
    ConfigLoad(String),

    #[error(
        "Required env var '{var}' for package '{package}' has no value. \
         Set it in this project's [env] section in hpm.toml."
    )]
    MissingRequiredEnv { var: String, package: String },
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn project_manager_creation() {
        let temp_dir = TempDir::new().unwrap();
        let storage_config = hpm_config::StorageConfig {
            home_dir: temp_dir.path().join(".hpm"),
            cache_dir: temp_dir.path().join(".hpm").join("cache"),
            packages_dir: temp_dir.path().join(".hpm").join("packages"),
            registry_cache_dir: temp_dir.path().join(".hpm").join("registry"),
        };

        let storage_manager = Arc::new(StorageManager::new(storage_config).unwrap());
        let project_root = temp_dir.path().join("project");
        std::fs::create_dir_all(&project_root).unwrap();

        let _project_manager = ProjectManager::new(project_root.clone(), storage_manager).unwrap();
        assert!(project_root.join(".hpm").join("packages").exists());
    }

    #[test]
    fn list_dependencies_empty_project() {
        let temp_dir = TempDir::new().unwrap();
        let storage_config = hpm_config::StorageConfig {
            home_dir: temp_dir.path().join(".hpm"),
            cache_dir: temp_dir.path().join(".hpm").join("cache"),
            packages_dir: temp_dir.path().join(".hpm").join("packages"),
            registry_cache_dir: temp_dir.path().join(".hpm").join("registry"),
        };

        let storage_manager = Arc::new(StorageManager::new(storage_config).unwrap());
        let project_root = temp_dir.path().join("project");
        std::fs::create_dir_all(&project_root).unwrap();

        let project_manager = ProjectManager::new(project_root, storage_manager).unwrap();
        let deps = project_manager.list_dependencies().unwrap();
        assert_eq!(deps.len(), 0);
    }

    #[test]
    fn create_houdini_package_basic() {
        let temp_dir = TempDir::new().unwrap();
        let storage_config = hpm_config::StorageConfig {
            home_dir: temp_dir.path().join(".hpm"),
            cache_dir: temp_dir.path().join(".hpm").join("cache"),
            packages_dir: temp_dir.path().join(".hpm").join("packages"),
            registry_cache_dir: temp_dir.path().join(".hpm").join("registry"),
        };

        let storage_manager = Arc::new(StorageManager::new(storage_config).unwrap());
        let project_root = temp_dir.path().join("project");
        std::fs::create_dir_all(&project_root).unwrap();

        let project_manager = ProjectManager::new(project_root, storage_manager).unwrap();

        let manifest = hpm_package::PackageManifest::new(
            "studio/test-package".to_string(),
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
            name: "test-package".to_string(),
            version: "1.0.0".to_string(),
            manifest,
            install_path: package_path.clone(),
            installed_at: std::time::SystemTime::now(),
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
        let storage_config = hpm_config::StorageConfig {
            home_dir: temp_dir.path().join(".hpm"),
            cache_dir: temp_dir.path().join(".hpm").join("cache"),
            packages_dir: temp_dir.path().join(".hpm").join("packages"),
            registry_cache_dir: temp_dir.path().join(".hpm").join("registry"),
        };

        let storage_manager = Arc::new(StorageManager::new(storage_config).unwrap());
        let project_root = temp_dir.path().join("project");
        std::fs::create_dir_all(&project_root).unwrap();

        let project_manager = ProjectManager::new(project_root, storage_manager).unwrap();

        // Create a manifest with an env var
        let mut manifest = hpm_package::PackageManifest::new(
            "studio/test-package".to_string(),
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
                value: Some("$HPM_PACKAGE_ROOT/default-config".to_string()),
                required: false,
            },
        );
        manifest.env = Some(pkg_env);

        let package_path = temp_dir.path().join("test-package@1.0.0");
        std::fs::create_dir_all(&package_path).unwrap();

        let installed_package = InstalledPackage {
            name: "test-package".to_string(),
            version: "1.0.0".to_string(),
            manifest,
            install_path: package_path.clone(),
            installed_at: std::time::SystemTime::now(),
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
                value: Some("/custom/config/path".to_string()),
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
        let storage_config = hpm_config::StorageConfig {
            home_dir: temp_dir.path().join(".hpm"),
            cache_dir: temp_dir.path().join(".hpm").join("cache"),
            packages_dir: temp_dir.path().join(".hpm").join("packages"),
            registry_cache_dir: temp_dir.path().join(".hpm").join("registry"),
        };
        let storage_manager = Arc::new(StorageManager::new(storage_config).unwrap());
        let project_root = temp_dir.path().join("project");
        std::fs::create_dir_all(&project_root).unwrap();
        let project_manager = ProjectManager::new(project_root, storage_manager).unwrap();

        let mut manifest = hpm_package::PackageManifest::new(
            "studio/needs-config".to_string(),
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
        manifest.env = Some(pkg_env);

        let package_path = temp_dir.path().join("needs-config@1.0.0");
        std::fs::create_dir_all(&package_path).unwrap();

        let installed_package = InstalledPackage {
            name: "needs-config".to_string(),
            version: "1.0.0".to_string(),
            manifest,
            install_path: package_path,
            installed_at: std::time::SystemTime::now(),
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
                value: Some("/work/project".to_string()),
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

    #[test]
    fn matches_spec_name_handles_scoped_and_bare() {
        let manifest = hpm_package::PackageManifest::new(
            "tumblehead/claudini2".to_string(),
            "Claudini 2".to_string(),
            "0.4.0".to_string(),
            None,
            None,
            None,
        );
        let pkg = InstalledPackage {
            name: "claudini2".to_string(),
            version: "0.4.0".to_string(),
            manifest,
            install_path: PathBuf::from("/tmp/claudini2@0.4.0"),
            installed_at: std::time::SystemTime::now(),
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
    async fn ensure_installed_short_circuits_on_scoped_name() {
        let temp_dir = TempDir::new().unwrap();
        let storage_config = hpm_config::StorageConfig {
            home_dir: temp_dir.path().join(".hpm"),
            cache_dir: temp_dir.path().join(".hpm").join("cache"),
            packages_dir: temp_dir.path().join(".hpm").join("packages"),
            registry_cache_dir: temp_dir.path().join(".hpm").join("registry"),
        };
        let storage_manager = Arc::new(StorageManager::new(storage_config).unwrap());
        let project_root = temp_dir.path().join("project");
        std::fs::create_dir_all(&project_root).unwrap();
        let project_manager = ProjectManager::new(project_root, storage_manager).unwrap();

        let manifest = hpm_package::PackageManifest::new(
            "tumblehead/tumblepipe".to_string(),
            "Tumblepipe".to_string(),
            "1.1.20".to_string(),
            None,
            None,
            None,
        );
        let installed = InstalledPackage {
            name: "tumblepipe".to_string(),
            version: "1.1.20".to_string(),
            manifest,
            install_path: temp_dir.path().join("tumblepipe@1.1.20"),
            installed_at: std::time::SystemTime::now(),
        };

        // registry_set: None — if the short-circuit misses, the function would
        // panic on `expect("registry set built above")`. Reaching that panic
        // is exactly the bug.
        let result = project_manager
            .ensure_installed(
                "tumblehead/tumblepipe",
                "1.1.20",
                &None,
                std::slice::from_ref(&installed),
            )
            .await
            .expect("scoped lookup must short-circuit on the bare-slug InstalledPackage");

        assert_eq!(result.name, "tumblepipe");
        assert_eq!(result.version, "1.1.20");
    }

    /// Regression: a Houdini manifest left over from a previous sync (e.g. a
    /// dev override that has since been removed) must be swept when its slug
    /// no longer appears in the dependency set. Otherwise Houdini keeps
    /// loading the stale package on launch.
    #[test]
    fn sweep_stale_houdini_manifests_removes_orphaned_json() {
        let temp_dir = TempDir::new().unwrap();
        let storage_config = hpm_config::StorageConfig {
            home_dir: temp_dir.path().join(".hpm"),
            cache_dir: temp_dir.path().join(".hpm").join("cache"),
            packages_dir: temp_dir.path().join(".hpm").join("packages"),
            registry_cache_dir: temp_dir.path().join(".hpm").join("registry"),
        };
        let storage_manager = Arc::new(StorageManager::new(storage_config).unwrap());
        let project_root = temp_dir.path().join("project");
        std::fs::create_dir_all(&project_root).unwrap();
        let project_manager = ProjectManager::new(project_root, storage_manager).unwrap();

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
            "creator/foo".to_string(),
            "Foo".to_string(),
            "1.0.0".to_string(),
            None,
            None,
            None,
        );
        let installed = InstalledPackage {
            name: "foo".to_string(),
            version: "1.0.0".to_string(),
            manifest,
            install_path: temp_dir.path().join("foo@1.0.0"),
            installed_at: std::time::SystemTime::now(),
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
        let storage_config = hpm_config::StorageConfig {
            home_dir: temp_dir.path().join(".hpm"),
            cache_dir: temp_dir.path().join(".hpm").join("cache"),
            packages_dir: temp_dir.path().join(".hpm").join("packages"),
            registry_cache_dir: temp_dir.path().join(".hpm").join("registry"),
        };
        let storage_manager = Arc::new(StorageManager::new(storage_config).unwrap());
        let project_root = temp_dir.path().join("project");
        std::fs::create_dir_all(&project_root).unwrap();
        let project_manager = ProjectManager::new(project_root, storage_manager).unwrap();

        let pkg_dir = &project_manager.project_config.packages_dir;
        let dev_only = pkg_dir.join("dev-only.json");
        std::fs::write(&dev_only, b"{}").unwrap();

        project_manager.sweep_stale_houdini_manifests(&[]).unwrap();

        assert!(
            !dev_only.exists(),
            "stale manifest must be swept even when the dep set is empty"
        );
    }
}
