//! Installing packages into Houdini's per-user preferences directory.
//!
//! A global install makes a package load in every session of one Houdini
//! version, with no project and no launcher wiring. It reuses the whole
//! project install path — same registries, same CAS, same venvs, same
//! manifest emitter — and differs in exactly two ways:
//!
//! 1. **Where the manifest goes.** Into Houdini's user `packages/` directory
//!    instead of `<project>/.hpm/packages/`.
//! 2. **What may be deleted there.** The project installer owns its output
//!    directory and sweeps anything it doesn't recognise. Houdini's user
//!    packages directory is not ours: it holds files from SideFX, other
//!    tools, and the user. Nothing here scans that directory to decide what
//!    to delete — every removal is driven off the ledger and touches only
//!    files hpm recorded writing. See [`ledger`].

pub mod ledger;

use std::path::{Path, PathBuf};

use hpm_package::{IoOp, PackageManifest, PackagePath, atomic_write};
use indexmap::IndexMap;
use thiserror::Error;
use tracing::{debug, info, warn};

use crate::houdini_prefs::{HoudiniPrefsError, HoudiniVersion, user_packages_dir};
use crate::project::houdini_emit::build_houdini_package;
use crate::registry::{RegistryError, RegistrySet};
use crate::storage::InstalledPackage;
use ledger::{GlobalEntry, Ledger, LedgerError};

#[derive(Debug, Error)]
pub enum GlobalError {
    #[error(transparent)]
    Io(#[from] IoOp),

    #[error(transparent)]
    Ledger(#[from] LedgerError),

    #[error(transparent)]
    Prefs(#[from] HoudiniPrefsError),

    #[error("Failed to resolve '{name}' from the configured registries: {source}")]
    Resolution {
        name: String,
        #[source]
        source: Box<RegistryError>,
    },

    #[error(
        "No registries are configured, so '{name}' cannot be resolved. \
         Add one with `hpm registry add <url> --name <alias>`."
    )]
    NoRegistries { name: String },

    #[error(
        "{name} {version} declares Houdini compatibility '{declared}', which does not \
         include Houdini {target}. Installing it would write a manifest that Houdini \
         silently ignores. Pick a version that supports {target}, or install into a \
         Houdini version the package supports."
    )]
    Incompatible {
        name: String,
        version: String,
        declared: String,
        target: HoudiniVersion,
    },

    #[error("'{name}' is not installed globally for Houdini {version}.")]
    NotInstalled {
        name: String,
        version: HoudiniVersion,
    },

    #[error(
        "The global ledger records '{name}' as manifest file '{manifest_file}', which is \
         not a filename hpm would have written. Refusing to delete it — a manifest name \
         must be a single `hpm-*.json` component inside Houdini's packages directory. \
         Fix or remove that ledger entry by hand."
    )]
    UnsafeLedgerEntry { name: String, manifest_file: String },

    #[error(transparent)]
    Project(#[from] Box<crate::project::ProjectError>),

    #[error("Failed to serialize the Houdini manifest for {path}: {source}")]
    ManifestSerialize {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
}

/// Filename hpm writes into Houdini's user packages directory.
///
/// The `hpm-` prefix marks the file as hpm-owned, so a human reading that
/// directory can tell at a glance which files are managed. The identity part
/// is the same `creator.slug` stem used for project manifests, so two
/// creators sharing a slug do not collide here either.
pub fn manifest_file_name(package: &PackagePath) -> String {
    format!("hpm-{}.json", package.file_stem())
}

/// Accept `candidate` only if it is a bare filename hpm itself could have
/// written: one path component, no separators, no `..`, `hpm-` prefixed and
/// `.json` suffixed.
///
/// Guards the one operation in this module that deletes: a ledger entry is
/// user-editable data, and joining an absolute or `..`-bearing string onto
/// the packages directory would reach outside it.
fn safe_manifest_file_name(candidate: &str) -> Option<&str> {
    if !candidate.starts_with("hpm-") || !candidate.ends_with(".json") {
        return None;
    }
    // One `Normal` component and nothing else — rejects `/abs/path`,
    // `../escape`, `a/b`, `.`, and (on Windows) `C:` prefixes.
    let mut components = Path::new(candidate).components();
    match (components.next(), components.next()) {
        (Some(std::path::Component::Normal(only)), None) if only == candidate => Some(candidate),
        _ => None,
    }
}

/// Where the manifest for `package` goes, for one Houdini version.
pub fn manifest_path(
    version: HoudiniVersion,
    package: &PackagePath,
) -> Result<PathBuf, GlobalError> {
    Ok(user_packages_dir(version)?.join(manifest_file_name(package)))
}

/// A globally installed package, as reported by [`list`].
#[derive(Debug, Clone)]
pub struct GlobalInstall {
    pub package: String,
    pub version: String,
    pub registry: Option<String>,
    pub manifest_path: PathBuf,
    pub install_path: PathBuf,
    /// False when the manifest hpm recorded writing is no longer on disk —
    /// deleted by hand, or by a Houdini reinstall. Reported rather than
    /// silently repaired so the user knows the ledger and disk disagree.
    pub manifest_present: bool,
}

/// Everything installed globally for one Houdini version.
pub fn list(hpm_home: &Path, version: HoudiniVersion) -> Result<Vec<GlobalInstall>, GlobalError> {
    let ledger = Ledger::load(&Ledger::path_for(hpm_home, version))?;
    let packages_dir = user_packages_dir(version)?;

    Ok(ledger
        .iter()
        .map(|(name, entry)| {
            let manifest_path = packages_dir.join(&entry.manifest_file);
            GlobalInstall {
                package: name.clone(),
                version: entry.version.clone(),
                registry: entry.registry.clone(),
                manifest_present: manifest_path.exists(),
                manifest_path,
                install_path: entry.install_path.clone(),
            }
        })
        .collect())
}

/// Remove a global install: delete the manifest hpm wrote, drop the ledger
/// entry.
///
/// Only the exact file named in the ledger is deleted. The CAS bytes stay —
/// they may be shared with a project, and `hpm clean` is the one command that
/// removes store content.
pub fn remove(
    hpm_home: &Path,
    version: HoudiniVersion,
    package: &PackagePath,
) -> Result<PathBuf, GlobalError> {
    let ledger_path = Ledger::path_for(hpm_home, version);
    let mut ledger = Ledger::load(&ledger_path)?;

    let entry = ledger
        .remove(package)
        .ok_or_else(|| GlobalError::NotInstalled {
            name: package.as_str().to_string(),
            version,
        })?;

    // The ledger is a plain JSON file the user can edit, and `Path::join`
    // with an absolute or `..`-bearing component escapes the base directory.
    // Deleting outside Houdini's packages directory is exactly what this
    // module promises never to do, so the recorded name must be a bare
    // filename hpm could itself have written.
    let file_name = safe_manifest_file_name(&entry.manifest_file).ok_or_else(|| {
        GlobalError::UnsafeLedgerEntry {
            name: package.as_str().to_string(),
            manifest_file: entry.manifest_file.clone(),
        }
    })?;

    let manifest_path = user_packages_dir(version)?.join(file_name);
    match std::fs::remove_file(&manifest_path) {
        Ok(()) => debug!("Removed global manifest {}", manifest_path.display()),
        // Already gone is success: the ledger entry is what we're really
        // clearing, and leaving it behind would make the package permanently
        // unremovable.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            warn!(
                "Global manifest {} was already absent",
                manifest_path.display()
            );
        }
        Err(e) => {
            return Err(IoOp::wrap("remove global Houdini manifest", &manifest_path, e).into());
        }
    }

    ledger.save(&ledger_path)?;
    Ok(manifest_path)
}

