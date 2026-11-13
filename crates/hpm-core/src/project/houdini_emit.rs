//! Houdini `packages/` manifest emission: per-package `<creator>.<slug>.json` files,
//! the project `[runtime]` overrides manifest, and the stale-manifest sweep.

use crate::storage::InstalledPackage;
use hpm_package::{EnvMethod, EnvValue, HoudiniPackage, InstallSource, IoOp, ManifestEnvEntry};
use indexmap::IndexMap;
use std::collections::HashMap;
use std::path::Path;
use tracing::{debug, warn};

use super::{ProjectError, ProjectManager};

/// The `hpath` entries of the Houdini package file at `manifest`, in order,
/// accepting both forms Houdini reads: a string and an array of strings.
/// Empty when the file has no `hpath`. An unreadable file is an
/// [`std::io::Error`]; one that is not JSON is one of kind `InvalidData`.
pub(crate) fn hpaths(manifest: &Path) -> std::io::Result<Vec<std::path::PathBuf>> {
    let text = std::fs::read_to_string(manifest)?;
    let json: serde_json::Value = serde_json::from_str(&text)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let as_path = |v: &serde_json::Value| v.as_str().map(std::path::PathBuf::from);
    Ok(match json.get("hpath") {
        Some(serde_json::Value::Array(items)) => items.iter().filter_map(as_path).collect(),
        Some(v) => as_path(v).into_iter().collect(),
        None => Vec::new(),
    })
}

/// Record that the Houdini manifest at `manifest_path` points at the dev copy
/// at `install_path`. A record that cannot be written fails the sync: the
/// project's claim is what keeps a reclaim from removing the copy while the
/// manifest still points at it, and a missing one only shows later, as a
/// package gone from under a running Houdini. The install's hold on the copy
/// is left in place then, so it stays protected in this process.
pub(super) fn record_project_claim(
    install_path: &Path,
    manifest_path: &Path,
) -> Result<(), ProjectError> {
    crate::storage::dev_users::record_project_user(install_path, manifest_path).map_err(|e| {
        hpm_package::IoOp::wrap("record a project's claim on dev copy", install_path, e)
    })?;
    Ok(())
}

/// File name of the project-level `[runtime]` overrides manifest, written
/// into the project packages dir alongside the per-package `<creator>.<slug>.json`
/// files.
///
/// Houdini applies env entries from the package files in a directory in
/// byte-wise ascending filename order, and `~` (0x7E) sorts after every
/// character allowed in a package slug (`[a-z0-9-]`), so this file is
/// always processed last: project overrides merge after — or replace —
/// every package contribution, and are applied exactly once no matter how
/// many packages declare the same variable. (Emitting the override into
/// each declaring package's file, as hpm did before, applies it once per
/// declaring package.) Ordering verified against Houdini 21.0.688.
pub const PROJECT_OVERRIDES_FILE: &str = "~hpm-project-overrides.json";

impl ProjectManager {
    /// Remove `.json` files in the project's packages dir whose
    /// `<creator>.<slug>` stem is not in `installed_packages`. Non-`.json`
    /// entries are left alone, as is the project overrides manifest.
    ///
    /// This directory belongs to hpm, so an unrecognized `.json` is treated
    /// as stale output rather than someone else's file — that includes
    /// manifests written by an older hpm under the bare `<slug>.json` name,
    /// which is how those get migrated. `hpm global` writes into a directory
    /// hpm does *not* own and deliberately has no equivalent sweep.
    pub(super) fn sweep_stale_houdini_manifests(
        &self,
        installed_packages: &[InstalledPackage],
    ) -> Result<(), ProjectError> {
        let packages_dir = &self.project_paths.packages_dir;
        if !packages_dir.exists() {
            return Ok(());
        }

        let valid_stems: std::collections::HashSet<String> = installed_packages
            .iter()
            .map(|pkg| pkg.manifest.package.path.file_stem())
            .collect();

        let entries = std::fs::read_dir(packages_dir)
            .map_err(|e| IoOp::wrap("read project packages directory", packages_dir, e))?;

        for entry in entries {
            let path = entry
                .map_err(|e| IoOp::wrap("read project packages directory", packages_dir, e))?
                .path();
            if !path.is_file() {
                continue;
            }
            let file_name = match path.file_name().and_then(|n| n.to_str()) {
                Some(name) => name,
                None => continue,
            };
            // The project overrides manifest is not a per-package file;
            // write_project_overrides_manifest owns its lifecycle.
            if file_name == PROJECT_OVERRIDES_FILE {
                continue;
            }
            let stem = match file_name.strip_suffix(".json") {
                Some(stem) => stem,
                None => continue,
            };
            if valid_stems.contains(stem) {
                continue;
            }

            match std::fs::remove_file(&path) {
                Ok(()) => debug!("Removed stale Houdini manifest: {}", path.display()),
                Err(e) => {
                    // Don't fail the whole sync if one stale manifest can't be
                    // removed (e.g. Houdini holds it open on Windows). Surface
                    // it so the user can act.
                    warn!(
                        "Failed to remove stale Houdini manifest {}: {}",
                        path.display(),
                        e
                    );
                }
            }
        }

        Ok(())
    }

