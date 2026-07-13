//! Package archiving, checksumming, and signing.
//!
//! Produces a `{name}-{version}.zip` from a package directory, with SHA-256
//! checksum and optional Ed25519 signature. Split into:
//!
//! - `stage_filter` — `[stage]` filtering, ignore rules, and the staging walk
//! - `archive` — zip creation
//! - `signing` — checksums and Ed25519 signing
//!
//! Everything public re-exports here, so `crate::packer::*` paths are stable.

use hpm_package::IoOp;
use hpm_package::manifest::StageConfig;
use hpm_package::platform::Platform;
use std::path::Path;

mod archive;
mod signing;
mod stage_filter;

pub use archive::{PackResult, create_archive};
pub use signing::{
    SigningKey, compute_archive_checksum, compute_bytes_checksum, load_signing_key,
    load_signing_key_from_pem, sign_archive, sign_bytes,
};
pub use stage_filter::{StageFilter, build_ignore_rules, stage_to_dir};

/// Errors from packing operations.
#[derive(Debug, thiserror::Error)]
pub enum PackError {
    #[error(transparent)]
    Io(#[from] IoOp),

    #[error("Zip error: {0}")]
    Zip(#[from] zip::result::ZipError),

    #[error("Invalid signing key: {0}")]
    SigningKey(String),

    #[error("Ignore pattern error: {0}")]
    IgnorePattern(#[from] ignore::Error),

    #[error("Invalid glob pattern: {0}")]
    GlobPattern(String),
}

/// Pack a package directory into a signed, checksummed archive.
pub fn pack(
    package_dir: &Path,
    name: &str,
    version: &str,
    output_dir: &Path,
    signing_key: Option<&SigningKey>,
    platform: Option<&Platform>,
    stage_config: &StageConfig,
    inject_files: &[(String, Vec<u8>)],
) -> Result<PackResult, PackError> {
    let ignore = build_ignore_rules(package_dir)?;

    let stage_filter = StageFilter::new(stage_config, platform)?;

    let archive_path = create_archive(
        package_dir,
        name,
        version,
        output_dir,
        &ignore,
        platform,
        Some(&stage_filter),
        inject_files,
    )?;
    let checksum = compute_archive_checksum(&archive_path)?;

    let (signature, key_id) = match signing_key {
        Some(key) => {
            let (sig, kid) = sign_archive(&archive_path, key)?;
            (Some(sig), Some(kid))
        }
        None => (None, None),
    };

    Ok(PackResult {
        archive_path,
        checksum,
        signature,
        key_id,
        platform: platform.map(|p| p.to_string()),
    })
}

#[cfg(test)]
mod tests;
