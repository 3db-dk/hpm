use super::houdini_emit::build_houdini_package;
use super::*;
use hpm_package::{ManifestEnvEntry, PackagePath};
use indexmap::IndexMap;
use std::path::Path;
use tempfile::TempDir;

/// Build the `(Config, StorageManager)` pair every `ProjectManager` test
/// needs, rooted inside `temp_dir`. The CAS lives at `<temp>/.hpm/packages`.
/// Shared with the sibling emission-model and Houdini-conformance test
/// modules.
pub(crate) fn test_setup(temp_dir: &Path) -> (Arc<Config>, Arc<StorageManager>) {
    let storage_config = hpm_config::StorageConfig {
        home_dir: temp_dir.join(".hpm"),
        cache_dir: temp_dir.join(".hpm").join("cache"),
        packages_dir: temp_dir.join(".hpm").join("packages"),
        registry_cache_dir: temp_dir.join(".hpm").join("registry"),
    };
    let config = Arc::new(Config {
        storage: storage_config.clone(),
        ..Default::default()
    });
    let storage_manager = Arc::new(StorageManager::new(storage_config).unwrap());
    (config, storage_manager)
}

#[tokio::test]
async fn project_manager_creation() {
    let temp_dir = TempDir::new().unwrap();
    let (config, storage_manager) = test_setup(temp_dir.path());
    let project_root = temp_dir.path().join("project");
    std::fs::create_dir_all(&project_root).unwrap();

    let _project_manager =
        ProjectManager::new(project_root.clone(), storage_manager, config).unwrap();
    assert!(project_root.join(".hpm").join("packages").exists());
}

#[tokio::test]
async fn new_with_auth_none_matches_new() {
    // Regression: `new(...)` must remain a one-line delegate to
    // `new_with_auth(..., None)`. If the two paths diverge, anonymous
    // callers (the CLI, every existing embedder) would silently drift
    // from the authenticated path's behavior.
    let temp_dir = TempDir::new().unwrap();
    let (config, storage_manager) = test_setup(temp_dir.path());
    let project_root = temp_dir.path().join("project");
    std::fs::create_dir_all(&project_root).unwrap();

    let pm =
        ProjectManager::new_with_auth(project_root.clone(), storage_manager, config, None).unwrap();
    assert!(pm.auth_token.is_none());
    assert!(project_root.join(".hpm").join("packages").exists());
}

#[tokio::test]
async fn new_with_auth_some_stashes_token() {
    let temp_dir = TempDir::new().unwrap();
    let (config, storage_manager) = test_setup(temp_dir.path());
    let project_root = temp_dir.path().join("project");
    std::fs::create_dir_all(&project_root).unwrap();

    let pm = ProjectManager::new_with_auth(
        project_root,
        storage_manager,
        config,
        Some("supabase-access-token-xyz".to_string()),
    )
    .unwrap();
    assert_eq!(pm.auth_token.as_deref(), Some("supabase-access-token-xyz"));
}

#[test]
fn list_dependencies_empty_project() {
    let temp_dir = TempDir::new().unwrap();
    let (config, storage_manager) = test_setup(temp_dir.path());
    let project_root = temp_dir.path().join("project");
    std::fs::create_dir_all(&project_root).unwrap();

    let project_manager = ProjectManager::new(project_root, storage_manager, config).unwrap();
    let deps = project_manager.list_dependencies().unwrap();
    assert_eq!(deps.len(), 0);
}

#[test]
fn build_houdini_package_basic() {
    let temp_dir = TempDir::new().unwrap();

    let manifest = hpm_package::PackageManifest::new(
        PackagePath::new("studio/test-package").unwrap(),
        "Test Package".to_string(),
        "1.0.0".to_string(),
        Some("A test package".to_string()),
        Vec::new(),
        None,
    );

    let package_path = temp_dir.path().join("test-package@1.0.0");
    std::fs::create_dir_all(package_path.join("python")).unwrap();
    std::fs::create_dir_all(package_path.join("otls")).unwrap();

    let installed_package = InstalledPackage {
        version: "1.0.0".to_string(),
        manifest,
        install_path: package_path.clone(),
        is_dev: false,
    };

    let houdini_package =
        build_houdini_package(&installed_package, None, &IndexMap::new()).unwrap();
    assert_eq!(
        houdini_package.hpath,
        Some(vec![package_path.to_string_lossy().to_string()])
    );
    assert!(houdini_package.env.is_some());
}

#[test]
fn build_houdini_package_with_project_env_overrides() {
    let temp_dir = TempDir::new().unwrap();

    // Create a manifest with an env var
    let mut manifest = hpm_package::PackageManifest::new(
        PackagePath::new("studio/test-package").unwrap(),
        "Test Package".to_string(),
        "1.0.0".to_string(),
        Some("A test package".to_string()),
        Vec::new(),
        None,
    );
    let mut pkg_env = IndexMap::new();
    pkg_env.insert(
        "MY_CONFIG".to_string(),
        ManifestEnvEntry {
            method: hpm_package::EnvMethod::Set,
            value: Some("$HPM_PACKAGE_ROOT/default-config".into()),
            required: false,
        },
    );
    manifest.runtime = pkg_env;

    let package_path = temp_dir.path().join("test-package@1.0.0");
    std::fs::create_dir_all(&package_path).unwrap();

    let installed_package = InstalledPackage {
        version: "1.0.0".to_string(),
        manifest,
        install_path: package_path.clone(),
        is_dev: false,
    };

    // Without override: should use package default
    let houdini_package =
        build_houdini_package(&installed_package, None, &IndexMap::new()).unwrap();
    assert_eq!(
        houdini_package.hpath,
        Some(vec![package_path.to_string_lossy().to_string()])
    );
    let env_entries = houdini_package.env.as_ref().unwrap();
    let my_config_entry = env_entries
        .iter()
        .find(|m| m.contains_key("MY_CONFIG"))
        .unwrap();
    match my_config_entry.get("MY_CONFIG").unwrap() {
        // `set` emits a bare flat string: a later flat value overwrites a
        // path-registered variable (OCIO, PYTHONPATH) rather than being
        // appended onto Houdini's flat seed.
        hpm_package::HoudiniEnvValue::Simple(value) => {
            assert!(value.ends_with("/default-config"));
        }
        _ => panic!("Expected Simple env value for `set`"),
    }

    // With a `set` project override: the package's entry is suppressed —
    // the value is carried by the project overrides manifest instead.
    let mut project_overrides = IndexMap::new();
    project_overrides.insert(
        "MY_CONFIG".to_string(),
        ManifestEnvEntry {
            method: hpm_package::EnvMethod::Set,
            value: Some("/custom/config/path".into()),
            required: false,
        },
    );

    let houdini_package =
        build_houdini_package(&installed_package, None, &project_overrides).unwrap();
    assert!(
        find_env_entry(&houdini_package, "MY_CONFIG").is_none(),
        "a `set` override must suppress the package's own entry"
    );

    let overrides_pkg = ProjectManager::build_project_overrides_package(&project_overrides)
        .unwrap()
        .expect("a valued override must produce an overrides manifest");
    match find_env_entry(&overrides_pkg, "MY_CONFIG").unwrap() {
        hpm_package::HoudiniEnvValue::Simple(value) => {
            assert_eq!(value, "/custom/config/path");
        }
        _ => panic!("Expected Simple env value for `set`"),
    }
}

#[tokio::test]
async fn build_houdini_package_required_env_without_override_errors() {
    let temp_dir = TempDir::new().unwrap();

    let mut manifest = hpm_package::PackageManifest::new(
        PackagePath::new("studio/needs-config").unwrap(),
        "Needs Config".to_string(),
        "1.0.0".to_string(),
        None,
        Vec::new(),
        None,
    );
    let mut pkg_env = IndexMap::new();
    pkg_env.insert(
        "PROJECT_ROOT".to_string(),
        ManifestEnvEntry {
            method: hpm_package::EnvMethod::Set,
            value: None,
            required: true,
        },
    );
    manifest.runtime = pkg_env;

    let package_path = temp_dir.path().join("needs-config@1.0.0");
    std::fs::create_dir_all(&package_path).unwrap();

    let installed_package = InstalledPackage {
        version: "1.0.0".to_string(),
        manifest,
        install_path: package_path,
        is_dev: false,
    };

    // No project override: required placeholder must trigger MissingRequiredEnv.
    let err = build_houdini_package(&installed_package, None, &IndexMap::new()).unwrap_err();
    match err {
        ProjectError::MissingRequiredEnv { var, package } => {
            assert_eq!(var, "PROJECT_ROOT");
            assert_eq!(package, "needs-config");
        }
        other => panic!("Expected MissingRequiredEnv, got {:?}", other),
    }

    // Project override supplies a value: the package generates cleanly
    // (nothing to emit for the placeholder) and the overrides manifest
    // carries the value.
    let mut overrides = IndexMap::new();
    overrides.insert(
        "PROJECT_ROOT".to_string(),
        ManifestEnvEntry {
            method: hpm_package::EnvMethod::Set,
            value: Some("/work/project".into()),
            required: false,
        },
    );
    let pkg = build_houdini_package(&installed_package, None, &overrides).unwrap();
    assert!(
        find_env_entry(&pkg, "PROJECT_ROOT").is_none(),
        "a valueless placeholder has nothing to emit in the package file"
    );
    let overrides_pkg = ProjectManager::build_project_overrides_package(&overrides)
        .unwrap()
        .expect("a valued override must produce an overrides manifest");
    match find_env_entry(&overrides_pkg, "PROJECT_ROOT").unwrap() {
        hpm_package::HoudiniEnvValue::Simple(value) => {
            assert_eq!(value, "/work/project");
        }
        _ => panic!("Expected Simple env value for `set`"),
    }
}

/// Helper: locate a single [runtime]-emitted entry by key.
fn find_env_entry<'a>(
    pkg: &'a hpm_package::HoudiniPackage,
    key: &str,
) -> Option<&'a hpm_package::HoudiniEnvValue> {
    pkg.env.as_ref()?.iter().find_map(|m| m.get(key))
}

/// Build a `[runtime]` entry with conditional variants gated on
/// install_source. The value is only test data: a real dev install is a
/// staged copy with no build tree (see the user guide's HDK plugins section).
fn dev_only_runtime_entry(value: &str) -> ManifestEnvEntry {
    use hpm_package::{Condition, EnvValue, EnvValueBranch};
    ManifestEnvEntry {
        method: hpm_package::EnvMethod::Prepend,
        value: Some(EnvValue::Conditional(vec![EnvValueBranch {
            when: Condition {
                install_source: Some(hpm_package::InstallSource::Dev),
                ..Default::default()
            },
            set: value.to_string(),
        }])),
        required: false,
    }
}