    pub(super) fn generate_houdini_manifest_with_python(
        &self,
        installed_package: &InstalledPackage,
        venv_site_packages: Option<&Path>,
        project_env_overrides: &IndexMap<String, ManifestEnvEntry>,
    ) -> Result<(), ProjectError> {
        let houdini_package =
            build_houdini_package(installed_package, venv_site_packages, project_env_overrides)?;
        let manifest_path = self
            .project_paths
            .package_manifest_path(&installed_package.manifest.package.path);

        let content = serde_json::to_vec_pretty(&houdini_package).map_err(|source| {
            ProjectError::HoudiniManifestSerialize {
                path: manifest_path.clone(),
                source,
            }
        })?;
        hpm_package::atomic_write(&manifest_path, content)?;

        // The manifest now points `hpath` at this install. For a dev copy,
        // record that this project uses it, then release the hold the install
        // took until this moment (see `storage::dev_users`).
        if installed_package.is_dev {
            record_project_claim(&installed_package.install_path, &manifest_path)?;
            crate::storage::dev_users::settle(&installed_package.install_path);
        }

        debug!(
            "Generated Houdini manifest for {}",
            installed_package.manifest.package.slug()
        );
        Ok(())
    }

    /// Build the Houdini package carrying the project's `[runtime]` entries,
    /// destined for [`PROJECT_OVERRIDES_FILE`]. Returns `None` when nothing
    /// survives lowering (empty table, valueless placeholders, or every
    /// conditional branch filtered out) — the caller removes the file then.
    ///
    /// Entries lower with no substitutions: a project-level value has no
    /// owning package, so `$HPM_PACKAGE_ROOT` is meaningless here and would
    /// pass through for Houdini to expand as an (undefined, hence empty)
    /// variable — warn when one is spotted. `install_source`-conditional
    /// branches filter as a published (non-dev) consumer; that axis gates
    /// package installs and has no project-level meaning.
    pub(super) fn build_project_overrides_package(
        project_env_overrides: &IndexMap<String, ManifestEnvEntry>,
    ) -> Result<Option<HoudiniPackage>, ProjectError> {
        let mut env = vec![];
        for (key, entry) in project_env_overrides {
            let references_package_root = match &entry.value {
                Some(EnvValue::Flat(s)) => s.contains("$HPM_PACKAGE_ROOT"),
                Some(EnvValue::Conditional(branches)) => {
                    branches.iter().any(|b| b.set.contains("$HPM_PACKAGE_ROOT"))
                }
                None => false,
            };
            if references_package_root {
                warn!(
                    "project [runtime] override '{key}' references $HPM_PACKAGE_ROOT, \
                     which is undefined at project level and will expand to an empty string"
                );
            }

            let lowered = entry.lower(&[], InstallSource::Registry).map_err(|e| {
                ProjectError::InvalidEnvExpression {
                    var: key.clone(),
                    package: "the project's hpm.toml [runtime]".to_string(),
                    message: e.to_string(),
                }
            })?;
            if let Some(houdini_value) = lowered {
                let mut env_map = HashMap::new();
                env_map.insert(key.clone(), houdini_value);
                env.push(env_map);
            }
        }

        if env.is_empty() {
            return Ok(None);
        }
        Ok(Some(HoudiniPackage {
            hpath: None,
            env: Some(env),
            enable: None,
            requires: None,
            recommends: None,
        }))
    }

