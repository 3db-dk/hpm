//! Project-aware garbage collection for the global store: orphaned CAS
//! packages, orphaned `_dev/` installs, and orphaned Python venvs — plus the
//! [`ComprehensiveCleanupResult`] aggregate of a full cleanup pass.

use crate::discovery::ProjectDiscovery;
use crate::graph::{DependencyResolver, PackageId};
use crate::python::PythonHome;
use crate::python::cleanup::{CleanupResult, PythonCleanupAnalyzer};
use crate::storage::InstalledPackage;
use hpm_config::ProjectsConfig;
use hpm_package::PackageManifest;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{info, warn};

use super::dev_install::{remove_install_entry, source_hash};
use super::dev_users::{
    Unrecorded, container_would_empty, is_copy_container, project_dev_copies, reclaim_dev_copies,
    record_project_manifests, remove_empty_container,
};
use super::{DevInstall, StorageError, StorageManager};
use crate::process_users::process_state;

/// Result of comprehensive cleanup including both packages and Python environments
#[derive(Debug)]
pub struct ComprehensiveCleanupResult {
    pub removed_packages: Vec<String>,
    /// Orphaned dev (path-dep) installs removed from `_dev/`. Identifiers are
    /// `_dev/<slug>@<version>` so CLI output makes the source obvious.
    pub removed_dev_installs: Vec<String>,
    pub python_cleanup: CleanupResult,
}

/// Everything one cleanup decides, from a single look at the store and the
/// projects. The dry run reports it; the real run executes it, so the two
/// cannot disagree about what is unused.
#[derive(Debug, Default)]
struct CleanupPlan {
    /// Registry packages no project and no global install reaches.
    orphan_packages: Vec<PackageId>,
    /// `None` when there was nothing to judge against: no dev installs, or
    /// no projects discovered (with no projects every install would look
    /// unused, so cleanup is skipped).
    dev: Option<DevPlan>,
}

#[derive(Debug)]
struct DevPlan {
    /// The `(slug, version)` coordinates projects' path dependencies claim,
    /// each with the source workspace it installs from.
    needed: HashMap<(String, String), PathBuf>,
    /// Dev installs no project claims.
    orphans: Vec<DevInstall>,
    /// Dev installs some project claims.
    referenced: Vec<DevInstall>,
    /// Each discovered project's `.hpm/packages`, whose manifests name the
    /// dev copies the project uses.
    project_packages_dirs: Vec<PathBuf>,
}

impl ComprehensiveCleanupResult {
    /// Total number of items that would be cleaned (packages + dev installs + venvs)
    pub fn total_items_that_would_be_cleaned(&self) -> usize {
        self.removed_packages.len()
            + self.removed_dev_installs.len()
            + self.python_cleanup.items_that_would_be_cleaned()
    }
}

impl StorageManager {
    /// Look at the store and the projects once and decide everything a
    /// cleanup removes. Writes nothing.
    async fn plan_cleanup(
        &self,
        projects_config: &ProjectsConfig,
    ) -> Result<CleanupPlan, StorageError> {
        let installed = self.list_installed()?;
        let dev_installs = self.list_dev_installs()?;
        if installed.is_empty() && dev_installs.is_empty() {
            info!("No packages installed - cleanup not needed");
            return Ok(CleanupPlan::default());
        }

        let projects = ProjectDiscovery::new(projects_config.clone()).find_projects()?;
        if projects.is_empty() {
            warn!(
                "No HPM-managed projects found - skipping cleanup to prevent removing all packages"
            );
            return Ok(CleanupPlan::default());
        }
        info!(
            "Found {} HPM-managed projects for cleanup analysis",
            projects.len()
        );

        let orphan_packages = if installed.is_empty() {
            Vec::new()
        } else {
            self.orphan_packages(&installed, &projects).await?
        };

        let dev = if dev_installs.is_empty() {
            None
        } else {
            let needed = dev_needs(&projects);
            let (orphans, referenced) = dev_installs
                .into_iter()
                .partition(|d| !needed.contains_key(&(d.slug.clone(), d.version.clone())));
            Some(DevPlan {
                needed,
                orphans,
                referenced,
                project_packages_dirs: projects
                    .iter()
                    .map(|p| p.path.join(".hpm").join("packages"))
                    .collect(),
            })
        };

        Ok(CleanupPlan {
            orphan_packages,
            dev,
        })
    }