#[test]
fn runtime_install_source_dev_gates_emission() {
    let temp_dir = TempDir::new().unwrap();

    let mut manifest = hpm_package::PackageManifest::new(
        PackagePath::new("studio/hdk-plugin").unwrap(),
        "HDK Plugin".to_string(),
        "1.0.0".to_string(),
        None,
        Vec::new(),
        None,
    );
    let mut runtime = IndexMap::new();
    runtime.insert(
        "HOUDINI_DSO_PATH".to_string(),
        dev_only_runtime_entry("$HPM_PACKAGE_ROOT/build/Release"),
    );
    manifest.runtime = runtime;

    let package_path = temp_dir.path().join("hdk-plugin@1.0.0");
    std::fs::create_dir_all(&package_path).unwrap();

    // is_dev = false: the dev-only variant filters out, the entry has
    // no surviving branches, so HOUDINI_DSO_PATH is not emitted at all.
    let non_dev = InstalledPackage {
        version: "1.0.0".to_string(),
        manifest: manifest.clone(),
        install_path: package_path.clone(),
        is_dev: false,
    };
    let pkg = build_houdini_package(&non_dev, None, &IndexMap::new()).unwrap();
    assert!(
        find_env_entry(&pkg, "HOUDINI_DSO_PATH").is_none(),
        "install_source = 'dev' must not be emitted for non-dev installs"
    );

    // is_dev = true: the dev variant fires, $HPM_PACKAGE_ROOT expands.
    let dev = InstalledPackage {
        version: "1.0.0".to_string(),
        manifest,
        install_path: package_path.clone(),
        is_dev: true,
    };
    let pkg = build_houdini_package(&dev, None, &IndexMap::new()).unwrap();
    let entry = find_env_entry(&pkg, "HOUDINI_DSO_PATH")
        .expect("dev-gated variant must be emitted for dev installs");
    // An install-source-only gate is unconditional at runtime (the axis is
    // stripped before compilation), so the value collapses to a plain
    // Detailed list — Houdini's expression grammar has no `true` literal
    // to key a conditional-array element with.
    match entry {
        hpm_package::HoudiniEnvValue::Detailed { method, value } => {
            assert_eq!(method.as_str(), "prepend");
            assert_eq!(
                value,
                &vec![format!("{}/build/Release", package_path.display())]
            );
        }
        other => panic!("expected the collapsed dev value, got {other:?}"),
    }
}

/// Helper: collect every [runtime]-emitted entry for a key, in emission
/// order. `append`/`prepend` overrides combine with the package value, so
/// a key can legitimately appear in more than one env map.
fn collect_env_entries<'a>(
    pkg: &'a hpm_package::HoudiniPackage,
    key: &str,
) -> Vec<&'a hpm_package::HoudiniEnvValue> {
    pkg.env
        .as_ref()
        .map(|env| env.iter().filter_map(|m| m.get(key)).collect())
        .unwrap_or_default()
}

#[test]
fn project_prepend_override_combines_with_dev_variant() {
    let temp_dir = TempDir::new().unwrap();

    let mut manifest = hpm_package::PackageManifest::new(
        PackagePath::new("studio/overridable").unwrap(),
        "Overridable".to_string(),
        "1.0.0".to_string(),
        None,
        Vec::new(),
        None,
    );
    let mut runtime = IndexMap::new();
    runtime.insert(
        "HOUDINI_DSO_PATH".to_string(),
        dev_only_runtime_entry("$HPM_PACKAGE_ROOT/build/Release"),
    );
    manifest.runtime = runtime;

    let package_path = temp_dir.path().join("overridable@1.0.0");
    std::fs::create_dir_all(&package_path).unwrap();

    let installed = InstalledPackage {
        version: "1.0.0".to_string(),
        manifest,
        install_path: package_path.clone(),
        is_dev: true,
    };

    let mut overrides = IndexMap::new();
    overrides.insert(
        "HOUDINI_DSO_PATH".to_string(),
        ManifestEnvEntry {
            method: hpm_package::EnvMethod::Prepend,
            value: Some("/opt/forced/dso".into()),
            required: false,
        },
    );
    let pkg = build_houdini_package(&installed, None, &overrides).unwrap();

    // A `prepend` override combines: the package's own (dev-gated)
    // contribution stays in the package file; the project's prepend is
    // carried once by the overrides manifest, which Houdini processes
    // after every package file.
    let entries = collect_env_entries(&pkg, "HOUDINI_DSO_PATH");
    assert_eq!(
        entries.len(),
        1,
        "the package file must carry only the package's own value"
    );
    match entries[0] {
        hpm_package::HoudiniEnvValue::Detailed { method, value } => {
            assert_eq!(method.as_str(), "prepend");
            assert_eq!(
                value,
                &vec![format!("{}/build/Release", package_path.display())]
            );
        }
        other => panic!("expected the package's dev variant, got {other:?}"),
    }
    let overrides_pkg = ProjectManager::build_project_overrides_package(&overrides)
        .unwrap()
        .expect("a valued override must produce an overrides manifest");
    match find_env_entry(&overrides_pkg, "HOUDINI_DSO_PATH").unwrap() {
        hpm_package::HoudiniEnvValue::Detailed { method, value } => {
            assert_eq!(method.as_str(), "prepend");
            assert_eq!(value, &vec!["/opt/forced/dso".to_string()]);
        }
        other => panic!("expected the project's prepend in the overrides manifest, got {other:?}"),
    }
}

#[test]
fn project_append_override_combines_with_package_value() {
    // The core regression: a project `append`/`prepend` override against a
    // package-provided env var must extend it, emitting both the package's
    // value and the project's, not replace the package's contribution.
    let temp_dir = TempDir::new().unwrap();

    let mut manifest = hpm_package::PackageManifest::new(
        PackagePath::new("studio/lib").unwrap(),
        "Lib".to_string(),
        "1.0.0".to_string(),
        None,
        Vec::new(),
        None,
    );
    let mut runtime = IndexMap::new();
    runtime.insert(
        "PYTHONPATH".to_string(),
        ManifestEnvEntry {
            method: hpm_package::EnvMethod::Append,
            value: Some("$HPM_PACKAGE_ROOT/python".into()),
            required: false,
        },
    );
    manifest.runtime = runtime;

    let package_path = temp_dir.path().join("lib@1.0.0");
    std::fs::create_dir_all(&package_path).unwrap();

    let installed = InstalledPackage {
        version: "1.0.0".to_string(),
        manifest,
        install_path: package_path.clone(),
        is_dev: false,
    };

    let mut overrides = IndexMap::new();
    overrides.insert(
        "PYTHONPATH".to_string(),
        ManifestEnvEntry {
            method: hpm_package::EnvMethod::Append,
            value: Some("/work/project/extra".into()),
            required: false,
        },
    );
    let pkg = build_houdini_package(&installed, None, &overrides).unwrap();

    let entries = collect_env_entries(&pkg, "PYTHONPATH");
    assert_eq!(
        entries.len(),
        1,
        "append override must leave the package value in place, not replace it"
    );
    match entries[0] {
        hpm_package::HoudiniEnvValue::Detailed { method, value } => {
            assert_eq!(method.as_str(), "append");
            assert_eq!(value, &vec![format!("{}/python", package_path.display())]);
        }
        other => panic!("expected the package value, got {other:?}"),
    }
    // The project's append is carried once by the overrides manifest.
    let overrides_pkg = ProjectManager::build_project_overrides_package(&overrides)
        .unwrap()
        .expect("a valued override must produce an overrides manifest");
    match find_env_entry(&overrides_pkg, "PYTHONPATH").unwrap() {
        hpm_package::HoudiniEnvValue::Detailed { method, value } => {
            assert_eq!(method.as_str(), "append");
            assert_eq!(value, &vec!["/work/project/extra".to_string()]);
        }
        other => panic!("expected the project's append in the overrides manifest, got {other:?}"),
    }
}

#[test]
fn project_set_override_still_replaces_package_value() {
    // `set` keeps replace semantics: only the project's value is emitted.
    let temp_dir = TempDir::new().unwrap();

    let mut manifest = hpm_package::PackageManifest::new(
        PackagePath::new("studio/lib").unwrap(),
        "Lib".to_string(),
        "1.0.0".to_string(),
        None,
        Vec::new(),
        None,
    );
    let mut runtime = IndexMap::new();
    runtime.insert(
        "MY_CONFIG".to_string(),
        ManifestEnvEntry {
            method: hpm_package::EnvMethod::Set,
            value: Some("$HPM_PACKAGE_ROOT/default".into()),
            required: false,
        },
    );
    manifest.runtime = runtime;

    let package_path = temp_dir.path().join("lib@1.0.0");
    std::fs::create_dir_all(&package_path).unwrap();

    let installed = InstalledPackage {
        version: "1.0.0".to_string(),
        manifest,
        install_path: package_path,
        is_dev: false,
    };

    let mut overrides = IndexMap::new();
    overrides.insert(
        "MY_CONFIG".to_string(),
        ManifestEnvEntry {
            method: hpm_package::EnvMethod::Set,
            value: Some("/custom".into()),
            required: false,
        },
    );
    let pkg = build_houdini_package(&installed, None, &overrides).unwrap();

    let entries = collect_env_entries(&pkg, "MY_CONFIG");
    assert_eq!(
        entries.len(),
        0,
        "set override must suppress the package's entry"
    );
    let overrides_pkg = ProjectManager::build_project_overrides_package(&overrides)
        .unwrap()
        .expect("a valued override must produce an overrides manifest");
    match find_env_entry(&overrides_pkg, "MY_CONFIG").unwrap() {
        // `set` emits a bare flat string so the override overwrites a
        // path-registered variable cleanly, load-order-independent.
        hpm_package::HoudiniEnvValue::Simple(value) => {
            assert_eq!(value, "/custom");
        }
        other => panic!("expected Simple env value for `set`, got {other:?}"),
    }
}

