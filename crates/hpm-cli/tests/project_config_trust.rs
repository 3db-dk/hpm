//! A project's `.hpm/config.toml` is content of whatever repository the
//! user cloned: it must not be able to choose where hpm keeps its own
//! state, tools or signing key.

#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Output};
use tempfile::TempDir;

fn hpm(home: &Path, cwd: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_hpm"))
        .args(args)
        .current_dir(cwd)
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env_remove("RUST_LOG")
        .output()
        .unwrap()
}

/// Where hpm keeps the pinned uv under a home directory. Bump with
/// `UV_VERSION` in hpm-core's `python/bundled.rs`.
const UV_DIR: &str = "tools/uv-0.13.6/uv";

fn stub(path: &Path, marker: &Path) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        path,
        format!("#!/bin/sh\ntouch '{}'\nexit 1\n", marker.display()),
    )
    .unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// A project that moves hpm's home into its own tree and ships a `uv`
/// there does not get that file run by `hpm install`.
#[test]
fn a_project_config_cannot_supply_the_uv_hpm_runs() {
    let home = TempDir::new().unwrap();
    let work = TempDir::new().unwrap();
    let project = work.path().join("project");

    // The user's own hpm home already holds a uv; a stub that records that
    // it ran keeps the test offline.
    let user_marker = work.path().join("user-uv-ran");
    stub(&home.path().join(".hpm").join(UV_DIR), &user_marker);

    let planted_marker = work.path().join("planted-uv-ran");
    stub(&project.join("state").join(UV_DIR), &planted_marker);
    std::fs::create_dir_all(project.join(".hpm")).unwrap();
    std::fs::write(
        project.join(".hpm/config.toml"),
        "[storage]\nhome_dir = \"state\"\n",
    )
    .unwrap();
    std::fs::write(
        project.join("hpm.toml"),
        "[package]\npath = \"studio/project\"\nname = \"Project\"\nversion = \"0.1.0\"\n\n[dependencies]\n\"studio/sub\" = { path = \"sub\" }\n",
    )
    .unwrap();
    std::fs::create_dir_all(project.join("sub")).unwrap();
    std::fs::write(
        project.join("sub/hpm.toml"),
        "[package]\npath = \"studio/sub\"\nname = \"Sub\"\nversion = \"0.1.0\"\n\n[python_dependencies]\nnumpy = \">=1.20\"\n",
    )
    .unwrap();

    let out = hpm(home.path(), &project, &["install"]);
    assert!(
        !planted_marker.exists(),
        "hpm ran a uv from the project tree; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        user_marker.exists(),
        "the user's uv was not reached; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}
