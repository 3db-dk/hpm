//! # HPM Core
//!
//! Core orchestration for the Houdini Package Manager: package storage,
//! fetching, project sync, Houdini manifest emission, packing, and cleanup.
//!
//! ## Modules
//!
//! - [`project`] — [`ProjectManager`]: install/sync a project's `hpm.toml`
//!   dependencies and emit the Houdini `packages/*.json` manifests
//! - [`storage`] — [`StorageManager`]: the global content-addressed store
//!   under `~/.hpm/packages/` (plus `_dev/` for path installs) and the
//!   project-aware cleanup pipeline
//! - [`archive_fetcher`] — download, verify, and extract package archives
//! - [`packer`] — build, checksum, and sign release archives (`hpm pack`)
//! - [`registry`] — registry resolution ([`RegistrySet`])
//! - [`lock`] — `hpm.lock` reading/writing and checksum verification
//! - [`discovery`] / [`graph`] — project discovery and dependency
//!   reachability, backing `hpm clean`
//! - [`python`] — bundled uv, venv management, Houdini→Python ABI mapping
//! - [`script_run`] — shared `[scripts]` runner for embedders
//! - [`asset_index`], [`fetch_manifest`], [`package_source`], [`tree_hash`]
//!   — supporting building blocks
//!
//! ## Storage layout
//!
//! ```text
//! ~/.hpm/
//! ├── packages/            # global package store
//! │   ├── slug@1.0.0/      # CAS install (registry/URL content)
//! │   └── _dev/            # path-dep installs (links / hashed copies)
//! ├── cache/               # downloaded archive cache
//! ├── fetch/               # fetcher extraction staging
//! ├── venvs/               # shared Python virtual environments
//! └── registry/            # registry index cache
//! ```
//!
//! Per project, generated Houdini manifests land in `.hpm/packages/` next to
//! the project's `hpm.toml`, referencing the global store by absolute path.
//! Cleanup is project-aware: a package reachable from any discovered
//! project's dependency graph is never removed.

pub mod archive_fetcher;
pub mod asset_index;
pub mod discovery;
pub mod fetch_manifest;
pub mod graph;
pub(crate) mod http;
pub mod lock;
pub mod package_source;
pub mod packer;
pub(crate) mod process_util;
pub mod project;
pub mod python;
pub mod registry;
pub mod script_run;
pub mod storage;
pub mod tree_hash;

// ==========================================================================
// Stable library API
// ==========================================================================
//
// Re-exports below are organised by how downstream consumers (CLI, the
// TumbleTrove desktop client) tend to reach for them. Everything here is
// also accessible via the submodule path (`hpm_core::project::ProjectManager`
// etc.); the top-level aliases just spare callers a deeper import.

// Project orchestration — the entry point most library consumers use.
pub use project::{InstallOutcome, PackageRunEnv, ProjectDependency, ProjectError, ProjectManager};

// Shared `[scripts]` runner. Embedders implement `ScriptSink` (spawn +
// diagnostics) and drive `run_script` / `run_prepack`; the script-env
// contract (PATH/VIRTUAL_ENV/PYTHONPATH/HPM_PACKAGE_ROOT) lives in one place.
pub use script_run::{
    PreparedScript, ScriptRunError, ScriptSink, prepare_script, run_prepack, run_script,
};

// Configuration. Re-exported from hpm-config so a single `hpm-core` dep
// covers both for embedded callers.
pub use hpm_config::{Config, RegistrySourceConfig, RegistryType};

// Storage / CAS layer.
pub use storage::{StorageError, StorageManager};

// Registry: prefer `RegistrySet::from_config(&Config)` as the entry point.
// `ApiRegistry`/`GitRegistry` are only useful if you're building a custom
// `RegistrySet`.
pub use registry::{
    ApiRegistry, GitRegistry, Registry, RegistryEntry, RegistryError, RegistrySet, SearchResults,
};

// Lock file. `LockFile::load` / `LockFile::save` and `verify_checksums`
// are the common surface; the sub-types are needed to read or build
// individual entries.
pub use lock::{
    LockError, LockFile, LockMetadata, LockPackageInfo, LockedDependency, LockedPythonDependency,
    LockedSource,
};

// Project discovery and reachability — `hpm clean` machinery, but
// re-exported for downstream tools that want to do orphan analysis
// against a custom project set.
pub use discovery::{DiscoveredProject, DiscoveryError, ProjectDiscovery};
pub use graph::{DependencyError, DependencyGraph, DependencyResolver, PackageId, PackageNode};

// Lower-level building blocks. Used by the CLI's install + pack commands
// today. Stay re-exported because the boundary between "library callers
// shouldn't touch this" and "library callers want to script their own
// install/pack flow" isn't sharp; if a downstream consumer needs them,
// they should be reachable from the top-level alias.
pub use archive_fetcher::{ArchiveFetcher, FetchError, FetchResult};
pub use asset_index::{AssetIndex, AssetIndexError, collect_assets};
pub use fetch_manifest::{FetchManifestError, fetch_manifest};
pub use package_source::{PackageSource, PackageSourceError};
pub use packer::{PackError, PackResult};
