//! [`PythonError`] — failures raised by the Python dependency subsystem:
//! bundled-uv bootstrap, Houdini→Python ABI mapping, dependency resolution,
//! and venv management.

use hpm_package::IoOp;
use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum PythonError {
    /// IO failure with the operation verb, path, and source `io::Error`.
    #[error(transparent)]
    Io(#[from] IoOp),

    /// The user's home directory could not be determined — typically because
    /// `$HOME` (Unix) or `%USERPROFILE%` (Windows) is unset in the invoking
    /// environment.
    #[error(
        "Could not locate the user's home directory ({}). HPM stores \
         its tools, caches, and venvs under ~/.hpm — set the variable \
         or run from a shell where it is available.",
        if cfg!(windows) { "%USERPROFILE%" } else { "$HOME" }
    )]
    HomeDirNotFound,

    /// No UV release archive exists for the current OS/arch combination.
    #[error("UV is not available for this platform")]
    UnsupportedPlatform,

    /// Network-level failure downloading the UV release archive.
    #[error("Failed to download UV from {url}")]
    UvDownload {
        url: String,
        #[source]
        source: reqwest::Error,
    },

    /// The UV download endpoint answered with a non-success HTTP status.
    #[error("Failed to download UV: HTTP {status} from {url}")]
    UvDownloadStatus {
        url: String,
        status: reqwest::StatusCode,
    },

    /// The UV release archive has an extension we don't know how to unpack.
    #[error("Unknown archive format: {url}")]
    UnknownArchiveFormat { url: String },

    /// The UV zip release archive could not be read.
    #[error("Failed to read UV zip archive")]
    Zip(#[from] zip::result::ZipError),

    /// The UV release archive was readable but did not contain the uv binary.
    #[error("UV binary not found in archive")]
    UvBinaryNotInArchive,

    /// Extraction claimed success but the binary is not on disk afterwards.
    #[error("UV binary not found after extraction at {}", path.display())]
    UvBinaryMissing { path: PathBuf },

    /// A uv invocation exited non-zero. `context` names the operation being
    /// attempted (defaults to the uv arg list); `stderr` is uv's own
    /// diagnostic output, which carries the actionable detail (e.g. the
    /// conflicting requirement pair on a resolution failure).
    #[error("{context}: {stderr}")]
    UvCommand { context: String, stderr: String },

    /// A Python version string didn't parse as `major[.minor[.patch]]`.
    #[error("Invalid Python version '{input}': {reason}")]
    InvalidPythonVersion { input: String, reason: String },

    /// A Houdini version string could not be parsed for ABI mapping.
    #[error(
        "Could not parse Houdini version '{input}': expected a numeric \
         major version (e.g. '21' or '20.5')"
    )]
    HoudiniVersionParse { input: String },

    /// A parseable Houdini version with no known Python ABI mapping —
    /// either past-EOL or an unrecognised future major.
    #[error(
        "No Python version mapping for Houdini {version}; supported versions are 20.5+, 21, 22. \
         Houdini 19.x (Python 3.7) and 20.0–20.4 (Python 3.9) are past EOL."
    )]
    UnsupportedHoudiniVersion { version: String },

    /// Two manifests pin the same Python package to different versions.
    #[error("Conflicting versions for package {package}: {existing} vs {requested}")]
    DependencyVersionConflict {
        package: String,
        existing: String,
        requested: String,
    },

    /// Two manifests imply different Python ABIs.
    #[error("Conflicting Python versions: {existing} vs {requested}")]
    PythonVersionConflict { existing: String, requested: String },

    /// Temp requirements-file plumbing failed. `NamedTempFile` has no stable
    /// path to report, so this carries the operation instead of an `IoOp`.
    #[error("Failed to {op} temporary requirements file")]
    RequirementsFile {
        op: &'static str,
        #[source]
        source: std::io::Error,
    },

    /// A venv path contains non-UTF-8 that uv's CLI cannot receive.
    #[error("Venv Python path is not UTF-8: {}", path.display())]
    NonUtf8Path { path: PathBuf },

    /// uv exited zero but site-packages doesn't contain the packages we
    /// asked for — the guard that would have caught the `--target` install
    /// regression loudly instead of silently.
    #[error(
        "uv reported success but {} is missing the installed packages",
        site_packages.display()
    )]
    VenvVerification { site_packages: PathBuf },

    /// metadata.json exists but doesn't deserialize (schema drift across
    /// hpm versions is handled by the staleness check; this surfaces only
    /// where a parse failure is a hard error).
    #[error("Failed to parse venv metadata at {}", path.display())]
    MetadataParse {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },

    /// Venv metadata failed to serialize.
    #[error("Failed to serialize venv metadata")]
    MetadataSerialize(#[source] serde_json::Error),

    /// A `spawn_blocking` task panicked or was cancelled.
    #[error("Background task failed")]
    TaskJoin(#[from] tokio::task::JoinError),
}

impl PythonError {
    /// Replace the caller-facing context on a [`UvCommand`](Self::UvCommand)
    /// failure ("Failed to install managed Python 3.11", ...). Other variants
    /// already carry their own coordinates and pass through unchanged.
    pub(crate) fn uv_context(self, context: impl Into<String>) -> Self {
        match self {
            Self::UvCommand { stderr, .. } => Self::UvCommand {
                context: context.into(),
                stderr,
            },
            other => other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn uv_context_rewrites_only_uv_command() {
        let err = PythonError::UvCommand {
            context: "uv pip install".to_string(),
            stderr: "boom".to_string(),
        }
        .uv_context("Failed to install packages into virtual environment");
        assert_eq!(
            err.to_string(),
            "Failed to install packages into virtual environment: boom"
        );

        let untouched = PythonError::UnsupportedPlatform.uv_context("ignored");
        assert!(matches!(untouched, PythonError::UnsupportedPlatform));
    }

    #[test]
    fn io_variant_display_is_transparent() {
        let err: PythonError = IoOp::wrap(
            "read",
            Path::new("/x"),
            std::io::Error::from(std::io::ErrorKind::NotFound),
        )
        .into();
        assert_eq!(err.to_string(), "failed to read /x");
    }

    #[test]
    fn home_dir_message_names_the_platform_variable() {
        let msg = PythonError::HomeDirNotFound.to_string();
        if cfg!(windows) {
            assert!(msg.contains("%USERPROFILE%"), "{msg}");
        } else {
            assert!(msg.contains("$HOME"), "{msg}");
        }
    }
}
