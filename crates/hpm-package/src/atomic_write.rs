//! Crash-safe file writes via stage-and-rename.
//!
//! A crash or killed process mid-write must never leave a half-written
//! lockfile, config, venv metadata, Houdini package file or install record:
//! the next load would refuse it (and for a venv, rebuild it). Every such
//! write goes through [`atomic_write`], which stages to a sibling and renames
//! onto the target.

use crate::IoOp;
use std::path::Path;
use std::path::PathBuf;

/// Stage `content` to a new sibling temporary file, then rename onto `path`.
///
/// The temporary file is created exclusively under a name of its own
/// (`<path>.<pid>-<n>.tmp`), so a file or link already sitting where it
/// would go (planted in a cloned project, or left by another writer) is
/// never opened or written through; the rename replaces `path` itself,
/// never a link's target. A `<path>.tmp` left by an older hpm's interrupted
/// save is removed (the entry itself, never what a link points at).
///
/// Errors are reported as [`IoOp`], so callers whose error enums carry
/// `#[from] IoOp` lift them with `?`. The `op` field on the returned
/// `IoOp` is `"stage"` for the temp-file write and `"commit"` for the
/// rename, with the path pointing at the *target* path in both cases
/// (the temp file is an implementation detail; the caller's error reads
/// "failed to commit /path/to/file").
///
/// ```no_run
/// use hpm_package::{IoOp, atomic_write};
/// use std::path::Path;
///
/// # fn try_main() -> Result<(), IoOp> {
/// atomic_write(Path::new("/var/lib/hpm/state.json"), b"{}")?;
/// # Ok(())
/// # }
/// ```
pub fn atomic_write(path: &Path, content: impl AsRef<[u8]>) -> Result<(), IoOp> {
    let content = content.as_ref();
    let legacy = legacy_tmp_sibling(path);
    if std::fs::symlink_metadata(&legacy).is_ok() {
        let _ = std::fs::remove_file(&legacy);
    }
    let tmp_path = retry_transient(|| create_staging_file(path, content))
        .map_err(|e| IoOp::wrap("stage", path, e))?;
    if let Err(e) = retry_transient(|| std::fs::rename(&tmp_path, path)) {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(IoOp::wrap("commit", path, e));
    }
    Ok(())
}

/// Create a fresh sibling of `path` with `create_new` (which refuses an
/// existing entry, links included) and write `content` to it. Returns its
/// path.
fn create_staging_file(path: &Path, content: &[u8]) -> std::io::Result<PathBuf> {
    use std::io::Write;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    loop {
        let mut name = path.as_os_str().to_os_string();
        name.push(format!(
            ".{}-{}.tmp",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let tmp = PathBuf::from(name);
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)
        {
            Ok(mut file) => {
                if let Err(e) = file.write_all(content) {
                    drop(file);
                    let _ = std::fs::remove_file(&tmp);
                    return Err(e);
                }
                return Ok(tmp);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
}

/// Run `op`, retrying briefly while it fails the way Windows fails a file
/// operation that another process interferes with for a moment: an
/// antivirus scanner or search indexer opening a just-written file without
/// delete sharing makes a rename onto it, or of it, fail with "access
/// denied" or a sharing violation until it lets go. Elsewhere, and for any
/// other error, the first result stands.
fn retry_transient<T>(mut op: impl FnMut() -> std::io::Result<T>) -> std::io::Result<T> {
    #[cfg(windows)]
    {
        // ERROR_ACCESS_DENIED, ERROR_SHARING_VIOLATION, ERROR_LOCK_VIOLATION.
        const TRANSIENT: [i32; 3] = [5, 32, 33];
        for delay_ms in [1, 2, 5, 10, 20, 50, 100, 200, 400] {
            match op() {
                Err(e)
                    if e.raw_os_error()
                        .is_some_and(|code| TRANSIENT.contains(&code)) =>
                {
                    std::thread::sleep(std::time::Duration::from_millis(delay_ms));
                }
                result => return result,
            }
        }
    }
    op()
}

/// The fixed `<path>.tmp` older hpm versions staged to.
fn legacy_tmp_sibling(path: &Path) -> PathBuf {
    let mut tmp = path.as_os_str().to_os_string();
    tmp.push(".tmp");
    PathBuf::from(tmp)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn writes_content_to_target() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("state.json");

        atomic_write(&target, b"{\"hello\": 1}").unwrap();

        assert_eq!(fs::read_to_string(&target).unwrap(), "{\"hello\": 1}");
        // The .tmp file should not remain after a successful rename.
        assert!(!dir.path().join("state.json.tmp").exists());
    }

    #[test]
    fn overwrites_stale_tmp_from_interrupted_save() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("hpm.lock");
        let stale_tmp = dir.path().join("hpm.lock.tmp");

        // Simulate a previously crashed save: a leftover `.tmp` with
        // garbage in it. Subsequent atomic_writes must replace it
        // rather than fail.
        fs::write(&stale_tmp, b"GARBAGE").unwrap();

        atomic_write(&target, b"fresh").unwrap();

        assert_eq!(fs::read_to_string(&target).unwrap(), "fresh");
        assert!(!stale_tmp.exists());
    }

    /// A scanner holding the target open without delete sharing for a
    /// moment (as Windows Defender does with a file it has just seen
    /// written) must not fail the write: the rename is retried until it
    /// lets go.
    #[cfg(windows)]
    #[test]
    fn a_target_held_open_for_a_moment_is_still_replaced() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("record.json");
        fs::write(&target, b"old").unwrap();
        // FILE_SHARE_READ only: no delete sharing, so a rename onto it fails.
        let held = fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&target)
            .unwrap();
        let releaser = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(100));
            drop(held);
        });
        atomic_write(&target, b"new").unwrap();
        releaser.join().unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), "new");
    }

    #[test]
    fn overwrites_existing_target() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("config.toml");
        fs::write(&target, b"old").unwrap();

        atomic_write(&target, b"new").unwrap();

        assert_eq!(fs::read_to_string(&target).unwrap(), "new");
    }

    #[test]
    fn returns_iop_when_parent_missing() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("does/not/exist/state.json");

        let err = atomic_write(&target, b"x").unwrap_err();

        // The verb identifies which step failed, the path is the
        // caller-supplied target (not the .tmp implementation detail).
        assert_eq!(err.op, "stage");
        assert_eq!(err.path, target);
    }
}
