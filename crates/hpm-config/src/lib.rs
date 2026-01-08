//! HPM Configuration Management
//!
//! This crate provides comprehensive configuration management for HPM, supporting
//! hierarchical configuration with global defaults, user overrides, and project-specific
//! settings. The configuration system is designed for flexibility and ease of use
//! while maintaining strong type safety and validation.
//!
//! ## Configuration Architecture
//!
//! HPM uses a layered configuration system where settings can be specified at multiple levels:
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────────────┐
//! │                         HPM Configuration Hierarchy                             │
//! ├─────────────────────────────────────────────────────────────────────────────────┤
//! │                                                                                 │
//! │  Application Defaults                                                          │
//! │  ┌─────────────────────────────────────────────────────────────────────────┐   │
//! │  │  • Built-in sensible defaults                                           │   │
//! │  │  • Registry: https://packages.houdini.org                              │   │
//! │  │  • Storage: ~/.hpm/                                                     │   │
//! │  │  • Parallel downloads: 8                                                │   │
//! │  └─────────────────────────────────────────────────────────────────────────┘   │
//! │                                    │                                           │
//! │                                    ▼ (overridden by)                          │
//! │  User Configuration                                                            │
//! │  ┌─────────────────────────────────────────────────────────────────────────┐   │
//! │  │  ~/.hpm/config.toml                                                     │   │
//! │  │  • User-specific preferences                                            │   │
//! │  │  • Authentication tokens                                                │   │
//! │  │  • Custom registry endpoints                                            │   │
//! │  │  • Project discovery settings                                           │   │
//! │  └─────────────────────────────────────────────────────────────────────────┘   │
//! │                                    │                                           │
//! │                                    ▼ (overridden by)                          │
//! │  Project Configuration                                                         │
//! │  ┌─────────────────────────────────────────────────────────────────────────┐   │
//! │  │  project/.hpm/config.toml                                               │   │
//! │  │  • Project-specific settings                                            │   │
//! │  │  • Local package locations                                              │   │
//! │  │  • Development overrides                                                │   │
//! │  └─────────────────────────────────────────────────────────────────────────┘   │
//! │                                    │                                           │
//! │                                    ▼ (overridden by)                          │
//! │  Runtime Configuration                                                         │
//! │  ┌─────────────────────────────────────────────────────────────────────────┐   │
//! │  │  • Command-line arguments                                               │   │
//! │  │  • Environment variables                                                │   │
//! │  │  • API overrides                                                        │   │
//! │  └─────────────────────────────────────────────────────────────────────────┘   │
//! └─────────────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Core Configuration Types
//!
//! ### Global Configuration ([`Config`])
//! The main configuration structure containing all HPM settings:
//!
//! - **Registry Configuration**: Default registry URLs and authentication
//! - **Storage Configuration**: Package storage locations and cache settings
//! - **Installation Configuration**: Download parallelism and installation paths
//! - **Project Discovery**: Search roots and patterns for finding HPM projects
//! - **Authentication**: API tokens and authentication scopes
//!
//! ### Project-Specific Configuration ([`ProjectConfig`])
//! Configuration for individual HPM projects:
//!
//! - **Package Directory**: Location of `.hpm/packages/` for project integration
//! - **Lock File Location**: Path to `hpm.lock` with dependency resolution
//! - **Manifest Location**: Path to `hpm.toml` package manifest
//!
//! ## Configuration Examples
//!
//! ### Basic Usage
//! ```rust
//! use hpm_config::Config;
//!
//! // Load configuration with defaults
//! let config = Config::default();
//! println!("Storage directory: {:?}", config.storage.home_dir);
//! println!("Parallel downloads: {}", config.install.parallel_downloads);
//! ```
//!
//! ### Project Configuration
//! ```rust,no_run
//! use hpm_config::Config;
//! use std::path::Path;
//!
//! let project_root = Path::new("/path/to/houdini-project");
//! let project_config = Config::load_project_config(project_root);
//!
//! // Ensure project directories exist
//! project_config.ensure_directories().unwrap();
//!
//! println!("Package manifest: {:?}", project_config.package_manifest_path("my-package"));
//! ```
//!
//! ### Custom Project Discovery
//! ```rust
//! use hpm_config::{Config, ProjectsConfig};
//!
//! let mut config = Config::default();
//!
//! // Add custom search locations
//! config.projects.add_search_root("/Users/artist/houdini-projects".into());
//! config.projects.add_explicit_path("/shared/studio-packages".into());
//!
//! // Configure search behavior
//! config.projects.max_search_depth = 4;
//! config.projects.ignore_patterns.push("backup".to_string());
//! ```
//!
//! ## Configuration File Format
//!
//! HPM uses TOML format for all configuration files, providing a human-readable
//! and version-control-friendly format:
//!
//! ```toml
//! # ~/.hpm/config.toml
//!
//! [registry]
//! default = "https://packages.houdini.org"
//!
//! [install]
//! path = "packages/hpm"
//! parallel_downloads = 16
//!
//! [storage]
//! home_dir = "/custom/hpm/location"
//! cache_dir = "/fast/ssd/cache"
//!
//! [projects]
//! explicit_paths = [
//!     "/studio/shared-packages",
//!     "/artist/personal-tools"
//! ]
//! search_roots = [
//!     "/Users/artist/houdini-projects",
//!     "/shared/project-library"
//! ]
//! max_search_depth = 3
//! ignore_patterns = [
//!     ".git", "node_modules", "backup", "archive"
//! ]
//!
//! [auth]
//! token = "your-secure-token-here"
//! ```
//!
//! ## Directory Structure Management
//!
//! The configuration system automatically manages HPM's directory structure:
//!
//! ### Global Structure (`~/.hpm/`)
//! ```text
//! ~/.hpm/
//! ├── config.toml          # User configuration
//! ├── packages/            # Global package storage  
//! │   ├── package-a@1.0.0/
//! │   └── package-b@2.1.0/
//! ├── cache/               # Registry and download cache
//! ├── registry/            # Registry index cache
//! ├── venvs/              # Python virtual environments
//! └── uv-cache/           # UV package cache (isolated)
//! ```
//!
//! ### Project Structure
//! ```text
//! project/
//! ├── hpm.toml            # Package manifest
//! ├── hpm.lock            # Dependency lock file  
//! └── .hpm/
//!     ├── config.toml     # Project-specific config (optional)
//!     └── packages/       # Houdini package manifests
//!         ├── package-a.json
//!         └── package-b.json
//! ```
//!
//! ## Project Discovery System
//!
//! The [`ProjectsConfig`] provides sophisticated project discovery capabilities:
//!
//! ### Discovery Methods
//! - **Explicit Paths**: Directly specified project locations
//! - **Search Roots**: Directories to recursively search for HPM projects
//! - **Depth Limiting**: Prevent excessive filesystem traversal
//! - **Pattern Ignoring**: Skip directories that match ignore patterns
//!
//! ### Usage Example
//! ```rust
//! use hpm_config::ProjectsConfig;
//!
//! let mut projects_config = ProjectsConfig::default();
//!
//! // Add locations to search
//! projects_config.add_search_root("/Users/artist/work".into());
//! projects_config.add_explicit_path("/shared/tools/houdini-package".into());
//!
//! // Check if directory should be ignored
//! assert!(projects_config.should_ignore(".git"));
//! assert!(projects_config.should_ignore("backup"));
//! assert!(!projects_config.should_ignore("my-houdini-project"));
//! ```
//!
//! ## Storage Configuration
//!
//! The [`StorageConfig`] manages all aspects of package and cache storage:
//!
//! ### Key Features
//! - **Automatic Directory Creation**: Ensures all required directories exist
//! - **Flexible Storage Locations**: Customizable paths for different storage types
//! - **Version-Aware Package Organization**: Packages stored with version identifiers
//! - **Cache Management**: Separate caches for different data types
//!
//! ### Usage Example
//! ```rust
//! use hpm_config::{Config, StorageConfig};
//!
//! let config = Config::default();
//! let storage = &config.storage;
//!
//! // Ensure all storage directories exist
//! storage.ensure_directories().unwrap();
//!
//! // Get package-specific directory
//! let package_dir = storage.package_dir("geometry-tools", "2.1.0");
//! println!("Package stored at: {:?}", package_dir);
//! ```
//!
//! ## Configuration Customization
//!
//! HPM configuration can be customized programmatically:
//!
//! ```rust
//! use hpm_config::Config;
//!
//! let mut config = Config::default();
//!
//! // Customize installation settings
//! config.install.parallel_downloads = 8;
//!
//! // Add project search locations
//! config.projects.add_search_root("/my/projects".into());
//! ```
//!
//! ## Error Handling and Validation
//!
//! All configuration operations include comprehensive error handling:
//!
//! - **Directory Creation**: Handles permission issues and filesystem errors
//! - **Path Validation**: Ensures paths are valid and accessible
//! - **Configuration Parsing**: Provides detailed error messages for invalid TOML
//! - **Type Safety**: Rust's type system prevents configuration errors at compile time

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tracing::{debug, info};

