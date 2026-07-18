//! Per-project paths derived from a project root (`.hpm/packages/`,
//! `hpm.lock`, `hpm.toml`).

use hpm_package::PackagePath;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize)]
pub struct ProjectPaths {
    pub packages_dir: PathBuf,
    pub lock_file: PathBuf,
    pub manifest_file: PathBuf,
}

impl ProjectPaths {
    pub fn ensure_directories(&self) -> std::io::Result<()> {
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
