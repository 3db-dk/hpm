//! Tests for pre-0.16 manifest detection and migration.

use super::*;
use crate::env_value::EnvValue;
use crate::manifest::parse_manifest_str;
use crate::platform::Platform;

/// A representative pre-0.16 manifest exercising every changed section.
const LEGACY_FULL: &str = r#"
[package]
path = "studio/my-pkg"
name = "My Pkg"
version = "1.0.0"

[houdini]
min_version = "20.5"
max_version = "21.0"

[env.HOUDINI_DSO_PATH]
method = "prepend"
value = "$HPM_PACKAGE_ROOT/dso"

[dev.env.HOUDINI_DSO_PATH]
method = "prepend"
value = "$HPM_PACKAGE_ROOT/build/Release"

[native]
platforms = ["linux-x86_64"]

[native.linux-x86_64]
files = ["dso/linux-x86_64/*"]

[scripts]
build = "make"

[scripts.platform.windows]
build = "build.bat"
"#;

fn migrate_str(content: &str) -> (PackageManifest, MigrationReport) {
    let (manifest, report) = parse_manifest_str(content).expect("parses");
    (manifest, report.expect("detected as legacy"))
}

#[test]
fn detects_legacy_markers() {
    for marker in [
        "[houdini]\nmin_version = \"20.5\"",
        "[env.FOO]\nmethod = \"set\"\nvalue = \"x\"",
        "[dev.env.FOO]\nmethod = \"set\"\nvalue = \"x\"",
        "[native]\nplatforms = [\"linux-x86_64\"]",
        "[scripts.platform.windows]\nbuild = \"b.bat\"",
    ] {
        let toml =
            format!("[package]\npath = \"a/b\"\nname = \"B\"\nversion = \"1.0.0\"\n{marker}\n");
        let table: toml::Table = toml::from_str(&toml).unwrap();
        assert!(is_legacy(&table), "should flag legacy: {marker}");
    }
}

#[test]
fn current_manifest_is_not_legacy() {
    let current = r#"
[package]
path = "studio/p"
name = "P"
version = "1.0.0"

[compat]
houdini = ">=20.5"
platforms = ["linux-x86_64"]

[runtime.FOO]
method = "set"
value = "bar"

[stage.platform.linux-x86_64]
place = [{ from = "dso/linux-x86_64/*", to = "dso/linux-x86_64/" }]

[scripts.build]
cmd = "make"
"#;
    let table: toml::Table = toml::from_str(current).unwrap();
    assert!(!is_legacy(&table));

    let (_, report) = parse_manifest_str(current).expect("parses");
    assert!(
        report.is_none(),
        "current manifest yields no migration report"
    );
}

#[test]
fn current_script_named_platform_is_not_legacy() {
    // A current table-form script entry that happens to be named `platform`
    // carries a `cmd` key, distinguishing it from the old per-OS table.
    let current = r#"
[package]
path = "studio/p"
name = "P"
version = "1.0.0"

[scripts.platform]
cmd = "echo hi"
"#;
    let table: toml::Table = toml::from_str(current).unwrap();
    assert!(!is_legacy(&table));
}

#[test]
fn houdini_min_max_becomes_range() {
    let (manifest, _) = migrate_str(LEGACY_FULL);
    assert_eq!(
        manifest.compat.houdini.as_ref().map(|r| r.as_str()),
        Some(">=20.5, <21.0")
    );
}

#[test]
fn houdini_min_only_and_max_only() {
    let min_only = LegacyHoudini {
        min_version: Some("20.5".into()),
        max_version: None,
    };
    assert_eq!(houdini_range_string(&min_only).as_deref(), Some(">=20.5"));

    let max_only = LegacyHoudini {
        min_version: None,
        max_version: Some("22".into()),
    };
    assert_eq!(houdini_range_string(&max_only).as_deref(), Some("<22"));
}

#[test]
fn env_and_dev_env_merge_onto_install_source() {
    let (manifest, _) = migrate_str(LEGACY_FULL);
    let entry = manifest.runtime.get("HOUDINI_DSO_PATH").expect("present");
    assert_eq!(entry.method, EnvMethod::Prepend);
    let EnvValue::Conditional(branches) = entry.value.as_ref().unwrap() else {
        panic!("expected conditional value");
    };
    assert_eq!(branches.len(), 2);
    // Dev branch first, gated to install_source = "dev".
    assert_eq!(branches[0].when.install_source.as_deref(), Some("dev"));
    assert_eq!(branches[0].set, "$HPM_PACKAGE_ROOT/build/Release");
    // Base branch is the unconditional fallback.
    assert!(branches[1].when.is_empty());
    assert_eq!(branches[1].set, "$HPM_PACKAGE_ROOT/dso");
}

