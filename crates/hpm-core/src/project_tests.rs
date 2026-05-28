use super::*;
use hpm_package::PackagePath;
use tempfile::TempDir;

/// Build the `(Config, StorageManager)` pair every `ProjectManager` test
/// needs, rooted inside `temp_dir`. The CAS lives at `<temp>/.hpm/packages`.
fn test_setup(temp_dir: &Path) -> (Arc<Config>, Arc<StorageManager>) {
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
fn create_houdini_package_basic() {
    let temp_dir = TempDir::new().unwrap();
    let (config, storage_manager) = test_setup(temp_dir.path());
    let project_root = temp_dir.path().join("project");
    std::fs::create_dir_all(&project_root).unwrap();

    let project_manager = ProjectManager::new(project_root, storage_manager, config).unwrap();

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

    let houdini_package = project_manager
        .create_houdini_package(&installed_package)
        .unwrap();
    assert_eq!(
        houdini_package.hpath,
        Some(vec![package_path.to_string_lossy().to_string()])
    );
    assert!(houdini_package.env.is_some());
}

#[test]
fn create_houdini_package_with_project_env_overrides() {
    let temp_dir = TempDir::new().unwrap();
    let (config, storage_manager) = test_setup(temp_dir.path());
    let project_root = temp_dir.path().join("project");
    std::fs::create_dir_all(&project_root).unwrap();

    let project_manager = ProjectManager::new(project_root, storage_manager, config).unwrap();

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
    let houdini_package = project_manager
        .create_houdini_package(&installed_package)
        .unwrap();
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
        hpm_package::HoudiniEnvValue::Detailed { value, .. } => {
            assert!(value.ends_with("/default-config"));
        }
        _ => panic!("Expected Detailed env value"),
    }

    // With project override: should use override value
    let mut project_overrides = IndexMap::new();
    project_overrides.insert(
        "MY_CONFIG".to_string(),
        ManifestEnvEntry {
            method: hpm_package::EnvMethod::Set,
            value: Some("/custom/config/path".into()),
            required: false,
        },
    );

    let houdini_package = project_manager
        .create_houdini_package_with_python(&installed_package, None, &project_overrides)
        .unwrap();
    let env_entries = houdini_package.env.as_ref().unwrap();
    let my_config_entry = env_entries
        .iter()
        .find(|m| m.contains_key("MY_CONFIG"))
        .unwrap();
    match my_config_entry.get("MY_CONFIG").unwrap() {
        hpm_package::HoudiniEnvValue::Detailed { method, value } => {
            assert_eq!(value, "/custom/config/path");
            assert_eq!(method, "set");
        }
        _ => panic!("Expected Detailed env value"),
    }
}

