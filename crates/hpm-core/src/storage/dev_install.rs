//! Path-dependency ("dev") install support: link/copy primitives and the
//! `_dev/` subtree under `packages_dir`.
//!
//! These helpers keep the platform-specific symlink/junction handling and
//! the install-entry removal semantics out of `storage.rs` so the
//! [`StorageManager`](super::StorageManager) impl can read top-down.

use super::error::StorageError;
use hpm_package::IoOp;
use hpm_package::path_util::relative_path_to_forward_slash;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use tracing::warn;

/// Subdirectory of `packages_dir` reserved for path-installed (dev) packages.
/// Kept out of the registry CAS namespace so a dev install of `foo@1.0.0`
/// can coexist with — and is never substituted for — a registry install at
/// the same coordinate.
pub(super) const DEV_INSTALL_DIR: &str = "_dev";

/// Sidecar written into a `DevCopy` install recording a fingerprint of the
/// source workspace (relative path + length + mtime of every file) as it was
/// at copy time. On the next dev launch, [`dev_copy_is_current`] recomputes
/// the source fingerprint and compares: an exact match means the workspace
/// hasn't changed, so the destructive clear-and-recopy can be skipped.
///
/// That skip is the whole point. For a native package the dev copy holds the
/// DSOs a concurrently-running Houdini has memory-mapped, so on Windows the
/// removal step fails with `ERROR_ACCESS_DENIED` (`os error 5`) and surfaces as
/// [`StorageError::PackageInUse`] — even when the content is byte-for-byte
/// unchanged and the user simply has another Houdini open. Recognizing the
/// unchanged copy and leaving it in place removes the lock contention, matching
/// the "already installed" short-circuits the registry/URL specs already have.
pub(super) const DEV_SRC_FINGERPRINT_FILE: &str = ".hpm-devsrc";

#[derive(Debug, Clone, Copy)]
pub(super) enum InstallStyle {
    /// Registry/URL fetch → copy into the CAS at `packages_dir/<slug>@<ver>/`.
    CasCopy,
    /// Path dep → copy into `packages_dir/_dev/<slug>@<ver>/`.
    DevCopy,
    /// Path dep → symlink/junction at `packages_dir/_dev/<slug>@<ver>/`
    /// pointing at the workspace.
    DevLink,
}

impl InstallStyle {
    pub(super) fn log_kind(self) -> &'static str {
        match self {
            InstallStyle::CasCopy => "",
            InstallStyle::DevCopy => "dev ",
            InstallStyle::DevLink => "dev-link ",
        }
    }
}

/// A path-installed (dev) package entry under `<packages_dir>/_dev/`.
///
/// Identity comes from the directory name (`<slug>@<version>`), not from
/// reading the entry's `hpm.toml` — link installs that point at a deleted
/// workspace still surface as a `DevInstall` so cleanup can collect them.
#[derive(Debug, Clone)]
pub struct DevInstall {
    pub slug: String,
    pub version: String,
    pub install_path: PathBuf,
}

impl DevInstall {
    /// Identifier used in CLI output and `removed_dev_installs`. Prefixed
    /// with `_dev/` so users can distinguish dev cleanup from CAS cleanup
    /// in the same `hpm clean` listing.
    pub fn identifier(&self) -> String {
        format!("_dev/{}@{}", self.slug, self.version)
    }
}

/// Remove `path` if it is a symlink/junction, without following the link.
pub(super) fn remove_dev_link(path: &std::path::Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        // On Unix, symlink-to-directory entries are removed via `remove_file`.
        std::fs::remove_file(path)
    }
    #[cfg(windows)]
    {
        // `junction::delete` strips the reparse point but leaves the now-empty
        // directory stub in place — re-creating the link at the same path
        // would then fail with ERROR_ALREADY_EXISTS (os error 183). Remove the
        // stub explicitly. The same applies to NTFS symlinks-to-dirs, whose
        // reparse point sits on a directory entry that survives `delete`.
        junction::delete(path)?;
        std::fs::remove_dir(path)
    }
}

