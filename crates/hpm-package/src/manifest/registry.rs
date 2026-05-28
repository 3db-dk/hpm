//! Registry references inside `hpm.toml`'s `[[registries]]` array.

use serde::{Deserialize, Serialize};

/// The type of registry backend.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RegistryType {
    Api,
    Git,
}

/// A registry declared in hpm.toml's `[[registries]]` array.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryConfig {
    pub name: String,
    pub url: String,
    #[serde(rename = "type")]
    pub registry_type: RegistryType,
}
