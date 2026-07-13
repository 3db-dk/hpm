//! Zip archive creation for `hpm pack`.

use hpm_package::IoOp;
use hpm_package::platform::Platform;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use zip::DateTime;
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

use super::PackError;
use super::stage_filter::{StageFilter, collect_stage_entries};

/// Result of a successful pack operation.
#[derive(Debug)]
pub struct PackResult {
    /// Path to the created archive.
    pub archive_path: PathBuf,
    /// Hex-encoded SHA-256 checksum of the archive.
    pub checksum: String,
    /// Base64-encoded Ed25519 signature (if signing key provided).
    pub signature: Option<String>,
    /// Hex-encoded first 8 bytes of the public key (if signing key provided).
    pub key_id: Option<String>,
    /// Platform tag if this is a platform-specific archive.
    pub platform: Option<String>,
}

/// Create a zip archive of the package directory, filtering via ignore
/// rules and `[stage]`.
///
/// Files are added in sorted order for deterministic output. When a `[stage]`
/// filter is supplied, each file's archive path is rewritten via the
/// matching `place` rule; unmatched files ship at their workspace-relative
/// path.
pub fn create_archive(
    package_dir: &Path,
    name: &str,
    version: &str,
    output_dir: &Path,
    ignore: &ignore::gitignore::Gitignore,
    platform: Option<&Platform>,
    stage_filter: Option<&StageFilter>,
    inject_files: &[(String, Vec<u8>)],
) -> Result<PathBuf, PackError> {
    // Replace `/` with `-` in package name for flat archive filenames
    let safe_name = name.replace('/', "-");
    let archive_name = match platform {
        Some(p) => format!("{}-{}-{}.zip", safe_name, version, p),
        None => format!("{}-{}.zip", safe_name, version),
    };
    let archive_path = output_dir.join(&archive_name);

    // Collect (source path, archive path) pairs, sorted for determinism.
    let entries = collect_stage_entries(package_dir, ignore, stage_filter, None)?;

    // Create zip
    let file = fs::File::create(&archive_path)
        .map_err(|e| IoOp::wrap("create archive", &archive_path, e))?;
    let mut zip = ZipWriter::new(file);
    // Pin the entry timestamp to a fixed value. With the `time` feature,
    // `SimpleFileOptions::default()` stamps each entry with the current time,
    // which (at the 2-second MS-DOS resolution) makes two packs of the same
    // tree produce different bytes — and thus different checksums — whenever
    // they straddle a 2-second boundary. A fixed epoch keeps archives
    // byte-for-byte reproducible, which the package checksum relies on.
    let options = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .last_modified_time(DateTime::default());

    for (source, archive_name) in &entries {
        zip.start_file(archive_name.as_str(), options)?;
        let mut f =
            fs::File::open(source).map_err(|e| IoOp::wrap("open source file", source, e))?;
        let mut buf = Vec::new();
        f.read_to_end(&mut buf)
            .map_err(|e| IoOp::wrap("read source file", source, e))?;
        zip.write_all(&buf)
            .map_err(|e| IoOp::wrap("write zip entry to", &archive_path, e))?;
    }

    // Write injected files
    for (name, content) in inject_files {
        zip.start_file(name.as_str(), options)?;
        zip.write_all(content)
            .map_err(|e| IoOp::wrap("write injected zip entry to", &archive_path, e))?;
    }

    zip.finish()?;
    Ok(archive_path)
}
