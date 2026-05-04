use super::manifest_utils::{determine_manifest_path, load_manifest};
use crate::progress::OperationProgress;
use anyhow::{Context, Result};
use hpm_core::{
    ArchiveFetcher, LockFile, LockedDependency, LockedPythonDependency, PackageSource,
    StorageManager,
};
use hpm_package::{EnvMethod, HoudiniEnvValue, HoudiniPackage, ManifestEnvEntry, PackageManifest};
use indexmap::IndexMap;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::task::JoinSet;
use tracing::{debug, info, warn};

/// Install dependencies from hpm.toml manifest
///
/// This function reads the hpm.toml file from the specified path (or current directory),
/// resolves all dependencies (both HPM and Python), and ensures they are installed
/// and configured in the .hpm directory structure.
///
/// # Arguments
///
/// * `manifest_path` - Optional path to hpm.toml file
/// * `frozen_lockfile` - If true, fail if lock file is missing or would change
pub async fn install_dependencies(
    manifest_path: Option<PathBuf>,
    frozen_lockfile: bool,
) -> Result<()> {
    info!("Starting dependency installation");

    if frozen_lockfile {
        info!("Using frozen lockfile mode - lock file must exist and not change");
    }

    let mut progress = OperationProgress::new();
    progress.start("Installing dependencies");

    // Determine manifest path
    let manifest_path = determine_manifest_path(manifest_path)?;
    info!("Using manifest: {}", manifest_path.display());

    // Load and validate manifest
    progress.set_message("Loading manifest");
    let manifest = load_manifest(&manifest_path)
        .with_context(|| format!("Failed to load manifest from {}", manifest_path.display()))?;

    info!(
        "Installing dependencies for package: {} v{}",
        manifest.package.name, manifest.package.version
    );

    // Create .hpm directory structure
    let project_dir = manifest_path
        .parent()
        .context("Manifest file has no parent directory")?;
    let hpm_dir = project_dir.join(".hpm");

    progress.set_message("Setting up project directory");
    setup_hpm_directory(&hpm_dir)
        .await
        .context("Failed to setup .hpm directory")?;

    // Load existing lock file if present for checksum verification
    let lock_path = project_dir.join("hpm.lock");

    // Frozen lockfile mode requires lock file to exist
    if frozen_lockfile && !lock_path.exists() {
        return Err(anyhow::anyhow!(
            "--frozen-lockfile requires hpm.lock to exist. Run 'hpm install' first to generate it."
        ));
    }

    let existing_lock = if lock_path.exists() {
        match LockFile::load(&lock_path) {
            Ok(lock) => {
                info!("Loaded existing lock file for verification");
                Some(lock)
            }
            Err(e) => {
                // Frozen mode promises the install reproduces the lockfile.
                // Silently skipping an unparseable lockfile would bypass that
                // promise — surface it instead so the user can repair or
                // regenerate the lockfile explicitly.
                if frozen_lockfile {
                    return Err(anyhow::anyhow!(
                        "--frozen-lockfile requires a valid hpm.lock, but loading it failed: {}. \
                         Re-run without --frozen-lockfile to regenerate it.",
                        e
                    ));
                }
                warn!("Failed to load existing lock file: {}", e);
                None
            }
        }
    } else {
        None
    };

    // Verify cached packages against lock file checksums
    if let Some(ref lock) = existing_lock {
        progress.set_message("Verifying cached packages");
        let config = hpm_config::Config::load()
            .map_err(|e| anyhow::anyhow!("Failed to load HPM configuration: {e}"))?;
        let packages_dir = &config.storage.packages_dir;

        if let Err(e) = lock.verify_checksums(packages_dir) {
            return Err(anyhow::anyhow!(
                "Package integrity check failed: {}. Delete the corrupted package and run 'hpm install' again.",
                e
            ));
        }
        info!("Cached packages verified successfully");

        // Check for stale lock file (>90 days old)
        if let Some(ref metadata) = lock.metadata {
            if let Some(days) = metadata.days_since_generated() {
                if days > 90 {
                    warn!(
                        "Lock file is {} days old. Consider running 'hpm update' to check for newer versions.",
                        days
                    );
                }
            }
        }
    }

    // Install HPM dependencies
    let install_results = if let Some(dependencies) = &manifest.dependencies {
        if !dependencies.is_empty() {
            progress.set_message(format!(
                "Installing {} HPM dependencies",
                dependencies.len()
            ));
            info!("Installing {} HPM dependencies", dependencies.len());
            install_hpm_dependencies(dependencies)
                .await
                .context("Failed to install HPM dependencies")?
        } else {
            info!("No HPM dependencies to install");
            HashMap::new()
        }
    } else {
        info!("No HPM dependencies specified");
        HashMap::new()
    };

    // Collect manifests from installed dependencies
    let mut all_manifests = vec![manifest.clone()];
    debug!(
        "Checking {} installed packages for Python dependencies",
        install_results.len()
    );
    for (name, result) in &install_results {
        match load_package_manifest(&result.package_path) {
            Ok(Some(dep_manifest)) => {
                info!(
                    "Loaded manifest from dependency '{}' with {} Python deps",
                    name,
                    dep_manifest
                        .python_dependencies
                        .as_ref()
                        .map(|d| d.len())
                        .unwrap_or(0)
                );
                all_manifests.push(dep_manifest);
            }
            Ok(None) => {
                debug!("Dependency '{}' has no hpm.toml", name);
            }
            Err(e) => {
                warn!("Failed to load manifest from dependency '{}': {}", name, e);
            }
        }
    }

    // Count total Python dependencies across all manifests
    let total_python_deps: usize = all_manifests
        .iter()
        .filter_map(|m| m.python_dependencies.as_ref())
        .map(|deps| deps.len())
        .sum();

    // Install Python dependencies from all manifests (root + dependencies).
    // Returns the venv's site-packages path (if any), which is then threaded into
    // the generated Houdini manifests as a PYTHONPATH entry.
    let venv_site_packages = if total_python_deps > 0 {
        progress.set_message(format!(
            "Installing {} Python dependencies from {} packages",
            total_python_deps,
            all_manifests.len()
        ));
        info!(
            "Installing {} Python dependencies from {} packages",
            total_python_deps,
            all_manifests.len()
        );
        install_python_dependencies(&all_manifests)
            .await
            .context("Failed to install Python dependencies")?
    } else {
        info!("No Python dependencies specified in any package");
        None
    };

    // Write per-package Houdini package.json files into .hpm/packages/{name}.json
    // so Houdini can load each installed dep and so the shared venv's PYTHONPATH
    // reaches packages that declare [python_dependencies].
    progress.set_message("Generating Houdini package manifests");
    write_houdini_manifests(
        &hpm_dir,
        &install_results,
        venv_site_packages.as_deref(),
        manifest.env.as_ref(),
    )
    .await
    .context("Failed to generate Houdini package manifests")?;

    // Sweep stale entries (manifests, symlinks, Windows ref files) for deps
    // that have left the set since the previous install. Houdini reads every
    // <name>.json on launch and follows every symlink in the dir, so leaving
    // them behind keeps removed deps live in the project.
    sweep_stale_install_entries(&hpm_dir, &install_results)
        .await
        .context("Failed to sweep stale install entries")?;

    // Generate or update lock file (skip in frozen lockfile mode)
    if frozen_lockfile {
        info!("Skipping lock file update (--frozen-lockfile)");
    } else {
        progress.set_message("Generating lock file");
        generate_lock_file(&manifest, project_dir, &install_results)
            .await
            .context("Failed to generate lock file")?;
    }

    progress.finish_success("Dependencies installed");
    info!("Dependency installation completed successfully");
    Ok(())
}

