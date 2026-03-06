use crate::archive_fetcher::ArchiveFetcher;
use crate::package_source::PackageSource;
use crate::storage::{InstalledPackage, PackageSpec, StorageManager};
use hpm_config::ProjectConfig;
use hpm_package::{HoudiniPackage, PackageManifest};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{debug, info};

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
        let config = hpm_config::Config::load().unwrap_or_default();
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
        if !self.project_config.manifest_file.exists() {
            return Ok(None);
        }

        let manifest_content = std::fs::read_to_string(&self.project_config.manifest_file)
            .map_err(|e| ProjectError::ManifestRead(e.to_string()))?;

        let manifest: PackageManifest = toml::from_str(&manifest_content)
            .map_err(|e| ProjectError::ManifestParse(e.to_string()))?;

        Ok(Some(manifest))
    }

    pub async fn add_dependency(&self, spec: &PackageSpec) -> Result<(), ProjectError> {
        info!("Adding dependency: {} {}", spec.name, spec.version_req);

        // 1. Install package to global storage if not already installed
        let installed_package = if !self
            .storage_manager
            .package_exists(&spec.name, spec.version_req.as_str())
        {
            self.storage_manager
                .install_package(spec)
                .await
                .map_err(|e| ProjectError::PackageInstallation(e.to_string()))?
        } else {
            // Find existing installed package
            let installed_packages = self
                .storage_manager
                .list_installed()
                .map_err(|e| ProjectError::StorageRead(e.to_string()))?;

            installed_packages
                .into_iter()
                .find(|p| p.name == spec.name && p.is_compatible_with(&spec.version_req))
                .ok_or_else(|| ProjectError::PackageNotFound(spec.name.clone()))?
        };

        // 2. Generate Houdini package manifest
        self.generate_houdini_manifest(&installed_package)?;

        // 3. Update project manifest and lock file
        self.update_project_manifest(spec)?;

        info!("Successfully added dependency: {}", spec.name);
        Ok(())
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

        if let Some(dependencies) = project_manifest.dependencies {
            // Build registry set once (lazily) for any registry-resolved deps
            let registry_set = {
                let has_registry_deps = dependencies.values().any(|spec| spec.is_registry());
                if has_registry_deps {
                    let config = hpm_config::Config::load().unwrap_or_default();
                    Some(crate::registry::RegistrySet::from_configs(
                        &config.registries,
                        &config.storage.registry_cache_dir,
                    ))
                } else {
                    None
                }
            };

            for (name, dep_spec) in dependencies {
                // Skip if already linked
                if self.is_dependency_linked(&name) {
                    continue;
                }

                // Build a PackageSource from the dependency spec and use fetcher for remote deps
                match &dep_spec {
                    hpm_package::DependencySpec::Simple(version)
                    | hpm_package::DependencySpec::Registry { version, .. } => {
                        let rs = registry_set.as_ref().expect("registry set built above");
                        let entry = rs.get_version(&name, version).await.map_err(|e| {
                            ProjectError::InvalidDependency(format!(
                                "Failed to resolve {}@{} from registry: {}",
                                name, version, e
                            ))
                        })?;
                        self.fetch_and_install(
                            &name,
                            version,
                            PackageSource::Url {
                                url: entry.dl,
                                version: version.clone(),
                            },
                        )
                        .await?;
                    }
                    hpm_package::DependencySpec::Url { url, version, .. } => {
                        self.fetch_and_install(
                            &name,
                            version,
                            PackageSource::Url {
                                url: url.clone(),
                                version: version.clone(),
                            },
                        )
                        .await?;
                    }
                    hpm_package::DependencySpec::Path { path, .. } => {
                        let source_path = std::path::Path::new(path);
                        let installed =
                            self.storage_manager
                                .install_from_path(source_path)
                                .await
                                .map_err(|e| ProjectError::PackageInstallation(e.to_string()))?;
                        self.generate_houdini_manifest(&installed)?;
                    }
                }
            }
        }

        info!("Successfully synced project dependencies");
        Ok(())
    }

    /// Fetch a remote package and install it to global storage.
    async fn fetch_and_install(
        &self,
        name: &str,
        version: &str,
        source: PackageSource,
    ) -> Result<(), ProjectError> {
        // Check if already in global storage
        if self.storage_manager.package_exists(name, version) {
            info!("Package {}@{} already installed", name, version);
            let installed_packages = self
                .storage_manager
                .list_installed()
                .map_err(|e| ProjectError::StorageRead(e.to_string()))?;
            if let Some(pkg) = installed_packages
                .into_iter()
                .find(|p| p.name == name && p.version == version)
            {
                self.generate_houdini_manifest(&pkg)?;
            }
            return Ok(());
        }

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

        // Generate Houdini package manifest
        self.generate_houdini_manifest(&installed)?;

        info!("Successfully fetched and installed {}@{}", name, version);
        Ok(())
    }

    fn generate_houdini_manifest(
        &self,
        installed_package: &InstalledPackage,
    ) -> Result<(), ProjectError> {
        let houdini_package = self.create_houdini_package(installed_package)?;
        let manifest_path = self
            .project_config
            .package_manifest_path(&installed_package.name);

        // Write directly to file using buffered writer to avoid intermediate string allocation
        let file = std::fs::File::create(&manifest_path)
            .map_err(|e| ProjectError::ManifestWrite(e.to_string()))?;
        let writer = std::io::BufWriter::new(file);

        serde_json::to_writer_pretty(writer, &houdini_package)
            .map_err(|e| ProjectError::JsonSerialization(e.to_string()))?;

        debug!("Generated Houdini manifest for {}", installed_package.name);
        Ok(())
    }

    fn create_houdini_package(
        &self,
        installed_package: &InstalledPackage,
    ) -> Result<HoudiniPackage, ProjectError> {
        let package_path = &installed_package.install_path;

        // Build hpath entries
        let mut hpath = vec![];
        if package_path.join("otls").exists() {
            hpath.push(package_path.join("otls").to_string_lossy().to_string());
        }

        // Build environment variables
        let mut env = vec![];

        // Python path
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
            std::fs::read_to_string(manifest_path)
                .map_err(|e| ProjectError::ManifestRead(e.to_string()))?
        } else {
            // Return error if no manifest exists - user should run `hpm init` first
            return Err(ProjectError::ManifestRead(
                "No hpm.toml found. Run 'hpm init' to create a package first.".to_string(),
            ));
        };

        // Parse as editable TOML document
        let mut doc: toml_edit::DocumentMut = content
            .parse()
            .map_err(|e: toml_edit::TomlError| ProjectError::ManifestParse(e.to_string()))?;

        // Ensure [dependencies] table exists
        if !doc.contains_key("dependencies") {
            doc["dependencies"] = toml_edit::Item::Table(toml_edit::Table::new());
        }

        // Add or update the dependency
        let deps_table = doc["dependencies"].as_table_mut().ok_or_else(|| {
            ProjectError::ManifestParse("[dependencies] is not a table".to_string())
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

        let content = std::fs::read_to_string(manifest_path)
            .map_err(|e| ProjectError::ManifestRead(e.to_string()))?;

        // Parse as editable TOML document
        let mut doc: toml_edit::DocumentMut = content
            .parse()
            .map_err(|e: toml_edit::TomlError| ProjectError::ManifestParse(e.to_string()))?;

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

    fn is_dependency_linked(&self, name: &str) -> bool {
        let manifest_path = self.project_config.package_manifest_path(name);
        manifest_path.exists()
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
            "test-package".to_string(),
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
            install_path: package_path,
            installed_at: std::time::SystemTime::now(),
        };

        let houdini_package = project_manager
            .create_houdini_package(&installed_package)
            .unwrap();
        assert!(houdini_package.hpath.is_some());
        assert!(houdini_package.env.is_some());
    }
}
