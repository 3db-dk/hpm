use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub registry: RegistryConfig,
    pub install: InstallConfig,
    pub auth: Option<AuthConfig>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RegistryConfig {
    pub default: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct InstallConfig {
    pub path: String,
    pub parallel_downloads: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AuthConfig {
    pub token: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            registry: RegistryConfig {
                default: "https://packages.houdini.org".to_string(),
            },
            install: InstallConfig {
                path: "packages/hpm".to_string(),
                parallel_downloads: 8,
            },
            auth: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_default_values() {
        let config = Config::default();
        assert_eq!(config.registry.default, "https://packages.houdini.org");
        assert_eq!(config.install.path, "packages/hpm");
        assert_eq!(config.install.parallel_downloads, 8);
        assert!(config.auth.is_none());
    }

    #[test]
    fn config_serialization() {
        let config = Config::default();
        let serialized = serde_json::to_string(&config).unwrap();
        let deserialized: Config = serde_json::from_str(&serialized).unwrap();

        assert_eq!(config.registry.default, deserialized.registry.default);
        assert_eq!(config.install.path, deserialized.install.path);
    }
}