/// Setup the .hpm directory structure
async fn setup_hpm_directory(hpm_dir: &Path) -> Result<()> {
    info!("Setting up .hpm directory: {}", hpm_dir.display());

    // Create main .hpm directory
    tokio::fs::create_dir_all(hpm_dir)
        .await
        .with_context(|| format!("Failed to create .hpm directory: {}", hpm_dir.display()))?;

    // Create subdirectories
    let packages_dir = hpm_dir.join("packages");
    tokio::fs::create_dir_all(&packages_dir)
        .await
        .with_context(|| {
            format!(
                "Failed to create packages directory: {}",
                packages_dir.display()
            )
        })?;

    info!(".hpm directory structure created");
    Ok(())
}

/// Result of installing a single package.
#[derive(Debug)]
struct PackageInstallResult {
    /// SHA-256 checksum of the installed package contents.
    checksum: String,
    /// Path to the installed package directory.
    package_path: PathBuf,
    /// The resolved package source (URL or path) used for the lock file.
    resolved_source: PackageSource,
}

/// Install HPM package dependencies.
///
/// Fetches URL/registry deps in parallel via `ArchiveFetcher` (downloading +
/// extracting into a fetch staging dir), then copies each into the global
/// `StorageManager` CAS via `install_from_path` — the same content-
/// addressable layout `ProjectManager::sync_dependencies` uses, so both
/// commands now write to one canonical location at
/// `<packages_dir>/<slug>@<version>/`. Path deps skip the fetcher entirely
/// and go through `install_from_path_dev`, landing under
/// `<packages_dir>/_dev/<slug>@<version>/` (registry/URL deps can't be
/// poisoned by a dev install at the same coordinate).
///
/// No project-side symlinks are created. The Houdini JSON manifests
/// written downstream embed absolute paths into the CAS.
async fn install_hpm_dependencies(
    dependencies: &indexmap::IndexMap<String, hpm_package::DependencySpec>,
) -> Result<HashMap<String, PackageInstallResult>> {
    info!("Installing HPM dependencies...");

    let config = hpm_config::Config::load()
        .map_err(|e| anyhow::anyhow!("Failed to load HPM configuration: {e}"))?;

    // Fetcher staging dir lives next to the canonical CAS (not inside it),
    // so a half-extracted archive on `/tmp` filling the disk doesn't trash
    // a successful previous install.
    let cache_dir = config.storage.cache_dir.clone();
    let staging_dir = config
        .storage
        .packages_dir
        .parent()
        .unwrap_or(Path::new("."))
        .join("fetch");
    let fetcher = ArchiveFetcher::new(cache_dir, staging_dir)
        .context("Failed to initialize archive fetcher")?;

    let storage_manager = Arc::new(
        StorageManager::new(config.storage.clone())
            .context("Failed to initialize storage manager")?,
    );

    // Build registry set for any registry-resolved deps (shared across tasks)
    let registry_set = {
        let has_registry_deps = dependencies.values().any(|spec| spec.is_registry());
        if has_registry_deps {
            Some(Arc::new(super::registry::build_registry_set(&config)))
        } else {
            None
        }
    };

    // Phase 1: spawn all installs in parallel via a JoinSet. Each task
    // handles its own (resolve → fetch → copy-into-CAS) chain so the
    // expensive network and extraction steps overlap across deps.
    let mut tasks: JoinSet<anyhow::Result<(String, PackageInstallResult)>> = JoinSet::new();
    for (name, spec) in dependencies.iter() {
        let fetcher = fetcher.clone();
        let storage = storage_manager.clone();
        let name = name.clone();
        let spec = spec.clone();
        let registry_set = registry_set.clone();

        tasks.spawn(async move {
            info!("Processing dependency: {}", name);

            let result = match spec {
                hpm_package::DependencySpec::Simple(version)
                | hpm_package::DependencySpec::Registry { version, .. } => {
                    info!("  {} - Registry @ {}", name, version);
                    let rs = registry_set
                        .as_ref()
                        .expect("registry set built for registry deps");
                    let entry = rs.get_version(&name, &version).await.with_context(|| {
                        format!("Failed to resolve {}@{} from registry", name, version)
                    })?;
                    let source = PackageSource::url(&entry.dl, &version)
                        .context("Invalid URL from registry")?;
                    if let Some(warning) = source.security_warning() {
                        warn!("Security: {} - {}", name, warning);
                    }
                    fetch_and_install(&fetcher, &storage, &name, source).await?
                }
                hpm_package::DependencySpec::Url {
                    url,
                    version,
                    optional,
                } => {
                    info!("  {} - Url: {} @ {}", name, url, version);
                    if optional {
                        debug!("  {} is optional", name);
                    }
                    let source = PackageSource::url(&url, &version).context("Invalid URL")?;
                    if let Some(warning) = source.security_warning() {
                        warn!("Security: {} - {}", name, warning);
                    }
                    fetch_and_install(&fetcher, &storage, &name, source).await?
                }
                hpm_package::DependencySpec::Path { path, optional } => {
                    info!("  {} - Path: {}", name, path);
                    if optional {
                        debug!("  {} is optional", name);
                    }
                    install_path_dep(&storage, &path).await?
                }
            };

            Ok::<_, anyhow::Error>((name, result))
        });
    }

    info!("Installing {} packages in parallel...", tasks.len());
    let mut results = HashMap::new();
    while let Some(joined) = tasks.join_next().await {
        let (name, result) = joined.context("Install task panicked")??;
        info!("  {} installed successfully", name);
        results.insert(name, result);
    }

    info!("Installed {} HPM packages", results.len());
    Ok(results)
}

