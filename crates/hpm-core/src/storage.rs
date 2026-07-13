//! Global package store: CAS-backed install, removal, and orphan cleanup.
//!
//! [`StorageManager`] owns `~/.hpm/packages/`, `_dev/` for path installs,
//! and the cleanup pipeline. The supporting types live in submodules:
//!
//! - [`error`] — [`StorageError`]
//! - [`types`] — [`InstalledPackage`], [`PackageSpec`], [`VersionReq`]
//! - [`dev_install`] — `_dev/` path-install primitives (link, copy, remove)
//! - [`cleanup`] — the project-aware GC pipeline and
//!   [`ComprehensiveCleanupResult`] aggregate

use hpm_config::StorageConfig;
use hpm_package::{IoOp, ManifestLoadError, PackageManifest};
use std::path::PathBuf;
use tracing::{debug, info, warn};

pub mod cleanup;
pub mod dev_install;
pub mod error;
pub mod types;

pub use cleanup::ComprehensiveCleanupResult;
pub use dev_install::DevInstall;
pub use error::StorageError;
pub use types::{InstalledPackage, PackageSpec, VersionReq};

use dev_install::{
    DEV_INSTALL_DIR, InstallStyle, clear_container_link, clear_existing_install,
    commit_staged_copy, create_dev_link, dev_copy_is_complete, dev_copy_target,
    remove_install_entry, source_hash, stage_dir,
};
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

    pub fn package_exists(&self, name: &str, version: &str) -> bool {
        let package_dir = self.config.package_dir(name, version);
        package_dir.exists() && package_dir.join("hpm.toml").exists()
    }

    pub fn get_package_path(&self, name: &str, version: &str) -> PathBuf {
        self.config.package_dir(name, version)
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

    /// Recursively collect installed packages from a directory.
    ///
    /// With scoped package paths (e.g. `creator/slug`), packages live at
    /// `~/.hpm/packages/creator/slug@version/`. Directories without `@` in
    /// their name are treated as scope directories and are walked one level
    /// deeper. Directories with `@` are treated as package directories.
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

        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            let dir_name = match path.file_name().and_then(|n| n.to_str()) {
                Some(name) => name.to_string(),
                None => continue,
            };

            if dir_name == DEV_INSTALL_DIR {
                continue;
            }

            if dir_name.contains('@') {
                // This is a package directory (e.g. `slug@1.0.0` or `fire-fx@2.0.0`)
                if let Some(installed_package) = self.parse_installed_package(path)? {
                    packages.push(installed_package);
                }
            } else {
                // This is a scope directory (e.g. `creator`), walk into it
                self.collect_installed_packages(&entry.path(), packages)?;
            }
        }

        Ok(())
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
    /// holding the mapped binary. The returned [`InstalledPackage`] is a copy
    /// in that case, not a link.
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
        let dev_container = self
            .config
            .packages_dir
            .join(DEV_INSTALL_DIR)
            .join(format!("{}@{}", name, version));

        match style {
            InstallStyle::CasCopy => {
                let target_dir = self.config.package_dir(name, version);
                // Symlink-safe replacement: never `remove_dir_all` a junction.
                clear_existing_install(&target_dir, name, version)?;
                self.copy_directory(source_path, &target_dir)?;
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
                // `<container>/<hash>`, where `hash` fingerprints the source
                // workspace, and the regenerated Houdini manifest points `hpath`
                // there. A rebuild yields a *new* hash directory: a
                // concurrently-running Houdini keeps mapping the directory it was
                // launched from, so nothing is ever removed out from under a live
                // process. This is what eliminates the Windows `os error 5`
                // (`PackageInUse`) failure on the rebuild-then-relaunch loop,
                // which the old in-place clear-and-recopy could not avoid once a
                // second session had the copy mapped.
                let hash = source_hash(source_path)
                    .map_err(|e| IoOp::wrap("fingerprint dev source", source_path, e))?;
                let target_dir = dev_copy_target(&dev_container, &hash);

                // Clear only a stale *link* left by a prior DevLink install of
                // this coordinate; a directory of live hashes is left in place.
                clear_container_link(&dev_container)?;

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
                    std::fs::create_dir_all(&dev_container)
                        .map_err(|e| IoOp::wrap("create dev container", &dev_container, e))?;
                    // Copy into a hidden staging dir, then commit with an atomic
                    // rename so a crash mid-copy never leaves a half-populated
                    // hash directory that a later launch would trust as complete.
                    let staged = stage_dir(&dev_container);
                    if let Err(e) = self.copy_directory(source_path, &staged) {
                        let _ = std::fs::remove_dir_all(&staged);
                        return Err(e);
                    }
                    commit_staged_copy(&staged, &target_dir)?;
                    info!("Successfully installed {kind}{}@{}", name, version);
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

    /// Copy a directory recursively
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

    pub async fn remove_package(&self, name: &str, version: &str) -> Result<(), StorageError> {
        let package_dir = self.config.package_dir(name, version);

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
        let dev_root = self.config.packages_dir.join(DEV_INSTALL_DIR);
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
            // `<slug>@<version>` — split on the *last* `@` for defensiveness
            // even though install never produces multiple separators.
            let Some((slug, version)) = name.rsplit_once('@') else {
                continue;
            };
            if slug.is_empty() || version.is_empty() {
                continue;
            }

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

#[cfg(test)]
#[path = "storage_tests.rs"]
mod tests;