/// Create a symlink (Unix) or junction (Windows) at `link` pointing at the
/// absolute `target`. The target must be a directory.
pub(super) fn create_dev_link(
    target: &std::path::Path,
    link: &std::path::Path,
) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link)
    }
    #[cfg(windows)]
    {
        // Junctions are intentional here (vs NTFS symlinks): they don't
        // require Developer Mode or admin, which makes the link-install
        // workflow viable on a stock Houdini workstation.
        junction::create(target, link)
    }
}

/// Returns true when the entry at `path` is a symlink (Unix) or a
/// junction/symlink (Windows). Caller must have already verified the entry
/// exists (typically by reading `symlink_metadata` themselves) — this helper
/// is a pure file-type predicate that doesn't follow links.
///
/// Returns `Err` on Windows when `junction::exists` fails for a *genuine*
/// reason (permission denied, FS error). The error must propagate —
/// silently treating "junction check failed" as "not a junction" would let
/// `remove_dir_all` recurse into the user's workspace the next time around.
///
/// `ERROR_NOT_A_REPARSE_POINT` (Windows error 4390) is *not* a genuine
/// failure — it is how the underlying `DeviceIoControl(FSCTL_GET_REPARSE_POINT)`
/// reports "this exists but isn't a reparse point", i.e. the negative
/// answer for our predicate. Map it to `Ok(false)`. Without this, every
/// `_dev/<slug>@<version>/` install made with `install_as_dev_copy`
/// (a plain directory, no reparse point) raises an IoOp on
/// inspection, breaking orphan detection and the copy→link switch path.
pub(super) fn is_link_entry(
    meta: &std::fs::Metadata,
    path: &std::path::Path,
) -> std::io::Result<bool> {
    if meta.file_type().is_symlink() {
        return Ok(true);
    }
    #[cfg(windows)]
    {
        // Older Rust stdlib reports junctions as non-symlinks; ask the
        // junction crate directly so callers never accidentally fall through
        // to `remove_dir_all` on a reparse point.
        const ERROR_NOT_A_REPARSE_POINT: i32 = 4390;
        match junction::exists(path) {
            Ok(b) => Ok(b),
            Err(e) if e.raw_os_error() == Some(ERROR_NOT_A_REPARSE_POINT) => Ok(false),
            Err(e) => Err(e),
        }
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        Ok(false)
    }
}

/// Remove an install entry without following links. The caller is
/// responsible for verifying the entry exists (and supplying the matching
/// metadata) so this stays a pure removal primitive.
///
/// - Symlink/junction → remove the link entry itself.
/// - Real directory → `remove_dir_all`, with Houdini-handle errors lifted to
///   [`StorageError::PackageInUse`] so the user gets an actionable message.
pub(super) fn remove_install_entry(
    target_dir: &std::path::Path,
    meta: &std::fs::Metadata,
    name: &str,
    version: &str,
) -> Result<(), StorageError> {
    if is_link_entry(meta, target_dir)
        .map_err(|e| IoOp::wrap("inspect install entry at", target_dir, e))?
    {
        return remove_dev_link(target_dir)
            .map_err(|e| IoOp::wrap("remove dev link at", target_dir, e).into());
    }
    std::fs::remove_dir_all(target_dir).map_err(|e| {
        // On Windows, a running Houdini process holds open handles to files
        // inside the package dir, so removal fails with ERROR_ACCESS_DENIED
        // (os error 5 → PermissionDenied). Map it to an actionable error
        // instead of leaking a raw OS code.
        if e.kind() == std::io::ErrorKind::PermissionDenied {
            StorageError::PackageInUse {
                name: name.to_string(),
                version: version.to_string(),
                source: e,
            }
        } else {
            IoOp::wrap("remove install entry at", target_dir, e).into()
        }
    })
}