/// Check a package against the Houdini version it is being installed for.
///
/// A manifest whose `enable` expression evaluates false loads nothing and
/// says nothing, so an incompatible install would look like it worked and
/// then do nothing at all. A package that declares no range is compatible
/// with everything by construction — the emitted manifest carries no
/// `enable` clause — so there is nothing to check.
pub fn check_compatible(
    manifest: &PackageManifest,
    target: HoudiniVersion,
) -> Result<(), GlobalError> {
    let Some(range) = manifest.compat.houdini.as_ref() else {
        return Ok(());
    };
    if range.matches_version(target.major, target.minor, target.build) {
        return Ok(());
    }
    Err(GlobalError::Incompatible {
        name: manifest.package.identifier().to_string(),
        version: manifest.package.version.clone(),
        declared: range.as_str().to_string(),
        target,
    })
}

/// Resolve `name` at `version_req` through `registries`, honouring a pin.
pub async fn resolve_entry(
    registries: &RegistrySet,
    name: &str,
    version_req: &str,
    registry: Option<&str>,
) -> Result<crate::registry::RegistryEntry, GlobalError> {
    if registries.is_empty() {
        return Err(GlobalError::NoRegistries {
            name: name.to_string(),
        });
    }
    registries
        .resolve_in(name, version_req, registry)
        .await
        .map_err(|source| GlobalError::Resolution {
            name: name.to_string(),
            source: Box::new(source),
        })
}

