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
/// Archive mode for an executable file. Unix only: a Windows host has no
/// executable bit to read, so it stamps every entry [`MODE_REGULAR`].
#[cfg(unix)]
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
    /// The target platform of a platform-specific archive.
    pub platform: Option<Platform>,
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
    create_archive_as(
        ArchiveKind::Zip,
        package_dir,
        name,
        version,
        output_dir,
        ignore,
        platform,
        stage_filter,
        layout,
    )
}

/// The container an `hpm pack` archive is written in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveKind {
    /// `.zip`, every file compressed on its own. What every hpm reads.
    Zip,
    /// `.tar.zst`: one tar, compressed as a single zstd stream with
    /// long-distance matching ("solid"). Near-identical files (a package
    /// shipping every version of a compiled graph, scenes that differ by a
    /// few nodes) compress against each other, which zip cannot do. Only
    /// hpm builds whose extractor knows the zstd magic can install it, so a
    /// release publishes it next to the zip, never instead of it.
    TarZst,
}

impl ArchiveKind {
    /// The file extension, without the leading dot.
    pub fn extension(self) -> &'static str {
        match self {
            ArchiveKind::Zip => "zip",
            ArchiveKind::TarZst => "tar.zst",
        }
    }
}

/// zstd parameters of a [`ArchiveKind::TarZst`] archive.
#[derive(Debug, Clone, Copy)]
pub struct SolidParams {
    /// zstd level (1..=22). 19 is the highest level that needs no
    /// `--ultra` and keeps decoding at full speed.
    pub level: i32,
    /// log2 of the match window. 27 (128 MiB) is the largest window every
    /// zstd decoder accepts without raising its limit, so a user can unpack
    /// the archive with a stock `zstd -d` / `tar --zstd` as well.
    pub window_log: u32,
    /// Compression threads; 0 = single-threaded.
    pub workers: u32,
}

impl Default for SolidParams {
    fn default() -> Self {
        Self {
            level: 19,
            window_log: 27,
            workers: std::thread::available_parallelism()
                .map(|n| n.get() as u32)
                .unwrap_or(1),
        }
    }
}

/// [`create_archive`] in the given container. Both kinds carry exactly the
/// same entries in the same order: the solid archive is the zip's content in
/// another wrapper, so an install from either yields the same tree (and the
/// same tree checksum a lockfile records).
#[allow(clippy::too_many_arguments)]
pub fn create_archive_as(
    kind: ArchiveKind,
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
    let ext = kind.extension();
    let archive_name = match platform {
        Some(p) => format!("{}-{}-{}.{}", safe_name, version, p, ext),
        None => format!("{}-{}.{}", safe_name, version, ext),
    };
    let archive_path = output_dir.join(&archive_name);

    // Collect (source path, archive path) pairs, sorted for determinism.
    let staged = collect_stage_entries(package_dir, ignore, stage_filter, None)?;
    super::stage_filter::refuse_private_keys(&staged)?;
    let entries = plan_entries(&staged, layout);

    // Create the output directory if it doesn't exist. `File::create` only
    // creates the file, so a missing parent surfaced as a bare
    // `No such file or directory (os error 2)` naming the *archive* — which
    // reads as "the archive couldn't be written" rather than "the directory
    // you asked me to write it into isn't there". An output directory is
    // ours to produce, and a release script that writes into `dist/` on a
    // clean checkout shouldn't have to mkdir it first.
    fs::create_dir_all(output_dir)
        .map_err(|e| IoOp::wrap("create archive output directory", output_dir, e))?;

    match kind {
        ArchiveKind::Zip => write_zip(&entries, &archive_path)?,
        ArchiveKind::TarZst => write_tar_zst(&entries, &archive_path, SolidParams::default())?,
    }
    Ok(archive_path)
}

/// Where one archive entry's bytes come from.
pub(crate) enum EntrySource<'a> {
    /// A staged package file, with the Unix mode to record for it.
    File(&'a Path, u32),
    /// Bytes generated at pack time (the bundled Houdini descriptor).
    Injected(&'a [u8]),
}

/// One archive entry: its `/`-separated name and its bytes.
pub(crate) struct PlannedEntry<'a> {
    pub name: String,
    pub source: EntrySource<'a>,
}

