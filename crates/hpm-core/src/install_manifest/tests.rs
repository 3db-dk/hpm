use super::*;
use proptest::prelude::*;
use std::collections::BTreeMap;
use tempfile::TempDir;

fn write(path: &Path, bytes: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

/// A package directory named like a store install, so its record has a
/// sibling path.
fn package_dir(tmp: &TempDir) -> PathBuf {
    let dir = tmp.path().join("tool@1.0.0");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn a_tree_checked_against_its_own_record_is_intact() {
    let tmp = TempDir::new().unwrap();
    let dir = package_dir(&tmp);
    write(&dir.join("hpm.toml"), b"[package]");
    write(&dir.join("bin/tool"), b"\x7fELF...");
    let manifest = record_tree(&dir).unwrap();
    assert_eq!(
        manifest
            .files
            .iter()
            .map(|f| f.path.as_str())
            .collect::<Vec<_>>(),
        ["bin/tool", "hpm.toml"]
    );
    assert!(check(&dir, &manifest).is_intact());
}

#[test]
fn the_record_round_trips_beside_the_package_and_is_forgotten() {
    let tmp = TempDir::new().unwrap();
    let dir = package_dir(&tmp);
    write(&dir.join("a.txt"), b"a");
    let manifest = record_tree(&dir).unwrap();

    assert_eq!(verify(&dir).unwrap(), Verification::Unrecorded);
    super::write(&dir, &manifest).unwrap();
    let sibling = tmp.path().join("tool@1.0.0.files.json");
    assert!(sibling.is_file(), "the record lives next to the package");
    assert!(
        !dir.join("tool@1.0.0.files.json").exists(),
        "and never inside it"
    );
    assert_eq!(read(&dir).unwrap(), Some(manifest.clone()));

    // A second write replaces the first (Windows' rename does not).
    super::write(&dir, &manifest).unwrap();
    assert_eq!(read(&dir).unwrap(), Some(manifest));

    forget(&dir);
    assert!(!sibling.exists());
    assert_eq!(verify(&dir).unwrap(), Verification::Unrecorded);
}

#[test]
fn a_record_naming_a_path_outside_the_package_is_refused() {
    for bad in ["../escape", "/etc/passwd", "a/../../b", "", "a\\b", "./a"] {
        let tmp = TempDir::new().unwrap();
        let dir = package_dir(&tmp);
        let manifest = InstallManifest {
            format: FORMAT,
            files: vec![FileRecord {
                path: bad.to_string(),
                size: 0,
                sha256: String::new(),
            }],
        };
        super::write(&dir, &manifest).unwrap();
        assert!(read(&dir).is_err(), "{bad:?} must be refused");
    }
}

#[test]
fn a_record_from_a_newer_hpm_is_refused_not_misread() {
    let tmp = TempDir::new().unwrap();
    let dir = package_dir(&tmp);
    super::write(
        &dir,
        &InstallManifest {
            format: FORMAT + 1,
            files: vec![],
        },
    )
    .unwrap();
    assert!(read(&dir).is_err());
}

#[test]
fn a_garbled_record_is_an_error_not_unrecorded() {
    let tmp = TempDir::new().unwrap();
    let dir = package_dir(&tmp);
    std::fs::write(record_path(&dir).unwrap(), b"{not json").unwrap();
    assert!(verify(&dir).is_err());
}

// ---------------------------------------------------------------------------
// Property: exactly the recorded files whose content changed are flagged.
// ---------------------------------------------------------------------------

/// Paths a package may install. Drawn from one small pool so the initial
/// tree, the mutations and the stray files a session writes actually land on
/// the same paths — independent random paths would almost never collide, and
/// the "untouched / extra files are never flagged" half would hold trivially.
const POOL: &[&str] = &[
    "hpm.toml",
    "bin/tool",
    "bin/tool.exe",
    "python/pkg/__init__.py",
    "python/pkg/mod.py",
    "otls/node.hda",
    "deep/a/b/c.bin",
];

/// Files Houdini and Python write into a package while it is used, plus pool
/// paths: a pool path the install did not record is just as "extra".
const STRAYS: &[&str] = &[
    "python/pkg/__pycache__/mod.cpython-311.pyc",
    "__pycache__/x.pyc",
    "otls/backup/node_bak1.hda",
    "bin/tool",
    "hpm.toml",
    "deep/a/new.txt",
];

#[derive(Debug, Clone)]
enum Mutation {
    Keep,
    Delete,
    /// Truncate to `n % len` bytes (only applied to non-empty files).
    Truncate(usize),
    Append(Vec<u8>),
    /// Same length, at least one byte different (only for non-empty files).
    FlipByte(usize),
    /// Write the same bytes back: touches the mtime, changes nothing.
    RewriteIdentical,
    /// Widen the mode (Unix; nothing to widen on Windows): not content, so
    /// not a problem. Kept on every platform so the strategy below needs no
    /// `cfg` inside `prop_oneof!`, which does not expand one.
    Chmod,
    /// Replace the file with a directory of the same name.
    BecomeDir,
}

fn mutation() -> impl Strategy<Value = Mutation> {
    prop_oneof![
        Just(Mutation::Keep),
        Just(Mutation::Delete),
        any::<usize>().prop_map(Mutation::Truncate),
        prop::collection::vec(any::<u8>(), 1..8).prop_map(Mutation::Append),
        any::<usize>().prop_map(Mutation::FlipByte),
        Just(Mutation::RewriteIdentical),
        Just(Mutation::Chmod),
        Just(Mutation::BecomeDir),
    ]
}

fn tree() -> impl Strategy<Value = BTreeMap<&'static str, Vec<u8>>> {
    prop::collection::btree_map(
        prop::sample::select(POOL),
        prop::collection::vec(any::<u8>(), 0..48),
        1..POOL.len(),
    )
}

/// Apply `m` to the file at `rel` (recorded with `bytes`), returning what a
/// correct check must report for it.
fn apply(dir: &Path, rel: &str, bytes: &[u8], m: &Mutation) -> Option<ProblemKind> {
    let path = dir.join(rel);
    let len = bytes.len();
    match m {
        Mutation::Keep => None,
        Mutation::Delete => {
            std::fs::remove_file(&path).unwrap();
            Some(ProblemKind::Missing)
        }
        Mutation::Truncate(n) if len > 0 => {
            let n = n % len;
            std::fs::write(&path, &bytes[..n]).unwrap();
            Some(ProblemKind::Truncated {
                expected: len as u64,
                actual: n as u64,
            })
        }
        Mutation::FlipByte(i) if len > 0 => {
            let mut b = bytes.to_vec();
            b[i % len] ^= 0xFF;
            std::fs::write(&path, b).unwrap();
            Some(ProblemKind::Changed)
        }
        Mutation::Truncate(_) | Mutation::FlipByte(_) => None,
        Mutation::Append(extra) => {
            let mut b = bytes.to_vec();
            b.extend_from_slice(extra);
            std::fs::write(&path, b).unwrap();
            Some(ProblemKind::Changed)
        }
        Mutation::RewriteIdentical => {
            std::fs::write(&path, bytes).unwrap();
            None
        }
        Mutation::Chmod => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
            }
            None
        }
        Mutation::BecomeDir => {
            std::fs::remove_file(&path).unwrap();
            std::fs::create_dir_all(path.join("inner")).unwrap();
            Some(ProblemKind::Missing)
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, ..ProptestConfig::default() })]

    /// Exhaustive over nothing, sampled over everything: trees from `POOL`,
    /// one mutation per recorded file, and stray files written into the tree
    /// afterwards. The flagged set must be exactly the recorded files whose
    /// content or existence changed, each with the right kind — never a
    /// stray, never a file rewritten with its own bytes or re-moded.
    #[test]
    fn prop_only_changed_files_are_flagged(
        files in tree(),
        mutations in prop::collection::vec(mutation(), POOL.len()),
        strays in prop::collection::vec(
            (prop::sample::select(STRAYS), prop::collection::vec(any::<u8>(), 0..8)),
            0..4,
        ),
    ) {
        let tmp = TempDir::new().unwrap();
        let dir = package_dir(&tmp);
        for (rel, bytes) in &files {
            write(&dir.join(rel), bytes);
        }
        let manifest = record_tree(&dir).unwrap();
        prop_assert_eq!(manifest.files.len(), files.len());

        let mut expected = BTreeMap::new();
        for ((rel, bytes), m) in files.iter().zip(&mutations) {
            if let Some(kind) = apply(&dir, rel, bytes, m) {
                expected.insert(rel.to_string(), kind);
            }
        }
        for (rel, bytes) in &strays {
            // A stray only lands where nothing is: a stray over a recorded
            // path would be a mutation, which the list above already covers.
            let path = dir.join(rel);
            let blocked = path.ancestors().skip(1).any(|a| a.is_file());
            if !path.exists() && !files.contains_key(rel) && !blocked {
                write(&path, bytes);
            }
        }

        let report = check(&dir, &manifest);
        prop_assert_eq!(report.files, files.len());
        let actual: BTreeMap<String, ProblemKind> = report
            .problems
            .into_iter()
            .map(|p| (p.path, p.kind))
            .collect();
        prop_assert_eq!(actual, expected);
    }
}

#[test]
fn hash_file_meets_the_digest_spec() {
    crate::digest_spec::check_file_hasher(|p| {
        let (size, hex) = hash_file(p).unwrap();
        assert_eq!(size, std::fs::metadata(p).unwrap().len());
        hex
    });
}