/// Write the Houdini manifest for `installed` into the user packages
/// directory and record it in the ledger.
///
/// `venv_site_packages` mirrors the project path: packages declaring Python
/// dependencies get the shared venv's `site-packages` on `PYTHONPATH`.
pub fn write_install(
    hpm_home: &Path,
    version: HoudiniVersion,
    installed: &InstalledPackage,
    venv_site_packages: Option<&Path>,
    registry: Option<&str>,
) -> Result<PathBuf, GlobalError> {
    let package = &installed.manifest.package.path;

    // Identical to what a project install would emit — same builder, no
    // project-level [runtime] overrides, since there is no project.
    let houdini_package = build_houdini_package(installed, venv_site_packages, &IndexMap::new())
        .map_err(|e| GlobalError::Project(Box::new(e)))?;

    let packages_dir = user_packages_dir(version)?;
    std::fs::create_dir_all(&packages_dir)
        .map_err(|e| IoOp::wrap("create Houdini user packages directory", &packages_dir, e))?;

    let file_name = manifest_file_name(package);
    let manifest_path = packages_dir.join(&file_name);
    let content = serde_json::to_vec_pretty(&houdini_package).map_err(|source| {
        GlobalError::ManifestSerialize {
            path: manifest_path.clone(),
            source,
        }
    })?;
    atomic_write(&manifest_path, content)?;

    let ledger_path = Ledger::path_for(hpm_home, version);
    let mut ledger = Ledger::load(&ledger_path)?;
    ledger.insert(
        package,
        GlobalEntry {
            version: installed.version.clone(),
            registry: registry.map(str::to_string),
            manifest_file: file_name,
            install_path: installed.install_path.clone(),
        },
    );
    ledger.save(&ledger_path)?;

    info!(
        "Globally installed {}@{} for Houdini {}",
        package.as_str(),
        installed.version,
        version
    );
    Ok(manifest_path)
}

/// Install `name` globally for one Houdini version.
///
/// Mirrors the project install path step for step — resolve through the
/// configured registries, fetch into the shared CAS, resolve Python
/// dependencies into a shared venv, emit a Houdini manifest — with the
/// manifest landing in Houdini's user packages directory and the result
/// recorded in the ledger.
///
/// The compatibility check runs *after* resolution (the manifest is only
/// known once the package is fetched) but *before* anything is written, so a
/// rejected install leaves no manifest and no ledger entry behind.
pub async fn add(
    config: &hpm_config::Config,
    storage: &crate::storage::StorageManager,
    fetcher: &crate::archive_fetcher::ArchiveFetcher,
    registries: &RegistrySet,
    version: HoudiniVersion,
    name: &str,
    version_req: &str,
    registry: Option<&str>,
) -> Result<GlobalInstall, GlobalError> {
    let entry = resolve_entry(registries, name, version_req, registry).await?;

    let source = crate::package_source::PackageSource::url(entry.dl.clone(), &entry.version)
        .and_then(|s| s.with_registry_checksum(entry.cksum.as_deref()))
        .map_err(|e| GlobalError::Project(Box::new(e.into())))?;

    let (installed, _checksum) =
        crate::project::fetch_and_install_pkg(storage, fetcher, name, &entry.version, source)
            .await
            .map_err(|e| GlobalError::Project(Box::new(e)))?;

    check_compatible(&installed.manifest, version)?;

    let venv_site_packages = resolve_global_python(&installed, version).await?;

    let hpm_home = &config.storage.home_dir;
    let manifest_path = write_install(
        hpm_home,
        version,
        &installed,
        venv_site_packages.as_deref(),
        registry,
    )?;

    Ok(GlobalInstall {
        package: installed.manifest.package.identifier().to_string(),
        version: installed.version.clone(),
        registry: registry.map(str::to_string),
        manifest_present: true,
        manifest_path,
        install_path: installed.install_path,
    })
}

