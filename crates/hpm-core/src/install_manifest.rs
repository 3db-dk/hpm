//! Per-file record of an installed package, and the check against it.
//!
//! When a downloaded package is copied into the store, hpm writes down every
//! file it installed: its path, its size and its SHA-256. [`verify`] compares
//! the installed tree with that record and reports each recorded file that is
//! missing, shorter than it was written, or holds different bytes.
//!
//! Two things shape it:
//!
//! - **Only recorded files are checked.** Houdini and Python write into a
//!   package's directory as it is used (`__pycache__`, `.pyc`, an HDA's backup
//!   folder), so a digest of the whole tree would call every used package
//!   damaged. A file that is not in the record is never reported.
//! - **The record lives next to the package, not inside it**, as
//!   `<slug>@<version>.files.json` beside `<slug>@<version>/`, like
//!   [`crate::exec_mode`]'s stamp: [`crate::tree_hash`] digests every file in
//!   the tree, so an in-tree record would change the checksum `hpm.lock`
//!   verifies, and Houdini would see it as part of the package.
//!
//! The record is written as the *last* step of an install
//! ([`crate::StorageManager::install_into_cas`]) and removed as the first step
//! of a removal, so an install interrupted part-way through its copy leaves a
//! tree whose record is either the previous install's (gone with it) or
//! absent. A tree installed before this module existed has no record at all:
//! [`Verification::Unrecorded`] says so, and nothing is guessed about it.
//!
//! Only content is recorded. File modes are [`crate::exec_mode`]'s concern,
//! and timestamps change whenever anything touches a file.

use hpm_package::IoOp;
use hpm_package::path_util::relative_path_to_forward_slash;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use tracing::warn;

/// Suffix of the sibling file holding a package's record.
pub const RECORD_SUFFIX: &str = ".files.json";

/// The record format this hpm writes and reads.
const FORMAT: u32 = 1;

/// Every file an install put into a package directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstallManifest {
    /// Record format; a record from a newer hpm is refused rather than
    /// misread.
    pub format: u32,
    /// One entry per regular file, sorted by path.
    pub files: Vec<FileRecord>,
}

/// One installed file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileRecord {
    /// Path relative to the package directory, with forward slashes.
    pub path: String,
    /// Size in bytes.
    pub size: u64,
    /// Lowercase hex SHA-256 of the contents.
    pub sha256: String,
}

/// What [`verify`] found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Verification {
    /// The package has no record: it was installed by an hpm older than this
    /// module, or is not a store install (a dev copy, a link). Nothing can be
    /// said about its files.
    Unrecorded,
    /// The tree was compared with its record.
    Checked(IntegrityReport),
}

/// The result of comparing a tree with its record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IntegrityReport {
    /// Number of recorded files.
    pub files: usize,
    /// The recorded files that are not as they were installed, in record
    /// order. Empty when the tree is intact.
    pub problems: Vec<FileProblem>,
}

impl IntegrityReport {
    /// True when every recorded file is as it was installed.
    pub fn is_intact(&self) -> bool {
        self.problems.is_empty()
    }
}

/// A recorded file that is not as it was installed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileProblem {
    /// The file's recorded path (forward slashes, relative to the package).
    pub path: String,
    /// What is wrong with it.
    pub kind: ProblemKind,
}

/// How a recorded file differs from its record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProblemKind {
    /// Nothing at the path, or something that is not a regular file.
    Missing,
    /// Shorter than it was written: a download, extraction or copy cut
    /// short.
    Truncated { expected: u64, actual: u64 },
    /// Same path, different bytes: longer than it was written, or the same
    /// size with other contents.
    Changed,
    /// The file is there but could not be read.
    Unreadable { error: String },
}

/// Path of the record for `package_dir`, or `None` for a directory with no
/// usable name (a root path).
pub fn record_path(package_dir: &Path) -> Option<PathBuf> {
    let name = package_dir.file_name()?.to_str()?;
    Some(package_dir.with_file_name(format!("{name}{RECORD_SUFFIX}")))
}

/// Record every regular file under `dir` (blocking).
///
/// Walk and read errors propagate: a record built over a partly unreadable
/// tree would describe files that were never installed that way.
pub fn record_tree(dir: &Path) -> Result<InstallManifest, IoOp> {
    let mut files = Vec::new();
    for entry in walkdir::WalkDir::new(dir).min_depth(1) {
        let entry = entry.map_err(|e| {
            let path = e
                .path()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| dir.to_path_buf());
            IoOp::wrap(
                "walk directory to record",
                &path,
                e.into_io_error()
                    .unwrap_or_else(|| std::io::Error::other("walk error")),
            )
        })?;
        if !entry.file_type().is_file() {
            continue;
        }
        let relative = entry.path().strip_prefix(dir).unwrap_or(entry.path());
        let (size, sha256) = hash_file(entry.path())
            .map_err(|e| IoOp::wrap("read file to record", entry.path(), e))?;
        files.push(FileRecord {
            path: relative_path_to_forward_slash(relative),
            size,
            sha256,
        });
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(InstallManifest {
        format: FORMAT,
        files,
    })
}

