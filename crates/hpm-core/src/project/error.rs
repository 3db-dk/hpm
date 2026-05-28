//! [`ProjectError`] — failures raised by
//! [`ProjectManager`](crate::project::ProjectManager) across install, sync,
//! manifest editing, and Houdini-package generation.

use crate::archive_fetcher::FetchError;
use crate::package_source::PackageSourceError;
use crate::registry::RegistryError;
use crate::storage::StorageError;
use hpm_package::{IoOp, ManifestLoadError};
use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum ProjectError {
    /// IO failure with the operation verb, path, and source `io::Error`.
    /// Subsumes the prior DirectoryCreation / DirectoryRead / ManifestIo
    /// variants — all three carried the same shape (op + path + io::Error).
    #[error(transparent)]
    Io(#[from] IoOp),

    #[error(transparent)]
    Manifest(#[from] ManifestLoadError),

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;
    use std::path::Path;

    /// IoOp → ProjectError via the derived `#[from]` impl. This is the
    /// hot path that replaced the prior `DirectoryCreation` / `DirectoryRead`
    /// / `ManifestIo` variants — every `?` on a filesystem call in the
    /// project layer now flows through here.
    #[test]
    fn from_io_op_lifts_to_io_variant() {
        let inner = IoOp::wrap(
            "create directory",
            Path::new("/tmp/nope"),
            io::Error::from(io::ErrorKind::PermissionDenied),
        );
        let err: ProjectError = inner.into();
        match err {
            ProjectError::Io(io_op) => {
                assert_eq!(io_op.op, "create directory");
                assert_eq!(io_op.path, PathBuf::from("/tmp/nope"));
                assert_eq!(io_op.source.kind(), io::ErrorKind::PermissionDenied);
            }
            other => panic!("expected ProjectError::Io, got {other:?}"),
        }
    }

    #[test]
    fn io_variant_display_is_transparent() {
        // `#[error(transparent)]` means Display must forward to IoOp.
        let err: ProjectError = IoOp::wrap(
            "read",
            Path::new("/x"),
            io::Error::from(io::ErrorKind::NotFound),
        )
        .into();
        assert_eq!(err.to_string(), "failed to read /x");
    }

    #[test]
    fn from_manifest_load_error_lifts_to_manifest_variant() {
        let inner = ManifestLoadError::NotFound {
            path: PathBuf::from("/tmp/hpm.toml"),
        };
        let err: ProjectError = inner.into();
        assert!(matches!(err, ProjectError::Manifest(_)));
    }

    #[test]
    fn from_package_source_error_lifts_to_invalid_package_source_variant() {
        let inner = PackageSourceError::InvalidUrl("ftp://nope".to_string());
        let err: ProjectError = inner.into();
        assert!(matches!(err, ProjectError::InvalidPackageSource(_)));
    }

    /// `StorageError` is boxed inside `ProjectError::Storage`. The hand-
    /// written `From<StorageError>` (see above) is what lets `?` stay
    /// terse at call sites — without it callers would have to box
    /// manually. Regression test for that contract.
    #[test]
    fn from_storage_error_boxes_into_storage_variant() {
        let inner = StorageError::PackageNotFound("studio/foo@1.0.0".to_string());
        let err: ProjectError = inner.into();
        match err {
            ProjectError::Storage(boxed) => {
                assert!(matches!(*boxed, StorageError::PackageNotFound(_)));
            }
            other => panic!("expected ProjectError::Storage, got {other:?}"),
        }
    }

    /// `FetchError` boxes the same way — see the `StorageError` test.
    #[test]
    fn from_fetch_error_boxes_into_fetch_variant() {
        let inner = FetchError::PathTraversalDetected("../../etc/passwd".to_string());
        let err: ProjectError = inner.into();
        match err {
            ProjectError::Fetch(boxed) => {
                assert!(matches!(*boxed, FetchError::PathTraversalDetected(_)));
            }
            other => panic!("expected ProjectError::Fetch, got {other:?}"),
        }
    }

    /// Smoke test that `?` on each sub-error converts cleanly in a real
    /// function body — guards against accidental removal of one of the
    /// `From` impls.
    #[test]
    fn question_mark_lifts_each_sub_error() {
        fn via_io() -> Result<(), ProjectError> {
            Err(IoOp::wrap(
                "open",
                Path::new("/x"),
                io::Error::from(io::ErrorKind::NotFound),
            ))?;
            Ok(())
        }
        fn via_manifest() -> Result<(), ProjectError> {
            Err(ManifestLoadError::NotFound {
                path: PathBuf::from("/x"),
            })?;
            Ok(())
        }
        fn via_pkg_source() -> Result<(), ProjectError> {
            Err(PackageSourceError::InvalidVersion("".to_string()))?;
            Ok(())
        }
        fn via_storage() -> Result<(), ProjectError> {
            Err(StorageError::NotImplemented("x".to_string()))?;
            Ok(())
        }
        fn via_fetch() -> Result<(), ProjectError> {
            Err(FetchError::ExtractionError("bad".to_string()))?;
            Ok(())
        }

        assert!(matches!(via_io(), Err(ProjectError::Io(_))));
        assert!(matches!(via_manifest(), Err(ProjectError::Manifest(_))));
        assert!(matches!(
            via_pkg_source(),
            Err(ProjectError::InvalidPackageSource(_))
        ));
        assert!(matches!(via_storage(), Err(ProjectError::Storage(_))));
        assert!(matches!(via_fetch(), Err(ProjectError::Fetch(_))));
    }
}