#[test]
fn dev_only_env_key_is_gated_to_dev() {
    let legacy = r#"
[package]
path = "studio/p"
name = "P"
version = "1.0.0"

[dev.env.ONLY_DEV]
method = "set"
value = "x"
"#;
    let (manifest, _) = migrate_str(legacy);
    let entry = manifest.runtime.get("ONLY_DEV").expect("present");
    let EnvValue::Conditional(branches) = entry.value.as_ref().unwrap() else {
        panic!("expected conditional");
    };
    assert_eq!(branches.len(), 1);
    assert_eq!(branches[0].when.install_source.as_deref(), Some("dev"));
}

#[test]
fn native_becomes_compat_platforms_and_stage_place_rules() {
    let (manifest, report) = migrate_str(LEGACY_FULL);
    assert!(manifest.compat.platforms.contains(&Platform::LinuxX86_64));

    let rules = manifest
        .stage
        .platform
        .entries
        .get("linux-x86_64")
        .expect("stage rules present");
    assert_eq!(rules.place.len(), 1);
    assert_eq!(rules.place[0].from, "dso/linux-x86_64/*");
    assert_eq!(rules.place[0].to, "dso/linux-x86_64/");

    // The lossy place-rule derivation is flagged for review.
    assert!(
        report
            .warnings
            .iter()
            .any(|w| matches!(w, MigrationWarning::ReviewPlaceRule { .. }))
    );
}

#[test]
fn scripts_platform_becomes_conditional_cmd() {
    let (manifest, _) = migrate_str(LEGACY_FULL);
    let entry = manifest.script_for("build").expect("present");
    let ScriptEntry::WithEnv(env) = &entry else {
        panic!("expected table-form entry");
    };
    let EnvValue::Conditional(branches) = &env.cmd else {
        panic!("expected conditional cmd");
    };
    assert_eq!(branches.len(), 2);
    assert_eq!(branches[0].when.os.as_deref(), Some("windows"));
    assert_eq!(branches[0].set, "build.bat");
    assert!(branches[1].when.is_empty());
    assert_eq!(branches[1].set, "make");
}

#[test]
fn migrated_manifest_validates() {
    let (manifest, _) = migrate_str(LEGACY_FULL);
    manifest
        .validate()
        .expect("migrated manifest is structurally valid");
}

#[test]
fn passthrough_sections_survive() {
    let legacy = r#"
[package]
path = "studio/p"
name = "P"
version = "1.0.0"

[houdini]
min_version = "20.5"

[dependencies]
other = { url = "https://example.com/o/1.0.0/o-1.0.0.zip", version = "1.0.0" }

[python_dependencies]
numpy = ">=1.20.0"
"#;
    let (manifest, _) = migrate_str(legacy);
    assert!(manifest.dependencies.contains_key("other"));
    assert!(manifest.python_dependencies.contains_key("numpy"));
}

#[test]
fn derive_place_to_cases() {
    assert_eq!(derive_place_to("dso/linux-x86_64/*"), "dso/linux-x86_64/");
    assert_eq!(derive_place_to("dso/**"), "dso/");
    assert_eq!(derive_place_to("lib/foo.dll"), "lib/");
    assert_eq!(derive_place_to("*.dll"), "./");
    assert_eq!(derive_place_to("foo.dll"), "./");
}

#[test]
fn from_path_reads_legacy_transparently() {
    // Proves install/run/pack (which all go through `from_path`) accept an
    // old-format manifest on disk with no caller changes.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("hpm.toml");
    std::fs::write(&path, LEGACY_FULL).unwrap();

    let manifest = PackageManifest::from_path(&path).expect("legacy manifest loads");
    assert_eq!(
        manifest.compat.houdini.as_ref().map(|r| r.as_str()),
        Some(">=20.5, <21.0")
    );

    let (_, report) = PackageManifest::from_path_migrating(&path).unwrap();
    assert!(report.is_some(), "from_path_migrating surfaces the report");
}

#[test]
fn env_method_mismatch_is_reported() {
    let legacy = r#"
[package]
path = "studio/p"
name = "P"
version = "1.0.0"

[env.VAR]
method = "prepend"
value = "base"

[dev.env.VAR]
method = "append"
value = "dev"
"#;
    let (_, report) = migrate_str(legacy);
    assert!(
        report
            .warnings
            .iter()
            .any(|w| matches!(w, MigrationWarning::EnvMethodMismatch { .. }))
    );
}
