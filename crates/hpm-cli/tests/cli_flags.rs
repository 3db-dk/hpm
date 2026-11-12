//! Command-line surface: global flags parse on either side of every
//! subcommand, invalid enum values are refused by clap, `-C dir` means the
//! same as `-m dir`, and `hpm add name@version` records the bare name.

use clap::{CommandFactory, Parser};
use hpm_cli::Cli;
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Output};
use tempfile::TempDir;

/// One valid invocation of every subcommand.
const INVOCATIONS: &[&[&str]] = &[
    &["init", "demo", "--vcs", "none"],
    &["add", "acme/tools@1.0.0"],
    &["remove", "acme/tools"],
    &["update", "--dry-run"],
    &["list", "--tree"],
    &["search", "rig"],
    &["run", "build", "--", "x"],
    &["install", "--frozen-lockfile"],
    &["check", "-m", "pkg"],
    &["build", "-o", "dist", "--platform", "linux-x86_64"],
    &["pack", "--output", "dist", "--json"],
    &["audit"],
    &["clean", "--dry-run"],
    &["registry", "add", "https://example.com", "--type", "api"],
    &["registry", "list"],
    &["global", "add", "acme/tools", "--houdini", "21.0"],
    &["global", "list", "--houdini", "21.0.512"],
    &["completions", "zsh"],
];

const GLOBALS: &[&[&str]] = &[
    &["-v"],
    &["-vv"],
    &["-q"],
    &["--color", "never"],
    &["-C", "somewhere"],
];

#[test]
fn the_command_definition_is_consistent() {
    Cli::command().debug_assert();
}

/// Every global flag parses before and after every subcommand.
#[test]
fn global_flags_parse_on_either_side_of_every_subcommand() {
    for invocation in INVOCATIONS {
        for global in GLOBALS {
            let before: Vec<&str> = ["hpm"]
                .iter()
                .chain(global.iter())
                .chain(invocation.iter())
                .copied()
                .collect();
            let after: Vec<&str> = ["hpm"]
                .iter()
                .chain(invocation.iter())
                .chain(global.iter())
                .copied()
                .collect();
            // `run` forwards everything after its script name.
            for args in [
                before,
                if invocation[0] == "run" {
                    vec![]
                } else {
                    after
                },
            ] {
                if args.is_empty() {
                    continue;
                }
                if let Err(e) = Cli::try_parse_from(&args) {
                    panic!("{args:?}: {e}");
                }
            }
        }
    }
}

/// The form other repositories' CI runs keeps parsing: pack's own
/// `--output <dir>` and `--json`, and the global `--output` format before
/// the subcommand.
#[test]
fn pack_output_dir_and_json_keep_their_meaning() {
    Cli::try_parse_from(["hpm", "pack", "--output", "dist", "--json"]).unwrap();
    Cli::try_parse_from(["hpm", "--output", "json", "list"]).unwrap();
}

#[test]
fn invalid_values_are_refused_by_clap() {
    use clap::error::ErrorKind;
    for (args, kind) in [
        (
            &["hpm", "init", "x", "--vcs", "gti"][..],
            ErrorKind::InvalidValue,
        ),
        (
            &["hpm", "registry", "add", "u", "--type", "svn"],
            ErrorKind::ValueValidation,
        ),
        (
            &["hpm", "pack", "--platform", "linux-arm64"],
            ErrorKind::ValueValidation,
        ),
        (
            &["hpm", "build", "--platform", "macos"],
            ErrorKind::ValueValidation,
        ),
        (
            &["hpm", "global", "list", "--houdini", "twenty"],
            ErrorKind::ValueValidation,
        ),
        (
            &["hpm", "clean", "--python-only", "--comprehensive"],
            ErrorKind::ArgumentConflict,
        ),
    ] {
        match Cli::try_parse_from(args) {
            Ok(_) => panic!("{args:?} parsed"),
            Err(e) => assert_eq!(e.kind(), kind, "{args:?}: {e}"),
        }
    }
}

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

