//! HPM Configuration Management
//!
//! Layered configuration for HPM: built-in defaults are overridden by the user
//! config at `~/.hpm/config.toml`, which is in turn overridden by the project
//! config at `<cwd>/.hpm/config.toml`. The merge happens in [`Config::load`].
//!
//! Each section lives in its own module:
//!
//! - [`install`] — where packages land inside a project, download parallelism
//! - [`storage`] — global on-disk layout (`~/.hpm/packages/`, caches)
//! - [`projects`] — project-discovery roots and ignore patterns
//! - [`project`] — per-project derived paths (`hpm.lock`, `hpm.toml`)
//! - [`registry`] — registry source list
//! - [`signing`] — package-signing key path
//! - [`builder`] — programmatic [`Config`] construction
//! - [`error`] — [`ConfigError`]
//!
//! ## Basic usage
//!
//! ```rust
//! use hpm_config::Config;
//!
//! let config = Config::default();
//! println!("Storage directory: {:?}", config.storage.home_dir);
//! ```
//!
//! ## Custom project discovery
//!
//! ```rust
//! use hpm_config::Config;
//!
//! let mut config = Config::default();
//! config.projects.add_search_root("/Users/artist/houdini-projects".into());
//! config.projects.add_explicit_path("/shared/studio-packages".into());
//! config.projects.max_search_depth = 4;
//! ```

pub mod builder;
pub mod error;
pub mod install;
pub mod project_paths;
pub mod projects;
pub mod registry;
pub mod signing;
pub mod storage;

pub use builder::ConfigBuilder;
pub use error::ConfigError;
pub use install::InstallConfig;
pub use project_paths::ProjectPaths;
pub use projects::ProjectsConfig;
pub use registry::{RegistrySourceConfig, RegistryType};
pub use signing::SigningConfig;
pub use storage::StorageConfig;

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tracing::{debug, info, warn};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub install: InstallConfig,
    #[serde(default)]
    pub storage: StorageConfig,
    #[serde(default)]
    pub projects: ProjectsConfig,
    #[serde(default)]
    pub registries: Vec<RegistrySourceConfig>,
    #[serde(default)]
    pub signing: SigningConfig,
}

/// Locate the user's home directory.
///
/// Avoids the `dirs` / `home` crates to keep the supply-chain surface small.
pub(crate) fn user_home() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var_os("USERPROFILE").map(PathBuf::from)
    }
    #[cfg(not(windows))]
    {
        std::env::var_os("HOME").map(PathBuf::from)
    }
}

impl Config {
    pub fn default_home_dir() -> PathBuf {
        storage::default_home_dir()
    }

    /// Load configuration from the standard locations.
    ///
    /// This loads configuration in the following order (later sources override earlier):
    /// 1. Built-in defaults
    /// 2. User config: `~/.hpm/config.toml`
    /// 3. Project config: `.hpm/config.toml` in current directory (if it exists)
    ///
    /// If no config files exist, returns the default configuration.
    pub fn load() -> Result<Self, ConfigError> {
        let mut config = Self::default();

        // Load user config from ~/.hpm/config.toml.
        // A malformed user config must not lock the user out: every CLI
        // command calls `load()`, so propagating a parse error here leaves
        // no way to run `hpm config ...` to repair it. Fall back to defaults
        // and warn instead. Project configs below are user-authored and
        // project-scoped, so those still fail hard.
        let user_config_path = Self::default_home_dir().join("config.toml");
        if user_config_path.exists() {
            debug!("Loading user config from {:?}", user_config_path);
            match Self::load_from_path(&user_config_path) {
                Ok(user_config) => {
                    config.merge(user_config);
                    debug!("Loaded user configuration from {:?}", user_config_path);
                }
                Err(e) => {
                    warn!(
                        "Ignoring malformed user config at {:?}: {}. Using defaults.",
                        user_config_path, e
                    );
                }
            }
        }

        // Load project config from .hpm/config.toml in current directory
        if let Ok(current_dir) = std::env::current_dir() {
            let project_config_path = current_dir.join(".hpm").join("config.toml");
            if project_config_path.exists() {
                debug!("Loading project config from {:?}", project_config_path);
                let project_config = Self::load_from_path(&project_config_path)?;
                config.merge(project_config);
                debug!(
                    "Loaded project configuration from {:?}",
                    project_config_path
                );
            }
        }

        Ok(config)
    }

    /// Load configuration from a specific path.
    pub fn load_from_path(path: &Path) -> Result<Self, ConfigError> {
        let content = std::fs::read_to_string(path).map_err(|e| ConfigError::Read {
            path: path.to_path_buf(),
            source: e,
        })?;

        Self::parse_toml(&content, path)
    }

    /// Parse configuration from a TOML string.
    fn parse_toml(content: &str, path: &Path) -> Result<Self, ConfigError> {
        toml::from_str(content).map_err(|e| ConfigError::Parse {
            path: path.to_path_buf(),
            source: Box::new(e),
        })
    }