    /// Registry packages that neither a project (directly or transitively)
    /// nor a global install needs.
    async fn orphan_packages(
        &self,
        installed: &[InstalledPackage],
        projects: &[crate::discovery::DiscoveredProject],
    ) -> Result<Vec<PackageId>, StorageError> {
        let resolver = DependencyResolver::new(Arc::new(self.clone()));
        let dependency_graph = resolver.build_dependency_graph(projects).await?;
        let roots: Vec<PackageId> = dependency_graph
            .nodes()
            .filter(|node| node.is_root)
            .map(|node| node.id.clone())
            .collect();
        let mut needed = dependency_graph.mark_reachable_from_roots(&roots);
        // Globally installed packages are roots too. They belong to no
        // project, so the project-driven marking cannot reach them: without
        // this, every `hpm global` install is classified as unreferenced and
        // its store directory deleted, leaving a manifest in Houdini's
        // packages directory pointing at a path that no longer exists.
        needed.extend(self.global_roots()?);
        info!(
            "{} of {} installed packages are needed (projects, their dependencies, global installs)",
            needed.len(),
            installed.len()
        );
        let all: HashSet<PackageId> = installed.iter().map(PackageId::from).collect();
        Ok(all.difference(&needed).cloned().collect())
    }

    /// GC roots contributed by `hpm global` installs, across every Houdini
    /// version's ledger.
    ///
    /// A ledger that cannot be read is a hard error rather than an empty
    /// result: "there are no global installs" and "I cannot tell what is
    /// installed globally" must not look the same to a garbage collector,
    /// since the second answered as the first deletes live packages.
    ///
    /// Ids are built the same way [`PackageId::from`] builds them for
    /// installed packages, so they line up with the marking set. That keying
    /// is by bare slug, so a global install also shelters a same-slug package
    /// from another creator at the same version. Over-protection is the safe
    /// direction for a collector; under-protection deletes live files.
    fn global_roots(&self) -> Result<HashSet<PackageId>, StorageError> {
        // LedgerError names the file or directory it could not read.
        let entries =
            crate::global::ledger::all_global_entries(&self.config.home_dir).map_err(|error| {
                StorageError::GlobalLedger {
                    message: error.to_string(),
                }
            })?;

        // The ledger keys installs by package (a key that is not one fails
        // the load above); the graph keys by bare slug.
        Ok(entries
            .into_iter()
            .map(|(package, entry)| PackageId::new(package.slug().to_string(), entry.version))
            .collect())
    }

    /// Remove orphaned packages. Returns identifiers of the packages actually removed.
    pub async fn cleanup_unused(
        &self,
        projects_config: &ProjectsConfig,
    ) -> Result<Vec<String>, StorageError> {
        info!("Starting project-aware package cleanup");
        let plan = self.plan_cleanup(projects_config).await?;
        Ok(self.remove_orphan_packages(&plan).await)
    }

    /// Plan — but don't execute — an orphan cleanup: the identifiers
    /// [`Self::cleanup_unused`] would remove. Writes nothing.
    pub async fn cleanup_unused_dry_run(
        &self,
        projects_config: &ProjectsConfig,
    ) -> Result<Vec<String>, StorageError> {
        let plan = self.plan_cleanup(projects_config).await?;
        Ok(orphan_package_ids(&plan))
    }

    async fn remove_orphan_packages(&self, plan: &CleanupPlan) -> Vec<String> {
        if plan.orphan_packages.is_empty() {
            info!("No orphaned packages found - cleanup not needed");
            return Vec::new();
        }
        info!(
            "Found {} orphaned packages to remove",
            plan.orphan_packages.len()
        );
        let mut removed = Vec::new();
        for package_id in &plan.orphan_packages {
            match self
                .remove_package(&package_id.name, &package_id.version)
                .await
            {
                Ok(()) => {
                    info!("Removed orphaned package: {}", package_id.identifier());
                    removed.push(package_id.identifier());
                }
                Err(e) => warn!(
                    "Failed to remove package {}: {}",
                    package_id.identifier(),
                    e
                ),
            }
        }
        info!(
            "Cleanup completed: removed {} orphaned packages",
            removed.len()
        );
        removed
    }

