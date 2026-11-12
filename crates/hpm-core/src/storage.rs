//! Global package store: CAS-backed install, removal, and orphan cleanup.
//!
//! [`StorageManager`] owns `~/.hpm/packages/`, `_dev/` for path installs,
//! and the cleanup pipeline. The supporting types live in submodules:
//!
//! - [`error`] — [`StorageError`]
//! - [`types`] — [`InstalledPackage`]
//! - [`layout`] — [`StoreLayout`], where every store and download path lives
//! - [`dev_install`] — `_dev/` path-install primitives (link, copy, remove)
//! - [`cleanup`] — the project-aware GC pipeline and
//!   [`ComprehensiveCleanupResult`] aggregate

use hpm_config::StorageConfig;
use hpm_package::{IoOp, ManifestLoadError, PackageManifest};
use std::path::PathBuf;
use tracing::{debug, info, warn};

pub mod cleanup;
pub mod dev_install;
pub mod dev_users;
pub mod error;
pub mod layout;
pub mod types;

pub use cleanup::ComprehensiveCleanupResult;
pub use dev_install::DevInstall;
pub use error::StorageError;
pub use layout::StoreLayout;
pub use types::InstalledPackage;

use dev_install::{
    DEV_INSTALL_DIR, InstallStyle, clear_container_link, clear_existing_install,
    commit_staged_copy, copy_dev_image, create_dev_link, dev_copy_image, dev_copy_is_complete,
    dev_copy_target, remove_install_entry, stage_dir,
};
/// Whether `dir` holds a store copy an install committed: a real directory
/// (not a link) with its `hpm.toml`.
fn is_committed_copy(dir: &std::path::Path) -> bool {
    std::fs::symlink_metadata(dir).is_ok_and(|m| m.is_dir() && !dev_install::is_link(&m))
        && dir.join("hpm.toml").is_file()
}

/// Remove a staging directory this process made and no longer needs. A
/// failure is logged; the leftover is reclaimed by
/// [`StorageManager::sweep_fetch_staging`] once this process is gone.
fn remove_stage(staged: &std::path::Path) {
    if let Err(e) = std::fs::remove_dir_all(staged)
        && e.kind() != std::io::ErrorKind::NotFound
    {
        warn!(
            "Could not remove staging directory {}: {e}",
            staged.display()
        );
    }
}

#[derive(Debug, Clone)]
pub struct StorageManager {
    pub config: StorageConfig,
}

impl StorageManager {
    pub fn new(config: StorageConfig) -> Result<Self, StorageError> {
        let manager = Self { config };
        manager.ensure_directories()?;
        Ok(manager)
    }

    fn ensure_directories(&self) -> Result<(), StorageError> {
        self.config.ensure_directories().map_err(|e| {
            IoOp::wrap("create storage directories under", &self.config.home_dir, e)
        })?;
        info!("Ensured storage directories exist");
        Ok(())
    }

    /// The layout of this store: where packages, dev containers and
    /// downloads live.
    pub fn layout(&self) -> StoreLayout {
        StoreLayout::new(&self.config)
    }

    /// Whether the store holds `name@version`. `name` is a dependency key
    /// (`creator/slug`) or a bare slug; the store keys by slug.
    pub fn package_exists(&self, name: &str, version: &str) -> bool {
        self.get_package_path(name, version)
            .join("hpm.toml")
            .exists()
    }

    /// The store directory for `name@version` (see [`StoreLayout::cas_dir`]).
    pub fn get_package_path(&self, name: &str, version: &str) -> PathBuf {
        self.layout().cas_dir(name, version)
    }

