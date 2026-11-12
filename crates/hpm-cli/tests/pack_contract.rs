//! What `hpm pack` ships and refuses, through the binary: where the bundled
//! `{slug}.json` lands for an authored, generated or case-differing
//! descriptor, and each gate that stops a pack before or after archiving.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::TempDir;

const MANIFEST: &str = "[package]\npath = \"acme/packme\"\nname = \"Packme\"\nversion = \"0.4.0\"\ndescription = \"d\"\nauthors = [\"Ada\"]\n\n[compat]\nhoudini = \"^21\"\n";

struct Pkg {
    home: TempDir,
    work: TempDir,
}

impl Pkg {
    fn new(manifest: &str) -> Self {
        let pkg = Pkg {
            home: TempDir::new().unwrap(),
            work: TempDir::new().unwrap(),
        };
        std::fs::create_dir_all(pkg.dir().join("otls")).unwrap();
        std::fs::write(pkg.dir().join("otls/packme.hda"), "hda").unwrap();
        std::fs::write(pkg.dir().join("hpm.toml"), manifest).unwrap();
        pkg
    }

    fn dir(&self) -> PathBuf {
        self.work.path().join("packme")
    }

    fn dist(&self) -> PathBuf {
        self.work.path().join("dist")
    }

    fn write(&self, rel: &str, content: &str) {
        let path = self.dir().join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    fn pack(&self, extra: &[&str]) -> Output {
        let dist = self.dist();
        let mut args = vec!["pack", "--output", dist.to_str().unwrap()];
        args.extend_from_slice(extra);
        Command::new(env!("CARGO_BIN_EXE_hpm"))
            .args(&args)
            .current_dir(self.dir())
            .env("HOME", self.home.path())
            .env("USERPROFILE", self.home.path())
            .env_remove("RUST_LOG")
            .env_remove("HPM_SIGNING_KEY")
            .output()
            .unwrap()
    }

    fn archives(&self) -> Vec<PathBuf> {
        match std::fs::read_dir(self.dist()) {
            Ok(entries) => entries.map(|e| e.unwrap().path()).collect(),
            Err(_) => Vec::new(),
        }
    }
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// Every entry name and its bytes.
fn entries(archive: &Path) -> Vec<(String, Vec<u8>)> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(archive).unwrap()).unwrap();
    (0..zip.len())
        .map(|i| {
            let mut file = zip.by_index(i).unwrap();
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes).unwrap();
            (file.name().unwrap().into_owned(), bytes)
        })
        .collect()
}

fn packed(pkg: &Pkg, extra: &[&str]) -> Vec<(String, Vec<u8>)> {
    let out = pkg.pack(extra);
    assert!(out.status.success(), "{}", stderr(&out));
    let archives = pkg.archives();
    assert_eq!(archives.len(), 1, "{archives:?}");
    entries(&archives[0])
}

fn descriptors(entries: &[(String, Vec<u8>)]) -> Vec<&(String, Vec<u8>)> {
    entries
        .iter()
        .filter(|(name, _)| name.rsplit('/').next() == Some("packme.json"))
        .collect()
}

/// Exactly one `packme.json`, at the archive root, whether generated or
/// authored; an authored one ships byte-identical.
#[test]
fn one_descriptor_at_the_root_generated_or_authored() {
    let generated = Pkg::new(MANIFEST);
    let entries = packed(&generated, &[]);
    let found = descriptors(&entries);
    assert_eq!(
        found.len(),
        1,
        "{:?}",
        entries.iter().map(|e| &e.0).collect::<Vec<_>>()
    );
    assert_eq!(found[0].0, "packme.json");
    let json: serde_json::Value = serde_json::from_slice(&found[0].1).unwrap();
    assert!(json.is_object());
    assert!(entries.iter().any(|(n, _)| n == "packme/otls/packme.hda"));

    let authored = Pkg::new(MANIFEST);
    let text = "{\n  \"hpath\": \"$HOUDINI_PACKAGE_PATH/packme/otls\", \"note\": \"by hand\"\n}\n";
    authored.write("packme.json", text);
    let entries = packed(&authored, &[]);
    let found = descriptors(&entries);
    assert_eq!(
        found.len(),
        1,
        "{:?}",
        entries.iter().map(|e| &e.0).collect::<Vec<_>>()
    );
    assert_eq!(found[0].0, "packme.json");
    assert_eq!(found[0].1, text.as_bytes());
}

/// A descriptor whose name differs only by case is not the descriptor: the
/// pack generates one, warns, and ships the other file as content.
#[test]
fn a_case_differing_descriptor_is_not_shipped_as_the_descriptor() {
    let pkg = Pkg::new(MANIFEST);
    pkg.write("Packme.json", "{\"by\": \"hand\"}");
    let out = pkg.pack(&[]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("Ignoring Packme.json"),
        "{}",
        stderr(&out)
    );
    let entries = entries(&pkg.archives()[0]);
    let root = entries.iter().find(|(n, _)| n == "packme.json").unwrap();
    assert_ne!(root.1, b"{\"by\": \"hand\"}");
}