/// Resolve a globally installed package's Python dependencies into the shared
/// venv store, returning its `site-packages`.
///
/// The Houdini version drives the interpreter ABI. In a project this comes
/// from the root manifest's `[compat].houdini`; a global install has no root
/// manifest, so the `--houdini` target stands in. Getting this wrong would
/// build the venv against the wrong CPython and crash on import inside
/// Houdini.
async fn resolve_global_python(
    installed: &InstalledPackage,
    version: HoudiniVersion,
) -> Result<Option<PathBuf>, GlobalError> {
    if installed.manifest.python_dependencies.is_empty() {
        return Ok(None);
    }

    let to_project = |e: crate::python::PythonError| {
        GlobalError::Project(Box::new(crate::project::ProjectError::PythonResolution(
            e.into(),
        )))
    };

    crate::python::initialize().await.map_err(to_project)?;

    let collected = crate::python::collect_python_dependencies(
        Some(&version.as_dir_component()),
        std::slice::from_ref(&installed.manifest),
    )
    .await
    .map_err(to_project)?;

    if collected.dependencies.is_empty() {
        return Ok(None);
    }

    let resolved = crate::python::resolve_dependencies(&collected)
        .await
        .map_err(to_project)?;

    let venv_manager = crate::python::VenvManager::new().map_err(to_project)?;
    let venv_path = venv_manager
        .ensure_virtual_environment_for(&resolved, &[installed.venv_ref()])
        .await
        .map_err(to_project)?;

    Ok(Some(venv_manager.get_python_site_packages_path(
        &venv_path,
        &resolved.python_version,
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use hpm_package::{CompatConfig, HoudiniRange};

    fn manifest_with_compat(range: Option<&str>) -> PackageManifest {
        let mut manifest = PackageManifest::new(
            PackagePath::new("acme/tools").unwrap(),
            "Tools".to_string(),
            "1.0.0".to_string(),
            None,
            Vec::new(),
            None,
        );
        manifest.compat = CompatConfig {
            houdini: range.map(|r| HoudiniRange::parse(r).unwrap()),
            platforms: Vec::new(),
        };
        manifest
    }

    fn hv(s: &str) -> HoudiniVersion {
        HoudiniVersion::parse(s).unwrap()
    }

    #[test]
    fn manifest_file_name_is_prefixed_and_creator_scoped() {
        let a = manifest_file_name(&PackagePath::new("creator-a/tools").unwrap());
        let b = manifest_file_name(&PackagePath::new("creator-b/tools").unwrap());
        assert_eq!(a, "hpm-creator-a.tools.json");
        assert_ne!(a, b, "same slug from different creators must not collide");
        assert!(
            a.starts_with("hpm-"),
            "hpm-owned files must be identifiable"
        );
    }

    #[test]
    fn compatible_package_passes() {
        assert!(check_compatible(&manifest_with_compat(Some("^21")), hv("21.0")).is_ok());
        assert!(check_compatible(&manifest_with_compat(Some(">=20.5, <22")), hv("21.5")).is_ok());
    }

    /// The failure this prevents: an out-of-range package writes a manifest
    /// whose `enable` is false, so Houdini loads nothing and reports nothing.
    #[test]
    fn incompatible_package_is_rejected_up_front() {
        let err = check_compatible(&manifest_with_compat(Some("^21")), hv("22.0")).unwrap_err();
        assert!(
            matches!(err, GlobalError::Incompatible { .. }),
            "got {err:?}"
        );
    }

    #[test]
    fn package_without_a_declared_range_is_compatible() {
        assert!(check_compatible(&manifest_with_compat(None), hv("22.0")).is_ok());
    }

    /// A build-level bound must be satisfiable. Evaluating the range against
    /// `major.minor.0` rejected every target the user could name, making the
    /// package impossible to install globally at all.
    #[test]
    fn build_level_compat_bounds_are_satisfiable() {
        let manifest = manifest_with_compat(Some(">=20.5.445, <22"));
        assert!(check_compatible(&manifest, hv("20.5.500")).is_ok());
        assert!(check_compatible(&manifest, hv("20.5")).is_ok());
        assert!(check_compatible(&manifest, hv("20.5.400")).is_err());
        assert!(check_compatible(&manifest, hv("22.0")).is_err());
    }

    /// The ledger is user-editable, and `Path::join` with an absolute or
    /// `..`-bearing component escapes the base directory. `remove` must not
    /// be steerable into deleting a file outside Houdini's packages dir.
    #[test]
    fn only_hpm_owned_bare_filenames_are_deletable() {
        assert_eq!(
            safe_manifest_file_name("hpm-acme.tools.json"),
            Some("hpm-acme.tools.json")
        );

        for hostile in [
            "../../../etc/passwd",
            "../hpm-acme.tools.json",
            "sub/hpm-acme.tools.json",
            "/etc/hpm-evil.json",
            "SideFX_Labs.json",     // not hpm-owned
            "hpm-acme.tools.txt",   // not a manifest
            "hpm-acme.tools.json/", // trailing separator
            "",
            ".",
            "..",
        ] {
            assert_eq!(
                safe_manifest_file_name(hostile),
                None,
                "{hostile:?} must be rejected"
            );
        }
    }

    /// The name hpm writes must itself pass the guard that gates deletion,
    /// or a package could be installed and then never removed.
    #[test]
    fn generated_manifest_names_pass_the_deletion_guard() {
        for name in ["acme/tools", "creator-a/b-c", "x/y0-9"] {
            let file = manifest_file_name(&PackagePath::new(name).unwrap());
            assert_eq!(
                safe_manifest_file_name(&file),
                Some(file.as_str()),
                "hpm wrote {file:?} but would refuse to delete it"
            );
        }
    }
}
