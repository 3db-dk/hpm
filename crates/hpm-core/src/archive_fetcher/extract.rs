//! Pure archive extraction: format sniffing, ZIP and tar.gz extraction,
//! common-root-prefix stripping, and path-traversal safety validation.

use flate2::read::GzDecoder;
use std::io::Read;
use std::path::{Path, PathBuf};
use tracing::{debug, warn};

use super::FetchError;

/// Detected archive container format.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum ArchiveFormat {
    Zip,
    TarGz,
}

/// Sniff the archive format from the file's leading bytes.
///
/// We never trust the URL or filename extension — registry URLs frequently
/// disagree with their actual payload. ZIP starts with the local-file-header
/// magic `50 4B 03 04`; gzip (and therefore tar.gz) starts with `1F 8B`.
fn detect_archive_format(archive_path: &Path) -> Result<ArchiveFormat, FetchError> {
    let mut file = std::fs::File::open(archive_path)?;
    let mut magic = [0u8; 4];
    let n = file.read(&mut magic)?;
    if n >= 4 && &magic[0..4] == b"PK\x03\x04" {
        return Ok(ArchiveFormat::Zip);
    }
    if n >= 2 && magic[0] == 0x1F && magic[1] == 0x8B {
        return Ok(ArchiveFormat::TarGz);
    }
    Err(FetchError::ExtractionError(format!(
        "Unrecognized archive format (magic bytes: {:02X?}); expected ZIP (PK..) or gzip/tar.gz (1F 8B)",
        &magic[..n]
    )))
}

/// Extract an archive to the target directory (blocking operation).
///
/// Dispatches to the ZIP or tar.gz extractor based on magic bytes — registry
/// URLs lie about formats, so the file's content is the source of truth.
/// This is a standalone function designed to be called from `spawn_blocking`.
pub(super) fn extract_archive_sync(
    archive_path: &Path,
    target_dir: &Path,
) -> Result<(), FetchError> {
    match detect_archive_format(archive_path)? {
        ArchiveFormat::Zip => extract_zip_sync(archive_path, target_dir),
        ArchiveFormat::TarGz => extract_tar_gz_sync(archive_path, target_dir),
    }
}

/// Extract a ZIP archive to the target directory (blocking operation).
fn extract_zip_sync(archive_path: &Path, target_dir: &Path) -> Result<(), FetchError> {
    let file = std::fs::File::open(archive_path)?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| FetchError::ExtractionError(e.to_string()))?;

    // Resolve the layout: hpackage (strip `{slug}/`, skip root `{slug}.json`),
    // Git-style single root dir (strip it), or flat (extract as-is).
    let layout = archive_layout_sync(&archive)?;

    // Create target directory
    std::fs::create_dir_all(target_dir)?;

    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .map_err(|e| FetchError::ExtractionError(e.to_string()))?;

        if let Some(ref skip) = layout.skip_entry
            && file.name() == skip
        {
            continue;
        }

        let raw_path = match file.enclosed_name() {
            Some(p) => p.to_path_buf(),
            None => {
                warn!("Skipping file with invalid path in archive");
                continue;
            }
        };

        // Strip the common prefix
        let relative_path = if let Some(ref prefix) = layout.strip_prefix {
            match raw_path.strip_prefix(prefix) {
                Ok(p) => p.to_path_buf(),
                Err(_) => raw_path,
            }
        } else {
            raw_path
        };

        // Skip empty paths (the root directory itself after stripping prefix)
        if relative_path.as_os_str().is_empty() {
            continue;
        }

        // Security check: ensure no path traversal
        validate_path_safety_sync(&relative_path)?;

        let target_path = target_dir.join(&relative_path);

        // A zip stores a symlink as an entry whose *content* is the target
        // path, flagged only in the Unix mode bits. Nothing here inspected
        // those, so such an entry was written out as a regular file holding a
        // path string — a silently corrupt install rather than a refused one.
        // Skipping matches the tar path and keeps the extractor's long-
        // standing "we do not materialise links" posture, which is what makes
        // path-traversal validation sufficient: a link is the one entry type
        // that can redirect a *later* entry's write outside the target
        // directory, and validating its target at creation time would not stop
        // that. `hpm pack` resolves symlinks to their contents, so its own
        // archives never contain one.
        if is_symlink_entry(&file) {
            warn!(
                "Skipping symlink entry in archive: {} (links are not extracted)",
                relative_path.display()
            );
            continue;
        }

        if file.is_dir() {
            std::fs::create_dir_all(&target_path)?;
        } else {
            // Ensure parent directory exists
            if let Some(parent) = target_path.parent() {
                std::fs::create_dir_all(parent)?;
            }

            let mut outfile = std::fs::File::create(&target_path)?;
            std::io::copy(&mut file, &mut outfile)?;
            drop(outfile);

            #[cfg(unix)]
            finalize_mode(&target_path, file.unix_mode())?;
        }
    }

    Ok(())
}

