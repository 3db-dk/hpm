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

/// Archive mode for a non-executable file.
const MODE_REGULAR: u32 = 0o644;
/// Archive mode for an executable file.
const MODE_EXECUTABLE: u32 = 0o755;

/// The Unix mode to stamp on `source`'s archive entry.
///
/// `SimpleFileOptions::default()` carries no permissions, which makes the zip
/// writer record 0o644 for *every* entry — silently stripping the executable
/// bit off anything the package ships as a program. That is not a cosmetic
/// loss: `[scripts]` entries name concrete files (`bin/macos-aarch64/tt_setup`,
/// `./configure.sh`), and an install materialised from such an archive fails
/// at spawn with `Permission denied (os error 13)` on every Unix host while
/// working fine on Windows, where executability isn't a file mode. So the
/// source file's own mode is what decides.
///
/// The mode is normalised to exactly 0o755 / 0o644 rather than forwarded
/// verbatim, because the group/other bits of a checkout depend on the packing
/// user's umask (0o644 under umask 022, 0o664 under umask 002). Forwarding
/// them would make the archive bytes — and therefore the package checksum —
/// differ between two machines packing an identical tree. Only the executable
/// bit is semantically meaningful here, which is the same reduction Git makes.
///
/// On Windows there is no mode to read; every entry ships as `MODE_REGULAR`.
/// A Windows-packed archive can therefore not carry an executable bit, which
/// is fine for the per-platform archives CI produces (a Windows worker packs
/// `.exe`s, whose executability is not a file mode) but means a Unix payload
/// must not be packed from a Windows host. The extractor's content-sniffing
/// fallback covers that case — see `archive_fetcher::extract::finalize_mode`.
fn entry_mode(source: &Path) -> u32 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        match fs::metadata(source) {
            Ok(md) if md.permissions().mode() & 0o111 != 0 => MODE_EXECUTABLE,
            _ => MODE_REGULAR,
        }
    }
    #[cfg(not(unix))]
    {
        let _ = source;
        MODE_REGULAR
    }
}

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
    /// What the shipped native payload turned out to require of a host,
    /// derived by reading the binaries — never declared. Empty for archives
    /// with nothing inspectable in them.
    pub requirements: super::platform_lint::PayloadRequirements,
    /// Advisory findings about the payload. Reported, never enforced: pack
    /// surfaces them and the release owner decides what they mean.
    pub warnings: Vec<super::platform_lint::PlatformWarning>,
}

/// Create a zip archive of the package directory, filtering via ignore
/// rules and `[stage]`.
///
/// Files are added in sorted order for deterministic output. When a `[stage]`
/// filter is supplied, each file's archive path is rewritten via the
/// matching `place` rule; unmatched files ship at their workspace-relative
/// path.
///
/// With `layout.content_prefix` set (the package slug), every staged entry is
/// placed under `{content_prefix}/` while `layout.inject_files` stay at the
/// archive root. This produces the Houdini "hpackage" layout — `{slug}.json`
/// at the root next to a `{slug}/` content folder — so extracting the archive
/// straight into a Houdini packages directory resolves the generated json's
/// `$HOUDINI_PACKAGE_PATH/{slug}/...` paths. A staged file whose archive path
/// collides with an injected name is skipped (the injected bytes win), which
/// keeps a hand-written `{slug}.json` at the root instead of shipping it
/// twice.
pub fn create_archive(
    package_dir: &Path,
    name: &str,
    version: &str,
    output_dir: &Path,
    ignore: &ignore::gitignore::Gitignore,
    platform: Option<&Platform>,
    stage_filter: Option<&StageFilter>,
    layout: super::ArchiveLayout<'_>,
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

    // Create the output directory if it doesn't exist. `File::create` only
    // creates the file, so a missing parent surfaced as a bare
    // `No such file or directory (os error 2)` naming the *archive* — which
    // reads as "the archive couldn't be written" rather than "the directory
    // you asked me to write it into isn't there". An output directory is
    // ours to produce, and a release script that writes into `dist/` on a
    // clean checkout shouldn't have to mkdir it first.
    fs::create_dir_all(output_dir)
        .map_err(|e| IoOp::wrap("create archive output directory", output_dir, e))?;

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
        .last_modified_time(DateTime::default())
        .unix_permissions(MODE_REGULAR);

    for (source, archive_name) in &entries {
        // The injected bytes win over a staged file at the same archive path
        // (e.g. a hand-written {slug}.json ships at the root, not under the
        // content prefix, and never twice).
        if layout.inject_files.iter().any(|(n, _)| n == archive_name) {
            continue;
        }
        let entry_name = match layout.content_prefix {
            Some(prefix) => format!("{}/{}", prefix, archive_name),
            None => archive_name.clone(),
        };
        zip.start_file(
            entry_name.as_str(),
            options.unix_permissions(entry_mode(source)),
        )?;
        let mut f =
            fs::File::open(source).map_err(|e| IoOp::wrap("open source file", source, e))?;
        let mut buf = Vec::new();
        f.read_to_end(&mut buf)
            .map_err(|e| IoOp::wrap("read source file", source, e))?;
        zip.write_all(&buf)
            .map_err(|e| IoOp::wrap("write zip entry to", &archive_path, e))?;
    }

    // Write injected files (always at the archive root)
    for (name, content) in layout.inject_files {
        zip.start_file(name.as_str(), options)?;
        zip.write_all(content)
            .map_err(|e| IoOp::wrap("write injected zip entry to", &archive_path, e))?;
    }

    zip.finish()?;
    Ok(archive_path)
}