    /// Remove dev installs that no project's path-dependency claims, then
    /// reclaim superseded content copies of the installs that remain.
    /// Returns identifiers of the entries actually removed.
    ///
    /// Reclamation removes every `<container>/<hash>` copy nothing uses,
    /// except the one matching the current source (kept so an unchanged
    /// relaunch does not recopy). "Uses" is the rule in
    /// [`dev_users`](super::dev_users): a project whose manifest points at the
    /// copy, or a recorded process that still runs. An orphaned container
    /// whose copies are all unused goes; one with a copy in use stays.
    pub async fn cleanup_unused_dev_installs(
        &self,
        projects_config: &ProjectsConfig,
    ) -> Result<Vec<String>, StorageError> {
        let plan = self.plan_cleanup(projects_config).await?;
        Ok(self.remove_orphan_dev_installs(&plan))
    }

    /// Plan — but don't execute — a dev cleanup: the identifiers
    /// [`Self::cleanup_unused_dev_installs`] would remove. Writes nothing.
    pub async fn cleanup_unused_dev_installs_dry_run(
        &self,
        projects_config: &ProjectsConfig,
    ) -> Result<Vec<String>, StorageError> {
        let plan = self.plan_cleanup(projects_config).await?;
        Ok(orphan_dev_ids(&plan))
    }

