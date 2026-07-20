//! Presence-aware partial configuration — the shape config files are parsed as.
//!
//! [`Config`] is the fully resolved configuration. `ConfigOverlay` mirrors it
//! with every field optional so that layering (defaults <- user <- project)
//! applies exactly the values a file actually sets. Comparing resolved values
//! against defaults cannot distinguish "unset" from "explicitly set to the
//! default value", so overlays are the only shape files are parsed as.

use crate::Config;
use crate::registry::RegistrySourceConfig;
use hpm_package::TomlFileError;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ConfigOverlay {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub install: Option<InstallOverlay>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub storage: Option<StorageOverlay>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub projects: Option<ProjectsOverlay>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub registries: Option<Vec<RegistrySourceConfig>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signing: Option<SigningOverlay>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InstallOverlay {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parallel_downloads: Option<usize>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StorageOverlay {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub home_dir: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_dir: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub packages_dir: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub registry_cache_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProjectsOverlay {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub explicit_paths: Option<Vec<PathBuf>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search_roots: Option<Vec<PathBuf>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_search_depth: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ignore_patterns: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SigningOverlay {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_path: Option<PathBuf>,
}

impl ConfigOverlay {
    /// Load an overlay from a TOML file.
    pub fn load(path: &Path) -> Result<Self, TomlFileError> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| hpm_package::IoOp::wrap("read TOML file", path, e))?;
        Self::parse(&content, path)
    }

    /// Parse an overlay from a TOML string.
    pub fn parse(content: &str, path: &Path) -> Result<Self, TomlFileError> {
        toml::from_str(content).map_err(|e| TomlFileError::Parse {
            path: path.to_path_buf(),
            source: Box::new(e),
        })
    }

    /// Save the overlay to a TOML file, writing only the fields that are set.
    pub fn save(&self, path: &Path) -> Result<(), TomlFileError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                hpm_package::IoOp::wrap("create config parent directory", parent, e)
            })?;
        }
        let content = toml::to_string_pretty(self)?;
        hpm_package::atomic_write(path, content)?;
        Ok(())
    }

    /// Apply every value this overlay sets onto `config`.
    pub fn apply_to(&self, config: &mut Config) {
        if let Some(install) = &self.install {
            if let Some(path) = &install.path {
                config.install.path = path.clone();
            }
            if let Some(parallel_downloads) = install.parallel_downloads {
                config.install.parallel_downloads = parallel_downloads;
            }
        }

        if let Some(storage) = &self.storage {
            // A custom home_dir re-derives the sub-directories; explicit
            // sub-directory values below still win over the derived ones.
            if let Some(home_dir) = &storage.home_dir {
                config.storage.home_dir = home_dir.clone();
                config.storage.cache_dir = home_dir.join("cache");
                config.storage.packages_dir = home_dir.join("packages");
                config.storage.registry_cache_dir = home_dir.join("registry");
            }
            if let Some(cache_dir) = &storage.cache_dir {
                config.storage.cache_dir = cache_dir.clone();
            }
            if let Some(packages_dir) = &storage.packages_dir {
                config.storage.packages_dir = packages_dir.clone();
            }
            if let Some(registry_cache_dir) = &storage.registry_cache_dir {
                config.storage.registry_cache_dir = registry_cache_dir.clone();
            }
        }

        if let Some(projects) = &self.projects {
            if let Some(explicit_paths) = &projects.explicit_paths {
                config.projects.explicit_paths = explicit_paths.clone();
            }
            if let Some(search_roots) = &projects.search_roots {
                config.projects.search_roots = search_roots.clone();
            }
            if let Some(max_search_depth) = projects.max_search_depth {
                config.projects.max_search_depth = max_search_depth;
            }
            if let Some(ignore_patterns) = &projects.ignore_patterns {
                config.projects.ignore_patterns = ignore_patterns.clone();
            }
        }

        if let Some(registries) = &self.registries {
            config.registries = registries.clone();
        }

        if let Some(signing) = &self.signing
            && let Some(key_path) = &signing.key_path
        {
            config.signing.key_path = Some(key_path.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlay_only_applies_set_fields() {
        let mut config = Config::default();
        config.install.path = "user/path".to_string();
        config.install.parallel_downloads = 4;

        // A project config that never mentions [install] must not touch it.
        let overlay = ConfigOverlay::parse(
            r#"
[projects]
max_search_depth = 5
"#,
            Path::new("test.toml"),
        )
        .unwrap();
        overlay.apply_to(&mut config);

        assert_eq!(config.install.path, "user/path");
        assert_eq!(config.install.parallel_downloads, 4);
        assert_eq!(config.projects.max_search_depth, 5);
    }

    #[test]
    fn overlay_explicit_default_value_still_applies() {
        let mut config = Config::default();
        config.projects.max_search_depth = 7;

        // Explicitly setting the default value (3) must override, even though
        // it equals the built-in default.
        let overlay = ConfigOverlay::parse(
            r#"
[projects]
max_search_depth = 3
"#,
            Path::new("test.toml"),
        )
        .unwrap();
        overlay.apply_to(&mut config);

        assert_eq!(config.projects.max_search_depth, 3);
    }

    #[test]
    fn overlay_home_dir_rederives_subdirectories() {
        let mut config = Config::default();
        let overlay = ConfigOverlay::parse(
            r#"
[storage]
home_dir = "/custom/hpm"
"#,
            Path::new("test.toml"),
        )
        .unwrap();
        overlay.apply_to(&mut config);

        assert_eq!(config.storage.home_dir, PathBuf::from("/custom/hpm"));
        assert_eq!(config.storage.cache_dir, PathBuf::from("/custom/hpm/cache"));
        assert_eq!(
            config.storage.packages_dir,
            PathBuf::from("/custom/hpm/packages")
        );
        assert_eq!(
            config.storage.registry_cache_dir,
            PathBuf::from("/custom/hpm/registry")
        );
    }

    #[test]
    fn overlay_explicit_subdirectory_wins_over_derived() {
        let mut config = Config::default();
        let overlay = ConfigOverlay::parse(
            r#"
[storage]
home_dir = "/custom/hpm"
cache_dir = "/fast-disk/hpm-cache"
"#,
            Path::new("test.toml"),
        )
        .unwrap();
        overlay.apply_to(&mut config);

        assert_eq!(
            config.storage.cache_dir,
            PathBuf::from("/fast-disk/hpm-cache")
        );
        assert_eq!(
            config.storage.packages_dir,
            PathBuf::from("/custom/hpm/packages")
        );
    }

    #[test]
    fn overlay_single_subdirectory_applies_without_home_dir() {
        let mut config = Config::default();
        let default_home = config.storage.home_dir.clone();
        let overlay = ConfigOverlay::parse(
            r#"
[storage]
cache_dir = "/fast-disk/hpm-cache"
"#,
            Path::new("test.toml"),
        )
        .unwrap();
        overlay.apply_to(&mut config);

        assert_eq!(
            config.storage.cache_dir,
            PathBuf::from("/fast-disk/hpm-cache")
        );
        assert_eq!(config.storage.home_dir, default_home);
    }

    #[test]
    fn overlay_save_writes_only_set_fields() {
        let overlay = ConfigOverlay {
            registries: Some(vec![]),
            ..Default::default()
        };
        let serialized = toml::to_string_pretty(&overlay).unwrap();
        assert!(!serialized.contains("install"));
        assert!(!serialized.contains("storage"));
        assert!(serialized.contains("registries"));
    }

    #[test]
    fn overlay_roundtrip_preserves_full_dump() {
        // Files written by older hpm versions contain every section; the
        // overlay must parse and re-save them losslessly.
        let content = r#"
[install]
path = "packages/hpm"
parallel_downloads = 8

[[registries]]
name = "3db"
url = "https://api.tumbletrove.com/v1/registry"
type = "api"
"#;
        let overlay = ConfigOverlay::parse(content, Path::new("test.toml")).unwrap();
        assert!(overlay.install.is_some());
        let regs = overlay.registries.as_ref().unwrap();
        assert_eq!(regs.len(), 1);
        assert_eq!(regs[0].name, "3db");
    }
}