    /// Write [`PROJECT_OVERRIDES_FILE`] from the project's `[runtime]`
    /// table, or remove it when there is nothing to emit. Every path that
    /// (re)generates per-package manifests calls this so the overrides
    /// manifest stays in lockstep.
    pub(super) fn write_project_overrides_manifest(
        &self,
        project_env_overrides: &IndexMap<String, ManifestEnvEntry>,
    ) -> Result<(), ProjectError> {
        let path = self.project_paths.packages_dir.join(PROJECT_OVERRIDES_FILE);
        match Self::build_project_overrides_package(project_env_overrides)? {
            Some(package) => {
                let content = serde_json::to_vec_pretty(&package).map_err(|source| {
                    ProjectError::HoudiniManifestSerialize {
                        path: path.clone(),
                        source,
                    }
                })?;
                hpm_package::atomic_write(&path, content)?;
                debug!("Generated project overrides manifest");
            }
            None => match std::fs::remove_file(&path) {
                Ok(()) => debug!("Removed project overrides manifest (nothing to emit)"),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => {
                    return Err(IoOp::wrap("remove project overrides manifest", &path, e).into());
                }
            },
        }
        Ok(())
    }
}

/// Build the Houdini package description hpm emits for an installed package.
///
/// A free function rather than a method: it reads nothing from the project —
/// only the installed package, the venv path, and the overrides map — and
/// `hpm global` needs the identical output for a package that has no project
/// at all. Keeping one implementation is what guarantees a globally
/// installed package is wired up exactly like a project-installed one.
pub fn build_houdini_package(
    installed_package: &InstalledPackage,
    venv_site_packages: Option<&Path>,
    project_env_overrides: &IndexMap<String, ManifestEnvEntry>,
) -> Result<HoudiniPackage, ProjectError> {
    let package_path = &installed_package.install_path;

    // Point hpath at the package root so Houdini auto-discovers convention
    // subdirs (otls/, desktop/, toolbar/, python_panels/, viewer_states/,
    // python3.11libs/, etc.). See sidefx.com/docs/houdini/ref/plugins.html.
    let hpath = vec![package_path.to_string_lossy().to_string()];

    // Build environment variables
    let mut env = vec![];

    // Inject venv site-packages for packages that declare python_dependencies
    if let Some(site_packages) = venv_site_packages
        && !installed_package.manifest.python_dependencies.is_empty()
    {
        let mut python_env = HashMap::new();
        python_env.insert(
            "PYTHONPATH".to_string(),
            hpm_package::HoudiniEnvValue::prepend(site_packages.to_string_lossy()),
        );
        env.push(python_env);
    }

    // Package's own python/ directory
    if package_path.join("python").exists() {
        let mut python_env = HashMap::new();
        python_env.insert(
            "PYTHONPATH".to_string(),
            hpm_package::HoudiniEnvValue::prepend(package_path.join("python").to_string_lossy()),
        );
        env.push(python_env);
    }

    // Scripts path
    if package_path.join("scripts").exists() {
        let mut scripts_env = HashMap::new();
        scripts_env.insert(
            "HOUDINI_SCRIPT_PATH".to_string(),
            hpm_package::HoudiniEnvValue::prepend(package_path.join("scripts").to_string_lossy()),
        );
        env.push(scripts_env);
    }

    // Append user-defined env vars from [runtime], reconciling each
    // package entry with any project-level [runtime] override of the
    // same key. The override itself is NOT emitted here — it lives in
    // the project overrides manifest (PROJECT_OVERRIDES_FILE), which
    // Houdini processes after every per-package file. Emitting it per
    // declaring package would apply it once per package that declares
    // the var. Per key:
    //
    // * no override — emit the package's own entry.
    // * `set` override — the package's entry is suppressed; the
    //   overrides manifest carries the value (a flat string for a
    //   plain `set`) that would win anyway, since it processes last.
    // * `append` / `prepend` override — the package's entry is
    //   emitted; the overrides manifest merges the project value in
    //   after all package contributions.
    //
    // Each entry's conditional variants are filtered by `is_dev` —
    // branches gated to a non-matching `install_source` drop out, so
    // dev-only contributions never reach a registry consumer's Houdini
    // manifest and registry-only contributions never reach a dev
    // install. A required-but-unsupplied placeholder (no value from the
    // package and none from the project) surfaces as `MissingRequiredEnv`.
    if !installed_package.manifest.runtime.is_empty() {
        let pkg_root = package_path.to_string_lossy().into_owned();
        let user_runtime = &installed_package.manifest.runtime;
        let slug = installed_package.manifest.package.slug().to_string();
        let source = installed_package.install_source();

        // Lower one entry and, unless it is inert in this install
        // context (every variant install-source-filtered out), push it
        // onto `env` under `key`.
        let emit = |key: &str,
                    entry: &ManifestEnvEntry,
                    env: &mut Vec<HashMap<String, hpm_package::HoudiniEnvValue>>|
         -> Result<(), ProjectError> {
            let lowered = entry
                .lower(&[("$HPM_PACKAGE_ROOT", &pkg_root)], source)
                .map_err(|e| ProjectError::InvalidEnvExpression {
                    var: key.to_string(),
                    package: slug.clone(),
                    message: e.to_string(),
                })?;
            if let Some(houdini_value) = lowered {
                let mut env_map = HashMap::new();
                env_map.insert(key.to_string(), houdini_value);
                env.push(env_map);
            }
            Ok(())
        };

        for key in user_runtime.keys() {
            let pkg_entry = user_runtime
                .get(key)
                .expect("key originates from package's [runtime]");
            let project_override = project_env_overrides.get(key);

            match project_override {
                // No project override: emit the package's own entry. A
                // valueless entry here is an unsatisfied required
                // placeholder.
                None => {
                    if pkg_entry.value.is_none() {
                        return Err(ProjectError::MissingRequiredEnv {
                            var: key.clone(),
                            package: slug.clone(),
                        });
                    }
                    emit(key, pkg_entry, &mut env)?;
                }
                // `set` replaces the package's contribution wholesale:
                // suppress the package entry; the overrides manifest
                // carries the project value (emitted flat, so it
                // overwrites even a path-registered variable).
                Some(over) if over.method == EnvMethod::Set => {
                    if over.value.is_none() {
                        return Err(ProjectError::MissingRequiredEnv {
                            var: key.clone(),
                            package: slug.clone(),
                        });
                    }
                }
                // `append` / `prepend` combine with the package value:
                // emit the package's entry; the overrides manifest
                // merges the project value in after it. A valueless
                // package entry (required placeholder) contributes
                // nothing and is satisfied by the project's value.
                Some(over) => {
                    if pkg_entry.value.is_none() && over.value.is_none() {
                        return Err(ProjectError::MissingRequiredEnv {
                            var: key.clone(),
                            package: slug.clone(),
                        });
                    }
                    if pkg_entry.value.is_some() {
                        emit(key, pkg_entry, &mut env)?;
                    }
                }
            }
        }
    }

    // Generate enable condition from [compat].houdini. The range is a
    // `HoudiniRange` newtype that validated at parse time, so emitting
    // the expression is infallible here.
    let enable = installed_package
        .manifest
        .compat
        .houdini
        .as_ref()
        .map(hpm_package::HoudiniRange::to_enable_expression);

    Ok(HoudiniPackage {
        hpath: if hpath.is_empty() { None } else { Some(hpath) },
        env: if env.is_empty() { None } else { Some(env) },
        enable,
        requires: None,
        recommends: None,
    })
}

