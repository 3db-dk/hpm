//! Executable-bit repair for package content.
//!
//! Archives reach hpm from arbitrary hosts — GitHub Releases, SideFX hpack, a
//! studio's own bucket, a contributor's `zip` on Windows — and a great many
//! zip producers drop Unix modes entirely, stamping every entry `0o644`.
//! Installing such an archive faithfully yields a package whose own declared
//! programs cannot be spawned: `Permission denied (os error 13)` on macOS and
//! Linux, while the identical package works on Windows, where executability is
//! not a file mode.
//!
//! The rule lives here rather than in the extractor because it has two callers
//! with the same question — "is this file a program that lost its bit?" —
//! asked at different moments:
//!
//! - [`repaired_mode`] / [`looks_executable`] answer it per entry as an archive
//!   is extracted (see `archive_fetcher::extract`).
//! - [`ensure_repaired`] answers it for a tree that is *already* installed.
//!   Extraction-time repair alone never reaches those: the install path
//!   short-circuits on an already-present version and never re-extracts, so a
//!   tree installed by an older hpm keeps `0o644` forever — until the package
//!   happens to change version, which for a working package it never does.
//!
//! The repair is additive and content-driven throughout: a mode that already
//! declares execute is untouched, a restrictive mode is not widened past its
//! read bits (`0o640` becomes `0o750`), and a file that is not a program is
//! never made executable. It grants nothing a well-formed archive could not
//! claim for itself by declaring `0o755`, and hpm already runs `[scripts]`
//! programs out of the installed tree.

use std::path::{Path, PathBuf};

#[cfg(unix)]
use tracing::{debug, warn};

/// Suffix of the sibling file recording that a tree has been swept. Written
/// next to the package directory rather than inside it: [`crate::tree_hash`]
/// digests every file in the tree except `.hpm-checksum`, so an in-tree marker
/// would change the package's recorded checksum and fail
/// `LockFile::verify_checksums` — including for an hpm old enough not to know
/// to skip it.
const STAMP_SUFFIX: &str = ".exec-modes";

/// Contents of the stamp file: the revision of the rule that swept the tree.
/// A tree stamped with an older revision is swept again, so widening
/// [`looks_executable`] later can be rolled out to existing installs by
/// bumping this.
#[cfg(unix)]
const STAMP_REVISION: &str = "1";

/// Path of the stamp recording that `package_dir` has been swept.
fn stamp_path(package_dir: &Path) -> Option<PathBuf> {
    let name = package_dir.file_name()?.to_str()?;
    Some(package_dir.with_file_name(format!("{name}{STAMP_SUFFIX}")))
}

/// True if `package_dir` has already been swept by the current rule revision.
#[cfg(unix)]
fn already_repaired(package_dir: &Path) -> bool {
    stamp_path(package_dir)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .is_some_and(|s| s.trim() == STAMP_REVISION)
}

/// Record that `package_dir` has been swept. Best-effort: a failure here costs
/// a repeated sweep, not correctness, so it warns rather than propagating.
#[cfg(unix)]
fn write_stamp(package_dir: &Path) {
    let Some(path) = stamp_path(package_dir) else {
        return;
    };
    if let Err(e) = std::fs::write(&path, STAMP_REVISION) {
        warn!(
            "Could not record the executable-mode sweep at {}: {} \
             (harmless; the sweep will run again)",
            path.display(),
            e
        );
    }
}

/// Delete the stamp for `package_dir`, if any. Called when the package
/// directory is removed so a stale sibling doesn't outlive its tree.
pub fn forget_repair(package_dir: &Path) {
    if let Some(path) = stamp_path(package_dir) {
        let _ = std::fs::remove_file(path);
    }
}

/// Restore lost executable bits across an already-installed package tree,
/// once.
///
/// Returns the number of files repaired. Errors are logged and swallowed: a
/// caller is on its way to *using* the package, and a tree that cannot be
/// swept is no worse off than before the sweep existed.
///
/// The sweep is skipped entirely once a tree carries a current stamp, so the
/// walk costs one pass per installed version rather than one per resolve.
#[cfg(unix)]
pub fn ensure_repaired(package_dir: &Path) -> usize {
    if already_repaired(package_dir) {
        return 0;
    }
    let repaired = repair_tree(package_dir);
    if repaired > 0 {
        debug!(
            "Restored the executable bit on {} file(s) under {}",
            repaired,
            package_dir.display()
        );
    }
    write_stamp(package_dir);
    repaired
}

