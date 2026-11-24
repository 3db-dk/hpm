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

/// How a sync treats `hpm.lock` ([`ProjectManager::sync_dependencies_with`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SyncOptions {
    /// Every registry and URL package the sync installs, listed or
    /// required, must be pinned by the lock at the version installed, with
    /// a checksum, or the sync fails before installing it. The versions the
    /// lock recorded are kept; a set that no longer fits them is an error.
    pub frozen: bool,
    /// Choose every package the project does not pin anew, the newest
    /// version that fits, instead of keeping the version the lock recorded
    /// while it still fits. What `hpm update` installs with.
    pub refresh: bool,
}

impl ProjectManager {
    /// Install the project's dependencies, reproducing `hpm.lock` when the
    /// project has one: every package whose locked version is the one the
    /// manifest asks for must match its locked checksum, whether it is
    /// fetched or reused from the store. A lock that exists but cannot be
    /// read is an error.
    pub async fn sync_dependencies(&self) -> Result<Vec<(String, InstallOutcome)>, ProjectError> {
        let lock_path = &self.project_paths.lock_file;
        let lock = if lock_path.exists() {
            Some(crate::lock::LockFile::load(lock_path).map_err(|source| {
                ProjectError::LockUnreadable {
                    lock: lock_path.clone(),
                    source: Box::new(source),
                }
            })?)
        } else {
            None
        };
        self.sync_dependencies_with_lock(lock.as_ref(), false).await
    }

    /// [`Self::sync_dependencies`] against the given lock. With `frozen`,
    /// every registry and URL package must be pinned by the lock at the
    /// version installed with a checksum, or the sync fails before it is
    /// installed. [`Self::sync_dependencies_with`] with the lock's versions
    /// kept.
    pub async fn sync_dependencies_with_lock(
        &self,
        lock: Option<&crate::lock::LockFile>,
        frozen: bool,
    ) -> Result<Vec<(String, InstallOutcome)>, ProjectError> {
        self.sync_dependencies_with(
            lock,
            SyncOptions {
                frozen,
                refresh: false,
            },
        )
        .await
    }

