//! Per-project paths derived from a project root (`.hpm/packages/`,
//! `hpm.lock`, `hpm.toml`).

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

    pub fn package_manifest_path(&self, name: &str) -> PathBuf {
        self.packages_dir.join(format!("{}.json", name))
    }
}
