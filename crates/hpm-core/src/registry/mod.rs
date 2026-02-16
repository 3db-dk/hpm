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
use thiserror::Error;

pub use api::ApiRegistry;
pub use git::GitRegistry;
pub use types::{RegistryConfig, RegistryDependency, RegistryEntry, SearchResults};

/// Errors that can occur during registry operations.
#[derive(Error, Debug)]
pub enum RegistryError {
    #[error("Package '{name}' not found in registry")]
    PackageNotFound { name: String },

    #[error("Version '{version}' of package '{name}' not found in registry")]
    VersionNotFound { name: String, version: String },

    #[error("Failed to connect to registry: {0}")]
    NetworkError(#[from] reqwest::Error),

    #[error("Failed to parse registry data: {0}")]
    ParseError(String),

    #[error("Registry I/O error: {0}")]
    IoError(#[from] std::io::Error),

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

    /// Get the registry configuration/metadata.
    async fn config(&self) -> Result<RegistryConfig, RegistryError>;

    /// Get the registry display name.
    fn name(&self) -> &str;
}

/// A collection of registries that can be searched in order.
pub struct RegistrySet {
    registries: Vec<Box<dyn Registry>>,
}

impl RegistrySet {
    pub fn new() -> Self {
        Self {
            registries: Vec::new(),
        }
    }

    pub fn add(&mut self, registry: Box<dyn Registry>) {
        self.registries.push(registry);
    }

    /// Search all registries and merge results.
    pub async fn search(&self, query: &str) -> Result<SearchResults, RegistryError> {
        let mut all_packages = Vec::new();
        for registry in &self.registries {
            match registry.search(query).await {
                Ok(results) => all_packages.extend(results.packages),
                Err(RegistryError::NetworkError(_)) => continue, // skip unavailable registries
                Err(e) => return Err(e),
            }
        }
        let total = all_packages.len();
        Ok(SearchResults {
            packages: all_packages,
            total,
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

    /// Refresh all registries.
    pub async fn refresh_all(&self) -> Result<(), RegistryError> {
        for registry in &self.registries {
            registry.refresh().await?;
        }
        Ok(())
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