    fn remove_orphan_dev_installs(&self, plan: &CleanupPlan) -> Vec<String> {
        let Some(dev) = &plan.dev else {
            return Vec::new();
        };
        // Make every project's dev copies known before anything is
        // reclaimed: a project last synced before copies recorded their
        // users has none on file, and unrecorded would read as unused. This
        // is a data migration; the dry run simulates it.
        for dir in &dev.project_packages_dirs {
            record_project_manifests(dir);
        }

        let mut removed = Vec::new();
        if dev.orphans.is_empty() {
            info!("No orphaned dev installs found");
        }
        for install in &dev.orphans {
            // A link install is unlinked, never followed. A copy container
            // goes copy by copy under the users rule, so a copy a running
            // process still reads is kept even though no project asks for its
            // version any more; the container goes once nothing is left.
            if is_copy_container(&install.install_path) {
                reclaim_dev_copies(
                    &install.install_path,
                    None,
                    Unrecorded::Remove,
                    process_state,
                );
                if remove_empty_container(&install.install_path) {
                    info!("Removed orphaned dev install: {}", install.identifier());
                    removed.push(install.identifier());
                } else {
                    info!(
                        "Kept dev install {}: a copy in it is still in use",
                        install.identifier()
                    );
                }
                continue;
            }
            let meta = match std::fs::symlink_metadata(&install.install_path) {
                Ok(m) => m,
                Err(e) => {
                    warn!(
                        "Failed to stat dev install {} at {}: {}",
                        install.identifier(),
                        install.install_path.display(),
                        e
                    );
                    continue;
                }
            };
            match remove_install_entry(
                &install.install_path,
                &meta,
                &install.slug,
                &install.version,
            ) {
                Ok(()) => {
                    info!("Removed orphaned dev install: {}", install.identifier());
                    removed.push(install.identifier());
                }
                Err(e) => warn!(
                    "Failed to remove dev install {}: {}",
                    install.identifier(),
                    e
                ),
            }
        }

        // Reclaim superseded content copies of the installs still claimed,
        // keeping the copy of the current source.
        for install in &dev.referenced {
            let Some(source) = dev
                .needed
                .get(&(install.slug.clone(), install.version.clone()))
            else {
                continue;
            };
            // Only copy containers carry hash subdirs; judged without
            // following a link, which `is_dir` would do into the workspace.
            if !is_copy_container(&install.install_path) {
                continue;
            }
            match source_hash(source) {
                Ok(hash) => {
                    reclaim_dev_copies(
                        &install.install_path,
                        Some(&hash),
                        Unrecorded::Remove,
                        process_state,
                    );
                }
                Err(e) => warn!(
                    "Could not fingerprint source for {} at {}; skipping copy reclamation: {}",
                    install.identifier(),
                    source.display(),
                    e
                ),
            }
        }
        removed
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

        let plan = self.plan_cleanup(projects_config).await?;

        // 1. Registry packages, and 2. dev (path-dep) installs, which
        //    `list_installed` does not list.
        let (removed_packages, removed_dev_installs) = if dry_run {
            (orphan_package_ids(&plan), orphan_dev_ids(&plan))
        } else {
            (
                self.remove_orphan_packages(&plan).await,
                self.remove_orphan_dev_installs(&plan),
            )
        };

        // 3. The packages that remain (or would remain). Venv ownership is
        //    recorded scoped (`creator/slug@version`), while package ids are
        //    `slug@version`: filter by id, emit the venv reference.
        let removed: HashSet<&str> = removed_packages.iter().map(String::as_str).collect();
        let remaining_packages: Vec<String> = self
            .list_installed()?
            .iter()
            .filter(|p| !removed.contains(PackageId::from(*p).identifier().as_str()))
            .map(InstalledPackage::venv_ref)
            .collect();

        // 4. Python virtual environment cleanup against the remaining set.
        let python_analyzer = PythonCleanupAnalyzer::new(&PythonHome::for_storage(&self.config));
        let orphaned_venvs = python_analyzer
            .analyze_orphaned_venvs(&remaining_packages)
            .await?;
        let python_cleanup = python_analyzer
            .cleanup_orphaned_venvs(&orphaned_venvs, dry_run)
            .await?;

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

        let python_analyzer = PythonCleanupAnalyzer::new(&PythonHome::for_storage(&self.config));

        // Get list of all active packages
        let active_packages = self.list_installed()?;
        let active_package_names: Vec<String> = active_packages
            .iter()
            .map(InstalledPackage::venv_ref)
            .collect();

        // Find orphaned virtual environments
        let orphaned_venvs = python_analyzer
            .analyze_orphaned_venvs(&active_package_names)
            .await?;

        // Clean up (or dry run)
        let result = python_analyzer
            .cleanup_orphaned_venvs(&orphaned_venvs, dry_run)
            .await?;

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

/// The identifiers a plan's registry-package removal would produce.
fn orphan_package_ids(plan: &CleanupPlan) -> Vec<String> {
    let ids: Vec<String> = plan
        .orphan_packages
        .iter()
        .map(PackageId::identifier)
        .collect();
    info!("Dry run: would remove {} orphaned packages", ids.len());
    for id in &ids {
        info!("Would remove: {id}");
    }
    ids
}

/// The identifiers a plan's dev removal would produce, judged without
/// changing anything: every orphaned link install, and every orphaned copy
/// container whose copies are all unused, counting the copies the projects'
/// manifests point at as used (the real run records those first).
fn orphan_dev_ids(plan: &CleanupPlan) -> Vec<String> {
    let Some(dev) = &plan.dev else {
        return Vec::new();
    };
    let project_copies: HashSet<PathBuf> = dev
        .project_packages_dirs
        .iter()
        .flat_map(|dir| project_dev_copies(dir))
        .map(|(_, copy)| copy)
        .collect();
    let ids: Vec<String> = dev
        .orphans
        .iter()
        .filter(|install| {
            !is_copy_container(&install.install_path)
                || container_would_empty(&install.install_path, process_state, &project_copies)
        })
        .map(DevInstall::identifier)
        .collect();
    info!("Dry run: would remove {} orphaned dev installs", ids.len());
    for id in &ids {
        info!("Would remove: {id}");
    }
    ids
}

/// The `(slug, version)` coordinates the projects' path dependencies claim,
/// each with its source workspace. A dependency whose source manifest cannot
/// be read is skipped with a warning: a broken project does not block
/// cleanup, and re-running `hpm install` re-creates what it needs.
fn dev_needs(
    projects: &[crate::discovery::DiscoveredProject],
) -> HashMap<(String, String), PathBuf> {
    let mut needed = HashMap::new();
    for project in projects {
        for (dep_name, spec) in &project.manifest.dependencies {
            let hpm_package::DependencySpec::Path { path, .. } = spec else {
                continue;
            };
            // Resolved against the project directory, as install does.
            let source = project.path.join(path);
            match PackageManifest::from_path(&source.join("hpm.toml")) {
                Ok(m) => {
                    needed.insert(
                        (m.package.slug().to_string(), m.package.version.clone()),
                        source,
                    );
                }
                Err(e) => warn!(
                    "Project {} has path dep {} pointing at {}, but its manifest is unreadable ({}); \
                     dev install from this dep will not be protected from cleanup",
                    project.path.display(),
                    dep_name,
                    source.display(),
                    e
                ),
            }
        }
    }
    needed
}
