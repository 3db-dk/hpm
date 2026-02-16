//! Registry type definitions.
//!
//! Types representing package entries in a registry, registry configuration,
//! and dependency specifications within registry entries.

use serde::{Deserialize, Serialize};

/// A single version entry in the registry.
///
/// Each published version of a package has one entry. For git-based registries,
/// these are stored as one JSON object per line in the package index file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryEntry {
    /// Package name
    pub name: String,
    /// Version string (semver)
    #[serde(rename = "vers")]
    pub version: String,
    /// Dependencies
    #[serde(default)]
    pub deps: Vec<RegistryDependency>,
    /// SHA-256 checksum of the package archive (hex-encoded, prefixed with "sha256:")
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cksum: Option<String>,
    /// Download URL for the package archive
    pub dl: String,
    /// Ed25519 signature of the checksum (prefixed with "ed25519:")
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sig: Option<String>,
    /// Key ID used for signing
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kid: Option<String>,
    /// Houdini version compatibility range (e.g., ">=20.0,<22.0")
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub houdini_compat: Option<String>,
    /// Whether this version has been yanked
    #[serde(default)]
    pub yanked: bool,
    /// Package description (optional, mainly for search results)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Package author(s) (optional, mainly for search results)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    /// Package license (optional)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
}

/// A dependency listed in a registry entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryDependency {
    /// Dependency package name
    pub name: String,
    /// Version requirement (e.g., ">=2.0", "^1.5")
    pub req: String,
    /// Whether this is an optional dependency
    #[serde(default)]
    pub optional: bool,
}

/// Registry configuration, served at the root of a registry.
///
/// For git registries this is `config.json` at the repo root.
/// For API registries this is served at `GET /config`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryConfig {
    /// Human-readable name of the registry
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// API base URL (for registries that support HTTP access)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api: Option<String>,
    /// URL to fetch public signing keys
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub public_keys_url: Option<String>,
}

/// Search results returned from a registry search.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResults {
    /// Matching entries (latest version per package)
    pub packages: Vec<RegistryEntry>,
    /// Total number of matches (may be more than returned)
    pub total: usize,
}

impl RegistryEntry {
    /// Returns the SHA-256 checksum without the "sha256:" prefix, if present.
    pub fn checksum_hex(&self) -> Option<&str> {
        self.cksum
            .as_deref()
            .map(|c| c.strip_prefix("sha256:").unwrap_or(c))
    }
}
