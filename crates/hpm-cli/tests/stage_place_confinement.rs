//! A `[stage]` place rule decides where a file lands inside an install
//! image; it must never decide where a file lands on the machine.

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

/// A path dependency's place rule cannot make `hpm install` write outside
/// the dev copy, whether `to` is absolute or climbs out with `..`.
#[test]
fn a_dependency_place_rule_cannot_write_outside_the_dev_copy() {
    let platform = hpm_package::Platform::current().expect("host platform");
    let home = TempDir::new().unwrap();
    let work = TempDir::new().unwrap();
    let escaped_abs = work.path().join("escaped-abs.txt");
    let escaped_rel = work.path().join("escaped-rel.txt");
    let climb = format!(
        "{}{}",
        "../".repeat(40),
        escaped_rel
            .to_str()
            .unwrap()
            .replace('\\', "/")
            .trim_start_matches('/')
    );

    for (label, to, escaped) in [
        (
            "absolute",
            escaped_abs.to_str().unwrap().replace('\\', "/"),
            &escaped_abs,
        ),
        ("climbing", climb, &escaped_rel),
    ] {
        let project = work.path().join(format!("project-{label}"));
        std::fs::create_dir_all(project.join("sub/payload")).unwrap();
        std::fs::write(project.join("sub/payload/x.txt"), "attacker bytes").unwrap();
        std::fs::write(
            project.join("hpm.toml"),
            "[package]\npath = \"studio/project\"\nname = \"Project\"\nversion = \"0.1.0\"\n\n[dependencies]\n\"studio/sub\" = { path = \"sub\" }\n",
        )
        .unwrap();
        std::fs::write(
            project.join("sub/hpm.toml"),
            format!(
                "[package]\npath = \"studio/sub\"\nname = \"Sub\"\nversion = \"0.1.0\"\n\n[compat]\nplatforms = [\"{platform}\"]\n\n[[stage.platform.{platform}.place]]\nfrom = \"payload/x.txt\"\nto = '{to}'\n"
            ),
        )
        .unwrap();

        let out = hpm(home.path(), &project, &["install"]);
        assert!(
            !escaped.exists(),
            "{label}: hpm install wrote outside the dev copy; stderr: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}
