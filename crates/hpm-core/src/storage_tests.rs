use super::*;
use tempfile::TempDir;

#[test]
fn storage_manager_creation() {
    let temp_dir = TempDir::new().unwrap();
    let storage_config = StorageConfig {
        home_dir: temp_dir.path().to_path_buf(),
        cache_dir: temp_dir.path().join("cache"),
        packages_dir: temp_dir.path().join("packages"),
        registry_cache_dir: temp_dir.path().join("registry"),
    };

    let _storage_manager = StorageManager::new(storage_config).unwrap();
    assert!(temp_dir.path().join("packages").exists());
    assert!(temp_dir.path().join("cache").exists());
    assert!(temp_dir.path().join("registry").exists());
}

#[test]
fn package_exists_check() {
    let temp_dir = TempDir::new().unwrap();
    let storage_config = StorageConfig {
        home_dir: temp_dir.path().to_path_buf(),
        cache_dir: temp_dir.path().join("cache"),
        packages_dir: temp_dir.path().join("packages"),
        registry_cache_dir: temp_dir.path().join("registry"),
    };

    let storage_manager = StorageManager::new(storage_config).unwrap();

    assert!(!storage_manager.package_exists("test-package", "1.0.0"));

    // Create a fake package directory
    let package_dir = temp_dir.path().join("packages").join("test-package@1.0.0");
    std::fs::create_dir_all(&package_dir).unwrap();
    std::fs::write(
        package_dir.join("hpm.toml"),
        "[package]\npath = \"studio/test-package\"\nname = \"Test Package\"\nversion = \"1.0.0\"",
    )
    .unwrap();

    assert!(storage_manager.package_exists("test-package", "1.0.0"));
}

#[test]
fn list_installed_packages() {
    let temp_dir = TempDir::new().unwrap();
    let storage_config = StorageConfig {
        home_dir: temp_dir.path().to_path_buf(),
        cache_dir: temp_dir.path().join("cache"),
        packages_dir: temp_dir.path().join("packages"),
        registry_cache_dir: temp_dir.path().join("registry"),
    };

    let storage_manager = StorageManager::new(storage_config).unwrap();

    // Initially no packages
    let packages = storage_manager.list_installed().unwrap();
    assert_eq!(packages.len(), 0);

    // Create a fake package
    let package_dir = temp_dir.path().join("packages").join("test-package@1.0.0");
    std::fs::create_dir_all(&package_dir).unwrap();

    let manifest_content = r#"
[package]
path = "studio/test-package"
name = "Test Package"
version = "1.0.0"
description = "A test package"

[compat]
houdini = ">=20.5"
"#;
    std::fs::write(package_dir.join("hpm.toml"), manifest_content).unwrap();

    let packages = storage_manager.list_installed().unwrap();
    assert_eq!(packages.len(), 1);
    assert_eq!(packages[0].slug(), "test-package");
    assert_eq!(packages[0].version, "1.0.0");
}

// Error path tests

#[tokio::test]
async fn remove_nonexistent_package_fails() {
    let temp_dir = TempDir::new().unwrap();
    let storage_config = StorageConfig {
        home_dir: temp_dir.path().to_path_buf(),
        packages_dir: temp_dir.path().join("packages"),
        cache_dir: temp_dir.path().join("cache"),
        registry_cache_dir: temp_dir.path().join("registry"),
    };
    let storage_manager = StorageManager::new(storage_config).unwrap();

    let result = storage_manager.remove_package("nonexistent", "1.0.0").await;
    assert!(result.is_err());
    match result {
        Err(StorageError::PackageNotFound(msg)) => {
            assert!(msg.contains("nonexistent"));
        }
        _ => panic!("Expected PackageNotFound error"),
    }
}