    /// Sweep every installed package for programs that lost their executable
    /// bit, returning `(trees_swept, files_repaired)`.
    ///
    /// The per-package sweep already runs wherever hpm reuses a tree, but that
    /// only reaches packages a caller happens to touch. An embedder that wants
    /// the store made consistent as part of *its* update — rather than the
    /// next time the user opens the right screen — calls this once at startup.
    /// Stamps make it cheap to repeat: a tree already swept costs one small
    /// read, so only a store carrying pre-repair installs does real work.
    ///
    /// A store that cannot be listed is not an error worth failing a startup
    /// over; it is logged and reported as nothing swept.
    pub fn repair_exec_modes(&self) -> (usize, usize) {
        let installed = match self.list_installed() {
            Ok(installed) => installed,
            Err(e) => {
                warn!("Could not list packages for the executable-mode sweep: {e}");
                return (0, 0);
            }
        };

        let mut swept = 0;
        let mut repaired = 0;
        for pkg in &installed {
            let fixed = crate::exec_mode::ensure_repaired(&pkg.install_path);
            if fixed > 0 {
                info!(
                    "Restored the executable bit on {} file(s) in {}@{}",
                    fixed,
                    pkg.slug(),
                    pkg.version
                );
                repaired += fixed;
            }
            swept += 1;
        }
        (swept, repaired)
    }

    /// The directories downloads are extracted into before they are copied
    /// into the store: the current one, and the one older hpm versions used
    /// when it differs (see [`StoreLayout::legacy_fetch_dir`]).
    fn fetch_staging_dirs(&self) -> Vec<PathBuf> {
        let layout = self.layout();
        let mut dirs = vec![layout.fetch_dir().to_path_buf()];
        dirs.extend(layout.legacy_fetch_dir());
        dirs
    }

