//! Houdini `packages/` manifest emission: per-package `<slug>.json` files,
//! the project `[runtime]` overrides manifest, and the stale-manifest sweep.

use crate::storage::InstalledPackage;
use hpm_package::{EnvMethod, EnvValue, HoudiniPackage, IoOp, ManifestEnvEntry};
use indexmap::IndexMap;
use std::collections::HashMap;
use std::path::Path;
use tracing::{debug, info, warn};

use super::{ProjectError, ProjectManager};

/// File name of the project-level `[runtime]` overrides manifest, written
/// into the project packages dir alongside the per-package `<slug>.json`
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

        for entry in entries.flatten() {
            let path = entry.path();
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
        let houdini_package = self.create_houdini_package_with_python(
            installed_package,
            venv_site_packages,
            project_env_overrides,
        )?;
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

            let lowered =
                entry
                    .lower(&[], false)
                    .map_err(|e| ProjectError::InvalidEnvExpression {
                        var: key.clone(),
                        package: "the project's hpm.toml [runtime]".to_string(),
                        message: e.to_string(),
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

    #[cfg(test)]
    pub(super) fn create_houdini_package(
        &self,
        installed_package: &InstalledPackage,
    ) -> Result<HoudiniPackage, ProjectError> {
        self.create_houdini_package_with_python(installed_package, None, &IndexMap::new())
    }

    pub(super) fn create_houdini_package_with_python(
        &self,
        installed_package: &InstalledPackage,
        venv_site_packages: Option<&Path>,
        project_env_overrides: &IndexMap<String, ManifestEnvEntry>,
    ) -> Result<HoudiniPackage, ProjectError> {
        build_houdini_package(installed_package, venv_site_packages, project_env_overrides)
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
    {
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
                hpm_package::HoudiniEnvValue::prepend(
                    package_path.join("python").to_string_lossy(),
                ),
            );
            env.push(python_env);
        }

        // Scripts path
        if package_path.join("scripts").exists() {
            let mut scripts_env = HashMap::new();
            scripts_env.insert(
                "HOUDINI_SCRIPT_PATH".to_string(),
                hpm_package::HoudiniEnvValue::prepend(
                    package_path.join("scripts").to_string_lossy(),
                ),
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
            let is_dev = installed_package.is_dev;

            // Lower one entry and, unless it is inert in this install
            // context (every variant install-source-filtered out), push it
            // onto `env` under `key`.
            let emit = |key: &str,
                        entry: &ManifestEnvEntry,
                        env: &mut Vec<HashMap<String, hpm_package::HoudiniEnvValue>>|
             -> Result<(), ProjectError> {
                let lowered = entry
                    .lower(&[("$HPM_PACKAGE_ROOT", &pkg_root)], is_dev)
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
}

impl ProjectManager {
    pub fn generate_houdini_manifests(&self) -> Result<(), ProjectError> {
        info!("Regenerating all Houdini manifests");

        let dependencies = self.list_dependencies()?;
        let project_env_overrides = self
            .load_project_manifest()?
            .map(|m| m.runtime)
            .unwrap_or_default();

        for dep in dependencies {
            if let Some(installed_package) = dep.installed_package {
                self.generate_houdini_manifest_with_python(
                    &installed_package,
                    None,
                    &project_env_overrides,
                )?;
            }
        }
        self.write_project_overrides_manifest(&project_env_overrides)?;

        info!("Successfully regenerated all Houdini manifests");
        Ok(())
    }
}