/// Replace whatever is currently at `target_dir` with a clean slate, with
/// link-aware removal semantics. Always safe to call before installing.
///
/// - Missing → no-op.
/// - Symlink/junction → remove the link entry itself; never follow.
/// - Real directory → `remove_dir_all`, with Houdini-handle errors lifted to
///   [`StorageError::PackageInUse`] so the user gets an actionable message.
pub(super) fn clear_existing_install(
    target_dir: &std::path::Path,
    name: &str,
    version: &str,
) -> Result<(), StorageError> {
    let meta = match std::fs::symlink_metadata(target_dir) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(IoOp::wrap("stat install target", target_dir, e).into()),
    };

    if is_link_entry(&meta, target_dir)
        .map_err(|e| IoOp::wrap("inspect install entry at", target_dir, e))?
    {
        warn!(
            "replacing existing link install for {}@{} at {}",
            name,
            version,
            target_dir.display()
        );
    } else {
        warn!(
            "package {}@{} already exists, removing old version",
            name, version
        );
    }
    remove_install_entry(target_dir, &meta, name, version)
}

/// Fingerprint the `source` workspace as a hex SHA-256 over every file's
/// relative path, byte length, and modification time. Stat-only — it never
/// reads file contents, so it stays cheap even for a native package with
/// hundreds of megabytes of DSOs.
///
/// The fingerprint compares the source against *itself over time* (via the
/// [`DEV_SRC_FINGERPRINT_FILE`] sidecar), never against the installed copy, so
/// it doesn't matter that `std::fs::copy` drops mtimes on the target: any
/// workspace rebuild bumps a source file's mtime (and usually its length),
/// which changes the digest and forces a recopy. Symlinks are skipped, matching
/// the file-only walk the CAS checksum uses.
fn source_fingerprint(source: &Path) -> std::io::Result<String> {
    let mut hasher = Sha256::new();

    let mut entries: Vec<_> = walkdir::WalkDir::new(source)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .collect();
    // Sort for a deterministic digest independent of readdir order.
    entries.sort_by(|a, b| a.path().cmp(b.path()));

    for entry in entries {
        let relative_path = entry.path().strip_prefix(source).unwrap_or(entry.path());
        hasher.update(relative_path_to_forward_slash(relative_path).as_bytes());
        hasher.update(b"\0");

        let meta = entry.metadata()?;
        hasher.update(meta.len().to_le_bytes());
        if let Ok(dur) = meta.modified().and_then(|m| {
            m.duration_since(std::time::UNIX_EPOCH)
                .map_err(std::io::Error::other)
        }) {
            hasher.update(dur.as_secs().to_le_bytes());
            hasher.update(dur.subsec_nanos().to_le_bytes());
        }
    }

    Ok(hex_digest(hasher))
}

/// Hex SHA-256 over a directory's file paths *and* contents, skipping the
/// [`DEV_SRC_FINGERPRINT_FILE`] sidecar (which lives only in the installed
/// copy, never in the source). Unlike [`source_fingerprint`] this reads every
/// byte, so it's reserved for the fallback comparison — proving two trees are
/// genuinely identical when the cheap mtime fingerprint is missing or stale.
fn content_digest(dir: &Path) -> std::io::Result<String> {
    let mut hasher = Sha256::new();

    let mut entries: Vec<_> = walkdir::WalkDir::new(dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter(|e| e.file_name() != DEV_SRC_FINGERPRINT_FILE)
        .collect();
    entries.sort_by(|a, b| a.path().cmp(b.path()));

    for entry in entries {
        let relative_path = entry.path().strip_prefix(dir).unwrap_or(entry.path());
        hasher.update(relative_path_to_forward_slash(relative_path).as_bytes());
        hasher.update(b"\0");

        let bytes = std::fs::read(entry.path())?;
        hasher.update((bytes.len() as u64).to_le_bytes());
        hasher.update(&bytes);
    }

    Ok(hex_digest(hasher))
}

fn hex_digest(hasher: Sha256) -> String {
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect()
}

/// Decide whether the existing dev copy at `target` already reflects `source`,
/// so the caller can skip the destructive clear-and-recopy.
///
/// - Missing target, or a link/junction entry → `Ok(false)`: there's nothing to
///   short-circuit, and a link must go through the link-safe removal path.
/// - Cheap path: the source's [`source_fingerprint`] matches the sidecar
///   recorded at the last copy → `Ok(true)`, stat-only, no bytes read.
/// - Fallback: the sidecar is missing (first launch after this feature shipped)
///   or stale from an mtime-only touch with no content change. A full
///   [`content_digest`] comparison confirms the trees are identical; on a match
///   the sidecar is refreshed so the next launch takes the cheap path. Writing
///   a *new* small file into the package dir succeeds even while Houdini has the
///   existing DSOs mapped, so this stays lock-free.
pub(super) fn dev_copy_is_current(source: &Path, target: &Path) -> std::io::Result<bool> {
    let meta = match std::fs::symlink_metadata(target) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e),
    };
    // A link/junction or non-directory entry can't be compared as a plain copy;
    // let clear_existing_install handle it with link-safe removal.
    if is_link_entry(&meta, target)? || !meta.is_dir() {
        return Ok(false);
    }

    let source_fp = source_fingerprint(source)?;

    let sidecar = target.join(DEV_SRC_FINGERPRINT_FILE);
    if let Ok(stored) = std::fs::read_to_string(&sidecar) {
        if stored.trim() == source_fp {
            return Ok(true);
        }
    }

    if content_digest(source)? == content_digest(target)? {
        if let Err(e) = std::fs::write(&sidecar, &source_fp) {
            warn!(
                "failed to refresh dev fingerprint at {}: {}",
                sidecar.display(),
                e
            );
        }
        return Ok(true);
    }

    Ok(false)
}

