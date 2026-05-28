//! Path-dependency ("dev") install support: link/copy primitives and the
//! `_dev/` subtree under `packages_dir`.
//!
//! These helpers keep the platform-specific symlink/junction handling and
//! the install-entry removal semantics out of `storage.rs` so the
//! [`StorageManager`](super::StorageManager) impl can read top-down.

use super::error::StorageError;
use hpm_package::IoOp;
use std::path::PathBuf;
use tracing::warn;

/// Subdirectory of `packages_dir` reserved for path-installed (dev) packages.
/// Kept out of the registry CAS namespace so a dev install of `foo@1.0.0`
/// can coexist with — and is never substituted for — a registry install at
/// the same coordinate.
pub(super) const DEV_INSTALL_DIR: &str = "_dev";

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
}
