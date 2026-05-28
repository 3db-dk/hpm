//! Installation settings: where packages land inside a project and how many
//! downloads run in parallel.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallConfig {
    #[serde(default = "default_install_path")]
    pub path: String,
    #[serde(default = "default_parallel_downloads")]
    pub parallel_downloads: usize,
}

pub(crate) fn default_install_path() -> String {
    "packages/hpm".to_string()
}

pub(crate) fn default_parallel_downloads() -> usize {
    8
}

impl Default for InstallConfig {
    fn default() -> Self {
        Self {
            path: default_install_path(),
            parallel_downloads: default_parallel_downloads(),
        }
    }
}
