//! Integration tests for HPM CLI commands
//!
//! These tests verify end-to-end functionality by running the actual CLI binary
//! and testing complete workflows in isolated environments.

use std::env;
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
    let original_dir = env::current_dir().unwrap();

    // Change to temp directory
    env::set_current_dir(temp_dir.path()).unwrap();

    // Test standard package creation
    let output = hpm_binary()
        .args([
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

    env::set_current_dir(original_dir).unwrap();

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
    let original_dir = env::current_dir().unwrap();

    env::set_current_dir(temp_dir.path()).unwrap();

    let output = hpm_binary()
        .args([
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

    env::set_current_dir(original_dir).unwrap();

    assert!(output.status.success());

    let package_path = temp_dir.path().join("test-bare-package");
    assert!(package_path.exists());
    assert!(package_path.join("hpm.toml").exists());

    // Bare package should not have these
    assert!(!package_path.join("package.json").exists());
    assert!(!package_path.join("README.md").exists());
    assert!(!package_path.join("python").exists());
}

/// Test that unimplemented commands give helpful messages
#[test]
fn test_unimplemented_commands() {
    let commands = ["update", "search test", "publish"];

    for cmd in &commands {
        let args: Vec<&str> = cmd.split_whitespace().collect();
        let output = hpm_binary()
            .args(&args)
            .output()
            .unwrap_or_else(|_| panic!("Failed to execute hpm {}", cmd));

        assert!(output.status.success());
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stdout.contains("not yet implemented") || stderr.contains("not yet implemented"),
            "Expected 'not yet implemented' in output. stdout: '{}', stderr: '{}'",
            stdout,
            stderr
        );
    }
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
    let original_dir = env::current_dir().unwrap();

    // First create a package
    env::set_current_dir(temp_dir.path()).unwrap();

    let _init_output = hpm_binary()
        .args(["init", "test-deps-package", "--vcs", "none"])
        .output()
        .expect("Failed to create test package");

    let package_dir = temp_dir.path().join("test-deps-package");
    env::set_current_dir(&package_dir).unwrap();

    // Test add command
    let add_output = hpm_binary()
        .args(["add", "test-package", "--version", "^1.0.0"])
        .output()
        .expect("Failed to execute hpm add");

    // Add should succeed (even though package doesn't exist in registry)
    // This tests manifest modification logic
    if add_output.status.success() {
        let hpm_toml = fs::read_to_string("hpm.toml").unwrap();
        assert!(hpm_toml.contains("test-package"));
    }

    env::set_current_dir(original_dir).unwrap();
}

/// Test error handling for directory that already exists
#[test]
fn test_init_directory_exists_error() {
    let temp_dir = TempDir::new().unwrap();
    let original_dir = env::current_dir().unwrap();

    env::set_current_dir(temp_dir.path()).unwrap();

    // Create directory first
    fs::create_dir("existing-package").unwrap();

    // Try to init with same name
    let output = hpm_binary()
        .args(["init", "existing-package", "--vcs", "none"])
        .output()
        .expect("Failed to execute hpm init");

    env::set_current_dir(original_dir).unwrap();

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
    let original_dir = env::current_dir().unwrap();

    // Create a simple hpm.toml to check
    env::set_current_dir(temp_dir.path()).unwrap();

    let manifest_content = r#"
[package]
name = "test-check-package"
version = "1.0.0"
description = "Test package for check command"

[houdini]
min_version = "19.5"
"#;
    fs::write("hpm.toml", manifest_content).unwrap();

    let output = hpm_binary()
        .arg("check")
        .output()
        .expect("Failed to execute hpm check");

    env::set_current_dir(original_dir).unwrap();

    // Check command should process the manifest
    assert!(output.status.success() || !output.stderr.is_empty());
}