#[tokio::test]
async fn create_houdini_package_required_env_without_override_errors() {
    let temp_dir = TempDir::new().unwrap();
    let (config, storage_manager) = test_setup(temp_dir.path());
    let project_root = temp_dir.path().join("project");
    std::fs::create_dir_all(&project_root).unwrap();
    let project_manager = ProjectManager::new(project_root, storage_manager, config).unwrap();

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
    let err = project_manager
        .create_houdini_package(&installed_package)
        .unwrap_err();
    match err {
        ProjectError::MissingRequiredEnv { var, package } => {
            assert_eq!(var, "PROJECT_ROOT");
            assert_eq!(package, "needs-config");
        }
        other => panic!("Expected MissingRequiredEnv, got {:?}", other),
    }

    // Project override supplies a value: should succeed and emit it.
    let mut overrides = IndexMap::new();
    overrides.insert(
        "PROJECT_ROOT".to_string(),
        ManifestEnvEntry {
            method: hpm_package::EnvMethod::Set,
            value: Some("/work/project".into()),
            required: false,
        },
    );
    let pkg = project_manager
        .create_houdini_package_with_python(&installed_package, None, &overrides)
        .unwrap();
    let entry = pkg
        .env
        .as_ref()
        .unwrap()
        .iter()
        .find(|m| m.contains_key("PROJECT_ROOT"))
        .unwrap();
    match entry.get("PROJECT_ROOT").unwrap() {
        hpm_package::HoudiniEnvValue::Detailed { value, method } => {
            assert_eq!(value, "/work/project");
            assert_eq!(method, "set");
        }
        _ => panic!("Expected Detailed env value"),
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
/// install_source. Mirrors the canonical HDK-plugin use case.
fn dev_only_runtime_entry(value: &str) -> ManifestEnvEntry {
    use hpm_package::{Condition, EnvValue, EnvValueBranch};
    ManifestEnvEntry {
        method: hpm_package::EnvMethod::Prepend,
        value: Some(EnvValue::Conditional(vec![EnvValueBranch {
            when: Condition {
                install_source: Some("dev".to_string()),
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
    let (config, storage_manager) = test_setup(temp_dir.path());
    let project_root = temp_dir.path().join("project");
    std::fs::create_dir_all(&project_root).unwrap();
    let project_manager = ProjectManager::new(project_root, storage_manager, config).unwrap();

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
    let pkg = project_manager.create_houdini_package(&non_dev).unwrap();
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
    let pkg = project_manager.create_houdini_package(&dev).unwrap();
    let entry = find_env_entry(&pkg, "HOUDINI_DSO_PATH")
        .expect("dev-gated variant must be emitted for dev installs");
    // Conditional values lower to DetailedConditional with one entry
    // keyed by the runtime expression ("true" for an install-source-only
    // gate, since install_source is stripped before compile_condition).
    match entry {
        hpm_package::HoudiniEnvValue::DetailedConditional { method, value } => {
            assert_eq!(method, "prepend");
            assert_eq!(value.len(), 1);
            let v = value[0].values().next().unwrap();
            let expected = format!("{}/build/Release", package_path.display());
            assert_eq!(v, &expected);
        }
        other => panic!("expected DetailedConditional env value, got {other:?}"),
    }
}

#[test]
fn project_override_wins_over_install_source_dev_variant() {
    let temp_dir = TempDir::new().unwrap();
    let (config, storage_manager) = test_setup(temp_dir.path());
    let project_root = temp_dir.path().join("project");
    std::fs::create_dir_all(&project_root).unwrap();
    let project_manager = ProjectManager::new(project_root, storage_manager, config).unwrap();

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
        install_path: package_path,
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
    let pkg = project_manager
        .create_houdini_package_with_python(&installed, None, &overrides)
        .unwrap();
    let entry = find_env_entry(&pkg, "HOUDINI_DSO_PATH")
        .expect("dev-gated key must still emit when project overrides it");
    match entry {
        hpm_package::HoudiniEnvValue::Detailed { value, .. } => {
            assert_eq!(value, "/opt/forced/dso");
        }
        other => panic!("expected Detailed env value, got {other:?}"),
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
    let spec = hpm_package::DependencySpec::Simple("1.1.20".to_string());
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
    // Short-circuited Simple/Registry: no fresh fetch -> no checksum / source.
    assert!(outcome.checksum.is_none());
    assert!(outcome.source.is_none());
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

    // Simulate the prior sync's output: foo.json (current dep) and
    // stale.json (dep that left the set).
    let pkg_dir = &project_manager.project_paths.packages_dir;
    let foo_json = pkg_dir.join("foo.json");
    let stale_json = pkg_dir.join("stale.json");
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

/// An empty dependency set must still sweep prior `<slug>.json` files.
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
        let (config, storage_manager) = test_setup(temp_dir.path());
        let project_root = temp_dir.path().join("project");
        std::fs::create_dir_all(&project_root).unwrap();
        let pm = ProjectManager::new(project_root, storage_manager, config).unwrap();

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

        let a = pm.create_houdini_package(&InstalledPackage {
            version: "1.0.0".to_string(),
            manifest: with_dev,
            install_path: pkg_path.clone(),
            is_dev: false,
        });
        let b = pm.create_houdini_package(&InstalledPackage {
            version: "1.0.0".to_string(),
            manifest: without,
            install_path: pkg_path,
            is_dev: false,
        });

        // Compare via Debug — HoudiniPackage / HoudiniEnvValue don't
        // implement PartialEq, and a regression that fires the dev
        // branch for is_dev=false would diverge here even when both
        // sides succeed.
        prop_assert_eq!(format!("{:?}", a), format!("{:?}", b));
    }
}
