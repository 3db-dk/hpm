use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub registry: RegistryConfig,
    pub install: InstallConfig,
    pub storage: StorageConfig,
    pub projects: ProjectsConfig,
    pub auth: Option<AuthConfig>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RegistryConfig {
    pub default: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct InstallConfig {
    pub path: String,
    pub parallel_downloads: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AuthConfig {
    pub token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    pub home_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub packages_dir: PathBuf,
    pub registry_cache_dir: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectsConfig {
    pub explicit_paths: Vec<PathBuf>,
    pub search_roots: Vec<PathBuf>,
    pub max_search_depth: usize,
    pub ignore_patterns: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProjectConfig {
    pub packages_dir: PathBuf,
    pub lock_file: PathBuf,
    pub manifest_file: PathBuf,
}

impl Default for Config {
    fn default() -> Self {
        let home_dir = Self::default_home_dir();

        Self {
            registry: RegistryConfig {
                default: "https://packages.houdini.org".to_string(),
            },
            install: InstallConfig {
                path: "packages/hpm".to_string(),
                parallel_downloads: 8,
            },
            storage: StorageConfig {
                home_dir: home_dir.clone(),
                cache_dir: home_dir.join("cache"),
                packages_dir: home_dir.join("packages"),
                registry_cache_dir: home_dir.join("registry"),
            },
            projects: ProjectsConfig::default(),
            auth: None,
        }
    }
}

impl Config {
    pub fn default_home_dir() -> PathBuf {
        if let Some(home_dir) = home::home_dir() {
            home_dir.join(".hpm")
        } else {
            PathBuf::from(".hpm")
        }
    }

    pub fn load_project_config(project_root: &Path) -> ProjectConfig {
        let hpm_dir = project_root.join(".hpm");
        ProjectConfig {
            packages_dir: hpm_dir.join("packages"),
            lock_file: project_root.join("hpm.lock"),
            manifest_file: project_root.join("hpm.toml"),
        }
    }
}

impl StorageConfig {
    pub fn ensure_directories(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.home_dir)?;
        std::fs::create_dir_all(&self.cache_dir)?;
        std::fs::create_dir_all(&self.packages_dir)?;
        std::fs::create_dir_all(&self.registry_cache_dir)?;
        Ok(())
    }

    pub fn package_dir(&self, name: &str, version: &str) -> PathBuf {
        self.packages_dir.join(format!("{}@{}", name, version))
    }
}

impl Default for ProjectsConfig {
    fn default() -> Self {
        Self {
            explicit_paths: vec![],
            search_roots: vec![],
            max_search_depth: 3,
            ignore_patterns: vec![
                ".git".to_string(),
                ".hg".to_string(),
                ".svn".to_string(),
                "node_modules".to_string(),
                "backup".to_string(),
                "archive".to_string(),
                ".cache".to_string(),
                "temp".to_string(),
                "tmp".to_string(),
            ],
        }
    }
}

impl ProjectsConfig {
    pub fn add_explicit_path(&mut self, path: PathBuf) {
        if !self.explicit_paths.contains(&path) {
            self.explicit_paths.push(path);
        }
    }

    pub fn add_search_root(&mut self, path: PathBuf) {
        if !self.search_roots.contains(&path) {
            self.search_roots.push(path);
        }
    }

    pub fn should_ignore(&self, dir_name: &str) -> bool {
        self.ignore_patterns.iter().any(|pattern| {
            // Simple pattern matching - could be enhanced with glob patterns
            dir_name == pattern || dir_name.starts_with(pattern)
        })
    }
}

impl ProjectConfig {
    pub fn ensure_directories(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.packages_dir)?;
        Ok(())
    }

    pub fn package_manifest_path(&self, name: &str) -> PathBuf {
        self.packages_dir.join(format!("{}.json", name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_default_values() {
        let config = Config::default();
        assert_eq!(config.registry.default, "https://packages.houdini.org");
        assert_eq!(config.install.path, "packages/hpm");
        assert_eq!(config.install.parallel_downloads, 8);
        assert!(config.auth.is_none());
        assert!(config.storage.home_dir.ends_with(".hpm"));
        assert!(config.storage.packages_dir.ends_with("packages"));
        assert_eq!(config.projects.max_search_depth, 3);
        assert!(config.projects.explicit_paths.is_empty());
        assert!(config.projects.search_roots.is_empty());
    }

    #[test]
    fn config_serialization() {
        let config = Config::default();
        let serialized = serde_json::to_string(&config).unwrap();
        let deserialized: Config = serde_json::from_str(&serialized).unwrap();

        assert_eq!(config.registry.default, deserialized.registry.default);
        assert_eq!(config.install.path, deserialized.install.path);
        assert_eq!(config.storage.home_dir, deserialized.storage.home_dir);
        assert_eq!(
            config.projects.max_search_depth,
            deserialized.projects.max_search_depth
        );
    }

    #[test]
    fn projects_config_ignore_patterns() {
        let projects_config = ProjectsConfig::default();

        assert!(projects_config.should_ignore(".git"));
        assert!(projects_config.should_ignore("backup"));
        assert!(projects_config.should_ignore("node_modules"));
        assert!(!projects_config.should_ignore("my-project"));
        assert!(!projects_config.should_ignore("houdini-scenes"));
    }

    #[test]
    fn projects_config_path_management() {
        let mut projects_config = ProjectsConfig::default();
        let test_path = PathBuf::from("/test/project");
        let search_root = PathBuf::from("/test/projects");

        projects_config.add_explicit_path(test_path.clone());
        projects_config.add_search_root(search_root.clone());

        assert!(projects_config.explicit_paths.contains(&test_path));
        assert!(projects_config.search_roots.contains(&search_root));

        // Adding duplicate should not create duplicate entries
        projects_config.add_explicit_path(test_path.clone());
        assert_eq!(projects_config.explicit_paths.len(), 1);
    }

    #[test]
    fn storage_config_package_directory() {
        let config = Config::default();
        let pkg_dir = config.storage.package_dir("test-package", "1.0.0");
        assert!(pkg_dir
            .to_string_lossy()
            .ends_with("packages/test-package@1.0.0"));
    }

    #[test]
    fn project_config_package_manifest_path() {
        let project_root = PathBuf::from("/test/project");
        let project_config = Config::load_project_config(&project_root);
        let manifest_path = project_config.package_manifest_path("test-package");
        assert!(manifest_path
            .to_string_lossy()
            .ends_with(".hpm/packages/test-package.json"));
    }

    #[test]
    fn project_config_structure() {
        let project_root = PathBuf::from("/test/project");
        let project_config = Config::load_project_config(&project_root);

        assert!(project_config
            .packages_dir
            .to_string_lossy()
            .ends_with(".hpm/packages"));
        assert!(project_config
            .lock_file
            .to_string_lossy()
            .ends_with("hpm.lock"));
        assert!(project_config
            .manifest_file
            .to_string_lossy()
            .ends_with("hpm.toml"));
    }
}
