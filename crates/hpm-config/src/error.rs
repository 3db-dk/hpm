//! Error type for configuration loading and persistence.

use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("Failed to read config file: {path}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("Failed to parse config file: {path}")]
    Parse {
        path: PathBuf,
        #[source]
        source: Box<toml::de::Error>,
    },

    #[error("Failed to serialize config")]
    Serialize(#[from] toml::ser::Error),

    #[error("Failed to write config file: {path}")]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}
