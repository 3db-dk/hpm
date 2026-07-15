//! Property test of the package.json emission layer against the modeled
//! (verified-real) Houdini env semantics in `houdini_env_model`.
//!
//! Prior coverage asserted the emitted JSON and *assumed* Houdini honors
//! `method` on it — the assumption that made the 0.19.0 override fix look
//! correct while flat-string custom vars were being silently overwritten.
//! Here the emitted files are instead run through a model of what Houdini
//! actually does, so the properties hold end to end:
//!
//! - no package value is lost under `append` / `prepend` project overrides;
//! - a project override is applied exactly once, however many packages
//!   declare the variable, and lands at the correct end;
//! - a `set` override replaces every package contribution;
//! - `append` / `prepend` entries are list-valued (mergeable), while a
//!   plain `set` is emitted flat so it overwrites even a path-registered
//!   variable; both are Houdini-accepted shapes (the model panics
//!   otherwise).

use super::houdini_env_model::{VarState, apply_package_files};
use super::tests::test_setup;
use super::*;
use hpm_package::{EnvMethod, ManifestEnvEntry, PackagePath};
use indexmap::IndexMap;
use proptest::prelude::*;
use tempfile::TempDir;

/// Slugs chosen so their byte-wise file order (= Houdini's processing
/// order) matches their index order, with the overrides file after all.
const SLUGS: [&str; 3] = ["alpha-tools", "midway-kit", "zulu-rig"];
const VARS: [&str; 3] = ["HPMT_SHARED_PATH", "HPMT_ASSET_ROOT", "HPMT_FLAG_LIST"];

fn method_from(index: u8) -> EnvMethod {
    match index % 3 {
        0 => EnvMethod::Set,
        1 => EnvMethod::Prepend,
        _ => EnvMethod::Append,
    }
}

fn pkg_value(slug: &str, var: &str) -> String {
    format!("v-{slug}-{var}")
}

fn override_value(var: &str) -> String {
    format!("ov-{var}")
}

