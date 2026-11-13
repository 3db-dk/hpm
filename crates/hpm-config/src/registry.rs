//! Registry source configuration: how HPM reaches each upstream (API endpoint
//! or git remote).

use serde::{Deserialize, Serialize};

// One registry-backend enum for the whole workspace — the manifest's
// `[[registries]]` and the config's registry list describe the same concept.
pub use hpm_package::RegistryType;

impl From<&hpm_package::RegistryConfig> for RegistrySourceConfig {
    /// A manifest's `[[registries]]` entry as a registry source.
    fn from(manifest: &hpm_package::RegistryConfig) -> Self {
        Self {
            name: manifest.name.clone(),
            url: manifest.url.clone(),
            registry_type: manifest.registry_type.clone(),
        }
    }
}

/// Configuration for a single package registry.
///
/// The url is checked against its backend when the config is loaded
/// ([`hpm_package::validate_registry_url`]), so a bad entry fails naming the
/// registry instead of reaching `git`.
#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, try_from = "RawRegistrySourceConfig")]
pub struct RegistrySourceConfig {
    /// Display name / alias for this registry
    pub name: String,
    /// URL: API base URL or git remote URL
    pub url: String,
    /// Registry type: "api" or "git"
    #[serde(rename = "type")]
    pub registry_type: RegistryType,
}

/// The unchecked on-disk shape of a [`RegistrySourceConfig`].
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRegistrySourceConfig {
    name: String,
    url: String,
    #[serde(rename = "type")]
    registry_type: RegistryType,
}

impl TryFrom<RawRegistrySourceConfig> for RegistrySourceConfig {
    type Error = String;

    fn try_from(raw: RawRegistrySourceConfig) -> Result<Self, Self::Error> {
        hpm_package::validate_registry_url(&raw.registry_type, &raw.url)
            .map_err(|e| format!("registry '{}': {e}", raw.name))?;
        Ok(Self {
            name: raw.name,
            url: raw.url,
            registry_type: raw.registry_type,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hostile_git_url_fails_the_config_load_and_a_normal_one_loads() {
        let load = |url: &str| {
            toml::from_str::<RegistrySourceConfig>(&format!(
                "name = \"studio\"\nurl = '{url}'\ntype = \"git\"\n"
            ))
        };
        let err = load("--upload-pack=x").unwrap_err().to_string();
        assert!(err.contains("registry 'studio'"), "{err}");
        assert!(load("ext::sh -c x").is_err());
        assert!(load("git@github.com:studio/registry.git").is_ok());
        assert!(load("/mnt/nas/registry.git").is_ok());
    }
}