/// Fetch a registry/URL package via `ArchiveFetcher` (download + extract
/// into the staging dir) and then copy from staging into the canonical
/// `StorageManager` CAS at `<packages_dir>/<slug>@<version>/`.
async fn fetch_and_install(
    fetcher: &ArchiveFetcher,
    storage: &StorageManager,
    name: &str,
    source: PackageSource,
) -> Result<PackageInstallResult> {
    let fetch_result = fetcher
        .fetch(&source, name)
        .await
        .with_context(|| format!("Failed to fetch package: {}", name))?;

    if fetch_result.from_cache {
        info!("  {} found in cache", name);
    } else {
        info!("  {} downloaded and extracted", name);
    }
    debug!(
        "  {} checksum: {}",
        name,
        &fetch_result.checksum[..fetch_result.checksum.len().min(16)]
    );

    let installed = storage
        .install_from_path(&fetch_result.package_path)
        .await
        .with_context(|| format!("Failed to install {} into the global CAS", name))?;

    Ok(PackageInstallResult {
        checksum: fetch_result.checksum,
        package_path: installed.install_path,
        resolved_source: source,
    })
}

/// Install a path-dependency. Bypasses the fetcher entirely — the source
/// directory is copied to `<packages_dir>/_dev/<slug>@<version>/` so dev
/// content can never substitute for a registry coordinate.
async fn install_path_dep(storage: &StorageManager, path: &str) -> Result<PackageInstallResult> {
    let source_path = Path::new(path);
    let installed = storage
        .install_from_path_dev(source_path)
        .await
        .with_context(|| format!("Failed to install path dep at {}", path))?;
    let resolved_source = PackageSource::path(source_path);
    Ok(PackageInstallResult {
        // Path deps don't carry a meaningful network checksum; the lockfile
        // records the source-tree hash via verify_checksums on the install
        // dir, which is enough to detect tampering after the copy.
        checksum: String::new(),
        package_path: installed.install_path,
        resolved_source,
    })
}

