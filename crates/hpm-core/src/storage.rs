use crate::dependency::{DependencyResolver, PackageId};
use crate::discovery::ProjectDiscovery;
use hpm_config::{ProjectsConfig, StorageConfig};
use hpm_package::{ManifestLoadError, PackageManifest};
use hpm_python::cleanup::{CleanupResult, PythonCleanupAnalyzer};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{debug, info, warn};

pub mod types;
pub use types::{InstalledPackage, PackageSpec, VersionReq};

/// Subdirectory of `packages_dir` reserved for path-installed (dev) packages.
/// Kept out of the registry CAS namespace so a dev install of `foo@1.0.0`
/// can coexist with — and is never substituted for — a registry install at
/// the same coordinate.
const DEV_INSTALL_DIR: &str = "_dev";

/// Remove `path` if it is a symlink/junction, without following the link.
fn remove_dev_link(path: &std::path::Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        // On Unix, symlink-to-directory entries are removed via `remove_file`.
        std::fs::remove_file(path)
    }
    #[cfg(windows)]
    {
        // `junction::delete` strips the reparse point but leaves the now-empty
        // directory stub in place — re-creating the link at the same path
        // would then fail with ERROR_ALREADY_EXISTS (os error 183). Remove the
        // stub explicitly. The same applies to NTFS symlinks-to-dirs, whose
        // reparse point sits on a directory entry that survives `delete`.
        junction::delete(path)?;
        std::fs::remove_dir(path)
    }
}

/// Create a symlink (Unix) or junction (Windows) at `link` pointing at the
/// absolute `target`. The target must be a directory.
fn create_dev_link(target: &std::path::Path, link: &std::path::Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link)
    }
    #[cfg(windows)]
    {
        // Junctions are intentional here (vs NTFS symlinks): they don't
        // require Developer Mode or admin, which makes the link-install
        // workflow viable on a stock Houdini workstation.
        junction::create(target, link)
    }
}

/// Returns true when the entry at `path` is a symlink (Unix) or a
/// junction/symlink (Windows). Caller must have already verified the entry
/// exists (typically by reading `symlink_metadata` themselves) — this helper
/// is a pure file-type predicate that doesn't follow links.
fn is_link_entry(meta: &std::fs::Metadata, path: &std::path::Path) -> bool {
    if meta.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        // Older Rust stdlib reports junctions as non-symlinks; ask the
        // junction crate directly so callers never accidentally fall through
        // to `remove_dir_all` on a reparse point.
        junction::exists(path).unwrap_or(false)
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        false
    }
}

/// Remove an install entry without following links. The caller is
/// responsible for verifying the entry exists (and supplying the matching
/// metadata) so this stays a pure removal primitive.
///
/// - Symlink/junction → remove the link entry itself.
/// - Real directory → `remove_dir_all`, with Houdini-handle errors lifted to
///   [`StorageError::PackageInUse`] so the user gets an actionable message.
fn remove_install_entry(
    target_dir: &std::path::Path,
    meta: &std::fs::Metadata,
    name: &str,
    version: &str,
) -> Result<(), StorageError> {
    if is_link_entry(meta, target_dir) {
        return remove_dev_link(target_dir).map_err(StorageError::DirectoryRemoval);
    }
    std::fs::remove_dir_all(target_dir).map_err(|e| {
        // On Windows, a running Houdini process holds open handles to files
        // inside the package dir, so removal fails with ERROR_ACCESS_DENIED
        // (os error 5 → PermissionDenied). Map it to an actionable error
        // instead of leaking a raw OS code.
        if e.kind() == std::io::ErrorKind::PermissionDenied {
            StorageError::PackageInUse {
                name: name.to_string(),
                version: version.to_string(),
                source: e,
            }
        } else {
            StorageError::DirectoryRemoval(e)
        }
    })
}

/// Replace whatever is currently at `target_dir` with a clean slate, with
/// link-aware removal semantics. Always safe to call before installing.
///
/// - Missing → no-op.
/// - Symlink/junction → remove the link entry itself; never follow.
/// - Real directory → `remove_dir_all`, with Houdini-handle errors lifted to
///   [`StorageError::PackageInUse`] so the user gets an actionable message.
fn clear_existing_install(
    target_dir: &std::path::Path,
    name: &str,
    version: &str,
) -> Result<(), StorageError> {
    let meta = match std::fs::symlink_metadata(target_dir) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(StorageError::DirectoryRemoval(e)),
    };

    if is_link_entry(&meta, target_dir) {
        warn!(
            "replacing existing link install for {}@{} at {}",
            name,
            version,
            target_dir.display()
        );
    } else {
        warn!(
            "package {}@{} already exists, removing old version",
            name, version
        );
    }
    remove_install_entry(target_dir, &meta, name, version)
}

#[derive(Debug, Clone, Copy)]
enum InstallStyle {
    /// Registry/URL fetch → copy into the CAS at `packages_dir/<slug>@<ver>/`.
    CasCopy,
    /// Path dep → copy into `packages_dir/_dev/<slug>@<ver>/`.
    DevCopy,
    /// Path dep → symlink/junction at `packages_dir/_dev/<slug>@<ver>/`
    /// pointing at the workspace.
    DevLink,
}

impl InstallStyle {
    fn log_kind(self) -> &'static str {
        match self {
            InstallStyle::CasCopy => "",
            InstallStyle::DevCopy => "dev ",
            InstallStyle::DevLink => "dev-link ",
        }
    }
}

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

/// A path-installed (dev) package entry under `<packages_dir>/_dev/`.
///
/// Identity comes from the directory name (`<slug>@<version>`), not from
/// reading the entry's `hpm.toml` — link installs that point at a deleted
/// workspace still surface as a `DevInstall` so cleanup can collect them.
#[derive(Debug, Clone)]
pub struct DevInstall {
    pub slug: String,
    pub version: String,
    pub install_path: PathBuf,
}

