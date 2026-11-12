//! Characterization snapshots of CLI output that users and scripts read:
//! human and JSON renderings, the JSON error document, `pack --json`,
//! `clean --dry-run`, registry and global management, and what `init`
//! writes. HOME is isolated per test; temp paths, timestamps, ANSI colour
//! codes, timings and archive hashes are masked.

use std::path::Path;
use std::process::{Command, Output};
use tempfile::TempDir;

const FIXTURE_MANIFEST: &str = r#"[package]
path = "acme/demo"
name = "Demo"
version = "1.2.3"
description = "A fixture package"
authors = ["Ada <ada@example.com>"]
license = "MIT"

[compat]
houdini = ">=20.5, <22"

[dependencies]
"acme/tools" = "1.0.0"
"acme/rig" = { version = "2.0.0", registry = "studio", optional = true }
"other/local" = { path = "../local", link = true }
"other/remote" = { url = "https://example.com/remote-1.0.0.zip", version = "1.0.0" }

[python_dependencies]
numpy = ">=1.24"
requests = { version = "^2.31", extras = ["socks"] }
"#;

struct Env {
    home: TempDir,
    work: TempDir,
}

impl Env {
    fn new() -> Self {
        Env {
            home: TempDir::new().unwrap(),
            work: TempDir::new().unwrap(),
        }
    }

    fn project(&self) -> std::path::PathBuf {
        let dir = self.work.path().join("demo");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("hpm.toml"), FIXTURE_MANIFEST).unwrap();
        dir
    }

    fn hpm(&self, cwd: &Path, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_hpm"))
            .args(args)
            .current_dir(cwd)
            .env("HOME", self.home.path())
            .env("USERPROFILE", self.home.path())
            .env_remove("RUST_LOG")
            .env_remove("HPM_SIGNING_KEY")
            .output()
            .unwrap()
    }

    /// Render an invocation as exit code, stdout and stderr, with the
    /// volatile parts masked.
    fn run(&self, cwd: &Path, args: &[&str]) -> String {
        let out = self.hpm(cwd, args);
        let text = format!(
            "args: {}\nexit: {}\n--- stdout\n{}--- stderr\n{}",
            args.join(" "),
            out.status.code().map_or("signal".into(), |c| c.to_string()),
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr),
        );
        self.mask(&text)
    }

    fn mask(&self, text: &str) -> String {
        let mut text = text.to_string();
        for (dir, label) in [(self.home.path(), "[HOME]"), (self.work.path(), "[WORK]")] {
            // macOS temp dirs are reached through /var and /private/var.
            let canonical = dir.canonicalize().unwrap();
            for form in [
                canonical.to_string_lossy().into_owned(),
                dir.to_string_lossy().into_owned(),
            ] {
                text = text.replace(&form, label);
                text = text.replace(&form.replace('\\', "\\\\"), label);
            }
        }
        // JSON escapes a Windows separator as `\\`; both forms become `/`.
        text.replace("\\\\", "/").replace('\\', "/")
    }
}