#[test]
fn matches_spec_name_handles_scoped_and_bare() {
    let manifest = hpm_package::PackageManifest::new(
        PackagePath::new("tumblehead/claudini2").unwrap(),
        "Claudini 2".to_string(),
        "0.4.0".to_string(),
        None,
        Vec::new(),
        None,
    );
    let pkg = InstalledPackage {
        version: "0.4.0".to_string(),
        manifest,
        install_path: PathBuf::from("/tmp/claudini2@0.4.0"),
        is_dev: false,
    };

    assert!(ProjectManager::matches_spec_name(
        &pkg,
        "tumblehead/claudini2"
    ));
    assert!(ProjectManager::matches_spec_name(&pkg, "claudini2"));
    assert!(!ProjectManager::matches_spec_name(&pkg, "other/claudini2"));
    assert!(!ProjectManager::matches_spec_name(&pkg, "unrelated"));
}

/// Regression: when a project's hpm.toml lists a scoped dependency name
/// (`creator/slug`), but the installed-packages cache stores the bare slug
/// in `InstalledPackage.name`, the short-circuit must still fire. Otherwise
/// every `sync_dependencies` re-fetches and re-installs every dep, and on
/// Windows the remove-and-recopy can fail with os error 5 when another
/// Houdini holds the package dir open.
#[tokio::test]
async fn install_one_dep_short_circuits_on_scoped_name() {
    let temp_dir = TempDir::new().unwrap();
    let (_config, storage_manager) = test_setup(temp_dir.path());
    let fetcher =
        ArchiveFetcher::new(temp_dir.path().join("cache"), temp_dir.path().join("fetch")).unwrap();

    let manifest = hpm_package::PackageManifest::new(
        PackagePath::new("tumblehead/tumblepipe").unwrap(),
        "Tumblepipe".to_string(),
        "1.1.20".to_string(),
        None,
        Vec::new(),
        None,
    );
    let installed = InstalledPackage {
        version: "1.1.20".to_string(),
        manifest,
        install_path: temp_dir.path().join("tumblepipe@1.1.20"),
        is_dev: false,
    };

    // registry_set: None — if the short-circuit misses, install_one_dep
    // would panic on `expect("registry set built when registry deps
    // present")`. Reaching that panic is exactly the bug.
    let spec = hpm_package::DependencySpec::registry("1.1.20", None);
    let outcome = install_one_dep(
        &storage_manager,
        &fetcher,
        None,
        std::slice::from_ref(&installed),
        "tumblehead/tumblepipe",
        &spec,
    )
    .await
    .expect("scoped lookup must short-circuit on the bare-slug InstalledPackage");

    assert_eq!(outcome.package.slug(), "tumblepipe");
    assert_eq!(outcome.package.version, "1.1.20");
    // Short-circuited Registry: no fresh fetch -> no checksum / source.
    assert!(outcome.checksum.is_none());
    assert!(outcome.source.is_none());
}

/// Regression: reusing an already-installed tree must repair a program that
/// lost its executable bit in the archive.
///
/// The short-circuit above is exactly why the extraction-time repair never
/// reached an existing install — no re-extraction, so nothing ever looked at
/// the tree again. A user on the affected install saw the package's own
/// `tt_setup` die with `Permission denied (os error 13)` on every launch, and
/// upgrading hpm changed nothing.
#[cfg(unix)]
#[tokio::test]
async fn install_one_dep_repairs_a_lost_executable_bit_on_reuse() {
    use std::os::unix::fs::PermissionsExt;

    let temp_dir = TempDir::new().unwrap();
    let (_config, storage_manager) = test_setup(temp_dir.path());
    let fetcher =
        ArchiveFetcher::new(temp_dir.path().join("cache"), temp_dir.path().join("fetch")).unwrap();

    // A tree as an older hpm extracted it: the Mach-O `tt_setup` the manifest
    // declares as a script, stamped 0644 by the producing zip.
    let install_path = temp_dir.path().join("tumblepipe@1.39.2");
    let tool = install_path.join("bin/macos-aarch64/tt_setup");
    std::fs::create_dir_all(tool.parent().unwrap()).unwrap();
    std::fs::write(&tool, [0xCFu8, 0xFA, 0xED, 0xFE]).unwrap();
    std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o644)).unwrap();

    let manifest = hpm_package::PackageManifest::new(
        PackagePath::new("tumblehead/tumblepipe").unwrap(),
        "Tumblepipe".to_string(),
        "1.39.2".to_string(),
        None,
        Vec::new(),
        None,
    );
    let installed = InstalledPackage {
        version: "1.39.2".to_string(),
        manifest,
        install_path,
        is_dev: false,
    };

    let spec = hpm_package::DependencySpec::registry("1.39.2", None);
    install_one_dep(
        &storage_manager,
        &fetcher,
        None,
        std::slice::from_ref(&installed),
        "tumblehead/tumblepipe",
        &spec,
    )
    .await
    .expect("already-installed dep short-circuits");

    let mode = std::fs::metadata(&tool).unwrap().permissions().mode() & 0o7777;
    assert_eq!(mode, 0o755, "the reused tree must come back runnable");
}

/// Regression: a registry-resolved dep that misses the installed-cache
/// short-circuit, with no registries configured, must fail as
/// `NoRegistriesConfigured` (pointing the user at `hpm registry add`) rather
/// than resolving against the empty set and surfacing a misleading
/// `VersionNotFound`.
#[tokio::test]
async fn install_one_dep_no_registries_reports_not_configured() {
    let temp_dir = TempDir::new().unwrap();
    let (_config, storage_manager) = test_setup(temp_dir.path());
    let fetcher =
        ArchiveFetcher::new(temp_dir.path().join("cache"), temp_dir.path().join("fetch")).unwrap();

    // Empty registry set, and no matching installed package, so the
    // resolution branch is reached with nothing to resolve against.
    let registry_set = crate::registry::RegistrySet::new();
    let spec = hpm_package::DependencySpec::registry("1.12.2", None);
    let err = install_one_dep(
        &storage_manager,
        &fetcher,
        Some(&registry_set),
        &[],
        "tumblepipe",
        &spec,
    )
    .await
    .expect_err("no registries configured must be an error");

    match err {
        ProjectError::NoRegistriesConfigured { name, version_req } => {
            assert_eq!(name, "tumblepipe");
            assert_eq!(version_req, "1.12.2");
        }
        other => panic!("expected NoRegistriesConfigured, got {other:?}"),
    }
}

/// Regression: a Houdini manifest left over from a previous sync (e.g. a
/// dev override that has since been removed) must be swept when its slug
/// no longer appears in the dependency set. Otherwise Houdini keeps
/// loading the stale package on launch.
#[test]
fn sweep_stale_houdini_manifests_removes_orphaned_json() {
    let temp_dir = TempDir::new().unwrap();
    let (config, storage_manager) = test_setup(temp_dir.path());
    let project_root = temp_dir.path().join("project");
    std::fs::create_dir_all(&project_root).unwrap();
    let project_manager = ProjectManager::new(project_root, storage_manager, config).unwrap();

    // Simulate the prior sync's output: creator.foo.json (current dep) and
    // creator.stale.json (dep that left the set).
    let pkg_dir = &project_manager.project_paths.packages_dir;
    let foo_json = pkg_dir.join("creator.foo.json");
    let stale_json = pkg_dir.join("creator.stale.json");
    let unrelated = pkg_dir.join("README.md");
    std::fs::write(&foo_json, b"{}").unwrap();
    std::fs::write(&stale_json, b"{}").unwrap();
    std::fs::write(&unrelated, b"hello").unwrap();

    let manifest = hpm_package::PackageManifest::new(
        PackagePath::new("creator/foo").unwrap(),
        "Foo".to_string(),
        "1.0.0".to_string(),
        None,
        Vec::new(),
        None,
    );
    let installed = InstalledPackage {
        version: "1.0.0".to_string(),
        manifest,
        install_path: temp_dir.path().join("foo@1.0.0"),
        is_dev: false,
    };

    project_manager
        .sweep_stale_houdini_manifests(std::slice::from_ref(&installed))
        .unwrap();

    assert!(foo_json.exists(), "current dep manifest must be kept");
    assert!(!stale_json.exists(), "stale dep manifest must be swept");
    assert!(unrelated.exists(), "non-json files must be left alone");
}

/// Two creators publishing the same slug must each get their own manifest.
/// Before the filename carried the creator, the second package installed
/// silently overwrote the first, and Houdini loaded only one of them.
#[test]
fn manifests_for_colliding_slugs_do_not_overwrite_each_other() {
    let temp_dir = TempDir::new().unwrap();
    let (config, storage_manager) = test_setup(temp_dir.path());
    let project_root = temp_dir.path().join("project");
    std::fs::create_dir_all(&project_root).unwrap();
    let project_manager = ProjectManager::new(project_root, storage_manager, config).unwrap();

    let make = |creator: &str| {
        let manifest = hpm_package::PackageManifest::new(
            PackagePath::new(format!("{creator}/tools")).unwrap(),
            "Tools".to_string(),
            "1.0.0".to_string(),
            None,
            Vec::new(),
            None,
        );
        InstalledPackage {
            version: "1.0.0".to_string(),
            manifest,
            install_path: temp_dir.path().join(format!("{creator}-tools@1.0.0")),
            is_dev: false,
        }
    };
    let a = make("creator-a");
    let b = make("creator-b");

    for pkg in [&a, &b] {
        project_manager
            .generate_houdini_manifest_with_python(pkg, None, &IndexMap::new())
            .unwrap();
    }

    let pkg_dir = &project_manager.project_paths.packages_dir;
    assert!(pkg_dir.join("creator-a.tools.json").exists());
    assert!(pkg_dir.join("creator-b.tools.json").exists());

    // And both survive a sweep that knows about both.
    project_manager
        .sweep_stale_houdini_manifests(&[a, b])
        .unwrap();
    assert!(pkg_dir.join("creator-a.tools.json").exists());
    assert!(pkg_dir.join("creator-b.tools.json").exists());
}

