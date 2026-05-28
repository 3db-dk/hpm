//! Global storage layout: where HPM keeps packages, caches, and registry
//! indexes on disk.

use hpm_package::user_home;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    #[serde(default = "default_storage_home")]
    pub home_dir: PathBuf,
    #[serde(default = "default_storage_cache")]
    pub cache_dir: PathBuf,
    #[serde(default = "default_storage_packages")]
    pub packages_dir: PathBuf,
    #[serde(default = "default_storage_registry_cache")]
    pub registry_cache_dir: PathBuf,
}

/// Default `~/.hpm/` location. Falls back to `.hpm` in CWD on machines where
/// `$HOME` / `%USERPROFILE%` is unset (typically test runners), so a stray
/// integration test never tries to write to `/`.
pub fn default_home_dir() -> PathBuf {
    user_home()
        .map(|h| h.join(".hpm"))
        .unwrap_or_else(|| PathBuf::from(".hpm"))
}

fn default_storage_home() -> PathBuf {
    default_home_dir()
}

fn default_storage_cache() -> PathBuf {
    default_storage_home().join("cache")
}

fn default_storage_packages() -> PathBuf {
    default_storage_home().join("packages")
}

fn default_storage_registry_cache() -> PathBuf {
    default_storage_home().join("registry")
}

impl Default for StorageConfig {
    fn default() -> Self {
        let home_dir = default_storage_home();
        Self {
            cache_dir: home_dir.join("cache"),
            packages_dir: home_dir.join("packages"),
            registry_cache_dir: home_dir.join("registry"),
            home_dir,
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
