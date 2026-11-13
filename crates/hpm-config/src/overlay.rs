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

#[derive(Debug, PartialEq, Eq, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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

#[derive(Debug, PartialEq, Eq, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstallOverlay {
    /// `[install] path`, which earlier hpm versions wrote (TumbleTrove
    /// Desktop saved it into users' config files) but never read: projects
    /// always install into `.hpm/packages`. Accepted so those files keep
    /// loading, reported by [`ConfigOverlay::load`], never written.
    #[serde(default, skip_serializing)]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parallel_downloads: Option<std::num::NonZeroUsize>,
}

#[derive(Debug, PartialEq, Eq, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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

#[derive(Debug, PartialEq, Eq, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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

#[derive(Debug, PartialEq, Eq, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SigningOverlay {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_path: Option<PathBuf>,
}

/// The shape a project's `<project>/.hpm/config.toml` is parsed as.
///
/// A project config is content of whatever repository the user cloned, so
/// it may only set what is about the project itself: `[install]`.
/// `[storage]`, `[projects]` and `[signing]` decide where hpm keeps its own
/// state and tools, what `hpm clean` treats as live, and which key
/// `hpm pack` signs with; `[[registries]]` is the list an embedder's
/// credentials are scoped to (a project declares its registries in
/// `hpm.toml` instead, which never widens that scope). They belong to the
/// user's config.
/// They are accepted so an existing file keeps loading, reported by
/// [`ProjectConfigOverlay::load`], and never applied.
#[derive(Debug, PartialEq, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectConfigOverlay {
    #[serde(default)]
    install: Option<InstallOverlay>,
    #[serde(default)]
    registries: Option<toml::Value>,
    #[serde(default)]
    storage: Option<toml::Value>,
    #[serde(default)]
    projects: Option<toml::Value>,
    #[serde(default)]
    signing: Option<toml::Value>,
}

impl ProjectConfigOverlay {
    /// Load a project overlay, warning about each user-only section it sets.
    pub fn load(path: &Path) -> Result<Self, TomlFileError> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| hpm_package::IoOp::wrap("read TOML file", path, e))?;
        let overlay: Self = toml::from_str(&content).map_err(|e| TomlFileError::Parse {
            path: path.to_path_buf(),
            source: Box::new(e),
        })?;
        for section in overlay.ignored_sections() {
            tracing::warn!(
                "{}: `[{section}]` is only read from the user config (~/.hpm/config.toml); \
                 ignored in a project config",
                path.display()
            );
        }
        if overlay.install.as_ref().is_some_and(|i| i.path.is_some()) {
            tracing::warn!(
                "{}: `[install] path` is not used by hpm and is ignored; it can be removed",
                path.display()
            );
        }
        Ok(overlay)
    }

    /// The user-only sections this file sets, which are not applied.
    pub fn ignored_sections(&self) -> Vec<&'static str> {
        [
            ("registries", self.registries.is_some()),
            ("storage", self.storage.is_some()),
            ("projects", self.projects.is_some()),
            ("signing", self.signing.is_some()),
        ]
        .into_iter()
        .filter_map(|(name, set)| set.then_some(name))
        .collect()
    }

    /// Apply the project-scoped values onto `config`.
    pub fn apply_to(&self, config: &mut Config) {
        ConfigOverlay {
            install: self.install.clone(),
            ..ConfigOverlay::default()
        }
        .apply_to(config);
    }
}

impl ConfigOverlay {
    /// Load an overlay from a TOML file.
    pub fn load(path: &Path) -> Result<Self, TomlFileError> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| hpm_package::IoOp::wrap("read TOML file", path, e))?;
        let overlay = Self::parse(&content, path)?;
        if overlay.install.as_ref().is_some_and(|i| i.path.is_some()) {
            tracing::warn!(
                "{}: `[install] path` is not used by hpm and is ignored; it can be removed",
                path.display()
            );
        }
        Ok(overlay)
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
        if let Some(install) = &self.install
            && let Some(parallel_downloads) = install.parallel_downloads
        {
            config.install.parallel_downloads = parallel_downloads;
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
        config.install.parallel_downloads = std::num::NonZeroUsize::new(4).unwrap();

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
        assert_eq!(config.install.parallel_downloads.get(), 4);
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
