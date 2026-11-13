use super::*;
use hpm_config::ProjectsConfig;
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

/// The store-wide sweep an embedder runs at startup: every installed package
/// is repaired without anyone having to touch it first, and a second run costs
/// nothing because each tree is stamped.
#[cfg(unix)]
#[test]
fn repair_exec_modes_sweeps_every_installed_package() {
    use std::os::unix::fs::PermissionsExt;

    let temp_dir = TempDir::new().unwrap();
    let storage_config = StorageConfig {
        home_dir: temp_dir.path().to_path_buf(),
        cache_dir: temp_dir.path().join("cache"),
        packages_dir: temp_dir.path().join("packages"),
        registry_cache_dir: temp_dir.path().join("registry"),
    };
    let storage_manager = StorageManager::new(storage_config).unwrap();

    // Two packages installed the way an older hpm left them, plus a data file
    // that must stay untouched.
    let mut tools = Vec::new();
    for (slug, version) in [("tumblepipe", "1.39.2"), ("otherpkg", "0.2.0")] {
        let dir = temp_dir
            .path()
            .join("packages")
            .join(format!("{slug}@{version}"));
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        std::fs::write(
            dir.join("hpm.toml"),
            format!(
                "[package]\npath = \"studio/{slug}\"\nname = \"{slug}\"\nversion = \"{version}\"\n"
            ),
        )
        .unwrap();
        let tool = dir.join("bin/tt_setup");
        // Mach-O 64-bit little-endian magic.
        std::fs::write(&tool, [0xCFu8, 0xFA, 0xED, 0xFE]).unwrap();
        std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o644)).unwrap();
        let data = dir.join("data.hda");
        std::fs::write(&data, b"INDX").unwrap();
        // Pinned rather than left to the umask, which differs between
        // machines (002 on the Linux worker gave 0o664) and is not what this
        // test is about: only that the sweep leaves a data file's mode alone.
        std::fs::set_permissions(&data, std::fs::Permissions::from_mode(0o644)).unwrap();
        tools.push((tool, data));
    }

    let (swept, repaired) = storage_manager.repair_exec_modes();
    assert_eq!(swept, 2);
    assert_eq!(repaired, 2);
    for (tool, data) in &tools {
        let mode =
            |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o7777;
        assert_eq!(mode(tool), 0o755);
        assert_eq!(mode(data), 0o644, "a data file is never made executable");
    }

    // Stamped: a repeat sweep visits both trees and repairs nothing.
    let (swept, repaired) = storage_manager.repair_exec_modes();
    assert_eq!((swept, repaired), (2, 0));
}

/// Regression: a single corrupt cached manifest must not abort the whole CAS
/// walk. One package with an unknown platform (or any unparseable manifest)
/// previously made `list_installed` return `Err`, which wedged reconcile,
/// env-var discovery, and every project launch — even for projects that don't
/// depend on the broken package. `list_installed` must warn, skip the broken
/// entry, and still surface every valid package.
#[test]
fn list_installed_skips_unparseable_manifest() {
    let temp_dir = TempDir::new().unwrap();
    let storage_config = StorageConfig {
        home_dir: temp_dir.path().to_path_buf(),
        cache_dir: temp_dir.path().join("cache"),
        packages_dir: temp_dir.path().join("packages"),
        registry_cache_dir: temp_dir.path().join("registry"),
    };
    let storage_manager = StorageManager::new(storage_config).unwrap();

    // A valid package the launcher does depend on.
    let good_dir = temp_dir.path().join("packages").join("good-package@1.0.0");
    std::fs::create_dir_all(&good_dir).unwrap();
    std::fs::write(
        good_dir.join("hpm.toml"),
        r#"
[package]
path = "studio/good-package"
name = "Good Package"
version = "1.0.0"
"#,
    )
    .unwrap();

    // A broken cached package with an invalid platform value, mirroring the
    // real-world `macos-universal` corruption.
    let bad_dir = temp_dir.path().join("packages").join("asset-browser@0.1.0");
    std::fs::create_dir_all(&bad_dir).unwrap();
    std::fs::write(
        bad_dir.join("hpm.toml"),
        r#"
[package]
path = "studio/asset-browser"
name = "Asset Browser"
version = "0.1.0"

[compat]
platforms = ["windows-x86_64", "linux-x86_64", "macos-universal"]
"#,
    )
    .unwrap();

    let packages = storage_manager.list_installed().unwrap();
    assert_eq!(
        packages.len(),
        1,
        "broken manifest must be skipped, not abort the listing; got {:?}",
        packages.iter().map(|p| p.slug()).collect::<Vec<_>>()
    );
    assert_eq!(packages[0].slug(), "good-package");
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
    // `remove_package` resolves to. `get_package_path(name, version)` uses the
    // bare-slug layout, so the link lives at `<packages_dir>/<slug>@<v>/`.
    let cas_path = storage_manager.get_package_path("foo", "1.0.0").unwrap();
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

    // A corrupt manifest must not abort the whole CAS walk — it is warned and
    // skipped so the rest of the store stays usable. (Previously this returned
    // Err, which wedged every consumer over one bad cached package.)
    let packages = storage_manager.list_installed().unwrap();
    assert!(
        packages.is_empty(),
        "corrupted manifest must be skipped, not surfaced; got {:?}",
        packages.iter().map(|p| p.slug()).collect::<Vec<_>>()
    );
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

    let result = storage_manager
        .install_into_cas(&source_dir, "creator/foo", "1.0.0")
        .await;
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

    // The copy lives at a content-addressed `<container>/<hash>` subdir under
    // the `_dev` subtree, and `install_path` points at that hash directory.
    let container = temp_dir
        .path()
        .join("packages")
        .join("_dev")
        .join("foo@1.0.0");
    assert_eq!(installed.install_path.parent().unwrap(), container);
    assert!(installed.install_path.join("MARKER").exists());

    // The registry CAS path must remain empty.
    let registry_cas = temp_dir.path().join("packages").join("foo@1.0.0");
    assert!(
        !registry_cas.exists(),
        "dev install must not touch the registry CAS path"
    );
}

