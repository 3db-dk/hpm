//! Public data types returned by [`ProjectManager`] operations.

use crate::lock::LockedSource;
use crate::storage::InstalledPackage;

#[derive(Debug, Clone)]
pub struct ProjectDependency {
    pub name: String,
    pub version: String,
    pub installed_package: Option<InstalledPackage>,
}

/// Per-dependency record returned from `sync_dependencies`, carrying the
/// install path plus the metadata a lockfile needs.
///
/// `checksum` and `source` are `Option` because a sync that short-circuits
/// on the CAS (already-installed package) doesn't re-fetch from the
/// registry, so it has no fresh SHA-256 and (for `Simple`/`Registry` specs)
/// no fresh URL to record. Callers wanting lockfile fidelity can backfill
/// those `None` fields from a prior lockfile entry.
#[derive(Debug, Clone)]
pub struct InstallOutcome {
    pub package: InstalledPackage,
    /// SHA-256 of the archive — `Some` when the dep was freshly fetched,
    /// `None` for path deps and short-circuited CAS hits.
    pub checksum: Option<String>,
    /// Lockfile source — `Some` when we know the URL (fresh fetch or `Url`
    /// spec) or for path deps, `None` for `Simple`/`Registry` short-circuits.
    pub source: Option<LockedSource>,
}
