//! Registry source configuration: how HPM reaches each upstream (API endpoint
//! or git remote).

use serde::{Deserialize, Serialize};

// One registry-backend enum for the whole workspace — the manifest's
// `[[registries]]` and the config's registry list describe the same concept.
pub use hpm_package::RegistryType;

/// Configuration for a single package registry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistrySourceConfig {
    /// Display name / alias for this registry
    pub name: String,
    /// URL: API base URL or git remote URL
    pub url: String,
    /// Registry type: "api" or "git"
    #[serde(rename = "type")]
    pub registry_type: RegistryType,
}