/// Defensive: if a junction/symlink ever lands at the registry CAS path
/// (manually, or via future code), `remove_package` must remove the link
/// entry itself rather than follow it. On Unix this is mostly a
/// belt-and-braces check (remove_dir_all on a symlink errors anyway); on
/// Windows this is the load-bearing safety property.
#[tokio::test]
async fn remove_package_unlinks_symlink_entries() {
    let temp_dir = TempDir::new().unwrap();
    let storage_config = StorageConfig {
        home_dir: temp_dir.path().to_path_buf(),
        packages_dir: temp_dir.path().join("packages"),
        cache_dir: temp_dir.path().join("cache"),
        registry_cache_dir: temp_dir.path().join("registry"),
    };
    let storage_manager = StorageManager::new(storage_config).unwrap();

    // Stand up an external "workspace" with a manifest.
    let external = temp_dir.path().join("external-workspace");
    write_source_package(&external, "creator/foo", "1.0.0", "external-marker");

    // Manually plant a symlink/junction at the registry CAS path that
    // `remove_package` resolves to. `package_dir(name, version)` uses the
    // bare-slug layout, so the link lives at `<packages_dir>/<slug>@<v>/`.
    let cas_path = storage_manager.config.package_dir("foo", "1.0.0");
    std::fs::create_dir_all(cas_path.parent().unwrap()).unwrap();
    create_dev_link(&external, &cas_path).unwrap();
    assert!(cas_path.join("MARKER").exists());

    storage_manager
        .remove_package("foo", "1.0.0")
        .await
        .unwrap();

    // CAS path is gone, external workspace survives intact.
    assert!(std::fs::symlink_metadata(&cas_path).is_err());
    assert_eq!(
        std::fs::read_to_string(external.join("MARKER")).unwrap(),
        "external-marker"
    );
}

#[test]
fn list_packages_with_corrupted_manifest() {
    let temp_dir = TempDir::new().unwrap();
    let storage_config = StorageConfig {
        home_dir: temp_dir.path().to_path_buf(),
        packages_dir: temp_dir.path().join("packages"),
        cache_dir: temp_dir.path().join("cache"),
        registry_cache_dir: temp_dir.path().join("registry"),
    };
    let storage_manager = StorageManager::new(storage_config).unwrap();

    // Create a package directory with a corrupted manifest
    let package_dir = temp_dir.path().join("packages").join("corrupted-pkg@1.0.0");
    std::fs::create_dir_all(&package_dir).unwrap();
    std::fs::write(
        package_dir.join("hpm.toml"),
        "this is not valid toml { [ broken",
    )
    .unwrap();

    let result = storage_manager.list_installed();
    assert!(result.is_err());
    match result {
        Err(StorageError::Manifest(ManifestLoadError::Parse { path, .. })) => {
            assert!(path.ends_with("hpm.toml"));
        }
        other => panic!("Expected Manifest::Parse error, got: {:?}", other),
    }
}

#[test]
fn list_packages_skips_non_directories() {
    let temp_dir = TempDir::new().unwrap();
    let storage_config = StorageConfig {
        home_dir: temp_dir.path().to_path_buf(),
        packages_dir: temp_dir.path().join("packages"),
        cache_dir: temp_dir.path().join("cache"),
        registry_cache_dir: temp_dir.path().join("registry"),
    };
    let storage_manager = StorageManager::new(storage_config).unwrap();

    // Create the packages directory and add a file (not a directory)
    std::fs::create_dir_all(temp_dir.path().join("packages")).unwrap();
    std::fs::write(
        temp_dir.path().join("packages").join("random-file.txt"),
        "not a package",
    )
    .unwrap();

    // Should not error, just skip the file
    let packages = storage_manager.list_installed().unwrap();
    assert!(packages.is_empty());
}

#[test]
fn list_packages_skips_directories_without_manifest() {
    let temp_dir = TempDir::new().unwrap();
    let storage_config = StorageConfig {
        home_dir: temp_dir.path().to_path_buf(),
        packages_dir: temp_dir.path().join("packages"),
        cache_dir: temp_dir.path().join("cache"),
        registry_cache_dir: temp_dir.path().join("registry"),
    };
    let storage_manager = StorageManager::new(storage_config).unwrap();

    // Create a package directory without hpm.toml
    let package_dir = temp_dir.path().join("packages").join("empty-pkg@1.0.0");
    std::fs::create_dir_all(&package_dir).unwrap();
    std::fs::write(package_dir.join("README.md"), "no manifest here").unwrap();

    // Should not error, just skip directories without manifest
    let packages = storage_manager.list_installed().unwrap();
    assert!(packages.is_empty());
}

