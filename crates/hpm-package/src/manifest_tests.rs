use super::*;

fn make_manifest() -> PackageManifest {
    PackageManifest::new(
        PackagePath::new("studio/test").unwrap(),
        "Test".to_string(),
        "1.0.0".to_string(),
        None,
        Vec::new(),
        None,
    )
}

#[test]
fn strict_rejects_empty_name_and_version() {
    // Both `name` and `version` empty: independent concerns, so the
    // strict pass should collect both errors in one report rather than
    // short-circuiting on the first.
    let mut m = make_manifest();
    m.package.name = String::new();
    m.package.version = String::new();
    let report = m.validate_with(ValidationLevel::Strict);
    assert!(!report.is_ok());
    assert!(
        report.errors.iter().any(|e| e.contains("name")),
        "missing name error: {:?}",
        report.errors
    );
    assert!(
        report.errors.iter().any(|e| e.contains("version")),
        "missing version error: {:?}",
        report.errors
    );
}

#[test]
fn parses_operators_array() {
    let toml = r#"
[package]
path = "studio/test"
name = "Test"
version = "1.0.0"

[[operators]]
kind = "hda"
type_name = "studio::rbd_configure::2.0"
label = "RBD Configure"
category = "Sop"
tab_submenu = "Studio/Dynamics"
icon = "SOP_rbd"
source = "otls/rbd.hda"

[[operators]]
kind = "dso"
type_name = "studio::fast_scatter"
category = "Sop"
"#;
    let m = parse_manifest_str(toml).unwrap();
    assert_eq!(m.operators.len(), 2);
    assert_eq!(m.operators[0].kind, OperatorKind::Hda);
    assert_eq!(m.operators[0].type_name, "studio::rbd_configure::2.0");
    assert_eq!(m.operators[0].label.as_deref(), Some("RBD Configure"));
    assert_eq!(m.operators[0].category, "Sop");
    assert_eq!(
        m.operators[0].tab_submenu.as_deref(),
        Some("Studio/Dynamics")
    );
    assert_eq!(m.operators[0].icon.as_deref(), Some("SOP_rbd"));
    assert_eq!(
        m.operators[0].source,
        Some(OperatorSource::Single("otls/rbd.hda".to_string()))
    );
    // Optional fields default to None.
    assert_eq!(m.operators[1].kind, OperatorKind::Dso);
    assert_eq!(m.operators[1].label, None);
    assert_eq!(m.operators[1].source, None);
    assert_eq!(m.operators[1].tab_submenu, None);
}

#[test]
fn parses_per_platform_operator_source() {
    let toml = r#"
[package]
path = "studio/test"
name = "Test"
version = "1.0.0"

[compat]
platforms = ["linux-x86_64", "macos-aarch64"]

[[operators]]
kind = "dso"
type_name = "studio::fast_scatter"
category = "Sop"
source = { linux-x86_64 = "dso/linux-x86_64/scatter.so", macos-aarch64 = "dso/macos-aarch64/scatter.dylib" }
"#;
    let m = parse_manifest_str(toml).unwrap();
    assert!(m.validate().is_ok(), "{:?}", m.validate());
    let resolved = m.operators[0].resolved_source(Some(&Platform::LinuxX86_64));
    assert_eq!(
        resolved,
        SourceResolution::Path("dso/linux-x86_64/scatter.so")
    );
    // A platform not in the table is not shipped for that operator.
    let win = m.operators[0].resolved_source(Some(&Platform::WindowsX86_64));
    assert_eq!(win, SourceResolution::NotForPlatform);
}

#[test]
fn strict_rejects_per_platform_source_key_not_in_compat() {
    let toml = r#"
[package]
path = "studio/test"
name = "Test"
version = "1.0.0"

[compat]
platforms = ["linux-x86_64"]

[[operators]]
kind = "dso"
type_name = "studio::fast_scatter"
category = "Sop"
source = { macos-aarch64 = "dso/macos-aarch64/scatter.dylib" }
"#;
    let m = parse_manifest_str(toml).unwrap();
    let report = m.validate_with(ValidationLevel::Strict);
    assert!(!report.is_ok());
    assert!(
        report
            .errors
            .iter()
            .any(|e| e.contains("macos-aarch64") && e.contains("[compat].platforms")),
        "{:?}",
        report.errors
    );
}

#[test]
fn strict_rejects_operator_missing_required_fields() {
    let mut m = make_manifest();
    m.operators.push(OperatorDecl {
        kind: OperatorKind::Dso,
        type_name: String::new(),
        category: "  ".to_string(),
        label: None,
        tab_submenu: None,
        icon: None,
        source: None,
    });
    let report = m.validate_with(ValidationLevel::Strict);
    assert!(!report.is_ok());
    assert!(
        report.errors.iter().any(|e| e.contains("type_name")),
        "missing type_name error: {:?}",
        report.errors
    );
    assert!(
        report.errors.iter().any(|e| e.contains("category")),
        "missing category error: {:?}",
        report.errors
    );
}

#[test]
fn strict_rejects_non_semver_version() {
    let mut m = make_manifest();
    m.package.version = "not.a.version".to_string();
    let report = m.validate_with(ValidationLevel::Strict);
    assert!(!report.is_ok());
    assert!(
        report.errors.iter().any(|e| e.contains("semantic version")),
        "{:?}",
        report.errors
    );
    // validate() collapses the report to its first error.
    assert!(m.validate().is_err());
}

#[test]
fn strict_rejects_place_rule_with_empty_from_or_to() {
    let mut m = make_manifest();
    m.compat.platforms = vec![Platform::LinuxX86_64];
    m.stage.platform.entries.insert(
        "linux-x86_64".to_string(),
        StagePlatformRules {
            place: vec![
                PlaceRule {
                    from: "   ".to_string(),
                    to: "dso/foo.so".to_string(),
                },
                PlaceRule {
                    from: "build/foo.so".to_string(),
                    to: String::new(),
                },
            ],
        },
    );
    let report = m.validate_with(ValidationLevel::Strict);
    assert_eq!(
        report.errors.len(),
        2,
        "one error per malformed rule: {:?}",
        report.errors
    );
    assert!(
        report
            .errors
            .iter()
            .any(|e| e.contains("place[0]") && e.contains("`from`")),
        "{:?}",
        report.errors
    );
    assert!(
        report
            .errors
            .iter()
            .any(|e| e.contains("place[1]") && e.contains("`to`")),
        "{:?}",
        report.errors
    );
}