/// Migration: manifests written by an older hpm used the bare slug. They
/// are not in the current naming scheme, so the sweep clears them and the
/// emitter rewrites them under the creator-scoped name — otherwise Houdini
/// would load the package twice, once per filename.
#[test]
fn sweep_removes_legacy_bare_slug_manifests() {
    let temp_dir = TempDir::new().unwrap();
    let (config, storage_manager) = test_setup(temp_dir.path());
    let project_root = temp_dir.path().join("project");
    std::fs::create_dir_all(&project_root).unwrap();
    let project_manager = ProjectManager::new(project_root, storage_manager, config).unwrap();

    let pkg_dir = &project_manager.project_paths.packages_dir;
    let legacy = pkg_dir.join("foo.json");
    std::fs::write(&legacy, b"{}").unwrap();

    let manifest = hpm_package::PackageManifest::new(
        PackagePath::new("creator/foo").unwrap(),
        "Foo".to_string(),
        "1.0.0".to_string(),
        None,
        Vec::new(),
        None,
    );
    let installed = InstalledPackage {
        version: "1.0.0".to_string(),
        manifest,
        install_path: temp_dir.path().join("foo@1.0.0"),
        is_dev: false,
    };

    project_manager
        .sweep_stale_houdini_manifests(std::slice::from_ref(&installed))
        .unwrap();

    assert!(
        !legacy.exists(),
        "legacy bare-slug manifest must be swept so the package loads once"
    );
}

/// An empty dependency set must still sweep prior manifests.
/// This is the dev-override-removed-and-package-disappeared case: the
/// project resolves zero deps, so nothing iterates the json-write loop,
/// and only the sweep can clear the stale manifest.
#[test]
fn sweep_stale_houdini_manifests_empty_set_clears_all_json() {
    let temp_dir = TempDir::new().unwrap();
    let (config, storage_manager) = test_setup(temp_dir.path());
    let project_root = temp_dir.path().join("project");
    std::fs::create_dir_all(&project_root).unwrap();
    let project_manager = ProjectManager::new(project_root, storage_manager, config).unwrap();

    let pkg_dir = &project_manager.project_paths.packages_dir;
    let dev_only = pkg_dir.join("dev-only.json");
    std::fs::write(&dev_only, b"{}").unwrap();

    project_manager.sweep_stale_houdini_manifests(&[]).unwrap();

    assert!(
        !dev_only.exists(),
        "stale manifest must be swept even when the dep set is empty"
    );
}

/// The project overrides manifest is not a per-package file — the sweep
/// must leave it alone even though its stem is not an installed slug.
#[test]
fn sweep_stale_houdini_manifests_keeps_project_overrides_file() {
    let temp_dir = TempDir::new().unwrap();
    let (config, storage_manager) = test_setup(temp_dir.path());
    let project_root = temp_dir.path().join("project");
    std::fs::create_dir_all(&project_root).unwrap();
    let project_manager = ProjectManager::new(project_root, storage_manager, config).unwrap();

    let overrides_file = project_manager
        .project_paths
        .packages_dir
        .join(PROJECT_OVERRIDES_FILE);
    std::fs::write(&overrides_file, b"{}").unwrap();

    project_manager.sweep_stale_houdini_manifests(&[]).unwrap();

    assert!(
        overrides_file.exists(),
        "the project overrides manifest must survive the sweep"
    );
}

/// `write_project_overrides_manifest` writes the file when the project has
/// `[runtime]` entries and removes it again when they are gone, so a stale
/// override never outlives its hpm.toml entry.
#[test]
fn write_project_overrides_manifest_lifecycle() {
    let temp_dir = TempDir::new().unwrap();
    let (config, storage_manager) = test_setup(temp_dir.path());
    let project_root = temp_dir.path().join("project");
    std::fs::create_dir_all(&project_root).unwrap();
    let project_manager = ProjectManager::new(project_root, storage_manager, config).unwrap();

    let overrides_file = project_manager
        .project_paths
        .packages_dir
        .join(PROJECT_OVERRIDES_FILE);

    let mut overrides = IndexMap::new();
    overrides.insert(
        "TOOL_PATH".to_string(),
        ManifestEnvEntry {
            method: hpm_package::EnvMethod::Append,
            value: Some("/studio/tools".into()),
            required: false,
        },
    );
    project_manager
        .write_project_overrides_manifest(&overrides)
        .unwrap();
    assert!(overrides_file.exists());

    // The emitted JSON must be list-valued with a Houdini-valid method.
    let json: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&overrides_file).unwrap()).unwrap();
    let entry = &json["env"][0]["TOOL_PATH"];
    assert_eq!(entry["method"], "append");
    assert_eq!(entry["value"], serde_json::json!(["/studio/tools"]));

    // Empty table: the file is removed. Removing again is a no-op.
    project_manager
        .write_project_overrides_manifest(&IndexMap::new())
        .unwrap();
    assert!(
        !overrides_file.exists(),
        "an empty [runtime] table must remove the overrides manifest"
    );
    project_manager
        .write_project_overrides_manifest(&IndexMap::new())
        .unwrap();
}

// ---- Property test for install_source filter --------------------------

use proptest::prelude::*;

proptest! {
    /// Safety contract: a `[runtime]` entry whose only variant is gated
    /// `install_source = "dev"` never reaches the Houdini manifest for a
    /// non-dev install. The is_dev=false output must match the output
    /// produced from a manifest where the entry is absent.
    #[test]
    fn prop_install_source_dev_inert_for_registry_install(
        value in prop_oneof![
            Just("$HPM_PACKAGE_ROOT/build/Release".to_string()),
            Just("$HPM_PACKAGE_ROOT/dso".to_string()),
            Just("/abs/static".to_string()),
        ],
        key in prop::sample::select(
            vec!["ALPHA_PATH", "BETA_PATH", "GAMMA_PATH"]
        ),
    ) {
        let temp_dir = TempDir::new().unwrap();

        let mut with_dev = hpm_package::PackageManifest::new(
            PackagePath::new("studio/inertness").unwrap(),
            "Inertness".to_string(),
            "1.0.0".to_string(),
            None,
            Vec::new(),
            None);
        let mut runtime = IndexMap::new();
        runtime.insert(key.to_string(), dev_only_runtime_entry(&value));
        with_dev.runtime = runtime;

        let mut without = with_dev.clone();
        without.runtime = Default::default();

        let pkg_path = temp_dir.path().join("inertness@1.0.0");
        std::fs::create_dir_all(&pkg_path).unwrap();

        let a = build_houdini_package(&InstalledPackage {
            version: "1.0.0".to_string(),
            manifest: with_dev,
            install_path: pkg_path.clone(),
            is_dev: false,
        }, None, &IndexMap::new());
        let b = build_houdini_package(&InstalledPackage {
            version: "1.0.0".to_string(),
            manifest: without,
            install_path: pkg_path,
            is_dev: false,
        }, None, &IndexMap::new());

        // Compare via Debug — HoudiniPackage / HoudiniEnvValue don't
        // implement PartialEq, and a regression that fires the dev
        // branch for is_dev=false would diverge here even when both
        // sides succeed.
        prop_assert_eq!(format!("{:?}", a), format!("{:?}", b));
    }
}

/// Regression: `list_dependencies` reads the manifest directory back, so it
/// has to understand the `<creator>.<slug>.json` naming.
///
/// It previously trimmed `.json` and compared the result against the bare
/// slug, which after the rename can never match — every dependency came back
/// with `installed_package: None` and a dotted name, silently turning
/// every consumer of the listing into a no-op.
#[test]
fn list_dependencies_resolves_creator_scoped_manifests() {
    let temp_dir = TempDir::new().unwrap();
    let (config, storage_manager) = test_setup(temp_dir.path());
    let project_root = temp_dir.path().join("project");
    std::fs::create_dir_all(&project_root).unwrap();

    // Put a real package in the store so the lookup has something to find.
    let store_dir = config.storage.packages_dir.join("tools@1.0.0");
    std::fs::create_dir_all(&store_dir).unwrap();
    std::fs::write(
        store_dir.join("hpm.toml"),
        "[package]\npath = \"acme/tools\"\nname = \"Tools\"\nversion = \"1.0.0\"\n",
    )
    .unwrap();

    let project_manager = ProjectManager::new(project_root, storage_manager, config).unwrap();
    let pkg_dir = &project_manager.project_paths.packages_dir;
    std::fs::write(pkg_dir.join("acme.tools.json"), b"{}").unwrap();

    let deps = project_manager.list_dependencies().unwrap();
    assert_eq!(deps.len(), 1, "expected one dependency, got {deps:?}");
    assert_eq!(deps[0].name, "acme/tools", "name must be the scoped path");
    assert!(
        deps[0].installed_package.is_some(),
        "the installed package must be resolved, not reported as missing"
    );
}

/// The installed copy reported for a dependency is the version the project
/// installs, not whichever version the store scan meets first. With
/// TumblePipe 1.40.1 and 1.76.0 both in the store, a project on 1.76.0 was
/// handed 1.40.1, and TumbleTrove Desktop ran that release's (absent)
/// `tt_prepare`, so a launch never migrated the project.
///
/// Exhaustive over which of three installed versions the manifest pins,
/// plus no pin (highest wins), in a store holding all three.
#[test]
fn list_dependencies_reports_the_version_the_project_installs() {
    for pin in [Some("0.9.0"), Some("1.0.0"), Some("1.2.0"), None] {
        let temp_dir = TempDir::new().unwrap();
        let (config, storage_manager) = test_setup(temp_dir.path());
        let project_root = temp_dir.path().join("project");
        std::fs::create_dir_all(&project_root).unwrap();
        for version in ["1.0.0", "0.9.0", "1.2.0"] {
            let dir = config.storage.packages_dir.join(format!("tools@{version}"));
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(
                dir.join("hpm.toml"),
                format!(
                    "[package]\npath = \"acme/tools\"\nname = \"Tools\"\nversion = \"{version}\"\n"
                ),
            )
            .unwrap();
        }
        if let Some(version) = pin {
            std::fs::write(
                project_root.join("hpm.toml"),
                format!(
                    "[package]\npath = \"acme/project\"\nname = \"P\"\nversion = \"0.1.0\"\n\n\
                     [dependencies]\n\"acme/tools\" = \"{version}\"\n"
                ),
            )
            .unwrap();
        }
        let project_manager = ProjectManager::new(project_root, storage_manager, config).unwrap();
        let pkg_dir = &project_manager.project_paths.packages_dir;
        std::fs::write(pkg_dir.join("acme.tools.json"), b"{}").unwrap();

        let deps = project_manager.list_dependencies().unwrap();
        let got = deps[0]
            .installed_package
            .as_ref()
            .map(|p| p.manifest.package.version.clone());
        assert_eq!(got.as_deref(), Some(pin.unwrap_or("1.2.0")), "pin {pin:?}");
    }
}