impl DevInstall {
    /// Identifier used in CLI output and `removed_dev_installs`. Prefixed
    /// with `_dev/` so users can distinguish dev cleanup from CAS cleanup
    /// in the same `hpm clean` listing.
    pub fn identifier(&self) -> String {
        format!("_dev/{}@{}", self.slug, self.version)
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
        self.config
            .ensure_directories()
            .map_err(StorageError::DirectoryCreation)?;
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
    /// (`install_from_path_dev`) and is intentionally invisible to
    /// `list_installed`. Otherwise an `ensure_installed` cache lookup that
    /// matches on `(name, version)` could return dev content for a registry
    /// resolution at the same coordinate — see the CAS-poisoning bug.
    fn collect_installed_packages(
        &self,
        dir: &std::path::Path,
        packages: &mut Vec<InstalledPackage>,
    ) -> Result<(), StorageError> {
        let entries =
            std::fs::read_dir(dir).map_err(|e| StorageError::DirectoryRead(e.to_string()))?;

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
    /// For user-authored path dependencies, use [`install_from_path_dev`]
    /// instead — that keeps dev content out of the registry CAS so a dev
    /// install of `foo@1.0.0` doesn't get served to a different project that
    /// resolves the same coordinate from a registry.
    ///
    /// [`install_from_path_dev`]: Self::install_from_path_dev
    pub async fn install_from_path(
        &self,
        source_path: &std::path::Path,
    ) -> Result<InstalledPackage, StorageError> {
        self.install_from_path_inner(source_path, InstallStyle::CasCopy)
            .await
    }

    /// Install a path-dependency into the dev subtree
    /// (`<packages_dir>/_dev/<slug>@<version>/`).
    ///
    /// Dev installs live in their own namespace because they share `(slug,
    /// version)` keys with registry packages but carry user-authored content
    /// that must not be cached as the canonical install for that coordinate.
    pub async fn install_from_path_dev(
        &self,
        source_path: &std::path::Path,
    ) -> Result<InstalledPackage, StorageError> {
        self.install_from_path_inner(source_path, InstallStyle::DevCopy)
            .await
    }

    /// Install a path-dependency into the dev subtree as a symlink (Unix) or
    /// junction (Windows). Working-tree edits at `source_path` become visible
    /// to a live Houdini session immediately, with no re-sync.
    ///
    /// Same namespace isolation as [`install_from_path_dev`]: the link entry
    /// lives at `<packages_dir>/_dev/<slug>@<version>/`, never in the registry
    /// CAS. Registry resolutions at the same coordinate are unaffected.
    pub async fn install_from_path_dev_link(
        &self,
        source_path: &std::path::Path,
    ) -> Result<InstalledPackage, StorageError> {
        self.install_from_path_inner(source_path, InstallStyle::DevLink)
            .await
    }

    async fn install_from_path_inner(
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
                let absolute_source = std::fs::canonicalize(source_path).map_err(|e| {
                    StorageError::DirectoryRead(format!(
                        "Failed to canonicalize link source {}: {}",
                        source_path.display(),
                        e
                    ))
                })?;
                if let Some(parent) = target_dir.parent() {
                    std::fs::create_dir_all(parent).map_err(StorageError::DirectoryCreation)?;
                }
                create_dev_link(&absolute_source, &target_dir).map_err(|e| {
                    StorageError::DirectoryRead(format!(
                        "Failed to create dev link {} -> {}: {}",
                        target_dir.display(),
                        absolute_source.display(),
                        e
                    ))
                })?;
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
        std::fs::create_dir_all(target).map_err(StorageError::DirectoryCreation)?;

        for entry in walkdir::WalkDir::new(source)
            .min_depth(1)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let relative_path = entry
                .path()
                .strip_prefix(source)
                .map_err(|e| StorageError::DirectoryRead(e.to_string()))?;
            let target_path = target.join(relative_path);

            if entry.file_type().is_dir() {
                std::fs::create_dir_all(&target_path).map_err(StorageError::DirectoryCreation)?;
            } else {
                // Ensure parent directory exists
                if let Some(parent) = target_path.parent() {
                    std::fs::create_dir_all(parent).map_err(StorageError::DirectoryCreation)?;
                }
                std::fs::copy(entry.path(), &target_path).map_err(|e| {
                    StorageError::DirectoryRead(format!("Failed to copy file: {}", e))
                })?;
            }
        }

        Ok(())
    }

    /// Find the best installed version matching a requirement
    pub fn find_installed(&self, name: &str, version_req: &VersionReq) -> Option<InstalledPackage> {
        let installed = self.list_installed().ok()?;
        installed
            .into_iter()
            .filter(|pkg| {
                pkg.manifest.package.slug() == name && pkg.is_compatible_with(version_req)
            })
            .max_by(|a, b| {
                // Compare versions - prefer higher versions
                match (
                    semver::Version::parse(&a.version),
                    semver::Version::parse(&b.version),
                ) {
                    (Ok(va), Ok(vb)) => va.cmp(&vb),
                    _ => a.version.cmp(&b.version),
                }
            })
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
            Err(e) => return Err(StorageError::DirectoryRemoval(e)),
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
        let projects = project_discovery
            .find_projects()
            .map_err(|e| StorageError::ProjectDiscovery(e.to_string()))?;

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
        let dependency_graph = resolver
            .build_dependency_graph(&projects)
            .await
            .map_err(|e| StorageError::DependencyResolution(e.to_string()))?;

        // 4. Collect root packages (directly required by projects)
        let root_packages: Vec<PackageId> = dependency_graph
            .nodes()
            .values()
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
        let entries =
            std::fs::read_dir(&dev_root).map_err(|e| StorageError::DirectoryRead(e.to_string()))?;
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
        let projects = project_discovery
            .find_projects()
            .map_err(|e| StorageError::ProjectDiscovery(e.to_string()))?;

        if projects.is_empty() {
            warn!(
                "No HPM-managed projects found - skipping dev cleanup to prevent removing dev installs"
            );
            return Ok(Vec::new());
        }

        let mut needed: HashSet<(String, String)> = HashSet::new();
        for project in &projects {
            let Some(deps) = &project.manifest.dependencies else {
                continue;
            };
            for (dep_name, spec) in deps {
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
        let all_installed = self
            .list_installed()
            .map_err(|e| StorageError::DirectoryRead(e.to_string()))?;
        let remaining_packages: Vec<String> = all_installed
            .into_iter()
            .filter_map(|p| {
                let id = format!("{}@{}", p.manifest.package.slug(), p.version);
                (!removed_packages.contains(&id)).then_some(id)
            })
            .collect();

        // 4. Python virtual environment cleanup against the remaining set.
        let python_analyzer = PythonCleanupAnalyzer::new();
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

        let python_analyzer = PythonCleanupAnalyzer::new();

        // Get list of all active packages
        let active_packages = self
            .list_installed()
            .map_err(|e| StorageError::DirectoryRead(e.to_string()))?;
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

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("Directory creation failed: {0}")]
    DirectoryCreation(#[source] std::io::Error),

    #[error("Directory read failed: {0}")]
    DirectoryRead(String),

    #[error("Directory removal failed: {0}")]
    DirectoryRemoval(#[source] std::io::Error),

    #[error(transparent)]
    Manifest(#[from] ManifestLoadError),

    #[error("Metadata read failed: {0}")]
    MetadataRead(#[source] std::io::Error),

    #[error("Package not found: {0}")]
    PackageNotFound(String),

    #[error("Feature not implemented: {0}")]
    NotImplemented(String),

    #[error("Project discovery failed: {0}")]
    ProjectDiscovery(String),

    #[error("Dependency resolution failed: {0}")]
    DependencyResolution(String),

    #[error("Python cleanup failed: {0}")]
    PythonCleanup(String),

    #[error(
        "Package {name}@{version} is in use by another process; close any \
         running Houdini that depends on it and try again ({source})"
    )]
    PackageInUse {
        name: String,
        version: String,
        #[source]
        source: std::io::Error,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn storage_manager_creation() {
        let temp_dir = TempDir::new().unwrap();
        let storage_config = StorageConfig {
            home_dir: temp_dir.path().to_path_buf(),
            cache_dir: temp_dir.path().join("cache"),
            packages_dir: temp_dir.path().join("packages"),
            registry_cache_dir: temp_dir.path().join("registry"),
        };

        let _storage_manager = StorageManager::new(storage_config).unwrap();
        assert!(temp_dir.path().join("packages").exists());
        assert!(temp_dir.path().join("cache").exists());
        assert!(temp_dir.path().join("registry").exists());
    }

    #[test]
    fn package_exists_check() {
        let temp_dir = TempDir::new().unwrap();
        let storage_config = StorageConfig {
            home_dir: temp_dir.path().to_path_buf(),
            cache_dir: temp_dir.path().join("cache"),
            packages_dir: temp_dir.path().join("packages"),
            registry_cache_dir: temp_dir.path().join("registry"),
        };

        let storage_manager = StorageManager::new(storage_config).unwrap();

        assert!(!storage_manager.package_exists("test-package", "1.0.0"));

        // Create a fake package directory
        let package_dir = temp_dir.path().join("packages").join("test-package@1.0.0");
        std::fs::create_dir_all(&package_dir).unwrap();
        std::fs::write(
            package_dir.join("hpm.toml"),
            "[package]\npath = \"studio/test-package\"\nname = \"Test Package\"\nversion = \"1.0.0\"",
        )
        .unwrap();

        assert!(storage_manager.package_exists("test-package", "1.0.0"));
    }

    #[test]
    fn list_installed_packages() {
        let temp_dir = TempDir::new().unwrap();
        let storage_config = StorageConfig {
            home_dir: temp_dir.path().to_path_buf(),
            cache_dir: temp_dir.path().join("cache"),
            packages_dir: temp_dir.path().join("packages"),
            registry_cache_dir: temp_dir.path().join("registry"),
        };

        let storage_manager = StorageManager::new(storage_config).unwrap();

        // Initially no packages
        let packages = storage_manager.list_installed().unwrap();
        assert_eq!(packages.len(), 0);

        // Create a fake package
        let package_dir = temp_dir.path().join("packages").join("test-package@1.0.0");
        std::fs::create_dir_all(&package_dir).unwrap();

        let manifest_content = r#"
[package]
path = "studio/test-package"
name = "Test Package"
version = "1.0.0"
description = "A test package"

[compat]
houdini = ">=20.5"
"#;
        std::fs::write(package_dir.join("hpm.toml"), manifest_content).unwrap();

        let packages = storage_manager.list_installed().unwrap();
        assert_eq!(packages.len(), 1);
        assert_eq!(packages[0].slug(), "test-package");
        assert_eq!(packages[0].version, "1.0.0");
    }

    // Error path tests

    #[tokio::test]
    async fn remove_nonexistent_package_fails() {
        let temp_dir = TempDir::new().unwrap();
        let storage_config = StorageConfig {
            home_dir: temp_dir.path().to_path_buf(),
            packages_dir: temp_dir.path().join("packages"),
            cache_dir: temp_dir.path().join("cache"),
            registry_cache_dir: temp_dir.path().join("registry"),
        };
        let storage_manager = StorageManager::new(storage_config).unwrap();

        let result = storage_manager.remove_package("nonexistent", "1.0.0").await;
        assert!(result.is_err());
        match result {
            Err(StorageError::PackageNotFound(msg)) => {
                assert!(msg.contains("nonexistent"));
            }
            _ => panic!("Expected PackageNotFound error"),
        }
    }

    /// Defensive: if a junction/symlink ever lands at the registry CAS path
    /// (manually, or via future code), `remove_package` must remove the link
    /// entry itself rather than follow it. On Unix this is mostly a
    /// belt-and-braces check (remove_dir_all on a symlink errors anyway); on
    /// Windows this is the load-bearing safety property.
    #[tokio::test]
    async fn remove_package_unlinks_symlink_entries() {
        let temp_dir = TempDir::new().unwrap();
        let storage_config = StorageConfig {
            home_dir: temp_dir.path().to_path_buf(),
            packages_dir: temp_dir.path().join("packages"),
            cache_dir: temp_dir.path().join("cache"),
            registry_cache_dir: temp_dir.path().join("registry"),
        };
        let storage_manager = StorageManager::new(storage_config).unwrap();

        // Stand up an external "workspace" with a manifest.
        let external = temp_dir.path().join("external-workspace");
        write_source_package(&external, "creator/foo", "1.0.0", "external-marker");

        // Manually plant a symlink/junction at the registry CAS path that
        // `remove_package` resolves to. `package_dir(name, version)` uses the
        // bare-slug layout, so the link lives at `<packages_dir>/<slug>@<v>/`.
        let cas_path = storage_manager.config.package_dir("foo", "1.0.0");
        std::fs::create_dir_all(cas_path.parent().unwrap()).unwrap();
        create_dev_link(&external, &cas_path).unwrap();
        assert!(cas_path.join("MARKER").exists());

        storage_manager
            .remove_package("foo", "1.0.0")
            .await
            .unwrap();

        // CAS path is gone, external workspace survives intact.
        assert!(std::fs::symlink_metadata(&cas_path).is_err());
        assert_eq!(
            std::fs::read_to_string(external.join("MARKER")).unwrap(),
            "external-marker"
        );
    }

    #[test]
    fn list_packages_with_corrupted_manifest() {
        let temp_dir = TempDir::new().unwrap();
        let storage_config = StorageConfig {
            home_dir: temp_dir.path().to_path_buf(),
            packages_dir: temp_dir.path().join("packages"),
            cache_dir: temp_dir.path().join("cache"),
            registry_cache_dir: temp_dir.path().join("registry"),
        };
        let storage_manager = StorageManager::new(storage_config).unwrap();

        // Create a package directory with a corrupted manifest
        let package_dir = temp_dir.path().join("packages").join("corrupted-pkg@1.0.0");
        std::fs::create_dir_all(&package_dir).unwrap();
        std::fs::write(
            package_dir.join("hpm.toml"),
            "this is not valid toml { [ broken",
        )
        .unwrap();

        let result = storage_manager.list_installed();
        assert!(result.is_err());
        match result {
            Err(StorageError::Manifest(ManifestLoadError::Parse { path, .. })) => {
                assert!(path.ends_with("hpm.toml"));
            }
            other => panic!("Expected Manifest::Parse error, got: {:?}", other),
        }
    }

    #[test]
    fn list_packages_skips_non_directories() {
        let temp_dir = TempDir::new().unwrap();
        let storage_config = StorageConfig {
            home_dir: temp_dir.path().to_path_buf(),
            packages_dir: temp_dir.path().join("packages"),
            cache_dir: temp_dir.path().join("cache"),
            registry_cache_dir: temp_dir.path().join("registry"),
        };
        let storage_manager = StorageManager::new(storage_config).unwrap();

        // Create the packages directory and add a file (not a directory)
        std::fs::create_dir_all(temp_dir.path().join("packages")).unwrap();
        std::fs::write(
            temp_dir.path().join("packages").join("random-file.txt"),
            "not a package",
        )
        .unwrap();

        // Should not error, just skip the file
        let packages = storage_manager.list_installed().unwrap();
        assert!(packages.is_empty());
    }

    #[test]
    fn list_packages_skips_directories_without_manifest() {
        let temp_dir = TempDir::new().unwrap();
        let storage_config = StorageConfig {
            home_dir: temp_dir.path().to_path_buf(),
            packages_dir: temp_dir.path().join("packages"),
            cache_dir: temp_dir.path().join("cache"),
            registry_cache_dir: temp_dir.path().join("registry"),
        };
        let storage_manager = StorageManager::new(storage_config).unwrap();

        // Create a package directory without hpm.toml
        let package_dir = temp_dir.path().join("packages").join("empty-pkg@1.0.0");
        std::fs::create_dir_all(&package_dir).unwrap();
        std::fs::write(package_dir.join("README.md"), "no manifest here").unwrap();

        // Should not error, just skip directories without manifest
        let packages = storage_manager.list_installed().unwrap();
        assert!(packages.is_empty());
    }

    #[test]
    fn list_installed_scoped_packages() {
        let temp_dir = TempDir::new().unwrap();
        let storage_config = StorageConfig {
            home_dir: temp_dir.path().to_path_buf(),
            cache_dir: temp_dir.path().join("cache"),
            packages_dir: temp_dir.path().join("packages"),
            registry_cache_dir: temp_dir.path().join("registry"),
        };

        let storage_manager = StorageManager::new(storage_config).unwrap();

        // Create a scoped package at packages/tumblehead/fire-fx@1.0.0/
        let package_dir = temp_dir
            .path()
            .join("packages")
            .join("tumblehead")
            .join("fire-fx@1.0.0");
        std::fs::create_dir_all(&package_dir).unwrap();

        let manifest_content = r#"
[package]
path = "tumblehead/fire-fx"
name = "Fire FX"
version = "1.0.0"
description = "A fire effects package"

[compat]
houdini = ">=20.5"
"#;
        std::fs::write(package_dir.join("hpm.toml"), manifest_content).unwrap();

        // Also create a non-scoped package at packages/old-pkg@2.0.0/
        let old_pkg_dir = temp_dir.path().join("packages").join("old-pkg@2.0.0");
        std::fs::create_dir_all(&old_pkg_dir).unwrap();
        std::fs::write(
            old_pkg_dir.join("hpm.toml"),
            "[package]\npath = \"studio/old-pkg\"\nname = \"Old Package\"\nversion = \"2.0.0\"",
        )
        .unwrap();

        let packages = storage_manager.list_installed().unwrap();
        assert_eq!(packages.len(), 2);

        // Find the scoped package
        let fire_fx = packages
            .iter()
            .find(|p| p.manifest.package.slug() == "fire-fx")
            .unwrap();
        assert_eq!(fire_fx.version, "1.0.0");

        // Find the non-scoped package
        let old_pkg = packages
            .iter()
            .find(|p| p.manifest.package.slug() == "old-pkg")
            .unwrap();
        assert_eq!(old_pkg.version, "2.0.0");
    }

    #[tokio::test]
    async fn install_from_path_without_manifest_fails() {
        let temp_dir = TempDir::new().unwrap();
        let storage_config = StorageConfig {
            home_dir: temp_dir.path().to_path_buf(),
            packages_dir: temp_dir.path().join("packages"),
            cache_dir: temp_dir.path().join("cache"),
            registry_cache_dir: temp_dir.path().join("registry"),
        };
        let storage_manager = StorageManager::new(storage_config).unwrap();

        // Create a source directory without hpm.toml
        let source_dir = temp_dir.path().join("source-pkg");
        std::fs::create_dir_all(&source_dir).unwrap();

        let result = storage_manager.install_from_path(&source_dir).await;
        assert!(result.is_err());
        match result {
            Err(StorageError::Manifest(ManifestLoadError::NotFound { path })) => {
                assert!(path.ends_with("hpm.toml"));
            }
            other => panic!("Expected Manifest::NotFound error, got: {:?}", other),
        }
    }

    /// Build a minimal source package directory at `dir` with the given
    /// scoped path, version, and a marker file recording who created it.
    /// The marker lets a test distinguish dev content from registry content
    /// after it lands in the CAS.
    fn write_source_package(
        dir: &std::path::Path,
        package_path: &str,
        version: &str,
        marker: &str,
    ) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(
            dir.join("hpm.toml"),
            format!(
                "[package]\npath = \"{package_path}\"\nname = \"{package_path}\"\nversion = \"{version}\"\n",
            ),
        )
        .unwrap();
        std::fs::write(dir.join("MARKER"), marker).unwrap();
    }

    /// Regression: a dev install must land in the `_dev` subtree, not in the
    /// registry CAS. Otherwise a registry resolution at the same `(slug,
    /// version)` would pick up the dev content via the CAS short-circuit.
    #[tokio::test]
    async fn install_from_path_dev_targets_dev_subtree() {
        let temp_dir = TempDir::new().unwrap();
        let storage_config = StorageConfig {
            home_dir: temp_dir.path().to_path_buf(),
            packages_dir: temp_dir.path().join("packages"),
            cache_dir: temp_dir.path().join("cache"),
            registry_cache_dir: temp_dir.path().join("registry"),
        };
        let storage_manager = StorageManager::new(storage_config).unwrap();

        let source = temp_dir.path().join("dev-source");
        write_source_package(&source, "creator/foo", "1.0.0", "from-dev-source");

        let installed = storage_manager
            .install_from_path_dev(&source)
            .await
            .unwrap();

        let expected = temp_dir
            .path()
            .join("packages")
            .join("_dev")
            .join("foo@1.0.0");
        assert_eq!(installed.install_path, expected);
        assert!(expected.join("MARKER").exists());

        // The registry CAS path must remain empty.
        let registry_cas = temp_dir.path().join("packages").join("foo@1.0.0");
        assert!(
            !registry_cas.exists(),
            "dev install must not touch the registry CAS path"
        );
    }

    /// Regression: `list_installed` is the cache the project's
    /// `ensure_installed`/`ensure_installed_from_url` short-circuits consult.
    /// If a dev install showed up there, a different project resolving the
    /// same coordinate from a registry would skip the fetch and silently
    /// hand out dev content. Skipping the `_dev` subtree closes that path.
    #[tokio::test]
    async fn list_installed_skips_dev_subtree() {
        let temp_dir = TempDir::new().unwrap();
        let storage_config = StorageConfig {
            home_dir: temp_dir.path().to_path_buf(),
            packages_dir: temp_dir.path().join("packages"),
            cache_dir: temp_dir.path().join("cache"),
            registry_cache_dir: temp_dir.path().join("registry"),
        };
        let storage_manager = StorageManager::new(storage_config).unwrap();

        // Dev install of foo@1.0.0
        let dev_source = temp_dir.path().join("dev-source");
        write_source_package(&dev_source, "creator/foo", "1.0.0", "dev");
        storage_manager
            .install_from_path_dev(&dev_source)
            .await
            .unwrap();

        // Independent registry-style install of bar@2.0.0
        let reg_source = temp_dir.path().join("reg-source");
        write_source_package(&reg_source, "creator/bar", "2.0.0", "registry");
        storage_manager
            .install_from_path(&reg_source)
            .await
            .unwrap();

        let listed = storage_manager.list_installed().unwrap();
        let names: Vec<&str> = listed.iter().map(|p| p.manifest.package.slug()).collect();
        assert!(
            !names.contains(&"foo"),
            "list_installed must hide dev installs; got {:?}",
            names
        );
        assert!(
            names.contains(&"bar"),
            "list_installed must surface registry installs; got {:?}",
            names
        );
    }

    /// Regression: a dev install at `foo@1.0.0` must coexist with a registry
    /// install at `foo@1.0.0`. Each lives in its own subtree, so neither
    /// install's content overwrites the other when both are present.
    #[tokio::test]
    async fn dev_and_registry_installs_coexist_at_same_coordinate() {
        let temp_dir = TempDir::new().unwrap();
        let storage_config = StorageConfig {
            home_dir: temp_dir.path().to_path_buf(),
            packages_dir: temp_dir.path().join("packages"),
            cache_dir: temp_dir.path().join("cache"),
            registry_cache_dir: temp_dir.path().join("registry"),
        };
        let storage_manager = StorageManager::new(storage_config).unwrap();

        let dev_source = temp_dir.path().join("dev-source");
        write_source_package(&dev_source, "creator/foo", "1.0.0", "dev-content");
        storage_manager
            .install_from_path_dev(&dev_source)
            .await
            .unwrap();

        let reg_source = temp_dir.path().join("reg-source");
        write_source_package(&reg_source, "creator/foo", "1.0.0", "registry-content");
        storage_manager
            .install_from_path(&reg_source)
            .await
            .unwrap();

        let dev_marker = temp_dir
            .path()
            .join("packages")
            .join("_dev")
            .join("foo@1.0.0")
            .join("MARKER");
        let reg_marker = temp_dir
            .path()
            .join("packages")
            .join("foo@1.0.0")
            .join("MARKER");
        assert_eq!(std::fs::read_to_string(&dev_marker).unwrap(), "dev-content");
        assert_eq!(
            std::fs::read_to_string(&reg_marker).unwrap(),
            "registry-content"
        );
    }

    /// Link-mode dev install creates a symlink/junction at
    /// `_dev/<slug>@<version>/`. Reading through the link must reach the
    /// workspace (this is the whole point of the feature).
    #[tokio::test]
    async fn install_from_path_dev_link_creates_link_to_workspace() {
        let temp_dir = TempDir::new().unwrap();
        let storage_config = StorageConfig {
            home_dir: temp_dir.path().to_path_buf(),
            packages_dir: temp_dir.path().join("packages"),
            cache_dir: temp_dir.path().join("cache"),
            registry_cache_dir: temp_dir.path().join("registry"),
        };
        let storage_manager = StorageManager::new(storage_config).unwrap();

        let source = temp_dir.path().join("link-source");
        write_source_package(&source, "creator/foo", "1.0.0", "link-content");

        let installed = storage_manager
            .install_from_path_dev_link(&source)
            .await
            .unwrap();

        let expected = temp_dir
            .path()
            .join("packages")
            .join("_dev")
            .join("foo@1.0.0");
        assert_eq!(installed.install_path, expected);

        // The install entry is a symlink/junction, not a real directory.
        let meta = std::fs::symlink_metadata(&expected).unwrap();
        let is_link = meta.file_type().is_symlink() || {
            #[cfg(windows)]
            {
                junction::exists(&expected).unwrap_or(false)
            }
            #[cfg(not(windows))]
            {
                false
            }
        };
        assert!(is_link, "dev link install must be a symlink/junction");

        // Reading through the link reaches the workspace.
        assert_eq!(
            std::fs::read_to_string(expected.join("MARKER")).unwrap(),
            "link-content"
        );
    }

    /// Live-edit guarantee: a file written into the workspace *after* the
    /// link install becomes visible through the install_path. This is the
    /// whole reason the feature exists.
    #[tokio::test]
    async fn dev_link_install_reflects_live_workspace_edits() {
        let temp_dir = TempDir::new().unwrap();
        let storage_config = StorageConfig {
            home_dir: temp_dir.path().to_path_buf(),
            packages_dir: temp_dir.path().join("packages"),
            cache_dir: temp_dir.path().join("cache"),
            registry_cache_dir: temp_dir.path().join("registry"),
        };
        let storage_manager = StorageManager::new(storage_config).unwrap();

        let source = temp_dir.path().join("live-source");
        write_source_package(&source, "creator/foo", "1.0.0", "initial");
        let installed = storage_manager
            .install_from_path_dev_link(&source)
            .await
            .unwrap();

        // Simulate a working-tree edit after install.
        std::fs::write(source.join("new_file.txt"), "edited-after-install").unwrap();

        assert_eq!(
            std::fs::read_to_string(installed.install_path.join("new_file.txt")).unwrap(),
            "edited-after-install"
        );
    }

    /// Repeated link-installs must replace the link entry without nuking the
    /// workspace. This is the safety property the symlink-aware removal
    /// branch in `clear_existing_install` enforces — without it,
    /// `remove_dir_all` on a Windows junction would recursively delete the
    /// user's source tree.
    #[tokio::test]
    async fn repeated_dev_link_install_preserves_workspace() {
        let temp_dir = TempDir::new().unwrap();
        let storage_config = StorageConfig {
            home_dir: temp_dir.path().to_path_buf(),
            packages_dir: temp_dir.path().join("packages"),
            cache_dir: temp_dir.path().join("cache"),
            registry_cache_dir: temp_dir.path().join("registry"),
        };
        let storage_manager = StorageManager::new(storage_config).unwrap();

        let source = temp_dir.path().join("workspace");
        write_source_package(&source, "creator/foo", "1.0.0", "workspace-marker");
        std::fs::write(source.join("user-script.py"), "# user authored").unwrap();

        storage_manager
            .install_from_path_dev_link(&source)
            .await
            .unwrap();
        storage_manager
            .install_from_path_dev_link(&source)
            .await
            .unwrap();

        // Workspace files must survive — both the marker and the user file.
        assert_eq!(
            std::fs::read_to_string(source.join("MARKER")).unwrap(),
            "workspace-marker"
        );
        assert_eq!(
            std::fs::read_to_string(source.join("user-script.py")).unwrap(),
            "# user authored"
        );
        assert!(source.join("hpm.toml").exists());
    }

    /// Switching install styles at the same coordinate is allowed and must
    /// not delete the workspace: a copy install replaced by a link install
    /// (or vice versa) goes through `clear_existing_install`, which uses
    /// `remove_dir_all` for real dirs and link-safe removal for links.
    #[tokio::test]
    async fn switching_copy_to_link_does_not_touch_workspace() {
        let temp_dir = TempDir::new().unwrap();
        let storage_config = StorageConfig {
            home_dir: temp_dir.path().to_path_buf(),
            packages_dir: temp_dir.path().join("packages"),
            cache_dir: temp_dir.path().join("cache"),
            registry_cache_dir: temp_dir.path().join("registry"),
        };
        let storage_manager = StorageManager::new(storage_config).unwrap();

        let source = temp_dir.path().join("workspace");
        write_source_package(&source, "creator/foo", "1.0.0", "ws");

        // First: copy install lays down a real directory at the dev path.
        storage_manager
            .install_from_path_dev(&source)
            .await
            .unwrap();

        // Then: link install must replace the real dir without traversing
        // into the workspace.
        storage_manager
            .install_from_path_dev_link(&source)
            .await
            .unwrap();

        assert_eq!(
            std::fs::read_to_string(source.join("MARKER")).unwrap(),
            "ws"
        );

        // And the reverse: link → copy.
        storage_manager
            .install_from_path_dev(&source)
            .await
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(source.join("MARKER")).unwrap(),
            "ws"
        );
    }

    /// Link install must respect the same `_dev/` namespace isolation as
    /// copy-install. A registry install at the same coordinate is unaffected.
    #[tokio::test]
    async fn dev_link_and_registry_installs_coexist_at_same_coordinate() {
        let temp_dir = TempDir::new().unwrap();
        let storage_config = StorageConfig {
            home_dir: temp_dir.path().to_path_buf(),
            packages_dir: temp_dir.path().join("packages"),
            cache_dir: temp_dir.path().join("cache"),
            registry_cache_dir: temp_dir.path().join("registry"),
        };
        let storage_manager = StorageManager::new(storage_config).unwrap();

        let link_source = temp_dir.path().join("link-source");
        write_source_package(&link_source, "creator/foo", "1.0.0", "link-content");
        storage_manager
            .install_from_path_dev_link(&link_source)
            .await
            .unwrap();

        let reg_source = temp_dir.path().join("reg-source");
        write_source_package(&reg_source, "creator/foo", "1.0.0", "registry-content");
        storage_manager
            .install_from_path(&reg_source)
            .await
            .unwrap();

        let link_marker = temp_dir
            .path()
            .join("packages")
            .join("_dev")
            .join("foo@1.0.0")
            .join("MARKER");
        let reg_marker = temp_dir
            .path()
            .join("packages")
            .join("foo@1.0.0")
            .join("MARKER");
        assert_eq!(
            std::fs::read_to_string(&link_marker).unwrap(),
            "link-content"
        );
        assert_eq!(
            std::fs::read_to_string(&reg_marker).unwrap(),
            "registry-content"
        );

        // And `list_installed` still ignores the dev subtree, even when the
        // entry is a link rather than a real directory.
        let listed = storage_manager.list_installed().unwrap();
        let names: Vec<&str> = listed.iter().map(|p| p.manifest.package.slug()).collect();
        assert_eq!(
            names,
            vec!["foo"],
            "only the registry install should surface"
        );
    }

    // ------------------------------------------------------------------
    // Dev (path-dep) install cleanup
    // ------------------------------------------------------------------

    /// Build a project hpm.toml at `dir` that depends on a single path dep
    /// pointing at `dep_path` (relative or absolute string written verbatim).
    fn write_project_with_path_dep(
        dir: &std::path::Path,
        project_slug: &str,
        version: &str,
        dep_name: &str,
        dep_path: &str,
        link: bool,
    ) {
        std::fs::create_dir_all(dir).unwrap();
        let link_field = if link { ", link = true" } else { "" };
        std::fs::write(
            dir.join("hpm.toml"),
            format!(
                "[package]\n\
                 path = \"studio/{project_slug}\"\n\
                 name = \"{project_slug}\"\n\
                 version = \"{version}\"\n\
                 \n\
                 [dependencies]\n\
                 \"{dep_name}\" = {{ path = \"{dep_path}\"{link_field} }}\n",
            ),
        )
        .unwrap();
    }

    fn projects_config_with(paths: Vec<std::path::PathBuf>) -> ProjectsConfig {
        ProjectsConfig {
            explicit_paths: paths,
            search_roots: vec![],
            max_search_depth: 0,
            ignore_patterns: vec![],
        }
    }

    /// A dev install that no project's path-dep claims must be classified
    /// as orphan and removed.
    #[tokio::test]
    async fn unreferenced_dev_install_is_orphan() {
        let temp_dir = TempDir::new().unwrap();
        let storage_config = StorageConfig {
            home_dir: temp_dir.path().to_path_buf(),
            packages_dir: temp_dir.path().join("packages"),
            cache_dir: temp_dir.path().join("cache"),
            registry_cache_dir: temp_dir.path().join("registry"),
        };
        let storage_manager = StorageManager::new(storage_config).unwrap();

        // Plant a dev install for `studio/orphan@1.0.0`.
        let source = temp_dir.path().join("orphan-source");
        write_source_package(&source, "studio/orphan", "1.0.0", "orphan-marker");
        storage_manager
            .install_from_path_dev(&source)
            .await
            .unwrap();
        let dev_path = temp_dir
            .path()
            .join("packages")
            .join("_dev")
            .join("orphan@1.0.0");
        assert!(dev_path.exists());

        // A project exists but doesn't reference this dep.
        let project_dir = temp_dir.path().join("project");
        write_project_with_path_dep(
            &project_dir,
            "consumer",
            "1.0.0",
            "studio/something-else",
            "../something-else-src",
            false,
        );
        // The "something-else" source doesn't exist — the dep is broken, but
        // that's irrelevant to this test (the dep would also fail to claim
        // the `orphan` dev install regardless).

        let projects_cfg = projects_config_with(vec![project_dir]);
        let removed = storage_manager
            .cleanup_unused_dev_installs(&projects_cfg)
            .await
            .unwrap();

        assert_eq!(removed, vec!["_dev/orphan@1.0.0"]);
        assert!(!dev_path.exists());
        // Source workspace is untouched.
        assert!(source.join("MARKER").exists());
    }

    /// A dev install that any project's path-dep manifest resolves to must
    /// be protected from cleanup.
    #[tokio::test]
    async fn referenced_dev_install_survives_cleanup() {
        let temp_dir = TempDir::new().unwrap();
        let storage_config = StorageConfig {
            home_dir: temp_dir.path().to_path_buf(),
            packages_dir: temp_dir.path().join("packages"),
            cache_dir: temp_dir.path().join("cache"),
            registry_cache_dir: temp_dir.path().join("registry"),
        };
        let storage_manager = StorageManager::new(storage_config).unwrap();

        // Plant a dev install for `studio/keep@2.0.0`.
        let source = temp_dir.path().join("keep-source");
        write_source_package(&source, "studio/keep", "2.0.0", "keep-marker");
        storage_manager
            .install_from_path_dev(&source)
            .await
            .unwrap();
        let dev_path = temp_dir
            .path()
            .join("packages")
            .join("_dev")
            .join("keep@2.0.0");
        assert!(dev_path.exists());

        // Project references the same source workspace via path dep.
        let project_dir = temp_dir.path().join("project");
        write_project_with_path_dep(
            &project_dir,
            "consumer",
            "1.0.0",
            "studio/keep",
            "../keep-source",
            false,
        );

        let projects_cfg = projects_config_with(vec![project_dir]);
        let removed = storage_manager
            .cleanup_unused_dev_installs(&projects_cfg)
            .await
            .unwrap();

        assert!(removed.is_empty(), "no orphans expected, got {removed:?}");
        assert!(dev_path.exists(), "referenced dev install must survive");
    }

    /// Cleaning up a link-mode dev install must remove the link entry only,
    /// never follow it into the workspace.
    #[tokio::test]
    async fn orphan_link_install_cleanup_preserves_source() {
        let temp_dir = TempDir::new().unwrap();
        let storage_config = StorageConfig {
            home_dir: temp_dir.path().to_path_buf(),
            packages_dir: temp_dir.path().join("packages"),
            cache_dir: temp_dir.path().join("cache"),
            registry_cache_dir: temp_dir.path().join("registry"),
        };
        let storage_manager = StorageManager::new(storage_config).unwrap();

        // Link install with no referencing project.
        let workspace = temp_dir.path().join("workspace");
        write_source_package(&workspace, "studio/linked", "1.0.0", "workspace-marker");
        std::fs::write(workspace.join("user-file.py"), "# user authored").unwrap();
        storage_manager
            .install_from_path_dev_link(&workspace)
            .await
            .unwrap();
        let dev_path = temp_dir
            .path()
            .join("packages")
            .join("_dev")
            .join("linked@1.0.0");

        // A different project exists, doesn't claim `linked`.
        let project_dir = temp_dir.path().join("project");
        write_project_with_path_dep(
            &project_dir,
            "consumer",
            "1.0.0",
            "studio/other",
            "../other",
            false,
        );

        let projects_cfg = projects_config_with(vec![project_dir]);
        let removed = storage_manager
            .cleanup_unused_dev_installs(&projects_cfg)
            .await
            .unwrap();

        assert_eq!(removed, vec!["_dev/linked@1.0.0"]);
        assert!(std::fs::symlink_metadata(&dev_path).is_err());
        // Workspace files survive — the link unlinked, the workspace did not.
        assert_eq!(
            std::fs::read_to_string(workspace.join("MARKER")).unwrap(),
            "workspace-marker"
        );
        assert_eq!(
            std::fs::read_to_string(workspace.join("user-file.py")).unwrap(),
            "# user authored"
        );
    }

    /// A project with an unresolvable path-dep source (e.g. workspace moved
    /// or deleted) must not bypass cleanup of other dev installs. We log a
    /// warning for the broken dep and continue.
    #[tokio::test]
    async fn unresolvable_path_dep_does_not_block_cleanup() {
        let temp_dir = TempDir::new().unwrap();
        let storage_config = StorageConfig {
            home_dir: temp_dir.path().to_path_buf(),
            packages_dir: temp_dir.path().join("packages"),
            cache_dir: temp_dir.path().join("cache"),
            registry_cache_dir: temp_dir.path().join("registry"),
        };
        let storage_manager = StorageManager::new(storage_config).unwrap();

        // Plant two dev installs.
        let alpha_src = temp_dir.path().join("alpha-src");
        write_source_package(&alpha_src, "studio/alpha", "1.0.0", "alpha");
        storage_manager
            .install_from_path_dev(&alpha_src)
            .await
            .unwrap();
        let beta_src = temp_dir.path().join("beta-src");
        write_source_package(&beta_src, "studio/beta", "1.0.0", "beta");
        storage_manager
            .install_from_path_dev(&beta_src)
            .await
            .unwrap();

        // Project references `alpha` correctly, but `beta`'s path points at
        // a directory that doesn't have an hpm.toml — `from_path` errors.
        let project_dir = temp_dir.path().join("project");
        std::fs::create_dir_all(&project_dir).unwrap();
        std::fs::write(
            project_dir.join("hpm.toml"),
            "[package]\n\
             path = \"studio/consumer\"\n\
             name = \"consumer\"\n\
             version = \"1.0.0\"\n\
             \n\
             [dependencies]\n\
             \"studio/alpha\" = { path = \"../alpha-src\" }\n\
             \"studio/beta\" = { path = \"../does-not-exist\" }\n",
        )
        .unwrap();

        let projects_cfg = projects_config_with(vec![project_dir]);
        let removed = storage_manager
            .cleanup_unused_dev_installs(&projects_cfg)
            .await
            .unwrap();

        // `alpha` is referenced → survives.
        // `beta`'s referencing dep is unresolvable → `beta` looks orphaned
        // (correct: a broken dep cannot protect anything from cleanup).
        assert_eq!(removed, vec!["_dev/beta@1.0.0"]);
        let alpha_path = temp_dir
            .path()
            .join("packages")
            .join("_dev")
            .join("alpha@1.0.0");
        let beta_path = temp_dir
            .path()
            .join("packages")
            .join("_dev")
            .join("beta@1.0.0");
        assert!(alpha_path.exists());
        assert!(!beta_path.exists());
    }

    /// Safety guard: an empty projects list means we can't tell whether any
    /// dev install is needed, so we must not remove anything. Matches the
    /// existing CAS-cleanup behavior.
    #[tokio::test]
    async fn no_projects_skips_dev_cleanup() {
        let temp_dir = TempDir::new().unwrap();
        let storage_config = StorageConfig {
            home_dir: temp_dir.path().to_path_buf(),
            packages_dir: temp_dir.path().join("packages"),
            cache_dir: temp_dir.path().join("cache"),
            registry_cache_dir: temp_dir.path().join("registry"),
        };
        let storage_manager = StorageManager::new(storage_config).unwrap();

        let source = temp_dir.path().join("orphan-src");
        write_source_package(&source, "studio/orphan", "1.0.0", "orphan");
        storage_manager
            .install_from_path_dev(&source)
            .await
            .unwrap();
        let dev_path = temp_dir
            .path()
            .join("packages")
            .join("_dev")
            .join("orphan@1.0.0");

        let projects_cfg = projects_config_with(vec![]);
        let removed = storage_manager
            .cleanup_unused_dev_installs(&projects_cfg)
            .await
            .unwrap();

        assert!(removed.is_empty());
        assert!(dev_path.exists(), "no projects → no cleanup");
    }

    /// `cleanup_comprehensive` carries dev orphans through to the result.
    #[tokio::test]
    async fn cleanup_comprehensive_reports_dev_orphans() {
        let temp_dir = TempDir::new().unwrap();
        let storage_config = StorageConfig {
            home_dir: temp_dir.path().to_path_buf(),
            packages_dir: temp_dir.path().join("packages"),
            cache_dir: temp_dir.path().join("cache"),
            registry_cache_dir: temp_dir.path().join("registry"),
        };
        let storage_manager = StorageManager::new(storage_config).unwrap();

        let orphan_src = temp_dir.path().join("orphan-src");
        write_source_package(&orphan_src, "studio/orphan", "1.0.0", "orphan");
        storage_manager
            .install_from_path_dev(&orphan_src)
            .await
            .unwrap();

        // Non-empty project list, none claiming `orphan`.
        let project_dir = temp_dir.path().join("project");
        write_project_with_path_dep(
            &project_dir,
            "consumer",
            "1.0.0",
            "studio/anything",
            "../anything",
            false,
        );
        let projects_cfg = projects_config_with(vec![project_dir]);

        // Dry-run first — nothing removed but the report is populated.
        let dry = storage_manager
            .cleanup_comprehensive(&projects_cfg, true)
            .await
            .unwrap();
        assert_eq!(dry.removed_dev_installs, vec!["_dev/orphan@1.0.0"]);
        let dev_path = temp_dir
            .path()
            .join("packages")
            .join("_dev")
            .join("orphan@1.0.0");
        assert!(dev_path.exists(), "dry-run must not delete");

        // Real run — the dev install is gone.
        let real = storage_manager
            .cleanup_comprehensive(&projects_cfg, false)
            .await
            .unwrap();
        assert_eq!(real.removed_dev_installs, vec!["_dev/orphan@1.0.0"]);
        assert!(!dev_path.exists());
    }
}