/// The entries of an archive, in write order: the staged files (sorted),
/// each under the content prefix, then the injected files at the root.
pub(crate) fn plan_entries<'a>(
    staged: &'a [(PathBuf, String)],
    layout: super::ArchiveLayout<'a>,
) -> Vec<PlannedEntry<'a>> {
    let mut entries = Vec::with_capacity(staged.len() + layout.inject_files.len());
    for (source, archive_name) in staged {
        // The injected bytes win over a staged file at the same archive path
        // (e.g. a hand-written {slug}.json ships at the root, not under the
        // content prefix, and never twice).
        if layout.inject_files.iter().any(|(n, _)| n == archive_name) {
            continue;
        }
        let name = match layout.content_prefix {
            Some(prefix) => format!("{}/{}", prefix, archive_name),
            None => archive_name.clone(),
        };
        entries.push(PlannedEntry {
            name,
            source: EntrySource::File(source, entry_mode(source)),
        });
    }
    // Injected files are always at the archive root.
    for (name, content) in layout.inject_files {
        entries.push(PlannedEntry {
            name: name.clone(),
            source: EntrySource::Injected(content),
        });
    }
    entries
}

pub(crate) fn write_zip(
    entries: &[PlannedEntry<'_>],
    archive_path: &Path,
) -> Result<(), PackError> {
    let file = fs::File::create(archive_path)
        .map_err(|e| IoOp::wrap("create archive", archive_path, e))?;
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

    for entry in entries {
        match entry.source {
            EntrySource::File(source, mode) => {
                zip.start_file(entry.name.as_str(), options.unix_permissions(mode))?;
                let mut f = fs::File::open(source)
                    .map_err(|e| IoOp::wrap("open source file", source, e))?;
                let mut buf = Vec::new();
                f.read_to_end(&mut buf)
                    .map_err(|e| IoOp::wrap("read source file", source, e))?;
                zip.write_all(&buf)
                    .map_err(|e| IoOp::wrap("write zip entry to", archive_path, e))?;
            }
            EntrySource::Injected(content) => {
                zip.start_file(entry.name.as_str(), options)?;
                zip.write_all(content)
                    .map_err(|e| IoOp::wrap("write injected zip entry to", archive_path, e))?;
            }
        }
    }

    zip.finish()?;
    Ok(())
}

/// Write `entries` as one tar compressed as a single zstd stream.
///
/// Reproducible like the zip: every header carries mtime 0, uid/gid 0 and the
/// normalised mode, and entries are written in the zip's order, so two packs
/// of one tree are byte-identical.
pub(crate) fn write_tar_zst(
    entries: &[PlannedEntry<'_>],
    archive_path: &Path,
    params: SolidParams,
) -> Result<(), PackError> {
    let wrap = |e: std::io::Error| IoOp::wrap("write solid archive", archive_path, e);
    let file = fs::File::create(archive_path)
        .map_err(|e| IoOp::wrap("create archive", archive_path, e))?;
    let mut encoder =
        zstd::stream::write::Encoder::new(std::io::BufWriter::new(file), params.level)
            .map_err(wrap)?;
    encoder.long_distance_matching(true).map_err(wrap)?;
    encoder.window_log(params.window_log).map_err(wrap)?;
    encoder.include_checksum(true).map_err(wrap)?;
    if params.workers > 0 {
        encoder.multithread(params.workers).map_err(wrap)?;
    }

    let mut tar = tar::Builder::new(encoder);
    for entry in entries {
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Regular);
        header.set_mtime(0);
        header.set_uid(0);
        header.set_gid(0);
        match entry.source {
            EntrySource::File(source, mode) => {
                let f = fs::File::open(source)
                    .map_err(|e| IoOp::wrap("open source file", source, e))?;
                let len = f
                    .metadata()
                    .map_err(|e| IoOp::wrap("read source file", source, e))?
                    .len();
                header.set_size(len);
                header.set_mode(mode);
                tar.append_data(&mut header, &entry.name, f).map_err(wrap)?;
            }
            EntrySource::Injected(content) => {
                header.set_size(content.len() as u64);
                header.set_mode(MODE_REGULAR);
                tar.append_data(&mut header, &entry.name, content)
                    .map_err(wrap)?;
            }
        }
    }
    let encoder = tar.into_inner().map_err(wrap)?;
    let mut out = encoder.finish().map_err(wrap)?;
    out.flush().map_err(wrap)?;
    Ok(())
}