/// Load manifest from an installed package directory. Returns `Ok(None)`
/// when the directory has no `hpm.toml` (e.g. a legacy or non-HPM package
/// dropped into the symlink dir); read/parse errors propagate via
/// [`ManifestLoadError`].
fn load_package_manifest(package_path: &Path) -> Result<Option<PackageManifest>> {
    let manifest_path = package_path.join("hpm.toml");
    match PackageManifest::from_path(&manifest_path) {
        Ok(manifest) => Ok(Some(manifest)),
        Err(hpm_package::ManifestLoadError::NotFound { .. }) => {
            debug!("No hpm.toml found in package: {}", package_path.display());
            Ok(None)
        }
        Err(e) => Err(e.into()),
    }
}

/// Install Python dependencies using the hpm-python crate.
///
/// Collects Python dependencies from the root manifest AND all installed HPM
/// package dependencies, then resolves and installs them into a shared,
/// content-addressable virtual environment. Returns the venv's site-packages
/// path so the caller can wire it into Houdini package manifests.
async fn install_python_dependencies(manifests: &[PackageManifest]) -> Result<Option<PathBuf>> {
    info!("Installing Python dependencies...");

    hpm_python::initialize()
        .await
        .context("Failed to initialize Python dependency management")?;

    let python_deps = hpm_python::collect_python_dependencies(manifests)
        .await
        .context("Failed to collect Python dependencies")?;

    if python_deps.dependencies.is_empty() {
        info!("No Python dependencies to process");
        return Ok(None);
    }

    info!(
        "Found {} Python dependencies",
        python_deps.dependencies.len()
    );

    let resolved_deps = hpm_python::resolve_dependencies(&python_deps)
        .await
        .context("Failed to resolve Python dependencies")?;

    info!("Resolved {} Python packages", resolved_deps.packages.len());

    let venv_manager = hpm_python::VenvManager::new();
    let venv_path = venv_manager
        .ensure_virtual_environment(&resolved_deps)
        .await
        .context("Failed to create virtual environment")?;

    info!("Python virtual environment ready: {}", venv_path.display());

    Ok(Some(venv_manager.get_python_site_packages_path(
        &venv_path,
        &resolved_deps.python_version,
    )))
}

/// Remove `<name>.json` Houdini manifests in `<project>/.hpm/packages/`
/// for deps that have left the dependency set. Houdini reads every JSON
/// in this dir on launch, so an orphan manifest keeps loading the dropped
/// package; only entries we ourselves write (`.json`) are touched.
///
/// Earlier installs also created `<name>` symlinks and `<name>.hpmref`
/// reference files; both are gone now that JSON manifests embed absolute
/// CAS paths directly. Any leftovers from older installs are swept here
/// too, keyed off the bare-stem-matches-a-dep heuristic.
async fn sweep_stale_install_entries(
    hpm_dir: &Path,
    install_results: &HashMap<String, PackageInstallResult>,
) -> Result<()> {
    let packages_dir = hpm_dir.join("packages");
    if !packages_dir.exists() {
        return Ok(());
    }

    let valid: std::collections::HashSet<&str> =
        install_results.keys().map(|s| s.as_str()).collect();

    let mut entries = tokio::fs::read_dir(&packages_dir)
        .await
        .with_context(|| format!("Failed to read {}", packages_dir.display()))?;

    while let Some(entry) = entries
        .next_entry()
        .await
        .with_context(|| format!("Failed to enumerate {}", packages_dir.display()))?
    {
        let path = entry.path();
        let file_name = match path.file_name().and_then(|n| n.to_str()) {
            Some(name) => name,
            None => continue,
        };

        // Sweep our own entry shapes; anything else (README.md, .gitignore)
        // is left alone. The `<name>` and `<name>.hpmref` cases are kept so
        // upgrades from the previous symlink-based install layout clean up.
        let dep_name = if let Some(stem) = file_name.strip_suffix(".json") {
            stem
        } else if let Some(stem) = file_name.strip_suffix(".hpmref") {
            stem
        } else if !file_name.contains('.') {
            file_name
        } else {
            continue;
        };

        if valid.contains(dep_name) {
            continue;
        }

        let meta = match tokio::fs::symlink_metadata(&path).await {
            Ok(m) => m,
            Err(e) => {
                warn!("Failed to stat stale entry {}: {}", path.display(), e);
                continue;
            }
        };

        let result = if meta.file_type().is_dir() {
            tokio::fs::remove_dir_all(&path).await
        } else {
            tokio::fs::remove_file(&path).await
        };

        match result {
            Ok(()) => debug!("Removed stale install entry: {}", path.display()),
            Err(e) => warn!("Failed to remove stale entry {}: {}", path.display(), e),
        }
    }

    Ok(())
}

