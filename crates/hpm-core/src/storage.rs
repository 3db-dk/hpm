//! Global package store: CAS-backed install, removal, and orphan cleanup.
//!
//! [`StorageManager`] owns `~/.hpm/packages/`, `_dev/` for path installs,
//! and the cleanup pipeline. The supporting types live in submodules:
//!
//! - [`error`] — [`StorageError`]
//! - [`types`] — [`InstalledPackage`], [`PackageSpec`], [`VersionReq`]
//! - [`dev_install`] — `_dev/` path-install primitives (link, copy, remove)
//! - [`cleanup`] — [`ComprehensiveCleanupResult`] aggregate

use crate::discovery::ProjectDiscovery;
use crate::graph::{DependencyResolver, PackageId};
use crate::python::cleanup::{CleanupResult, PythonCleanupAnalyzer};
use hpm_config::{ProjectsConfig, StorageConfig};
use hpm_package::{IoOp, ManifestLoadError, PackageManifest};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
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
    DEV_INSTALL_DIR, InstallStyle, clear_existing_install, create_dev_link, remove_install_entry,
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
            Err(e) => return Err(StorageError::Manifest(e)),
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
        let kind = style.log_kind();
        info!(
            "Installing {kind}package from path: {}",
            source_path.display()
        );

        // Read and parse the manifest
        let manifest_path = source_path.join("hpm.toml");
        let manifest = PackageManifest::from_path(&manifest_path)?;

        let name = manifest.package.slug().to_string();
        let name = &name;
        let version = &manifest.package.version;

        info!(
            "Installing {kind}{}@{} from {}",
            name,
            version,
            source_path.display()
        );

        let target_dir = match style {
            InstallStyle::CasCopy => self.config.package_dir(name, version),
            InstallStyle::DevCopy | InstallStyle::DevLink => self
                .config
                .packages_dir
                .join(DEV_INSTALL_DIR)
                .join(format!("{}@{}", name, version)),
        };

        // Symlink-safe replacement: if the existing entry is a link (the
        // common case during repeat sync of a link-installed dep), we must
        // never `remove_dir_all` it — that would follow a Windows junction
        // into the user's workspace and recursively delete it.
        clear_existing_install(&target_dir, name, version)?;

        match style {
            InstallStyle::CasCopy | InstallStyle::DevCopy => {
                self.copy_directory(source_path, &target_dir)?;
            }
            InstallStyle::DevLink => {
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
            }
        }

        info!("Successfully installed {kind}{}@{}", name, version);

        let is_dev = matches!(style, InstallStyle::DevCopy | InstallStyle::DevLink);

        Ok(InstalledPackage {
            version: version.clone(),
            manifest,
            install_path: target_dir,
            is_dev,
        })
    }

    /// Copy a directory recursively
    fn copy_directory(
        &self,
        source: &std::path::Path,
        target: &std::path::Path,
    ) -> Result<(), StorageError> {
        std::fs::create_dir_all(target)
            .map_err(|e| IoOp::wrap("create install target", target, e))?;

        for entry in walkdir::WalkDir::new(source)
            .min_depth(1)
            .into_iter()
            .filter_map(|e| e.ok())
        {
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

    /// Find orphaned packages that are not needed by any active project.
    ///
    /// Returns the list of orphaned package IDs along with all installed package identifiers.
    async fn find_orphaned_packages(
        &self,
        projects_config: &ProjectsConfig,
    ) -> Result<Vec<PackageId>, StorageError> {
        // 1. Get all installed packages
        let all_installed = self.list_installed()?;

        if all_installed.is_empty() {
            info!("No packages installed - cleanup not needed");
            return Ok(vec![]);
        }

        info!(
            "Found {} installed packages to analyze",
            all_installed.len()
        );

        // 2. Discover projects using project configuration
        let project_discovery = ProjectDiscovery::new(projects_config.clone());
        let projects = project_discovery.find_projects()?;

        if projects.is_empty() {
            warn!(
                "No HPM-managed projects found - skipping cleanup to prevent removing all packages"
            );
            return Ok(vec![]);
        }

        info!(
            "Found {} HPM-managed projects for cleanup analysis",
            projects.len()
        );

        // 3. Build dependency graph from discovered projects
        let resolver = DependencyResolver::new(Arc::new(self.clone()));
        let dependency_graph = resolver.build_dependency_graph(&projects).await?;

        // 4. Collect root packages (directly required by projects)
        let root_packages: Vec<PackageId> = dependency_graph
            .nodes()
            .filter(|node| node.is_root)
            .map(|node| node.id.clone())
            .collect();

        info!(
            "Found {} root packages required by active projects",
            root_packages.len()
        );

        // 5. Mark all packages reachable from roots
        let needed_packages = dependency_graph.mark_reachable_from_roots(&root_packages);
        info!(
            "Marked {} packages as needed (including transitive dependencies)",
            needed_packages.len()
        );

        // 6. Find orphaned packages by comparing all installed packages to needed packages
        let all_package_ids: HashSet<PackageId> =
            all_installed.iter().map(PackageId::from).collect();

        let orphaned_packages: Vec<PackageId> = all_package_ids
            .difference(&needed_packages)
            .cloned()
            .collect();

        Ok(orphaned_packages)
    }

    /// Remove orphaned packages. Returns identifiers of the packages actually removed.
    pub async fn cleanup_unused(
        &self,
        projects_config: &ProjectsConfig,
    ) -> Result<Vec<String>, StorageError> {
        info!("Starting project-aware package cleanup");

        let orphaned_packages = self.find_orphaned_packages(projects_config).await?;

        if orphaned_packages.is_empty() {
            info!("No orphaned packages found - cleanup not needed");
            return Ok(vec![]);
        }

        info!(
            "Found {} orphaned packages to remove",
            orphaned_packages.len()
        );

        let mut removed_packages = Vec::new();
        for package_id in orphaned_packages {
            match self
                .remove_package(&package_id.name, &package_id.version)
                .await
            {
                Ok(()) => {
                    removed_packages.push(package_id.identifier());
                    info!("Removed orphaned package: {}", package_id.identifier());
                }
                Err(e) => {
                    warn!(
                        "Failed to remove package {}: {}",
                        package_id.identifier(),
                        e
                    );
                }
            }
        }

        info!(
            "Cleanup completed: removed {} orphaned packages",
            removed_packages.len()
        );
        Ok(removed_packages)
    }

    /// Plan — but don't execute — an orphan cleanup.
    ///
    /// Returns the list of package identifiers that `cleanup_unused` *would*
    /// remove if called. Safe to call repeatedly.
    pub async fn cleanup_unused_dry_run(
        &self,
        projects_config: &ProjectsConfig,
    ) -> Result<Vec<String>, StorageError> {
        let orphaned = self.find_orphaned_packages(projects_config).await?;
        let ids: Vec<String> = orphaned.iter().map(|id| id.identifier()).collect();
        info!("Dry run: would remove {} orphaned packages", ids.len());
        for id in &ids {
            info!("Would remove: {id}");
        }
        Ok(ids)
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

    /// Find dev installs that no known project's path-dependency claims.
    ///
    /// Walks every discovered project, parses its `hpm.toml`, and for each
    /// `DependencySpec::Path` resolves the source manifest to extract
    /// `(slug, version)`. The union of those tuples is the "needed" set; dev
    /// installs outside it are orphans.
    ///
    /// Source reads that fail (missing path, malformed manifest) log a
    /// warning and skip the dep. A broken project doesn't bypass cleanup —
    /// re-running `hpm sync` re-creates whatever it needs.
    async fn find_orphaned_dev_installs(
        &self,
        projects_config: &ProjectsConfig,
    ) -> Result<Vec<DevInstall>, StorageError> {
        let dev_installs = self.list_dev_installs()?;
        if dev_installs.is_empty() {
            return Ok(Vec::new());
        }

        let project_discovery = ProjectDiscovery::new(projects_config.clone());
        let projects = project_discovery.find_projects()?;

        if projects.is_empty() {
            warn!(
                "No HPM-managed projects found - skipping dev cleanup to prevent removing dev installs"
            );
            return Ok(Vec::new());
        }

        let mut needed: HashSet<(String, String)> = HashSet::new();
        for project in &projects {
            for (dep_name, spec) in &project.manifest.dependencies {
                let hpm_package::DependencySpec::Path { path, .. } = spec else {
                    continue;
                };
                // Resolve relative to the project directory, just like
                // `install_one_dep` does at install time.
                let source = project.path.join(path);
                let manifest_path = source.join("hpm.toml");
                match PackageManifest::from_path(&manifest_path) {
                    Ok(m) => {
                        needed.insert((m.package.slug().to_string(), m.package.version.clone()));
                    }
                    Err(e) => {
                        warn!(
                            "Project {} has path dep {} pointing at {}, but its manifest is unreadable ({}); \
                             dev install from this dep will not be protected from cleanup",
                            project.path.display(),
                            dep_name,
                            source.display(),
                            e
                        );
                    }
                }
            }
        }

        let orphans: Vec<DevInstall> = dev_installs
            .into_iter()
            .filter(|d| !needed.contains(&(d.slug.clone(), d.version.clone())))
            .collect();
        Ok(orphans)
    }

    /// Remove dev installs that no project's path-dependency claims.
    /// Returns identifiers of the entries actually removed.
    pub async fn cleanup_unused_dev_installs(
        &self,
        projects_config: &ProjectsConfig,
    ) -> Result<Vec<String>, StorageError> {
        let orphans = self.find_orphaned_dev_installs(projects_config).await?;
        if orphans.is_empty() {
            info!("No orphaned dev installs found");
            return Ok(Vec::new());
        }

        info!("Found {} orphaned dev installs to remove", orphans.len());
        let mut removed = Vec::new();
        for dev in orphans {
            // symlink_metadata + remove_install_entry is the same defensive
            // removal we use in `clear_existing_install` and `remove_package`:
            // a link install must be unlinked, never followed.
            let meta = match std::fs::symlink_metadata(&dev.install_path) {
                Ok(m) => m,
                Err(e) => {
                    warn!(
                        "Failed to stat dev install {} at {}: {}",
                        dev.identifier(),
                        dev.install_path.display(),
                        e
                    );
                    continue;
                }
            };
            match remove_install_entry(&dev.install_path, &meta, &dev.slug, &dev.version) {
                Ok(()) => {
                    info!("Removed orphaned dev install: {}", dev.identifier());
                    removed.push(dev.identifier());
                }
                Err(e) => {
                    warn!("Failed to remove dev install {}: {}", dev.identifier(), e);
                }
            }
        }

        Ok(removed)
    }

    /// Plan — but don't execute — a dev cleanup. Returns identifiers that
    /// `cleanup_unused_dev_installs` *would* remove if called.
    pub async fn cleanup_unused_dev_installs_dry_run(
        &self,
        projects_config: &ProjectsConfig,
    ) -> Result<Vec<String>, StorageError> {
        let orphans = self.find_orphaned_dev_installs(projects_config).await?;
        let ids: Vec<String> = orphans.iter().map(DevInstall::identifier).collect();
        info!("Dry run: would remove {} orphaned dev installs", ids.len());
        for id in &ids {
            info!("Would remove: {id}");
        }
        Ok(ids)
    }

    /// Comprehensive cleanup: orphaned packages + dev installs + orphaned
    /// Python virtual environments.
    ///
    /// When `dry_run` is true, nothing is removed — the result lists what
    /// *would* have been removed.
    pub async fn cleanup_comprehensive(
        &self,
        projects_config: &ProjectsConfig,
        dry_run: bool,
    ) -> Result<ComprehensiveCleanupResult, StorageError> {
        info!(
            "Starting comprehensive cleanup{} (packages + dev installs + Python environments)",
            if dry_run { " dry run" } else { "" }
        );

        // 1. Registry CAS package cleanup.
        let removed_packages = if dry_run {
            self.cleanup_unused_dry_run(projects_config).await?
        } else {
            self.cleanup_unused(projects_config).await?
        };

        // 2. Dev (path-dep) install cleanup. The `_dev/` subtree is filtered
        //    out of `list_installed`, so the CAS pass above never sees it;
        //    we need a parallel pass driven by project path-deps directly.
        let removed_dev_installs = if dry_run {
            self.cleanup_unused_dev_installs_dry_run(projects_config)
                .await?
        } else {
            self.cleanup_unused_dev_installs(projects_config).await?
        };

        // 3. Build the set of packages that remain (or would remain) after CAS cleanup.
        let all_installed = self.list_installed()?;
        let remaining_packages: Vec<String> = all_installed
            .into_iter()
            .filter_map(|p| {
                let id = format!("{}@{}", p.manifest.package.slug(), p.version);
                (!removed_packages.contains(&id)).then_some(id)
            })
            .collect();

        // 4. Python virtual environment cleanup against the remaining set.
        let python_analyzer =
            PythonCleanupAnalyzer::new().map_err(|e| StorageError::PythonCleanup(e.to_string()))?;
        let orphaned_venvs = python_analyzer
            .analyze_orphaned_venvs(&remaining_packages)
            .await
            .map_err(|e| StorageError::PythonCleanup(e.to_string()))?;

        let python_cleanup = python_analyzer
            .cleanup_orphaned_venvs(&orphaned_venvs, dry_run)
            .await
            .map_err(|e| StorageError::PythonCleanup(e.to_string()))?;

        let result = ComprehensiveCleanupResult {
            removed_packages,
            removed_dev_installs,
            python_cleanup,
        };

        if dry_run {
            info!(
                "Comprehensive cleanup dry run: {} packages, {} dev installs, {} venvs would be removed",
                result.removed_packages.len(),
                result.removed_dev_installs.len(),
                result.python_cleanup.items_that_would_be_cleaned()
            );
        } else {
            info!(
                "Comprehensive cleanup completed: {} packages, {} dev installs, {} venvs, {} space freed",
                result.removed_packages.len(),
                result.removed_dev_installs.len(),
                result.python_cleanup.items_cleaned(),
                result.python_cleanup.format_space_freed()
            );
        }

        Ok(result)
    }

    /// Clean up only Python virtual environments
    pub async fn cleanup_python_only(&self, dry_run: bool) -> Result<CleanupResult, StorageError> {
        info!("Starting Python-only cleanup (dry_run: {})", dry_run);

        let python_analyzer =
            PythonCleanupAnalyzer::new().map_err(|e| StorageError::PythonCleanup(e.to_string()))?;

        // Get list of all active packages
        let active_packages = self.list_installed()?;
        let active_package_names: Vec<String> = active_packages
            .into_iter()
            .map(|p| format!("{}@{}", p.manifest.package.slug(), p.version))
            .collect();

        // Find orphaned virtual environments
        let orphaned_venvs = python_analyzer
            .analyze_orphaned_venvs(&active_package_names)
            .await
            .map_err(|e| StorageError::PythonCleanup(e.to_string()))?;

        // Clean up (or dry run)
        let result = python_analyzer
            .cleanup_orphaned_venvs(&orphaned_venvs, dry_run)
            .await
            .map_err(|e| StorageError::PythonCleanup(e.to_string()))?;

        if dry_run {
            info!(
                "Python cleanup dry run: {} venvs would be cleaned",
                result.items_that_would_be_cleaned()
            );
        } else {
            info!(
                "Python cleanup completed: {} venvs cleaned, {} space freed",
                result.items_cleaned(),
                result.format_space_freed()
            );
        }

        Ok(result)
    }
}

#[cfg(test)]
#[path = "storage_tests.rs"]
mod tests;
