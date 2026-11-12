//! Installing a project's dependencies: the parallel sync, one dependency's
//! install, and the reclaim of dev copies a sync supersedes.

use crate::archive_fetcher::ArchiveFetcher;
use crate::lock::LockedSource;
use crate::package_source::PackageSource;
use crate::storage::{InstalledPackage, StorageManager};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::task::JoinSet;
use tracing::info;

use super::{InstallOutcome, ProjectError, ProjectManager};

impl ProjectManager {
    pub async fn sync_dependencies(&self) -> Result<Vec<(String, InstallOutcome)>, ProjectError> {
        info!("Syncing project dependencies");

        let project_manifest = match self.load_project_manifest()? {
            Some(manifest) => manifest,
            None => {
                info!("No project manifest found, nothing to sync");
                return Ok(Vec::new());
            }
        };

        let project_houdini = project_manifest.compat.houdini_min();
        let project_env_overrides = project_manifest.runtime;
        let manifest_registries = project_manifest.registries;
        let dependencies = project_manifest.dependencies;

        // Build the project's registry set once for any registry-resolved
        // deps. Wrapped in Arc so each spawned task can hold a cheap clone.
        let registry_set: Option<Arc<crate::registry::RegistrySet>> =
            if dependencies.values().any(|spec| spec.is_registry()) {
                let set = crate::registry::RegistrySet::for_project(
                    &manifest_registries,
                    &self.config,
                    self.auth_token.as_deref(),
                )
                .map_err(|e| ProjectError::RegistryConfiguration(Box::new(e)))?;
                Some(Arc::new(set))
            } else {
                None
            };

        // Fetch list of globally installed packages once for short-circuit lookups
        let all_installed = Arc::new(self.storage_manager.list_installed()?);

        // Phase 1: spawn all installs in parallel via a JoinSet. Each task owns
        // a clone of (StorageManager, ArchiveFetcher, RegistrySet) and its dep
        // spec, so resolve→fetch→copy-into-CAS chains overlap across deps.
        let mut tasks: JoinSet<(String, Result<InstallOutcome, ProjectError>)> = JoinSet::new();
        for (name, spec) in dependencies {
            let storage = self.storage_manager.clone();
            let fetcher = self.fetcher.clone();
            let registry_set = registry_set.clone();
            let all_installed = all_installed.clone();

            tasks.spawn(async move {
                let result = install_one_dep(
                    &storage,
                    &fetcher,
                    registry_set.as_deref(),
                    &all_installed,
                    &name,
                    &spec,
                )
                .await;
                (name, result)
            });
        }

        let mut outcomes: Vec<(String, InstallOutcome)> = Vec::new();
        while let Some(joined) = tasks.join_next().await {
            // A spawned-task panic leaks structural confusion; let the runtime
            // surface it rather than synthesising a typed ProjectError.
            let (name, result) = joined.expect("dependency install task panicked");
            outcomes.push((name, result?));
        }

        // Snapshot the installed-package list once for downstream steps; the
        // outcomes hang onto richer metadata for the lockfile, but Python
        // resolution / manifest emission only need the InstalledPackage.
        let installed: Vec<InstalledPackage> =
            outcomes.iter().map(|(_, o)| o.package.clone()).collect();

        // The venv for the installed packages' Python dependencies, if any.
        let manifests: Vec<hpm_package::PackageManifest> =
            installed.iter().map(|p| p.manifest.clone()).collect();
        let venv_site_packages =
            super::python_env::ensure_venv(project_houdini.as_deref(), &manifests, &installed, &[])
                .await?
                .map(|venv| venv.site_packages);

        // Generate Houdini JSON manifests for all packages, plus the
        // project overrides manifest that Houdini processes after them.
        for pkg in &installed {
            self.generate_houdini_manifest_with_python(
                pkg,
                venv_site_packages.as_deref(),
                &project_env_overrides,
            )?;
        }
        self.write_project_overrides_manifest(&project_env_overrides)?;

        // Sweep stale per-package manifests left over from previous syncs.
        // Houdini reads every .json file in `packages_dir` on launch, so a
        // manifest whose slug has dropped out of the dependency set (dev override
        // removed, registry yank, manual edit) keeps loading the package even
        // though hpm.toml no longer asks for it.
        self.sweep_stale_houdini_manifests(&installed)?;

        reclaim_superseded_dev_copies(&installed);

        info!("Successfully synced project dependencies");
        Ok(outcomes)
    }
}