/// Whether a zip entry describes a symbolic link.
///
/// Zip has no dedicated link record: the type lives in the high bits of the
/// Unix mode an archive optionally carries (`S_IFMT` == `S_IFLNK`), and the
/// entry's payload is the target path. Producers on non-Unix hosts record no
/// mode at all, in which case there is nothing to detect and nothing to skip.
/// Checked on every platform, not just Unix — a Windows install must make the
/// same file set as a Linux one, or the same archive yields different trees.
fn is_symlink_entry(file: &zip::read::ZipFile<'_, std::fs::File>) -> bool {
    const S_IFMT: u32 = 0o170000;
    const S_IFLNK: u32 = 0o120000;
    file.unix_mode()
        .is_some_and(|mode| mode & S_IFMT == S_IFLNK)
}

/// Resolve the extraction layout of a zip archive (blocking operation).
fn archive_layout_sync(
    archive: &zip::ZipArchive<std::fs::File>,
) -> Result<ExtractPlan, FetchError> {
    if archive.is_empty() {
        return Ok(ExtractPlan::default());
    }

    let mut names = Vec::with_capacity(archive.len());
    for i in 0..archive.len() {
        let name = archive
            .name_for_index(i)
            .ok_or_else(|| FetchError::ExtractionError("Invalid archive entry".to_string()))?;
        names.push(name.to_string());
    }
    Ok(resolve_archive_layout(&names))
}

/// Resolved extraction layout: a root prefix to strip from every entry, and
/// an optional exact entry name to skip entirely.
#[derive(Debug, Default, PartialEq, Eq)]
struct ExtractPlan {
    strip_prefix: Option<PathBuf>,
    skip_entry: Option<String>,
}

/// Decide how to extract based on the entry names.
///
/// Tries the hpackage layout first (`hpm pack` output: `{slug}.json` at the
/// root next to a `{slug}/` content folder), then the Git/SideFX
/// single-root-directory convention, then falls back to as-is extraction
/// (legacy flat hpm archives).
fn resolve_archive_layout(names: &[String]) -> ExtractPlan {
    if let Some((prefix, root_json)) = find_hpackage_layout(names) {
        return ExtractPlan {
            strip_prefix: Some(prefix),
            skip_entry: Some(root_json),
        };
    }
    ExtractPlan {
        strip_prefix: find_common_root_prefix(names),
        skip_entry: None,
    }
}

/// Detect the Houdini "hpackage" layout `hpm pack` produces: exactly one
/// root-level `{slug}.json` plus every other entry under `{slug}/`.
///
/// Returns `(content_prefix, root_json_name)`. The json is skipped on
/// install: it exists so a *manual* extraction into a Houdini packages
/// directory works without HPM; HPM-managed installs generate their own
/// package jsons, and the content is installed flat (prefix stripped) like
/// every other layout.
///
/// Legacy flat hpm archives (content at the root next to `{slug}.json`) do
/// not match — any root entry besides the json breaks the "everything under
/// `{slug}/`" requirement — and keep their as-is extraction.
fn find_hpackage_layout(names: &[String]) -> Option<(PathBuf, String)> {
    let mut json_stem: Option<&str> = None;
    for name in names {
        if !name.contains('/') {
            let stem = name.strip_suffix(".json")?; // non-json root entry: not hpackage
            if json_stem.is_some() {
                return None; // two root jsons: ambiguous, leave as-is
            }
            json_stem = Some(stem);
        }
    }
    let stem = json_stem?;
    let dir_prefix = format!("{}/", stem);
    let json_name = format!("{}.json", stem);
    for name in names {
        if name == &json_name {
            continue;
        }
        if !name.starts_with(&dir_prefix) {
            return None;
        }
    }
    Some((PathBuf::from(stem), json_name))
}