/// A path dependency's installed copy is its dev install under `_dev/`, the
/// one the emitted Houdini manifest's `hpath` names, never the newest
/// registry copy of the package. A dev launch of TumblePipe 1.89.0 with
/// 1.88.1 in the store was handed 1.88.1, so Desktop ran that release's
/// `tt_prepare` and the project was left a migration short.
///
/// Also: with no readable `hpath`, a path dependency reports no copy rather
/// than a registry one.
#[test]
fn list_dependencies_reports_a_path_dependencys_dev_install() {
    for emitted in [true, false] {
        let temp_dir = TempDir::new().unwrap();
        let (config, storage_manager) = test_setup(temp_dir.path());
        let project_root = temp_dir.path().join("project");
        std::fs::create_dir_all(&project_root).unwrap();
        let manifest = |version: &str| {
            format!("[package]\npath = \"acme/tools\"\nname = \"Tools\"\nversion = \"{version}\"\n")
        };
        let registry = config.storage.packages_dir.join("tools@1.88.1");
        std::fs::create_dir_all(&registry).unwrap();
        std::fs::write(registry.join("hpm.toml"), manifest("1.88.1")).unwrap();
        let dev = config
            .storage
            .packages_dir
            .join("_dev")
            .join("tools@1.89.0")
            .join("c528e8cc");
        std::fs::create_dir_all(&dev).unwrap();
        std::fs::write(dev.join("hpm.toml"), manifest("1.89.0")).unwrap();
        std::fs::write(
            project_root.join("hpm.toml"),
            "[package]\npath = \"acme/project\"\nname = \"P\"\nversion = \"0.1.0\"\n\n\
             [dependencies]\n\"acme/tools\" = { path = \"C:/work/tools\", link = true }\n",
        )
        .unwrap();
        let project_manager = ProjectManager::new(project_root, storage_manager, config).unwrap();
        let pkg_dir = &project_manager.project_paths.packages_dir;
        let body = if emitted {
            serde_json::json!({ "hpath": [dev.to_string_lossy()] }).to_string()
        } else {
            "{}".to_string()
        };
        std::fs::write(pkg_dir.join("acme.tools.json"), body).unwrap();

        let deps = project_manager.list_dependencies().unwrap();
        let got = deps[0].installed_package.as_ref();
        if emitted {
            let got = got.expect("the dev install must be found");
            assert_eq!(got.manifest.package.version, "1.89.0");
            assert_eq!(got.install_path, dev);
            assert!(got.is_dev);
        } else {
            assert!(got.is_none(), "no hpath: got {got:?}, want none");
        }
    }
}

/// A registry dependency follows the emitted `hpath` too, so it is reported
/// as the copy Houdini loads, and an `hpath` outside the store is ignored
/// in favour of the pinned version.
#[test]
fn list_dependencies_follows_the_emitted_hpath() {
    for (target, want) in [("1.0.0", "1.0.0"), ("1.2.0", "1.2.0"), ("outside", "1.2.0")] {
        let temp_dir = TempDir::new().unwrap();
        let (config, storage_manager) = test_setup(temp_dir.path());
        let project_root = temp_dir.path().join("project");
        std::fs::create_dir_all(&project_root).unwrap();
        let manifest = |version: &str| {
            format!("[package]\npath = \"acme/tools\"\nname = \"Tools\"\nversion = \"{version}\"\n")
        };
        for version in ["1.0.0", "1.2.0"] {
            let dir = config.storage.packages_dir.join(format!("tools@{version}"));
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("hpm.toml"), manifest(version)).unwrap();
        }
        let outside = temp_dir.path().join("elsewhere").join("tools@1.0.0");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("hpm.toml"), manifest("1.0.0")).unwrap();
        std::fs::write(
            project_root.join("hpm.toml"),
            "[package]\npath = \"acme/project\"\nname = \"P\"\nversion = \"0.1.0\"\n\n\
             [dependencies]\n\"acme/tools\" = \"1.2.0\"\n",
        )
        .unwrap();
        let hpath = if target == "outside" {
            outside.clone()
        } else {
            config.storage.packages_dir.join(format!("tools@{target}"))
        };
        let project_manager = ProjectManager::new(project_root, storage_manager, config).unwrap();
        let pkg_dir = &project_manager.project_paths.packages_dir;
        std::fs::write(
            pkg_dir.join("acme.tools.json"),
            serde_json::json!({ "hpath": [hpath.to_string_lossy()] }).to_string(),
        )
        .unwrap();

        let deps = project_manager.list_dependencies().unwrap();
        let got = deps[0]
            .installed_package
            .as_ref()
            .map(|p| p.manifest.package.version.as_str());
        assert_eq!(got, Some(want), "hpath at {target}");
    }
}

/// What a dependency's emitted Houdini manifest says, for the
/// `list_dependencies` property below.
#[derive(Debug, Clone)]
enum Emitted {
    /// `hpath` names the registry copy of this version (installed or not).
    Registry(&'static str),
    /// `hpath` names the dev install (present or not).
    Dev,
    /// `hpath` names a package directory outside the store.
    Outside,
    /// `hpath` names an installed copy of a *different* package.
    Foreign,
    /// A manifest with no `hpath`.
    NoHpath,
    /// Not JSON at all.
    Malformed,
}

/// How the project's `hpm.toml` declares the dependency.
#[derive(Debug, Clone)]
enum Declared {
    NoManifest,
    Pinned(&'static str),
    Path,
}

const VERSIONS: [&str; 4] = ["0.9.0", "1.0.0", "1.2.0", "1.88.1"];

fn emitted_strategy() -> impl Strategy<Value = Emitted> {
    prop_oneof![
        prop::sample::select(VERSIONS.to_vec()).prop_map(Emitted::Registry),
        Just(Emitted::Dev),
        Just(Emitted::Outside),
        Just(Emitted::Foreign),
        Just(Emitted::NoHpath),
        Just(Emitted::Malformed),
    ]
}

fn declared_strategy() -> impl Strategy<Value = Declared> {
    prop_oneof![
        Just(Declared::NoManifest),
        // "2.0.0" is never installed: a pin the store cannot satisfy.
        prop::sample::select(vec!["0.9.0", "1.0.0", "1.2.0", "1.88.1", "2.0.0"])
            .prop_map(Declared::Pinned),
        Just(Declared::Path),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(96))]

    /// `list_dependencies` reports, for any store (some registry versions,
    /// maybe a dev install) and any way the project declares and emits the
    /// dependency, exactly the copy this oracle names:
    ///
    /// 1. the install the emitted `hpath` names, when it is one of this
    ///    package's installs inside the store, dev installs included;
    /// 2. else nothing for a path dependency (never a registry build);
    /// 3. else the pinned version if installed, nothing if not;
    /// 4. else (no manifest) the highest registry version installed.
    ///
    /// Rule 1 for dev installs and rule 2 are the 2026-11-02 regression: a
    /// dev launch of TumblePipe 1.89.0 was handed the registry 1.88.1.
    #[test]
    fn prop_list_dependencies_reports_the_copy_houdini_loads(
        installed in prop::collection::vec(any::<bool>(), VERSIONS.len()),
        dev_present in any::<bool>(),
        emitted in emitted_strategy(),
        declared in declared_strategy(),
    ) {
        let temp_dir = TempDir::new().unwrap();
        let (config, storage_manager) = test_setup(temp_dir.path());
        let store = config.storage.packages_dir.clone();
        let project_root = temp_dir.path().join("project");
        std::fs::create_dir_all(&project_root).unwrap();
        let manifest = |path: &str, version: &str| {
            format!("[package]\npath = \"{path}\"\nname = \"T\"\nversion = \"{version}\"\n")
        };
        let registry_dir = |v: &str| store.join(format!("tools@{v}"));
        for (v, present) in VERSIONS.iter().zip(&installed) {
            if *present {
                std::fs::create_dir_all(registry_dir(v)).unwrap();
                std::fs::write(registry_dir(v).join("hpm.toml"), manifest("acme/tools", v)).unwrap();
            }
        }
        let dev_dir = store.join("_dev").join("tools@1.89.0").join("c528e8cc");
        if dev_present {
            std::fs::create_dir_all(&dev_dir).unwrap();
            std::fs::write(dev_dir.join("hpm.toml"), manifest("acme/tools", "1.89.0")).unwrap();
        }
        let foreign_dir = store.join("other@1.0.0");
        std::fs::create_dir_all(&foreign_dir).unwrap();
        std::fs::write(foreign_dir.join("hpm.toml"), manifest("acme/other", "1.0.0")).unwrap();
        let outside_dir = temp_dir.path().join("elsewhere").join("tools@1.0.0");
        std::fs::create_dir_all(&outside_dir).unwrap();
        std::fs::write(outside_dir.join("hpm.toml"), manifest("acme/tools", "1.0.0")).unwrap();

        let dependency = match &declared {
            Declared::NoManifest => None,
            Declared::Pinned(v) => Some(format!("\"{v}\"")),
            Declared::Path => Some("{ path = \"C:/work/tools\", link = true }".to_string()),
        };
        if let Some(dep) = dependency {
            std::fs::write(
                project_root.join("hpm.toml"),
                format!(
                    "[package]\npath = \"acme/project\"\nname = \"P\"\nversion = \"0.1.0\"\n\n\
                     [dependencies]\n\"acme/tools\" = {dep}\n"
                ),
            )
            .unwrap();
        }
        let project_manager = ProjectManager::new(project_root, storage_manager, config).unwrap();
        let hpath_body = |dir: &Path| serde_json::json!({ "hpath": [dir.to_string_lossy()] }).to_string();
        let body = match &emitted {
            Emitted::Registry(v) => hpath_body(&registry_dir(v)),
            Emitted::Dev => hpath_body(&dev_dir),
            Emitted::Outside => hpath_body(&outside_dir),
            Emitted::Foreign => hpath_body(&foreign_dir),
            Emitted::NoHpath => "{}".to_string(),
            Emitted::Malformed => "not json".to_string(),
        };
        std::fs::write(project_manager.project_paths.packages_dir.join("acme.tools.json"), body)
            .unwrap();

        // The oracle.
        let is_installed = |v: &str| {
            VERSIONS.iter().zip(&installed).any(|(x, present)| *x == v && *present)
        };
        let from_hpath = match &emitted {
            Emitted::Registry(v) if is_installed(v) => Some((v.to_string(), false)),
            Emitted::Dev if dev_present => Some(("1.89.0".to_string(), true)),
            _ => None,
        };
        let want = from_hpath.or_else(|| match &declared {
            Declared::Path => None,
            Declared::Pinned(v) => is_installed(v).then(|| (v.to_string(), false)),
            Declared::NoManifest => VERSIONS
                .iter()
                .zip(&installed)
                .filter(|(_, present)| **present)
                .map(|(v, _)| semver::Version::parse(v).unwrap())
                .max()
                .map(|v| (v.to_string(), false)),
        });

        let deps = project_manager.list_dependencies().unwrap();
        prop_assert_eq!(deps.len(), 1);
        let got = deps[0]
            .installed_package
            .as_ref()
            .map(|p| (p.manifest.package.version.clone(), p.is_dev));
        prop_assert_eq!(got, want);
    }
}

/// A pinned version that is not in the store resolves to nothing, never to
/// another installed version of the package.
#[test]
fn list_dependencies_never_substitutes_another_version() {
    let temp_dir = TempDir::new().unwrap();
    let (config, storage_manager) = test_setup(temp_dir.path());
    let project_root = temp_dir.path().join("project");
    std::fs::create_dir_all(&project_root).unwrap();
    let dir = config.storage.packages_dir.join("tools@1.0.0");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("hpm.toml"),
        "[package]\npath = \"acme/tools\"\nname = \"Tools\"\nversion = \"1.0.0\"\n",
    )
    .unwrap();
    std::fs::write(
        project_root.join("hpm.toml"),
        "[package]\npath = \"acme/project\"\nname = \"P\"\nversion = \"0.1.0\"\n\n\
         [dependencies]\n\"acme/tools\" = \"2.0.0\"\n",
    )
    .unwrap();
    let project_manager = ProjectManager::new(project_root, storage_manager, config).unwrap();
    std::fs::write(
        project_manager
            .project_paths
            .packages_dir
            .join("acme.tools.json"),
        b"{}",
    )
    .unwrap();
    let deps = project_manager.list_dependencies().unwrap();
    assert!(deps[0].installed_package.is_none(), "{deps:?}");
}