/// Hand back a tree that is already in the CAS, sweeping its executable bits
/// first.
///
/// This is the other half of the extraction-time repair in
/// [`crate::exec_mode`]. Every reuse path below returns without re-extracting,
/// which is exactly why an old install never healed itself: a package whose
/// programs lost their executable bit in the archive stayed unrunnable through
/// every later `install`, `sync`, and hpm upgrade, because the only code that
/// could fix it ran at extraction and extraction never happened again.
///
/// Sweeping here rather than at spawn time keeps it content-driven — it covers
/// the helper binary a `[scripts]` entry shells out to, not just the declared
/// entry point — and [`ensure_repaired`] stamps the tree, so the walk costs one
/// pass per installed version rather than one per sync.
///
/// [`ensure_repaired`]: crate::exec_mode::ensure_repaired
async fn reuse_installed(pkg: &InstalledPackage) {
    let dir = pkg.install_path.clone();
    // A filesystem walk on the runtime's worker would stall the other installs
    // sharing it; this is the same reason extraction runs on the blocking pool.
    // A panic in the repair is a bug: re-raise it rather than drop it.
    if let Err(e) =
        tokio::task::spawn_blocking(move || crate::exec_mode::ensure_repaired(&dir)).await
    {
        std::panic::resume_unwind(e.into_panic());
    }
}

/// Install a single dependency, short-circuiting if it's already in the
/// global CAS. Spawnable from the JoinSet in `sync_dependencies` — takes
/// shared state by `&` (cloned into the task by the caller).
///
/// `all_installed` is the snapshot of `StorageManager::list_installed()`
/// captured before installs began; comparing against it avoids re-fetching
/// packages that another concurrent task may also be racing to install
/// (the CAS is idempotent under `install_into_cas`, but skipping the
/// network round-trip and the remove-and-recopy that `install_into_cas`
/// performs is worth the shared snapshot — that recopy is the
/// well-known Windows `os error 5` trigger when Houdini holds files open).
///
/// Returns `InstallOutcome` with `checksum` / `source` populated only when
/// they're known: fresh fetches get both, `Url`-spec short-circuits get the
/// URL only, `Registry` short-circuits get neither (the lockfile builder
/// can backfill those from the prior lockfile).
pub(super) async fn install_one_dep(
    storage: &StorageManager,
    fetcher: &ArchiveFetcher,
    registry_set: Option<&crate::registry::RegistrySet>,
    all_installed: &[InstalledPackage],
    name: &str,
    spec: &hpm_package::DependencySpec,
) -> Result<InstallOutcome, ProjectError> {
    use hpm_package::DependencySpec;
    match spec {
        DependencySpec::Registry {
            version, registry, ..
        } => {
            if let Some(pkg) = all_installed
                .iter()
                .find(|p| ProjectManager::matches_spec_name(p, name) && p.version == *version)
            {
                info!("Package {}@{} already installed", name, version);
                reuse_installed(pkg).await;
                return Ok(InstallOutcome {
                    package: pkg.clone(),
                    checksum: None,
                    source: None,
                });
            }
            let rs = registry_set.expect("registry set built when registry deps present");
            // A registry-resolved dep with no registries configured is its own
            // failure, distinct from "package not found": resolving against an
            // empty set would otherwise surface a misleading VersionNotFound.
            // Point the user at `hpm registry add`.
            if rs.is_empty() {
                return Err(ProjectError::NoRegistriesConfigured {
                    name: name.to_string(),
                    version_req: version.clone(),
                });
            }
            let entry = rs
                .get_version_in(name, version, registry.as_deref())
                .await
                .map_err(|source| ProjectError::RegistryResolution {
                    name: name.to_string(),
                    version_req: version.clone(),
                    source: Box::new(source),
                })?;
            let url = entry.dl.clone();
            let source = PackageSource::url(url.clone(), version)?
                .with_registry_checksum(entry.cksum.as_deref())?;
            let (package, checksum) =
                fetch_and_install_pkg(storage, fetcher, name, version, source).await?;
            Ok(InstallOutcome {
                package,
                checksum: Some(checksum),
                source: Some(LockedSource::url(url, version.clone())),
            })
        }
        DependencySpec::Url { url, version, .. } => {
            if let Some(pkg) = all_installed
                .iter()
                .find(|p| ProjectManager::matches_spec_name(p, name) && p.version == *version)
            {
                info!("Package {}@{} already installed", name, version);
                reuse_installed(pkg).await;
                return Ok(InstallOutcome {
                    package: pkg.clone(),
                    checksum: None,
                    source: Some(LockedSource::url(url.clone(), version.clone())),
                });
            }
            let source = PackageSource::url(url, version)?;
            let (package, checksum) =
                fetch_and_install_pkg(storage, fetcher, name, version, source).await?;
            Ok(InstallOutcome {
                package,
                checksum: Some(checksum),
                source: Some(LockedSource::url(url.clone(), version.clone())),
            })
        }
        DependencySpec::Path { path, link, .. } => {
            // Unlike the Registry/Url arms there's no `all_installed`
            // skip here: a path dep's workspace can change between syncs, so we
            // always re-enter the installer. `install_inner` content-addresses
            // dev copies (`_dev/<slug>@<version>/<source-hash>/`), so an
            // unchanged workspace resolves to the same hash directory and is
            // reused untouched, while a rebuild lands in a fresh directory —
            // never removing a copy a running Houdini may have mapped.
            let source_path = std::path::Path::new(path);
            let package = if *link {
                storage.install_as_dev_link(source_path).await?
            } else {
                storage.install_as_dev_copy(source_path).await?
            };
            Ok(InstallOutcome {
                package,
                checksum: None,
                source: Some(LockedSource::path(source_path)),
            })
        }
    }
}

