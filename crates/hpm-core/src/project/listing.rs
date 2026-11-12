//! Which installed copy each of a project's dependencies uses.

use crate::storage::InstalledPackage;
use hpm_package::IoOp;
use std::path::{Path, PathBuf};

use super::{ProjectDependency, ProjectError, ProjectManager, houdini_emit};

impl ProjectManager {
    /// The project's dependencies, each with the installed copy it uses.
    ///
    /// The copy is the install the package's emitted Houdini manifest
    /// points `hpath` at, which is what Houdini loads: for a path dependency
    /// that is its dev install under `_dev/`, which the store listing never
    /// shows. Without a readable `hpath`, it is the version the project's
    /// manifest names, or the highest installed when it names none; a path
    /// dependency then has none. Choosing by version alone handed a dev
    /// launch's TumbleTrove Desktop the newest *registry* TumblePipe
    /// (1.88.1) for a 1.89.0 dev override, so it ran that release's
    /// `tt_prepare`, whose migrations stopped a layout short of what the
    /// dev build reads. Before that it was whichever version of the
    /// package the store scan met first: a project on TumblePipe 1.76.0
    /// with 1.40.1 also in the store was handed 1.40.1, so TumbleTrove
    /// Desktop ran that release's `tt_*` hooks (or none, as it declared
    /// none), and manifest generation could point Houdini at it.
    pub fn list_dependencies(&self) -> Result<Vec<ProjectDependency>, ProjectError> {
        let mut dependencies = vec![];

        if !self.project_paths.packages_dir.exists() {
            return Ok(dependencies);
        }

        let entries = std::fs::read_dir(&self.project_paths.packages_dir).map_err(|e| {
            IoOp::wrap(
                "read project packages directory",
                &self.project_paths.packages_dir,
                e,
            )
        })?;

        let installed_packages = self.storage_manager.list_installed()?;
        let declared = self
            .load_project_manifest()?
            .map(|m| m.dependencies)
            .unwrap_or_default();

        for entry in entries {
            let path = entry
                .map_err(|e| {
                    IoOp::wrap(
                        "read project packages directory",
                        &self.project_paths.packages_dir,
                        e,
                    )
                })?
                .path();
            let Some(file_name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            // The overrides manifest is not a per-package file.
            if file_name == houdini_emit::PROJECT_OVERRIDES_FILE {
                continue;
            }
            let Some(stem) = file_name.strip_suffix(".json") else {
                continue;
            };
            // Manifests are named `<creator>.<slug>.json`. Anything else is a
            // legacy bare-slug file or someone else's — the stale sweep deals
            // with those; reporting them as dependencies would attach a name
            // that matches no installed package.
            let Some(package_path) = hpm_package::PackagePath::from_file_stem(stem) else {
                continue;
            };

            // The install the emitted manifest points Houdini at is the one
            // the project uses, dev installs included. Only without it (an
            // unreadable or hand-written file) is the copy chosen by version.
            let spec = declared.get(package_path.as_str());
            let emitted = emitted_install_dir(&path)
                .and_then(|dir| self.storage_manager.installed_at(&dir))
                .filter(|p| p.manifest.package.path == package_path);
            let installed_package = match (emitted, spec) {
                (Some(pkg), _) => Some(pkg),
                // A path dependency's copy lives under `_dev/`, which the
                // store listing hides: any registry copy found there is a
                // different build, so report none rather than that one.
                (None, Some(hpm_package::DependencySpec::Path { .. })) => None,
                (None, spec) => {
                    let pinned = spec.and_then(|spec| match spec {
                        hpm_package::DependencySpec::Registry { version, .. }
                        | hpm_package::DependencySpec::Url { version, .. } => {
                            Some(version.as_str())
                        }
                        hpm_package::DependencySpec::Path { .. } => None,
                    });
                    installed_copy(&installed_packages, &package_path, pinned).cloned()
                }
            };

            dependencies.push(ProjectDependency {
                name: package_path.as_str().to_string(),
                installed_package,
            });
        }

        Ok(dependencies)
    }
}

/// The install directory an emitted Houdini package manifest (`<creator>.
/// <slug>.json`) points at: its first `hpath` entry, written by
/// [`houdini_emit::build_houdini_package`] as the install's root. `None`
/// for a file that cannot be read or has no `hpath`.
fn emitted_install_dir(manifest: &Path) -> Option<PathBuf> {
    houdini_emit::hpaths(manifest).ok()?.into_iter().next()
}

/// The installed copy of `path` a project uses: the `pinned` version when
/// the manifest names one, else the highest version installed. `None` when
/// the pinned version is not in the store: another version would be a
/// different package from the one the project installs.
fn installed_copy<'a>(
    installed: &'a [InstalledPackage],
    path: &hpm_package::PackagePath,
    pinned: Option<&str>,
) -> Option<&'a InstalledPackage> {
    let mut copies = installed
        .iter()
        .filter(|p| &p.manifest.package.path == path);
    match pinned {
        Some(version) => copies.find(|p| p.manifest.package.version == version),
        None => copies.max_by(|a, b| {
            match (
                semver::Version::parse(&a.manifest.package.version),
                semver::Version::parse(&b.manifest.package.version),
            ) {
                (Ok(x), Ok(y)) => x.cmp(&y),
                _ => a.manifest.package.version.cmp(&b.manifest.package.version),
            }
        }),
    }
}
