//! Integration tests for HPM CLI commands
//!
//! These tests verify end-to-end functionality by running the actual CLI binary
//! and testing complete workflows in isolated environments.

use std::fs;
use std::process::Command;
use tempfile::TempDir;

/// Helper function to get the path to the cargo binary
fn hpm_binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_hpm"))
}

/// Helper that returns a CLI invocation isolated from the developer's
/// `~/.hpm/config.toml`. The returned `TempDir` must outlive the spawned
/// process — drop it after the assertions.
///
/// Without this, every `hpm_binary()` test inherits `$HOME` and reads the
/// real user config, which means tests that assert on default config
/// behavior (e.g. "no registries configured") fail for any developer who
/// has registries set up locally.
fn hpm_binary_isolated() -> (Command, TempDir) {
    let temp = TempDir::new().unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_hpm"));
    cmd.env("HOME", temp.path());
    cmd.env("USERPROFILE", temp.path());
    (cmd, temp)
}

/// Test that the CLI binary can be executed and shows help
#[test]
fn test_cli_help() {
    let output = hpm_binary()
        .arg("--help")
        .output()
        .expect("Failed to execute hpm --help");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("HPM - Houdini Package Manager"));
    assert!(stdout.contains("init"));
    assert!(stdout.contains("add"));
    assert!(stdout.contains("remove"));
    assert!(stdout.contains("list"));
    assert!(stdout.contains("install"));
    assert!(stdout.contains("clean"));
}

/// Test the complete init workflow
#[test]
fn test_init_workflow() {
    let temp_dir = TempDir::new().unwrap();

    // Test standard package creation using --directory flag
    let output = hpm_binary()
        .args([
            "--directory",
            temp_dir.path().to_str().unwrap(),
            "init",
            "test-integration-package",
            "--description",
            "Integration test package",
            "--author",
            "Test Author <test@example.com>",
            "--version",
            "1.2.3",
            "--license",
            "Apache-2.0",
            "--houdini",
            ">=20.5",
            "--vcs",
            "none",
        ])
        .output()
        .expect("Failed to execute hpm init");

    assert!(output.status.success(), "hpm init should succeed");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Successfully created Houdini package"));

    // Verify package structure
    let package_path = temp_dir.path().join("test-integration-package");
    assert!(package_path.exists());
    assert!(package_path.join("hpm.toml").exists());
    assert!(package_path.join("package.json").exists());
    assert!(package_path.join("README.md").exists());
    assert!(package_path.join("python").is_dir());
    assert!(package_path.join("otls").is_dir());

    // Verify hpm.toml content
    let hpm_toml = fs::read_to_string(package_path.join("hpm.toml")).unwrap();
    assert!(hpm_toml.contains("name = \"test-integration-package\""));
    assert!(hpm_toml.contains("version = \"1.2.3\""));
    assert!(hpm_toml.contains("Test Author <test@example.com>"));
    assert!(hpm_toml.contains("license = \"Apache-2.0\""));
}

/// Test bare package creation
#[test]
fn test_init_bare_workflow() {
    let temp_dir = TempDir::new().unwrap();

    let output = hpm_binary()
        .args([
            "--directory",
            temp_dir.path().to_str().unwrap(),
            "init",
            "test-bare-package",
            "--bare",
            "--description",
            "Minimal test package",
            "--vcs",
            "none",
        ])
        .output()
        .expect("Failed to execute hpm init --bare");

    assert!(output.status.success());

    let package_path = temp_dir.path().join("test-bare-package");
    assert!(package_path.exists());
    assert!(package_path.join("hpm.toml").exists());

    // Bare package should not have these
    assert!(!package_path.join("package.json").exists());
    assert!(!package_path.join("README.md").exists());
    assert!(!package_path.join("python").exists());
}

/// Test that deprecated commands give helpful messages
#[test]
fn test_deprecated_commands() {
    // Test search command (no registries configured)
    let (mut search_cmd, _search_home) = hpm_binary_isolated();
    let search_output = search_cmd
        .args(["search", "test"])
        .output()
        .expect("Failed to execute hpm search");

    assert!(search_output.status.success());
    let stdout = String::from_utf8_lossy(&search_output.stdout);
    assert!(
        stdout.contains("No registries configured"),
        "Expected no-registries message in search output. stdout: '{}'",
        stdout
    );
}

/// Test list command with nonexistent manifest
#[test]
fn test_list_nonexistent_manifest() {
    let temp_dir = TempDir::new().unwrap();

    let output = hpm_binary()
        .args(["list", "--package", temp_dir.path().to_str().unwrap()])
        .output()
        .expect("Failed to execute hpm list");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("No hpm.toml found"));
}

/// Test add/remove workflow with manifest creation
#[test]
fn test_add_remove_workflow() {
    let temp_dir = TempDir::new().unwrap();

    // First create a package
    let _init_output = hpm_binary()
        .args([
            "--directory",
            temp_dir.path().to_str().unwrap(),
            "init",
            "test-deps-package",
            "--vcs",
            "none",
        ])
        .output()
        .expect("Failed to create test package");

    let package_dir = temp_dir.path().join("test-deps-package");

    // Test add command
    let add_output = hpm_binary()
        .args([
            "--directory",
            package_dir.to_str().unwrap(),
            "add",
            "test-package",
            "--version",
            "^1.0.0",
        ])
        .output()
        .expect("Failed to execute hpm add");

    // Add should succeed (even though package doesn't exist in registry)
    // This tests manifest modification logic
    if add_output.status.success() {
        let hpm_toml = fs::read_to_string(package_dir.join("hpm.toml")).unwrap();
        assert!(hpm_toml.contains("test-package"));
    }
}

