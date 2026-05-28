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
/// ```ignore
/// #[derive(Debug, thiserror::Error)]
/// pub enum MyError {
///     #[error(transparent)]
///     Io(#[from] hpm_package::io_error::IoOp),
///     // ...
/// }
/// ```
///
/// Construction at the call site stays terse via [`Self::wrap`]:
///
/// ```ignore
/// std::fs::read_dir(&path).map_err(|e| IoOp::wrap("read directory", &path, e))?;
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
