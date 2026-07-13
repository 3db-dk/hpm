//! Project-aware garbage collection for the global store: orphaned CAS
//! packages, orphaned `_dev/` installs, and orphaned Python venvs — plus the
//! [`ComprehensiveCleanupResult`] aggregate of a full cleanup pass.

use crate::discovery::ProjectDiscovery;
use crate::graph::{DependencyResolver, PackageId};
use crate::python::cleanup::{CleanupResult, PythonCleanupAnalyzer};
use hpm_config::ProjectsConfig;
use hpm_package::PackageManifest;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{info, warn};

use super::dev_install::{prune_stale_dev_hashes, remove_install_entry, source_hash};
use super::{DevInstall, StorageError, StorageManager};

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

impl StorageManager {
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

    /// Resolve which `(slug, version)` dev coordinates the discovered projects
    /// still need, mapped to the source workspace each is installed from.
    ///
    /// Walks every discovered project, parses its `hpm.toml`, and for each
    /// `DependencySpec::Path` resolves the source manifest to extract
    /// `(slug, version)` and the source path. Source reads that fail (missing
    /// path, malformed manifest) log a warning and skip the dep — a broken
    /// project doesn't bypass cleanup, since re-running `hpm sync` re-creates
    /// whatever it needs.
    ///
    /// Returns `None` when no HPM-managed projects are discovered at all: with
    /// nothing to compare against, cleanup is skipped rather than treating every
    /// dev install as an orphan.
    async fn resolve_dev_needs(
        &self,
        projects_config: &ProjectsConfig,
    ) -> Result<Option<HashMap<(String, String), PathBuf>>, StorageError> {
        let project_discovery = ProjectDiscovery::new(projects_config.clone());
        let projects = project_discovery.find_projects()?;

        if projects.is_empty() {
            warn!(
                "No HPM-managed projects found - skipping dev cleanup to prevent removing dev installs"
            );
            return Ok(None);
        }

        let mut needed: HashMap<(String, String), PathBuf> = HashMap::new();
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
                        needed.insert(
                            (m.package.slug().to_string(), m.package.version.clone()),
                            source,
                        );
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

        Ok(Some(needed))
    }

    /// Find dev installs that no known project's path-dependency claims.
    /// The union of needed `(slug, version)` tuples is the "needed" set; dev
    /// installs outside it are orphans.
    async fn find_orphaned_dev_installs(
        &self,
        projects_config: &ProjectsConfig,
    ) -> Result<Vec<DevInstall>, StorageError> {
        let dev_installs = self.list_dev_installs()?;
        if dev_installs.is_empty() {
            return Ok(Vec::new());
        }

        let Some(needed) = self.resolve_dev_needs(projects_config).await? else {
            return Ok(Vec::new());
        };

        let orphans: Vec<DevInstall> = dev_installs
            .into_iter()
            .filter(|d| !needed.contains_key(&(d.slug.clone(), d.version.clone())))
            .collect();
        Ok(orphans)
    }

    /// Remove dev installs that no project's path-dependency claims, then
    /// reclaim superseded content copies of the installs that remain.
    /// Returns identifiers of the entries actually removed.
    ///
    /// Reclamation prunes every `<container>/<hash>` directory except the one
    /// matching the current source, so the accumulated builds from a dev
    /// iteration loop don't grow `_dev/` without bound. It is best-effort and
    /// carries the same "run when Houdini sessions are closed" expectation as
    /// the CAS package cleanup: a copy still mapped by a live process is skipped
    /// (on Windows the OS lock fails the removal) rather than force-removed.
    pub async fn cleanup_unused_dev_installs(
        &self,
        projects_config: &ProjectsConfig,
    ) -> Result<Vec<String>, StorageError> {
        let dev_installs = self.list_dev_installs()?;
        if dev_installs.is_empty() {
            info!("No dev installs found");
            return Ok(Vec::new());
        }

        let Some(needed) = self.resolve_dev_needs(projects_config).await? else {
            return Ok(Vec::new());
        };

        let (orphans, referenced): (Vec<DevInstall>, Vec<DevInstall>) = dev_installs
            .into_iter()
            .partition(|d| !needed.contains_key(&(d.slug.clone(), d.version.clone())));

        let mut removed = Vec::new();
        if orphans.is_empty() {
            info!("No orphaned dev installs found");
        } else {
            info!("Found {} orphaned dev installs to remove", orphans.len());
            for dev in orphans {
                // symlink_metadata + remove_install_entry is the same defensive
                // removal we use in `clear_existing_install` and `remove_package`:
                // a link install must be unlinked, never followed. Removing a
                // whole container reclaims all of its hash copies at once.
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
        }

        // Reclaim superseded content copies of the installs that are still
        // referenced by a project. The current hash is computed from the same
        // source path the install resolves from.
        for dev in &referenced {
            let Some(source) = needed.get(&(dev.slug.clone(), dev.version.clone())) else {
                continue;
            };
            // Only copy containers (real directories) carry hash subdirs; a link
            // install has no superseded copies to reclaim.
            if !dev.install_path.is_dir() {
                continue;
            }
            match source_hash(source) {
                Ok(hash) => {
                    let n = prune_stale_dev_hashes(&dev.install_path, &hash);
                    if n > 0 {
                        info!(
                            "Reclaimed {} superseded dev {} for {}",
                            n,
                            if n == 1 { "copy" } else { "copies" },
                            dev.identifier()
                        );
                    }
                }
                Err(e) => warn!(
                    "Could not fingerprint source for {} at {}; skipping copy reclamation: {}",
                    dev.identifier(),
                    source.display(),
                    e
                ),
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