/// Files in the manifest directory that are not per-package manifests must
/// not surface as dependencies — they would carry a name matching no
/// installed package.
#[test]
fn list_dependencies_ignores_non_package_manifests() {
    let temp_dir = TempDir::new().unwrap();
    let (config, storage_manager) = test_setup(temp_dir.path());
    let project_root = temp_dir.path().join("project");
    std::fs::create_dir_all(&project_root).unwrap();
    let project_manager = ProjectManager::new(project_root, storage_manager, config).unwrap();

    let pkg_dir = &project_manager.project_paths.packages_dir;
    std::fs::write(pkg_dir.join(PROJECT_OVERRIDES_FILE), b"{}").unwrap();
    std::fs::write(pkg_dir.join("legacy-bare-slug.json"), b"{}").unwrap();
    std::fs::write(pkg_dir.join("README.md"), b"hi").unwrap();

    let deps = project_manager.list_dependencies().unwrap();
    assert!(deps.is_empty(), "expected no dependencies, got {deps:?}");
}

/// Hash directories in a dev container, sorted.
fn dev_hashes(container: &Path) -> Vec<String> {
    let mut out: Vec<String> = std::fs::read_dir(container)
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| n.len() == 64)
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out
}

/// The reclaim after a sync runs detached; wait for the container to hold
/// `want` copies.
async fn wait_for_dev_hashes(container: &Path, want: usize) -> Vec<String> {
    for _ in 0..200 {
        let got = dev_hashes(container);
        if got.len() == want {
            return got;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    dev_hashes(container)
}

/// A dev copy lives exactly as long as a project's manifest points at it.
/// Each sync after a workspace change used to add a full copy that nothing
/// removed (~490 copies, ~500 GB on one machine). Now the sync that moves a
/// project to a new copy reclaims the one it left, and keeps a copy another
/// project still points at.
#[tokio::test(flavor = "multi_thread")]
async fn sync_reclaims_the_dev_copy_it_supersedes_and_no_other() {
    let temp_dir = TempDir::new().unwrap();
    let (config, storage_manager) = test_setup(temp_dir.path());
    let workspace = temp_dir.path().join("workspace");
    std::fs::create_dir_all(workspace.join("otls")).unwrap();
    std::fs::write(
        workspace.join("hpm.toml"),
        "[package]\npath = \"acme/rig\"\nname = \"Rig\"\nversion = \"1.0.0\"\n",
    )
    .unwrap();
    std::fs::write(workspace.join("otls/rig.hda"), "v1").unwrap();

    let project = |name: &str| {
        let root = temp_dir.path().join(name);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("hpm.toml"),
            format!(
                "[package]\npath = \"acme/{name}\"\nname = \"P\"\nversion = \"0.1.0\"\n\n\
                 [dependencies]\n\"acme/rig\" = {{ path = \"{}\" }}\n",
                workspace.to_string_lossy().replace('\\', "/")
            ),
        )
        .unwrap();
        ProjectManager::new(root, storage_manager.clone(), config.clone()).unwrap()
    };
    let a = project("a");
    let b = project("b");
    let container = config.storage.packages_dir.join("_dev").join("rig@1.0.0");
    let hpath_of = |pm: &ProjectManager| {
        let json: serde_json::Value = serde_json::from_slice(
            &std::fs::read(pm.project_paths.packages_dir.join("acme.rig.json")).unwrap(),
        )
        .unwrap();
        PathBuf::from(json["hpath"][0].as_str().unwrap())
    };

    a.sync_dependencies().await.unwrap();
    b.sync_dependencies().await.unwrap();
    let first = wait_for_dev_hashes(&container, 1).await;
    assert_eq!(first.len(), 1, "both projects share the one copy");

    // The workspace changes; only A syncs. B still points at the old copy.
    std::fs::write(workspace.join("otls/rig.hda"), "version two").unwrap();
    a.sync_dependencies().await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    let both = dev_hashes(&container);
    assert_eq!(
        both.len(),
        2,
        "B's copy is kept while B points at it: {both:?}"
    );
    assert!(hpath_of(&b).is_dir());
    assert_ne!(hpath_of(&a), hpath_of(&b));

    // B syncs too: nothing points at the first copy any more.
    b.sync_dependencies().await.unwrap();
    let last = wait_for_dev_hashes(&container, 1).await;
    assert_eq!(last.len(), 1, "the superseded copy is reclaimed: {last:?}");
    assert_eq!(hpath_of(&a), container.join(&last[0]));
    assert_eq!(hpath_of(&b), container.join(&last[0]));
    assert!(container.join(&last[0]).join("otls/rig.hda").is_file());

    // An unchanged sync reuses the copy and reclaims nothing.
    a.sync_dependencies().await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    assert_eq!(dev_hashes(&container), last);
}

mod dev_copy_props {
    use super::*;
    use std::collections::BTreeSet;

    #[derive(Debug, Clone, Copy)]
    enum Step {
        /// A shipped file changes (a rebuilt DSO, a saved HDA).
        Edit,
        /// Project 0 or 1 syncs (a dev launch, a hython run's pre-sync).
        Sync(usize),
        /// A run against project 0 or 1 starts and holds its copies.
        Hold(usize),
        /// Every run ends.
        Release,
        /// The package's version goes up: its next sync installs into a new
        /// container, leaving the old one to the same-package sweep.
        Bump,
    }

    fn step() -> impl Strategy<Value = Step> {
        prop_oneof![
            2 => Just(Step::Edit),
            4 => (0usize..2).prop_map(Step::Sync),
            1 => (0usize..2).prop_map(Step::Hold),
            1 => Just(Step::Release),
            1 => Just(Step::Bump),
        ]
    }

    fn hpath_of(pm: &ProjectManager) -> Option<PathBuf> {
        let bytes = std::fs::read(pm.project_paths.packages_dir.join("acme.rig.json")).ok()?;
        let json: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
        Some(PathBuf::from(json["hpath"][0].as_str()?))
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(24))]

        /// Over any interleaving of workspace edits, syncs of two projects
        /// sharing a dev package, and runs holding a project's copies, with
        /// each sync's reclaim running in the background against the steps
        /// after it:
        ///
        /// - **after every step**, every copy a project points at and every
        ///   copy a run holds exists, complete: no reclaim ever takes a copy
        ///   from under a user;
        /// - **at the end**, a reclaim leaves exactly those copies: disk use
        ///   is bounded by projects and runs, not by launches.
        #[test]
        fn prop_copies_live_exactly_as_long_as_they_are_used(
            steps in prop::collection::vec(step(), 1..12),
        ) {
            let temp_dir = TempDir::new().unwrap();
            let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
            rt.block_on(async {
                let (config, storage_manager) = test_setup(temp_dir.path());
                let workspace = temp_dir.path().join("workspace");
                std::fs::create_dir_all(workspace.join("otls")).unwrap();
                let write_version = |minor: usize| {
                    std::fs::write(
                        workspace.join("hpm.toml"),
                        format!(
                            "[package]\npath = \"acme/rig\"\nname = \"Rig\"\nversion = \"1.{minor}.0\"\n"
                        ),
                    )
                    .unwrap();
                };
                let mut minor = 0usize;
                write_version(minor);
                std::fs::write(workspace.join("otls/rig.hda"), "x").unwrap();
                let projects: Vec<ProjectManager> = (0..2)
                    .map(|i| {
                        let root = temp_dir.path().join(format!("p{i}"));
                        std::fs::create_dir_all(&root).unwrap();
                        std::fs::write(
                            root.join("hpm.toml"),
                            format!(
                                "[package]\npath = \"acme/p{i}\"\nname = \"P\"\nversion = \"0.1.0\"\n\n\
                                 [dependencies]\n\"acme/rig\" = {{ path = \"{}\" }}\n",
                                workspace.to_string_lossy().replace('\\', "/")
                            ),
                        )
                        .unwrap();
                        ProjectManager::new(root, storage_manager.clone(), config.clone()).unwrap()
                    })
                    .collect();
                let dev_root = config.storage.packages_dir.join("_dev");
                let mut edits = 1usize;
                // What the runs hold, by the model (the copy the project
                // points at when the run starts), kept apart from the holds
                // themselves so a hold that protects nothing cannot also
                // shrink what the property checks.
                let mut held: Vec<PathBuf> = Vec::new();
                let mut holds: Vec<crate::storage::dev_users::DevCopyHold> = Vec::new();

                for step in &steps {
                    match *step {
                        Step::Edit => {
                            edits += 1;
                            // A new size, so the hash moves whatever the
                            // filesystem's mtime resolution.
                            std::fs::write(workspace.join("otls/rig.hda"), "x".repeat(edits)).unwrap();
                        }
                        Step::Sync(i) => {
                            projects[i].sync_dependencies().await.unwrap();
                        }
                        Step::Hold(i) => {
                            let packages = &projects[i].project_paths.packages_dir;
                            holds.extend(crate::storage::dev_users::hold_project_dev_copies(packages));
                            held.extend(hpath_of(&projects[i]));
                        }
                        Step::Release => {
                            holds.clear();
                            held.clear();
                        }
                        Step::Bump => {
                            minor += 1;
                            write_version(minor);
                        }
                    }
                    let in_use: BTreeSet<PathBuf> = projects
                        .iter()
                        .filter_map(hpath_of)
                        .chain(held.iter().cloned())
                        .collect();
                    for copy in &in_use {
                        prop_assert!(
                            copy.join("hpm.toml").is_file() && copy.join("otls/rig.hda").is_file(),
                            "{} is in use but gone after {:?}", copy.display(), step
                        );
                    }
                }

                let in_use: BTreeSet<PathBuf> = projects
                    .iter()
                    .filter_map(hpath_of)
                    .chain(held.iter().cloned())
                    .collect();
                // A final pass over every version's container, as the
                // same-package sweep after a sync would make it.
                let containers: Vec<PathBuf> = std::fs::read_dir(&dev_root)
                    .map(|entries| entries.flatten().map(|e| e.path()).collect())
                    .unwrap_or_default();
                for container in &containers {
                    crate::storage::dev_users::reclaim_dev_copies(
                        container,
                        None,
                        crate::storage::dev_users::Unrecorded::Keep,
                        crate::process_users::process_state,
                    );
                    crate::storage::dev_users::remove_empty_container(container);
                }
                let mut left: BTreeSet<PathBuf> = BTreeSet::new();
                let mut containers_left: BTreeSet<PathBuf> = BTreeSet::new();
                for container in std::fs::read_dir(&dev_root)
                    .map(|entries| entries.flatten().map(|e| e.path()).collect::<Vec<_>>())
                    .unwrap_or_default()
                {
                    containers_left.insert(container.clone());
                    for entry in std::fs::read_dir(&container).unwrap().flatten() {
                        if entry.file_name().len() == 64 {
                            left.insert(entry.path());
                        }
                    }
                }
                let containers_in_use: BTreeSet<PathBuf> =
                    in_use.iter().filter_map(|p| p.parent().map(Path::to_path_buf)).collect();
                prop_assert_eq!(containers_left, containers_in_use);
                prop_assert_eq!(left, in_use);
                Ok(())
            })?;
        }
    }
}