macro_rules! snapshot {
    ($name:expr, $value:expr) => {
        insta::with_settings!({
            filters => vec![
                (r"\x1b\[[0-9;]*m", ""),
                (r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d+Z", "[TIME]"),
                (r#""elapsed_ms": ?\d+"#, r#""elapsed_ms": [MS]"#),
                (r"\b[0-9a-f]{64}\b", "[SHA256]"),
            ],
            prepend_module_to_snapshot => false,
        }, {
            insta::assert_snapshot!($name, $value);
        });
    };
}

#[test]
fn list_human() {
    let env = Env::new();
    let dir = env.project();
    snapshot!("list_human", env.run(&dir, &["list"]));
}

#[test]
fn list_json() {
    let env = Env::new();
    let dir = env.project();
    snapshot!("list_json", env.run(&dir, &["--output", "json", "list"]));
}

#[test]
fn check_json_compact() {
    let env = Env::new();
    let dir = env.project();
    for sub in ["otls", "python"] {
        std::fs::create_dir_all(dir.join(sub)).unwrap();
    }
    std::fs::write(dir.join("otls/demo.hda"), "x").unwrap();
    snapshot!(
        "check_json_compact",
        env.run(&dir, &["--output", "json-compact", "check"])
    );
}

/// `hpm check` across layouts that reach every finding: no manifest, a
/// manifest that does not parse, and a package with a missing README, an
/// empty `otls`, git without `.gitignore`, scripts, and large files (one of
/// them ignored by `.gitignore`).
#[test]
fn check_findings_by_layout() {
    let env = Env::new();
    let mut report = String::new();

    let empty = env.work.path().join("empty");
    std::fs::create_dir_all(&empty).unwrap();
    report += &env.run(&empty, &["--output", "json-compact", "check"]);

    let broken = env.work.path().join("broken");
    std::fs::create_dir_all(&broken).unwrap();
    std::fs::write(broken.join("hpm.toml"), "[package\n").unwrap();
    report += &env.run(&broken, &["--output", "json-compact", "check"]);

    let full = env.work.path().join("full");
    for sub in ["otls", "scripts", ".git", "build"] {
        std::fs::create_dir_all(full.join(sub)).unwrap();
    }
    std::fs::write(
        full.join("hpm.toml"),
        r#"[package]
path = "acme/full"
name = "Full"
version = "1.0.0"
description = "Every finding"
license = "MIT"
readme = "docs/README.md"
keywords = ["x"]

[compat]
houdini = ">=21"
platforms = ["linux-x86_64"]

[scripts]
build = "make"

[scripts.setup]
cmd = "python setup.py"
label = "Set up"
description = "Prepare the workspace"
"#,
    )
    .unwrap();
    std::fs::write(full.join("LICENSE"), "MIT").unwrap();
    let big = std::fs::File::create(full.join("scripts/big.bin")).unwrap();
    big.set_len(11 * 1024 * 1024).unwrap();
    report += &env.run(&full, &["--output", "json-compact", "check"]);
    report += &env.run(&full, &["check"]);

    std::fs::write(full.join(".gitignore"), "build/\n").unwrap();
    let ignored = std::fs::File::create(full.join("build/huge.bin")).unwrap();
    ignored.set_len(120 * 1024 * 1024).unwrap();
    std::fs::create_dir_all(full.join("docs")).unwrap();
    std::fs::write(full.join("docs/README.md"), "# Full").unwrap();
    report += &env.run(&full, &["--output", "json-compact", "check"]);
    report += &env.run(&full, &["--quiet", "check"]);

    snapshot!("check_findings_by_layout", report);
}

#[test]
fn search_json_without_registries() {
    let env = Env::new();
    snapshot!(
        "search_json_without_registries",
        env.run(env.work.path(), &["--output", "json", "search", "rig"])
    );
}

#[test]
fn error_document_json() {
    let env = Env::new();
    snapshot!(
        "error_document_json",
        env.run(env.work.path(), &["--output", "json", "list"])
    );
}

#[test]
fn pack_json() {
    let env = Env::new();
    let dir = env.work.path().join("packme");
    std::fs::create_dir_all(dir.join("otls")).unwrap();
    std::fs::write(
        dir.join("hpm.toml"),
        "[package]\npath = \"acme/packme\"\nname = \"Packme\"\nversion = \"0.4.0\"\ndescription = \"d\"\nauthors = [\"Ada\"]\n\n[compat]\nhoudini = \"^21\"\n\n[[operators]]\nkind = \"hda\"\ntype_name = \"acme::packme::1.0\"\ncategory = \"Sop\"\nsource = \"otls/packme.hda\"\n",
    )
    .unwrap();
    std::fs::write(dir.join("otls/packme.hda"), "hda bytes").unwrap();
    let dist = env.work.path().join("dist");
    snapshot!(
        "pack_json",
        env.run(
            &dir,
            &["pack", "--output", dist.to_str().unwrap(), "--json"]
        )
    );
}

#[test]
fn clean_dry_run() {
    let env = Env::new();
    snapshot!(
        "clean_dry_run",
        env.run(env.work.path(), &["clean", "--dry-run"])
    );
}

#[test]
fn registry_add_list_remove() {
    let env = Env::new();
    let w = env.work.path();
    let transcript = [
        env.run(
            w,
            &[
                "registry",
                "add",
                "https://example.com/registry",
                "--name",
                "studio",
                "--type",
                "api",
            ],
        ),
        env.run(w, &["registry", "list"]),
        env.run(w, &["registry", "remove", "studio"]),
        env.run(w, &["registry", "list"]),
    ]
    .join("\n");
    snapshot!("registry_add_list_remove", transcript);
}

#[test]
fn global_list_empty() {
    let env = Env::new();
    let w = env.work.path();
    let transcript = [
        env.run(w, &["global", "list", "--houdini", "21.0"]),
        env.run(
            w,
            &["--output", "json", "global", "list", "--houdini", "21.0"],
        ),
    ]
    .join("\n");
    snapshot!("global_list_empty", transcript);
}

/// What `init` writes. The tree printout's order follows `read_dir` and is
/// not stable across platforms, so the created paths are listed sorted and
/// the file contents are snapshotted instead.
#[test]
fn init_writes() {
    let env = Env::new();
    let w = env.work.path();
    let out = env.hpm(w, &["init", "demo-pkg", "--vcs", "none"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let root = w.join("demo-pkg");
    let mut paths: Vec<String> = walk(&root)
        .into_iter()
        .map(|p| {
            p.strip_prefix(&root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();
    paths.sort();
    let mut report = format!(
        "stdout:\n{}\npaths:\n{}\n",
        String::from_utf8_lossy(&out.stdout),
        paths.join("\n")
    );
    for file in [
        "hpm.toml",
        "package.json",
        "README.md",
        ".gitignore",
        "python/__init__.py",
    ] {
        let path = root.join(file);
        if path.exists() {
            report += &format!("\n--- {file}\n{}", std::fs::read_to_string(path).unwrap());
        }
    }
    snapshot!("init_writes", env.mask(&report));
}

fn walk(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        out.push(path.clone());
        if path.is_dir() {
            out.extend(walk(&path));
        }
    }
    out
}