/// A gate case: label, manifest, extra arguments, an extra file to write,
/// and the message the refusal must carry.
type GateCase<'a> = (
    &'a str,
    String,
    Vec<&'a str>,
    Option<(&'a str, &'a str)>,
    &'a str,
);

/// Each gate refuses the pack with its message and leaves no archive.
#[test]
fn gates_refuse_the_pack_and_leave_no_archive() {
    let operators = "\n[[operators]]\nkind = \"hda\"\ntype_name = \"acme::packme::1.0\"\ncategory = \"Sop\"\nsource = \"otls/absent.hda\"\n";
    let platforms = MANIFEST.replace(
        "houdini = \"^21\"\n",
        "houdini = \"^21\"\nplatforms = [\"linux-x86_64\", \"macos-aarch64\", \"macos-x86_64\", \"windows-x86_64\", \"linux-aarch64\", \"windows-aarch64\"]\n",
    );
    // One declared platform that is not this host.
    let not_host = if (std::env::consts::OS, std::env::consts::ARCH) == ("linux", "aarch64") {
        "windows-aarch64"
    } else {
        "linux-aarch64"
    };
    let only_other = MANIFEST.replace(
        "houdini = \"^21\"\n",
        &format!("houdini = \"^21\"\nplatforms = [\"{not_host}\"]\n"),
    );
    // hpackage accepts a single lower bound only, so the --sidefx cases use
    // one to reach the gate each is about.
    let sidefx = MANIFEST.replace("houdini = \"^21\"", "houdini = \">=21.0\"");
    let cases: Vec<GateCase> = vec![
        (
            "invalid package",
            MANIFEST.replace("version = \"0.4.0\"", "version = \"x\""),
            vec![],
            None,
            "Package validation failed",
        ),
        (
            "platform without platforms",
            MANIFEST.to_string(),
            vec!["--platform", "linux-x86_64"],
            None,
            "has no [compat].platforms",
        ),
        (
            "unknown platform",
            platforms.clone(),
            vec!["--platform", "linux-arm64"],
            None,
            "unknown platform 'linux-arm64'",
        ),
        (
            "undeclared platform",
            only_other.clone(),
            vec!["--platform", "macos-x86_64"],
            None,
            "is not declared in [compat].platforms",
        ),
        (
            "host platform undeclared",
            only_other.clone(),
            vec![],
            None,
            "is not declared in [compat].platforms",
        ),
        (
            "sidefx without README.md",
            sidefx.clone(),
            vec!["--sidefx"],
            None,
            "requires a README.md",
        ),
        (
            "sidefx with readme.md",
            sidefx.clone(),
            vec!["--sidefx"],
            Some(("readme.md", "# x")),
            "this package has readme.md",
        ),
        (
            "sidefx placeholder",
            sidefx.clone(),
            vec!["--sidefx"],
            Some(("README.md", "<enter a one-line description>")),
            "placeholder line",
        ),
        (
            "sidefx authored descriptor refused",
            sidefx.clone(),
            vec!["--sidefx"],
            Some(("packme.json", "{}")),
            "packme.json declares no `enable`",
        ),
        (
            "sidefx bounded range",
            MANIFEST.to_string(),
            vec!["--sidefx"],
            Some(("README.md", "# Packme\n\nA package.\n")),
            "compiles to 2 clauses",
        ),
        (
            "verify-assets missing source",
            format!("{MANIFEST}{operators}"),
            vec!["--verify-assets"],
            None,
            "The archive was removed",
        ),
    ];
    for (label, manifest, args, extra_file, message) in cases {
        let pkg = Pkg::new(&manifest);
        if let Some((rel, content)) = extra_file {
            pkg.write(rel, content);
        }
        if label == "sidefx authored descriptor refused" {
            pkg.write("README.md", "# Packme\n\nA package.\n");
        }
        let out = pkg.pack(&args);
        assert!(!out.status.success(), "{label}: packed");
        assert!(stderr(&out).contains(message), "{label}: {}", stderr(&out));
        assert!(
            pkg.archives().is_empty(),
            "{label}: left {:?}",
            pkg.archives()
        );
    }
}

/// Without `--verify-assets` a missing operator source is a warning and the
/// archive stays.
#[test]
fn a_missing_operator_source_warns_without_verify_assets() {
    let pkg = Pkg::new(&format!(
        "{MANIFEST}\n[[operators]]\nkind = \"hda\"\ntype_name = \"acme::packme::1.0\"\ncategory = \"Sop\"\nsource = \"otls/absent.hda\"\n"
    ));
    let out = pkg.pack(&["--json"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("Declared operator source not found in archive: otls/absent.hda")
    );
    assert_eq!(pkg.archives().len(), 1);
}