#[test]
fn list_installed_scoped_packages() {
    let temp_dir = TempDir::new().unwrap();
    let storage_config = StorageConfig {
        home_dir: temp_dir.path().to_path_buf(),
        cache_dir: temp_dir.path().join("cache"),
        packages_dir: temp_dir.path().join("packages"),
        registry_cache_dir: temp_dir.path().join("registry"),
    };

    let storage_manager = StorageManager::new(storage_config).unwrap();

    // Create a scoped package at packages/tumblehead/fire-fx@1.0.0/
    let package_dir = temp_dir
        .path()
        .join("packages")
        .join("tumblehead")
        .join("fire-fx@1.0.0");
    std::fs::create_dir_all(&package_dir).unwrap();

    let manifest_content = r#"
[package]
path = "tumblehead/fire-fx"
name = "Fire FX"
version = "1.0.0"
description = "A fire effects package"

[compat]
houdini = ">=20.5"
"#;
    std::fs::write(package_dir.join("hpm.toml"), manifest_content).unwrap();

    // Also create a non-scoped package at packages/old-pkg@2.0.0/
    let old_pkg_dir = temp_dir.path().join("packages").join("old-pkg@2.0.0");
    std::fs::create_dir_all(&old_pkg_dir).unwrap();
    std::fs::write(
        old_pkg_dir.join("hpm.toml"),
        "[package]\npath = \"studio/old-pkg\"\nname = \"Old Package\"\nversion = \"2.0.0\"",
    )
    .unwrap();

    let packages = storage_manager.list_installed().unwrap();
    assert_eq!(packages.len(), 2);

    // Find the scoped package
    let fire_fx = packages
        .iter()
        .find(|p| p.manifest.package.slug() == "fire-fx")
        .unwrap();
    assert_eq!(fire_fx.version, "1.0.0");

    // Find the non-scoped package
    let old_pkg = packages
        .iter()
        .find(|p| p.manifest.package.slug() == "old-pkg")
        .unwrap();
    assert_eq!(old_pkg.version, "2.0.0");
}

#[tokio::test]
async fn install_into_cas_without_manifest_fails() {
    let temp_dir = TempDir::new().unwrap();
    let storage_config = StorageConfig {
        home_dir: temp_dir.path().to_path_buf(),
        packages_dir: temp_dir.path().join("packages"),
        cache_dir: temp_dir.path().join("cache"),
        registry_cache_dir: temp_dir.path().join("registry"),
    };
    let storage_manager = StorageManager::new(storage_config).unwrap();

    // Create a source directory without hpm.toml
    let source_dir = temp_dir.path().join("source-pkg");
    std::fs::create_dir_all(&source_dir).unwrap();

    let result = storage_manager.install_into_cas(&source_dir).await;
    assert!(result.is_err());
    match result {
        Err(StorageError::Manifest(ManifestLoadError::NotFound { path })) => {
            assert!(path.ends_with("hpm.toml"));
        }
        other => panic!("Expected Manifest::NotFound error, got: {:?}", other),
    }
}

/// Build a minimal source package directory at `dir` with the given
/// scoped path, version, and a marker file recording who created it.
/// The marker lets a test distinguish dev content from registry content
/// after it lands in the CAS.
fn write_source_package(dir: &std::path::Path, package_path: &str, version: &str, marker: &str) {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(
            dir.join("hpm.toml"),
            format!(
                "[package]\npath = \"{package_path}\"\nname = \"{package_path}\"\nversion = \"{version}\"\n",
            ),
        )
        .unwrap();
    std::fs::write(dir.join("MARKER"), marker).unwrap();
}

/// Regression: a dev install must land in the `_dev` subtree, not in the
/// registry CAS. Otherwise a registry resolution at the same `(slug,
/// version)` would pick up the dev content via the CAS short-circuit.
#[tokio::test]
async fn install_as_dev_copy_targets_dev_subtree() {
    let temp_dir = TempDir::new().unwrap();
    let storage_config = StorageConfig {
        home_dir: temp_dir.path().to_path_buf(),
        packages_dir: temp_dir.path().join("packages"),
        cache_dir: temp_dir.path().join("cache"),
        registry_cache_dir: temp_dir.path().join("registry"),
    };
    let storage_manager = StorageManager::new(storage_config).unwrap();

    let source = temp_dir.path().join("dev-source");
    write_source_package(&source, "creator/foo", "1.0.0", "from-dev-source");

    let installed = storage_manager.install_as_dev_copy(&source).await.unwrap();

    let expected = temp_dir
        .path()
        .join("packages")
        .join("_dev")
        .join("foo@1.0.0");
    assert_eq!(installed.install_path, expected);
    assert!(expected.join("MARKER").exists());

    // The registry CAS path must remain empty.
    let registry_cas = temp_dir.path().join("packages").join("foo@1.0.0");
    assert!(
        !registry_cas.exists(),
        "dev install must not touch the registry CAS path"
    );
}