#[test]
fn strict_rejects_unknown_stage_platform_key() {
    let mut m = make_manifest();
    m.stage
        .platform
        .entries
        .insert("not-a-platform".to_string(), StagePlatformRules::default());
    let report = m.validate_with(ValidationLevel::Strict);
    assert!(!report.is_ok());
    assert!(
        report
            .errors
            .iter()
            .any(|e| e.contains("[stage.platform.not-a-platform]")),
        "{:?}",
        report.errors
    );
}

#[test]
fn strict_rejects_empty_conditional_cmd_list() {
    let mut m = make_manifest();
    m.scripts.commands.insert(
        "tt".to_string(),
        ScriptEntry::WithEnv(ScriptEnv {
            cmd: EnvValue::Conditional(Vec::new()),
            python: None,
            requirements: Vec::new(),
            label: None,
            description: None,
            package_env: false,
        }),
    );
    let report = m.validate_with(ValidationLevel::Strict);
    assert!(!report.is_ok());
    assert!(
        report
            .errors
            .iter()
            .any(|e| e.contains("tt") && e.contains("must not be empty")),
        "{:?}",
        report.errors
    );
}

#[test]
fn strict_accepts_plain_script_entries() {
    // Regression: the script-validation loop must early-return for
    // `ScriptEntry::Plain` and `EnvValue::Flat` rather than complain.
    let mut m = make_manifest();
    m.scripts.commands.insert(
        "setup".to_string(),
        ScriptEntry::Plain("echo hi".to_string()),
    );
    m.scripts.commands.insert(
        "build".to_string(),
        ScriptEntry::WithEnv(ScriptEnv {
            cmd: EnvValue::Flat("cargo build".to_string()),
            python: None,
            requirements: Vec::new(),
            label: None,
            description: None,
            package_env: false,
        }),
    );
    let report = m.validate_with(ValidationLevel::Strict);
    assert!(report.is_ok(), "{:?}", report.errors);
}

#[test]
fn publish_level_still_runs_structural_errors() {
    // Publish layers warnings on top of strict — it must not drop the
    // hard errors from the strict pass.
    let mut m = make_manifest();
    m.package.name = String::new();
    let report = m.validate_with(ValidationLevel::Publish);
    assert!(!report.is_ok(), "structural error should be retained");
    assert!(report.errors.iter().any(|e| e.contains("name")));
}

#[test]
fn validate_with_publish_emits_warnings_for_missing_metadata() {
    // A manifest that's structurally valid but missing the
    // publish-quality fields should pass Strict and emit warnings
    // at Publish level — not errors.
    let mut manifest = PackageManifest::new(
        PackagePath::new("studio/test").unwrap(),
        "Test".to_string(),
        "1.0.0".to_string(),
        None,
        Vec::new(),
        None,
    );
    // Clear the publish-quality fields the constructor pre-populates.
    manifest.package.keywords = Vec::new();
    manifest.compat = CompatConfig::default();

    let strict = manifest.validate_with(ValidationLevel::Strict);
    assert!(strict.is_ok());
    assert!(strict.warnings.is_empty(), "strict level emits no warnings");

    let publish = manifest.validate_with(ValidationLevel::Publish);
    assert!(publish.is_ok());
    assert_eq!(publish.warnings.len(), 4, "{:?}", publish.warnings);
    assert!(publish.warnings.iter().any(|w| w.contains("description")));
    assert!(publish.warnings.iter().any(|w| w.contains("authors")));
    assert!(publish.warnings.iter().any(|w| w.contains("keywords")));
    assert!(
        publish
            .warnings
            .iter()
            .any(|w| w.contains("[compat].houdini"))
    );
}

#[test]
fn houdini_package_no_version_constraints() {
    // Edge case: Houdini package generation without version constraints
    let mut manifest = PackageManifest::new(
        PackagePath::new("studio/test").unwrap(),
        "Test".to_string(),
        "1.0.0".to_string(),
        None,
        Vec::new(),
        None,
    );

    manifest.compat = CompatConfig {
        houdini: None,
        platforms: Vec::new(),
    };

    let houdini_pkg = manifest
        .generate_houdini_package()
        .expect("test manifest produces valid Houdini expr");
    assert!(houdini_pkg.enable.is_none());
}

#[test]
fn compat_houdini_compiles_to_enable_expression() {
    let toml_str = r#"
[package]
path = "studio/test"
name = "Test"
version = "1.0.0"

[compat]
houdini = ">=20.5, <22"
"#;
    let manifest: PackageManifest = toml::from_str(toml_str).unwrap();
    let pkg = manifest
        .generate_houdini_package()
        .expect("test manifest produces valid Houdini expr");
    assert_eq!(
        pkg.enable.as_deref(),
        Some("(houdini_version >= '20.5' and houdini_version < '22')")
    );
}

#[test]
fn compat_houdini_invalid_range_rejected_at_parse() {
    // HoudiniRange validates at deserialize time, so a malformed
    // range fails the TOML parse rather than reaching validate().
    let toml_str = r#"
[package]
path = "studio/test"
name = "Test"
version = "1.0.0"

[compat]
houdini = "not-a-version"
"#;
    let err = toml::from_str::<PackageManifest>(toml_str)
        .expect_err("invalid houdini range should fail at deserialize");
    let msg = err.to_string();
    assert!(
        msg.contains("houdini") || msg.contains("version requirement"),
        "error must point at the houdini range: {msg}"
    );
}

