//! `hpm install` against a project's `hpm.lock`.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Output};
use tempfile::TempDir;

const MANIFEST: &str = "[package]\npath = \"studio/proj\"\nname = \"proj\"\nversion = \"0.1.0\"\n\n[dependencies]\n\"acme/tools\" = { url = \"https://example.invalid/tools.zip\", version = \"1.0.0\" }\n";
const TOOLS_MANIFEST: &str =
    "[package]\npath = \"acme/tools\"\nname = \"Tools\"\nversion = \"1.0.0\"\n";

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

/// A project whose dependency's archive sits in the user's download cache
/// (so nothing touches the network), with an hpm.lock holding `checksum`.
fn project(checksum: Option<&str>) -> (TempDir, TempDir, std::path::PathBuf) {
    let home = TempDir::new().unwrap();
    let work = TempDir::new().unwrap();
    let cache = home.path().join(".hpm/cache");
    std::fs::create_dir_all(&cache).unwrap();
    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut buf);
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("tools/hpm.toml", options).unwrap();
        zip.write_all(TOOLS_MANIFEST.as_bytes()).unwrap();
        zip.start_file("tools/data.txt", options).unwrap();
        zip.write_all(b"release bytes").unwrap();
        zip.finish().unwrap();
    }
    std::fs::write(cache.join("acme-tools-1.0.0"), buf.into_inner()).unwrap();

    let project = work.path().join("proj");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(project.join("hpm.toml"), MANIFEST).unwrap();
    let checksum_line = checksum
        .map(|c| format!("checksum = \"{c}\"\n"))
        .unwrap_or_default();
    std::fs::write(
        project.join("hpm.lock"),
        format!(
            "version = 1\n\n[package]\nname = \"proj\"\nversion = \"0.1.0\"\n\n[dependencies.\"acme/tools\"]\nversion = \"1.0.0\"\n{checksum_line}\n[dependencies.\"acme/tools\".source]\ntype = \"url\"\nurl = \"https://example.invalid/tools.zip\"\nversion = \"1.0.0\"\n"
        ),
    )
    .unwrap();
    (home, work, project)
}

/// The tree checksum of the cached archive's contents, as `hpm install`
/// records it.
fn release_checksum() -> String {
    let (home, _work, project) = project(None);
    let out = hpm(home.path(), &project, &["install"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let lock = std::fs::read_to_string(project.join("hpm.lock")).unwrap();
    lock.lines()
        .find_map(|l| l.strip_prefix("checksum = \""))
        .map(|c| c.trim_end_matches('"').to_string())
        .expect("lock records a checksum")
}

/// A committed lock installs on a machine whose store has never held the
/// package.
#[test]
fn a_locked_project_installs_on_a_fresh_machine() {
    let checksum = release_checksum();
    let (home, _work, project) = project(Some(&checksum));
    let out = hpm(home.path(), &project, &["install"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = hpm(home.path(), &project, &["install", "--frozen-lockfile"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// `--frozen-lockfile` refuses a lock entry that pins no checksum: frozen
/// installs exist to reproduce exact bytes.
#[test]
fn frozen_lockfile_requires_every_package_to_carry_a_checksum() {
    let (home, _work, project) = project(None);
    let out = hpm(home.path(), &project, &["install", "--frozen-lockfile"]);
    assert!(
        !out.status.success(),
        "--frozen-lockfile installed a package the lock pins no checksum for"
    );
}

/// A lock that exists but cannot be read is never silently replaced: the
/// install stops instead of installing unverified and rewriting it.
#[test]
fn an_unreadable_lock_stops_the_install() {
    let (home, _work, project) = project(None);
    std::fs::write(project.join("hpm.lock"), "this is [not a lock").unwrap();
    let out = hpm(home.path(), &project, &["install"]);
    assert!(
        !out.status.success(),
        "installed past an unreadable lock: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(project.join("hpm.lock")).unwrap(),
        "this is [not a lock",
        "the unreadable lock was overwritten"
    );
}