/// Find a common single-component root directory across a set of archive entry names.
///
/// Returns `Some(prefix)` only if every entry starts with the same first path
/// component — matches Git/SideFX archive convention where everything sits
/// under a single `pkg-name-version/` directory.
fn find_common_root_prefix(names: &[String]) -> Option<PathBuf> {
    let first = names.first()?;
    let first_component = PathBuf::from(first)
        .components()
        .next()?
        .as_os_str()
        .to_owned();
    let prefix = PathBuf::from(&first_component);
    let prefix_str = prefix.to_str()?;
    for name in names {
        if !name.starts_with(prefix_str) {
            return None;
        }
    }
    Some(prefix)
}

/// Extract a gzipped tar archive to the target directory (blocking operation).
fn extract_tar_gz_sync(archive_path: &Path, target_dir: &Path) -> Result<(), FetchError> {
    // Pass 1: enumerate entry names so we can detect a common root prefix.
    // The tar crate's `Archive` is single-pass, so we open it twice.
    let names = {
        let file = std::fs::File::open(archive_path)?;
        let gz = GzDecoder::new(file);
        let mut archive = tar::Archive::new(gz);
        let mut names = Vec::new();
        for entry in archive
            .entries()
            .map_err(|e| FetchError::ExtractionError(e.to_string()))?
        {
            let entry = entry.map_err(|e| FetchError::ExtractionError(e.to_string()))?;
            let path = entry
                .path()
                .map_err(|e| FetchError::ExtractionError(e.to_string()))?;
            names.push(path.to_string_lossy().into_owned());
        }
        names
    };
    let layout = resolve_archive_layout(&names);

    std::fs::create_dir_all(target_dir)?;

    // Pass 2: extract.
    let file = std::fs::File::open(archive_path)?;
    let gz = GzDecoder::new(file);
    let mut archive = tar::Archive::new(gz);
    archive.set_preserve_permissions(true);
    archive.set_overwrite(true);

    for entry in archive
        .entries()
        .map_err(|e| FetchError::ExtractionError(e.to_string()))?
    {
        let mut entry = entry.map_err(|e| FetchError::ExtractionError(e.to_string()))?;
        let raw_path = entry
            .path()
            .map_err(|e| FetchError::ExtractionError(e.to_string()))?
            .into_owned();

        if let Some(ref skip) = layout.skip_entry
            && raw_path.to_string_lossy() == skip.as_str()
        {
            continue;
        }

        let relative_path = if let Some(ref prefix) = layout.strip_prefix {
            match raw_path.strip_prefix(prefix) {
                Ok(p) => p.to_path_buf(),
                Err(_) => raw_path,
            }
        } else {
            raw_path
        };

        if relative_path.as_os_str().is_empty() {
            continue;
        }

        validate_path_safety_sync(&relative_path)?;

        let target_path = target_dir.join(&relative_path);
        let entry_type = entry.header().entry_type();

        if entry_type.is_dir() {
            std::fs::create_dir_all(&target_path)?;
        } else if entry_type.is_file() {
            if let Some(parent) = target_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut outfile = std::fs::File::create(&target_path)?;
            std::io::copy(&mut entry, &mut outfile)?;
            drop(outfile);

            #[cfg(unix)]
            finalize_mode(&target_path, entry.header().mode().ok())?;
        } else {
            // Symlinks, hardlinks, devices, etc. — skipped intentionally to
            // keep the same security posture as the ZIP path (which doesn't
            // honor symlinks either).
            debug!(
                "Skipping non-regular tar entry: {} ({:?})",
                relative_path.display(),
                entry_type
            );
        }
    }

    Ok(())
}