#[test]
fn compat_houdini_min_extracts_lower_bound() {
    let compat = CompatConfig {
        houdini: Some(HoudiniRange::parse(">=20.5, <22").unwrap()),
        platforms: Vec::new(),
    };
    assert_eq!(compat.houdini_min(), Some("20.5".to_string()));
    let compat = CompatConfig {
        houdini: Some(HoudiniRange::parse("^21").unwrap()),
        platforms: Vec::new(),
    };
    assert_eq!(compat.houdini_min(), Some("21".to_string()));
    let compat = CompatConfig::default();
    assert_eq!(compat.houdini_min(), None);
}

#[test]
fn registries_deserialize_from_toml() {
    let toml_str = r#"
[package]
path = "studio/my-context"
name = "My Context"
version = "0.1.0"

[[registries]]
name = "houdinihub"
url = "https://api.3db.dk/v1/registry"
type = "api"

[[registries]]
name = "studio-internal"
url = "https://packages.studio.com/v1/registry"
type = "git"

[dependencies]
"studio/test" = "0.2.0"
"#;
    let manifest: PackageManifest = toml::from_str(toml_str).unwrap();
    let registries = manifest.registries;
    assert_eq!(registries.len(), 2);
    assert_eq!(registries[0].name, "houdinihub");
    assert_eq!(registries[0].registry_type, RegistryType::Api);
    assert_eq!(registries[1].name, "studio-internal");
    assert_eq!(registries[1].registry_type, RegistryType::Git);
}

#[test]
fn runtime_deserialize_from_toml() {
    let toml_str = r#"
[package]
path = "studio/my-package"
name = "My Package"
version = "0.1.0"

[runtime]
MY_PLUGIN_ROOT = { method = "set", value = "$HPM_PACKAGE_ROOT/config" }
HOUDINI_TOOLBAR_PATH = { method = "prepend", value = "$HPM_PACKAGE_ROOT/toolbar" }
"#;
    let manifest: PackageManifest = toml::from_str(toml_str).unwrap();
    let runtime = manifest.runtime;
    assert_eq!(runtime.len(), 2);
    assert_eq!(runtime["MY_PLUGIN_ROOT"].method, EnvMethod::Set);
    assert_eq!(
        runtime["MY_PLUGIN_ROOT"]
            .value
            .as_ref()
            .and_then(EnvValue::as_flat),
        Some("$HPM_PACKAGE_ROOT/config")
    );
    assert!(!runtime["MY_PLUGIN_ROOT"].required);
    assert_eq!(runtime["HOUDINI_TOOLBAR_PATH"].method, EnvMethod::Prepend);
}

#[test]
fn runtime_required_without_value_deserializes() {
    let toml_str = r#"
[package]
path = "studio/my-package"
name = "My Package"
version = "0.1.0"

[runtime]
PROJECT_ROOT = { method = "set", required = true }
"#;
    let manifest: PackageManifest = toml::from_str(toml_str).unwrap();
    let runtime = &manifest.runtime;
    assert_eq!(runtime["PROJECT_ROOT"].method, EnvMethod::Set);
    assert!(runtime["PROJECT_ROOT"].value.is_none());
    assert!(runtime["PROJECT_ROOT"].required);
    assert!(manifest.validate().is_ok());
}

#[test]
fn runtime_missing_value_without_required_is_invalid() {
    // serde happily accepts the missing value (it's now Option), but
    // validate() rejects it because non-required entries must declare a
    // value.
    let toml_str = r#"
[package]
path = "studio/my-package"
name = "My Package"
version = "0.1.0"

[runtime]
LEAKED = { method = "set" }
"#;
    let manifest: PackageManifest = toml::from_str(toml_str).unwrap();
    let err = manifest.validate().unwrap_err();
    assert!(err.contains("LEAKED"));
    assert!(err.contains("required"));
}

#[test]
fn generate_houdini_package_skips_required_placeholders() {
    let mut manifest = PackageManifest::new(
        PackagePath::new("studio/test").unwrap(),
        "Test".to_string(),
        "1.0.0".to_string(),
        None,
        Vec::new(),
        None,
    );
    let mut runtime = IndexMap::new();
    runtime.insert(
        "PROJECT_ROOT".to_string(),
        ManifestEnvEntry {
            method: EnvMethod::Set,
            value: None,
            required: true,
        },
    );
    runtime.insert(
        "WITH_VALUE".to_string(),
        ManifestEnvEntry {
            method: EnvMethod::Set,
            value: Some("/somewhere".into()),
            required: false,
        },
    );
    manifest.runtime = runtime;

    let pkg = manifest
        .generate_houdini_package()
        .expect("test manifest produces valid Houdini expr");
    let env_list = pkg.env.unwrap();
    // 2 hardcoded (PYTHONPATH, HOUDINI_SCRIPT_PATH) + WITH_VALUE only.
    assert_eq!(env_list.len(), 3);
    assert!(
        env_list.iter().all(|m| !m.contains_key("PROJECT_ROOT")),
        "required-but-unsupplied placeholder should be skipped"
    );
    assert!(env_list.iter().any(|m| m.contains_key("WITH_VALUE")));
}

#[test]
fn runtime_invalid_method_rejected() {
    let toml_str = r#"
[package]
path = "studio/my-package"
name = "My Package"
version = "0.1.0"

[runtime]
MY_VAR = { method = "invalid", value = "foo" }
"#;
    let result: Result<PackageManifest, _> = toml::from_str(toml_str);
    assert!(result.is_err());
}