/// Regression: `list_installed` is the cache the project's
/// `ensure_installed`/`ensure_installed_from_url` short-circuits consult.
/// If a dev install showed up there, a different project resolving the
/// same coordinate from a registry would skip the fetch and silently
/// hand out dev content. Skipping the `_dev` subtree closes that path.
#[tokio::test]
async fn list_installed_skips_dev_subtree() {
    let temp_dir = TempDir::new().unwrap();
    let storage_config = StorageConfig {
        home_dir: temp_dir.path().to_path_buf(),
        packages_dir: temp_dir.path().join("packages"),
        cache_dir: temp_dir.path().join("cache"),
        registry_cache_dir: temp_dir.path().join("registry"),
    };
    let storage_manager = StorageManager::new(storage_config).unwrap();

    // Dev install of foo@1.0.0
    let dev_source = temp_dir.path().join("dev-source");
    write_source_package(&dev_source, "creator/foo", "1.0.0", "dev");
    storage_manager
        .install_as_dev_copy(&dev_source)
        .await
        .unwrap();

    // Independent registry-style install of bar@2.0.0
    let reg_source = temp_dir.path().join("reg-source");
    write_source_package(&reg_source, "creator/bar", "2.0.0", "registry");
    storage_manager.install_into_cas(&reg_source).await.unwrap();

    let listed = storage_manager.list_installed().unwrap();
    let names: Vec<&str> = listed.iter().map(|p| p.manifest.package.slug()).collect();
    assert!(
        !names.contains(&"foo"),
        "list_installed must hide dev installs; got {:?}",
        names
    );
    assert!(
        names.contains(&"bar"),
        "list_installed must surface registry installs; got {:?}",
        names
    );
}

/// Regression: a dev install at `foo@1.0.0` must coexist with a registry
/// install at `foo@1.0.0`. Each lives in its own subtree, so neither
/// install's content overwrites the other when both are present.
#[tokio::test]
async fn dev_and_registry_installs_coexist_at_same_coordinate() {
    let temp_dir = TempDir::new().unwrap();
    let storage_config = StorageConfig {
        home_dir: temp_dir.path().to_path_buf(),
        packages_dir: temp_dir.path().join("packages"),
        cache_dir: temp_dir.path().join("cache"),
        registry_cache_dir: temp_dir.path().join("registry"),
    };
    let storage_manager = StorageManager::new(storage_config).unwrap();

    let dev_source = temp_dir.path().join("dev-source");
    write_source_package(&dev_source, "creator/foo", "1.0.0", "dev-content");
    storage_manager
        .install_as_dev_copy(&dev_source)
        .await
        .unwrap();

    let reg_source = temp_dir.path().join("reg-source");
    write_source_package(&reg_source, "creator/foo", "1.0.0", "registry-content");
    storage_manager.install_into_cas(&reg_source).await.unwrap();

    let dev_marker = temp_dir
        .path()
        .join("packages")
        .join("_dev")
        .join("foo@1.0.0")
        .join("MARKER");
    let reg_marker = temp_dir
        .path()
        .join("packages")
        .join("foo@1.0.0")
        .join("MARKER");
    assert_eq!(std::fs::read_to_string(&dev_marker).unwrap(), "dev-content");
    assert_eq!(
        std::fs::read_to_string(&reg_marker).unwrap(),
        "registry-content"
    );
}

/// Link-mode dev install creates a symlink/junction at
/// `_dev/<slug>@<version>/`. Reading through the link must reach the
/// workspace (this is the whole point of the feature).
#[tokio::test]
async fn install_as_dev_link_creates_link_to_workspace() {
    let temp_dir = TempDir::new().unwrap();
    let storage_config = StorageConfig {
        home_dir: temp_dir.path().to_path_buf(),
        packages_dir: temp_dir.path().join("packages"),
        cache_dir: temp_dir.path().join("cache"),
        registry_cache_dir: temp_dir.path().join("registry"),
    };
    let storage_manager = StorageManager::new(storage_config).unwrap();

    let source = temp_dir.path().join("link-source");
    write_source_package(&source, "creator/foo", "1.0.0", "link-content");

    let installed = storage_manager.install_as_dev_link(&source).await.unwrap();

    let expected = temp_dir
        .path()
        .join("packages")
        .join("_dev")
        .join("foo@1.0.0");
    assert_eq!(installed.install_path, expected);

    // The install entry is a symlink/junction, not a real directory.
    let meta = std::fs::symlink_metadata(&expected).unwrap();
    let is_link = meta.file_type().is_symlink() || {
        #[cfg(windows)]
        {
            junction::exists(&expected).unwrap_or(false)
        }
        #[cfg(not(windows))]
        {
            false
        }
    };
    assert!(is_link, "dev link install must be a symlink/junction");

    // Reading through the link reaches the workspace.
    assert_eq!(
        std::fs::read_to_string(expected.join("MARKER")).unwrap(),
        "link-content"
    );
}