/// Write a Houdini package.json per installed dependency into `.hpm/packages/`.
///
/// Each file is picked up by Houdini when the project's `.hpm/packages` directory
/// is on `HOUDINI_PACKAGE_PATH`. Absolute paths are used instead of
/// `$HPM_PACKAGE_ROOT` so Houdini resolves them without additional env wiring.
/// Packages that declare `[python_dependencies]` get the shared venv's
/// `site-packages` prepended to `PYTHONPATH`.
async fn write_houdini_manifests(
    hpm_dir: &Path,
    install_results: &HashMap<String, PackageInstallResult>,
    venv_site_packages: Option<&Path>,
    project_env_overrides: Option<&IndexMap<String, ManifestEnvEntry>>,
) -> Result<()> {
    let packages_json_dir = hpm_dir.join("packages");
    tokio::fs::create_dir_all(&packages_json_dir)
        .await
        .with_context(|| {
            format!(
                "Failed to create Houdini package manifest directory: {}",
                packages_json_dir.display()
            )
        })?;

    for (name, result) in install_results {
        let manifest = match load_package_manifest(&result.package_path)? {
            Some(m) => m,
            None => {
                debug!("Skipping Houdini manifest for '{}' (no hpm.toml)", name);
                continue;
            }
        };

        let houdini_pkg = build_houdini_package_for_install(
            name,
            &manifest,
            &result.package_path,
            venv_site_packages,
            project_env_overrides,
        )
        .with_context(|| format!("Failed to build Houdini package for '{}'", name))?;

        let manifest_path = packages_json_dir.join(format!("{}.json", name));
        let file = std::fs::File::create(&manifest_path).with_context(|| {
            format!(
                "Failed to create Houdini manifest: {}",
                manifest_path.display()
            )
        })?;
        serde_json::to_writer_pretty(std::io::BufWriter::new(file), &houdini_pkg).with_context(
            || {
                format!(
                    "Failed to serialize Houdini manifest: {}",
                    manifest_path.display()
                )
            },
        )?;
        debug!("Wrote Houdini manifest: {}", manifest_path.display());
    }

    Ok(())
}

