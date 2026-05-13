//! `hpm update` — stub.
//!
//! This subcommand is not implemented yet. The previous implementation
//! returned fabricated update information (placeholder `find_available_updates`,
//! hardcoded `query_pypi_latest`, etc.) and gave users false confidence
//! that updates were being discovered. Better to fail loudly than lie.
//!
//! Real implementation requires:
//! - Querying each registry for the latest version matching each spec's
//!   `VersionReq` (semver), filtering yanked, picking the highest.
//! - Comparing against `hpm.lock` to compute the diff.
//! - Re-running the install/sync flow with the updated specs.
//! - For Python deps: re-resolving via UV with the same input set; UV picks
//!   the latest within constraints by default.
//!
//! Tracked as a follow-up to the install/sync consolidation that lands first.

use crate::output::OutputFormat;
use anyhow::{Result, bail};
use hpm_config::Config;
use std::path::PathBuf;

/// Fields are populated by main.rs from CLI flags and currently unread —
/// the real `update_packages` impl will consume them once it lands.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct UpdateOptions {
    pub package: Option<PathBuf>,
    pub packages: Vec<String>,
    pub dry_run: bool,
    pub yes: bool,
    pub output: OutputFormat,
}

impl Default for UpdateOptions {
    fn default() -> Self {
        Self {
            package: None,
            packages: Vec::new(),
            dry_run: false,
            yes: false,
            output: OutputFormat::Human,
        }
    }
}

pub async fn update_packages(_config: &Config, _options: UpdateOptions) -> Result<()> {
    bail!(
        "hpm update is not implemented yet.\n\
         \n\
         The previous implementation reported fabricated updates and was \
         removed in favour of failing loudly. A real implementation will land \
         alongside the install/sync consolidation.\n\
         \n\
         For now: edit hpm.toml manually, delete hpm.lock, and re-run hpm install."
    )
}
