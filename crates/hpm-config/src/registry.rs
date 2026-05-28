//! Registry source configuration: how HPM reaches each upstream (API endpoint
//! or git remote).

use serde::{Deserialize, Serialize};

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

/// The type of registry backend.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RegistryType {
    Api,
    Git,
}