/// `hpm -C dir X` behaves as `hpm X -m dir` for every command that takes a
/// manifest, run from an unrelated directory.
#[test]
fn dash_c_matches_dash_m_for_every_manifest_command() {
    let home = TempDir::new().unwrap();
    let work = TempDir::new().unwrap();
    let pkg = work.path().join("pkg");
    std::fs::create_dir_all(pkg.join("otls")).unwrap();
    std::fs::write(pkg.join("otls/a.hda"), "x").unwrap();
    std::fs::write(
        pkg.join("hpm.toml"),
        "[package]\npath = \"acme/pkg\"\nname = \"pkg\"\nversion = \"1.0.0\"\ndescription = \"d\"\nauthors = [\"A\"]\nkeywords = [\"k\"]\nlicense = \"MIT\"\n\n[compat]\nhoudini = \"^21\"\n\n[scripts]\nhello = \"echo hello\"\n\n[stage]\ninclude = [\"otls/**\"]\n",
    )
    .unwrap();
    std::fs::write(pkg.join("LICENSE"), "MIT").unwrap();
    std::fs::write(pkg.join("README.md"), "# pkg").unwrap();
    let elsewhere = work.path().join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    let pkg_str = pkg.to_str().unwrap();
    let canonical = pkg.canonicalize().unwrap();
    let normalize = |out: &Output| {
        let text = |b: &[u8]| {
            String::from_utf8_lossy(b)
                .replace(canonical.to_str().unwrap(), "[PKG]")
                .replace(pkg_str, "[PKG]")
        };
        (out.status.code(), text(&out.stdout))
    };

    for command in [
        &["list"][..],
        &["--output", "json", "list"],
        &["check"],
        &["install"],
        &["audit"],
        &["run", "hello"],
        &["build", "-o", "OUT"],
        &["pack", "--output", "OUT", "--json"],
    ] {
        let out_dir = |label: &str| work.path().join(label);
        let fix = |label: &str| -> Vec<String> {
            command
                .iter()
                .map(|a| {
                    if *a == "OUT" {
                        out_dir(label).to_str().unwrap().to_string()
                    } else {
                        a.to_string()
                    }
                })
                .collect()
        };
        let c_args: Vec<String> = ["-C".to_string(), pkg_str.to_string()]
            .into_iter()
            .chain(fix("out"))
            .collect();
        let mut m_args = fix("out");
        // `-m` goes after the subcommand name (and `run`'s goes before the
        // script name, which takes trailing arguments).
        let sub = m_args
            .iter()
            .position(|a| !a.starts_with('-') && a != "json")
            .unwrap();
        m_args.splice(sub + 1..sub + 1, ["-m".to_string(), pkg_str.to_string()]);

        let c_args: Vec<&str> = c_args.iter().map(String::as_str).collect();
        let m_args: Vec<&str> = m_args.iter().map(String::as_str).collect();
        let via_c = hpm(home.path(), &elsewhere, &c_args);
        let _ = std::fs::remove_dir_all(out_dir("out"));
        let via_m = hpm(home.path(), &elsewhere, &m_args);
        let _ = std::fs::remove_dir_all(out_dir("out"));
        assert_eq!(
            normalize(&via_c),
            normalize(&via_m),
            "{c_args:?} vs {m_args:?}\n-C stderr: {}\n-m stderr: {}",
            String::from_utf8_lossy(&via_c.stderr),
            String::from_utf8_lossy(&via_m.stderr)
        );
        assert_eq!(
            via_c.status.code(),
            Some(0),
            "{c_args:?}: {}",
            String::from_utf8_lossy(&via_c.stderr)
        );
    }
}

/// Serve `body` as the reply to every HTTP request until the test ends.
fn serve_forever(body: String) -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut buf = [0u8; 4096];
            let _ = stream.read(&mut buf);
            let reply = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(reply.as_bytes());
        }
    });
    format!("http://{addr}")
}

/// `hpm add acme/tools@1.0.0` records the dependency under `acme/tools`,
/// not under the argument as typed. (The archive download that follows
/// fails against this fake registry; the manifest edit comes first.)
#[test]
fn add_records_the_name_without_the_version() {
    let url = serve_forever(
        r#"{"versions":[{"name":"acme/tools","vers":"1.0.0","dl":"http://127.0.0.1:9/none.zip"}],"builds":[{"name":"acme/tools","vers":"1.0.0","dl":"http://127.0.0.1:9/none.zip"}]}"#
            .to_string(),
    );
    let home = TempDir::new().unwrap();
    std::fs::create_dir_all(home.path().join(".hpm")).unwrap();
    std::fs::write(
        home.path().join(".hpm/config.toml"),
        format!("[[registries]]\nname = \"fake\"\nurl = \"{url}\"\ntype = \"api\"\n"),
    )
    .unwrap();
    let pkg = TempDir::new().unwrap();
    std::fs::write(
        pkg.path().join("hpm.toml"),
        "[package]\npath = \"acme/app\"\nname = \"app\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();

    let out = hpm(home.path(), pkg.path(), &["add", "acme/tools@1.0.0"]);
    let manifest = std::fs::read_to_string(pkg.path().join("hpm.toml")).unwrap();
    let parsed: toml::Value = toml::from_str(&manifest).unwrap();
    let deps = parsed["dependencies"]
        .as_table()
        .unwrap_or_else(|| panic!("{manifest}\n{}", String::from_utf8_lossy(&out.stderr)));
    assert_eq!(
        deps.keys().collect::<Vec<_>>(),
        ["acme/tools"],
        "{manifest}\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}
