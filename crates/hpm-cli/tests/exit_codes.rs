//! What failing checks report and how they exit: `hpm audit` on an intact
//! and a tampered install, `hpm registry update` with a registry it cannot
//! reach, and `hpm update` in JSON mode against a local fake registry.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

/// Serve `routes` (path suffix -> body) to every request until the test ends;
/// the first route whose suffix matches the request path answers.
fn serve(routes: Vec<(String, Vec<u8>)>) -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut buf = [0u8; 8192];
            let n = stream.read(&mut buf).unwrap_or(0);
            let request = String::from_utf8_lossy(&buf[..n]).into_owned();
            let path = request.split_whitespace().nth(1).unwrap_or("/").to_string();
            let reply = match routes
                .iter()
                .find(|(suffix, _)| path.ends_with(suffix.as_str()))
            {
                Some((_, body)) => {
                    let mut reply = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    )
                    .into_bytes();
                    reply.extend_from_slice(body);
                    reply
                }
                None => b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    .to_vec(),
            };
            let _ = stream.write_all(&reply);
        }
    });
    format!("http://{addr}")
}

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

    fn run(&self, cwd: &Path, args: &[&str]) -> String {
        let out = Command::new(env!("CARGO_BIN_EXE_hpm"))
            .args(args)
            .current_dir(cwd)
            .env("HOME", self.home.path())
            .env("USERPROFILE", self.home.path())
            .env_remove("RUST_LOG")
            .output()
            .unwrap();
        let text = format!(
            "args: {}\nexit: {}\n--- stdout\n{}--- stderr\n{}",
            args.join(" "),
            out.status.code().map_or("signal".into(), |c| c.to_string()),
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr),
        );
        let mut text = text;
        for (dir, label) in [(self.home.path(), "[HOME]"), (self.work.path(), "[WORK]")] {
            let canonical = dir.canonicalize().unwrap();
            for form in [
                canonical.to_string_lossy().into_owned(),
                dir.to_string_lossy().into_owned(),
            ] {
                text = text.replace(&form, label);
            }
        }
        text.replace('\\', "/")
    }

    fn write(&self, rel: &str, content: &str) -> PathBuf {
        let path = self.work.path().join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, content).unwrap();
        path
    }

    fn config(&self, content: &str) {
        std::fs::create_dir_all(self.home.path().join(".hpm")).unwrap();
        std::fs::write(self.home.path().join(".hpm/config.toml"), content).unwrap();
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
                (r"127\.0\.0\.1:\d+", "[ADDR]"),
            ],
            prepend_module_to_snapshot => false,
        }, {
            insta::assert_snapshot!($name, $value);
        });
    };
}

/// A packed `local/dep` archive, built with the binary.
fn packed_dep(env: &Env) -> Vec<u8> {
    let dep = env.work.path().join("dep");
    env.write("dep/otls/a.hda", "hda");
    env.write(
        "dep/hpm.toml",
        "[package]\npath = \"local/dep\"\nname = \"dep\"\nversion = \"1.0.0\"\ndescription = \"d\"\nauthors = [\"a\"]\nkeywords = [\"k\"]\n",
    );
    env.run(
        &dep,
        &[
            "pack",
            "--output",
            env.work.path().join("srv").to_str().unwrap(),
        ],
    );
    std::fs::read(env.work.path().join("srv/dep-1.0.0.zip")).unwrap()
}

/// `hpm audit` passes an intact install; a file changed in the store copy
/// is reported.
#[test]
fn audit_intact_and_tampered() {
    let env = Env::new();
    let url = serve(vec![("/dep-1.0.0.zip".into(), packed_dep(&env))]);
    let app = env.work.path().join("app");
    env.write(
        "app/hpm.toml",
        &format!(
            "[package]\npath = \"local/app\"\nname = \"app\"\nversion = \"0.1.0\"\n\n[dependencies]\n\"local/dep\" = {{ url = \"{url}/dep-1.0.0.zip\", version = \"1.0.0\" }}\n"
        ),
    );
    let mut report = env.run(&app, &["install"]);
    report += &env.run(&app, &["audit"]);

    let installed = env.home.path().join(".hpm/packages/dep@1.0.0/otls/a.hda");
    let mut perms = std::fs::metadata(&installed).unwrap().permissions();
    #[allow(clippy::permissions_set_readonly_false)]
    perms.set_readonly(false);
    std::fs::set_permissions(&installed, perms).unwrap();
    std::fs::write(&installed, "tampered").unwrap();
    report += &env.run(&app, &["audit"]);
    snapshot!("audit_intact_and_tampered", report);
}

/// A git registry that cannot be fetched is reported.
#[test]
fn registry_update_with_an_unreachable_registry() {
    let env = Env::new();
    let missing = env.work.path().join("no-such-repo.git");
    env.config(&format!(
        "[[registries]]\nname = \"broken\"\nurl = \"{}\"\ntype = \"git\"\n",
        missing.to_string_lossy().replace('\\', "/")
    ));
    snapshot!(
        "registry_update_with_an_unreachable_registry",
        env.run(env.work.path(), &["registry", "update"])
    );
}

fn update_project(env: &Env) -> PathBuf {
    let versions = r#"{"versions":[{"name":"acme/tools","vers":"1.0.0","dl":"http://127.0.0.1:9/a.zip"},{"name":"acme/tools","vers":"1.2.0","dl":"http://127.0.0.1:9/b.zip"}]}"#;
    let url = serve(vec![(
        "/packages/acme/tools".into(),
        versions.as_bytes().to_vec(),
    )]);
    env.config(&format!(
        "[[registries]]\nname = \"fake\"\nurl = \"{url}\"\ntype = \"api\"\n"
    ));
    let app = env.work.path().join("app");
    env.write(
        "app/hpm.toml",
        "[package]\npath = \"local/app\"\nname = \"app\"\nversion = \"0.1.0\"\n\n[dependencies]\n\"acme/tools\" = \"1.0.0\"\n",
    );
    env.write(
        "app/hpm.lock",
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\n\n[dependencies.\"acme/tools\"]\nversion = \"1.0.0\"\n\n[dependencies.\"acme/tools\".source]\ntype = \"url\"\nurl = \"http://127.0.0.1:9/a.zip\"\nversion = \"1.0.0\"\n",
    );
    app
}

/// `hpm --output json update --dry-run` reports the available update and
/// changes nothing.
#[test]
fn update_dry_run_in_json() {
    let env = Env::new();
    let app = update_project(&env);
    let before = std::fs::read_to_string(app.join("hpm.toml")).unwrap();
    let report = env.run(&app, &["--output", "json", "update", "--dry-run"]);
    assert_eq!(
        std::fs::read_to_string(app.join("hpm.toml")).unwrap(),
        before
    );
    snapshot!("update_dry_run_in_json", report);
}
