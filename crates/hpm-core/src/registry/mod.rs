//! Package registry support for HPM.
//!
//! This module provides a trait-based registry abstraction with two implementations:
//! - [`ApiRegistry`]: HTTP-based registry (e.g., `https://api.3db.dk/v1/registry`)
//! - [`GitRegistry`]: Git-hosted index (Cargo-style, one JSON-lines file per package)
//!
//! Registries allow HPM to resolve package names to download URLs, checksums,
//! and dependency information without requiring users to specify Git URLs manually.

pub mod api;
pub mod git;
pub mod types;

use async_trait::async_trait;
use hpm_package::IoOp;
use thiserror::Error;

pub use api::ApiRegistry;
pub use git::GitRegistry;
pub use types::{RegistryDependency, RegistryEntry, SearchResults};

/// Errors that can occur during registry operations.
#[derive(Error, Debug)]
pub enum RegistryError {
    #[error("Package '{name}' not found in registry")]
    PackageNotFound { name: String },

    #[error("Version '{version}' of package '{name}' not found in registry")]
    VersionNotFound { name: String, version: String },

    #[error(
        "Version '{version}' of package '{name}' has no build compatible with host platform '{host}'"
    )]
    NoCompatibleBuild {
        name: String,
        version: String,
        host: String,
    },

    #[error("Failed to connect to registry: {0}")]
    NetworkError(#[from] reqwest::Error),

    #[error("Failed to parse registry data: {0}")]
    ParseError(String),

    #[error(transparent)]
    Io(#[from] IoOp),

    #[error("Git operation failed: {0}")]
    GitError(String),

    #[error("Registry not configured: {0}")]
    NotConfigured(String),

    #[error("Checksum mismatch for {name}@{version}: expected {expected}, got {actual}")]
    ChecksumMismatch {
        name: String,
        version: String,
        expected: String,
        actual: String,
    },
}

/// Trait for package registries.
///
/// Both API-based and Git-index-based registries implement this trait,
/// providing a unified interface for package discovery and resolution.
#[async_trait]
pub trait Registry: Send + Sync {
    /// Search the registry for packages matching a query string.
    async fn search(&self, query: &str) -> Result<SearchResults, RegistryError>;

    /// Get all versions of a package.
    async fn get_versions(&self, name: &str) -> Result<Vec<RegistryEntry>, RegistryError>;

    /// Get a specific version of a package.
    async fn get_version(&self, name: &str, version: &str) -> Result<RegistryEntry, RegistryError>;

    /// Refresh the local registry cache (fetch latest index).
    async fn refresh(&self) -> Result<(), RegistryError>;

    /// Get the registry display name.
    fn name(&self) -> &str;
}

/// A collection of registries that can be searched in order.
pub struct RegistrySet {
    registries: Vec<Box<dyn Registry>>,
}

/// Merged search results across a [`RegistrySet`], with the registries that
/// could not be reached listed explicitly.
pub struct SetSearchResults {
    /// Matching entries from every reachable registry.
    pub packages: Vec<RegistryEntry>,
    /// Registries skipped because they were unreachable, with the error.
    pub unavailable: Vec<(String, RegistryError)>,
}

impl RegistrySet {
    pub fn new() -> Self {
        Self {
            registries: Vec::new(),
        }
    }

    /// Build a `RegistrySet` from a full `Config`. Convenience wrapper around
    /// [`Self::from_configs`] for the common case where the caller just wants
    /// the set defined by the user's global config.
    pub fn from_config(config: &hpm_config::Config) -> Result<Self, RegistryError> {
        Self::from_configs(&config.registries, &config.storage.registry_cache_dir)
    }

    /// Build a `RegistrySet` from registry configurations.
    ///
    /// Use this when the registry list is overridden (e.g. by a project
    /// manifest's `[[registries]]`) rather than coming from `Config`. For the
    /// straight `Config`-driven case, prefer [`Self::from_config`].
    ///
    /// All API registries are built without authentication. For caller-driven
    /// auth (e.g. a desktop client passing a bearer token for visibility-gated
    /// registries), use [`Self::from_configs_with_auth`].
    ///
    /// # Arguments
    /// * `registries` - Registry configurations to add
    /// * `registry_cache_dir` - Directory for caching git registry indices
    pub fn from_configs(
        registries: &[hpm_config::RegistrySourceConfig],
        registry_cache_dir: &std::path::Path,
    ) -> Result<Self, RegistryError> {
        Self::from_configs_with_auth(registries, registry_cache_dir, None)
    }

    /// Like [`Self::from_configs`], but attaches a bearer token to every API
    /// registry's HTTP client when `auth_token` is `Some`.
    ///
    /// Git registries ignore the token — there is no auth story for the git
    /// index today. When `auth_token` is `None`, behavior is identical to
    /// [`Self::from_configs`].
    ///
    /// A registry entry that cannot be constructed (bad URL, bad token) is a
    /// hard error: silently dropping it from the set would later surface as
    /// a misleading "package not found".
    pub fn from_configs_with_auth(
        registries: &[hpm_config::RegistrySourceConfig],
        registry_cache_dir: &std::path::Path,
        auth_token: Option<&str>,
    ) -> Result<Self, RegistryError> {
        let mut set = Self::new();

        for reg in registries {
            match reg.registry_type {
                hpm_config::RegistryType::Api => {
                    let api_reg = ApiRegistry::with_auth_token(&reg.name, &reg.url, auth_token)
                        .map_err(|e| {
                            RegistryError::ParseError(format!(
                                "Registry '{}' could not be constructed: {}",
                                reg.name, e
                            ))
                        })?;
                    set.add(Box::new(api_reg));
                }
                hpm_config::RegistryType::Git => {
                    let cache_dir = registry_cache_dir.join(&reg.name);
                    let git_reg = GitRegistry::new(&reg.name, &reg.url, &cache_dir);
                    set.add(Box::new(git_reg));
                }
            }
        }

        Ok(set)
    }

    pub fn add(&mut self, registry: Box<dyn Registry>) {
        self.registries.push(registry);
    }

    /// Search all registries and merge results.
    ///
    /// An unreachable registry does not abort the whole search (the healthy
    /// registries' results are still useful), but it is reported in
    /// [`SetSearchResults::unavailable`] so callers can tell the user the
    /// results may be incomplete instead of silently omitting them.
    pub async fn search(&self, query: &str) -> Result<SetSearchResults, RegistryError> {
        let mut all_packages = Vec::new();
        let mut unavailable = Vec::new();
        for registry in &self.registries {
            match registry.search(query).await {
                Ok(results) => all_packages.extend(results.packages),
                Err(e @ RegistryError::NetworkError(_)) => {
                    unavailable.push((registry.name().to_string(), e));
                }
                Err(e) => return Err(e),
            }
        }
        Ok(SetSearchResults {
            packages: all_packages,
            unavailable,
        })
    }

    /// Resolve a package name across all registries (first match wins).
    pub async fn get_versions(&self, name: &str) -> Result<Vec<RegistryEntry>, RegistryError> {
        for registry in &self.registries {
            match registry.get_versions(name).await {
                Ok(versions) if !versions.is_empty() => return Ok(versions),
                Ok(_) => continue,
                Err(RegistryError::PackageNotFound { .. }) => continue,
                Err(e) => return Err(e),
            }
        }
        Err(RegistryError::PackageNotFound {
            name: name.to_string(),
        })
    }

    /// Resolve a specific version across all registries (first match wins).
    pub async fn get_version(
        &self,
        name: &str,
        version: &str,
    ) -> Result<RegistryEntry, RegistryError> {
        for registry in &self.registries {
            match registry.get_version(name, version).await {
                Ok(entry) => return Ok(entry),
                Err(RegistryError::PackageNotFound { .. })
                | Err(RegistryError::VersionNotFound { .. }) => continue,
                Err(e) => return Err(e),
            }
        }
        Err(RegistryError::VersionNotFound {
            name: name.to_string(),
            version: version.to_string(),
        })
    }

    pub fn is_empty(&self) -> bool {
        self.registries.is_empty()
    }
}

impl Default for RegistrySet {
    fn default() -> Self {
        Self::new()
    }
}