/// Live-edit guarantee: a file written into the workspace *after* the
/// link install becomes visible through the install_path. This is the
/// whole reason the feature exists.
#[tokio::test]
async fn dev_link_install_reflects_live_workspace_edits() {
    let temp_dir = TempDir::new().unwrap();
    let storage_config = StorageConfig {
        home_dir: temp_dir.path().to_path_buf(),
        packages_dir: temp_dir.path().join("packages"),
        cache_dir: temp_dir.path().join("cache"),
        registry_cache_dir: temp_dir.path().join("registry"),
    };
    let storage_manager = StorageManager::new(storage_config).unwrap();

    let source = temp_dir.path().join("live-source");
    write_source_package(&source, "creator/foo", "1.0.0", "initial");
    let installed = storage_manager.install_as_dev_link(&source).await.unwrap();

    // Simulate a working-tree edit after install.
    std::fs::write(source.join("new_file.txt"), "edited-after-install").unwrap();

    assert_eq!(
        std::fs::read_to_string(installed.install_path.join("new_file.txt")).unwrap(),
        "edited-after-install"
    );
}

/// Repeated link-installs must replace the link entry without nuking the
/// workspace. This is the safety property the symlink-aware removal
/// branch in `clear_existing_install` enforces — without it,
/// `remove_dir_all` on a Windows junction would recursively delete the
/// user's source tree.
#[tokio::test]
async fn repeated_dev_link_install_preserves_workspace() {
    let temp_dir = TempDir::new().unwrap();
    let storage_config = StorageConfig {
        home_dir: temp_dir.path().to_path_buf(),
        packages_dir: temp_dir.path().join("packages"),
        cache_dir: temp_dir.path().join("cache"),
        registry_cache_dir: temp_dir.path().join("registry"),
    };
    let storage_manager = StorageManager::new(storage_config).unwrap();

    let source = temp_dir.path().join("workspace");
    write_source_package(&source, "creator/foo", "1.0.0", "workspace-marker");
    std::fs::write(source.join("user-script.py"), "# user authored").unwrap();

    storage_manager.install_as_dev_link(&source).await.unwrap();
    storage_manager.install_as_dev_link(&source).await.unwrap();

    // Workspace files must survive — both the marker and the user file.
    assert_eq!(
        std::fs::read_to_string(source.join("MARKER")).unwrap(),
        "workspace-marker"
    );
    assert_eq!(
        std::fs::read_to_string(source.join("user-script.py")).unwrap(),
        "# user authored"
    );
    assert!(source.join("hpm.toml").exists());
}

/// Switching install styles at the same coordinate is allowed and must
/// not delete the workspace: a copy install replaced by a link install
/// (or vice versa) goes through `clear_existing_install`, which uses
/// `remove_dir_all` for real dirs and link-safe removal for links.
#[tokio::test]
async fn switching_copy_to_link_does_not_touch_workspace() {
    let temp_dir = TempDir::new().unwrap();
    let storage_config = StorageConfig {
        home_dir: temp_dir.path().to_path_buf(),
        packages_dir: temp_dir.path().join("packages"),
        cache_dir: temp_dir.path().join("cache"),
        registry_cache_dir: temp_dir.path().join("registry"),
    };
    let storage_manager = StorageManager::new(storage_config).unwrap();

    let source = temp_dir.path().join("workspace");
    write_source_package(&source, "creator/foo", "1.0.0", "ws");

    // First: copy install lays down a real directory at the dev path.
    storage_manager.install_as_dev_copy(&source).await.unwrap();

    // Then: link install must replace the real dir without traversing
    // into the workspace.
    storage_manager.install_as_dev_link(&source).await.unwrap();

    assert_eq!(
        std::fs::read_to_string(source.join("MARKER")).unwrap(),
        "ws"
    );

    // And the reverse: link → copy.
    storage_manager.install_as_dev_copy(&source).await.unwrap();
    assert_eq!(
        std::fs::read_to_string(source.join("MARKER")).unwrap(),
        "ws"
    );
}

