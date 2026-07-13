//! License-free conformance test of the generated package files against a
//! real Houdini, via `hconfig`.
//!
//! `HOUDINI_PACKAGE_VERBOSE=1 $HFS/bin/hconfig` prints a package log on
//! stderr whose `Resolved variables:` block shows the final value of every
//! variable defined by package files — including custom vars — without
//! needing a license. The test writes a project's package files through
//! the real emission path (per-package manifests plus the project
//! overrides manifest), points `HOUDINI_PACKAGE_DIR` at them, and asserts
//! the values Houdini actually resolves.
//!
//! This exists because the emission layer was validated for years against
//! its own JSON output while Houdini silently ignored `method` on
//! flat-string custom vars; only a real Houdini can keep that assumption
//! honest. The test skips (passing, with a note) when no Houdini
//! installation is found — set `HFS` to point at one explicitly, or rely
//! on the platform-standard install locations.

use super::tests::test_setup;
use super::*;
use hpm_package::{EnvMethod, ManifestEnvEntry, PackagePath};
use indexmap::IndexMap;
use std::collections::HashMap as StdHashMap;
use tempfile::TempDir;

fn hconfig_path(hfs: &Path) -> PathBuf {
    let exe = if cfg!(windows) {
        "hconfig.exe"
    } else {
        "hconfig"
    };
    hfs.join("bin").join(exe)
}

/// Locate a Houdini HFS root: `$HFS` first, then the platform-standard
/// install locations, newest-looking directory first (byte-wise descending
/// name sort — good enough to pick a working install).
fn find_hfs() -> Option<PathBuf> {
    if let Ok(hfs) = std::env::var("HFS") {
        let hfs = PathBuf::from(hfs);
        if hconfig_path(&hfs).exists() {
            return Some(hfs);
        }
    }

    // (scan root, dir-name prefix, path from install dir to HFS)
    let locations: &[(&str, &str, &str)] = if cfg!(target_os = "macos") {
        &[(
            "/Applications/Houdini",
            "Houdini",
            "Frameworks/Houdini.framework/Versions/Current/Resources",
        )]
    } else if cfg!(windows) {
        // The SideFX launcher installs to `C:\Houdini <ver>`; the plain
        // installer defaults to Program Files.
        &[
            (r"C:\", "Houdini ", ""),
            (r"C:\Program Files\Side Effects Software", "Houdini", ""),
        ]
    } else {
        &[("/opt", "hfs", "")]
    };

    for (root, prefix, suffix) in locations {
        let Ok(entries) = std::fs::read_dir(root) else {
            continue;
        };
        let mut names: Vec<String> = entries
            .flatten()
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|name| name.starts_with(prefix))
            .collect();
        names.sort_by(|a, b| b.as_bytes().cmp(a.as_bytes()));

        if let Some(hfs) = names
            .into_iter()
            .map(|name| Path::new(root).join(name).join(suffix))
            .find(|hfs| hconfig_path(hfs).exists())
        {
            return Some(hfs);
        }
    }
    None
}

/// Parse the `Resolved variables:` block of the verbose package log into
/// `name -> elements`. Single-line vars (`    NAME : value`) yield one
/// element; list vars put each element on its own 8-space-indented line.
fn parse_resolved_vars(log: &str) -> StdHashMap<String, Vec<String>> {
    let mut vars = StdHashMap::new();
    let Some(start) = log.find("Resolved variables:") else {
        return vars;
    };
    let mut current: Option<String> = None;
    for line in log[start..].lines().skip(1) {
        if line.starts_with("Loading Info:") {
            break;
        }
        if let Some(element) = line.strip_prefix("        ") {
            let element = element.trim();
            // "&" is Houdini's default-path marker, not a real element.
            if element.is_empty() || element == "&" {
                continue;
            }
            if let Some(name) = &current {
                vars.get_mut(name)
                    .expect("current var was inserted when seen")
                    .push(element.to_string());
            }
        } else if let Some(header) = line.strip_prefix("    ") {
            // Variable names cannot contain ':', so the first ':' splits
            // name from (optional inline) value.
            if let Some((name, value)) = header.split_once(':') {
                let name = name.trim().to_string();
                let value = value.trim();
                let elements = if value.is_empty() {
                    Vec::new()
                } else {
                    vec![value.to_string()]
                };
                vars.insert(name.clone(), elements);
                current = Some(name);
            }
        }
    }
    vars
}

