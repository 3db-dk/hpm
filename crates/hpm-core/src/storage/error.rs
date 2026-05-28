//! [`StorageError`] — failures raised by the global package store.

use crate::dependency::DependencyError;
use crate::discovery::DiscoveryError;
use hpm_package::{IoOp, ManifestLoadError};

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    /// IO failure with the operation verb, path, and source `io::Error`.
    /// Subsumes the prior DirectoryCreation/DirectoryRead/DirectoryRemoval/
    /// MetadataRead variants.
    #[error(transparent)]
    Io(#[from] IoOp),

    #[error(transparent)]
    Manifest(#[from] ManifestLoadError),

    #[error("Package not found: {0}")]
    PackageNotFound(String),

    #[error("Feature not implemented: {0}")]
    NotImplemented(String),

    #[error(transparent)]
    ProjectDiscovery(#[from] DiscoveryError),

    #[error(transparent)]
    DependencyResolution(Box<DependencyError>),

    #[error("Python cleanup failed: {0}")]
    PythonCleanup(String),

    #[error(
        "Package {name}@{version} is in use by another process; close any \
         running Houdini that depends on it and try again ({source})"
    )]
    PackageInUse {
        name: String,
        version: String,
        #[source]
        source: std::io::Error,
    },
}

impl From<DependencyError> for StorageError {
    fn from(err: DependencyError) -> Self {
        Self::DependencyResolution(Box::new(err))
    }
}