/// Build a `HoudiniPackage` for an installed dependency.
///
/// Mirrors the logic in `hpm_core::ProjectManager::create_houdini_package_with_python`
/// but operates on the values the install command already has on hand. Any
/// `$HPM_PACKAGE_ROOT` in user-declared env values is substituted with the
/// package's absolute install path.
fn build_houdini_package_for_install(
    package_name: &str,
    manifest: &PackageManifest,
    package_path: &Path,
    venv_site_packages: Option<&Path>,
    project_env_overrides: Option<&IndexMap<String, ManifestEnvEntry>>,
) -> Result<HoudiniPackage> {
    let package_path_str = package_path.to_string_lossy().to_string();

    let mut env: Vec<HashMap<String, HoudiniEnvValue>> = Vec::new();

    // Venv PYTHONPATH — only for packages that declare Python dependencies.
    if let Some(site_packages) = venv_site_packages {
        if manifest.python_dependencies.is_some() {
            let mut map = HashMap::new();
            map.insert(
                "PYTHONPATH".to_string(),
                HoudiniEnvValue::prepend(site_packages.to_string_lossy().to_string()),
            );
            env.push(map);
        }
    }

    // Package's bundled python/ directory, if present.
    let python_dir = package_path.join("python");
    if python_dir.exists() {
        let mut map = HashMap::new();
        map.insert(
            "PYTHONPATH".to_string(),
            HoudiniEnvValue::prepend(python_dir.to_string_lossy().to_string()),
        );
        env.push(map);
    }

    // Package's bundled scripts/ directory, if present.
    let scripts_dir = package_path.join("scripts");
    if scripts_dir.exists() {
        let mut map = HashMap::new();
        map.insert(
            "HOUDINI_SCRIPT_PATH".to_string(),
            HoudiniEnvValue::prepend(scripts_dir.to_string_lossy().to_string()),
        );
        env.push(map);
    }

    // User-declared [env] entries from the package's hpm.toml. Project-level
    // [env] in the consuming project's hpm.toml overrides per key. A
    // `required = true` placeholder with no value (and no override) is a hard
    // error — the package wouldn't be launchable without it.
    if let Some(user_env) = &manifest.env {
        for (key, entry) in user_env {
            let override_entry = project_env_overrides.and_then(|o| o.get(key));
            let effective_entry = override_entry.unwrap_or(entry);

            let raw_value = effective_entry.value.as_ref().ok_or_else(|| {
                anyhow::anyhow!(
                    "Required env var '{}' for package '{}' has no value. \
                     Set it in this project's [env] section in hpm.toml.",
                    key,
                    package_name
                )
            })?;

            let value = raw_value.replace("$HPM_PACKAGE_ROOT", &package_path_str);
            let houdini_value = match effective_entry.method {
                EnvMethod::Set => HoudiniEnvValue::set(value),
                EnvMethod::Prepend => HoudiniEnvValue::prepend(value),
                EnvMethod::Append => HoudiniEnvValue::append(value),
            };
            let mut map = HashMap::new();
            map.insert(key.clone(), houdini_value);
            env.push(map);
        }
    }

    let enable = manifest.houdini.as_ref().and_then(|cfg| {
        let mut conditions = Vec::new();
        if let Some(min_version) = &cfg.min_version {
            conditions.push(format!("houdini_version >= '{}'", min_version));
        }
        if let Some(max_version) = &cfg.max_version {
            conditions.push(format!("houdini_version <= '{}'", max_version));
        }
        if conditions.is_empty() {
            None
        } else {
            Some(conditions.join(" and "))
        }
    });

    Ok(HoudiniPackage {
        hpath: Some(vec![package_path_str]),
        env: if env.is_empty() { None } else { Some(env) },
        enable,
        requires: None,
        recommends: None,
    })
}