#[test]
fn runtime_install_source_dev_variant_drops_for_published_consumer() {
    // The HDK plugin pattern, expressed in the new shape. A single
    // [runtime] entry with two variants: dev-only build path + the
    // fallback published location. For a published consumer the dev
    // variant is filtered out so only the fallback ships.
    let toml_str = r#"
[package]
path = "studio/my-package"
name = "My Package"
version = "0.1.0"

[runtime.HOUDINI_DSO_PATH]
method = "prepend"
value = [
  { when = { install_source = "dev" }, set = "$HPM_PACKAGE_ROOT/build/Release" },
  { when = {}, set = "$HPM_PACKAGE_ROOT/dso" },
]
"#;
    let manifest: PackageManifest = toml::from_str(toml_str).unwrap();
    assert!(manifest.validate().is_ok());

    let pkg = manifest
        .generate_houdini_package()
        .expect("test manifest produces valid Houdini expr");
    let env_list = pkg.env.unwrap();
    // The HOUDINI_DSO_PATH entry must appear, but only the fallback
    // variant should be present (dev gate dropped).
    let dso_entry = env_list
        .iter()
        .find(|m| m.contains_key("HOUDINI_DSO_PATH"))
        .expect("HOUDINI_DSO_PATH should be emitted for published consumer");
    let value = &dso_entry["HOUDINI_DSO_PATH"];
    match value {
        // The single surviving branch is the empty `when = {}` fallback,
        // which is unconditional and collapses to a plain (list) value —
        // Houdini's expression grammar has no `true` literal to key a
        // conditional-array element with.
        HoudiniEnvValue::Detailed { method, value } => {
            assert_eq!(method, "prepend");
            assert_eq!(value, &vec!["$HPM_PACKAGE_ROOT/dso".to_string()]);
        }
        other => panic!("expected the collapsed fallback value, got {other:?}"),
    }
}

#[test]
fn runtime_install_source_only_drops_entry_for_published_consumer() {
    // When every variant is gated `install_source = "dev"`, the entry
    // disappears from a published consumer's package.json entirely.
    let toml_str = r#"
[package]
path = "studio/my-package"
name = "My Package"
version = "0.1.0"

[runtime.HOUDINI_DSO_PATH]
method = "prepend"
value = [
  { when = { install_source = "dev" }, set = "$HPM_PACKAGE_ROOT/build/Release" },
]
"#;
    let manifest: PackageManifest = toml::from_str(toml_str).unwrap();
    let pkg = manifest
        .generate_houdini_package()
        .expect("test manifest produces valid Houdini expr");
    let env_list = pkg.env.unwrap();
    assert!(
        env_list.iter().all(|m| !m.contains_key("HOUDINI_DSO_PATH")),
        "dev-only entries must not leak into the published Houdini manifest"
    );
}

#[test]
fn runtime_none_when_absent() {
    let toml_str = r#"
[package]
path = "studio/my-package"
name = "My Package"
version = "0.1.0"
"#;
    let manifest: PackageManifest = toml::from_str(toml_str).unwrap();
    assert!(manifest.runtime.is_empty());
}

#[test]
fn generate_houdini_package_includes_user_env() {
    let mut manifest = PackageManifest::new(
        PackagePath::new("studio/test").unwrap(),
        "Test".to_string(),
        "1.0.0".to_string(),
        None,
        Vec::new(),
        None,
    );

    let mut runtime = IndexMap::new();
    runtime.insert(
        "MY_VAR".to_string(),
        ManifestEnvEntry {
            method: EnvMethod::Set,
            value: Some("$HPM_PACKAGE_ROOT/data".into()),
            required: false,
        },
    );
    manifest.runtime = runtime;

    let houdini_pkg = manifest
        .generate_houdini_package()
        .expect("test manifest produces valid Houdini expr");
    assert_eq!(
        houdini_pkg.hpath,
        Some(vec!["$HPM_PACKAGE_ROOT".to_string()])
    );
    let env_list = houdini_pkg.env.unwrap();
    // 2 hardcoded (PYTHONPATH, HOUDINI_SCRIPT_PATH) + 1 user-defined
    assert_eq!(env_list.len(), 3);
    let last = &env_list[2];
    let val = last.get("MY_VAR").unwrap();
    match val {
        HoudiniEnvValue::Detailed { method, value } => {
            assert_eq!(method, "replace");
            assert_eq!(value, &vec!["$HPM_PACKAGE_ROOT/data".to_string()]);
        }
        _ => panic!("Expected Detailed variant"),
    }
}

#[test]
fn stage_deserialize_from_toml() {
    let toml_str = r#"
[package]
path = "studio/my-native-pkg"
name = "My Native Pkg"
version = "1.0.0"

[compat]
platforms = ["linux-x86_64", "macos-aarch64"]

[stage]
prepack = ["build-dso"]
include = ["python/**"]
exclude = ["src/**", "build/**"]

[stage.platform.linux-x86_64]
place = [
  { from = "build/linux/*.so", to = "dso/" },
]

[stage.platform.macos-aarch64]
place = [
  { from = "build/macos/*.dylib", to = "dso/" },
]
"#;
    let manifest: PackageManifest = toml::from_str(toml_str).unwrap();
    manifest.validate().unwrap();
    let compat = manifest.compat;
    assert_eq!(compat.platforms.len(), 2);
    let stage = &manifest.stage;
    assert_eq!(stage.prepack, vec!["build-dso".to_string()]);
    assert_eq!(stage.platform.entries.len(), 2);
    assert_eq!(
        stage.platform.entries["linux-x86_64"].place[0].from,
        "build/linux/*.so"
    );
    assert_eq!(stage.platform.entries["linux-x86_64"].place[0].to, "dso/");
}

#[test]
fn stage_empty_when_absent() {
    let toml_str = r#"
[package]
path = "studio/my-package"
name = "My Package"
version = "0.1.0"
"#;
    let manifest: PackageManifest = toml::from_str(toml_str).unwrap();
    assert!(manifest.stage.is_empty());
}

#[test]
fn compat_platforms_unknown_rejected() {
    // Unknown platform identifiers are rejected at deserialize time
    // by `Platform::TryFrom<String>`, so the manifest fails to parse
    // before validate ever runs.
    let toml_str = r#"
[package]
path = "studio/test"
name = "Test"
version = "1.0.0"

[compat]
platforms = ["linux-arm64"]
"#;
    let err = toml::from_str::<PackageManifest>(toml_str).unwrap_err();
    assert!(err.to_string().contains("linux-arm64"));
}