/// Apply an extracted file's Unix mode, restoring the executable bit when the
/// archive lost it.
///
/// `declared` is the mode the archive carries for the entry (`None` for a zip
/// written by a non-Unix producer, which records no mode at all). Normally it
/// is applied verbatim. The exception is the case this function exists for:
/// archives reach us from arbitrary hosts — GitHub Releases, SideFX hpack,
/// a studio's own S3 bucket, a contributor's `zip` on Windows — and a great
/// many zip producers drop Unix modes, stamping every entry 0o644. Extracting
/// such an archive faithfully yields a package whose shipped programs cannot
/// be spawned: `Permission denied (os error 13)` on Unix, while the identical
/// package works on Windows, where executability is not a file mode.
///
/// So when the resulting mode has no executable bit but the file's leading
/// bytes identify it as a program (ELF, Mach-O, or a `#!` script), the bit is
/// restored, mirroring the read bits so the mode stays consistent with the
/// declared access (0o644 -> 0o755, 0o640 -> 0o750). Only ever additive: a
/// mode that already declares execute is untouched, and a file that isn't a
/// program is never made executable.
///
/// This grants no privilege a well-formed archive couldn't claim for itself by
/// declaring 0o755 — and hpm already runs `[scripts]` programs out of the
/// installed tree. Archives are checksum- and signature-verified before they
/// get here.
///
/// The rule itself lives in [`crate::exec_mode`], which also applies it to
/// trees installed before this extractor existed — those are never
/// re-extracted, so extraction-time repair alone never reaches them.
#[cfg(unix)]
fn finalize_mode(target_path: &Path, declared: Option<u32>) -> Result<(), FetchError> {
    use std::os::unix::fs::PermissionsExt;

    let mode = declared.unwrap_or(0o644) & 0o7777;
    let repaired = crate::exec_mode::repaired_mode(mode, target_path);
    if repaired != mode {
        debug!(
            "Restoring executable bit on {} (archive declared {:04o})",
            target_path.display(),
            mode
        );
    }

    // With no declared mode and nothing to repair there is nothing to say
    // about this file — leave whatever `File::create` produced under the
    // caller's umask, as before.
    if declared.is_some() || repaired != mode {
        std::fs::set_permissions(target_path, std::fs::Permissions::from_mode(repaired))?;
    }
    Ok(())
}

