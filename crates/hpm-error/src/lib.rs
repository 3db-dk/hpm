use thiserror::Error;

#[derive(Debug, Error)]
pub enum HpmError {
    #[error("Package not found: {name}")]
    PackageNotFound { name: String },

    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Configuration error: {message}")]
    Config { message: String },

    #[error("Dependency resolution error: {message}")]
    Resolver { message: String },

    #[error("Installation error: {message}")]
    Install { message: String },
}
