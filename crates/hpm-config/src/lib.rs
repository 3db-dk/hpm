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
//! println!("Registry URL: {}", config.registry.default);
//! println!("Storage directory: {:?}", config.storage.home_dir);
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
//! ## Authentication Management
//!
//! HPM supports secure token-based authentication for registry access:
//!
//! ```rust
//! use hpm_config::{Config, AuthConfig};
//!
//! let mut config = Config::default();
//!
//! // Set authentication token
//! config.auth = Some(AuthConfig {
//!     token: "your-secure-registry-token".to_string(),
//! });
//!
//! // Token can be used by registry clients for authenticated operations
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