/// Record `source`'s fingerprint into `target`'s sidecar after a fresh dev
/// copy, so the next launch can take [`dev_copy_is_current`]'s cheap path.
/// Best-effort: a failure here only costs the next launch a content comparison,
/// so it never fails the install.
pub(super) fn write_dev_fingerprint(source: &Path, target: &Path) {
    match source_fingerprint(source) {
        Ok(fp) => {
            let sidecar = target.join(DEV_SRC_FINGERPRINT_FILE);
            if let Err(e) = std::fs::write(&sidecar, &fp) {
                warn!(
                    "failed to write dev fingerprint at {}: {}",
                    sidecar.display(),
                    e
                );
            }
        }
        Err(e) => warn!(
            "failed to compute dev fingerprint for {}: {}",
            source.display(),
            e
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression for the Windows reparse-point bug: a plain directory
    /// (a `DevCopy` install) must inspect as "not a link", not as
    /// an `ERROR_NOT_A_REPARSE_POINT` IoOp.
    ///
    /// The bug only surfaced on Windows (where `junction::exists` rides
    /// over a FSCTL ioctl that reports 4390 for non-reparse entries),
    /// but the *contract* — `is_link_entry` on a real directory returns
    /// `Ok(false)` — is platform-independent. Asserting it on every host
    /// keeps the contract from quietly re-regressing under future tightenings.
    #[test]
    fn plain_directory_inspects_as_non_link() {
        let tmp = tempfile::TempDir::new().unwrap();
        let dir = tmp.path().join("dev_copy_install");
        std::fs::create_dir(&dir).unwrap();
        let meta = std::fs::symlink_metadata(&dir).unwrap();

        let result = is_link_entry(&meta, &dir);
        assert!(
            matches!(result, Ok(false)),
            "expected Ok(false) for a plain directory, got {:?}",
            result
        );
    }

    /// On Unix, a symlink to a directory must inspect as a link. The
    /// Windows `junction` branch isn't exercised by this test (the entry
    /// is a symlink, so the early return fires first); pair with a manual
    /// Windows-side smoke test for full coverage.
    #[cfg(unix)]
    #[test]
    fn symlink_inspects_as_link() {
        let tmp = tempfile::TempDir::new().unwrap();
        let target = tmp.path().join("target");
        std::fs::create_dir(&target).unwrap();
        let link = tmp.path().join("the_link");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let meta = std::fs::symlink_metadata(&link).unwrap();

        assert!(matches!(is_link_entry(&meta, &link), Ok(true)));
    }

    /// Populate a fake dev copy of `src` at `dst` (plain recursive copy), then
    /// stamp the sidecar as the install path would.
    fn copy_and_stamp(src: &std::path::Path, dst: &std::path::Path) {
        for entry in walkdir::WalkDir::new(src).min_depth(1) {
            let entry = entry.unwrap();
            let rel = entry.path().strip_prefix(src).unwrap();
            let out = dst.join(rel);
            if entry.file_type().is_dir() {
                std::fs::create_dir_all(&out).unwrap();
            } else {
                if let Some(p) = out.parent() {
                    std::fs::create_dir_all(p).unwrap();
                }
                std::fs::copy(entry.path(), &out).unwrap();
            }
        }
        write_dev_fingerprint(src, dst);
    }

    fn write(path: &std::path::Path, contents: &str) {
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p).unwrap();
        }
        std::fs::write(path, contents).unwrap();
    }

    /// The happy path: an untouched workspace re-syncs as "current" via the
    /// cheap sidecar fingerprint, so the caller skips the destructive recopy.
    #[test]
    fn unchanged_source_is_current() {
        let tmp = tempfile::TempDir::new().unwrap();
        let src = tmp.path().join("src");
        let dst = tmp.path().join("dst");
        write(&src.join("hpm.toml"), "name = 'x'");
        write(&src.join("dso/plugin.so"), "BINARY");
        copy_and_stamp(&src, &dst);

        assert!(dev_copy_is_current(&src, &dst).unwrap());
    }

    /// A missing target has nothing to short-circuit.
    #[test]
    fn missing_target_is_not_current() {
        let tmp = tempfile::TempDir::new().unwrap();
        let src = tmp.path().join("src");
        write(&src.join("hpm.toml"), "name = 'x'");

        assert!(!dev_copy_is_current(&src, &tmp.path().join("absent")).unwrap());
    }

    /// Editing a source file (new length) makes the copy stale, forcing a recopy.
    #[test]
    fn edited_source_is_stale() {
        let tmp = tempfile::TempDir::new().unwrap();
        let src = tmp.path().join("src");
        let dst = tmp.path().join("dst");
        write(&src.join("hpm.toml"), "name = 'x'");
        write(&src.join("dso/plugin.so"), "BINARY");
        copy_and_stamp(&src, &dst);

        write(&src.join("dso/plugin.so"), "BINARY-REBUILT-LONGER");
        assert!(!dev_copy_is_current(&src, &dst).unwrap());
    }

    /// First launch after this feature shipped: no sidecar exists, but a
    /// byte-identical copy is still recognized via the content-digest fallback,
    /// which then writes the sidecar for a cheap next launch.
    #[test]
    fn missing_sidecar_falls_back_to_content_and_refreshes() {
        let tmp = tempfile::TempDir::new().unwrap();
        let src = tmp.path().join("src");
        let dst = tmp.path().join("dst");
        write(&src.join("hpm.toml"), "name = 'x'");
        write(&src.join("dso/plugin.so"), "BINARY");
        copy_and_stamp(&src, &dst);

        let sidecar = dst.join(DEV_SRC_FINGERPRINT_FILE);
        std::fs::remove_file(&sidecar).unwrap();
        assert!(!sidecar.exists());

        assert!(dev_copy_is_current(&src, &dst).unwrap());
        assert!(sidecar.exists(), "fallback should refresh the sidecar");
    }

    /// A stale sidecar (e.g. an mtime-only touch) but byte-identical content is
    /// still recognized as current through the content-digest fallback.
    #[test]
    fn stale_sidecar_but_identical_content_is_current() {
        let tmp = tempfile::TempDir::new().unwrap();
        let src = tmp.path().join("src");
        let dst = tmp.path().join("dst");
        write(&src.join("hpm.toml"), "name = 'x'");
        copy_and_stamp(&src, &dst);

        std::fs::write(dst.join(DEV_SRC_FINGERPRINT_FILE), "deadbeef-not-a-match").unwrap();
        assert!(dev_copy_is_current(&src, &dst).unwrap());
    }
}