/// When a project moves to a new version of a dev package, the old version's
/// container is reclaimed by the same-package sweep, but only once no project
/// points at it: unlike an age or version-order rule, another project still
/// on the old version keeps its copy.
#[tokio::test(flavor = "multi_thread")]
async fn a_new_version_reclaims_the_old_container_once_nothing_points_at_it() {
    let temp_dir = TempDir::new().unwrap();
    let (config, storage_manager) = test_setup(temp_dir.path());
    let workspace = temp_dir.path().join("workspace");
    std::fs::create_dir_all(workspace.join("otls")).unwrap();
    let version = |v: &str| {
        std::fs::write(
            workspace.join("hpm.toml"),
            format!("[package]\npath = \"acme/rig\"\nname = \"Rig\"\nversion = \"{v}\"\n"),
        )
        .unwrap();
    };
    version("1.0.0");
    std::fs::write(workspace.join("otls/rig.hda"), "v1").unwrap();
    let project = |name: &str| {
        let root = temp_dir.path().join(name);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("hpm.toml"),
            format!(
                "[package]\npath = \"acme/{name}\"\nname = \"P\"\nversion = \"0.1.0\"\n\n\
                 [dependencies]\n\"acme/rig\" = {{ path = \"{}\" }}\n",
                workspace.to_string_lossy().replace('\\', "/")
            ),
        )
        .unwrap();
        ProjectManager::new(root, storage_manager.clone(), config.clone()).unwrap()
    };
    let a = project("a");
    let b = project("b");
    a.sync_dependencies().await.unwrap();
    b.sync_dependencies().await.unwrap();
    let old = config.storage.packages_dir.join("_dev").join("rig@1.0.0");
    assert_eq!(wait_for_dev_hashes(&old, 1).await.len(), 1);

    version("1.1.0");
    a.sync_dependencies().await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    assert_eq!(
        dev_hashes(&old).len(),
        1,
        "B still points at 1.0.0; the container holds {:?}",
        walkdir::WalkDir::new(&old)
            .into_iter()
            .flatten()
            .map(|e| e.path().strip_prefix(&old).unwrap().display().to_string())
            .collect::<Vec<_>>()
    );

    b.sync_dependencies().await.unwrap();
    for _ in 0..200 {
        if !old.exists() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    assert!(!old.exists(), "1.0.0 is gone once nothing points at it");
    let new = config.storage.packages_dir.join("_dev").join("rig@1.1.0");
    assert_eq!(dev_hashes(&new).len(), 1);
}

mod lock_building {
    use super::*;
    use crate::lock::{LockFile, LockedDependency, LockedSource};

    fn outcome(
        name: &str,
        version: &str,
        checksum: Option<&str>,
        source: Option<LockedSource>,
    ) -> (String, InstallOutcome) {
        let manifest = hpm_package::parse_manifest_str(&format!(
            "[package]\npath = \"{name}\"\nname = \"x\"\nversion = \"{version}\"\n"
        ))
        .unwrap();
        (
            name.to_string(),
            InstallOutcome {
                package: InstalledPackage {
                    version: version.to_string(),
                    manifest,
                    install_path: PathBuf::from("/store").join(version),
                    is_dev: false,
                },
                checksum: checksum.map(str::to_string),
                source,
            },
        )
    }

    proptest::proptest! {
        #![proptest_config(proptest::prelude::ProptestConfig::with_cases(64))]

        /// `--frozen-lockfile`'s check: a lock built from a sync is current
        /// for that sync, through a save and load too, and stops being
        /// current when any dependency version, the dependency set, a
        /// Python requirement or the project's version changes.
        #[test]
        fn a_lock_is_current_exactly_for_the_sync_it_records(
            deps in proptest::collection::btree_map("acme/[a-z]{1,6}", (0u8..5, 0u8..5), 1..6),
            python in proptest::collection::btree_map("[a-z]{1,6}", "(>=|==)1\\.[0-9]", 0..4),
            which in proptest::prelude::any::<proptest::sample::Index>(),
        ) {
            let temp = TempDir::new().unwrap();
            let manifest_text = |version: &str, python: &std::collections::BTreeMap<String, String>| {
                let mut text = format!(
                    "[package]\npath = \"studio/proj\"\nname = \"proj\"\nversion = \"{version}\"\n\n[python_dependencies]\n"
                );
                for (name, req) in python {
                    text += &format!("{name} = \"{req}\"\n");
                }
                text
            };
            let manifest = hpm_package::parse_manifest_str(&manifest_text("0.1.0", &python)).unwrap();
            let outcomes: Vec<_> = deps
                .iter()
                .map(|(name, (major, minor))| {
                    let version = format!("{major}.{minor}.0");
                    let source = LockedSource::url(format!("https://r/{name}.zip"), version.clone());
                    outcome(name, &version, Some("aa"), Some(source))
                })
                .collect();
            let rt = tokio::runtime::Runtime::new().unwrap();
            let lock = rt.block_on(manager(&temp).lock_file(&manifest, &outcomes, None)).unwrap();
            proptest::prop_assert!(ProjectManager::lock_is_current(&manifest, &outcomes, &lock));
            let saved = temp.path().join("hpm.lock");
            lock.save(&saved).unwrap();
            let loaded = LockFile::load(&saved).unwrap();
            proptest::prop_assert!(ProjectManager::lock_is_current(&manifest, &outcomes, &loaded));

            let i = which.index(outcomes.len());
            let mut bumped = outcomes.clone();
            bumped[i].1.package.version = "9.9.9".to_string();
            proptest::prop_assert!(!ProjectManager::lock_is_current(&manifest, &bumped, &lock));
            let mut fewer = outcomes.clone();
            fewer.remove(i);
            proptest::prop_assert!(!ProjectManager::lock_is_current(&manifest, &fewer, &lock));
            let mut more_python = python.clone();
            more_python.insert("new0".to_string(), ">=1.0".to_string());
            let more = hpm_package::parse_manifest_str(&manifest_text("0.1.0", &more_python)).unwrap();
            proptest::prop_assert!(!ProjectManager::lock_is_current(&more, &outcomes, &lock));
            if let Some(dropped) = python.keys().next() {
                let mut fewer_python = python.clone();
                fewer_python.remove(dropped);
                let fewer = hpm_package::parse_manifest_str(&manifest_text("0.1.0", &fewer_python)).unwrap();
                proptest::prop_assert!(!ProjectManager::lock_is_current(&fewer, &outcomes, &lock));
            }
            let other_version = hpm_package::parse_manifest_str(&manifest_text("0.2.0", &python)).unwrap();
            proptest::prop_assert!(!ProjectManager::lock_is_current(&other_version, &outcomes, &lock));
        }
    }

    fn project() -> hpm_package::PackageManifest {
        hpm_package::parse_manifest_str(
            "[package]\npath = \"studio/proj\"\nname = \"proj\"\nversion = \"0.1.0\"\n\n\
             [dependencies]\n\"acme/cached\" = \"2.0.0\"\n\n\
             [python_dependencies]\nnumpy = \">=1.24\"\nrequests = { extras = [\"socks\"] }\n",
        )
        .unwrap()
    }

    fn manager(temp: &TempDir) -> ProjectManager {
        let (config, storage) = test_setup(temp.path());
        let root = temp.path().join("project");
        std::fs::create_dir_all(&root).unwrap();
        ProjectManager::new(root, storage, config).unwrap()
    }

    fn prior_with(source: LockedSource) -> LockFile {
        let mut prior = LockFile::new("proj".into(), "0.1.0".into());
        prior.add_dependency(
            "acme/cached".into(),
            LockedDependency {
                version: "2.0.0".into(),
                checksum: Some("bb".into()),
                source,
            },
        );
        prior
    }