/// Write `manifest` as the record for `package_dir`, atomically: a reader
/// sees the previous record or this one, never half of one.
pub fn write(package_dir: &Path, manifest: &InstallManifest) -> Result<(), IoOp> {
    let path = record_path(package_dir).ok_or_else(|| {
        IoOp::wrap(
            "name the install record for",
            package_dir,
            std::io::Error::other("package directory has no name"),
        )
    })?;
    let json = serde_json::to_vec(manifest)
        .map_err(|e| IoOp::wrap("encode install record", &path, std::io::Error::other(e)))?;
    let mut tmp = path.clone().into_os_string();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, json).map_err(|e| IoOp::wrap("write install record", &tmp, e))?;
    if let Err(e) = std::fs::rename(&tmp, &path) {
        // Windows' rename refuses to replace an existing file.
        if path.exists() {
            let _ = std::fs::remove_file(&path);
            if std::fs::rename(&tmp, &path).is_ok() {
                return Ok(());
            }
        }
        let _ = std::fs::remove_file(&tmp);
        return Err(IoOp::wrap("commit install record", &path, e));
    }
    Ok(())
}

/// Read the record for `package_dir`. `Ok(None)` when there is none; an
/// error when one exists but cannot be read, is not a record, names a path
/// outside the package, or was written by a newer hpm.
pub fn read(package_dir: &Path) -> Result<Option<InstallManifest>, IoOp> {
    let Some(path) = record_path(package_dir) else {
        return Ok(None);
    };
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(IoOp::wrap("read install record", &path, e)),
    };
    let bad = |why: String| IoOp::wrap("parse install record", &path, std::io::Error::other(why));
    let manifest: InstallManifest =
        serde_json::from_slice(&bytes).map_err(|e| bad(e.to_string()))?;
    if manifest.format != FORMAT {
        return Err(bad(format!(
            "record format {} is not one this hpm reads ({FORMAT})",
            manifest.format
        )));
    }
    if let Some(entry) = manifest.files.iter().find(|f| !is_inside(&f.path)) {
        return Err(bad(format!(
            "entry {:?} is not inside the package",
            entry.path
        )));
    }
    Ok(Some(manifest))
}

/// Remove the record for `package_dir`, if any. Called when the package
/// directory is removed or about to be replaced, so a record never outlives
/// the tree it describes.
pub fn forget(package_dir: &Path) {
    if let Some(path) = record_path(package_dir)
        && let Err(e) = std::fs::remove_file(&path)
        && e.kind() != std::io::ErrorKind::NotFound
    {
        warn!("Could not remove install record {}: {}", path.display(), e);
    }
}

/// Compare `package_dir` with its record (blocking).
pub fn verify(package_dir: &Path) -> Result<Verification, IoOp> {
    Ok(match read(package_dir)? {
        None => Verification::Unrecorded,
        Some(manifest) => Verification::Checked(check(package_dir, &manifest)),
    })
}

/// Compare `dir` with `manifest`. Never fails: a file that cannot be read is
/// reported as [`ProblemKind::Unreadable`] rather than aborting the check.
pub fn check(dir: &Path, manifest: &InstallManifest) -> IntegrityReport {
    let problems = manifest
        .files
        .iter()
        .filter_map(|record| {
            check_file(dir, record).map(|kind| FileProblem {
                path: record.path.clone(),
                kind,
            })
        })
        .collect();
    IntegrityReport {
        files: manifest.files.len(),
        problems,
    }
}

fn check_file(dir: &Path, record: &FileRecord) -> Option<ProblemKind> {
    let path = dir.join(&record.path);
    let meta = match std::fs::symlink_metadata(&path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Some(ProblemKind::Missing),
        Err(e) => {
            return Some(ProblemKind::Unreadable {
                error: e.to_string(),
            });
        }
    };
    if !meta.is_file() {
        return Some(ProblemKind::Missing);
    }
    let actual = meta.len();
    if actual < record.size {
        return Some(ProblemKind::Truncated {
            expected: record.size,
            actual,
        });
    }
    if actual > record.size {
        return Some(ProblemKind::Changed);
    }
    match hash_file(&path) {
        Ok((_, sha256)) if sha256 == record.sha256 => None,
        Ok(_) => Some(ProblemKind::Changed),
        Err(e) => Some(ProblemKind::Unreadable {
            error: e.to_string(),
        }),
    }
}

/// True if a recorded path names a file strictly inside the package.
fn is_inside(path: &str) -> bool {
    let p = Path::new(path);
    !path.is_empty()
        && !path.contains('\\')
        && p.components().all(|c| matches!(c, Component::Normal(_)))
}

/// Size and SHA-256 of a file, streamed.
fn hash_file(path: &Path) -> std::io::Result<(u64, String)> {
    let mut hasher = Sha256::new();
    let mut file = std::fs::File::open(path)?;
    let mut buffer = [0u8; 64 * 1024];
    let mut size = 0u64;
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        size += n as u64;
        hasher.update(&buffer[..n]);
    }
    let hex = hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    Ok((size, hex))
}

#[cfg(test)]
mod tests;