#[test]
fn stage_platform_not_in_compat_rejected() {
    let mut manifest = PackageManifest::new(
        PackagePath::new("studio/test").unwrap(),
        "Test".to_string(),
        "1.0.0".to_string(),
        None,
        Vec::new(),
        None,
    );
    manifest.compat = CompatConfig {
        houdini: None,
        platforms: vec![Platform::LinuxX86_64],
    };
    let mut entries = IndexMap::new();
    entries.insert(
        "windows-x86_64".to_string(),
        StagePlatformRules {
            place: vec![PlaceRule {
                from: "lib/*".to_string(),
                to: "lib/".to_string(),
            }],
        },
    );
    manifest.stage = StageConfig {
        platform: PlatformStaging { entries },
        ..Default::default()
    };
    let err = manifest.validate().unwrap_err();
    assert!(err.contains("not listed in [compat].platforms"), "{err}");
}

#[test]
fn stage_place_empty_from_rejected() {
    let mut manifest = PackageManifest::new(
        PackagePath::new("studio/test").unwrap(),
        "Test".to_string(),
        "1.0.0".to_string(),
        None,
        Vec::new(),
        None,
    );
    manifest.compat = CompatConfig {
        houdini: None,
        platforms: vec![Platform::LinuxX86_64],
    };
    let mut entries = IndexMap::new();
    entries.insert(
        "linux-x86_64".to_string(),
        StagePlatformRules {
            place: vec![PlaceRule {
                from: "".to_string(),
                to: "dso/".to_string(),
            }],
        },
    );
    manifest.stage = StageConfig {
        platform: PlatformStaging { entries },
        ..Default::default()
    };
    let err = manifest.validate().unwrap_err();
    assert!(err.contains("`from` must not be empty"), "{err}");
}

// Path-format validation lives in `package_path.rs` — see
// `PackagePath`'s tests for the well-formed/malformed cases.

#[test]
fn package_info_helpers() {
    let info = PackageInfo {
        path: PackagePath::new("tumblehead/tumble-rig").unwrap(),
        name: "TumbleRig".to_string(),
        version: "1.0.0".to_string(),
        description: None,
        authors: Vec::new(),
        license: None,
        readme: None,
        homepage: None,
        repository: None,
        documentation: None,
        keywords: Vec::new(),
        categories: Vec::new(),
    };
    assert_eq!(info.identifier(), "tumblehead/tumble-rig");
    assert_eq!(info.creator(), "tumblehead");
    assert_eq!(info.slug(), "tumble-rig");
}

#[test]
fn manifest_toml_roundtrip_with_path() {
    let manifest = PackageManifest::new(
        PackagePath::new("tumblehead/tumble-rig").unwrap(),
        "TumbleRig".to_string(),
        "1.0.0".to_string(),
        Some("A rig tool".to_string()),
        Vec::new(),
        Some("MIT".to_string()),
    );
    let toml_str = toml::to_string(&manifest).unwrap();
    assert!(toml_str.contains("path = \"tumblehead/tumble-rig\""));
    assert!(toml_str.contains("name = \"TumbleRig\""));

    let deserialized: PackageManifest = toml::from_str(&toml_str).unwrap();
    assert_eq!(deserialized.package.path, "tumblehead/tumble-rig");
    assert_eq!(deserialized.package.name, "TumbleRig");
}

#[test]
fn registries_none_when_absent() {
    let toml_str = r#"
[package]
path = "studio/my-context"
name = "My Context"
version = "0.1.0"
"#;
    let manifest: PackageManifest = toml::from_str(toml_str).unwrap();
    assert!(manifest.registries.is_empty());
}

#[test]
fn generate_houdini_native_package_full() {
    let toml_str = r#"
[package]
path = "creator/my-tool"
name = "My Cool Tool"
version = "1.2.3"

[compat]
houdini = ">=21.0"

[dependencies]
"studio/some-dep" = "1.0.0"

[runtime]
MY_VAR = { method = "prepend", value = "$HPM_PACKAGE_ROOT/scripts" }
"#;
    let manifest: PackageManifest = toml::from_str(toml_str).unwrap();
    let (filename, pkg) = manifest.generate_houdini_native_package().unwrap();

    assert_eq!(filename, "my-tool.json");
    assert_eq!(pkg.name, "my-tool");
    assert_eq!(pkg.hpath, "$HOUDINI_PACKAGE_PATH/my-tool");
    assert!(pkg.load_package_once);
    assert!(pkg.show);
    assert_eq!(pkg.enable.as_deref(), Some("houdini_version >= '21.0'"));
    assert_eq!(pkg.hpackage.version, "1.2.3");

    // First env entry is PKG_MY_TOOL
    let first_env = &pkg.env[0];
    assert!(first_env.contains_key("PKG_MY_TOOL"));

    // Second env entry has $HPM_PACKAGE_ROOT replaced
    let second_env = &pkg.env[1];
    let my_var = second_env.get("MY_VAR").unwrap();
    match my_var {
        crate::houdini::HoudiniEnvValue::Detailed { value, method } => {
            assert_eq!(
                value,
                &vec!["$HOUDINI_PACKAGE_PATH/my-tool/scripts".to_string()]
            );
            assert_eq!(method, "prepend");
        }
        _ => panic!("Expected Detailed variant"),
    }

    // Requires uses slug only
    assert_eq!(pkg.requires, Some(vec!["some-dep".to_string()]));
}

#[test]
fn generate_houdini_native_package_no_deps_no_houdini() {
    let toml_str = r#"
[package]
path = "studio/bare-pkg"
name = "Bare Package"
version = "0.1.0"
"#;
    let manifest: PackageManifest = toml::from_str(toml_str).unwrap();
    let (filename, pkg) = manifest.generate_houdini_native_package().unwrap();

    assert_eq!(filename, "bare-pkg.json");
    assert!(pkg.enable.is_none());
    assert!(pkg.requires.is_none());
    // Only the PKG_ env entry
    assert_eq!(pkg.env.len(), 1);
    assert!(pkg.env[0].contains_key("PKG_BARE_PKG"));
}