/// Fetch a remote package and copy it into the global CAS. Returns the
/// installed package alongside its tree hash (the hpm.lock checksum).
///
/// `pub(crate)` so `hpm global` installs go through the identical
/// fetch-and-CAS path as project installs rather than reimplementing it.
pub(crate) async fn fetch_and_install_pkg(
    storage: &StorageManager,
    fetcher: &ArchiveFetcher,
    name: &str,
    version: &str,
    source: PackageSource,
) -> Result<(InstalledPackage, String), ProjectError> {
    let (installed, checksum) = fetcher
        .fetch_into_store::<ProjectError>(storage, &source, name)
        .await?;
    info!("Successfully fetched and installed {}@{}", name, version);
    Ok((installed, checksum))
}

/// Reclaim the superseded copies in the dev containers this sync installed
/// into, now that this project's manifests point at the current ones. Each
/// dev launch after a rebuild used to add a full copy that nothing removed.
///
/// Off the sync, on a blocking thread nobody waits for: the first reclaim on
/// a machine that collected hundreds of copies deletes hundreds of gigabytes,
/// and no caller should wait on that. A deletion cut short leaves a hidden
/// tombstone that the next reclaim retries.
fn reclaim_superseded_dev_copies(installed: &[InstalledPackage]) {
    let containers: std::collections::BTreeSet<PathBuf> = installed
        .iter()
        .filter(|p| p.is_dev)
        .filter_map(|p| {
            crate::storage::dev_users::container_of(&p.install_path).map(Path::to_path_buf)
        })
        .collect();
    if containers.is_empty() {
        return;
    }
    let Ok(runtime) = tokio::runtime::Handle::try_current() else {
        return;
    };
    runtime.spawn_blocking(move || {
        use crate::storage::dev_users::{
            Unrecorded, reclaim_dev_copies, remove_empty_container, sibling_containers,
        };
        let state_of = crate::process_users::process_state;
        for container in &containers {
            reclaim_dev_copies(container, None, Unrecorded::Keep, state_of);
            // The same package's other versions: a project that moved to a
            // new version leaves the old container's copies unused, and no
            // sync installs into it again. The users rule decides there too,
            // so a version another project still points at keeps its copy.
            for sibling in sibling_containers(container) {
                if containers.contains(&sibling) {
                    continue;
                }
                reclaim_dev_copies(&sibling, None, Unrecorded::Keep, state_of);
                remove_empty_container(&sibling);
            }
        }
    });
}