    /// Remove what interrupted or older installs left behind, returning
    /// `(entries_removed, bytes_freed)`: extracted downloads in fetch staging
    /// (current and legacy locations) older than `min_age`, and store copies
    /// staged in `packages_dir` (`.stage-<pid>-<seq>`) by an install whose
    /// process is gone.
    ///
    /// Every fetch extracts into a directory of its own and removes it once
    /// the store holds the package, so staging only holds a fetch in flight
    /// or a leftover. An entry younger than `min_age` may be a fetch another
    /// process has in flight and is skipped. Nothing in staging is ever
    /// reused, so anything removed is simply downloaded again.
    pub fn sweep_fetch_staging(&self, min_age: std::time::Duration) -> (usize, u64) {
        let mut removed = 0;
        let mut freed = 0;
        for dir in self.fetch_staging_dirs() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                let Ok(meta) = std::fs::symlink_metadata(&path) else {
                    continue;
                };
                let old_enough = meta
                    .modified()
                    .ok()
                    .and_then(|m| m.elapsed().ok())
                    .is_some_and(|age| age >= min_age);
                if !old_enough {
                    continue;
                }
                let size = crate::storage::dir_size(&path);
                let result = if meta.is_dir() {
                    std::fs::remove_dir_all(&path)
                } else {
                    std::fs::remove_file(&path)
                };
                match result {
                    Ok(()) => {
                        removed += 1;
                        freed += size;
                    }
                    Err(e) => warn!(
                        "Could not remove fetch staging entry {}: {e}",
                        path.display()
                    ),
                }
            }
        }
        // Store copies staged by an install whose process is gone (crashed
        // or killed between copy and commit).
        if let Ok(entries) = std::fs::read_dir(&self.config.packages_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                let Some(pid) = dev_users::stage_owner(&name) else {
                    continue;
                };
                if !matches!(
                    crate::process_users::process_state(pid),
                    crate::process_users::ProcessState::Gone
                ) {
                    continue;
                }
                let path = entry.path();
                let size = crate::storage::dir_size(&path);
                match std::fs::remove_dir_all(&path) {
                    Ok(()) => {
                        removed += 1;
                        freed += size;
                    }
                    Err(e) => warn!("Could not remove store staging {}: {e}", path.display()),
                }
            }
        }
        if removed > 0 {
            info!("Removed {removed} leftover download(s) from fetch staging ({freed} bytes)");
        }
        (removed, freed)
    }

    /// Forget everything downloaded for `name@version`: its cached archive,
    /// any partial download, and any extracted copy still in fetch staging
    /// (see [`layout::is_download_of`]). Returns how many entries were
    /// removed.
    ///
    /// For an embedder that has found an installed copy damaged and is about
    /// to reinstall it. A cached archive is reused by the next install and is
    /// only checked when the registry supplied a checksum, so it could be the
    /// source of the damage; evicting it makes the reinstall download and
    /// verify the archive again.
    ///
    /// `name` is the dependency name the fetch was keyed by (`creator/slug`
    /// for a registry package). The store's own copy is not touched; moving
    /// it aside is the caller's decision, and an install never replaces a
    /// committed copy.
    pub fn evict_download(&self, name: &str, version: &str) -> usize {
        let key = layout::download_key(name, version);
        let layout = self.layout();
        let mut dirs = self.fetch_staging_dirs();
        dirs.push(layout.cache_dir().to_path_buf());
        dirs.extend(layout.legacy_cache_dir());
        let mut entries = Vec::new();
        for dir in dirs {
            let Ok(listing) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in listing.flatten() {
                if layout::is_download_of(&entry.file_name().to_string_lossy(), &key) {
                    entries.push(entry.path());
                }
            }
        }

        let mut removed = 0;
        for path in entries {
            let Ok(meta) = std::fs::symlink_metadata(&path) else {
                continue;
            };
            let result = if meta.is_dir() {
                std::fs::remove_dir_all(&path)
            } else {
                std::fs::remove_file(&path)
            };
            match result {
                Ok(()) => {
                    debug!("Evicted download {}", path.display());
                    removed += 1;
                }
                Err(e) => warn!("Could not evict download {}: {e}", path.display()),
            }
        }
        removed
    }

    pub fn list_installed(&self) -> Result<Vec<InstalledPackage>, StorageError> {
        let mut packages = Vec::new();

        if !self.config.packages_dir.exists() {
            return Ok(packages);
        }

        self.collect_installed_packages(&self.config.packages_dir, &mut packages)?;

        debug!("Found {} installed packages", packages.len());
        Ok(packages)
    }

    /// The installed package rooted at `dir`, if `dir` is one inside this
    /// store: a registry install, or a path dependency's dev install under
    /// `_dev/` (which [`list_installed`](Self::list_installed) hides).
    /// `None` for a directory outside the store or one without a readable
    /// `hpm.toml`.
    ///
    /// For callers that already know *which* install a project uses (the
    /// `hpath` of its emitted Houdini manifest) and need its manifest; it
    /// never chooses between versions.
    pub fn installed_at(&self, dir: &std::path::Path) -> Option<InstalledPackage> {
        if !dir.starts_with(&self.config.packages_dir) {
            return None;
        }
        let is_dev = dir.starts_with(self.layout().dev_root());
        match self.parse_installed_package(dir.to_path_buf()) {
            Ok(Some(pkg)) => Some(InstalledPackage { is_dev, ..pkg }),
            _ => None,
        }
    }

    /// Collect the store's packages: the `<slug>@<version>` directories
    /// directly in `dir` (see [`StoreLayout`]).
    ///
    /// The `_dev` subtree is reserved for path-installed packages
    /// (`install_as_dev_copy`) and is intentionally invisible to
    /// `list_installed`. Otherwise an `ensure_installed` cache lookup that
    /// matches on `(name, version)` could return dev content for a registry
    /// resolution at the same coordinate — see the CAS-poisoning bug.
    fn collect_installed_packages(
        &self,
        dir: &std::path::Path,
        packages: &mut Vec<InstalledPackage>,
    ) -> Result<(), StorageError> {
        let entries = std::fs::read_dir(dir).map_err(|e| IoOp::wrap("read directory", dir, e))?;

        for entry in entries {
            let path = entry
                .map_err(|e| IoOp::wrap("read directory", dir, e))?
                .path();
            let Some(dir_name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            // `_dev` holds path installs; hidden entries are staging
            // directories and tombstones; names without `@` are not store
            // copies. None of them is a package.
            if dir_name == DEV_INSTALL_DIR || dir_name.starts_with('.') || !dir_name.contains('@') {
                continue;
            }
            if !path.is_dir() {
                continue;
            }
            if let Some(installed_package) = self.parse_installed_package(path)? {
                packages.push(installed_package);
            }
        }

        Ok(())
    }

    /// Another install committed `name@version` while this one was copying:
    /// drop this one's stage and report the committed copy.
    fn keep_concurrent_install(
        &self,
        staged: &std::path::Path,
        target_dir: &std::path::Path,
        name: &str,
        version: &str,
    ) -> Result<InstalledPackage, StorageError> {
        remove_stage(staged);
        info!("{name}@{version} was installed concurrently; keeping that copy");
        self.parse_installed_package(target_dir.to_path_buf())?
            .ok_or_else(|| StorageError::PackageNotFound(format!("{name}@{version}")))
    }

    fn parse_installed_package(
        &self,
        package_dir: PathBuf,
    ) -> Result<Option<InstalledPackage>, StorageError> {
        if !package_dir.is_dir() {
            return Ok(None);
        }

        let manifest_path = package_dir.join("hpm.toml");
        let manifest = match PackageManifest::from_path(&manifest_path) {
            Ok(m) => m,
            // Directory without a manifest is not a package — skip silently
            // to keep `list_installed` resilient to stray scaffolding.
            Err(ManifestLoadError::NotFound { .. }) => return Ok(None),
            // A malformed manifest (bad TOML, unknown platform, etc.) is one
            // broken entry — it must not abort the whole CAS walk. Aborting
            // here wedges every consumer that lists installed packages
            // (reconcile, env-var discovery, project sync/launch) over a
            // single corrupt cached package, even for projects that don't
            // depend on it. Warn and skip so the rest of the store stays
            // usable; the broken package simply won't resolve from CAS.
            Err(e) => {
                warn!(
                    "Skipping unparseable package manifest at {}: {}",
                    manifest_path.display(),
                    e
                );
                return Ok(None);
            }
        };

        Ok(Some(InstalledPackage {
            version: manifest.package.version.clone(),
            manifest,
            install_path: package_dir,
            // collect_installed_packages skips the `_dev/` subtree, so any
            // package reached through this path came from the CAS.
            is_dev: false,
        }))
    }

    /// Install a package from a local directory path into the registry CAS.
    /// The directory must contain a valid hpm.toml manifest.
    ///
    /// Used for content that arrived through the registry/URL fetch pipeline.
    /// For user-authored path dependencies, use [`install_as_dev_copy`]
    /// instead — that keeps dev content out of the registry CAS so a dev
    /// install of `foo@1.0.0` doesn't get served to a different project that
    /// resolves the same coordinate from a registry.
    ///
    /// [`install_as_dev_copy`]: Self::install_as_dev_copy
    pub async fn install_into_cas(
        &self,
        source_path: &std::path::Path,
    ) -> Result<InstalledPackage, StorageError> {
        self.install_inner(source_path, InstallStyle::CasCopy).await
    }

    /// Install a path-dependency into the dev subtree
    /// (`<packages_dir>/_dev/<slug>@<version>/`).
    ///
    /// Dev installs live in their own namespace because they share `(slug,
    /// version)` keys with registry packages but carry user-authored content
    /// that must not be cached as the canonical install for that coordinate.
    ///
    /// The copy holds the package's install image, not the whole workspace:
    /// the files `hpm pack` / `hpm build` would stage for the host platform
    /// (`.gitignore` + `.hpmignore`, `[stage]` include/exclude and place
    /// rules), plus `hpm.toml`. It lands in a content-addressed
    /// `<slug>@<version>/<hash>/` directory named after those files.
    pub async fn install_as_dev_copy(
        &self,
        source_path: &std::path::Path,
    ) -> Result<InstalledPackage, StorageError> {
        self.install_inner(source_path, InstallStyle::DevCopy).await
    }

    /// Install a path-dependency into the dev subtree as a symlink (Unix) or
    /// junction (Windows). Working-tree edits at `source_path` become visible
    /// to a live Houdini session immediately, with no re-sync.
    ///
    /// Same namespace isolation as [`Self::install_as_dev_copy`]: the link entry
    /// lives at `<packages_dir>/_dev/<slug>@<version>/`, never in the registry
    /// CAS. Registry resolutions at the same coordinate are unaffected.
    ///
    /// Native-binary packages are the exception: if the manifest declares
    /// concrete `[compat].platforms` (see
    /// [`CompatConfig::declares_native_platforms`]), this falls back to a copy
    /// so an in-place DSO rebuild isn't blocked by a running Houdini session
    /// holding the mapped binary. The returned [`InstalledPackage`] is then a
    /// copy of the install image, exactly as [`Self::install_as_dev_copy`]
    /// makes, not a link.
    ///
    /// [`CompatConfig::declares_native_platforms`]: hpm_package::CompatConfig::declares_native_platforms
    pub async fn install_as_dev_link(
        &self,
        source_path: &std::path::Path,
    ) -> Result<InstalledPackage, StorageError> {
        self.install_inner(source_path, InstallStyle::DevLink).await
    }

    async fn install_inner(
        &self,
        source_path: &std::path::Path,
        style: InstallStyle,
    ) -> Result<InstalledPackage, StorageError> {
        // Read and parse the manifest before choosing the final install
        // style: its `[compat].platforms` can force a DevLink down to a
        // DevCopy (see below), which also changes the log kind.
        let manifest_path = source_path.join("hpm.toml");
        let manifest = PackageManifest::from_path(&manifest_path)?;

        let name = manifest.package.slug().to_string();
        let name = &name;
        let version = &manifest.package.version;

        // Link-mode is unsafe for native-binary packages. A Windows junction
        // (or Unix symlink) makes the workspace build output the very DSO/DLL
        // a running Houdini has memory-mapped, so an in-place rebuild fails
        // with LNK1104 / ERROR_SHARING_VIOLATION. It also buys nothing: a
        // mapped DSO can't be hot-reloaded into a live session — a new binary
        // needs a relaunch. Downgrade to a copy so the workspace file and the
        // mapped file are distinct physical files; the rebuilt binary is
        // picked up on the next dev launch (which re-copies and re-runs
        // prepack). Pure-data / pure-Python link installs are untouched.
        let style = if matches!(style, InstallStyle::DevLink)
            && manifest.compat.declares_native_platforms()
        {
            warn!(
                "{}@{} declares native platforms in [compat].platforms; \
                 installing as a dev copy instead of a link so an in-place \
                 native rebuild isn't blocked by a running Houdini session",
                name, version
            );
            InstallStyle::DevCopy
        } else {
            style
        };

        let kind = style.log_kind();
        info!(
            "Installing {kind}{}@{} from {}",
            name,
            version,
            source_path.display()
        );

        // Dev styles share the `_dev/<slug>@<version>` container. A copy lands
        // at a content-addressed `<container>/<hash>` subdirectory; a link's
        // entry *is* the container path.
        let dev_container = self.layout().dev_container(name, version);

        match style {
            InstallStyle::CasCopy => {
                let target_dir = self.get_package_path(name, version);
                // Record what is being installed from the extracted archive
                // itself, before the copy, so a copy cut short shows up as
                // damage rather than as a smaller healthy package.
                let record = {
                    let source = source_path.to_path_buf();
                    tokio::task::spawn_blocking(move || {
                        crate::install_manifest::record_tree(&source)
                    })
                    .await
                    .map_err(|e| {
                        IoOp::wrap("record install of", source_path, std::io::Error::other(e))
                    })??
                };
                // Copy into a private staging directory and rename it into
                // place, so the store never shows a half-copied tree under the
                // package's name, and two processes installing the same
                // version never write into one directory.
                let staged = stage_dir(&self.config.packages_dir);
                if let Err(e) = self.copy_directory(source_path, &staged) {
                    remove_stage(&staged);
                    return Err(e);
                }
                if is_committed_copy(&target_dir) {
                    // Another install finished this version first (copies
                    // only ever land whole, by rename). It may already be
                    // loaded by a running Houdini: keep it.
                    return self.keep_concurrent_install(&staged, &target_dir, name, version);
                }
                // A tree without `hpm.toml` (an older hpm's in-place copy cut
                // short) or a stale link is replaced; never `remove_dir_all`
                // a junction.
                if let Err(e) = clear_existing_install(&target_dir, name, version) {
                    remove_stage(&staged);
                    return Err(e);
                }
                crate::install_manifest::forget(&target_dir);
                if let Err(e) = std::fs::rename(&staged, &target_dir) {
                    // A rename only ever lands a whole tree, so a target that
                    // appeared since the clear is another install's finished
                    // copy of this version.
                    if is_committed_copy(&target_dir) {
                        return self.keep_concurrent_install(&staged, &target_dir, name, version);
                    }
                    remove_stage(&staged);
                    return Err(IoOp::wrap("commit store copy to", &target_dir, e).into());
                }
                // Written last: its presence says the copy completed. A
                // failure costs the ability to check this install, not the
                // install, so it warns.
                if let Err(e) = crate::install_manifest::write(&target_dir, &record) {
                    warn!(
                        "Installed {}@{} but could not write its install record: {}",
                        name, version, e
                    );
                }
                info!("Successfully installed {kind}{}@{}", name, version);
                Ok(InstalledPackage {
                    version: version.clone(),
                    manifest,
                    install_path: target_dir,
                    is_dev: false,
                })
            }

            InstallStyle::DevCopy => {
                // Content-addressed install. The copy lands at
                // `<container>/<hash>`, where `hash` fingerprints the staged
                // file set, and the regenerated Houdini manifest points `hpath`
                // there. A rebuild yields a *new* hash directory: a
                // concurrently-running Houdini keeps mapping the directory it was
                // launched from, so nothing is ever removed out from under a live
                // process. This is what eliminates the Windows `os error 5`
                // (`PackageInUse`) failure on the rebuild-then-relaunch loop,
                // which the old in-place clear-and-recopy could not avoid once a
                // second session had the copy mapped.
                //
                // Stage the same file set `hpm pack` / `hpm build` would ship
                // (ignore files + `[stage]`), not the raw workspace. Walking and
                // copying are blocking filesystem work, so keep them off the
                // async worker threads.
                let image = {
                    let source = source_path.to_path_buf();
                    let manifest = manifest.clone();
                    tokio::task::spawn_blocking(move || dev_copy_image(&source, &manifest))
                        .await
                        .map_err(|e| {
                            IoOp::wrap(
                                "fingerprint dev source",
                                source_path,
                                std::io::Error::other(e),
                            )
                        })??
                };
                let hash = image.hash.clone();
                let target_dir = dev_copy_target(&dev_container, &hash);

                // Clear only a stale *link* left by a prior DevLink install of
                // this coordinate; a directory of live hashes is left in place.
                clear_container_link(&dev_container)?;

                // Claim the copy before it exists, so a reclaim running
                // anywhere (another sync, another process) never sees it
                // without a user. The project's sync settles this once its
                // manifest points here; see `dev_users`. After the link check:
                // the records live in the container, which until then may be
                // a link into the user's workspace.
                dev_users::hold_until_settled(&target_dir);

                if dev_copy_is_complete(&target_dir) {
                    // Identical source already installed (an unchanged relaunch,
                    // or another session materialized this exact build): reuse it
                    // untouched — the analogue of the registry "already
                    // installed" short-circuit, now lock-free by construction.
                    info!(
                        "dev copy {}@{} ({}) already present; skipping recopy",
                        name, version, hash
                    );
                } else {
                    if let Err(e) = std::fs::create_dir_all(&dev_container) {
                        dev_users::settle(&target_dir);
                        return Err(IoOp::wrap("create dev container", &dev_container, e).into());
                    }
                    // Copy into a hidden staging dir, then commit with an atomic
                    // rename so a crash mid-copy never leaves a half-populated
                    // hash directory that a later launch would trust as complete.
                    let staged = stage_dir(&dev_container);
                    let file_count = image.entries.len();
                    let copied = {
                        let dest = staged.clone();
                        tokio::task::spawn_blocking(move || copy_dev_image(&image.entries, &dest))
                            .await
                            .map_err(|e| {
                                StorageError::from(IoOp::wrap(
                                    "copy dev image to",
                                    &staged,
                                    std::io::Error::other(e),
                                ))
                            })
                            .and_then(|r| r)
                    };
                    if let Err(e) = copied {
                        let _ = std::fs::remove_dir_all(&staged);
                        dev_users::settle(&target_dir);
                        return Err(e);
                    }
                    if let Err(e) = commit_staged_copy(&staged, &target_dir) {
                        dev_users::settle(&target_dir);
                        return Err(e);
                    }
                    info!(
                        "Successfully installed {kind}{}@{} ({} files)",
                        name, version, file_count
                    );
                }

                Ok(InstalledPackage {
                    version: version.clone(),
                    manifest,
                    install_path: target_dir,
                    is_dev: true,
                })
            }

            InstallStyle::DevLink => {
                let target_dir = dev_container;
                // Symlink-safe replacement: if the existing entry is a link (the
                // common case during repeat sync of a link-installed dep), we
                // must never `remove_dir_all` it — that would follow a Windows
                // junction into the user's workspace and recursively delete it. A
                // prior content-addressed copy directory here is replaced wholesale.
                clear_existing_install(&target_dir, name, version)?;

                // Junctions need absolute paths; symlinks behave more
                // predictably when absolute too. Canonicalize so the link
                // survives changes to the project's working directory.
                let absolute_source = std::fs::canonicalize(source_path)
                    .map_err(|e| IoOp::wrap("canonicalize link source", source_path, e))?;
                if let Some(parent) = target_dir.parent() {
                    std::fs::create_dir_all(parent)
                        .map_err(|e| IoOp::wrap("create dev parent directory", parent, e))?;
                }
                create_dev_link(&absolute_source, &target_dir)
                    .map_err(|e| IoOp::wrap("create dev link at", &target_dir, e))?;

                info!("Successfully installed {kind}{}@{}", name, version);
                Ok(InstalledPackage {
                    version: version.clone(),
                    manifest,
                    install_path: target_dir,
                    is_dev: true,
                })
            }
        }
    }

    /// Copy a directory recursively, unfiltered. Only for content that is
    /// already an install image (an extracted archive going into the CAS);
    /// dev copies go through `dev_copy_image` instead.
    fn copy_directory(
        &self,
        source: &std::path::Path,
        target: &std::path::Path,
    ) -> Result<(), StorageError> {
        std::fs::create_dir_all(target)
            .map_err(|e| IoOp::wrap("create install target", target, e))?;

        for entry in walkdir::WalkDir::new(source).min_depth(1) {
            let entry = entry.map_err(|e| {
                let path = e
                    .path()
                    .map(std::path::Path::to_path_buf)
                    .unwrap_or_else(|| source.to_path_buf());
                IoOp::wrap(
                    "walk source directory at",
                    &path,
                    e.into_io_error()
                        .unwrap_or_else(|| std::io::Error::other("walk error")),
                )
            })?;
            let relative_path = entry.path().strip_prefix(source).map_err(|_| {
                IoOp::wrap(
                    "strip workspace prefix from",
                    entry.path(),
                    std::io::Error::other("path outside workspace"),
                )
            })?;
            let target_path = target.join(relative_path);

            if entry.file_type().is_dir() {
                std::fs::create_dir_all(&target_path)
                    .map_err(|e| IoOp::wrap("create subdirectory", &target_path, e))?;
            } else {
                // Ensure parent directory exists
                if let Some(parent) = target_path.parent() {
                    std::fs::create_dir_all(parent)
                        .map_err(|e| IoOp::wrap("create file parent", parent, e))?;
                }
                std::fs::copy(entry.path(), &target_path)
                    .map_err(|e| IoOp::wrap("copy file to", &target_path, e))?;
            }
        }

        Ok(())
    }

    /// Compare an installed package with the record its install wrote
    /// (blocking; it reads every recorded file). See
    /// [`crate::install_manifest`].
    ///
    /// A dev install is always [`Verification::Unrecorded`]: it is a copy of
    /// the user's own working tree, and there is nothing to compare it with
    /// but itself.
    ///
    /// [`Verification::Unrecorded`]: crate::install_manifest::Verification::Unrecorded
    pub fn verify_installed(
        &self,
        package: &InstalledPackage,
    ) -> Result<crate::install_manifest::Verification, StorageError> {
        if package.is_dev {
            return Ok(crate::install_manifest::Verification::Unrecorded);
        }
        Ok(crate::install_manifest::verify(&package.install_path)?)
    }

    pub async fn remove_package(&self, name: &str, version: &str) -> Result<(), StorageError> {
        let package_dir = self.get_package_path(name, version);

        // `symlink_metadata` rather than `exists()` because `exists()` follows
        // links — a junction pointing at a missing target would falsely report
        // not-found, and we wouldn't reach the link-safe removal path below.
        let meta = match std::fs::symlink_metadata(&package_dir) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(StorageError::PackageNotFound(format!(
                    "{}@{}",
                    name, version
                )));
            }
            Err(e) => return Err(IoOp::wrap("stat package directory", &package_dir, e).into()),
        };

        info!("Removing package: {}@{}", name, version);
        remove_install_entry(&package_dir, &meta, name, version)
    }

    /// Enumerate dev (path-dep) installs in `<packages_dir>/_dev/`.
    ///
    /// Walks the dev subtree at one level deep, parsing the `<slug>@<version>`
    /// directory naming we control on the install side. Reads the directory
    /// name rather than the entry's `hpm.toml` so a link install pointing at a
    /// deleted workspace still surfaces here — that's exactly the case dev
    /// cleanup needs to reach.
    pub fn list_dev_installs(&self) -> Result<Vec<DevInstall>, StorageError> {
        let dev_root = self.layout().dev_root();
        if !dev_root.exists() {
            return Ok(Vec::new());
        }

        let mut out = Vec::new();
        let entries = std::fs::read_dir(&dev_root)
            .map_err(|e| IoOp::wrap("read dev install root", &dev_root, e))?;
        for entry in entries.flatten() {
            let path = entry.path();
            // Don't follow links — we want to know they exist, not what they
            // point at. `symlink_metadata` keeps a link install visible even
            // if its target has been deleted.
            let meta = match std::fs::symlink_metadata(&path) {
                Ok(m) => m,
                Err(_) => continue,
            };
            if !meta.is_dir() && !meta.file_type().is_symlink() {
                continue;
            }

            let name = match path.file_name().and_then(|n| n.to_str()) {
                Some(n) => n,
                None => continue,
            };
            let Some((slug, version)) = layout::parse_coordinate(name) else {
                continue;
            };

            out.push(DevInstall {
                slug: slug.to_string(),
                version: version.to_string(),
                install_path: path,
            });
        }

        debug!("Found {} dev installs in {}", out.len(), dev_root.display());
        Ok(out)
    }
}

/// Bytes under `path`, links counted as themselves and never followed.
pub(crate) fn dir_size(path: &std::path::Path) -> u64 {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return 0;
    };
    if !meta.is_dir() {
        return meta.len();
    }
    std::fs::read_dir(path)
        .map(|entries| entries.flatten().map(|e| dir_size(&e.path())).sum())
        .unwrap_or(0)
}

#[cfg(test)]
#[path = "storage_tests.rs"]
mod tests;