fn runtime_entry(method: EnvMethod, value: &str) -> ManifestEnvEntry {
    ManifestEnvEntry {
        method,
        value: Some(value.into()),
        required: false,
    }
}

/// End-to-end: two packages sharing a variable, plus project overrides in
/// every method, resolved by a real Houdini.
///
/// Regression for the flat-string emission bug: with the pre-0.28 format,
/// Houdini logged `WARNING: var HPMT_CONF_SHARED overwritten` and the
/// project override clobbered both package values.
#[test]
fn houdini_resolves_generated_packages_per_method() {
    let Some(hfs) = find_hfs() else {
        // HPM_REQUIRE_HOUDINI turns the skip into a failure so CI (whose
        // workers all have Houdini) cannot silently lose this coverage.
        assert!(
            std::env::var("HPM_REQUIRE_HOUDINI").is_err(),
            "HPM_REQUIRE_HOUDINI is set but no Houdini installation was found"
        );
        eprintln!(
            "SKIPPED houdini conformance test: no Houdini installation found \
             (set HFS to enable it)"
        );
        return;
    };

    let temp_dir = TempDir::new().unwrap();
    let (config, storage_manager) = test_setup(temp_dir.path());
    let project_root = temp_dir.path().join("project");
    std::fs::create_dir_all(&project_root).unwrap();
    let pm = ProjectManager::new(project_root, storage_manager, config).unwrap();

    // Project overrides: one per method, plus a var with no override and
    // an override for a var no package declares.
    let mut overrides = IndexMap::new();
    overrides.insert(
        "HPMT_CONF_SHARED".to_string(),
        runtime_entry(EnvMethod::Append, "project-shared"),
    );
    overrides.insert(
        "HPMT_CONF_PRE".to_string(),
        runtime_entry(EnvMethod::Prepend, "project-pre"),
    );
    overrides.insert(
        "HPMT_CONF_SET".to_string(),
        runtime_entry(EnvMethod::Set, "project-set"),
    );
    overrides.insert(
        "HPMT_CONF_ONLY".to_string(),
        runtime_entry(EnvMethod::Append, "project-only"),
    );

    // Two packages; both declare HPMT_CONF_SHARED. Slugs sort before the
    // `~`-prefixed overrides file, and alpha-tools before zulu-rig.
    let packages = [
        (
            "alpha-tools",
            vec![
                ("HPMT_CONF_SHARED", EnvMethod::Append, "alpha-shared"),
                ("HPMT_CONF_PRE", EnvMethod::Append, "alpha-pre"),
                ("HPMT_CONF_SET", EnvMethod::Set, "alpha-set"),
            ],
        ),
        (
            "zulu-rig",
            vec![
                ("HPMT_CONF_SHARED", EnvMethod::Append, "zulu-shared"),
                ("HPMT_CONF_SOLO", EnvMethod::Append, "zulu-solo"),
            ],
        ),
    ];
    // Conditional values, on alpha-tools only. Both have a matching first
    // branch and an unconditional fallback: exactly one element must fire.
    // (Houdini applies every matching conditional-array element, so the
    // emitted branches carry compiled mutual exclusion; and the fallback
    // must not use the broken `{"true": ...}` encoding, which defines a
    // stray variable named `true` instead.)
    let current_os = if cfg!(target_os = "macos") {
        hpm_package::OsKey::Macos
    } else if cfg!(windows) {
        hpm_package::OsKey::Windows
    } else {
        hpm_package::OsKey::Linux
    };
    let conditional_entry =
        |method: EnvMethod, when: hpm_package::Condition, matched: &str| ManifestEnvEntry {
            method,
            value: Some(hpm_package::EnvValue::Conditional(vec![
                hpm_package::EnvValueBranch {
                    when,
                    set: matched.to_string(),
                },
                hpm_package::EnvValueBranch {
                    when: hpm_package::Condition::default(),
                    set: "fallback-must-not-fire".to_string(),
                },
            ])),
            required: false,
        };
    let version_gated = conditional_entry(
        EnvMethod::Append,
        hpm_package::Condition {
            houdini: Some(hpm_package::HoudiniRange::parse(">=20").unwrap()),
            ..Default::default()
        },
        "version-matched",
    );
    let os_gated = conditional_entry(
        EnvMethod::Append,
        hpm_package::Condition {
            os: Some(current_os),
            ..Default::default()
        },
        "os-matched",
    );

    for (slug, entries) in packages {
        let mut manifest = PackageManifest::new(
            PackagePath::new(format!("studio/{slug}")).unwrap(),
            slug.to_string(),
            "1.0.0".to_string(),
            None,
            Vec::new(),
            None,
        );
        let mut runtime = IndexMap::new();
        for (var, method, value) in entries {
            runtime.insert(var.to_string(), runtime_entry(method, value));
        }
        if slug == "alpha-tools" {
            runtime.insert("HPMT_CONF_VERGATE".to_string(), version_gated.clone());
            runtime.insert("HPMT_CONF_OSGATE".to_string(), os_gated.clone());
        }
        manifest.runtime = runtime;

        let install_path = temp_dir.path().join(format!("{slug}@1.0.0"));
        std::fs::create_dir_all(&install_path).unwrap();
        let installed = InstalledPackage {
            version: "1.0.0".to_string(),
            manifest,
            install_path,
            is_dev: false,
        };
        pm.generate_houdini_manifest_with_python(&installed, None, &overrides)
            .unwrap();
    }
    pm.write_project_overrides_manifest(&overrides).unwrap();

    // hconfig writes the verbose package log to stderr. Its exit status is
    // unreliable (broken help-python setups make it exit non-zero after
    // the log is complete), so key off the log content instead.
    let output = std::process::Command::new(hconfig_path(&hfs))
        .env("HFS", &hfs)
        .env("HOUDINI_PACKAGE_DIR", &pm.project_paths.packages_dir)
        .env("HOUDINI_PACKAGE_VERBOSE", "1")
        .output()
        .expect("failed to spawn hconfig");
    let log = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        log.contains("Resolved variables:"),
        "hconfig produced no package log; output:\n{log}"
    );

    // The emitted format must not trip either failure mode of the old
    // flat-string emission.
    assert!(
        !log.contains("Unsupported method value"),
        "Houdini rejected an emitted method:\n{log}"
    );
    // Serialized nulls for absent fields draw this per generated file.
    assert!(
        !log.contains("Unsupported value for"),
        "Houdini rejected an emitted field value:\n{log}"
    );
    for var in [
        "HPMT_CONF_SHARED",
        "HPMT_CONF_PRE",
        "HPMT_CONF_SET",
        "HPMT_CONF_SOLO",
        "HPMT_CONF_ONLY",
    ] {
        assert!(
            !log.contains(&format!("var {var} overwritten")),
            "{var} was silently overwritten instead of merged:\n{log}"
        );
    }

    let vars = parse_resolved_vars(&log);
    let resolved = |name: &str| -> Vec<String> {
        vars.get(name)
            .unwrap_or_else(|| panic!("{name} missing from resolved variables:\n{log}"))
            .clone()
    };

    // append override: package values in file order, override once, last.
    assert_eq!(
        resolved("HPMT_CONF_SHARED"),
        ["alpha-shared", "zulu-shared", "project-shared"]
    );
    // prepend override: override once, first.
    assert_eq!(resolved("HPMT_CONF_PRE"), ["project-pre", "alpha-pre"]);
    // set override: replaces the package value wholesale.
    assert_eq!(resolved("HPMT_CONF_SET"), ["project-set"]);
    // no override: the package value survives untouched.
    assert_eq!(resolved("HPMT_CONF_SOLO"), ["zulu-solo"]);
    // override of an undeclared var: defined by the overrides manifest.
    assert_eq!(resolved("HPMT_CONF_ONLY"), ["project-only"]);

    // Conditional values: exactly the first matching branch fires — the
    // unconditional fallback is excluded by the compiled negation. The
    // os-gated case also proves compile_os's identifier matches Houdini's
    // houdini_os on this platform.
    assert_eq!(resolved("HPMT_CONF_VERGATE"), ["version-matched"]);
    assert_eq!(resolved("HPMT_CONF_OSGATE"), ["os-matched"]);
    // The broken always-true encoding defined a variable literally named
    // `true`; make sure nothing does that anymore.
    assert!(
        !vars.contains_key("true"),
        "a stray variable named 'true' was defined:\n{log}"
    );
}