#[test]
fn scripts_flat_map_roundtrip() {
    let toml_str = r#"
[package]
path = "studio/my-package"
name = "My Package"
version = "0.1.0"

[scripts]
build = "cargo build"
test = "cargo test"
"#;
    let manifest: PackageManifest = toml::from_str(toml_str).unwrap();
    let scripts = manifest.scripts;
    assert_eq!(scripts.commands.len(), 2);
    assert_eq!(
        scripts.commands["build"].resolve_cmd(None),
        Some("cargo build".to_string())
    );
    assert_eq!(
        scripts.commands["test"].resolve_cmd(None),
        Some("cargo test".to_string())
    );

    // Plain entries don't carry venv hints.
    assert!(!scripts.commands["build"].needs_venv());

    // Preserves declaration order in the flattened commands map.
    let names: Vec<&String> = scripts.commands.keys().collect();
    assert_eq!(names, vec!["build", "test"]);
}

#[test]
fn scripts_conditional_cmd_resolves_per_host_os() {
    let toml_str = r#"
[package]
path = "studio/claudini"
name = "Claudini"
version = "1.0.0"

[scripts]
build = "cargo build"

[scripts.register]
cmd = [
  { when = { os = "windows" }, set = "\"$HPM_PACKAGE_ROOT/plugin/bin/claudini2.exe\" register" },
  { when = { os = "macos"   }, set = "\"$HPM_PACKAGE_ROOT/plugin/bin/claudini2\" register" },
  { when = { os = "linux"   }, set = "\"$HPM_PACKAGE_ROOT/plugin/bin/claudini2\" register" },
]
"#;
    let manifest: PackageManifest = toml::from_str(toml_str).unwrap();
    manifest.validate().unwrap();

    let scripts = manifest.scripts;
    // Plain shorthand resolves unconditionally.
    assert_eq!(
        scripts.commands["build"].resolve_cmd(Some("linux")),
        Some("cargo build".to_string())
    );
    // Conditional cmd picks the host-specific variant.
    let register = &scripts.commands["register"];
    assert!(
        register
            .resolve_cmd(Some("windows"))
            .unwrap()
            .contains("claudini2.exe")
    );
    assert_eq!(
        register.resolve_cmd(Some("macos")),
        Some("\"$HPM_PACKAGE_ROOT/plugin/bin/claudini2\" register".to_string())
    );
    // No host OS supplied → no variant matches.
    assert_eq!(register.resolve_cmd(None), None);
}

#[test]
fn scripts_conditional_with_fallback_branch_matches_any_host() {
    let toml_str = r#"
[package]
path = "studio/tool"
name = "Tool"
version = "1.0.0"

[scripts.register]
cmd = [
  { when = { os = "windows" }, set = "tool.exe register" },
  { when = {},                  set = "tool register" },
]
"#;
    let manifest: PackageManifest = toml::from_str(toml_str).unwrap();
    let entry = &manifest.scripts.commands["register"];
    assert_eq!(
        entry.resolve_cmd(Some("windows")),
        Some("tool.exe register".to_string())
    );
    // Empty `when = {}` matches any other host as a fallback.
    assert_eq!(
        entry.resolve_cmd(Some("macos")),
        Some("tool register".to_string())
    );
}

#[test]
fn scripts_table_form_with_python_and_requirements() {
    let toml_str = r#"
[package]
path = "studio/tt"
name = "TT"
version = "1.0.0"

[scripts]
build = "cargo build"

[scripts.tt_setup]
cmd = "python scripts/tt_setup.py"
python = "3.11"
requirements = ["PySide6>=6.6"]
"#;
    let manifest: PackageManifest = toml::from_str(toml_str).unwrap();
    let scripts = manifest.scripts;

    assert_eq!(
        scripts.commands["build"].resolve_cmd(None),
        Some("cargo build".to_string())
    );
    assert!(!scripts.commands["build"].needs_venv());

    let setup = &scripts.commands["tt_setup"];
    assert_eq!(
        setup.resolve_cmd(None),
        Some("python scripts/tt_setup.py".to_string())
    );
    assert_eq!(setup.python(), Some("3.11"));
    assert_eq!(setup.requirements(), &["PySide6>=6.6".to_string()]);
    assert!(setup.needs_venv());
}

#[test]
fn scripts_table_form_inline_object() {
    let toml_str = r#"
[package]
path = "studio/tt"
name = "TT"
version = "1.0.0"

[scripts]
tt_setup = { cmd = "python scripts/tt_setup.py", python = "3.11", requirements = ["PySide6>=6.6"] }
"#;
    let manifest: PackageManifest = toml::from_str(toml_str).unwrap();
    let setup = &manifest.scripts.commands["tt_setup"];
    assert_eq!(
        setup.resolve_cmd(None),
        Some("python scripts/tt_setup.py".to_string())
    );
    assert_eq!(setup.python(), Some("3.11"));
    assert_eq!(setup.requirements(), &["PySide6>=6.6".to_string()]);
}

#[test]
fn scripts_table_form_without_venv_hints_is_legal() {
    let toml_str = r#"
[package]
path = "studio/tt"
name = "TT"
version = "1.0.0"

[scripts.lint]
cmd = "ruff ."
"#;
    let manifest: PackageManifest = toml::from_str(toml_str).unwrap();
    let lint = &manifest.scripts.commands["lint"];
    assert_eq!(lint.resolve_cmd(None), Some("ruff .".to_string()));
    assert!(!lint.needs_venv());
}