/// Content-addressed dev copies make the rebuild-then-relaunch loop safe. An
/// unchanged re-install reuses the exact same hash directory — no clear, no
/// recopy, which is the fix for the Windows `os error 5` (`PackageInUse`)
/// failure a concurrently-running Houdini triggered. A changed source installs
/// into a *new* hash directory and leaves the old one intact, so a live session
/// still mapping it is never disturbed.
#[tokio::test]
async fn dev_copy_reinstall_is_content_addressed() {
    let temp_dir = TempDir::new().unwrap();
    let storage_config = StorageConfig {
        home_dir: temp_dir.path().to_path_buf(),
        packages_dir: temp_dir.path().join("packages"),
        cache_dir: temp_dir.path().join("cache"),
        registry_cache_dir: temp_dir.path().join("registry"),
    };
    let storage_manager = StorageManager::new(storage_config).unwrap();

    let source = temp_dir.path().join("dev-source");
    write_source_package(&source, "creator/foo", "1.0.0", "v1");
    let first = storage_manager.install_as_dev_copy(&source).await.unwrap();

    // A sentinel inside the copy proves an unchanged re-install doesn't clear it.
    let sentinel = first.install_path.join("SENTINEL");
    std::fs::write(&sentinel, "would-be-removed-by-recopy").unwrap();

    // Source unchanged → the same hash dir is reused; the sentinel survives.
    let again = storage_manager.install_as_dev_copy(&source).await.unwrap();
    assert_eq!(
        again.install_path, first.install_path,
        "unchanged source reuses the content-addressed dir"
    );
    assert!(
        sentinel.exists(),
        "unchanged re-install must not clear the existing copy"
    );

    // Source changed → a new hash dir with fresh content; the old dir (and any
    // files a running Houdini mapped from it) is left untouched.
    write_source_package(&source, "creator/foo", "1.0.0", "v2-longer-marker");
    let rebuilt = storage_manager.install_as_dev_copy(&source).await.unwrap();
    assert_ne!(
        rebuilt.install_path, first.install_path,
        "changed source installs into a new content-addressed dir"
    );
    assert_eq!(
        std::fs::read_to_string(rebuilt.install_path.join("MARKER")).unwrap(),
        "v2-longer-marker"
    );
    assert!(
        first.install_path.exists() && sentinel.exists(),
        "the superseded copy is retained, never removed in place"
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
    storage_manager
        .install_into_cas(&reg_source, "creator/bar", "2.0.0")
        .await
        .unwrap();

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
    let dev_installed = storage_manager
        .install_as_dev_copy(&dev_source)
        .await
        .unwrap();

    let reg_source = temp_dir.path().join("reg-source");
    write_source_package(&reg_source, "creator/foo", "1.0.0", "registry-content");
    storage_manager
        .install_into_cas(&reg_source, "creator/foo", "1.0.0")
        .await
        .unwrap();

    let dev_marker = dev_installed.install_path.join("MARKER");
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

/// Native-binary packages must never be link-installed: a junction/symlink
/// makes the workspace build output the very DSO a running Houdini has
/// memory-mapped, blocking in-place rebuilds (Windows LNK1104). A dev-link
/// request for a package that declares concrete `[compat].platforms` is
/// silently downgraded to a copy — an independent physical file the linker
/// can overwrite freely.
#[tokio::test]
async fn dev_link_install_downgrades_to_copy_for_native_packages() {
    let temp_dir = TempDir::new().unwrap();
    let storage_config = StorageConfig {
        home_dir: temp_dir.path().to_path_buf(),
        packages_dir: temp_dir.path().join("packages"),
        cache_dir: temp_dir.path().join("cache"),
        registry_cache_dir: temp_dir.path().join("registry"),
    };
    let storage_manager = StorageManager::new(storage_config).unwrap();

    let source = temp_dir.path().join("native-source");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::write(
        source.join("hpm.toml"),
        "[package]\npath = \"creator/foo\"\nname = \"creator/foo\"\nversion = \"1.0.0\"\n\
         \n[compat]\nplatforms = [\"linux-x86_64\", \"windows-x86_64\"]\n",
    )
    .unwrap();
    std::fs::write(source.join("MARKER"), "native-content").unwrap();

    let installed = storage_manager.install_as_dev_link(&source).await.unwrap();

    // Downgraded to a content-addressed copy: `install_path` is a
    // `<container>/<hash>` subdir under the `_dev` subtree.
    let container = temp_dir
        .path()
        .join("packages")
        .join("_dev")
        .join("foo@1.0.0");
    assert_eq!(installed.install_path.parent().unwrap(), container);

    // Despite the DevLink request, the entry must be an independent copy —
    // a real directory, not a symlink/junction into the workspace.
    let meta = std::fs::symlink_metadata(&installed.install_path).unwrap();
    let is_link = meta.file_type().is_symlink() || {
        #[cfg(windows)]
        {
            junction::exists(&installed.install_path).unwrap_or(false)
        }
        #[cfg(not(windows))]
        {
            false
        }
    };
    assert!(
        !is_link,
        "native-platform dev install must be a copy, not a symlink/junction"
    );
    assert!(
        installed.is_dev,
        "downgraded install is still a dev install"
    );
    assert_eq!(
        std::fs::read_to_string(installed.install_path.join("MARKER")).unwrap(),
        "native-content"
    );
}

/// A `universal`-only package (pure-data / pure-Python) declares no concrete
/// native platform, so link-mode stays link-mode — live edits must still
/// reach a running session.
#[tokio::test]
async fn dev_link_install_keeps_link_for_universal_packages() {
    let temp_dir = TempDir::new().unwrap();
    let storage_config = StorageConfig {
        home_dir: temp_dir.path().to_path_buf(),
        packages_dir: temp_dir.path().join("packages"),
        cache_dir: temp_dir.path().join("cache"),
        registry_cache_dir: temp_dir.path().join("registry"),
    };
    let storage_manager = StorageManager::new(storage_config).unwrap();

    let source = temp_dir.path().join("universal-source");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::write(
        source.join("hpm.toml"),
        "[package]\npath = \"creator/foo\"\nname = \"creator/foo\"\nversion = \"1.0.0\"\n\
         \n[compat]\nplatforms = [\"universal\"]\n",
    )
    .unwrap();
    std::fs::write(source.join("MARKER"), "universal-content").unwrap();

    let installed = storage_manager.install_as_dev_link(&source).await.unwrap();

    let meta = std::fs::symlink_metadata(&installed.install_path).unwrap();
    let is_link = meta.file_type().is_symlink() || {
        #[cfg(windows)]
        {
            junction::exists(&installed.install_path).unwrap_or(false)
        }
        #[cfg(not(windows))]
        {
            false
        }
    };
    assert!(
        is_link,
        "universal-only dev link must stay a symlink/junction"
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
    storage_manager
        .install_into_cas(&reg_source, "creator/foo", "1.0.0")
        .await
        .unwrap();

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

/// Install a dev copy as an earlier process would have left it: the hold the
/// install takes for its own process (until a project's manifest points at
/// the copy) is released, since that process is long gone.
async fn dev_copy_from_an_earlier_run(storage: &StorageManager, source: &std::path::Path) {
    let installed = storage.install_as_dev_copy(source).await.unwrap();
    dev_users::settle(&installed.install_path);
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
    dev_copy_from_an_earlier_run(&storage_manager, &source).await;
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
    dev_copy_from_an_earlier_run(&storage_manager, &alpha_src).await;
    let beta_src = temp_dir.path().join("beta-src");
    write_source_package(&beta_src, "studio/beta", "1.0.0", "beta");
    dev_copy_from_an_earlier_run(&storage_manager, &beta_src).await;

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
    dev_copy_from_an_earlier_run(&storage_manager, &orphan_src).await;

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

// ------------------------------------------------------------------
// Registry (CAS) package cleanup
// ------------------------------------------------------------------

/// Plant an installed registry package at `packages/<creator>/<slug>@<version>/`,
/// declaring `deps` (name, requirement) as its own registry dependencies.
fn plant_installed(home: &std::path::Path, path: &str, version: &str, deps: &[(&str, &str)]) {
    let slug = path.split_once('/').unwrap().1;
    let dir = home.join("packages").join(format!("{slug}@{version}"));
    std::fs::create_dir_all(&dir).unwrap();
    let mut manifest =
        format!("[package]\npath = \"{path}\"\nname = \"{slug}\"\nversion = \"{version}\"\n");
    if !deps.is_empty() {
        manifest.push_str("\n[dependencies]\n");
        for (name, req) in deps {
            manifest.push_str(&format!("\"{name}\" = \"{req}\"\n"));
        }
    }
    std::fs::write(dir.join("hpm.toml"), manifest).unwrap();
}

/// A project manifest naming `deps` exactly as TumbleTrove Desktop writes
/// one: scoped keys, each at the exact version a launch selected.
fn write_project(dir: &std::path::Path, deps: &[(&str, &str)]) {
    std::fs::create_dir_all(dir).unwrap();
    let mut manifest =
        "[package]\npath = \"studio/project\"\nname = \"project\"\nversion = \"0.1.0\"\n\n[dependencies]\n"
            .to_string();
    for (name, version) in deps {
        manifest.push_str(&format!("\"{name}\" = \"{version}\"\n"));
    }
    std::fs::write(dir.join("hpm.toml"), manifest).unwrap();
}

fn test_storage(temp: &TempDir) -> StorageManager {
    StorageManager::new(StorageConfig {
        home_dir: temp.path().to_path_buf(),
        packages_dir: temp.path().join("packages"),
        cache_dir: temp.path().join("cache"),
        registry_cache_dir: temp.path().join("registry"),
    })
    .unwrap()
}

/// With several versions of a package installed, cleanup keeps the one the
/// project names and offers the rest, whichever order the store scan finds
/// them in. It used to key the installed set by bare slug, keeping one
/// arbitrary version, and a scoped key matched nothing, so every version of
/// every package a Desktop project used was classified orphaned.
#[tokio::test]
async fn cleanup_keeps_the_version_a_project_names() {
    let temp = TempDir::new().unwrap();
    let storage = test_storage(&temp);
    for v in ["1.40.1", "1.74.3", "1.76.1"] {
        plant_installed(temp.path(), "tumbletrove/tumblepipe", v, &[]);
    }
    let project = temp.path().join("project-h21");
    write_project(&project, &[("tumbletrove/tumblepipe", "1.74.3")]);

    let mut orphans = storage
        .cleanup_unused_dry_run(&projects_config_with(vec![project]))
        .await
        .unwrap();
    orphans.sort();
    assert_eq!(orphans, vec!["tumblepipe@1.40.1", "tumblepipe@1.76.1"]);
}

/// Two projects on different versions keep both.
#[tokio::test]
async fn cleanup_keeps_each_projects_version() {
    let temp = TempDir::new().unwrap();
    let storage = test_storage(&temp);
    for v in ["1.74.3", "1.75.0", "1.76.1"] {
        plant_installed(temp.path(), "tumbletrove/tumblepipe", v, &[]);
    }
    let a = temp.path().join("a");
    let b = temp.path().join("b");
    write_project(&a, &[("tumbletrove/tumblepipe", "1.74.3")]);
    write_project(&b, &[("tumbletrove/tumblepipe", "1.76.1")]);

    let orphans = storage
        .cleanup_unused_dry_run(&projects_config_with(vec![a, b]))
        .await
        .unwrap();
    assert_eq!(orphans, vec!["tumblepipe@1.75.0"]);
}

/// A package's own dependency range protects the highest installed version
/// it admits, and only that one.
#[tokio::test]
async fn cleanup_keeps_the_copy_a_dependency_range_resolves_to() {
    let temp = TempDir::new().unwrap();
    let storage = test_storage(&temp);
    plant_installed(
        temp.path(),
        "tumbletrove/tumblepipe",
        "1.74.3",
        &[("tumbletrove/tumbletrove", ">=0.47, <0.48")],
    );
    for v in ["0.46.0", "0.47.1", "0.47.2", "0.48.0"] {
        plant_installed(temp.path(), "tumbletrove/tumbletrove", v, &[]);
    }
    let project = temp.path().join("project");
    write_project(&project, &[("tumbletrove/tumblepipe", "1.74.3")]);

    let mut orphans = storage
        .cleanup_unused_dry_run(&projects_config_with(vec![project]))
        .await
        .unwrap();
    orphans.sort();
    assert_eq!(
        orphans,
        vec![
            "tumbletrove@0.46.0",
            "tumbletrove@0.47.1",
            "tumbletrove@0.48.0"
        ]
    );
}

/// A version the project names but the store lacks protects no other
/// version of the package in its place.
#[tokio::test]
async fn a_missing_version_protects_no_substitute() {
    let temp = TempDir::new().unwrap();
    let storage = test_storage(&temp);
    plant_installed(temp.path(), "tumbletrove/tumblepipe", "1.76.1", &[]);
    let project = temp.path().join("project");
    write_project(&project, &[("tumbletrove/tumblepipe", "1.74.3")]);

    let orphans = storage
        .cleanup_unused_dry_run(&projects_config_with(vec![project]))
        .await
        .unwrap();
    assert_eq!(orphans, vec!["tumblepipe@1.76.1"]);
}

/// A bare-slug key, as older hand-written manifests use, still matches.
#[tokio::test]
async fn a_bare_slug_key_still_matches() {
    let temp = TempDir::new().unwrap();
    let storage = test_storage(&temp);
    plant_installed(temp.path(), "studio/tool", "1.0.0", &[]);
    plant_installed(temp.path(), "studio/tool", "2.0.0", &[]);
    let project = temp.path().join("project");
    write_project(&project, &[("tool", "1.0.0")]);

    let orphans = storage
        .cleanup_unused_dry_run(&projects_config_with(vec![project]))
        .await
        .unwrap();
    assert_eq!(orphans, vec!["tool@2.0.0"]);
}

// ------------------------------------------------------------------
// Fetch staging sweep
// ------------------------------------------------------------------

/// Old leftovers in staging are removed and counted; a young entry (an
/// extraction that may be in flight) and the store itself are not touched.
#[test]
fn sweep_fetch_staging_removes_only_old_leftovers() {
    let temp = TempDir::new().unwrap();
    let storage = test_storage(&temp);
    plant_installed(temp.path(), "studio/tool", "1.0.0", &[]);
    let staging = temp.path().join("fetch");
    let leftover = staging.join("studio-tool-1.0.0");
    std::fs::create_dir_all(leftover.join("python")).unwrap();
    std::fs::write(leftover.join("python").join("a.py"), vec![0u8; 1000]).unwrap();
    std::fs::write(leftover.join("hpm.toml"), vec![0u8; 24]).unwrap();

    // Nothing is old enough under an hour's minimum age.
    assert_eq!(
        storage.sweep_fetch_staging(std::time::Duration::from_secs(3600)),
        (0, 0)
    );
    assert!(leftover.exists());

    let (removed, freed) = storage.sweep_fetch_staging(std::time::Duration::ZERO);
    assert_eq!((removed, freed), (1, 1024));
    assert!(!leftover.exists());
    assert!(staging.exists(), "the staging directory itself stays");
    assert_eq!(
        storage.list_installed().unwrap().len(),
        1,
        "the store is untouched"
    );
}

#[test]
fn sweep_fetch_staging_without_a_staging_dir_is_a_no_op() {
    let temp = TempDir::new().unwrap();
    let storage = test_storage(&temp);
    assert_eq!(
        storage.sweep_fetch_staging(std::time::Duration::ZERO),
        (0, 0)
    );
}

/// Eviction removes the archive, its partial download and the extracted
/// staging copy for exactly the named version, and leaves the store, other
/// versions and other packages alone.
#[test]
fn evict_download_removes_only_that_versions_downloads() {
    let temp = TempDir::new().unwrap();
    let storage = test_storage(&temp);
    plant_installed(temp.path(), "studio/tool", "1.0.0", &[]);
    let staging = temp.path().join("fetch");
    let cache = temp.path().join("cache");
    std::fs::create_dir_all(staging.join("studio-tool-1.0.0").join("bin")).unwrap();
    std::fs::write(
        staging.join("studio-tool-1.0.0").join("bin").join("x"),
        b"x",
    )
    .unwrap();
    std::fs::create_dir_all(&cache).unwrap();
    std::fs::write(cache.join("studio-tool-1.0.0"), b"zip").unwrap();
    std::fs::write(cache.join("studio-tool-1.0.0.part"), b"zi").unwrap();
    // Neighbours that share a prefix must survive.
    std::fs::write(cache.join("studio-tool-1.0.01"), b"zip").unwrap();
    std::fs::write(cache.join("studio-tool-2.0.0"), b"zip").unwrap();
    std::fs::create_dir_all(staging.join("studio-other-1.0.0")).unwrap();

    assert_eq!(storage.evict_download("studio/tool", "1.0.0"), 3);

    assert!(!staging.join("studio-tool-1.0.0").exists());
    assert!(!cache.join("studio-tool-1.0.0").exists());
    assert!(!cache.join("studio-tool-1.0.0.part").exists());
    assert!(cache.join("studio-tool-1.0.01").exists());
    assert!(cache.join("studio-tool-2.0.0").exists());
    assert!(staging.join("studio-other-1.0.0").exists());
    assert_eq!(
        storage.list_installed().unwrap().len(),
        1,
        "the store is untouched"
    );

    assert_eq!(
        storage.evict_download("studio/tool", "1.0.0"),
        0,
        "nothing left to evict"
    );
}

/// Regression: a dev copy must stage the same file set `hpm pack` ships, not
/// the raw workspace. Copying `.git/`, cargo `target/` and CMake trees made a
/// native package's dev copy tens of gigabytes, so it outlasted the caller
/// that requested the launch. `.gitignore` drops build trees, `.hpmignore`'s
/// `!` re-includes a gitignored build output, `[stage]` include/exclude narrow
/// the rest, and `hpm.toml` ships even though the allowlist omits it.
#[tokio::test]
async fn dev_copy_stages_the_pack_file_set() {
    let temp_dir = TempDir::new().unwrap();
    let storage_config = StorageConfig {
        home_dir: temp_dir.path().to_path_buf(),
        packages_dir: temp_dir.path().join("packages"),
        cache_dir: temp_dir.path().join("cache"),
        registry_cache_dir: temp_dir.path().join("registry"),
    };
    let storage_manager = StorageManager::new(storage_config).unwrap();

    let source = temp_dir.path().join("dev-source");
    let write = |rel: &str, body: &str| {
        let path = source.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    };
    write(
        "hpm.toml",
        "[package]\npath = \"creator/foo\"\nname = \"Foo\"\nversion = \"1.0.0\"\n\n\
         [stage]\ninclude = [\"python/**\", \"resolver/**\", \"otls/**\"]\n\
         exclude = [\"**/__pycache__/**\"]\n",
    );
    write(".gitignore", "target/\nbuild-*/\nresolver/\n");
    write(".hpmignore", "!resolver/\n!resolver/**\n");
    write("python/foo/__init__.py", "x = 1");
    write("python/foo/__pycache__/__init__.cpython.pyc", "bytecode");
    write("resolver/lib/resolver.so", "BUILT");
    write("otls/foo.hda", "HDA");
    write(".git/HEAD", "ref: refs/heads/main");
    write("src/crate/target/debug/huge.rlib", "junk");
    write("build-release/CMakeCache.txt", "junk");
    write("tests/test_foo.py", "not in the allowlist");

    let installed = storage_manager.install_as_dev_copy(&source).await.unwrap();
    let root = &installed.install_path;

    let mut files: Vec<String> = walkdir::WalkDir::new(root)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .map(|e| {
            e.path()
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();
    files.sort();
    assert_eq!(
        files,
        vec![
            "hpm.toml",
            "otls/foo.hda",
            "python/foo/__init__.py",
            "resolver/lib/resolver.so",
        ]
    );

    // Churn in an ignored tree (a rebuild's object files, a git fetch) must not
    // change the content hash, or every launch would recopy.
    write("build-release/CMakeCache.txt", "junk-changed-and-longer");
    write(".git/HEAD", "ref: refs/heads/other-branch");
    let again = storage_manager.install_as_dev_copy(&source).await.unwrap();
    assert_eq!(again.install_path, installed.install_path);
}

/// A store install writes its record beside the tree, the record checks the
/// tree as intact, a damaged file is reported, a dev copy is never checked,
/// a reinstall replaces the record, and removal takes it away.
#[tokio::test]
async fn store_installs_carry_an_install_record_through_their_lifecycle() {
    use crate::install_manifest::{ProblemKind, Verification, record_path};

    let temp = TempDir::new().unwrap();
    let storage = test_storage(&temp);

    let source = temp.path().join("reg-source");
    write_source_package(&source, "creator/bar", "2.0.0", "registry");
    let installed = storage
        .install_into_cas(&source, "creator/bar", "2.0.0")
        .await
        .unwrap();
    let record = record_path(&installed.install_path).unwrap();
    assert!(record.is_file(), "a store install writes its record");
    assert_eq!(record.parent(), installed.install_path.parent());

    let Verification::Checked(report) = storage.verify_installed(&installed).unwrap() else {
        panic!("a fresh store install is recorded");
    };
    assert_eq!(report.files, 2);
    assert!(report.is_intact());

    // What a session writes into the package is not damage; a cut-short
    // file is.
    std::fs::create_dir_all(installed.install_path.join("__pycache__")).unwrap();
    std::fs::write(installed.install_path.join("__pycache__/x.pyc"), b"pyc").unwrap();
    std::fs::write(installed.install_path.join("MARKER"), b"reg").unwrap();
    let Verification::Checked(report) = storage.verify_installed(&installed).unwrap() else {
        panic!("still recorded");
    };
    assert_eq!(report.problems.len(), 1);
    assert_eq!(report.problems[0].path, "MARKER");
    assert_eq!(
        report.problems[0].kind,
        ProblemKind::Truncated {
            expected: 8,
            actual: 3
        }
    );

    // An install never replaces a committed copy (a running Houdini may
    // have it loaded): it keeps the existing one, damage and all.
    let kept = storage
        .install_into_cas(&source, "creator/bar", "2.0.0")
        .await
        .unwrap();
    assert_eq!(kept.install_path, installed.install_path);
    let Verification::Checked(report) = storage.verify_installed(&kept).unwrap() else {
        panic!("still recorded");
    };
    assert_eq!(
        report.problems.len(),
        1,
        "the existing copy is kept as it is"
    );

    // A repair moves the copy and its record aside first, as TumbleTrove
    // Desktop does; the reinstall then writes the tree and record together.
    let aside = temp.path().join("aside");
    std::fs::rename(&installed.install_path, &aside).unwrap();
    std::fs::rename(&record, temp.path().join("aside.files.json")).unwrap();
    let reinstalled = storage
        .install_into_cas(&source, "creator/bar", "2.0.0")
        .await
        .unwrap();
    let Verification::Checked(report) = storage.verify_installed(&reinstalled).unwrap() else {
        panic!("a reinstall is recorded");
    };
    assert!(report.is_intact());

    // A dev copy is the user's own tree: nothing to check it against.
    let dev_source = temp.path().join("dev-source");
    write_source_package(&dev_source, "creator/foo", "1.0.0", "dev");
    let dev = storage.install_as_dev_copy(&dev_source).await.unwrap();
    assert_eq!(
        storage.verify_installed(&dev).unwrap(),
        Verification::Unrecorded
    );

    storage.remove_package("bar", "2.0.0").await.unwrap();
    assert!(!record.exists(), "removal takes the record with the tree");
}

// ------------------------------------------------------------------
// Store layout: one directory per (slug, version)
// ------------------------------------------------------------------

mod store_layout {
    use super::*;
    use proptest::prelude::*;

    fn slug() -> impl Strategy<Value = String> {
        "[a-z][a-z0-9-]{0,10}[a-z0-9]"
    }

    fn version() -> impl Strategy<Value = String> {
        (0u32..20, 0u32..20, 0u32..20).prop_map(|(a, b, c)| format!("{a}.{b}.{c}"))
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(24))]

        /// Every lookup by slug or scoped name finds the directory `install_into_cas`
        /// wrote: `get_package_path`, `package_exists`, `list_installed` and
        /// the lockfile's `layout::cas_dir`, for the bare and the scoped name.
        #[test]
        fn lookups_by_slug_agree_with_the_installed_directory(
            creator in slug(), slug in slug(), version in version()
        ) {
            let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
            let temp = TempDir::new().unwrap();
            let storage = test_storage(&temp);
            let source = temp.path().join("src");
            std::fs::create_dir_all(&source).unwrap();
            std::fs::write(
                source.join("hpm.toml"),
                format!("[package]\npath = \"{creator}/{slug}\"\nname = \"x\"\nversion = \"{version}\"\n"),
            )
            .unwrap();
            let installed = rt.block_on(storage.install_into_cas(&source, &format!("{creator}/{slug}"), &version)).unwrap();

            prop_assert_eq!(&storage.get_package_path(&slug, &version).unwrap(), &installed.install_path);
            prop_assert!(storage.package_exists(&slug, &version));
            let scoped = format!("{creator}/{slug}");
            prop_assert_eq!(&storage.get_package_path(&scoped, &version).unwrap(), &installed.install_path);
            prop_assert!(storage.package_exists(&scoped, &version));
            prop_assert_eq!(
                crate::storage::layout::cas_dir(
                    &storage.config.packages_dir,
                    &format!("{creator}/{slug}"),
                    &version
                )
                .unwrap(),
                installed.install_path.clone()
            );
            let listed: Vec<_> = storage
                .list_installed()
                .unwrap()
                .into_iter()
                .map(|p| p.install_path)
                .collect();
            prop_assert_eq!(listed, vec![installed.install_path]);
        }
    }
}

/// With `packages_dir` and `cache_dir` moved out of `home_dir`, staging stays
/// under `home_dir/fetch`, archives go to `cache_dir`, and what older hpm
/// versions left at `<packages_dir>/../fetch` and `<home_dir>/cache` is still
/// swept and evicted.
#[test]
fn moved_store_keeps_one_staging_root_and_reclaims_the_old_ones() {
    let temp = TempDir::new().unwrap();
    let home = temp.path().join("home");
    let storage = StorageManager::new(StorageConfig {
        home_dir: home.clone(),
        packages_dir: temp.path().join("disk/packages"),
        cache_dir: temp.path().join("fast/cache"),
        registry_cache_dir: home.join("registry"),
    })
    .unwrap();
    let layout = storage.layout();
    assert_eq!(layout.fetch_dir(), home.join("fetch"));
    assert_eq!(layout.cache_dir(), temp.path().join("fast/cache"));
    let fetcher = crate::archive_fetcher::ArchiveFetcher::for_storage(&storage.config).unwrap();
    drop(fetcher);
    assert!(
        home.join("fetch").is_dir(),
        "the fetcher stages under home_dir"
    );

    let old_staging = temp.path().join("disk/fetch/studio-tool-1.0.0");
    std::fs::create_dir_all(&old_staging).unwrap();
    assert_eq!(storage.sweep_fetch_staging(std::time::Duration::ZERO).0, 1);
    assert!(!old_staging.exists());

    let old_cache = home.join("cache");
    std::fs::create_dir_all(&old_cache).unwrap();
    std::fs::write(old_cache.join("studio-tool-1.0.0"), b"zip").unwrap();
    std::fs::write(temp.path().join("fast/cache/studio-tool-1.0.0"), b"zip").unwrap();
    assert_eq!(storage.evict_download("studio/tool", "1.0.0"), 2);
}

/// A store copy staged by an install whose process is gone is swept; one
/// staged by a live process (this one) is an install in flight and stays.
/// Neither is ever listed as a package.
#[test]
fn store_stages_of_gone_installs_are_swept_and_never_listed() {
    let temp = TempDir::new().unwrap();
    let storage = test_storage(&temp);
    let packages = temp.path().join("packages");
    // PIDs this high are not handed out on any supported OS.
    let gone = packages.join(format!(".stage-{}-0", 0x7fff_fff0u32));
    let live = packages.join(format!(".stage-{}-0", std::process::id()));
    for stage in [&gone, &live] {
        std::fs::create_dir_all(stage.join("tool@1.0.0")).unwrap();
        std::fs::write(
            stage.join("tool@1.0.0").join("hpm.toml"),
            "[package]\npath = \"studio/tool\"\nname = \"t\"\nversion = \"1.0.0\"\n",
        )
        .unwrap();
    }
    assert!(storage.list_installed().unwrap().is_empty());

    let (removed, _) = storage.sweep_fetch_staging(std::time::Duration::from_secs(3600));
    assert_eq!(removed, 1);
    assert!(!gone.exists());
    assert!(live.exists());
}

// ------------------------------------------------------------------
// Cleanup plan: dry run equals real run
// ------------------------------------------------------------------

mod cleanup_plan {
    use super::*;
    use proptest::prelude::*;
    use std::collections::{BTreeMap, BTreeSet};

    const SLUGS: [&str; 4] = ["a", "b", "c", "d"];
    const VERSIONS: [&str; 2] = ["1.0.0", "2.0.0"];

    /// A package coordinate: indices into `SLUGS` and `VERSIONS`.
    type Coord = (usize, usize);

    /// A store (each installed coordinate with at most one dependency) and
    /// the project's direct dependencies.
    #[derive(Debug)]
    struct Fixture {
        installed: BTreeMap<Coord, Option<Coord>>,
        roots: BTreeMap<usize, usize>,
    }

    /// Plant an installed package where installs put it,
    /// `<packages>/<slug>@<version>`.
    fn plant_in_store(home: &std::path::Path, (s, v): Coord, dep: Option<Coord>) {
        let (slug, version) = (SLUGS[s], VERSIONS[v]);
        let dir = home.join("packages").join(format!("{slug}@{version}"));
        std::fs::create_dir_all(&dir).unwrap();
        let mut manifest = format!(
            "[package]\npath = \"studio/{slug}\"\nname = \"{slug}\"\nversion = \"{version}\"\n"
        );
        if let Some((ds, dv)) = dep {
            manifest.push_str(&format!(
                "\n[dependencies]\n\"studio/{}\" = \"{}\"\n",
                SLUGS[ds], VERSIONS[dv]
            ));
        }
        std::fs::write(dir.join("hpm.toml"), manifest).unwrap();
    }

    /// Run the dry and the real cleanup on `fixture`, returning both id sets
    /// and the model's: installed minus everything reachable from the roots.
    fn run(fixture: &Fixture) -> (BTreeSet<String>, BTreeSet<String>, BTreeSet<String>) {
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let temp = TempDir::new().unwrap();
        let storage = test_storage(&temp);
        for (coord, dep) in &fixture.installed {
            plant_in_store(temp.path(), *coord, *dep);
        }
        let project = temp.path().join("project");
        let names: Vec<(String, &str)> = fixture
            .roots
            .iter()
            .map(|(s, v)| (format!("studio/{}", SLUGS[*s]), VERSIONS[*v]))
            .collect();
        let refs: Vec<(&str, &str)> = names.iter().map(|(n, v)| (n.as_str(), *v)).collect();
        write_project(&project, &refs);
        let config = projects_config_with(vec![project]);

        let mut reachable = BTreeSet::new();
        let mut stack: Vec<Coord> = fixture.roots.iter().map(|(s, v)| (*s, *v)).collect();
        while let Some(c) = stack.pop() {
            if let Some(dep) = fixture.installed.get(&c)
                && reachable.insert(c)
            {
                stack.extend(*dep);
            }
        }
        let model = fixture
            .installed
            .keys()
            .filter(|c| !reachable.contains(c))
            .map(|(s, v)| format!("{}@{}", SLUGS[*s], VERSIONS[*v]))
            .collect();

        let dry = rt
            .block_on(storage.cleanup_unused_dry_run(&config))
            .unwrap();
        let real = rt.block_on(storage.cleanup_unused(&config)).unwrap();
        (dry.into_iter().collect(), real.into_iter().collect(), model)
    }

    /// One version per package: every coordinate of slug `s` is at
    /// `version_of[s]`, so the graph's one-version-per-name rule never bites.
    fn single_version_fixture() -> impl Strategy<Value = Fixture> {
        (
            prop::collection::vec(0..VERSIONS.len(), SLUGS.len()),
            prop::collection::vec((0..SLUGS.len(), prop::option::of(0..SLUGS.len())), 1..6),
            prop::collection::btree_set(0..SLUGS.len(), 0..3),
        )
            .prop_map(|(version_of, store, roots)| Fixture {
                installed: store
                    .into_iter()
                    .map(|(s, dep)| ((s, version_of[s]), dep.map(|d| (d, version_of[d]))))
                    .collect(),
                roots: roots.into_iter().map(|s| (s, version_of[s])).collect(),
            })
    }

    /// Any versions, including one package named at two versions.
    fn mixed_version_fixture() -> impl Strategy<Value = Fixture> {
        let coord = (0..SLUGS.len(), 0..VERSIONS.len());
        (
            prop::collection::vec((coord.clone(), prop::option::of(coord.clone())), 1..8),
            prop::collection::vec(coord, 0..3),
        )
            .prop_map(|(store, roots)| Fixture {
                installed: store.into_iter().collect(),
                roots: roots.into_iter().collect(),
            })
    }

    /// The project pins `a@2.0.0` and also uses `d@1.0.0`, which depends
    /// on `a@1.0.0`: both versions of `a` are kept, in either listing order.
    /// The graph used to keep `a@1.0.0` only when `d` was listed first.
    #[test]
    fn a_direct_pin_does_not_hide_a_dependency_on_another_version() {
        let fixture = Fixture {
            installed: [((0, 0), None), ((3, 0), Some((0, 0)))]
                .into_iter()
                .collect(),
            roots: [(3, 0), (0, 1)].into_iter().collect(),
        };
        let (dry, real, model) = run(&fixture);
        assert!(model.is_empty());
        assert_eq!((dry, real), (model.clone(), model));
    }

    /// `b@1.0.0` depending on `b@2.0.0` keeps `b@2.0.0`: the cycle guard is
    /// by name and version, not by name.
    #[test]
    fn a_dependency_on_another_version_of_itself_is_kept() {
        let fixture = Fixture {
            installed: [
                ((0, 1), Some((1, 0))),
                ((1, 0), Some((1, 1))),
                ((1, 1), None),
            ]
            .into_iter()
            .collect(),
            roots: [(0, 1)].into_iter().collect(),
        };
        let (dry, real, model) = run(&fixture);
        assert!(model.is_empty());
        assert_eq!((dry, real), (model.clone(), model));
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(48))]

        /// With one version per package, the dry run names exactly what the
        /// real run removes, and both are installed minus reachable.
        #[test]
        fn dry_and_real_run_both_equal_the_model(fixture in single_version_fixture()) {
            let (dry, real, model) = run(&fixture);
            prop_assert_eq!(&dry, &model);
            prop_assert_eq!(&real, &model);
        }

        /// With any versions, including a project pinning one version of a
        /// package that another package depends on at a different version,
        /// both runs still equal the model: every version reachable from the
        /// project is kept, whatever order the dependencies are listed in.
        #[test]
        fn dry_and_real_run_equal_the_model_with_any_versions(fixture in mixed_version_fixture()) {
            let (dry, real, model) = run(&fixture);
            prop_assert_eq!(&dry, &model);
            prop_assert_eq!(&real, &model);
        }
    }
}

/// Every path under `root` with its file bytes: equal before and after an
/// operation that must write nothing.
fn tree_snapshot(root: &std::path::Path) -> Vec<(std::path::PathBuf, Option<Vec<u8>>)> {
    let mut entries: Vec<_> = walkdir::WalkDir::new(root)
        .into_iter()
        .map(|e| {
            let e = e.unwrap();
            let bytes = e
                .file_type()
                .is_file()
                .then(|| std::fs::read(e.path()).unwrap());
            (e.path().strip_prefix(root).unwrap().to_path_buf(), bytes)
        })
        .collect();
    entries.sort();
    entries
}

/// An orphaned dev install whose copy something still uses is not offered
/// by the dry run and is kept by the real run, and the dry run writes
/// nothing. Users: a project whose emitted manifest still points at the
/// copy though its hpm.toml no longer claims it (synced before records
/// existed, so only the real run's backfill records it), and this process's
/// hold. With no user, both runs agree on removing it.
#[tokio::test]
async fn dev_dry_run_agrees_with_the_real_run_and_writes_nothing() {
    for user in ["project manifest", "process hold", "none"] {
        let temp = TempDir::new().unwrap();
        let storage = test_storage(&temp);
        let source = temp.path().join("orphan-src");
        write_source_package(&source, "studio/orphan", "1.0.0", "m");
        let copy = storage
            .install_as_dev_copy(&source)
            .await
            .unwrap()
            .install_path;
        dev_users::settle(&copy);

        let project = temp.path().join("project");
        write_project_with_path_dep(
            &project,
            "consumer",
            "1.0.0",
            "studio/other",
            "../other",
            false,
        );
        let packages = project.join(".hpm/packages");
        std::fs::create_dir_all(&packages).unwrap();
        let _hold = match user {
            "project manifest" => {
                let hpath = serde_json::json!({ "hpath": [copy.to_string_lossy()] });
                std::fs::write(packages.join("studio.orphan.json"), hpath.to_string()).unwrap();
                None
            }
            "process hold" => dev_users::DevCopyHold::acquire(&copy),
            _ => None,
        };
        let config = projects_config_with(vec![project]);

        let before = tree_snapshot(temp.path());
        let dry = storage
            .cleanup_unused_dev_installs_dry_run(&config)
            .await
            .unwrap();
        assert_eq!(
            tree_snapshot(temp.path()),
            before,
            "{user}: the dry run wrote something"
        );
        let real = storage.cleanup_unused_dev_installs(&config).await.unwrap();
        assert_eq!(
            dry, real,
            "{user}: the dry run must name exactly what the real run removes"
        );
        let expected: Vec<String> = if user == "none" {
            vec!["_dev/orphan@1.0.0".to_string()]
        } else {
            vec![]
        };
        assert_eq!(real, expected, "{user}");
        assert_eq!(copy.is_dir(), user != "none", "{user}");
    }
}

/// What counts as an installed package in the store: a `<slug>@<version>`
/// directory directly in it, holding `hpm.toml`. The install record and
/// exec-mode stamp beside it, `_dev`, staging and tombstone directories, a
/// directory without a manifest, and a directory without `@` (or anything
/// under one) are not packages.
#[test]
fn only_slug_at_version_directories_are_packages() {
    let temp = TempDir::new().unwrap();
    let storage = test_storage(&temp);
    let packages = temp.path().join("packages");
    let manifest = |slug: &str, v: &str| {
        format!("[package]\npath = \"studio/{slug}\"\nname = \"{slug}\"\nversion = \"{v}\"\n")
    };
    for (slug, v) in [("tool", "1.0.0"), ("tool", "2.0.0"), ("rig", "0.1.0")] {
        let dir = packages.join(format!("{slug}@{v}"));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("hpm.toml"), manifest(slug, v)).unwrap();
    }
    std::fs::write(packages.join("tool@1.0.0.files.json"), b"{}").unwrap();
    std::fs::write(packages.join("tool@1.0.0.exec-modes"), b"1").unwrap();
    std::fs::create_dir_all(packages.join("empty@1.0.0")).unwrap();
    // A directory without `@`, even one holding a manifest, is not a store
    // copy; nor is anything under it (no hpm writes `<creator>/<slug>@<v>`).
    std::fs::create_dir_all(packages.join("notes")).unwrap();
    std::fs::write(
        packages.join("notes").join("hpm.toml"),
        manifest("notes", "1.0.0"),
    )
    .unwrap();
    let scoped = packages.join("studio").join("scoped@1.0.0");
    std::fs::create_dir_all(&scoped).unwrap();
    std::fs::write(scoped.join("hpm.toml"), manifest("scoped", "1.0.0")).unwrap();
    for hidden in [".stage-1-0", ".reclaim-x"] {
        let dir = packages.join(hidden).join("x@1.0.0");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("hpm.toml"), manifest("x", "1.0.0")).unwrap();
    }
    let dev = packages.join("_dev").join("dev@1.0.0");
    std::fs::create_dir_all(&dev).unwrap();
    std::fs::write(dev.join("hpm.toml"), manifest("dev", "1.0.0")).unwrap();

    let mut listed: Vec<String> = storage
        .list_installed()
        .unwrap()
        .iter()
        .map(|p| {
            p.install_path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    listed.sort();
    assert_eq!(listed, ["rig@0.1.0", "tool@1.0.0", "tool@2.0.0"]);
}

/// A copy already in the store whose `hpm.toml` this hpm refuses (written
/// under looser rules) is kept, as every committed copy is, and the install
/// reports the manifest error, naming the file, rather than "not found".
#[tokio::test]
async fn reinstalling_over_a_copy_whose_manifest_no_longer_parses_names_the_manifest() {
    let temp = TempDir::new().unwrap();
    let storage = test_storage(&temp);
    let source = temp.path().join("reg-source");
    write_source_package(&source, "creator/bar", "2.0.0", "registry");
    let installed = storage
        .install_into_cas(&source, "creator/bar", "2.0.0")
        .await
        .unwrap();

    let manifest = installed.install_path.join("hpm.toml");
    let mut perms = std::fs::metadata(&manifest).unwrap().permissions();
    #[allow(clippy::permissions_set_readonly_false)]
    perms.set_readonly(false);
    std::fs::set_permissions(&manifest, perms).unwrap();
    let text = std::fs::read_to_string(&manifest).unwrap();
    std::fs::write(
        &manifest,
        format!("{text}\n[dependencies]\n\"creator/x\" = {{ version = \"1\", registy = \"r\" }}\n"),
    )
    .unwrap();

    let err = storage
        .install_into_cas(&source, "creator/bar", "2.0.0")
        .await
        .unwrap_err();
    assert!(
        matches!(err, StorageError::Manifest(_)),
        "expected the manifest error, got {err}"
    );
    assert!(err.to_string().contains("hpm.toml"), "{err}");
}

/// The store keys a registry install on its slug, so two creators' packages
/// with the same slug and version share a folder. The second install is
/// refused, naming both, instead of being handed the first creator's copy.
#[tokio::test]
async fn a_second_creators_package_with_the_same_slug_is_refused() {
    let temp = TempDir::new().unwrap();
    let storage = test_storage(&temp);
    let first = temp.path().join("first");
    write_source_package(&first, "acme/bar", "2.0.0", "acme");
    let installed = storage
        .install_into_cas(&first, "acme/bar", "2.0.0")
        .await
        .unwrap();

    let second = temp.path().join("second");
    write_source_package(&second, "other/bar", "2.0.0", "other");
    let err = storage
        .install_into_cas(&second, "other/bar", "2.0.0")
        .await
        .unwrap_err();
    let text = err.to_string();
    assert!(
        matches!(err, StorageError::SlugCollision { .. }),
        "expected a slug collision, got {text}"
    );
    assert!(
        text.contains("acme/bar") && text.contains("other/bar"),
        "{text}"
    );
    assert_eq!(
        std::fs::read_to_string(installed.install_path.join("MARKER")).unwrap(),
        "acme",
        "the first creator's copy is untouched"
    );
}

// ------------------------------------------------------------------
// Store identity: an archive fills only the slot it declares
// ------------------------------------------------------------------

mod store_identity {
    use super::*;
    use proptest::prelude::*;

    /// Small domains, so requested and declared identities often agree,
    /// often differ in one part, and sometimes differ only in case.
    fn creator() -> impl Strategy<Value = &'static str> {
        prop::sample::select(vec!["ana", "Ana", "bo"])
    }
    fn slug() -> impl Strategy<Value = &'static str> {
        prop::sample::select(vec!["tool", "Tool", "tools"])
    }
    fn version() -> impl Strategy<Value = &'static str> {
        prop::sample::select(vec!["1.0.0", "1.0.1", "2.0.0-rc.1"])
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(48))]

        /// `install_into_cas` stores an archive exactly when its manifest
        /// declares the identity it was requested as (ASCII case aside, a
        /// bare-slug request matching the slug); otherwise the requested
        /// slot stays empty.
        #[test]
        fn an_archive_fills_only_the_slot_it_declares(
            // A manifest's own path is lowercase; requests vary in case.
            declared in (
                prop::sample::select(vec!["ana", "bo"]),
                prop::sample::select(vec!["tool", "tools"]),
                version(),
            ),
            requested in (creator(), slug(), version()),
            scoped_request in any::<bool>(),
        ) {
            let (d_creator, d_slug, d_version) = declared;
            let (r_creator, r_slug, r_version) = requested;
            let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
            let temp = TempDir::new().unwrap();
            let storage = test_storage(&temp);
            let source = temp.path().join("src");
            std::fs::create_dir_all(&source).unwrap();
            std::fs::write(
                source.join("hpm.toml"),
                format!("[package]\npath = \"{d_creator}/{d_slug}\"\nname = \"x\"\nversion = \"{d_version}\"\n"),
            )
            .unwrap();
            let requested_name = if scoped_request {
                format!("{r_creator}/{r_slug}")
            } else {
                r_slug.to_string()
            };

            let same = (!scoped_request || d_creator.eq_ignore_ascii_case(r_creator))
                && d_slug.eq_ignore_ascii_case(r_slug)
                && d_version == r_version;
            let result = rt.block_on(storage.install_into_cas(&source, &requested_name, r_version));
            prop_assert_eq!(result.is_ok(), same, "{:?} as {}@{}: {:?}", declared, requested_name, r_version, result.as_ref().err().map(|e| e.to_string()));
            if !same {
                prop_assert!(
                    matches!(result, Err(StorageError::IdentityMismatch { .. })),
                    "{:?}", result
                );
                prop_assert!(!storage.package_exists(&requested_name, r_version));
                prop_assert!(storage.list_installed().unwrap().is_empty());
            }
        }
    }
}