/// Link install must respect the same `_dev/` namespace isolation as
/// copy-install. A registry install at the same coordinate is unaffected.
#[tokio::test]
async fn dev_link_and_registry_installs_coexist_at_same_coordinate() {
    let temp_dir = TempDir::new().unwrap();
    let storage_config = StorageConfig {
        home_dir: temp_dir.path().to_path_buf(),
        packages_dir: temp_dir.path().join("packages"),
        cache_dir: temp_dir.path().join("cache"),
        registry_cache_dir: temp_dir.path().join("registry"),
    };
    let storage_manager = StorageManager::new(storage_config).unwrap();

    let link_source = temp_dir.path().join("link-source");
    write_source_package(&link_source, "creator/foo", "1.0.0", "link-content");
    storage_manager
        .install_as_dev_link(&link_source)
        .await
        .unwrap();

    let reg_source = temp_dir.path().join("reg-source");
    write_source_package(&reg_source, "creator/foo", "1.0.0", "registry-content");
    storage_manager.install_into_cas(&reg_source).await.unwrap();

    let link_marker = temp_dir
        .path()
        .join("packages")
        .join("_dev")
        .join("foo@1.0.0")
        .join("MARKER");
    let reg_marker = temp_dir
        .path()
        .join("packages")
        .join("foo@1.0.0")
        .join("MARKER");
    assert_eq!(
        std::fs::read_to_string(&link_marker).unwrap(),
        "link-content"
    );
    assert_eq!(
        std::fs::read_to_string(&reg_marker).unwrap(),
        "registry-content"
    );

    // And `list_installed` still ignores the dev subtree, even when the
    // entry is a link rather than a real directory.
    let listed = storage_manager.list_installed().unwrap();
    let names: Vec<&str> = listed.iter().map(|p| p.manifest.package.slug()).collect();
    assert_eq!(
        names,
        vec!["foo"],
        "only the registry install should surface"
    );
}

// ------------------------------------------------------------------
// Dev (path-dep) install cleanup
// ------------------------------------------------------------------

/// Build a project hpm.toml at `dir` that depends on a single path dep
/// pointing at `dep_path` (relative or absolute string written verbatim).
fn write_project_with_path_dep(
    dir: &std::path::Path,
    project_slug: &str,
    version: &str,
    dep_name: &str,
    dep_path: &str,
    link: bool,
) {
    std::fs::create_dir_all(dir).unwrap();
    let link_field = if link { ", link = true" } else { "" };
    std::fs::write(
        dir.join("hpm.toml"),
        format!(
            "[package]\n\
                 path = \"studio/{project_slug}\"\n\
                 name = \"{project_slug}\"\n\
                 version = \"{version}\"\n\
                 \n\
                 [dependencies]\n\
                 \"{dep_name}\" = {{ path = \"{dep_path}\"{link_field} }}\n",
        ),
    )
    .unwrap();
}

fn projects_config_with(paths: Vec<std::path::PathBuf>) -> ProjectsConfig {
    ProjectsConfig {
        explicit_paths: paths,
        search_roots: vec![],
        max_search_depth: 0,
        ignore_patterns: vec![],
    }
}

/// A dev install that no project's path-dep claims must be classified
/// as orphan and removed.
#[tokio::test]
async fn unreferenced_dev_install_is_orphan() {
    let temp_dir = TempDir::new().unwrap();
    let storage_config = StorageConfig {
        home_dir: temp_dir.path().to_path_buf(),
        packages_dir: temp_dir.path().join("packages"),
        cache_dir: temp_dir.path().join("cache"),
        registry_cache_dir: temp_dir.path().join("registry"),
    };
    let storage_manager = StorageManager::new(storage_config).unwrap();

    // Plant a dev install for `studio/orphan@1.0.0`.
    let source = temp_dir.path().join("orphan-source");
    write_source_package(&source, "studio/orphan", "1.0.0", "orphan-marker");
    storage_manager.install_as_dev_copy(&source).await.unwrap();
    let dev_path = temp_dir
        .path()
        .join("packages")
        .join("_dev")
        .join("orphan@1.0.0");
    assert!(dev_path.exists());

    // A project exists but doesn't reference this dep.
    let project_dir = temp_dir.path().join("project");
    write_project_with_path_dep(
        &project_dir,
        "consumer",
        "1.0.0",
        "studio/something-else",
        "../something-else-src",
        false,
    );
    // The "something-else" source doesn't exist — the dep is broken, but
    // that's irrelevant to this test (the dep would also fail to claim
    // the `orphan` dev install regardless).

    let projects_cfg = projects_config_with(vec![project_dir]);
    let removed = storage_manager
        .cleanup_unused_dev_installs(&projects_cfg)
        .await
        .unwrap();

    assert_eq!(removed, vec!["_dev/orphan@1.0.0"]);
    assert!(!dev_path.exists());
    // Source workspace is untouched.
    assert!(source.join("MARKER").exists());
}