proptest! {
    #[test]
    fn prop_emission_respects_real_houdini_semantics(
        // Per (var, package): None = not declared, Some(m) = declared with
        // method m. Per var: optional project override method.
        declarations in prop::collection::vec(
            prop::collection::vec(prop::option::of(0u8..3), SLUGS.len()),
            VARS.len(),
        ),
        overrides in prop::collection::vec(prop::option::of(0u8..3), VARS.len()),
    ) {
        let temp_dir = TempDir::new().unwrap();
        let (config, storage_manager) = test_setup(temp_dir.path());
        let project_root = temp_dir.path().join("project");
        std::fs::create_dir_all(&project_root).unwrap();
        let pm = ProjectManager::new(project_root, storage_manager, config).unwrap();

        // A package-side `set` is emitted as a flat, non-mergeable string
        // (it must overwrite, so a path-registered variable like OCIO is
        // replaced rather than appended onto Houdini's seed). That makes
        // "no value is lost" false by design in two situations: when the
        // variable is shared by several packages (a later `set` resets the
        // earlier ones), and when a `prepend` / `append` project override
        // tries to merge into it (the flat value is non-mergeable, so the
        // override overwrites it). Both are legitimate `set` semantics but
        // outside the survival property, so those `set`s are fixed up to
        // `append` here; a solitary `set` with no merging override still
        // exercises the flat-overwrite path.
        let mut decls = declarations.clone();
        for (var_index, var_decls) in decls.iter_mut().enumerate() {
            let declarers = var_decls.iter().flatten().count();
            let has_merge_override = matches!(
                overrides[var_index].map(method_from),
                Some(EnvMethod::Prepend | EnvMethod::Append)
            );
            if declarers > 1 || has_merge_override {
                for method in var_decls.iter_mut().flatten() {
                    if method_from(*method) == EnvMethod::Set {
                        *method = 2; // append
                    }
                }
            }
        }

        // Project overrides table.
        let mut project_overrides: IndexMap<String, ManifestEnvEntry> = IndexMap::new();
        for (var_index, over) in overrides.iter().enumerate() {
            if let Some(method) = over {
                project_overrides.insert(
                    VARS[var_index].to_string(),
                    ManifestEnvEntry {
                        method: method_from(*method),
                        value: Some(override_value(VARS[var_index]).into()),
                        required: false,
                    },
                );
            }
        }

        // Emit one HoudiniPackage per slug that declares anything.
        let mut files: Vec<(String, serde_json::Value)> = Vec::new();
        for (slug_index, slug) in SLUGS.iter().enumerate() {
            let mut runtime = IndexMap::new();
            for (var_index, var) in VARS.iter().enumerate() {
                if let Some(method) = decls[var_index][slug_index] {
                    runtime.insert(
                        var.to_string(),
                        ManifestEnvEntry {
                            method: method_from(method),
                            value: Some(pkg_value(slug, var).into()),
                            required: false,
                        },
                    );
                }
            }
            if runtime.is_empty() {
                continue;
            }

            let mut manifest = hpm_package::PackageManifest::new(
                PackagePath::new(format!("studio/{slug}")).unwrap(),
                slug.to_string(),
                "1.0.0".to_string(),
                None,
                Vec::new(),
                None,
            );
            manifest.runtime = runtime;

            let install_path = temp_dir.path().join(format!("{slug}@1.0.0"));
            std::fs::create_dir_all(&install_path).unwrap();
            let installed = InstalledPackage {
                version: "1.0.0".to_string(),
                manifest,
                install_path,
                is_dev: false,
            };

            let pkg = pm
                .create_houdini_package_with_python(&installed, None, &project_overrides)
                .unwrap();
            files.push((format!("{slug}.json"), serde_json::to_value(&pkg).unwrap()));
        }

        if let Some(overrides_pkg) =
            ProjectManager::build_project_overrides_package(&project_overrides).unwrap()
        {
            files.push((
                PROJECT_OVERRIDES_FILE.to_string(),
                serde_json::to_value(&overrides_pkg).unwrap(),
            ));
        }

        // Run the emitted files through the verified Houdini model.
        let resolved = apply_package_files(&files);

        for (var_index, var) in VARS.iter().enumerate() {
            let declared_values: Vec<String> = SLUGS
                .iter()
                .enumerate()
                .filter(|(slug_index, _)| decls[var_index][*slug_index].is_some())
                .map(|(_, slug)| pkg_value(slug, var))
                .collect();
            let over_method = overrides[var_index].map(method_from);
            let over_value = override_value(var);

            let Some(state) = resolved.get(*var) else {
                prop_assert!(
                    declared_values.is_empty() && over_method.is_none(),
                    "{var}: declared or overridden but absent from the resolved set"
                );
                continue;
            };

            // A plain `set` is emitted flat (non-mergeable) on purpose — it
            // overwrites, which is what makes it correct for a
            // path-registered variable. That happens when the project
            // overrides with `set`, or (no override) a lone package declares
            // it with `set`; the declarers>1 fixup above rewrites shared
            // `set`s to `append`, so a surviving `set` is always solitary.
            // Everything else must stay list-valued (mergeable).
            let surviving_methods: Vec<EnvMethod> = (0..SLUGS.len())
                .filter_map(|slug_index| decls[var_index][slug_index].map(method_from))
                .collect();
            let flat_expected = match over_method {
                Some(EnvMethod::Set) => true,
                None => surviving_methods == [EnvMethod::Set],
                _ => false,
            };
            if !flat_expected {
                prop_assert!(
                    matches!(state, VarState::List(_)),
                    "{var}: emitted as a non-mergeable flat value: {state:?}"
                );
            }
            let elements = state.elements();

            let count = |needle: &str| elements.iter().filter(|e| *e == needle).count();

            match over_method {
                // `set` replaces every package contribution.
                Some(EnvMethod::Set) => {
                    prop_assert_eq!(
                        &elements,
                        &vec![over_value.clone()],
                        "{}: set override must replace wholesale",
                        var
                    );
                }
                Some(method @ (EnvMethod::Append | EnvMethod::Prepend)) => {
                    // The override is applied exactly once, at the right end.
                    prop_assert_eq!(
                        count(&over_value),
                        1,
                        "{}: override must be applied exactly once",
                        var
                    );
                    let expected_position = match method {
                        EnvMethod::Append => elements.last(),
                        _ => elements.first(),
                    };
                    prop_assert_eq!(
                        expected_position,
                        Some(&over_value),
                        "{}: override must land at the {} end",
                        var,
                        if method == EnvMethod::Append { "trailing" } else { "leading" }
                    );
                    // No package value is lost, and none is duplicated.
                    for value in &declared_values {
                        prop_assert_eq!(
                            count(value),
                            1,
                            "{}: package value {} lost or duplicated",
                            var,
                            value
                        );
                    }
                }
                None => {
                    for value in &declared_values {
                        prop_assert_eq!(
                            count(value),
                            1,
                            "{}: package value {} lost or duplicated",
                            var,
                            value
                        );
                    }
                    prop_assert_eq!(count(&over_value), 0);
                }
            }
        }
    }
}