    /// Merge another configuration into this one.
    /// Values from `other` override values in `self` if they are set.
    pub fn merge(&mut self, other: Self) {
        // Install config
        self.install.path = other.install.path;
        self.install.parallel_downloads = other.install.parallel_downloads;

        // Storage config - only override if different from defaults
        // (partial config might contain defaults we don't want to override)
        if other.storage.home_dir != Self::default().storage.home_dir {
            self.storage.home_dir = other.storage.home_dir.clone();
            self.storage.cache_dir = other.storage.cache_dir;
            self.storage.packages_dir = other.storage.packages_dir;
            self.storage.registry_cache_dir = other.storage.registry_cache_dir;
        }

        // Projects config
        if !other.projects.explicit_paths.is_empty() {
            self.projects.explicit_paths = other.projects.explicit_paths;
        }
        if !other.projects.search_roots.is_empty() {
            self.projects.search_roots = other.projects.search_roots;
        }
        if other.projects.max_search_depth != ProjectsConfig::default().max_search_depth {
            self.projects.max_search_depth = other.projects.max_search_depth;
        }
        if other.projects.ignore_patterns != ProjectsConfig::default().ignore_patterns {
            self.projects.ignore_patterns = other.projects.ignore_patterns;
        }

        // Registries config - replace if other has any
        if !other.registries.is_empty() {
            self.registries = other.registries;
        }

        // Signing config
        if other.signing.key_path.is_some() {
            self.signing.key_path = other.signing.key_path;
        }
    }

    /// Add a registry to the configuration.
    /// Returns false if a registry with the same name already exists.
    pub fn add_registry(&mut self, registry: RegistrySourceConfig) -> bool {
        if self.registries.iter().any(|r| r.name == registry.name) {
            return false;
        }
        self.registries.push(registry);
        true
    }

    /// Remove a registry by name. Returns true if found and removed.
    pub fn remove_registry(&mut self, name: &str) -> bool {
        let before = self.registries.len();
        self.registries.retain(|r| r.name != name);
        self.registries.len() < before
    }

    /// Get the cache directory for a specific git registry.
    pub fn registry_cache_path(&self, registry_name: &str) -> PathBuf {
        self.storage.registry_cache_dir.join(registry_name)
    }

    /// Save the configuration to a file.
    pub fn save(&self, path: &Path) -> Result<(), ConfigError> {
        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| ConfigError::Write {
                path: path.to_path_buf(),
                source: e,
            })?;
        }

        let content = toml::to_string_pretty(self)?;

        // Atomic write: stage to <path>.tmp, then rename. A crash or
        // interrupt mid-write would otherwise leave a truncated TOML that
        // every subsequent `Config::load()` has to warn-and-recover from.
        let mut tmp_path = path.as_os_str().to_os_string();
        tmp_path.push(".tmp");
        let tmp_path = PathBuf::from(tmp_path);
        std::fs::write(&tmp_path, content).map_err(|e| ConfigError::Write {
            path: tmp_path.clone(),
            source: e,
        })?;
        std::fs::rename(&tmp_path, path).map_err(|e| ConfigError::Write {
            path: path.to_path_buf(),
            source: e,
        })?;

        info!("Saved configuration to {:?}", path);
        Ok(())
    }

    /// Save the configuration to the user's config file (~/.hpm/config.toml).
    pub fn save_user_config(&self) -> Result<(), ConfigError> {
        let path = Self::default_home_dir().join("config.toml");
        self.save(&path)
    }

    /// Create a `ConfigBuilder` for constructing a `Config` programmatically
    /// without reading from disk.
    pub fn builder() -> ConfigBuilder {
        ConfigBuilder::default()
    }

    pub fn project_paths(project_root: &Path) -> ProjectPaths {
        let hpm_dir = project_root.join(".hpm");
        ProjectPaths {
            packages_dir: hpm_dir.join("packages"),
            lock_file: project_root.join("hpm.lock"),
            manifest_file: project_root.join("hpm.toml"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn config_default_values() {
        let config = Config::default();
        assert_eq!(config.install.path, "packages/hpm");
        assert_eq!(config.install.parallel_downloads, 8);
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

        assert_eq!(config.install.path, deserialized.install.path);
        assert_eq!(config.storage.home_dir, deserialized.storage.home_dir);
        assert_eq!(
            config.projects.max_search_depth,
            deserialized.projects.max_search_depth
        );
    }

    #[test]
    fn config_load_from_path() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.toml");

        let config_content = r#"
[install]
path = "custom/path"
parallel_downloads = 16
"#;
        std::fs::write(&config_path, config_content).unwrap();

        let config = Config::load_from_path(&config_path).unwrap();

        assert_eq!(config.install.path, "custom/path");
        assert_eq!(config.install.parallel_downloads, 16);
    }

    #[test]
    fn config_load_partial() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.toml");

        // Only specify some fields - others should use defaults
        let config_content = r#"
[install]
path = "custom/hpm"
"#;
        std::fs::write(&config_path, config_content).unwrap();

        let config = Config::load_from_path(&config_path).unwrap();

        // Custom value
        assert_eq!(config.install.path, "custom/hpm");
        // Default values for unspecified fields
        assert_eq!(config.install.parallel_downloads, 8);
    }

    #[test]
    fn config_load_empty_file() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.toml");

        // Empty config file should use all defaults
        std::fs::write(&config_path, "").unwrap();

        let config = Config::load_from_path(&config_path).unwrap();

        assert_eq!(config.install.path, "packages/hpm");
        assert_eq!(config.install.parallel_downloads, 8);
    }

    #[test]
    fn config_save_and_load() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.toml");

        let mut config = Config::default();
        config.install.path = "custom/install/path".to_string();
        config.install.parallel_downloads = 4;

        config.save(&config_path).unwrap();

        let loaded_config = Config::load_from_path(&config_path).unwrap();

        assert_eq!(loaded_config.install.path, "custom/install/path");
        assert_eq!(loaded_config.install.parallel_downloads, 4);
    }

    #[test]
    fn config_merge() {
        let mut base = Config::default();
        let mut override_config = Config::default();

        override_config.install.path = "override/path".to_string();
        override_config.install.parallel_downloads = 32;

        base.merge(override_config);

        assert_eq!(base.install.path, "override/path");
        assert_eq!(base.install.parallel_downloads, 32);
    }

    #[test]
    fn config_load_with_projects() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.toml");

        let config_content = r#"