#[test]
fn scripts_conditional_cmd_with_python_hints() {
    // The table form combines conditional cmd and venv hints.
    let toml_str = r#"
[package]
path = "studio/tt"
name = "TT"
version = "1.0.0"

[scripts.regen]
cmd = [
  { when = { os = "windows" }, set = "python scripts\\regen.py" },
  { when = {},                  set = "python scripts/regen.py" },
]
python = "3.11"
"#;
    let manifest: PackageManifest = toml::from_str(toml_str).unwrap();
    manifest.validate().unwrap();
    let regen = &manifest.scripts.commands["regen"];
    assert!(regen.needs_venv());
    assert_eq!(regen.python(), Some("3.11"));
    assert!(
        regen
            .resolve_cmd(Some("linux"))
            .unwrap()
            .contains("scripts/regen.py")
    );
    assert!(
        regen
            .resolve_cmd(Some("windows"))
            .unwrap()
            .contains("scripts\\regen.py")
    );
}

#[test]
fn scripts_when_rejects_non_os_axes() {
    // Only `os` is meaningful in script when selectors — HPM has no
    // Houdini/python/install_source context at `hpm run` time.
    let toml_str = r#"
[package]
path = "studio/tt"
name = "TT"
version = "1.0.0"

[scripts.bad]
cmd = [
  { when = { houdini = "^21" }, set = "x" },
]
"#;
    let manifest: PackageManifest = toml::from_str(toml_str).unwrap();
    let err = manifest.validate().unwrap_err();
    assert!(
        err.contains("only the `os` axis"),
        "unexpected error: {err}"
    );
}

#[test]
fn scripts_when_rejects_install_source() {
    let toml_str = r#"
[package]
path = "studio/tt"
name = "TT"
version = "1.0.0"

[scripts.bad]
cmd = [
  { when = { install_source = "dev" }, set = "x" },
]
"#;
    let manifest: PackageManifest = toml::from_str(toml_str).unwrap();
    assert!(manifest.validate().is_err());
}

#[test]
fn scripts_absent_resolves_empty() {
    let toml_str = r#"
[package]
path = "studio/pkg"
name = "Pkg"
version = "0.1.0"
"#;
    let manifest: PackageManifest = toml::from_str(toml_str).unwrap();
    assert!(manifest.scripts.is_empty());
    assert!(manifest.resolved_scripts().is_empty());
    assert!(manifest.script_for("anything").is_none());
}

#[test]
fn scripts_toml_roundtrip_preserves_conditional_cmd() {
    let toml_str = r#"
[package]
path = "studio/tool"
name = "Tool"
version = "1.0.0"

[scripts]
build = "cargo build"

[scripts.register]
cmd = [
  { when = { os = "windows" }, set = "tool.exe register" },
  { when = {},                  set = "tool register" },
]
"#;
    let manifest: PackageManifest = toml::from_str(toml_str).unwrap();
    let roundtrip = toml::to_string(&manifest).unwrap();
    let back: PackageManifest = toml::from_str(&roundtrip).unwrap();
    let scripts = back.scripts;
    assert_eq!(
        scripts.commands["build"].resolve_cmd(None),
        Some("cargo build".to_string())
    );
    assert_eq!(
        scripts.commands["register"].resolve_cmd(Some("windows")),
        Some("tool.exe register".to_string())
    );
    assert_eq!(
        scripts.commands["register"].resolve_cmd(Some("macos")),
        Some("tool register".to_string())
    );
}

#[test]
fn generate_houdini_native_package_env_root_replacement() {
    let mut manifest = PackageManifest::new(
        PackagePath::new("studio/test-pkg").unwrap(),
        "Test".to_string(),
        "1.0.0".to_string(),
        None,
        Vec::new(),
        None,
    );
    let mut runtime = IndexMap::new();
    runtime.insert(
        "PATH_A".to_string(),
        ManifestEnvEntry {
            method: EnvMethod::Set,
            value: Some("$HPM_PACKAGE_ROOT/a".into()),
            required: false,
        },
    );
    runtime.insert(
        "PATH_B".to_string(),
        ManifestEnvEntry {
            method: EnvMethod::Append,
            value: Some("$HPM_PACKAGE_ROOT/b:$HPM_PACKAGE_ROOT/c".into()),
            required: false,
        },
    );
    manifest.runtime = runtime;

    let (_, pkg) = manifest.generate_houdini_native_package().unwrap();

    // PATH_A
    match pkg.env[1].get("PATH_A").unwrap() {
        crate::houdini::HoudiniEnvValue::Detailed { value, .. } => {
            assert_eq!(value, &vec!["$HOUDINI_PACKAGE_PATH/test-pkg/a".to_string()]);
        }
        _ => panic!("Expected Detailed"),
    }
    // PATH_B with multiple replacements
    match pkg.env[2].get("PATH_B").unwrap() {
        crate::houdini::HoudiniEnvValue::Detailed { value, method } => {
            assert_eq!(
                value,
                &vec![
                    "$HOUDINI_PACKAGE_PATH/test-pkg/b:$HOUDINI_PACKAGE_PATH/test-pkg/c".to_string()
                ]
            );
            assert_eq!(method, "append");
        }
        _ => panic!("Expected Detailed"),
    }
}

#[test]
fn env_conditional_value_parses_from_toml() {
    let toml_str = r#"
[package]
path = "studio/multi-houdini"
name = "Multi Houdini"
version = "0.1.0"

[runtime.PXR_PLUGINPATH_NAME]
method = "prepend"
value = [
  { when = { houdini = "^21" }, set = "$HPM_PACKAGE_ROOT/resolver/houdini21/r" },
  { when = { houdini = "^22" }, set = "$HPM_PACKAGE_ROOT/resolver/houdini22/r" },
]
"#;
    let manifest: PackageManifest = toml::from_str(toml_str).unwrap();
    manifest.validate().unwrap();
    let entry = manifest.runtime.get("PXR_PLUGINPATH_NAME").unwrap();
    assert_eq!(entry.method, EnvMethod::Prepend);
    match entry.value.as_ref().unwrap() {
        EnvValue::Conditional(v) => {
            assert_eq!(v.len(), 2);
            assert_eq!(
                v[0].when.houdini.as_ref().map(HoudiniRange::as_str),
                Some("^21")
            );
            assert_eq!(
                v[1].when.houdini.as_ref().map(HoudiniRange::as_str),
                Some("^22")
            );
        }
        EnvValue::Flat(_) => panic!("expected conditional"),
    }
}

