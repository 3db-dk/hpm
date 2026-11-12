//! Registry references inside `hpm.toml`'s `[[registries]]` array.

use serde::{Deserialize, Serialize};

/// The type of registry backend.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RegistryType {
    Api,
    Git,
}

impl std::str::FromStr for RegistryType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "api" => Ok(Self::Api),
            "git" => Ok(Self::Git),
            other => Err(format!(
                "unknown registry type '{other}'; expected 'api' or 'git'"
            )),
        }
    }
}

/// A registry declared in hpm.toml's `[[registries]]` array.
#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegistryConfig {
    pub name: String,
    pub url: String,
    #[serde(rename = "type")]
    pub registry_type: RegistryType,
}
