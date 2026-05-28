//! [`ProjectError`] — failures raised by [`ProjectManager`] across install,
//! sync, manifest editing, and Houdini-package generation.

use crate::archive_fetcher::FetchError;
use crate::package_source::PackageSourceError;
use crate::registry::RegistryError;
use crate::storage::StorageError;
use hpm_package::ManifestLoadError;
use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum ProjectError {
    /// Failed to create a directory the project depends on (`.hpm/packages`,
    /// fetcher cache, etc.). Carries the typed `io::Error` so callers can
    /// match on `ErrorKind` (e.g. `PermissionDenied`).
    #[error("Failed to create directory {}", path.display())]
    DirectoryCreation {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// Failed to read a directory the project depends on.
    #[error("Failed to read directory {}", path.display())]
    DirectoryRead {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error(transparent)]
    Manifest(#[from] ManifestLoadError),

    /// I/O failure on a manifest file we own (hpm.toml or a per-package
    /// Houdini JSON). `op` is a verb like "read", "write", or "remove".
    #[error("Failed to {op} {}", path.display())]
    ManifestIo {
        op: &'static str,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// hpm.toml could not be parsed as an editable TOML document. Distinct
    /// from `Manifest(ManifestLoadError::Parse)` because the edit paths
    /// (`update_project_manifest`, `remove_from_project_manifest`) use
    /// `toml_edit::DocumentMut`, which carries its own error type.
    #[error("Failed to parse {} as editable TOML", path.display())]
    ManifestEdit {
        path: PathBuf,
        #[source]
        source: toml_edit::TomlError,
    },

    /// hpm.toml has the wrong structure for the operation (e.g.
    /// `[dependencies]` exists but is not a table).
    #[error("{}: {message}", path.display())]
    ManifestStructure { path: PathBuf, message: String },

    /// Failed to serialise a Houdini package.json.
    #[error("Failed to serialise Houdini manifest at {}", path.display())]
    HoudiniManifestSerialize {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },

    /// Global package storage CAS read/write failure. Boxed because
    /// `StorageError` is large; keeps `ProjectError` itself small enough
    /// that `Result<T, ProjectError>` stays cheap to return on the hot
    /// success path.
    #[error(transparent)]
    Storage(Box<StorageError>),

    /// Archive download / extract failure. Boxed; see `Storage`.
    #[error(transparent)]
    Fetch(Box<FetchError>),

    /// A package source URL could not be parsed.
    #[error(transparent)]
    InvalidPackageSource(#[from] PackageSourceError),

    /// Dependency requested but no registries are configured.
    #[error("Cannot install {name} {version_req}: no registries configured")]
    NoRegistriesConfigured { name: String, version_req: String },

    /// Registry lookup failed for `name@version_req`. Source is boxed;
    /// see `Storage`.
    #[error("Failed to resolve {name} {version_req} from registry")]
    RegistryResolution {
        name: String,
        version_req: String,
        #[source]
        source: Box<RegistryError>,
    },

    /// Registry returned versions, but none satisfied the requirement.
    #[error("No version of {name} matches requirement {version_req}")]
    NoMatchingVersion { name: String, version_req: String },

    /// Python dependency collection / resolution / venv creation failed.
    /// `hpm-python` returns `anyhow::Error`; we box the source rather than
    /// pull in anyhow at this layer.
    #[error("Python dependency resolution failed")]
    PythonResolution(#[source] Box<dyn std::error::Error + Send + Sync>),

    #[error(
        "Required env var '{var}' for package '{package}' has no value. \
         Set it in this project's [runtime] section in hpm.toml."
    )]
    MissingRequiredEnv { var: String, package: String },

    #[error("Invalid conditional value for env var '{var}' in package '{package}': {message}")]
    InvalidEnvExpression {
        var: String,
        package: String,
        message: String,
    },
    // (`[compat].houdini` is now a `HoudiniRange` newtype that validates
    // at deserialize time, so the prior `InvalidHoudiniCompat` variant is
    // unreachable and has been removed.)
}

// Hand-written so call sites can `?` from the unboxed source error types.
// thiserror's `#[from]` would only generate `From<Box<X>>`; we want the
// boxing to be invisible at the use site.
impl From<StorageError> for ProjectError {
    fn from(err: StorageError) -> Self {
        Self::Storage(Box::new(err))
    }
}

impl From<FetchError> for ProjectError {
    fn from(err: FetchError) -> Self {
        Self::Fetch(Box::new(err))
    }
}