#[cfg(test)]
mod build_snapshots {
    //! Characterization of the Houdini package JSON hpm writes for an
    //! installed package (project installs and `hpm global`), over the same
    //! fixture corpus as hpm-package's native descriptor snapshots.

    use super::*;
    use hpm_package::PackageManifest;

    const CORPUS: &[&str] = &["minimal", "runtime", "platforms", "deps_scripts"];

    fn render(name: &str) -> String {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../hpm-package/tests/fixtures/emit")
            .join(format!("{name}.toml"));
        let manifest = PackageManifest::from_path(&path).unwrap();
        let slug = manifest.package.slug().to_string();
        let version = manifest.package.version.clone();
        let mut overrides = IndexMap::new();
        overrides.insert(
            "STUDIO_LICENSE".to_string(),
            toml::from_str::<ManifestEnvEntry>("method = \"set\"\nvalue = \"5053@licsrv\"")
                .unwrap(),
        );
        let mut out = String::new();
        for (is_dev, venv) in [
            (false, None),
            (true, Some(Path::new("/venvs/0123abcd/site-packages"))),
        ] {
            let installed = InstalledPackage {
                version: version.clone(),
                manifest: manifest.clone(),
                install_path: std::path::PathBuf::from(format!("/store/{slug}@{version}")),
                is_dev,
            };
            out += &format!("--- is_dev={is_dev} venv={venv:?} overrides=STUDIO_LICENSE\n");
            match build_houdini_package(&installed, venv, &overrides) {
                Ok(pkg) => out += &serde_json::to_string_pretty(&pkg).unwrap(),
                Err(e) => out += &format!("error: {e}"),
            }
            out += "\n";
        }
        out.replace('\\', "/")
    }