[projects]
explicit_paths = ["/path/to/project1", "/path/to/project2"]
search_roots = ["/home/user/projects"]
max_search_depth = 5
ignore_patterns = ["backup", ".cache", "temp"]
"#;
        std::fs::write(&config_path, config_content).unwrap();

        let config = Config::load_from_path(&config_path).unwrap();

        assert_eq!(config.projects.explicit_paths.len(), 2);
        assert_eq!(config.projects.search_roots.len(), 1);
        assert_eq!(config.projects.max_search_depth, 5);
        assert_eq!(config.projects.ignore_patterns.len(), 3);
        assert!(
            config
                .projects
                .ignore_patterns
                .contains(&"backup".to_string())
        );
    }

    #[test]
    fn config_load_invalid_toml() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.toml");

        std::fs::write(&config_path, "invalid [ toml content").unwrap();

        let result = Config::load_from_path(&config_path);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ConfigError::Parse { .. }));
    }

    #[test]
    fn config_load_nonexistent_file() {
        let result = Config::load_from_path(Path::new("/nonexistent/config.toml"));
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ConfigError::Read { .. }));
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
        // Normalize path separators for cross-platform testing
        let pkg_dir_str = pkg_dir.to_string_lossy().replace('\\', "/");
        assert!(
            pkg_dir_str.ends_with("packages/test-package@1.0.0"),
            "Expected path to end with 'packages/test-package@1.0.0', got: {}",
            pkg_dir_str
        );
    }

    #[test]
    fn project_paths_package_manifest_path() {
        let project_root = PathBuf::from("/test/project");
        let project_paths = Config::project_paths(&project_root);
        let manifest_path = project_paths.package_manifest_path("test-package");
        // Normalize path separators for cross-platform testing
        let manifest_path_str = manifest_path.to_string_lossy().replace('\\', "/");
        assert!(
            manifest_path_str.ends_with(".hpm/packages/test-package.json"),
            "Expected path to end with '.hpm/packages/test-package.json', got: {}",
            manifest_path_str
        );
    }

    #[test]
    fn project_paths_structure() {
        let project_root = PathBuf::from("/test/project");
        let project_paths = Config::project_paths(&project_root);

        // Normalize path separators for cross-platform testing
        let packages_dir_str = project_paths
            .packages_dir
            .to_string_lossy()
            .replace('\\', "/");
        assert!(
            packages_dir_str.ends_with(".hpm/packages"),
            "Expected packages_dir to end with '.hpm/packages', got: {}",
            packages_dir_str
        );
        assert!(
            project_paths
                .lock_file
                .to_string_lossy()
                .ends_with("hpm.lock")
        );
        assert!(
            project_paths
                .manifest_file
                .to_string_lossy()
                .ends_with("hpm.toml")
        );
    }

    #[test]
    fn config_load_with_wrong_type_values() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.toml");

        // Write config with wrong types (max_search_depth should be integer, not string)
        std::fs::write(
            &config_path,
            r#"
[projects]
max_search_depth = "not_a_number"
"#,
        )
        .unwrap();

        let result = Config::load_from_path(&config_path);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ConfigError::Parse { .. }));
    }

    #[test]
    fn config_load_with_nested_invalid_toml() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.toml");

        // Write config with syntactically invalid nested structure
        std::fs::write(
            &config_path,
            r#"
[install]
path = "packages"
[storage
home_dir = "missing closing bracket"
"#,
        )
        .unwrap();

        let result = Config::load_from_path(&config_path);
        assert!(result.is_err());
    }

    #[test]
    fn config_error_display_messages() {
        // Test that error messages are informative
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.toml");
        std::fs::write(&config_path, "invalid { toml").unwrap();

        let result = Config::load_from_path(&config_path);
        let err = result.unwrap_err();
        let err_string = err.to_string();

        // Error message should mention the path
        assert!(err_string.contains("config.toml"));
    }
}
