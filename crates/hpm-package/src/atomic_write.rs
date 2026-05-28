//! Crash-safe file writes via stage-and-rename.
//!
//! `LockFile::save`, `Config::save`, `VenvMetadata::save`, and the Houdini
//! package.json emitter all wrote the target file in place. A crash or
//! killed process mid-write would leave a half-written file that the next
//! load would refuse — and in venv's case, would force an expensive
//! full venv rebuild via the self-heal path.
//!
//! All four call sites had collapsed into the same five lines: build a
//! `<path>.tmp` sibling, write into it, rename onto the target. This
//! module owns that pattern so a future fix (e.g. fsync the parent dir,
//! handle Windows rename semantics) lands in one place.

use crate::IoOp;
use std::path::Path;
use std::path::PathBuf;

/// Stage `content` to `<path>.tmp`, then rename onto `path`. Overwrites any
/// pre-existing `<path>.tmp` from a prior interrupted save.
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
    let tmp_path = tmp_sibling(path);
    std::fs::write(&tmp_path, content).map_err(|e| IoOp::wrap("stage", path, e))?;
    std::fs::rename(&tmp_path, path).map_err(|e| IoOp::wrap("commit", path, e))?;
    Ok(())
}

fn tmp_sibling(path: &Path) -> PathBuf {
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
