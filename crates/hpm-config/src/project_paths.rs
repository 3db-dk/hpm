//! Per-project paths derived from a project root (`.hpm/packages/`,
//! `hpm.lock`, `hpm.toml`).

use hpm_package::PackagePath;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectPaths {
    pub packages_dir: PathBuf,
    pub lock_file: PathBuf,
    pub manifest_file: PathBuf,
}

impl ProjectPaths {
    /// Create `.hpm/packages/`, refusing a `.hpm` or `.hpm/packages` that
    /// is a link: hpm writes and deletes package files there, and a project
    /// directory is often a repository someone else wrote, so a link would
    /// point those writes and deletions anywhere (Houdini's own packages
    /// folder, say). Links above the project root are not affected.
    pub fn ensure_directories(&self) -> std::io::Result<()> {
        let hpm_dir = self.packages_dir.parent();
        for dir in hpm_dir.into_iter().chain([self.packages_dir.as_path()]) {
            if std::fs::symlink_metadata(dir).is_ok_and(|m| m.file_type().is_symlink()) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!(
                        "{} is a link; hpm only writes into a project's own .hpm directory",
                        dir.display()
                    ),
                ));
            }
        }
        std::fs::create_dir_all(&self.packages_dir)?;
        Ok(())
    }

    /// Path of the Houdini manifest hpm emits for `path`.
    ///
    /// Named `<creator>.<slug>.json`. The creator segment is part of the
    /// filename because the slug alone is not unique — two creators may
    /// publish the same slug, and keying the file on the slug let the
    /// second install silently overwrite the first.
    pub fn package_manifest_path(&self, path: &PackagePath) -> PathBuf {
        self.packages_dir.join(format!("{}.json", path.file_stem()))
    }
}