/// Windows has no executable bit, so there is nothing to repair and no stamp
/// worth writing.
#[cfg(windows)]
pub fn ensure_repaired(_package_dir: &Path) -> usize {
    0
}

/// Walk `dir` and restore the executable bit on every regular file that looks
/// like a program and carries none. Unconditional, and deliberately not public:
/// [`ensure_repaired`] is the entry point, and it is the stamp that keeps this
/// to one sweep per tree rather than one per caller.
#[cfg(unix)]
fn repair_tree(dir: &Path) -> usize {
    use std::os::unix::fs::PermissionsExt;

    let mut repaired = 0;
    // `walkdir` does not follow symlinks by default, which is what we want:
    // extraction never creates them, and a link pointing out of the tree must
    // not have its target chmodded.
    for entry in walkdir::WalkDir::new(dir) {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                warn!("Skipping unreadable entry while repairing modes: {}", e);
                continue;
            }
        };
        if !entry.file_type().is_file() {
            continue;
        }
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        let mode = meta.permissions().mode() & 0o7777;
        let wanted = repaired_mode(mode, entry.path());
        if wanted == mode {
            continue;
        }
        match std::fs::set_permissions(entry.path(), std::fs::Permissions::from_mode(wanted)) {
            Ok(()) => {
                debug!(
                    "Restored executable bit on {} ({:04o} -> {:04o})",
                    entry.path().display(),
                    mode,
                    wanted
                );
                repaired += 1;
            }
            Err(e) => warn!(
                "Could not restore the executable bit on {}: {}",
                entry.path().display(),
                e
            ),
        }
    }
    repaired
}

/// The mode `path` should carry given the mode it has.
///
/// Returns `mode` unchanged unless it grants no execute permission *and* the
/// file's leading bytes identify it as a program, in which case the execute
/// bits mirror the read bits so the mode stays consistent with the access the
/// producer declared (`0o644` -> `0o755`, `0o640` -> `0o750`).
#[cfg(unix)]
pub fn repaired_mode(mode: u32, path: &Path) -> u32 {
    if mode & 0o111 == 0 && looks_executable(path) {
        mirror_read_bits(mode)
    } else {
        mode
    }
}

/// `mode` with each execute bit set wherever the matching read bit already is
/// (`0o644` -> `0o755`, `0o640` -> `0o750`).
///
/// Exposed separately for the caller that has better evidence than the content
/// sniff can produce: an embedder about to spawn a path a manifest declares as
/// a program knows it is one without reading a byte of it. That covers what a
/// sweep structurally cannot — a dev copy it does not walk, a file that
/// appeared after the tree was stamped — and the widening must stay identical
/// to the sweep's, hence one function rather than two.
#[cfg(unix)]
pub fn mirror_read_bits(mode: u32) -> u32 {
    mode | ((mode & 0o444) >> 2)
}

