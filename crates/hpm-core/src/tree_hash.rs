//! Deterministic SHA-256 content hash of a directory tree.
//!
//! This is the digest recorded in `hpm.lock` at install time and recomputed
//! by `LockFile::verify_checksums` — the two sides must agree byte-for-byte,
//! so there is exactly one implementation.

use hpm_package::IoOp;
use hpm_package::path_util::relative_path_to_forward_slash;
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::Path;

/// Name of the checksum cache file older hpm versions wrote into package
/// directories. Skipped so a stray leftover can't perturb the digest.
pub const CHECKSUM_CACHE_FILE: &str = ".hpm-checksum";

/// SHA-256 over every file's forward-slash relative path and contents, in
/// sorted order. Walk and read errors propagate — a digest computed over a
/// silently truncated tree would misreport the package hash.
pub fn hash_tree(dir: &Path) -> Result<String, IoOp> {
    let mut hasher = Sha256::new();

    let mut entries = Vec::new();
    for entry in walkdir::WalkDir::new(dir).sort_by_file_name() {
        let entry = entry.map_err(|e| {
            let path = e
                .path()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| dir.to_path_buf());
            IoOp::wrap(
                "walk directory for checksum",
                &path,
                e.into_io_error()
                    .unwrap_or_else(|| std::io::Error::other("walk error")),
            )
        })?;
        if !entry.file_type().is_file() || entry.file_name() == CHECKSUM_CACHE_FILE {
            continue;
        }
        entries.push(entry.path().to_path_buf());
    }
    entries.sort();

    let mut buffer = [0u8; 8192];
    for path in entries {
        let relative = path.strip_prefix(dir).unwrap_or(&path);
        hasher.update(relative_path_to_forward_slash(relative).as_bytes());

        let mut file = std::fs::File::open(&path)
            .map_err(|e| IoOp::wrap("open file for checksum", &path, e))?;
        loop {
            let n = file
                .read(&mut buffer)
                .map_err(|e| IoOp::wrap("read file for checksum", &path, e))?;
            if n == 0 {
                break;
            }
            hasher.update(&buffer[..n]);
        }
    }

    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write(path: &Path, content: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    #[test]
    fn identical_trees_hash_identically() {
        let a = TempDir::new().unwrap();
        let b = TempDir::new().unwrap();
        for root in [a.path(), b.path()] {
            write(&root.join("hpm.toml"), "[package]");
            write(&root.join("python/mod.py"), "x = 1");
        }
        assert_eq!(hash_tree(a.path()).unwrap(), hash_tree(b.path()).unwrap());
    }

    #[test]
    fn content_change_changes_hash() {
        let dir = TempDir::new().unwrap();
        write(&dir.path().join("hpm.toml"), "[package]");
        let before = hash_tree(dir.path()).unwrap();
        write(&dir.path().join("hpm.toml"), "[package]\nchanged = true");
        assert_ne!(before, hash_tree(dir.path()).unwrap());
    }

    #[test]
    fn stray_checksum_cache_file_is_ignored() {
        let dir = TempDir::new().unwrap();
        write(&dir.path().join("hpm.toml"), "[package]");
        let before = hash_tree(dir.path()).unwrap();
        write(&dir.path().join(CHECKSUM_CACHE_FILE), "deadbeef");
        assert_eq!(before, hash_tree(dir.path()).unwrap());
    }
}