    /// Install the project's dependencies and every package their versions
    /// require that the project does not list, directly or through each
    /// other ([`crate::registry::closure::select_closed`], the choice an
    /// application that embeds hpm makes too).
    ///
    /// A registry dependency written as a full version is pinned there; a
    /// range takes the newest version in it that fits. A path or URL
    /// dependency takes part at the version its `hpm.toml` declares,
    /// requiring what that file's registry dependencies require. Versions
    /// are held to the lower bound of the project's `[compat].houdini`, and
    /// requirements between packages must all hold, or the sync fails
    /// before installing anything from a registry.
    ///
    /// Unless `options.refresh`, a package the project does not pin keeps
    /// the version `lock` recorded for it while the set still fits that
    /// version; when it does not, every such package is chosen anew.
    pub async fn sync_dependencies_with(
        &self,
        lock: Option<&crate::lock::LockFile>,
        options: SyncOptions,
    ) -> Result<Vec<(String, InstallOutcome)>, ProjectError> {
        use crate::registry::closure::{self, Candidates};
        use hpm_package::DependencySpec;

        info!("Syncing project dependencies");
        // Re-checked on every sync: a long-lived manager (an embedder) must
        // not write into a `.hpm` replaced by a link since it was built.
        self.ensure_directories()?;

        let project_manifest = match self.load_project_manifest()? {
            Some(manifest) => manifest,
            None => {
                info!("No project manifest found, nothing to sync");
                return Ok(Vec::new());
            }
        };

        let project_houdini = project_manifest.compat.houdini_min();
        let houdini = project_houdini.as_deref().and_then(houdini_target);
        let project_env_overrides = project_manifest.runtime;
        let manifest_registries = project_manifest.registries;
        let dependencies = project_manifest.dependencies;

        // The lock entry for `name` at `version`, if the lock records that
        // very version. One for another version is stale and pins nothing.
        let locked_at = |name: &str, version: &str| -> Option<String> {
            lock.and_then(|l| l.get_dependency(name))
                .filter(|e| e.version == version)
                .and_then(|e| e.checksum.clone())
        };
        let frozen_check = |name: &str, version: &str| -> Result<(), ProjectError> {
            if options.frozen && locked_at(name, version).is_none() {
                return Err(ProjectError::FrozenLockIncomplete {
                    name: name.to_string(),
                });
            }
            Ok(())
        };

        // Fetch list of globally installed packages once for short-circuit lookups
        let all_installed = Arc::new(self.storage_manager.list_installed()?);
        let stored = |name: &str, version: &str| -> Option<&InstalledPackage> {
            all_installed
                .iter()
                .find(|p| ProjectManager::matches_spec_name(p, name) && p.version == version)
        };

        // URL dependencies first: their version is fixed, and what they
        // require is in the `hpm.toml` the archive carries.
        let mut outcomes: Vec<(String, InstallOutcome)> = Vec::new();
        for (name, spec) in &dependencies {
            if let DependencySpec::Url { version, .. } = spec {
                frozen_check(name, version)?;
            }
        }
        let url_deps: Vec<(String, DependencySpec)> = dependencies
            .iter()
            .filter(|(_, spec)| matches!(spec, DependencySpec::Url { .. }))
            .map(|(n, s)| (n.clone(), s.clone()))
            .collect();
        outcomes.extend(
            self.install_all(url_deps, None, &all_installed, &locked_at)
                .await?,
        );

        // The project's packages as the solver sees them: registry
        // dependencies in manifest order, then the packages on disk or
        // already fetched (path and URL dependencies) at their declared
        // version.
        let mut on_disk: Vec<Candidates> = Vec::new();
        for (name, spec) in &dependencies {
            match spec {
                DependencySpec::Path { path, .. } => {
                    let manifest = hpm_package::PackageManifest::from_path(
                        &std::path::Path::new(path).join("hpm.toml"),
                    )?;
                    on_disk.push(closure::working_copy(name, &manifest));
                }
                DependencySpec::Url { .. } => {
                    if let Some((_, o)) = outcomes.iter().find(|(n, _)| n == name) {
                        on_disk.push(Candidates {
                            on_disk: false,
                            ..closure::working_copy(name, &o.package.manifest)
                        });
                    }
                }
                DependencySpec::Registry { .. } => {}
            }
        }

        let registry_deps: Vec<(&String, &String, Option<&str>)> = dependencies
            .iter()
            .filter_map(|(name, spec)| match spec {
                DependencySpec::Registry {
                    version, registry, ..
                } => Some((name, version, registry.as_deref())),
                _ => None,
            })
            .collect();
        let anything_required = on_disk
            .iter()
            .any(|c| c.versions.iter().any(|e| !e.deps.is_empty()));
        let registry_set: Option<Arc<crate::registry::RegistrySet>> =
            if !registry_deps.is_empty() || anything_required {
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

        // Each registry dependency, fresh (as the manifest writes it) and
        // with the lock's version kept (a range pinned at the locked version
        // while that is in it).
        let mut listed_fresh: Vec<Candidates> = Vec::new();
        let mut listed_locked: Vec<Candidates> = Vec::new();
        for (name, version, registry) in &registry_deps {
            let requirement = crate::registry::requirement::VersionRequirement::parse(version)
                .map_err(|_| ProjectError::RegistryResolution {
                    name: name.to_string(),
                    version_req: version.to_string(),
                    source: Box::new(crate::registry::RegistryError::ParseError(format!(
                        "{version:?} is not a version or a version range"
                    ))),
                })?;
            if matches!(
                requirement,
                crate::registry::requirement::VersionRequirement::Exact(_)
            ) {
                // A pin is selected as written; only its requirements are
                // needed, from the store copy when there is one, so an
                // installed project resolves without the network. A pin the
                // registry cannot give is still selected, and installing it
                // reports why.
                let entry = match stored(name, version) {
                    Some(pkg) => Some(closure::entry_of(name, &pkg.manifest)),
                    None => match registry_set.as_deref() {
                        Some(set) if !set.is_empty() => {
                            set.get_version_in(name, version, *registry).await.ok()
                        }
                        _ => None,
                    },
                };
                let pinned = Candidates {
                    name: name.to_string(),
                    pin: Some(version.to_string()),
                    versions: entry.into_iter().collect(),
                    on_disk: false,
                };
                listed_fresh.push(pinned.clone());
                listed_locked.push(pinned);
                continue;
            }
            let set = registry_set
                .as_deref()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| ProjectError::NoRegistriesConfigured {
                    name: name.to_string(),
                    version_req: version.to_string(),
                })?;
            let versions: Vec<crate::registry::RegistryEntry> = set
                .get_versions_in(name, *registry)
                .await
                .map_err(|source| ProjectError::RegistryResolution {
                    name: name.to_string(),
                    version_req: version.to_string(),
                    source: Box::new(source),
                })?
                .into_iter()
                .filter(|e| {
                    semver::Version::parse(&e.version).is_ok_and(|v| requirement.matches(&v))
                })
                .collect();
            let locked_version = lock
                .and_then(|l| l.get_dependency(name))
                .map(|e| e.version.clone())
                .filter(|v| versions.iter().any(|e| &e.version == v));
            listed_locked.push(Candidates {
                name: name.to_string(),
                pin: locked_version,
                versions: versions.clone(),
                on_disk: false,
            });
            listed_fresh.push(Candidates {
                name: name.to_string(),
                pin: None,
                versions,
                on_disk: false,
            });
        }
        listed_fresh.extend(on_disk.iter().cloned());
        listed_locked.extend(on_disk);

        // The packages the lock recorded that the project does not list,
        // offered at the locked version: each is used exactly when the set
        // still requires it.
        let mut supplied: Vec<Candidates> = Vec::new();
        if !options.refresh {
            for (name, entry) in lock.map(|l| l.dependencies.iter()).into_iter().flatten() {
                if dependencies.contains_key(name) || !entry.source.is_url() {
                    continue;
                }
                let known = match stored(name, &entry.version) {
                    Some(pkg) => Some(closure::entry_of(name, &pkg.manifest)),
                    None => match registry_set.as_deref() {
                        Some(set) if !set.is_empty() => {
                            set.get_version_in(name, &entry.version, None).await.ok()
                        }
                        _ => None,
                    },
                };
                if let Some(known) = known {
                    supplied.push(Candidates {
                        name: name.clone(),
                        pin: Some(entry.version.clone()),
                        versions: vec![known],
                        on_disk: false,
                    });
                }
            }
        }

        let mut source = RegistrySource {
            set: registry_set.clone(),
        };
        let selections = if options.refresh {
            closure::select_closed(&listed_fresh, &[], houdini, &mut source, |_| false).await
        } else {
            closure::select_keeping(
                closure::Kept {
                    listed: &listed_locked,
                    supplied: &supplied,
                },
                &listed_fresh,
                houdini,
                &mut source,
                options.frozen,
            )
            .await
        }
        .map_err(ProjectError::Resolution)?;

        // What to install from a registry: each registry dependency at the
        // version chosen for it, and each required package the project does
        // not list. Path and URL dependencies install from their own source.
        let mut installs: Vec<(String, DependencySpec)> = Vec::new();
        for sel in &selections {
            match dependencies.get(&sel.name) {
                Some(DependencySpec::Registry {
                    registry, optional, ..
                }) => installs.push((
                    sel.name.clone(),
                    DependencySpec::Registry {
                        version: sel.version.clone(),
                        registry: registry.clone(),
                        optional: *optional,
                    },
                )),
                Some(_) => {}
                None => {
                    info!(
                        "Installing {} {}: a package the project uses requires it",
                        sel.name, sel.version
                    );
                    installs.push((
                        sel.name.clone(),
                        DependencySpec::Registry {
                            version: sel.version.clone(),
                            registry: None,
                            optional: false,
                        },
                    ));
                }
            }
        }
        for (name, spec) in &installs {
            if let DependencySpec::Registry { version, .. } = spec {
                frozen_check(name, version)?;
            }
        }
        installs.extend(
            dependencies
                .iter()
                .filter(|(_, spec)| matches!(spec, DependencySpec::Path { .. }))
                .map(|(n, s)| (n.clone(), s.clone())),
        );
        outcomes.extend(
            self.install_all(installs, registry_set, &all_installed, &locked_at)
                .await?,
        );

        // Snapshot the installed-package list once for downstream steps; the
        // outcomes hang onto richer metadata for the lockfile, but Python
        // resolution / manifest emission only need the InstalledPackage.
        let installed: Vec<InstalledPackage> =
            outcomes.iter().map(|(_, o)| o.package.clone()).collect();

        // The venv for the installed packages' Python dependencies, if any.
        let manifests: Vec<hpm_package::PackageManifest> =
            installed.iter().map(|p| p.manifest.clone()).collect();
        let venv_site_packages = super::python_env::ensure_venv(
            &crate::python::PythonHome::for_storage(&self.config.storage),
            project_houdini.as_deref(),
            &manifests,
            &installed,
            &[],
        )
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
        // removed, registry yank, manual edit, a requirement no longer
        // needed) keeps loading the package even though nothing asks for it.
        self.sweep_stale_houdini_manifests(&installed)?;

        reclaim_superseded_dev_copies(&installed);

        info!("Successfully synced project dependencies");
        Ok(outcomes)
    }

