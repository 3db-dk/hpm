//! Programmatic construction of a [`Config`] without touching the filesystem.
//!
//! Useful for library consumers (e.g. the desktop client) that manage their
//! own context directories and want to pass registry config per launch
//! context.

use crate::Config;
use crate::registry::{RegistrySourceConfig, RegistryType};
use std::path::PathBuf;

/// Builder for constructing a [`Config`] programmatically without file I/O.
///
/// # Example
///
/// ```rust
/// use hpm_config::{Config, RegistryType};
///
/// let config = Config::builder()
///     .registry("houdinihub", "https://api.3db.dk/v1/registry", RegistryType::Api)
///     .registry("studio", "https://packages.studio.com", RegistryType::Api)
///     .storage_dir("/custom/.hpm")
///     .install_path("packages/hpm")
///     .build();
/// ```
#[derive(Default)]
pub struct ConfigBuilder {
    registries: Vec<RegistrySourceConfig>,
    storage_dir: Option<PathBuf>,
    install_path: Option<String>,
    parallel_downloads: Option<usize>,
}

impl ConfigBuilder {
    /// Add a registry to the configuration.
    pub fn registry(mut self, name: &str, url: &str, registry_type: RegistryType) -> Self {
        self.registries.push(RegistrySourceConfig {
            name: name.to_string(),
            url: url.to_string(),
            registry_type,
        });
        self
    }

    /// Set the HPM storage directory (default: `~/.hpm`).
    ///
    /// This also sets cache, packages, and registry cache dirs as subdirectories.
    pub fn storage_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.storage_dir = Some(path.into());
        self
    }

    /// Set the install path (default: `"packages/hpm"`).
    pub fn install_path(mut self, path: &str) -> Self {
        self.install_path = Some(path.to_string());
        self
    }

    /// Set the number of parallel downloads (default: 8).
    pub fn parallel_downloads(mut self, n: usize) -> Self {
        self.parallel_downloads = Some(n);
        self
    }

    /// Build the `Config`.
    pub fn build(self) -> Config {
        let mut config = Config::default();

        if let Some(home_dir) = self.storage_dir {
            config.storage.cache_dir = home_dir.join("cache");
            config.storage.packages_dir = home_dir.join("packages");
            config.storage.registry_cache_dir = home_dir.join("registry");
            config.storage.home_dir = home_dir;
        }

        if let Some(path) = self.install_path {
            config.install.path = path;
        }

        if let Some(n) = self.parallel_downloads {
            config.install.parallel_downloads = n;
        }

        config.registries = self.registries;
        config
    }
}