/// Generate or update the hpm.lock file
async fn generate_lock_file(
    manifest: &PackageManifest,
    project_dir: &Path,
    install_results: &HashMap<String, PackageInstallResult>,
) -> Result<()> {
    info!("Generating lock file");

    let lock_file_path = project_dir.join("hpm.lock");

    // Create a new lock file
    let mut lock_file = LockFile::new(
        manifest.package.name.clone(),
        manifest.package.version.clone(),
    );

    // Add HPM dependencies with resolved versions and checksums
    if let Some(dependencies) = &manifest.dependencies {
        for (name, spec) in dependencies {
            // Get the checksum from installation results if available
            let checksum = install_results.get(name).map(|r| r.checksum.clone());

            let locked_dep = match spec {
                hpm_package::DependencySpec::Simple(_)
                | hpm_package::DependencySpec::Registry { .. } => {
                    let version = spec.version().unwrap_or("unknown").to_string();
                    let source = install_results
                        .get(name)
                        .map(|r| r.resolved_source.clone())
                        .unwrap_or_else(|| PackageSource::Url {
                            url: "unresolved".to_string(),
                            version: version.clone(),
                        });
                    LockedDependency {
                        version,
                        checksum,
                        source,
                        dependencies: Vec::new(),
                    }
                }
                hpm_package::DependencySpec::Url { url, version, .. } => LockedDependency {
                    version: version.clone(),
                    checksum,
                    source: PackageSource::Url {
                        url: url.clone(),
                        version: version.clone(),
                    },
                    dependencies: Vec::new(),
                },
                hpm_package::DependencySpec::Path { path, .. } => {
                    LockedDependency::from_path("local".to_string(), path.clone(), checksum)
                }
            };

            lock_file.add_dependency(name.clone(), locked_dep);
        }
    }

    // Add Python dependencies with resolved versions
    if let Some(python_deps) = &manifest.python_dependencies {
        for (name, spec) in python_deps {
            let version = match spec {
                hpm_package::PythonDependencySpec::Simple(v) => v.clone(),
                hpm_package::PythonDependencySpec::Detailed { version, .. } => {
                    version.clone().unwrap_or_else(|| "*".to_string())
                }
            };

            let locked_python_dep = LockedPythonDependency::new(version);
            lock_file.add_python_dependency(name.clone(), locked_python_dep);
        }
    }

    // Write the lock file
    let lock_content = lock_file
        .to_toml()
        .context("Failed to serialize lock file")?;

    tokio::fs::write(&lock_file_path, lock_content)
        .await
        .with_context(|| format!("Failed to write lock file: {}", lock_file_path.display()))?;

    info!("Lock file generated: {}", lock_file_path.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::test_fixtures::{CwdGuard, TestManifestOpts, write_test_manifest};
    use tempfile::TempDir;

    #[test]
    fn test_load_manifest_valid() {
        let temp_dir = TempDir::new().unwrap();
        write_test_manifest(
            temp_dir.path(),
            TestManifestOpts {
                include_deps: true,
                include_python_deps: true,
                ..Default::default()
            },
        )
        .unwrap();

        let manifest_path = temp_dir.path().join("hpm.toml");
        let result = load_manifest(&manifest_path);

        assert!(result.is_ok());
        let manifest = result.unwrap();
        assert_eq!(manifest.package.name, "test-package");
        assert_eq!(manifest.package.version, "1.0.0");
        assert!(manifest.dependencies.is_some());
        assert!(manifest.python_dependencies.is_some());
        assert_eq!(manifest.dependencies.as_ref().unwrap().len(), 2);
        assert_eq!(manifest.python_dependencies.as_ref().unwrap().len(), 3);
    }

    #[test]
    fn test_load_manifest_invalid_toml() {
        let temp_dir = TempDir::new().unwrap();
        let manifest_path = temp_dir.path().join("hpm.toml");

        std::fs::write(&manifest_path, "invalid toml content [[[").unwrap();

        let result = load_manifest(&manifest_path);
        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(
            error_msg.contains("failed to parse manifest at"),
            "expected ManifestLoadError::Parse text, got: {error_msg}"
        );
    }

    #[test]
    fn test_load_manifest_validation_failure() {
        let temp_dir = TempDir::new().unwrap();
        let manifest_path = temp_dir.path().join("hpm.toml");

        let invalid_content = r#"[package]
path = "studio/empty-name"
name = ""
version = "1.0.0"
"#;
        std::fs::write(&manifest_path, invalid_content).unwrap();

        let result = load_manifest(&manifest_path);
        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg.contains("Manifest validation failed"));
    }

    #[tokio::test]
    async fn test_setup_hpm_directory() {
        let temp_dir = TempDir::new().unwrap();
        let hpm_dir = temp_dir.path().join(".hpm");

        let result = setup_hpm_directory(&hpm_dir).await;

        assert!(result.is_ok());
        assert!(hpm_dir.exists());
        assert!(hpm_dir.is_dir());
        assert!(hpm_dir.join("packages").exists());
        assert!(hpm_dir.join("packages").is_dir());
    }

    #[tokio::test]
    async fn test_install_dependencies_basic_manifest() {
        let temp_dir = TempDir::new().unwrap();

        // Create test manifest without dependencies to test directory and lock file setup
        // (testing actual package installation requires network access and is not unit-testable)
        let manifest_content = r#"[package]
path = "studio/test-install-package"
name = "test-install-package"
version = "1.0.0"
description = "Test package for install command"
authors = ["Test Author <test@example.com>"]
license = "MIT"

[houdini]
min_version = "20.5"
"#;
        std::fs::write(temp_dir.path().join("hpm.toml"), manifest_content).unwrap();

        let _cwd = CwdGuard::enter(temp_dir.path());

        // Install dependencies (no deps, so this tests directory setup and lock file creation)
        let result = install_dependencies(None, false).await;

        // The function should complete successfully for manifests without dependencies
        // This tests the manifest parsing and directory setup logic
        assert!(result.is_ok());

        // Verify directory structure was created
        let hpm_dir = temp_dir.path().join(".hpm");
        assert!(hpm_dir.exists());
        assert!(hpm_dir.join("packages").exists());

        // Verify lock file was created
        let lock_file = temp_dir.path().join("hpm.lock");
        assert!(lock_file.exists());

        let lock_content = std::fs::read_to_string(lock_file).unwrap();
        assert!(lock_content.contains("test-install-package"));
        assert!(lock_content.contains("1.0.0"));
    }

    #[tokio::test]
    async fn test_install_dependencies_explicit_manifest_path() {
        let temp_dir = TempDir::new().unwrap();
        let manifest_path = temp_dir.path().join("custom-manifest.toml");

        // Create test manifest without dependencies to test directory setup only
        // (testing actual package installation requires network access)
        let manifest_content = r#"[package]
path = "studio/custom-path-package"
name = "custom-path-package"
version = "2.0.0"
description = "Test custom manifest path"
"#;
        std::fs::write(&manifest_path, manifest_content).unwrap();

        let result = install_dependencies(Some(manifest_path), false).await;

        assert!(result.is_ok());

        // Verify directory structure was created relative to manifest location
        let hpm_dir = temp_dir.path().join(".hpm");
        assert!(hpm_dir.exists());

        // Verify lock file was created in the same directory as the manifest
        let lock_file = temp_dir.path().join("hpm.lock");
        assert!(lock_file.exists());
    }

    #[test]
    fn test_install_dependencies_nonexistent_manifest() {
        let temp_dir = TempDir::new().unwrap();
        let nonexistent_path = temp_dir.path().join("nonexistent.toml");

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(install_dependencies(Some(nonexistent_path), false));

        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg.contains("does not exist"));
    }

    /// Build a fake `PackageInstallResult` for sweep tests. The path and
    /// checksum content don't matter — the sweep only consults the keys.
    fn fake_install_result(path: &Path) -> PackageInstallResult {
        PackageInstallResult {
            checksum: "deadbeef".to_string(),
            package_path: path.to_path_buf(),
            resolved_source: PackageSource::Path { path: path.into() },
        }
    }

    /// Regression: a `<name>.json` left over from the previous install must
    /// be removed when `<name>` drops out of the dep set, otherwise Houdini
    /// keeps loading the orphan package.
    #[tokio::test]
    async fn sweep_removes_stale_houdini_manifest() {
        let temp_dir = TempDir::new().unwrap();
        let hpm_dir = temp_dir.path().join(".hpm");
        let pkgs = hpm_dir.join("packages");
        tokio::fs::create_dir_all(&pkgs).await.unwrap();

        let foo_json = pkgs.join("foo.json");
        let stale_json = pkgs.join("stale.json");
        let unrelated = pkgs.join("README.md");
        tokio::fs::write(&foo_json, b"{}").await.unwrap();
        tokio::fs::write(&stale_json, b"{}").await.unwrap();
        tokio::fs::write(&unrelated, b"hi").await.unwrap();

        let mut results = HashMap::new();
        results.insert("foo".to_string(), fake_install_result(temp_dir.path()));

        sweep_stale_install_entries(&hpm_dir, &results)
            .await
            .unwrap();

        assert!(foo_json.exists(), "current dep manifest must be kept");
        assert!(!stale_json.exists(), "stale dep manifest must be swept");
        assert!(unrelated.exists(), "non-dep entries must be left alone");
    }

    /// Regression: the `<name>` symlink/dir created by `install_hpm_dependencies`
    /// also leaks when the dep leaves the set. The sweep must remove that too,
    /// otherwise Houdini sees a directory entry that still contains a package.
    #[tokio::test]
    async fn sweep_removes_stale_symlink_or_dir() {
        let temp_dir = TempDir::new().unwrap();
        let hpm_dir = temp_dir.path().join(".hpm");
        let pkgs = hpm_dir.join("packages");
        tokio::fs::create_dir_all(&pkgs).await.unwrap();

        // Simulate an old dep's directory entry. A real install would write
        // this as a symlink to the global package dir, but the sweep treats
        // dir/file/symlink uniformly; a regular dir is the simplest fixture.
        let stale_pkg = pkgs.join("old-pkg");
        tokio::fs::create_dir_all(&stale_pkg).await.unwrap();
        tokio::fs::write(stale_pkg.join("dummy"), b"x")
            .await
            .unwrap();

        // And the Windows-fallback ref file shape.
        let stale_ref = pkgs.join("old-pkg.hpmref");
        tokio::fs::write(&stale_ref, b"/some/path").await.unwrap();

        // Empty install set: every entry is stale.
        sweep_stale_install_entries(&hpm_dir, &HashMap::new())
            .await
            .unwrap();

        assert!(
            !stale_pkg.exists(),
            "stale package dir/symlink must be swept"
        );
        assert!(!stale_ref.exists(), "stale .hpmref must be swept");
    }

    /// `.hpmref` files keyed off a dep that *is* still in the set must be kept.
    /// The sweep strips the `.hpmref` suffix to derive the dep name.
    #[tokio::test]
    async fn sweep_keeps_active_hpmref() {
        let temp_dir = TempDir::new().unwrap();
        let hpm_dir = temp_dir.path().join(".hpm");
        let pkgs = hpm_dir.join("packages");
        tokio::fs::create_dir_all(&pkgs).await.unwrap();

        let active_ref = pkgs.join("foo.hpmref");
        tokio::fs::write(&active_ref, b"/some/path").await.unwrap();

        let mut results = HashMap::new();
        results.insert("foo".to_string(), fake_install_result(temp_dir.path()));

        sweep_stale_install_entries(&hpm_dir, &results)
            .await
            .unwrap();

        assert!(
            active_ref.exists(),
            "active dep .hpmref must be kept (suffix-stripped name match)"
        );
    }
}