    #[test]
    fn houdini_package_json() {
        for name in CORPUS {
            insta::assert_snapshot!(format!("build_{name}"), render(name));
        }
    }
}

#[cfg(test)]
mod hpaths_tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        /// `hpaths` reads back exactly the paths a package file names, in
        /// order, whether `hpath` is an array or (for one path) a string.
        #[test]
        fn hpaths_reads_back_what_was_written(
            paths in prop::collection::vec("/[a-z0-9 ._@-]{1,12}(/[a-z0-9 ._@-]{1,12}){0,3}", 0..4),
            as_string in any::<bool>(),
        ) {
            let dir = tempfile::TempDir::new().unwrap();
            let file = dir.path().join("acme.rig.json");
            let hpath = if as_string && paths.len() == 1 {
                serde_json::json!(paths[0])
            } else {
                serde_json::json!(paths)
            };
            std::fs::write(&file, serde_json::to_vec(&serde_json::json!({ "hpath": hpath, "env": [] })).unwrap()).unwrap();
            let read: Vec<String> = hpaths(&file).unwrap().iter().map(|p| p.to_string_lossy().into_owned()).collect();
            prop_assert_eq!(read, paths);
        }
    }

    #[test]
    fn hpaths_of_a_file_that_is_not_json_is_invalid_data() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("x.json");
        std::fs::write(&file, b"nope").unwrap();
        assert_eq!(
            hpaths(&file).unwrap_err().kind(),
            std::io::ErrorKind::InvalidData
        );
        assert_eq!(
            hpaths(&dir.path().join("missing.json")).unwrap_err().kind(),
            std::io::ErrorKind::NotFound
        );
    }

    /// A claim that cannot be written is an error, not a log line: without
    /// it the reclaim removes the copy while the manifest points at it.
    #[cfg(unix)]
    #[test]
    fn a_claim_that_cannot_be_recorded_fails() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::TempDir::new().unwrap();
        let hash = "a".repeat(64);
        let container = tmp.path().join("_dev").join("rig@1.0.0");
        let copy = container.join(&hash);
        std::fs::create_dir_all(&copy).unwrap();
        let records = container.join(".users").join(&hash);
        std::fs::create_dir_all(&records).unwrap();
        std::fs::set_permissions(&records, std::fs::Permissions::from_mode(0o555)).unwrap();
        let manifest = tmp.path().join("project/.hpm/packages/acme.rig.json");
        let result = super::record_project_claim(&copy, &manifest);
        std::fs::set_permissions(&records, std::fs::Permissions::from_mode(0o755)).unwrap();
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("record a project's claim on dev copy"),
            "{err}"
        );
    }
}
