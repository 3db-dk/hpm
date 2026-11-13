//! [`StorageError`] — failures raised by the global package store.

use crate::discovery::DiscoveryError;
use crate::graph::DependencyError;
use hpm_package::{IoOp, ManifestLoadError};

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    /// A package name or version that cannot name a directory.
    #[error(transparent)]
    InvalidCoordinate(#[from] crate::storage::layout::InvalidCoordinate),

    /// IO failure with the operation verb, path, and source `io::Error`.
    /// Subsumes the prior DirectoryCreation/DirectoryRead/DirectoryRemoval/
    /// MetadataRead variants.
    #[error(transparent)]
    Io(#[from] IoOp),

    #[error(transparent)]
    Manifest(#[from] ManifestLoadError),

    /// Building a dev copy's file set from the workspace's ignore files and
    /// `[stage]` rules failed (bad glob, unreadable `.hpmignore`, walk error).
    #[error("Failed to stage dev copy: {0}")]
    Stage(#[from] crate::packer::PackError),

    #[error("Package not found: {0}")]
    PackageNotFound(String),

    /// The store keys a registry install on `<slug>@<version>`, and that
    /// folder already holds another creator's package of the same slug and
    /// version. Installing would hand back the wrong package.
    #[error(
        "Cannot install {wanted}@{version}: the store already holds {installed}@{version} \
         under the same name ({}), and packages are stored by slug. Remove it \
         (hpm clean, once nothing uses it) or use one of the two packages.",
        path.display()
    )]
    SlugCollision {
        wanted: String,
        installed: String,
        version: String,
        path: std::path::PathBuf,
    },

    #[error(transparent)]
    ProjectDiscovery(#[from] DiscoveryError),

    #[error(transparent)]
    DependencyResolution(Box<DependencyError>),

    /// Boxed for the same size reason as `DependencyResolution`.
    #[error("Python cleanup failed: {0}")]
    PythonCleanup(#[source] Box<crate::python::PythonError>),

    #[error(
        "{message}. Refusing to clean up — without the global ledger, \
         globally installed packages would look unreferenced and be deleted."
    )]
    GlobalLedger { message: String },

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

impl From<crate::python::PythonError> for StorageError {
    fn from(e: crate::python::PythonError) -> Self {
        StorageError::PythonCleanup(Box::new(e))
    }
}

impl From<DependencyError> for StorageError {
    fn from(err: DependencyError) -> Self {
        Self::DependencyResolution(Box::new(err))
    }
}
