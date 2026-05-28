//! [`StorageError`] — failures raised by the global package store.

use hpm_package::ManifestLoadError;

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("Directory creation failed: {0}")]
    DirectoryCreation(#[source] std::io::Error),

    #[error("Directory read failed: {0}")]
    DirectoryRead(String),

    #[error("Directory removal failed: {0}")]
    DirectoryRemoval(#[source] std::io::Error),

    #[error(transparent)]
    Manifest(#[from] ManifestLoadError),

    #[error("Metadata read failed: {0}")]
    MetadataRead(#[source] std::io::Error),

    #[error("Package not found: {0}")]
    PackageNotFound(String),

    #[error("Feature not implemented: {0}")]
    NotImplemented(String),

    #[error("Project discovery failed: {0}")]
    ProjectDiscovery(String),

    #[error("Dependency resolution failed: {0}")]
    DependencyResolution(String),

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