/// True if `path`'s leading bytes identify an executable program: an ELF
/// object (Linux), a Mach-O object in either endianness or a universal binary
/// (macOS), or a `#!` interpreter script.
///
/// Content decides, so no manifest parsing or argv tokenizing is involved —
/// which also covers the helper binary a declared script shells out to, not
/// just the declared entry point.
///
/// Shared libraries (`.so` / `.dylib`) share the ELF/Mach-O magics and are
/// matched too. That is intentional and harmless — they are conventionally
/// 0o755, and marking one executable does not make it a program anyone runs.
#[cfg(unix)]
pub fn looks_executable(path: &Path) -> bool {
    use std::io::Read;

    let mut head = [0u8; 4];
    let Ok(mut f) = std::fs::File::open(path) else {
        return false;
    };
    let mut filled = 0;
    // A short read is not EOF, so loop until the buffer is full or the file is.
    while filled < head.len() {
        match f.read(&mut head[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(_) => return false,
        }
    }
    if filled >= 2 && &head[..2] == b"#!" {
        return true;
    }
    if filled < 4 {
        return false;
    }
    if &head == b"\x7fELF" {
        return true;
    }
    matches!(
        u32::from_be_bytes(head),
        // Mach-O thin, 32/64-bit, big- and little-endian.
        0xFEED_FACE | 0xFEED_FACF | 0xCEFA_EDFE | 0xCFFA_EDFE
        // Mach-O universal ("fat") binary, both byte orders.
        | 0xCAFE_BABE | 0xBEBA_FECA
    )
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use tempfile::TempDir;

    /// Mach-O 64-bit little-endian magic, as an arm64 `tt_setup` starts.
    const MACH_O_64: [u8; 4] = [0xCF, 0xFA, 0xED, 0xFE];

    fn write(dir: &Path, rel: &str, head: &[u8], mode: u32) -> PathBuf {
        let path = dir.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, head).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
        path
    }

    fn mode_of(path: &Path) -> u32 {
        std::fs::metadata(path).unwrap().permissions().mode() & 0o7777
    }

    #[test]
    fn magics_that_identify_a_program() {
        let temp = TempDir::new().unwrap();
        let probe = |bytes: &[u8]| {
            let p = temp.path().join("probe");
            std::fs::write(&p, bytes).unwrap();
            looks_executable(&p)
        };

        assert!(probe(b"\x7fELF\x02\x01"));
        assert!(probe(b"#!/usr/bin/env python3\n"));
        assert!(probe(&0xCFFA_EDFEu32.to_be_bytes())); // Mach-O 64-bit LE
        assert!(probe(&0xCAFE_BABEu32.to_be_bytes())); // Mach-O universal

        assert!(!probe(b"[package]\nname = \"x\"\n"));
        assert!(!probe(b""));
        // Too short to be any magic, and not a shebang.
        assert!(!probe(b"#"));
    }

    #[test]
    fn a_program_stripped_of_its_bit_is_made_runnable() {
        let dir = TempDir::new().unwrap();
        let macho = write(dir.path(), "bin/macos-aarch64/tt_setup", &MACH_O_64, 0o644);
        let elf = write(dir.path(), "bin/linux-x86_64/tt_setup", b"\x7fELF", 0o644);
        let script = write(
            dir.path(),
            "scripts/setup.py",
            b"#!/usr/bin/env python",
            0o644,
        );

        assert_eq!(repair_tree(dir.path()), 3);
        assert_eq!(mode_of(&macho), 0o755);
        assert_eq!(mode_of(&elf), 0o755);
        assert_eq!(mode_of(&script), 0o755);
    }

    #[test]
    fn execute_mirrors_read_rather_than_widening_access() {
        let dir = TempDir::new().unwrap();
        let tool = write(dir.path(), "bin/tool", &MACH_O_64, 0o640);
        assert_eq!(repair_tree(dir.path()), 1);
        assert_eq!(
            mode_of(&tool),
            0o750,
            "group keeps read+exec, other neither"
        );
    }

    #[test]
    fn a_file_that_is_not_a_program_is_never_made_executable() {
        let dir = TempDir::new().unwrap();
        let hda = write(dir.path(), "otls/Recipes.hda", b"INDX", 0o644);
        let toml = write(dir.path(), "hpm.toml", b"[package]", 0o644);
        let empty = write(dir.path(), "py/__init__.py", b"", 0o644);

        assert_eq!(repair_tree(dir.path()), 0);
        assert_eq!(mode_of(&hda), 0o644);
        assert_eq!(mode_of(&toml), 0o644);
        assert_eq!(mode_of(&empty), 0o644);
    }

    #[test]
    fn a_mode_that_already_declares_execute_is_left_alone() {
        let dir = TempDir::new().unwrap();
        // 0o700 is narrower than the 0o755 we would synthesize; the repair is
        // additive, so a producer that deliberately restricted access keeps it.
        let tool = write(dir.path(), "bin/tool", &MACH_O_64, 0o700);
        assert_eq!(repair_tree(dir.path()), 0);
        assert_eq!(mode_of(&tool), 0o700);
    }

    #[test]
    fn the_sweep_runs_once_per_tree() {
        let dir = TempDir::new().unwrap();
        let pkg = dir.path().join("tumblepipe@1.39.2");
        let tool = write(&pkg, "bin/macos-aarch64/tt_setup", &MACH_O_64, 0o644);

        assert_eq!(ensure_repaired(&pkg), 1);
        assert_eq!(mode_of(&tool), 0o755);

        // Re-stripping the bit by hand stands in for "the walk ran again":
        // a second sweep would restore it, a stamped tree leaves it alone.
        std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(ensure_repaired(&pkg), 0);
        assert_eq!(mode_of(&tool), 0o644);
    }

    #[test]
    fn a_stamp_from_an_older_rule_revision_sweeps_again() {
        let dir = TempDir::new().unwrap();
        let pkg = dir.path().join("tumblepipe@1.39.2");
        let tool = write(&pkg, "bin/tool", &MACH_O_64, 0o644);
        std::fs::write(stamp_path(&pkg).unwrap(), "0").unwrap();

        assert_eq!(ensure_repaired(&pkg), 1);
        assert_eq!(mode_of(&tool), 0o755);
    }

    #[test]
    fn the_stamp_lands_outside_the_package_tree() {
        // In-tree it would be digested by `tree_hash` and change the package's
        // recorded checksum — see the module docs.
        let dir = TempDir::new().unwrap();
        let pkg = dir.path().join("tumblepipe@1.39.2");
        write(&pkg, "hpm.toml", b"[package]", 0o644);

        ensure_repaired(&pkg);
        let stamp = stamp_path(&pkg).unwrap();
        assert!(stamp.is_file(), "stamp should exist");
        assert_eq!(stamp.parent(), pkg.parent());
        assert_eq!(
            std::fs::read_dir(&pkg).unwrap().count(),
            1,
            "package tree must contain only its own content"
        );

        forget_repair(&pkg);
        assert!(!stamp.exists());
    }

    // ---- Invariants ---------------------------------------------------
    //
    // The examples above pin the cases we know about. These pin the
    // properties, which is what a later change to the walk or the rule is
    // liable to break without breaking any single example: this code widens
    // permissions on files inside a package a user installed, so "only ever
    // adds execute, only to programs, and never touches anything else" has to
    // hold for every input, not for the four we thought of.

    /// The mode space is 4096 values, so this is exhaustive rather than
    /// sampled — a property test here would be strictly weaker.
    #[test]
    fn mirror_read_bits_only_ever_adds_execute_where_read_is_granted() {
        for mode in 0..=0o7777u32 {
            let out = mirror_read_bits(mode);

            assert_eq!(out & mode, mode, "{mode:04o}: a bit was cleared");
            assert_eq!(
                out & !0o111,
                mode & !0o111,
                "{mode:04o}: something other than an execute bit changed"
            );
            assert_eq!(
                out & 0o111,
                (mode & 0o111) | ((mode & 0o444) >> 2),
                "{mode:04o}: execute must mirror read, per class"
            );
            assert_eq!(
                out & 0o7000,
                mode & 0o7000,
                "{mode:04o}: setuid/setgid/sticky must survive untouched"
            );
            assert_eq!(mirror_read_bits(out), out, "{mode:04o}: not idempotent");
        }
    }

    /// A mode with no read bit for a class must not gain execute for it —
    /// stated separately because it is the one that keeps a deliberately
    /// private file private (`0o600` stays owner-only at `0o700`).
    #[test]
    fn mirror_read_bits_never_grants_execute_to_a_class_that_cannot_read() {
        for mode in 0..=0o7777u32 {
            let out = mirror_read_bits(mode);
            for shift in [6, 3, 0] {
                let can_read = mode & (0o4 << shift) != 0;
                let gained_exec = (out & !mode) & (0o1 << shift) != 0;
                assert!(
                    can_read || !gained_exec,
                    "{mode:04o}: gained execute for a class with no read"
                );
            }
        }
    }

    proptest::proptest! {
        /// Whatever the content, `repaired_mode` may only ever add execute —
        /// and only when the file is a program that has none. The content
        /// generator deliberately mixes real magics with arbitrary bytes so
        /// the "is a program" branch is exercised from both sides.
        #[test]
        fn prop_repaired_mode_is_additive_and_only_for_programs(
            content in proptest::collection::vec(proptest::num::u8::ANY, 0..64),
            mode in 0u32..=0o777,
        ) {
            let dir = TempDir::new().unwrap();
            let path = write(dir.path(), "probe", &content, mode);

            let out = repaired_mode(mode, &path);
            let is_program = looks_executable(&path);

            proptest::prop_assert_eq!(out & mode, mode, "a bit was cleared");
            proptest::prop_assert_eq!(out & !0o111, mode & !0o111, "a non-execute bit changed");
            if !is_program || mode & 0o111 != 0 {
                proptest::prop_assert_eq!(
                    out, mode,
                    "only a program with no execute bit may be changed"
                );
            } else {
                proptest::prop_assert_eq!(out, mirror_read_bits(mode));
            }
            proptest::prop_assert_eq!(repaired_mode(out, &path), out, "not idempotent");
        }

        /// The stamp must land beside the tree it describes and never within
        /// it, for any package directory name. This is the invariant with the
        /// worst failure mode in the module: `tree_hash` digests every file in
        /// a package except `.hpm-checksum`, so a stamp that ever landed
        /// inside would change the package's recorded checksum and fail
        /// `LockFile::verify_checksums` — for hpm versions that predate the
        /// stamp too, i.e. unfixably, on machines we'd never hear from.
        #[test]
        fn prop_the_stamp_is_always_a_sibling_never_a_child(
            scope in "[a-zA-Z0-9_.-]{1,20}",
            name in "[a-zA-Z0-9_.@+-]{1,30}",
        ) {
            let root = Path::new("/packages").join(&scope);
            let pkg = root.join(&name);
            let stamp = stamp_path(&pkg).expect("a named directory always has a stamp path");

            proptest::prop_assert_eq!(stamp.parent(), pkg.parent());
            proptest::prop_assert!(!stamp.starts_with(&pkg), "stamp must not sit inside the tree");
            proptest::prop_assert_ne!(&stamp, &pkg, "stamp must not be the tree itself");
        }

        /// Classification reads the leading bytes and nothing else. Without
        /// this, a future "search the file for a magic" would mark every HDA
        /// that happens to embed a compiled payload as a program.
        #[test]
        fn prop_looks_executable_ignores_everything_past_the_head(
            head in proptest::collection::vec(proptest::num::u8::ANY, 4..8),
            tail in proptest::collection::vec(proptest::num::u8::ANY, 0..128),
        ) {
            let dir = TempDir::new().unwrap();
            let short = write(dir.path(), "short", &head, 0o644);
            let long = write(dir.path(), "long", &[head.clone(), tail].concat(), 0o644);
            proptest::prop_assert_eq!(looks_executable(&short), looks_executable(&long));
        }

        /// The sweep over a whole tree: file *contents* are never touched, no
        /// permission is ever removed, and anything that isn't a program with
        /// a missing bit comes back byte-for-byte and mode-for-mode identical.
        /// Idempotence is asserted against a forced re-sweep, since the stamp
        /// would otherwise make the second call trivially a no-op.
        #[test]
        fn prop_sweeping_a_tree_changes_only_programs_missing_their_bit(
            files in proptest::collection::vec(
                (
                    proptest::collection::vec(proptest::num::u8::ANY, 0..32),
                    0u32..=0o777,
                ),
                1..6,
            ),
        ) {
            let dir = TempDir::new().unwrap();
            let pkg = dir.path().join("pkg@1.0.0");

            let before: Vec<_> = files
                .iter()
                .enumerate()
                .map(|(i, (content, mode))| {
                    let path = write(&pkg, &format!("sub/f{i}"), content, *mode);
                    let is_program = looks_executable(&path);
                    (path, content.clone(), *mode, is_program)
                })
                .collect();

            ensure_repaired(&pkg);
            let after_first: Vec<u32> = before.iter().map(|(p, ..)| mode_of(p)).collect();

            for ((path, _, mode, is_program), now) in before.iter().zip(&after_first) {
                let now = *now;
                proptest::prop_assert_eq!(now & mode, *mode, "a permission was removed");
                if !is_program || mode & 0o111 != 0 {
                    proptest::prop_assert_eq!(now, *mode, "untouched files must stay untouched");
                } else {
                    proptest::prop_assert_eq!(now, mirror_read_bits(*mode));
                }
                let _ = path;
            }

            // Second pass with the stamp removed, so the walk really runs
            // again rather than short-circuiting on the stamp.
            forget_repair(&pkg);
            ensure_repaired(&pkg);
            for ((path, ..), expected) in before.iter().zip(&after_first) {
                proptest::prop_assert_eq!(mode_of(path), *expected, "sweep is not idempotent");
            }

            // Contents last: a file generated `0o000` cannot be read back
            // until the test grants itself permission, and doing that earlier
            // would overwrite the modes under assertion.
            for (path, content, ..) in &before {
                std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
                proptest::prop_assert_eq!(
                    &std::fs::read(path).unwrap(), content,
                    "the repair must never rewrite a file's bytes"
                );
            }
        }
    }
}