    /// A fresh fetch records what it fetched; an install found in the store
    /// keeps the previous lock entry's checksum and source; a path
    /// dependency records its path; Python dependencies record their
    /// requirement (`*` when none).
    #[tokio::test]
    async fn the_lock_records_each_install_and_keeps_what_a_store_hit_cannot_know() {
        let temp = TempDir::new().unwrap();
        let fetched = LockedSource::url("https://r/tools-1.0.0.zip", "1.0.0");
        let path = LockedSource::path("../rig");
        let outcomes = vec![
            outcome("acme/tools", "1.0.0", Some("aa"), Some(fetched.clone())),
            outcome("acme/cached", "2.0.0", None, None),
            outcome("acme/rig", "0.1.0", None, Some(path.clone())),
        ];
        let prior_source = LockedSource::url("https://r/cached-2.0.0.zip", "2.0.0");
        let lock = manager(&temp)
            .lock_file(
                &project(),
                &outcomes,
                Some(&prior_with(prior_source.clone())),
            )
            .await
            .unwrap();
        let dep = |n: &str| lock.get_dependency(n).unwrap().clone();
        assert_eq!(dep("acme/tools").source, fetched);
        assert_eq!(dep("acme/tools").checksum.as_deref(), Some("aa"));
        assert_eq!(dep("acme/cached").source, prior_source);
        assert_eq!(dep("acme/cached").checksum.as_deref(), Some("bb"));
        assert_eq!(dep("acme/rig").source, path);
        assert_eq!(lock.python_dependencies["numpy"].version, ">=1.24");
        assert_eq!(lock.python_dependencies["requests"].version, "*");
        assert_eq!(lock.package.name, "proj");
    }

    /// A store hit with no usable previous entry (none, or an older hpm's
    /// `"unresolved"` placeholder) has its source looked up in the
    /// project's registries; it is never written as a URL nobody can fetch.
    /// With no registry to ask, building the lock fails and says why.
    #[tokio::test]
    async fn a_store_hit_without_a_known_source_is_resolved_never_invented() {
        let temp = TempDir::new().unwrap();
        let manager = manager(&temp);
        let outcomes = vec![outcome("acme/cached", "2.0.0", None, None)];
        let placeholder = prior_with(LockedSource::url("unresolved", "2.0.0"));
        for prior in [None, Some(&placeholder)] {
            match manager.lock_file(&project(), &outcomes, prior).await {
                Err(ProjectError::RegistryResolution {
                    name, version_req, ..
                }) => {
                    assert_eq!(
                        (name.as_str(), version_req.as_str()),
                        ("acme/cached", "2.0.0")
                    );
                }
                other => panic!("expected a resolution error, got {other:?}"),
            }
        }
    }
}

mod package_env_not_ready {
    use super::*;

    fn manager_at(temp: &TempDir, manifest: Option<&str>) -> ProjectManager {
        let (config, storage) = test_setup(temp.path());
        let root = temp.path().join("project");
        std::fs::create_dir_all(&root).unwrap();
        if let Some(m) = manifest {
            std::fs::write(root.join("hpm.toml"), m).unwrap();
        }
        ProjectManager::new(root, storage, config).unwrap()
    }

    const WITH_DEP: &str = "[package]\npath = \"studio/proj\"\nname = \"proj\"\nversion = \"0.1.0\"\n\n[dependencies]\n\"acme/tools\" = \"1.0.0\"\n";

    async fn message(manager: &ProjectManager) -> String {
        match manager.resolve_package_env(&[]).await {
            Err(
                e @ (ProjectError::PackageEnvNoManifest { .. }
                | ProjectError::PackageEnvNoLock { .. }
                | ProjectError::PackageEnvNotInstalled { .. }),
            ) => e.to_string(),
            other => panic!("expected a not-ready error, got {other:?}"),
        }
    }

    /// Each way a package environment is not ready yet is reported before
    /// anything is resolved, with a message that says what to do. The text
    /// is shown to users (by TumbleTrove Desktop too).
    #[tokio::test]
    async fn each_not_ready_case_says_what_to_do() {
        let temp = TempDir::new().unwrap();
        let no_manifest = message(&manager_at(&temp, None)).await;
        assert!(
            no_manifest.starts_with("No hpm.toml found at "),
            "{no_manifest}"
        );
        assert!(
            no_manifest.ends_with(" — a package environment needs a project manifest."),
            "{no_manifest}"
        );

        let temp = TempDir::new().unwrap();
        let no_lock = message(&manager_at(&temp, Some(WITH_DEP))).await;
        assert!(no_lock.starts_with("Could not read "), "{no_lock}");
        assert!(
            no_lock.ends_with(
                "Run 'hpm install' to resolve and lock this project's dependencies before running a package-env script."
            ),
            "{no_lock}"
        );

        let temp = TempDir::new().unwrap();
        let manager = manager_at(&temp, Some(WITH_DEP));
        let mut lock = crate::lock::LockFile::new("proj".into(), "0.1.0".into());
        lock.add_dependency(
            "acme/tools".into(),
            crate::lock::LockedDependency {
                version: "1.0.0".into(),
                checksum: None,
                source: crate::lock::LockedSource::url("https://r/t.zip", "1.0.0"),
            },
        );
        lock.save(&temp.path().join("project").join("hpm.lock"))
            .unwrap();
        assert_eq!(
            message(&manager).await,
            "Dependency acme/tools@1.0.0 is locked but not installed. Run 'hpm install' to populate the package environment."
        );
    }

    /// With nothing declaring Python dependencies, the environment is the
    /// project's own `python/` first, then each locked dependency's, and no
    /// venv (uv is not even fetched).
    #[tokio::test]
    async fn the_package_env_puts_the_project_python_first_and_builds_no_empty_venv() {
        let temp = TempDir::new().unwrap();
        let manager = manager_at(&temp, Some(WITH_DEP));
        let project = temp.path().join("project");
        std::fs::create_dir_all(project.join("python")).unwrap();
        let source = temp.path().join("tools-src");
        std::fs::create_dir_all(source.join("python")).unwrap();
        std::fs::write(
            source.join("hpm.toml"),
            "[package]\npath = \"acme/tools\"\nname = \"t\"\nversion = \"1.0.0\"\n",
        )
        .unwrap();
        let installed = manager
            .storage_manager
            .install_into_cas(&source)
            .await
            .unwrap();
        let mut lock = crate::lock::LockFile::new("proj".into(), "0.1.0".into());
        lock.add_dependency(
            "acme/tools".into(),
            crate::lock::LockedDependency {
                version: "1.0.0".into(),
                checksum: None,
                source: crate::lock::LockedSource::url("https://r/t.zip", "1.0.0"),
            },
        );
        lock.save(&project.join("hpm.lock")).unwrap();

        let env = manager.resolve_package_env(&[]).await.unwrap();
        assert_eq!(
            env.python_paths,
            vec![
                project.join("python"),
                installed.install_path.join("python")
            ]
        );
        assert_eq!((env.venv_bin, env.virtual_env), (None, None));
    }
}

/// `[install] parallel_downloads` bounds how many installs a sync runs at
/// once: four URL dependencies, served slowly by a local server that records
/// how many requests overlap, never overlap with a limit of 1, and do with 4.
#[tokio::test(flavor = "multi_thread")]
async fn parallel_downloads_bounds_concurrent_installs() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let temp_dir = TempDir::new().unwrap();
    let srv = temp_dir.path().join("srv");
    for i in 0..4 {
        let pkg = temp_dir.path().join(format!("src{i}"));
        std::fs::create_dir_all(pkg.join("otls")).unwrap();
        std::fs::write(pkg.join("otls/a.hda"), format!("pkg {i}")).unwrap();
        std::fs::write(
            pkg.join("hpm.toml"),
            format!("[package]\npath = \"acme/p{i}\"\nname = \"p{i}\"\nversion = \"1.0.0\"\n"),
        )
        .unwrap();
        crate::packer::pack(
            &pkg,
            &format!("p{i}"),
            "1.0.0",
            &srv,
            None,
            None,
            &hpm_package::StageConfig::default(),
            crate::packer::ArchiveLayout::default(),
        )
        .unwrap();
    }

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let active = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    {
        let (active, peak, srv) = (active.clone(), peak.clone(), srv.clone());
        tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let (active, peak, srv) = (active.clone(), peak.clone(), srv.clone());
                tokio::spawn(async move {
                    let mut buf = [0u8; 2048];
                    let n = socket.read(&mut buf).await.unwrap_or(0);
                    let request = String::from_utf8_lossy(&buf[..n]).into_owned();
                    let file = request
                        .split_whitespace()
                        .nth(1)
                        .unwrap_or("/")
                        .trim_start_matches('/')
                        .to_string();
                    let now = active.fetch_add(1, Ordering::SeqCst) + 1;
                    peak.fetch_max(now, Ordering::SeqCst);
                    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
                    let body = std::fs::read(srv.join(&file)).unwrap_or_default();
                    active.fetch_sub(1, Ordering::SeqCst);
                    let head = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = socket.write_all(head.as_bytes()).await;
                    let _ = socket.write_all(&body).await;
                    let _ = socket.shutdown().await;
                });
            }
        });
    }

    let deps: String = (0..4)
        .map(|i| {
            format!("\"acme/p{i}\" = {{ url = \"{base}/p{i}-1.0.0.zip\", version = \"1.0.0\" }}\n")
        })
        .collect();
    for (limit, home) in [(1usize, "one"), (4, "four")] {
        let root = temp_dir.path().join(format!("project-{home}"));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("hpm.toml"),
            format!("[package]\npath = \"acme/app\"\nname = \"app\"\nversion = \"0.1.0\"\n\n[dependencies]\n{deps}"),
        )
        .unwrap();
        let (base_config, storage) = test_setup(&temp_dir.path().join(home));
        let config = Arc::new(Config {
            install: hpm_config::InstallConfig {
                parallel_downloads: std::num::NonZeroUsize::new(limit).unwrap(),
            },
            ..(*base_config).clone()
        });
        peak.store(0, Ordering::SeqCst);
        let outcomes = ProjectManager::new(root, storage, config)
            .unwrap()
            .sync_dependencies()
            .await
            .unwrap();
        assert_eq!(outcomes.len(), 4);
        let seen = peak.load(Ordering::SeqCst);
        if limit == 1 {
            assert_eq!(seen, 1, "limit 1 let {seen} downloads overlap");
        } else {
            assert!(seen > 1, "limit 4 never overlapped downloads");
        }
    }
}