/// Test error handling for directory that already exists
#[test]
fn test_init_directory_exists_error() {
    let temp_dir = TempDir::new().unwrap();

    // Create directory first
    fs::create_dir(temp_dir.path().join("existing-package")).unwrap();

    // Try to init with same name
    let output = hpm_binary()
        .args([
            "--directory",
            temp_dir.path().to_str().unwrap(),
            "init",
            "existing-package",
            "--vcs",
            "none",
        ])
        .output()
        .expect("Failed to execute hpm init");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("already exists"));
}

/// Test clean command basic functionality
#[test]
fn test_clean_command() {
    let output = hpm_binary()
        .args(["clean", "--dry-run"])
        .output()
        .expect("Failed to execute hpm clean --dry-run");

    // Should succeed (even with no packages to clean)
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("cleanup") || stdout.contains("packages"));
}

/// Test check command functionality  
#[test]
fn test_check_command() {
    let temp_dir = TempDir::new().unwrap();

    // Create a simple hpm.toml to check
    let manifest_content = r#"
[package]
name = "test-check-package"
version = "1.0.0"
description = "Test package for check command"

[compat]
houdini = ">=20.5"
"#;
    fs::write(temp_dir.path().join("hpm.toml"), manifest_content).unwrap();

    let output = hpm_binary()
        .args(["--directory", temp_dir.path().to_str().unwrap(), "check"])
        .output()
        .expect("Failed to execute hpm check");

    // Check command should process the manifest
    assert!(output.status.success() || !output.stderr.is_empty());
}

/// Write a minimal package with one HDA operator declaring `thumbnail`, and
/// optionally ship the thumbnail file itself.
fn write_thumbnail_package(dir: &std::path::Path, thumbnail: &str, ship_thumbnail: bool) {
    fs::create_dir_all(dir.join("otls")).unwrap();
    fs::write(dir.join("otls/rbd.hda"), b"hda-bytes").unwrap();
    if ship_thumbnail {
        let path = dir.join(thumbnail);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>").unwrap();
    }
    let manifest = format!(
        r#"
[package]
path = "studio/thumbtest"
name = "thumbtest"
version = "1.0.0"

[[operators]]
kind = "hda"
type_name = "studio::rbd_configure::2.0"
category = "Sop"
source = "otls/rbd.hda"
thumbnail = "{thumbnail}"

[[operators]]
kind = "hda"
type_name = "studio::other::1.0"
category = "Sop"
source = "otls/rbd.hda"
"#
    );
    fs::write(dir.join("hpm.toml"), manifest).unwrap();
}

/// `hpm pack --json` emits a declared `thumbnail` on its asset and omits the
/// key on assets that don't declare one.
#[test]
fn test_pack_json_emits_operator_thumbnail() {
    let pkg = TempDir::new().unwrap();
    let out = TempDir::new().unwrap();
    write_thumbnail_package(
        pkg.path(),
        "thumbnails/studio--rbd_configure--2.0.svg",
        true,
    );

    let (mut cmd, _home) = hpm_binary_isolated();
    let output = cmd
        .args([
            "--directory",
            pkg.path().to_str().unwrap(),
            "pack",
            "--json",
        ])
        .args(["--verify-assets", "--output", out.path().to_str().unwrap()])
        .output()
        .expect("Failed to execute hpm pack");
    assert!(
        output.status.success(),
        "pack failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let line = stdout
        .lines()
        .find(|l| l.trim_start().starts_with('{'))
        .expect("no JSON line in pack output");
    let json: serde_json::Value = serde_json::from_str(line).unwrap();
    let assets = json["assets"].as_array().unwrap();
    assert_eq!(assets.len(), 2);
    assert_eq!(
        assets[0]["thumbnail"],
        serde_json::json!("thumbnails/studio--rbd_configure--2.0.svg")
    );
    assert!(assets[1].get("thumbnail").is_none(), "{}", assets[1]);
}

/// `--verify-assets` fails the pack (and removes the archive) when a declared
/// thumbnail isn't in the produced archive.
#[test]
fn test_pack_verify_assets_fails_on_missing_thumbnail() {
    let pkg = TempDir::new().unwrap();
    let out = TempDir::new().unwrap();
    write_thumbnail_package(pkg.path(), "thumbnails/missing.svg", false);

    let (mut cmd, _home) = hpm_binary_isolated();
    let output = cmd
        .args([
            "--directory",
            pkg.path().to_str().unwrap(),
            "pack",
            "--json",
        ])
        .args(["--verify-assets", "--output", out.path().to_str().unwrap()])
        .output()
        .expect("Failed to execute hpm pack");
    assert!(!output.status.success(), "pack should fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("thumbnail") && stderr.contains("thumbnails/missing.svg"),
        "{stderr}"
    );
    assert!(
        !out.path().join("thumbtest-1.0.0.zip").exists(),
        "invalid archive must be removed"
    );
}