#[test]
fn env_conditional_value_lowers_to_houdini_array() {
    let toml_str = r#"
[package]
path = "studio/multi"
name = "Multi"
version = "0.1.0"

[runtime.PXR_PLUGINPATH_NAME]
method = "prepend"
value = [
  { when = { houdini = "^21" }, set = "$HPM_PACKAGE_ROOT/h21/r" },
  { when = { houdini = "^22", os = "linux" }, set = "$HPM_PACKAGE_ROOT/h22/r" },
]
"#;
    let manifest: PackageManifest = toml::from_str(toml_str).unwrap();
    let entry = manifest.runtime.get("PXR_PLUGINPATH_NAME").unwrap();
    let lowered = entry
        .lower(&[("$HPM_PACKAGE_ROOT", "/abs/pkg")], false)
        .unwrap()
        .unwrap();
    match lowered {
        HoudiniEnvValue::DetailedConditional { method, value } => {
            assert_eq!(method, "prepend");
            assert_eq!(value.len(), 2);
            let first = &value[0];
            let key = first.keys().next().unwrap();
            assert_eq!(key, "houdini_version >= '21' and houdini_version < '22'");
            assert_eq!(first[key], "/abs/pkg/h21/r");
            let second = &value[1];
            let key2 = second.keys().next().unwrap();
            // The second branch carries the negation of the first, so at
            // most one branch fires (Houdini applies every matching
            // element, not the first match).
            assert_eq!(
                key2,
                "houdini_version >= '22' and houdini_version < '23' and houdini_os == 'linux' \
                 and ( houdini_version < '21' or houdini_version >= '22' )"
            );
            assert_eq!(second[key2], "/abs/pkg/h22/r");
        }
        _ => panic!("expected DetailedConditional"),
    }
}

#[test]
fn env_conditional_value_serializes_in_native_package() {
    let toml_str = r#"
[package]
path = "studio/multi-pkg"
name = "Multi"
version = "0.1.0"

[runtime.PXR_PLUGINPATH_NAME]
method = "prepend"
value = [
  { when = { houdini = "^21" }, set = "$HPM_PACKAGE_ROOT/h21/r" },
]
"#;
    let manifest: PackageManifest = toml::from_str(toml_str).unwrap();
    let (_, pkg) = manifest.generate_houdini_native_package().unwrap();
    let json = serde_json::to_string(&pkg).unwrap();
    // The conditional-object array form must round-trip into JSON with
    // method, value, and the embedded expression as the inner object key.
    assert!(json.contains("\"method\":\"prepend\""));
    assert!(json.contains("houdini_version >= '21' and houdini_version < '22'"));
    assert!(json.contains("$HOUDINI_PACKAGE_PATH/multi-pkg/h21/r"));
}

#[test]
fn env_conditional_value_with_invalid_req_fails_at_parse() {
    // Condition.houdini is a HoudiniRange newtype that validates
    // at deserialize, so a malformed range fails the TOML parse
    // rather than reaching validate().
    let toml_str = r#"
[package]
path = "studio/bad"
name = "Bad"
version = "0.1.0"

[runtime.X]
method = "set"
value = [
  { when = { houdini = "garbage" }, set = "x" },
]
"#;
    // Untagged enum (EnvValue) flattens the inner HoudiniRange
    // error into a generic "did not match any variant" message, so we
    // can only assert the parse fails — the specific error text is
    // upstream and not stable.
    assert!(
        toml::from_str::<PackageManifest>(toml_str).is_err(),
        "invalid houdini range should fail at deserialize"
    );
}

#[test]
fn env_conditional_value_empty_list_fails_validate() {
    // An empty conditional list is meaningless — flag it at validate()
    // rather than emitting an empty Houdini env entry.
    let toml_str = r#"
[package]
path = "studio/bad"
name = "Bad"
version = "0.1.0"

[runtime.X]
method = "set"
value = []
"#;
    let manifest: PackageManifest = toml::from_str(toml_str).unwrap();
    let err = manifest.validate().unwrap_err();
    assert!(err.contains("empty conditional"));
}

#[test]
fn env_pass_through_preserves_houdini_vars_in_flat_value() {
    // Regression: hpm only substitutes $HPM_PACKAGE_ROOT. Anything else
    // (notably $HOUDINI_MAJOR_RELEASE, $HFS, $HOUDINI_USER_PREF_DIR) must
    // pass through verbatim into the emitted package.json so Houdini's
    // own variable expansion does the work at startup.
    let mut manifest = PackageManifest::new(
        PackagePath::new("studio/passthrough").unwrap(),
        "Passthrough".to_string(),
        "1.0.0".to_string(),
        None,
        Vec::new(),
        None,
    );
    let mut runtime = IndexMap::new();
    runtime.insert(
        "PXR_PLUGINPATH_NAME".to_string(),
        ManifestEnvEntry {
            method: EnvMethod::Prepend,
            value: Some("$HPM_PACKAGE_ROOT/resolver/houdini$HOUDINI_MAJOR_RELEASE/r".into()),
            required: false,
        },
    );
    manifest.runtime = runtime;

    let pkg = manifest
        .generate_houdini_package()
        .expect("test manifest produces valid Houdini expr");
    let env_list = pkg.env.unwrap();
    let entry = env_list
        .iter()
        .find_map(|m| m.get("PXR_PLUGINPATH_NAME"))
        .unwrap();
    match entry {
        HoudiniEnvValue::Detailed { value, .. } => {
            assert!(
                value.iter().any(|v| v.contains("$HOUDINI_MAJOR_RELEASE")),
                "Houdini var must pass through verbatim, got: {:?}",
                value
            );
        }
        _ => panic!("expected Detailed flat value"),
    }
}
