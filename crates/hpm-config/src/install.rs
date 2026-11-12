//! Installation settings: how many package downloads a sync runs at once.

use serde::{Deserialize, Serialize};
use std::num::NonZeroUsize;

#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstallConfig {
    /// The most registry or URL packages a sync fetches and installs at once.
    #[serde(default = "default_parallel_downloads")]
    pub parallel_downloads: NonZeroUsize,
}

pub(crate) fn default_parallel_downloads() -> NonZeroUsize {
    NonZeroUsize::new(8).expect("8 is not zero")
}

impl Default for InstallConfig {
    fn default() -> Self {
        Self {
            parallel_downloads: default_parallel_downloads(),
        }
    }
}