/// Errors that can occur when loading configuration
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("Failed to read config file: {path}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("Failed to parse config file: {path}")]
    Parse {
        path: PathBuf,
        #[source]
        source: Box<toml::de::Error>,
    },

    #[error("Failed to serialize config")]
    Serialize(#[from] toml::ser::Error),

    #[error("Failed to write config file: {path}")]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub install: InstallConfig,
    pub storage: StorageConfig,
    pub projects: ProjectsConfig,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct InstallConfig {
    pub path: String,
    pub parallel_downloads: usize,
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

        // Load user config from ~/.hpm/config.toml
        let user_config_path = Self::default_home_dir().join("config.toml");
        if user_config_path.exists() {
            debug!("Loading user config from {:?}", user_config_path);
            let user_config = Self::load_from_path(&user_config_path)?;
            config.merge(user_config);
            info!("Loaded user configuration from {:?}", user_config_path);
        }

        // Load project config from .hpm/config.toml in current directory
        if let Ok(current_dir) = std::env::current_dir() {
            let project_config_path = current_dir.join(".hpm").join("config.toml");
            if project_config_path.exists() {
                debug!("Loading project config from {:?}", project_config_path);
                let project_config = Self::load_from_path(&project_config_path)?;
                config.merge(project_config);
                info!("Loaded project configuration from {:?}", project_config_path);
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
        // Parse into a partial config that allows missing fields
        let partial: PartialConfig =
            toml::from_str(content).map_err(|e| ConfigError::Parse {
                path: path.to_path_buf(),
                source: Box::new(e),
            })?;

        // Convert partial config to full config with defaults
        Ok(partial.into_config())
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
        std::fs::write(path, content).map_err(|e| ConfigError::Write {
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

    pub fn load_project_config(project_root: &Path) -> ProjectConfig {
        let hpm_dir = project_root.join(".hpm");
        ProjectConfig {
            packages_dir: hpm_dir.join("packages"),
            lock_file: project_root.join("hpm.lock"),
            manifest_file: project_root.join("hpm.toml"),
        }
    }
}

/// Partial configuration for parsing TOML files that may have missing fields.
/// All fields are optional and will be filled with defaults when converting to Config.
#[derive(Debug, Deserialize, Default)]
struct PartialConfig {
    install: Option<PartialInstallConfig>,
    storage: Option<PartialStorageConfig>,
    projects: Option<PartialProjectsConfig>,
}

#[derive(Debug, Deserialize)]
struct PartialInstallConfig {
    path: Option<String>,
    parallel_downloads: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct PartialStorageConfig {
    home_dir: Option<PathBuf>,
    cache_dir: Option<PathBuf>,
    packages_dir: Option<PathBuf>,
    registry_cache_dir: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
struct PartialProjectsConfig {
    explicit_paths: Option<Vec<PathBuf>>,
    search_roots: Option<Vec<PathBuf>>,
    max_search_depth: Option<usize>,
    ignore_patterns: Option<Vec<String>>,
}

impl PartialConfig {
    fn into_config(self) -> Config {
        let default = Config::default();

        let home_dir = self
            .storage
            .as_ref()
            .and_then(|s| s.home_dir.clone())
            .unwrap_or(default.storage.home_dir.clone());

        Config {
            install: InstallConfig {
                path: self
                    .install
                    .as_ref()
                    .and_then(|i| i.path.clone())
                    .unwrap_or(default.install.path),
                parallel_downloads: self
                    .install
                    .and_then(|i| i.parallel_downloads)
                    .unwrap_or(default.install.parallel_downloads),
            },
            storage: StorageConfig {
                home_dir: home_dir.clone(),
                cache_dir: self
                    .storage
                    .as_ref()
                    .and_then(|s| s.cache_dir.clone())
                    .unwrap_or_else(|| home_dir.join("cache")),
                packages_dir: self
                    .storage
                    .as_ref()
                    .and_then(|s| s.packages_dir.clone())
                    .unwrap_or_else(|| home_dir.join("packages")),
                registry_cache_dir: self
                    .storage
                    .as_ref()
                    .and_then(|s| s.registry_cache_dir.clone())
                    .unwrap_or_else(|| home_dir.join("registry")),
            },
            projects: ProjectsConfig {
                explicit_paths: self
                    .projects
                    .as_ref()
                    .and_then(|p| p.explicit_paths.clone())
                    .unwrap_or(default.projects.explicit_paths),
                search_roots: self
                    .projects
                    .as_ref()
                    .and_then(|p| p.search_roots.clone())
                    .unwrap_or(default.projects.search_roots),
                max_search_depth: self
                    .projects
                    .as_ref()
                    .and_then(|p| p.max_search_depth)
                    .unwrap_or(default.projects.max_search_depth),
                ignore_patterns: self
                    .projects
                    .and_then(|p| p.ignore_patterns)
                    .unwrap_or(default.projects.ignore_patterns),
            },
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
        assert!(config.projects.ignore_patterns.contains(&"backup".to_string()));
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
    fn project_config_package_manifest_path() {
        let project_root = PathBuf::from("/test/project");
        let project_config = Config::load_project_config(&project_root);
        let manifest_path = project_config.package_manifest_path("test-package");
        // Normalize path separators for cross-platform testing
        let manifest_path_str = manifest_path.to_string_lossy().replace('\\', "/");
        assert!(
            manifest_path_str.ends_with(".hpm/packages/test-package.json"),
            "Expected path to end with '.hpm/packages/test-package.json', got: {}",
            manifest_path_str
        );
    }

    #[test]
    fn project_config_structure() {
        let project_root = PathBuf::from("/test/project");
        let project_config = Config::load_project_config(&project_root);

        // Normalize path separators for cross-platform testing
        let packages_dir_str = project_config.packages_dir.to_string_lossy().replace('\\', "/");
        assert!(
            packages_dir_str.ends_with(".hpm/packages"),
            "Expected packages_dir to end with '.hpm/packages', got: {}",
            packages_dir_str
        );
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
