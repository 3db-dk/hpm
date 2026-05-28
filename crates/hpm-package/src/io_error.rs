//! Shared IO-error shape: an operation verb, the path it acted on, and the
//! underlying `std::io::Error`.
//!
//! Every crate above `hpm-package` raises IO failures that share the same
//! three coordinates ("we tried to *read* this *path* and got *this io
//! error*"). Each crate used to spell that out via its own custom variant —
//! `StorageError::DirectoryRead`, `ProjectError::ManifestIo`,
//! `DiscoveryError::DirectoryRead`, etc. — sometimes with the path, sometimes
//! with a stringified message, never consistently. `IoOp` consolidates the
//! shape so every variant carries the path + source error + verb, and Display
//! reads the same everywhere.

use std::fmt;
use std::path::PathBuf;

/// An IO failure: the operation verb, the path it acted on, and the
/// underlying `std::io::Error`.
///
/// Wrap one of these in your crate's error enum as the single IO-shaped
/// variant:
///
/// ```
/// use hpm_package::IoOp;
///
/// #[derive(Debug, thiserror::Error)]
/// pub enum MyError {
///     #[error(transparent)]
///     Io(#[from] IoOp),
/// }
///
/// // `?` lifts an `IoOp` straight into `MyError` via the `#[from]` impl.
/// fn check(path: &std::path::Path) -> Result<(), MyError> {
///     std::fs::metadata(path).map_err(|e| IoOp::wrap("stat", path, e))?;
///     Ok(())
/// }
/// ```
///
/// Construction at the call site stays terse via [`Self::wrap`]:
///
/// ```
/// use hpm_package::IoOp;
/// use std::path::PathBuf;
///
/// let missing = PathBuf::from("/definitely/not/here");
/// let err = std::fs::read_dir(&missing)
///     .map_err(|e| IoOp::wrap("read directory", &missing, e))
///     .unwrap_err();
/// assert!(err.to_string().starts_with("failed to read directory"));
/// ```
#[derive(Debug, thiserror::Error)]
pub struct IoOp {
    /// Verb describing what we were trying to do, in the third person ("read",
    /// "create", "remove", "copy"). Used verbatim in the Display impl.
    pub op: &'static str,
    /// The path the operation acted on. Displayed via `Path::display`.
    pub path: PathBuf,
    /// The underlying IO error. Set as the `#[source]` so error-chain walkers
    /// (anyhow, eyre, `Error::source`) see it.
    #[source]
    pub source: std::io::Error,
}

impl IoOp {
    /// Construct from the three coordinates. Prefer this over the struct
    /// literal at call sites — the `Into<PathBuf>` bound keeps `&Path` and
    /// `PathBuf` callers symmetric without explicit `.to_path_buf()`.
    pub fn wrap(op: &'static str, path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self {
            op,
            path: path.into(),
            source,
        }
    }
}

impl fmt::Display for IoOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "failed to {} {}", self.op, self.path.display())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;
    use std::io;
    use std::path::Path;

    fn sample(kind: io::ErrorKind, op: &'static str, path: &str) -> IoOp {
        IoOp::wrap(op, Path::new(path), io::Error::new(kind, "boom"))
    }

    #[test]
    fn wrap_accepts_path_ref_and_pathbuf() {
        // Both &Path and PathBuf should satisfy `Into<PathBuf>` — symmetry
        // across call sites is the whole point of the `Into` bound.
        let from_ref = IoOp::wrap(
            "read",
            Path::new("/tmp/a"),
            io::Error::from(io::ErrorKind::NotFound),
        );
        let from_owned = IoOp::wrap(
            "read",
            PathBuf::from("/tmp/a"),
            io::Error::from(io::ErrorKind::NotFound),
        );
        assert_eq!(from_ref.path, from_owned.path);
        assert_eq!(from_ref.op, from_owned.op);
    }

    #[test]
    fn display_format_is_failed_to_op_path() {
        let err = sample(io::ErrorKind::PermissionDenied, "create", "/var/protected");
        // Display must read "failed to <op> <path>" verbatim — downstream
        // crates `#[error(transparent)]` it, so the formatting here is the
        // user-facing error surface.
        assert_eq!(err.to_string(), "failed to create /var/protected");
    }

    #[test]
    fn source_returns_underlying_io_error() {
        let err = sample(io::ErrorKind::NotFound, "remove", "/missing");
        let source = err.source().expect("IoOp exposes its io::Error source");
        let io_err = source
            .downcast_ref::<io::Error>()
            .expect("source is std::io::Error");
        assert_eq!(io_err.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn from_into_lifts_iop_into_downstream_error() {
        // Mirrors how every consumer crate wires IoOp in: `#[from]` makes
        // `?` lift it. If this stops compiling, every call site breaks.
        #[derive(Debug, thiserror::Error)]
        enum DownstreamError {
            #[error(transparent)]
            Io(#[from] IoOp),
        }

        fn doit() -> Result<(), DownstreamError> {
            Err(sample(io::ErrorKind::Other, "open", "/x"))?;
            Ok(())
        }

        let err = doit().unwrap_err();
        assert!(matches!(err, DownstreamError::Io(_)));
        // Display must transparently forward to IoOp.
        assert_eq!(err.to_string(), "failed to open /x");
    }
}
