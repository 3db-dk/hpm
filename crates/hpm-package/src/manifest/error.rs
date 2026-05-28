//! Errors raised while loading a [`PackageManifest`](super::PackageManifest)
//! from disk.

use std::path::PathBuf;

/// Errors that can occur while loading a [`super::PackageManifest`] from disk.
///
/// Each variant carries the source path so error messages stay actionable
/// when manifest loading is buried inside multi-package operations
/// (`list_installed`, registry installs, project sync).
#[derive(Debug, thiserror::Error)]
pub enum ManifestLoadError {
    #[error("manifest not found: {}", .path.display())]
    NotFound { path: PathBuf },

    #[error("failed to read manifest at {}: {source}", .path.display())]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to parse manifest at {}: {source}", .path.display())]
    Parse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },
}