/// A dev install that any project's path-dep manifest resolves to must
/// be protected from cleanup.
#[tokio::test]
async fn referenced_dev_install_survives_cleanup() {
    let temp_dir = TempDir::new().unwrap();
    let storage_config = StorageConfig {
        home_dir: temp_dir.path().to_path_buf(),
        packages_dir: temp_dir.path().join("packages"),
        cache_dir: temp_dir.path().join("cache"),
        registry_cache_dir: temp_dir.path().join("registry"),
    };
    let storage_manager = StorageManager::new(storage_config).unwrap();

    // Plant a dev install for `studio/keep@2.0.0`.
    let source = temp_dir.path().join("keep-source");
    write_source_package(&source, "studio/keep", "2.0.0", "keep-marker");
    storage_manager.install_as_dev_copy(&source).await.unwrap();
    let dev_path = temp_dir
        .path()
        .join("packages")
        .join("_dev")
        .join("keep@2.0.0");
    assert!(dev_path.exists());

    // Project references the same source workspace via path dep.
    let project_dir = temp_dir.path().join("project");
    write_project_with_path_dep(
        &project_dir,
        "consumer",
        "1.0.0",
        "studio/keep",
        "../keep-source",
        false,
    );

    let projects_cfg = projects_config_with(vec![project_dir]);
    let removed = storage_manager
        .cleanup_unused_dev_installs(&projects_cfg)
        .await
        .unwrap();

    assert!(removed.is_empty(), "no orphans expected, got {removed:?}");
    assert!(dev_path.exists(), "referenced dev install must survive");
}

/// Cleaning up a link-mode dev install must remove the link entry only,
/// never follow it into the workspace.
#[tokio::test]
async fn orphan_link_install_cleanup_preserves_source() {
    let temp_dir = TempDir::new().unwrap();
    let storage_config = StorageConfig {
        home_dir: temp_dir.path().to_path_buf(),
        packages_dir: temp_dir.path().join("packages"),
        cache_dir: temp_dir.path().join("cache"),
        registry_cache_dir: temp_dir.path().join("registry"),
    };
    let storage_manager = StorageManager::new(storage_config).unwrap();

    // Link install with no referencing project.
    let workspace = temp_dir.path().join("workspace");
    write_source_package(&workspace, "studio/linked", "1.0.0", "workspace-marker");
    std::fs::write(workspace.join("user-file.py"), "# user authored").unwrap();
    storage_manager
        .install_as_dev_link(&workspace)
        .await
        .unwrap();
    let dev_path = temp_dir
        .path()
        .join("packages")
        .join("_dev")
        .join("linked@1.0.0");

    // A different project exists, doesn't claim `linked`.
    let project_dir = temp_dir.path().join("project");
    write_project_with_path_dep(
        &project_dir,
        "consumer",
        "1.0.0",
        "studio/other",
        "../other",
        false,
    );

    let projects_cfg = projects_config_with(vec![project_dir]);
    let removed = storage_manager
        .cleanup_unused_dev_installs(&projects_cfg)
        .await
        .unwrap();

    assert_eq!(removed, vec!["_dev/linked@1.0.0"]);
    assert!(std::fs::symlink_metadata(&dev_path).is_err());
    // Workspace files survive — the link unlinked, the workspace did not.
    assert_eq!(
        std::fs::read_to_string(workspace.join("MARKER")).unwrap(),
        "workspace-marker"
    );
    assert_eq!(
        std::fs::read_to_string(workspace.join("user-file.py")).unwrap(),
        "# user authored"
    );
}