    /// Install `deps` in parallel, at most `[install] parallel_downloads`
    /// at once. Each task owns a clone of (StorageManager, ArchiveFetcher,
    /// RegistrySet) and its spec, so resolve, fetch and copy-into-CAS
    /// overlap across packages.
    async fn install_all(
        &self,
        deps: Vec<(String, hpm_package::DependencySpec)>,
        registry_set: Option<Arc<crate::registry::RegistrySet>>,
        all_installed: &Arc<Vec<InstalledPackage>>,
        locked_at: &impl Fn(&str, &str) -> Option<String>,
    ) -> Result<Vec<(String, InstallOutcome)>, ProjectError> {
        let slots = Arc::new(tokio::sync::Semaphore::new(
            self.config.install.parallel_downloads.get(),
        ));
        let mut tasks: JoinSet<(String, Result<InstallOutcome, ProjectError>)> = JoinSet::new();
        for (name, spec) in deps {
            let storage = self.storage_manager.clone();
            let fetcher = self.fetcher.clone();
            let registry_set = registry_set.clone();
            let all_installed = all_installed.clone();
            let slots = slots.clone();
            let locked = match &spec {
                hpm_package::DependencySpec::Registry { version, .. }
                | hpm_package::DependencySpec::Url { version, .. } => locked_at(&name, version),
                hpm_package::DependencySpec::Path { .. } => None,
            };

            tasks.spawn(async move {
                let _slot = slots
                    .acquire_owned()
                    .await
                    .expect("the install semaphore is never closed");
                let result = install_one_dep(
                    &storage,
                    &fetcher,
                    registry_set.as_deref(),
                    &all_installed,
                    &name,
                    &spec,
                    locked.as_deref(),
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
        Ok(outcomes)
    }
}

/// The Houdini a project's `[compat].houdini` lower bound names, for the
/// solver: `22` reads as `22.0`. None when there is no usable bound.
fn houdini_target(lower_bound: &str) -> Option<crate::registry::solve::HoudiniTarget> {
    use crate::registry::solve::HoudiniTarget;
    HoudiniTarget::parse(lower_bound).or_else(|| HoudiniTarget::parse(&format!("{lower_bound}.0")))
}

/// Where a sync finds the versions of a package the project does not list:
/// the project's registries, first one that has it.
struct RegistrySource {
    set: Option<Arc<crate::registry::RegistrySet>>,
}

impl crate::registry::closure::VersionSource for RegistrySource {
    async fn versions(
        &mut self,
        name: &str,
    ) -> Result<Vec<crate::registry::RegistryEntry>, crate::registry::closure::Unsupplied> {
        use crate::registry::closure::Unsupplied;
        match self.set.as_deref() {
            Some(set) if !set.is_empty() => set
                .get_versions(name)
                .await
                .map_err(|e| Unsupplied::Failed(e.to_string())),
            _ => Err(Unsupplied::Failed(
                "no package registries are configured; run 'hpm registry add <url>' to add one"
                    .into(),
            )),
        }
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

/// Check a store copy about to be reused against the checksum `hpm.lock`
/// records for it. The store is shared by every project on the machine, so
/// a copy installed earlier may not be the bytes this project locked.
async fn verify_locked(
    pkg: &InstalledPackage,
    name: &str,
    locked_checksum: Option<&str>,
) -> Result<(), ProjectError> {
    let Some(expected) = locked_checksum else {
        return Ok(());
    };
    let dir = pkg.install_path.clone();
    let recorded = expected.to_string();
    let actual =
        tokio::task::spawn_blocking(move || crate::tree_hash::hash_tree_as(&dir, &recorded))
            .await
            .unwrap_or_else(|e| std::panic::resume_unwind(e.into_panic()))?;
    if actual == expected {
        Ok(())
    } else {
        Err(ProjectError::LockChecksumMismatch {
            name: name.to_string(),
            version: pkg.version.clone(),
            expected: expected.to_string(),
            actual,
        })
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
    locked_checksum: Option<&str>,
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
                verify_locked(pkg, name, locked_checksum).await?;
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
                fetch_and_install_pkg(storage, fetcher, name, version, source, locked_checksum)
                    .await?;
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
                verify_locked(pkg, name, locked_checksum).await?;
                reuse_installed(pkg).await;
                return Ok(InstallOutcome {
                    package: pkg.clone(),
                    checksum: None,
                    source: Some(LockedSource::url(url.clone(), version.clone())),
                });
            }
            let source = PackageSource::url(url, version)?;
            let (package, checksum) =
                fetch_and_install_pkg(storage, fetcher, name, version, source, locked_checksum)
                    .await?;
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
    locked_checksum: Option<&str>,
) -> Result<(InstalledPackage, String), ProjectError> {
    let (installed, checksum) = fetcher
        .fetch_into_store::<ProjectError>(storage, &source, name, locked_checksum)
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
