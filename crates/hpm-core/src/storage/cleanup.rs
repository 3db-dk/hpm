//! [`ComprehensiveCleanupResult`] — aggregate of a full
//! packages + dev installs + Python venvs cleanup pass.

use hpm_python::cleanup::CleanupResult;

/// Result of comprehensive cleanup including both packages and Python environments
#[derive(Debug)]
pub struct ComprehensiveCleanupResult {
    pub removed_packages: Vec<String>,
    /// Orphaned dev (path-dep) installs removed from `_dev/`. Identifiers are
    /// `_dev/<slug>@<version>` so CLI output makes the source obvious.
    pub removed_dev_installs: Vec<String>,
    pub python_cleanup: CleanupResult,
}

impl ComprehensiveCleanupResult {
    /// Total number of items cleaned (packages + dev installs + venvs)
    pub fn total_items_cleaned(&self) -> usize {
        self.removed_packages.len()
            + self.removed_dev_installs.len()
            + self.python_cleanup.items_cleaned()
    }

    /// Total number of items that would be cleaned (packages + dev installs + venvs)
    pub fn total_items_that_would_be_cleaned(&self) -> usize {
        self.removed_packages.len()
            + self.removed_dev_installs.len()
            + self.python_cleanup.items_that_would_be_cleaned()
    }
}