/// A project with an unresolvable path-dep source (e.g. workspace moved
/// or deleted) must not bypass cleanup of other dev installs. We log a
/// warning for the broken dep and continue.
#[tokio::test]
async fn unresolvable_path_dep_does_not_block_cleanup() {
    let temp_dir = TempDir::new().unwrap();
    let storage_config = StorageConfig {
        home_dir: temp_dir.path().to_path_buf(),
        packages_dir: temp_dir.path().join("packages"),
        cache_dir: temp_dir.path().join("cache"),
        registry_cache_dir: temp_dir.path().join("registry"),
    };
    let storage_manager = StorageManager::new(storage_config).unwrap();

    // Plant two dev installs.
    let alpha_src = temp_dir.path().join("alpha-src");
    write_source_package(&alpha_src, "studio/alpha", "1.0.0", "alpha");
    storage_manager
        .install_as_dev_copy(&alpha_src)
        .await
        .unwrap();
    let beta_src = temp_dir.path().join("beta-src");
    write_source_package(&beta_src, "studio/beta", "1.0.0", "beta");
    storage_manager
        .install_as_dev_copy(&beta_src)
        .await
        .unwrap();

    // Project references `alpha` correctly, but `beta`'s path points at
    // a directory that doesn't have an hpm.toml — `from_path` errors.
    let project_dir = temp_dir.path().join("project");
    std::fs::create_dir_all(&project_dir).unwrap();
    std::fs::write(
        project_dir.join("hpm.toml"),
        "[package]\n\
             path = \"studio/consumer\"\n\
             name = \"consumer\"\n\
             version = \"1.0.0\"\n\
             \n\
             [dependencies]\n\
             \"studio/alpha\" = { path = \"../alpha-src\" }\n\
             \"studio/beta\" = { path = \"../does-not-exist\" }\n",
    )
    .unwrap();

    let projects_cfg = projects_config_with(vec![project_dir]);
    let removed = storage_manager
        .cleanup_unused_dev_installs(&projects_cfg)
        .await
        .unwrap();

    // `alpha` is referenced → survives.
    // `beta`'s referencing dep is unresolvable → `beta` looks orphaned
    // (correct: a broken dep cannot protect anything from cleanup).
    assert_eq!(removed, vec!["_dev/beta@1.0.0"]);
    let alpha_path = temp_dir
        .path()
        .join("packages")
        .join("_dev")
        .join("alpha@1.0.0");
    let beta_path = temp_dir
        .path()
        .join("packages")
        .join("_dev")
        .join("beta@1.0.0");
    assert!(alpha_path.exists());
    assert!(!beta_path.exists());
}

/// Safety guard: an empty projects list means we can't tell whether any
/// dev install is needed, so we must not remove anything. Matches the
/// existing CAS-cleanup behavior.
#[tokio::test]
async fn no_projects_skips_dev_cleanup() {
    let temp_dir = TempDir::new().unwrap();
    let storage_config = StorageConfig {
        home_dir: temp_dir.path().to_path_buf(),
        packages_dir: temp_dir.path().join("packages"),
        cache_dir: temp_dir.path().join("cache"),
        registry_cache_dir: temp_dir.path().join("registry"),
    };
    let storage_manager = StorageManager::new(storage_config).unwrap();

    let source = temp_dir.path().join("orphan-src");
    write_source_package(&source, "studio/orphan", "1.0.0", "orphan");
    storage_manager.install_as_dev_copy(&source).await.unwrap();
    let dev_path = temp_dir
        .path()
        .join("packages")
        .join("_dev")
        .join("orphan@1.0.0");

    let projects_cfg = projects_config_with(vec![]);
    let removed = storage_manager
        .cleanup_unused_dev_installs(&projects_cfg)
        .await
        .unwrap();

    assert!(removed.is_empty());
    assert!(dev_path.exists(), "no projects → no cleanup");
}

/// `cleanup_comprehensive` carries dev orphans through to the result.
#[tokio::test]
async fn cleanup_comprehensive_reports_dev_orphans() {
    let temp_dir = TempDir::new().unwrap();
    let storage_config = StorageConfig {
        home_dir: temp_dir.path().to_path_buf(),
        packages_dir: temp_dir.path().join("packages"),
        cache_dir: temp_dir.path().join("cache"),
        registry_cache_dir: temp_dir.path().join("registry"),
    };
    let storage_manager = StorageManager::new(storage_config).unwrap();

    let orphan_src = temp_dir.path().join("orphan-src");
    write_source_package(&orphan_src, "studio/orphan", "1.0.0", "orphan");
    storage_manager
        .install_as_dev_copy(&orphan_src)
        .await
        .unwrap();

    // Non-empty project list, none claiming `orphan`.
    let project_dir = temp_dir.path().join("project");
    write_project_with_path_dep(
        &project_dir,
        "consumer",
        "1.0.0",
        "studio/anything",
        "../anything",
        false,
    );
    let projects_cfg = projects_config_with(vec![project_dir]);

    // Dry-run first — nothing removed but the report is populated.
    let dry = storage_manager
        .cleanup_comprehensive(&projects_cfg, true)
        .await
        .unwrap();
    assert_eq!(dry.removed_dev_installs, vec!["_dev/orphan@1.0.0"]);
    let dev_path = temp_dir
        .path()
        .join("packages")
        .join("_dev")
        .join("orphan@1.0.0");
    assert!(dev_path.exists(), "dry-run must not delete");

    // Real run — the dev install is gone.
    let real = storage_manager
        .cleanup_comprehensive(&projects_cfg, false)
        .await
        .unwrap();
    assert_eq!(real.removed_dev_installs, vec!["_dev/orphan@1.0.0"]);
    assert!(!dev_path.exists());
}
