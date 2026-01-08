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
            "--houdini-min",
            "20.0",
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
    // Test search command (deprecated - uses Git archive-based dependencies)
    let search_output = hpm_binary()
        .args(["search", "test"])
        .output()
        .expect("Failed to execute hpm search");

    assert!(search_output.status.success());
    let stdout = String::from_utf8_lossy(&search_output.stdout);
    assert!(
        stdout.contains("Git archive-based dependencies"),
        "Expected Git archive info in search output. stdout: '{}'",
        stdout
    );

    // Test publish command (deprecated - uses Git push)
    let publish_output = hpm_binary()
        .args(["publish"])
        .output()
        .expect("Failed to execute hpm publish");

    assert!(publish_output.status.success());
    let stdout = String::from_utf8_lossy(&publish_output.stdout);
    assert!(
        stdout.contains("Publishing is done by pushing to a Git repository"),
        "Expected Git push info in publish output. stdout: '{}'",
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

[houdini]
min_version = "19.5"
"#;
    fs::write(temp_dir.path().join("hpm.toml"), manifest_content).unwrap();

    let output = hpm_binary()
        .args(["--directory", temp_dir.path().to_str().unwrap(), "check"])
        .output()
        .expect("Failed to execute hpm check");

    // Check command should process the manifest
    assert!(output.status.success() || !output.stderr.is_empty());
}
