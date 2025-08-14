//! Registry-specific error types

use thiserror::Error;

#[derive(Error, Debug)]
pub enum RegistryError {
    #[error("Authentication failed: {0}")]
    AuthenticationFailed(String),

    #[error("Package not found: {name}")]
    PackageNotFound { name: String },

    #[error("Package version not found: {name}@{version}")]
    VersionNotFound { name: String, version: String },

    #[error("Package already exists: {name}@{version}")]
    PackageAlreadyExists { name: String, version: String },

    #[error("Invalid package data: {0}")]
    InvalidPackageData(String),

    #[error("Checksum mismatch: expected {expected}, got {actual}")]
    ChecksumMismatch { expected: String, actual: String },

    #[error("Package too large: {size} bytes (max: {max_size})")]
    PackageTooLarge { size: u64, max_size: u64 },

    #[error("Network error: {0}")]
    Network(#[from] tonic::transport::Error),

    #[error("gRPC error: {0}")]
    Grpc(#[from] tonic::Status),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Compression error: {0}")]
    Compression(String),

    #[error("Rate limit exceeded")]
    RateLimitExceeded,

    #[error("Insufficient permissions: {required}")]
    InsufficientPermissions { required: String },

    #[error("Internal server error: {0}")]
    Internal(String),
}

impl From<RegistryError> for tonic::Status {
    fn from(err: RegistryError) -> Self {
        match err {
            RegistryError::AuthenticationFailed(msg) => tonic::Status::unauthenticated(msg),
            RegistryError::PackageNotFound { .. } => tonic::Status::not_found(err.to_string()),
            RegistryError::VersionNotFound { .. } => tonic::Status::not_found(err.to_string()),
            RegistryError::PackageAlreadyExists { .. } => {
                tonic::Status::already_exists(err.to_string())
            }
            RegistryError::InvalidPackageData(msg) => tonic::Status::invalid_argument(msg),
            RegistryError::ChecksumMismatch { .. } => {
                tonic::Status::invalid_argument(err.to_string())
            }
            RegistryError::PackageTooLarge { .. } => {
                tonic::Status::invalid_argument(err.to_string())
            }
            RegistryError::RateLimitExceeded => {
                tonic::Status::resource_exhausted("Rate limit exceeded")
            }
            RegistryError::InsufficientPermissions { .. } => {
                tonic::Status::permission_denied(err.to_string())
            }
            RegistryError::Grpc(status) => status,
            _ => tonic::Status::internal(err.to_string()),
        }
    }
}