/// Validate that a path doesn't contain traversal attempts.
fn validate_path_safety_sync(path: &Path) -> Result<(), FetchError> {
    // Check for backslash-based traversal (e.g. from Windows-style archive entries)
    let path_str = path.to_string_lossy();
    if path_str.contains("..\\") || path_str.contains("../") || path_str == ".." {
        return Err(FetchError::PathTraversalDetected(
            path.display().to_string(),
        ));
    }
    for component in path.components() {
        if matches!(component, std::path::Component::ParentDir) {
            return Err(FetchError::PathTraversalDetected(
                path.display().to_string(),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    // Security tests for path traversal - CRITICAL, never delete
    // These tests validate archive extraction safety

    #[test]
    fn test_validate_path_safety() {
        // Safe paths
        assert!(validate_path_safety_sync(Path::new("foo/bar/baz.txt")).is_ok());
        assert!(validate_path_safety_sync(Path::new("src/lib.rs")).is_ok());

        // Unsafe paths
        assert!(validate_path_safety_sync(Path::new("../etc/passwd")).is_err());
        assert!(validate_path_safety_sync(Path::new("foo/../../etc/passwd")).is_err());
    }

    #[test]
    fn test_path_traversal_parent_directory() {
        // Path traversal with parent directory reference
        assert!(validate_path_safety_sync(Path::new("../secret")).is_err());
    }

    #[test]
    fn test_path_traversal_embedded() {
        // Path traversal embedded in path
        assert!(validate_path_safety_sync(Path::new("foo/../../../etc/passwd")).is_err());
    }

    #[test]
    fn test_path_traversal_windows_style() {
        // Windows-style path separators shouldn't bypass checks
        assert!(validate_path_safety_sync(Path::new("..\\secret")).is_err());
    }

    // --- Archive format detection + tar.gz extraction ---
    //
    // Regression coverage: an extractor that hardcodes ZIP fails with
    // "Could not find EOCD" on a tar.gz upload, and registry URLs routinely
    // disagree with their payload's actual format. These tests pin both
    // formats end-to-end so the same class of regression can't ship
    // undetected.

    use flate2::Compression;
    use flate2::write::GzEncoder;

    /// Build an in-memory tar.gz with `entries` = (relative path, contents)
    /// nested under a single root directory `root_dir`. Mirrors the layout
    /// `pkg-name-version/...` produced by `tar -czf` in package CI.
    fn build_test_tar_gz(root_dir: &str, entries: &[(&str, &[u8])]) -> Vec<u8> {
        let buf = Vec::new();
        let gz = GzEncoder::new(buf, Compression::default());
        let mut tar = tar::Builder::new(gz);
        for (rel_path, contents) in entries {
            let full_path = format!("{}/{}", root_dir, rel_path);
            let mut header = tar::Header::new_gnu();
            header.set_size(contents.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            tar.append_data(&mut header, &full_path, *contents).unwrap();
        }
        tar.into_inner().unwrap().finish().unwrap()
    }

    #[test]
    fn test_detect_archive_format_zip() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("a.bin");
        // Minimal ZIP magic header — enough for sniffing, not enough to extract.
        std::fs::write(&path, b"PK\x03\x04rest").unwrap();
        assert_eq!(detect_archive_format(&path).unwrap(), ArchiveFormat::Zip);
    }

    #[test]
    fn test_detect_archive_format_tar_gz() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("a.bin");
        let bytes = build_test_tar_gz("pkg-1.0", &[("hpm.toml", b"[package]")]);
        std::fs::write(&path, &bytes).unwrap();
        assert_eq!(detect_archive_format(&path).unwrap(), ArchiveFormat::TarGz);
    }

    #[test]
    fn test_detect_archive_format_unknown() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("a.bin");
        std::fs::write(&path, b"not an archive").unwrap();
        match detect_archive_format(&path) {
            Err(FetchError::ExtractionError(msg)) => {
                assert!(msg.contains("Unrecognized archive format"));
            }
            other => panic!("Expected ExtractionError, got {:?}", other),
        }
    }

    #[test]
    fn test_extract_tar_gz_strips_common_prefix() {
        let temp = TempDir::new().unwrap();
        let archive_path = temp.path().join("pkg.tar.gz");
        let extract_dir = temp.path().join("out");

        let bytes = build_test_tar_gz(
            "nodepilot-1.2.0",
            &[
                ("hpm.toml", b"[package]\nname = \"nodepilot\"\n"),
                ("python/main.py", b"print('hi')\n"),
            ],
        );
        std::fs::write(&archive_path, &bytes).unwrap();

        extract_archive_sync(&archive_path, &extract_dir).unwrap();

        // Common root should be stripped — files land directly under extract_dir,
        // not under nodepilot-1.2.0/.
        assert!(extract_dir.join("hpm.toml").exists());
        assert!(extract_dir.join("python/main.py").exists());
        assert!(!extract_dir.join("nodepilot-1.2.0").exists());

        let manifest = std::fs::read_to_string(extract_dir.join("hpm.toml")).unwrap();
        assert!(manifest.contains("nodepilot"));
    }

    #[test]
    fn test_extract_dispatches_on_magic_not_extension() {
        // Even if the file is named `.zip`, a tar.gz payload should extract
        // successfully — content is the source of truth, not the filename.
        let temp = TempDir::new().unwrap();
        let archive_path = temp.path().join("misnamed.zip");
        let extract_dir = temp.path().join("out");

        let bytes = build_test_tar_gz("pkg-1.0", &[("data.txt", b"hello")]);
        std::fs::write(&archive_path, &bytes).unwrap();

        extract_archive_sync(&archive_path, &extract_dir).unwrap();
        assert_eq!(
            std::fs::read_to_string(extract_dir.join("data.txt")).unwrap(),
            "hello"
        );
    }

    #[test]
    fn test_extract_tar_gz_rejects_path_traversal() {
        let temp = TempDir::new().unwrap();
        let archive_path = temp.path().join("evil.tar.gz");
        let extract_dir = temp.path().join("out");

        // The `tar::Builder` API rejects `..` paths at write time, so we
        // forge the entry by writing the malicious name straight into the
        // header's raw `name` bytes — matches what a hostile packager could
        // produce with a custom tar implementation.
        let buf = Vec::new();
        let gz = GzEncoder::new(buf, Compression::default());
        let mut tar = tar::Builder::new(gz);
        // Use a name that survives common-prefix stripping: prefix `pkg-1.0`
        // is identified and removed, leaving `../../escaped.txt` for the
        // safety validator to catch. This is the real attack shape — a
        // single-entry `../escape` would get its `..` eaten as the prefix.
        let mut header = tar::Header::new_old();
        let evil_name = b"pkg-1.0/../../escaped.txt";
        header.as_old_mut().name[..evil_name.len()].copy_from_slice(evil_name);
        let payload = b"pwn";
        header.set_size(payload.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        tar.append(&header, &payload[..]).unwrap();
        let bytes = tar.into_inner().unwrap().finish().unwrap();
        std::fs::write(&archive_path, &bytes).unwrap();

        match extract_archive_sync(&archive_path, &extract_dir) {
            Err(FetchError::PathTraversalDetected(_)) => {}
            other => panic!("Expected PathTraversalDetected, got {:?}", other),
        }
    }

    #[test]
    fn test_find_common_root_prefix_no_shared_root() {
        let names = vec!["a/x".to_string(), "b/y".to_string()];
        assert!(find_common_root_prefix(&names).is_none());
    }

    #[test]
    fn test_find_common_root_prefix_single_root() {
        let names = vec!["pkg-1.0/a".to_string(), "pkg-1.0/b/c".to_string()];
        assert_eq!(
            find_common_root_prefix(&names),
            Some(PathBuf::from("pkg-1.0"))
        );
    }

    /// Build an in-memory zip with the given (name, contents) entries.
    fn build_test_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
        use std::io::Write;
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut buf);
            let opts = zip::write::SimpleFileOptions::default();
            for (name, contents) in entries {
                zip.start_file(*name, opts).unwrap();
                zip.write_all(contents).unwrap();
            }
            zip.finish().unwrap();
        }
        buf.into_inner()
    }

    #[test]
    fn test_extract_zip_hpackage_layout_strips_wrapper_and_skips_root_json() {
        // The layout `hpm pack` produces: {slug}.json at the root, all
        // content under {slug}/. Install must strip the wrapper (flat
        // install tree, hpm.toml at the root) and skip the root json
        // (HPM generates its own package jsons; the shipped one exists for
        // manual no-HPM extraction into a Houdini packages directory).
        let temp = TempDir::new().unwrap();
        let archive_path = temp.path().join("pkg.zip");
        let extract_dir = temp.path().join("out");

        let bytes = build_test_zip(&[
            (
                "tumblerig.json",
                b"{\"hpath\": \"$HOUDINI_PACKAGE_PATH/tumblerig\"}",
            ),
            ("tumblerig/hpm.toml", b"[package]\nname = \"TumbleRig\"\n"),
            ("tumblerig/otls/tool.hda", b"hda"),
        ]);
        std::fs::write(&archive_path, &bytes).unwrap();

        extract_archive_sync(&archive_path, &extract_dir).unwrap();

        assert!(extract_dir.join("hpm.toml").exists());
        assert!(extract_dir.join("otls/tool.hda").exists());
        assert!(!extract_dir.join("tumblerig").exists());
        assert!(!extract_dir.join("tumblerig.json").exists());
    }

    #[test]
    fn test_extract_zip_legacy_flat_layout_unchanged() {
        // Pre-hpackage hpm archives: content at the root next to the json.
        // No wrapper to strip; everything (json included) extracts as-is.
        let temp = TempDir::new().unwrap();
        let archive_path = temp.path().join("pkg.zip");
        let extract_dir = temp.path().join("out");

        let bytes = build_test_zip(&[
            ("tumblerig.json", b"{}"),
            ("hpm.toml", b"[package]\nname = \"TumbleRig\"\n"),
            ("otls/tool.hda", b"hda"),
        ]);
        std::fs::write(&archive_path, &bytes).unwrap();

        extract_archive_sync(&archive_path, &extract_dir).unwrap();

        assert!(extract_dir.join("hpm.toml").exists());
        assert!(extract_dir.join("otls/tool.hda").exists());
        assert!(extract_dir.join("tumblerig.json").exists());
    }

    /// A 4-byte ELF header is enough for the magic probe; the payload never
    /// gets executed, only sniffed.
    const ELF_STUB: &[u8] = b"\x7fELFrest-of-a-binary";

    /// Regression: `hpm pack` shipped every zip entry as 0o644, so the
    /// executable a package declares in `[scripts]` extracted non-executable
    /// and every Unix spawn failed with `Permission denied (os error 13)`.
    /// The archive is repaired on the way out when its content says it is a
    /// program.
    #[cfg(unix)]
    #[test]
    fn test_extract_zip_restores_lost_executable_bit() {
        use std::os::unix::fs::PermissionsExt;

        let temp = TempDir::new().unwrap();
        let archive_path = temp.path().join("pkg.zip");
        let extract_dir = temp.path().join("out");

        // build_test_zip uses SimpleFileOptions::default() — i.e. exactly the
        // 0o644-for-everything archive the old packer produced.
        let bytes = build_test_zip(&[
            ("pkg/hpm.toml", b"[package]"),
            ("pkg/bin/tt_setup", ELF_STUB),
            ("pkg/scripts/setup.sh", b"#!/bin/sh\necho hi\n"),
            ("pkg/otls/tool.hda", b"not a program"),
        ]);
        std::fs::write(&archive_path, &bytes).unwrap();

        extract_archive_sync(&archive_path, &extract_dir).unwrap();

        let mode = |p: &str| {
            std::fs::metadata(extract_dir.join(p))
                .unwrap()
                .permissions()
                .mode()
                & 0o777
        };
        assert_eq!(mode("bin/tt_setup"), 0o755, "ELF binary must be runnable");
        assert_eq!(
            mode("scripts/setup.sh"),
            0o755,
            "#! script must be runnable"
        );
        // Plain data is never granted execute.
        assert_eq!(mode("otls/tool.hda"), 0o644);
    }

    /// The repair is additive only: an archive that already declares a mode
    /// keeps it, and a restrictive mode isn't widened beyond its read bits.
    #[cfg(unix)]
    #[test]
    fn test_extract_zip_preserves_declared_modes() {
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;

        let temp = TempDir::new().unwrap();
        let archive_path = temp.path().join("pkg.zip");
        let extract_dir = temp.path().join("out");

        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut buf);
            let owner_only = zip::write::SimpleFileOptions::default().unix_permissions(0o640);
            zip.start_file("pkg/bin/private_tool", owner_only).unwrap();
            zip.write_all(ELF_STUB).unwrap();
            let already_exec = zip::write::SimpleFileOptions::default().unix_permissions(0o755);
            zip.start_file("pkg/bin/tool", already_exec).unwrap();
            zip.write_all(ELF_STUB).unwrap();
            zip.finish().unwrap();
        }
        std::fs::write(&archive_path, buf.into_inner()).unwrap();

        extract_archive_sync(&archive_path, &extract_dir).unwrap();

        let mode = |p: &str| {
            std::fs::metadata(extract_dir.join(p))
                .unwrap()
                .permissions()
                .mode()
                & 0o777
        };
        // 0o640 gains execute only where read was granted — group/other stay shut.
        assert_eq!(mode("bin/private_tool"), 0o750);
        assert_eq!(mode("bin/tool"), 0o755);
    }

    /// Same repair on the tar.gz path, which third-party hosts also serve.
    #[cfg(unix)]
    #[test]
    fn test_extract_tar_gz_restores_lost_executable_bit() {
        use std::os::unix::fs::PermissionsExt;

        let temp = TempDir::new().unwrap();
        let archive_path = temp.path().join("pkg.tar.gz");
        let extract_dir = temp.path().join("out");

        // build_test_tar_gz stamps every entry 0o644.
        let bytes = build_test_tar_gz(
            "pkg-1.0",
            &[("hpm.toml", b"[package]"), ("bin/tool", ELF_STUB)],
        );
        std::fs::write(&archive_path, &bytes).unwrap();

        extract_archive_sync(&archive_path, &extract_dir).unwrap();

        let mode = std::fs::metadata(extract_dir.join("bin/tool"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o755);
    }

    /// Regression: a zip symlink entry was written out as a regular file whose
    /// contents were the target path — a silently corrupt install. It must be
    /// skipped, matching the tar path, and must not leave a decoy file behind.
    #[cfg(unix)]
    #[test]
    fn test_extract_zip_skips_symlink_entries() {
        use std::io::Write;

        let temp = TempDir::new().unwrap();
        let archive_path = temp.path().join("pkg.zip");
        let extract_dir = temp.path().join("out");

        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut buf);
            let plain = zip::write::SimpleFileOptions::default().unix_permissions(0o644);
            zip.start_file("pkg/lib/libfoo.so.1", plain).unwrap();
            zip.write_all(ELF_STUB).unwrap();
            // Written with the zip crate's own symlink API, so this is exactly
            // the encoding a real Unix producer emits for
            // `libfoo.so -> libfoo.so.1`. (`unix_permissions` cannot express
            // it: the crate masks the mode to 0o777, dropping S_IFLNK.)
            zip.add_symlink("pkg/lib/libfoo.so", "libfoo.so.1", plain)
                .unwrap();
            zip.finish().unwrap();
        }
        std::fs::write(&archive_path, buf.into_inner()).unwrap();

        extract_archive_sync(&archive_path, &extract_dir).unwrap();

        assert!(extract_dir.join("lib/libfoo.so.1").exists());
        let decoy = extract_dir.join("lib/libfoo.so");
        assert!(
            !decoy.exists() && decoy.symlink_metadata().is_err(),
            "the link must be skipped outright, not written as a file containing \
             its target path"
        );
    }

    /// A traversing symlink is the case that makes materialising links unsafe:
    /// it can redirect a *later* entry's write outside the target directory,
    /// which validating the link target at creation time would not prevent.
    #[cfg(unix)]
    #[test]
    fn test_extract_zip_skips_traversing_symlink() {
        use std::io::Write;

        let temp = TempDir::new().unwrap();
        let archive_path = temp.path().join("evil.zip");
        let extract_dir = temp.path().join("out");

        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut buf);
            let plain = zip::write::SimpleFileOptions::default().unix_permissions(0o644);
            zip.start_file("pkg/hpm.toml", plain).unwrap();
            zip.write_all(b"[package]").unwrap();
            zip.add_symlink("pkg/escape", "/etc", plain).unwrap();
            zip.finish().unwrap();
        }
        std::fs::write(&archive_path, buf.into_inner()).unwrap();

        extract_archive_sync(&archive_path, &extract_dir).unwrap();

        let escape = extract_dir.join("escape");
        assert!(escape.symlink_metadata().is_err(), "no link may be created");
        assert!(
            extract_dir.join("hpm.toml").exists(),
            "other entries still extract"
        );
    }

    #[test]
    fn test_find_hpackage_layout_detection() {
        // Positive: one root json + everything under the matching folder.
        let names = vec![
            "tumblerig.json".to_string(),
            "tumblerig/hpm.toml".to_string(),
            "tumblerig/otls/a.hda".to_string(),
        ];
        assert_eq!(
            find_hpackage_layout(&names),
            Some((PathBuf::from("tumblerig"), "tumblerig.json".to_string()))
        );

        // Root entry that isn't the json: legacy flat archive, no match.
        let flat = vec![
            "tumblerig.json".to_string(),
            "hpm.toml".to_string(),
            "otls/a.hda".to_string(),
        ];
        assert_eq!(find_hpackage_layout(&flat), None);

        // Json whose stem doesn't match the content folder: no match.
        let mismatched = vec!["other.json".to_string(), "tumblerig/hpm.toml".to_string()];
        assert_eq!(find_hpackage_layout(&mismatched), None);

        // Two root jsons: ambiguous, no match.
        let two = vec![
            "a.json".to_string(),
            "b.json".to_string(),
            "a/f".to_string(),
        ];
        assert_eq!(find_hpackage_layout(&two), None);

        // Git-style single root dir (no root json): handled by the
        // common-root-prefix path instead.
        let git = vec!["pkg-1.0/hpm.toml".to_string()];
        assert_eq!(find_hpackage_layout(&git), None);
    }
}
