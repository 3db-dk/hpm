//! Path-dependency ("dev") install support: link/copy primitives and the
//! `_dev/` subtree under `packages_dir`.
//!
//! These helpers keep the platform-specific symlink/junction handling and
//! the install-entry removal semantics out of `storage.rs` so the
//! [`StorageManager`](super::StorageManager) impl can read top-down.

use super::error::StorageError;
use crate::packer::{StageFilter, build_ignore_rules, collect_stage_entries};
use hpm_package::platform::Platform;
use hpm_package::{IoOp, PackageManifest};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use tracing::warn;

/// Subdirectory of `packages_dir` reserved for path-installed (dev) packages.
/// Kept out of the registry CAS namespace so a dev install of `foo@1.0.0`
/// can coexist with — and is never substituted for — a registry install at
/// the same coordinate.
pub(super) const DEV_INSTALL_DIR: &str = "_dev";

#[derive(Debug, Clone, Copy)]
pub(super) enum InstallStyle {
    /// Registry/URL fetch → copy into the CAS at `packages_dir/<slug>@<ver>/`.
    CasCopy,
    /// Path dep → copy into `packages_dir/_dev/<slug>@<ver>/`.
    DevCopy,
    /// Path dep → symlink/junction at `packages_dir/_dev/<slug>@<ver>/`
    /// pointing at the workspace.
    DevLink,
}

impl InstallStyle {
    pub(super) fn log_kind(self) -> &'static str {
        match self {
            InstallStyle::CasCopy => "",
            InstallStyle::DevCopy => "dev ",
            InstallStyle::DevLink => "dev-link ",
        }
    }
}

/// A path-installed (dev) package entry under `<packages_dir>/_dev/`.
///
/// Identity comes from the directory name (`<slug>@<version>`), not from
/// reading the entry's `hpm.toml` — link installs that point at a deleted
/// workspace still surface as a `DevInstall` so cleanup can collect them.
#[derive(Debug, Clone)]
pub struct DevInstall {
    pub slug: String,
    pub version: String,
    pub install_path: PathBuf,
}

impl DevInstall {
    /// Identifier used in CLI output and `removed_dev_installs`. Prefixed
    /// with `_dev/` so users can distinguish dev cleanup from CAS cleanup
    /// in the same `hpm clean` listing.
    pub fn identifier(&self) -> String {
        format!("_dev/{}@{}", self.slug, self.version)
    }
}

/// Remove `path` if it is a symlink/junction, without following the link.
pub(super) fn remove_dev_link(path: &std::path::Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        // On Unix, symlink-to-directory entries are removed via `remove_file`.
        std::fs::remove_file(path)
    }
    #[cfg(windows)]
    {
        // `junction::delete` strips the reparse point but leaves the now-empty
        // directory stub in place — re-creating the link at the same path
        // would then fail with ERROR_ALREADY_EXISTS (os error 183). Remove the
        // stub explicitly. The same applies to NTFS symlinks-to-dirs, whose
        // reparse point sits on a directory entry that survives `delete`.
        junction::delete(path)?;
        std::fs::remove_dir(path)
    }
}

/// Create a symlink (Unix) or junction (Windows) at `link` pointing at the
/// absolute `target`. The target must be a directory.
pub(super) fn create_dev_link(
    target: &std::path::Path,
    link: &std::path::Path,
) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link)
    }
    #[cfg(windows)]
    {
        // Junctions are intentional here (vs NTFS symlinks): they don't
        // require Developer Mode or admin, which makes the link-install
        // workflow viable on a stock Houdini workstation.
        junction::create(target, link)
    }
}

/// Returns true when the entry at `path` is a symlink (Unix) or a
/// junction/symlink (Windows). Caller must have already verified the entry
/// exists (typically by reading `symlink_metadata` themselves) — this helper
/// is a pure file-type predicate that doesn't follow links.
///
/// Returns `Err` on Windows when `junction::exists` fails for a *genuine*
/// reason (permission denied, FS error). The error must propagate —
/// silently treating "junction check failed" as "not a junction" would let
/// `remove_dir_all` recurse into the user's workspace the next time around.
///
/// `ERROR_NOT_A_REPARSE_POINT` (Windows error 4390) is *not* a genuine
/// failure — it is how the underlying `DeviceIoControl(FSCTL_GET_REPARSE_POINT)`
/// reports "this exists but isn't a reparse point", i.e. the negative
/// answer for our predicate. Map it to `Ok(false)`. Without this, every
/// `_dev/<slug>@<version>/` install made with `install_as_dev_copy`
/// (a plain directory, no reparse point) raises an IoOp on
/// inspection, breaking orphan detection and the copy→link switch path.
pub(super) fn is_link_entry(
    meta: &std::fs::Metadata,
    path: &std::path::Path,
) -> std::io::Result<bool> {
    if meta.file_type().is_symlink() {
        return Ok(true);
    }
    #[cfg(windows)]
    {
        // Older Rust stdlib reports junctions as non-symlinks; ask the
        // junction crate directly so callers never accidentally fall through
        // to `remove_dir_all` on a reparse point.
        const ERROR_NOT_A_REPARSE_POINT: i32 = 4390;
        match junction::exists(path) {
            Ok(b) => Ok(b),
            Err(e) if e.raw_os_error() == Some(ERROR_NOT_A_REPARSE_POINT) => Ok(false),
            Err(e) => Err(e),
        }
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        Ok(false)
    }
}

/// Remove an install entry without following links. The caller is
/// responsible for verifying the entry exists (and supplying the matching
/// metadata) so this stays a pure removal primitive.
///
/// - Symlink/junction → remove the link entry itself.
/// - Real directory → `remove_dir_all`, with Houdini-handle errors lifted to
///   [`StorageError::PackageInUse`] so the user gets an actionable message.
pub(super) fn remove_install_entry(
    target_dir: &std::path::Path,
    meta: &std::fs::Metadata,
    name: &str,
    version: &str,
) -> Result<(), StorageError> {
    // The executable-mode sweep records itself in a sibling file, since an
    // in-tree marker would perturb the package's tree hash. Drop it here so it
    // doesn't outlive the tree it describes.
    crate::exec_mode::forget_repair(target_dir);
    // Same for the install record, and first: a record must never describe
    // a tree that is half removed.
    crate::install_manifest::forget(target_dir);

    if is_link_entry(meta, target_dir)
        .map_err(|e| IoOp::wrap("inspect install entry at", target_dir, e))?
    {
        return remove_dev_link(target_dir)
            .map_err(|e| IoOp::wrap("remove dev link at", target_dir, e).into());
    }
    std::fs::remove_dir_all(target_dir).map_err(|e| {
        // On Windows, a running Houdini process holds open handles to files
        // inside the package dir, so removal fails with ERROR_ACCESS_DENIED
        // (os error 5 → PermissionDenied). Map it to an actionable error
        // instead of leaking a raw OS code.
        if e.kind() == std::io::ErrorKind::PermissionDenied {
            StorageError::PackageInUse {
                name: name.to_string(),
                version: version.to_string(),
                source: e,
            }
        } else {
            IoOp::wrap("remove install entry at", target_dir, e).into()
        }
    })
}

/// Replace whatever is currently at `target_dir` with a clean slate, with
/// link-aware removal semantics. Always safe to call before installing.
///
/// - Missing → no-op.
/// - Symlink/junction → remove the link entry itself; never follow.
/// - Real directory → `remove_dir_all`, with Houdini-handle errors lifted to
///   [`StorageError::PackageInUse`] so the user gets an actionable message.
pub(super) fn clear_existing_install(
    target_dir: &std::path::Path,
    name: &str,
    version: &str,
) -> Result<(), StorageError> {
    let meta = match std::fs::symlink_metadata(target_dir) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(IoOp::wrap("stat install target", target_dir, e).into()),
    };

    if is_link_entry(&meta, target_dir)
        .map_err(|e| IoOp::wrap("inspect install entry at", target_dir, e))?
    {
        warn!(
            "replacing existing link install for {}@{} at {}",
            name,
            version,
            target_dir.display()
        );
    } else {
        warn!(
            "package {}@{} already exists, removing old version",
            name, version
        );
    }
    remove_install_entry(target_dir, &meta, name, version)
}

/// Remove a stale dev *link* occupying the container path so the container can
/// become a directory of content-addressed copies. Called before a `DevCopy`
/// install, where the container (`_dev/<slug>@<version>`) may still hold a
/// junction/symlink left by a prior `DevLink` install of the same coordinate.
///
/// A real directory (holding one or more live hash copies) or a missing path is
/// left untouched: content addressing exists precisely so we never
/// `remove_dir_all` a directory a running Houdini may have memory-mapped.
pub(super) fn clear_container_link(container: &Path) -> Result<(), StorageError> {
    let meta = match std::fs::symlink_metadata(container) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(IoOp::wrap("stat dev container", container, e).into()),
    };
    if is_link_entry(&meta, container)
        .map_err(|e| IoOp::wrap("inspect dev container at", container, e))?
    {
        warn!(
            "replacing prior dev link at {} with a content-addressed copy",
            container.display()
        );
        remove_dev_link(container)
            .map_err(|e| IoOp::wrap("remove stale dev link at", container, e))?;
    }
    Ok(())
}

/// `[stage.profile.*]` table a dev copy is staged with — the same default
/// `hpm build` uses when no `--profile` is given.
const DEV_COPY_PROFILE: &str = "release";

/// The file set a dev copy installs, and the hash that names its directory.
pub(super) struct DevCopyImage {
    /// `(workspace source file, install-relative destination)`, sorted by
    /// destination.
    pub(super) entries: Vec<(PathBuf, String)>,
    /// Hex SHA-256 over the entries; see [`dev_copy_image`].
    pub(super) hash: String,
}

/// Compute the install image of the `source` workspace: exactly the files
/// `hpm pack` / `hpm build` would stage for the host platform. That means
/// `.gitignore` + `.hpmignore` (with `!` re-includes), the `[stage]`
/// include/exclude globs, and the host's `[stage.platform.*]` place rules.
/// A dev copy used to clone the whole workspace, `.git/`, cargo `target/`
/// dirs, CMake build trees and virtualenvs included. On a native package
/// that was tens of gigabytes, and the copy outlasted the caller.
///
/// `hpm.toml` is always part of the image, even when a `[stage].include`
/// allowlist omits it: the store reads it to recognise a complete copy
/// ([`dev_copy_is_complete`]) and to load the installed package.
///
/// The hash is a SHA-256 over each entry's destination path, byte length and
/// modification time. Stat-only, so it stays cheap even with hundreds of
/// megabytes of DSOs. It *names* the content-addressed install directory
/// (`_dev/<slug>@<version>/<hash>/`): a workspace rebuild bumps a staged
/// file's mtime, changing the digest, so the next launch installs into (and
/// points Houdini at) a fresh directory while a running Houdini keeps the one
/// it was launched from. Edits to ignored files (build trees, `.git/`) no
/// longer change the hash, so they no longer force a recopy.
pub(super) fn dev_copy_image(
    source: &Path,
    manifest: &PackageManifest,
) -> Result<DevCopyImage, StorageError> {
    let stage = manifest.stage.resolved_for_profile(DEV_COPY_PROFILE);
    // Mirror `hpm build`'s target: the host when the package declares
    // platforms, otherwise no per-platform placement.
    let target = if manifest.compat.platforms.is_empty() {
        None
    } else {
        Platform::current()
    };
    let filter = StageFilter::new(&stage, target.as_ref())?;
    let ignore = build_ignore_rules(source)?;
    let mut entries = collect_stage_entries(source, &ignore, Some(&filter), None)?;

    if !entries.iter().any(|(_, dest)| dest == "hpm.toml") {
        entries.push((source.join("hpm.toml"), "hpm.toml".to_string()));
    }
    // Sort for a deterministic digest independent of walk order and of where
    // place rules moved a file.
    entries.sort_by(|a, b| a.1.cmp(&b.1));

    let mut hasher = Sha256::new();
    for (path, dest) in &entries {
        hasher.update(dest.as_bytes());
        hasher.update(b"\0");
        // `metadata` follows links: a staged file symlink is copied as its
        // target's bytes, so the target's stat is what fingerprints it.
        let meta =
            std::fs::metadata(path).map_err(|e| IoOp::wrap("stat dev source file", path, e))?;
        hasher.update(meta.len().to_le_bytes());
        if let Ok(dur) = meta.modified().and_then(|m| {
            m.duration_since(std::time::UNIX_EPOCH)
                .map_err(std::io::Error::other)
        }) {
            hasher.update(dur.as_secs().to_le_bytes());
            hasher.update(dur.subsec_nanos().to_le_bytes());
        }
    }

    Ok(DevCopyImage {
        entries,
        hash: hex_digest(hasher),
    })
}

/// Hash naming the current dev copy of the `source` workspace: the
/// [`dev_copy_image`] hash, with the manifest read from `source`. Used by
/// cleanup to tell the live copy from superseded ones.
pub(super) fn source_hash(source: &Path) -> Result<String, StorageError> {
    let manifest = PackageManifest::from_path(&source.join("hpm.toml"))?;
    Ok(dev_copy_image(source, &manifest)?.hash)
}

/// Copy a [`DevCopyImage`]'s entries into `target`.
pub(super) fn copy_dev_image(
    entries: &[(PathBuf, String)],
    target: &Path,
) -> Result<(), StorageError> {
    std::fs::create_dir_all(target).map_err(|e| IoOp::wrap("create dev copy target", target, e))?;
    for (source, dest) in entries {
        let dest = target.join(dest);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| IoOp::wrap("create dev copy directory", parent, e))?;
        }
        std::fs::copy(source, &dest).map_err(|e| IoOp::wrap("copy file to", &dest, e))?;
    }
    Ok(())
}

fn hex_digest(hasher: Sha256) -> String {
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect()
}

/// True when `name` is a content-hash directory name — 64 lowercase hex chars,
/// as produced by [`source_hash`]. Used to tell content copies apart from the
/// legacy flat layout and from hidden staging directories under a container.
pub(super) fn is_hash_dir_name(name: &str) -> bool {
    name.len() == 64
        && name
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Content-addressed install path for a dev copy: `<container>/<source_hash>`.
pub(super) fn dev_copy_target(container: &Path, source_hash: &str) -> PathBuf {
    container.join(source_hash)
}

/// True when `target` holds a *complete* dev copy. Copies are committed by an
/// atomic rename of a fully-populated staging directory, so the presence of the
/// package manifest is a sufficient completeness signal — a hash directory can
/// only exist as the result of a committed copy.
pub(super) fn dev_copy_is_complete(target: &Path) -> bool {
    target.join("hpm.toml").is_file()
}

/// Process-local sequence for staging directory names, so two dev copies
/// materialized concurrently within one process never collide.
static STAGE_SEQ: AtomicU64 = AtomicU64::new(0);

/// A unique, hidden staging directory under `container` for an in-progress
/// copy. Hidden (`.`-prefixed, not a hash name) so GC and concurrent installs
/// of the same package never disturb it; unique via pid + a process-local
/// counter so two installs racing the same hash each stage into their own
/// directory.
pub(super) fn stage_dir(container: &Path) -> PathBuf {
    let seq = STAGE_SEQ.fetch_add(1, Ordering::Relaxed);
    container.join(format!(".stage-{}-{}", std::process::id(), seq))
}

/// Commit a fully-populated staging directory into its content-addressed home
/// with an atomic rename. Never removes an existing target: if another process
/// won the race and already materialized the same hash, the rename fails
/// (destination exists / not empty) and — because the content is identical by
/// construction — we drop our stage and report success.
pub(super) fn commit_staged_copy(staged: &Path, target: &Path) -> Result<(), StorageError> {
    match std::fs::rename(staged, target) {
        Ok(()) => Ok(()),
        // Lost the race: a complete copy at this hash already exists. Clean up
        // our stage (best-effort) and treat the install as already done.
        Err(_) if dev_copy_is_complete(target) => {
            let _ = std::fs::remove_dir_all(staged);
            Ok(())
        }
        Err(e) => Err(IoOp::wrap("commit dev copy to", target, e).into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression for the Windows reparse-point bug: a plain directory
    /// (a `DevCopy` install) must inspect as "not a link", not as
    /// an `ERROR_NOT_A_REPARSE_POINT` IoOp.
    ///
    /// The bug only surfaced on Windows (where `junction::exists` rides
    /// over a FSCTL ioctl that reports 4390 for non-reparse entries),
    /// but the *contract* — `is_link_entry` on a real directory returns
    /// `Ok(false)` — is platform-independent. Asserting it on every host
    /// keeps the contract from quietly re-regressing under future tightenings.
    #[test]
    fn plain_directory_inspects_as_non_link() {
        let tmp = tempfile::TempDir::new().unwrap();
        let dir = tmp.path().join("dev_copy_install");
        std::fs::create_dir(&dir).unwrap();
        let meta = std::fs::symlink_metadata(&dir).unwrap();

        let result = is_link_entry(&meta, &dir);
        assert!(
            matches!(result, Ok(false)),
            "expected Ok(false) for a plain directory, got {:?}",
            result
        );
    }

    /// On Unix, a symlink to a directory must inspect as a link. The
    /// Windows `junction` branch isn't exercised by this test (the entry
    /// is a symlink, so the early return fires first); pair with a manual
    /// Windows-side smoke test for full coverage.
    #[cfg(unix)]
    #[test]
    fn symlink_inspects_as_link() {
        let tmp = tempfile::TempDir::new().unwrap();
        let target = tmp.path().join("target");
        std::fs::create_dir(&target).unwrap();
        let link = tmp.path().join("the_link");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let meta = std::fs::symlink_metadata(&link).unwrap();

        assert!(matches!(is_link_entry(&meta, &link), Ok(true)));
    }

    fn write(path: &std::path::Path, contents: &str) {
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p).unwrap();
        }
        std::fs::write(path, contents).unwrap();
    }

    /// The hash names the content-addressed install dir, so it must be a stable
    /// 64-char hex string and change when a source file changes.
    #[test]
    fn source_hash_is_stable_hex_and_tracks_changes() {
        let tmp = tempfile::TempDir::new().unwrap();
        let src = tmp.path().join("src");
        write(
            &src.join("hpm.toml"),
            "[package]\npath = \"creator/x\"\nname = \"X\"\nversion = \"1.0.0\"\n",
        );
        write(&src.join("dso/plugin.so"), "BINARY");

        let first = source_hash(&src).unwrap();
        assert!(is_hash_dir_name(&first), "hash must be a valid dir name");
        assert_eq!(first, source_hash(&src).unwrap(), "hash is deterministic");

        // A longer file changes the length component of the digest.
        write(&src.join("dso/plugin.so"), "BINARY-REBUILT-LONGER");
        assert_ne!(
            first,
            source_hash(&src).unwrap(),
            "rebuild yields a new hash"
        );
    }

    #[test]
    fn is_hash_dir_name_rejects_non_hashes() {
        assert!(is_hash_dir_name(&"a".repeat(64)));
        assert!(!is_hash_dir_name(&"a".repeat(63)), "too short");
        assert!(!is_hash_dir_name(&"A".repeat(64)), "uppercase not hex");
        assert!(!is_hash_dir_name("hpm.toml"), "legacy flat entry");
        assert!(!is_hash_dir_name(".stage-1-0"), "hidden staging entry");
    }

    /// Committing a staged copy renames it into its hash home atomically.
    #[test]
    fn commit_staged_copy_moves_into_place() {
        let tmp = tempfile::TempDir::new().unwrap();
        let container = tmp.path().join("slug@1.0.0");
        std::fs::create_dir_all(&container).unwrap();
        let staged = stage_dir(&container);
        write(&staged.join("hpm.toml"), "name = 'x'");
        let target = dev_copy_target(&container, &"a".repeat(64));

        commit_staged_copy(&staged, &target).unwrap();
        assert!(dev_copy_is_complete(&target), "target holds the manifest");
        assert!(!staged.exists(), "staging dir consumed by the rename");
    }

    /// Losing the race — the target hash already exists complete — is success,
    /// and our stage is cleaned up rather than clobbering the winner.
    #[test]
    fn commit_staged_copy_tolerates_existing_target() {
        let tmp = tempfile::TempDir::new().unwrap();
        let container = tmp.path().join("slug@1.0.0");
        std::fs::create_dir_all(&container).unwrap();
        let target = dev_copy_target(&container, &"b".repeat(64));
        write(&target.join("hpm.toml"), "name = 'winner'");

        let staged = stage_dir(&container);
        write(&staged.join("hpm.toml"), "name = 'loser'");

        commit_staged_copy(&staged, &target).unwrap();
        assert!(!staged.exists(), "losing stage is cleaned up");
        assert_eq!(
            std::fs::read_to_string(target.join("hpm.toml")).unwrap(),
            "name = 'winner'",
            "the winner's content is preserved",
        );
    }

    /// A prior dev *link* at the container is cleared so a copy can take over;
    /// a real directory of hashes is never removed.
    #[cfg(unix)]
    #[test]
    fn clear_container_link_removes_link_but_keeps_directory() {
        let tmp = tempfile::TempDir::new().unwrap();
        let workspace = tmp.path().join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();

        // Link case: container is a symlink to the workspace.
        let link_container = tmp.path().join("linked@1.0.0");
        std::os::unix::fs::symlink(&workspace, &link_container).unwrap();
        clear_container_link(&link_container).unwrap();
        assert!(!link_container.exists(), "stale link removed");
        assert!(workspace.exists(), "link target (workspace) untouched");

        // Directory case: container holds a live hash copy and must survive.
        let dir_container = tmp.path().join("copied@1.0.0");
        let hash = "a".repeat(64);
        write(&dir_container.join(&hash).join("hpm.toml"), "name = 'x'");
        clear_container_link(&dir_container).unwrap();
        assert!(dir_container.join(&hash).exists(), "hash copy preserved");
    }

    mod properties {
        use super::*;
        use crate::packer::prop_support::{
            STAGE_EXCLUDES, STAGE_INCLUDES, file_tree, glob_subset, ignore_file, write_tree,
        };
        use proptest::prelude::*;
        use std::collections::BTreeSet;

        fn manifest_toml(include: &[String], exclude: &[String]) -> String {
            let list = |v: &[String]| {
                v.iter()
                    .map(|g| format!("{g:?}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            format!(
                "[package]\npath = \"creator/x\"\nname = \"X\"\nversion = \"1.0.0\"\n\n\
                 [stage]\ninclude = [{}]\nexclude = [{}]\n",
                list(include),
                list(exclude)
            )
        }

        proptest! {
            #![proptest_config(ProptestConfig::with_cases(96))]

            /// A dev copy carries exactly the files `hpm pack` would ship, plus
            /// `hpm.toml`, and its hash depends on those files only. Changing
            /// an excluded file leaves the hash (and so the install directory)
            /// alone; growing a staged file moves it. If the dev copy drifts
            /// from pack's file set, or the hash starts covering ignored files
            /// again (the full-workspace recopy that outlasted Desktop's launch
            /// request), this fails.
            #[test]
            fn prop_dev_copy_image_is_pack_file_set(
                files in file_tree(),
                gitignore in ignore_file(),
                hpmignore in ignore_file(),
                include in glob_subset(STAGE_INCLUDES),
                exclude in glob_subset(STAGE_EXCLUDES),
            ) {
                let tmp = tempfile::TempDir::new().unwrap();
                let src = tmp.path();
                write_tree(src, &files);
                std::fs::write(src.join(".gitignore"), &gitignore).unwrap();
                std::fs::write(src.join(".hpmignore"), &hpmignore).unwrap();
                std::fs::write(src.join("hpm.toml"), manifest_toml(&include, &exclude)).unwrap();
                let manifest = PackageManifest::from_path(&src.join("hpm.toml")).unwrap();

                // What pack stages, computed the way `hpm pack` does.
                let stage = manifest.stage.resolved_for_profile(DEV_COPY_PROFILE);
                let filter = StageFilter::new(&stage, None).unwrap();
                let ignore = build_ignore_rules(src).unwrap();
                let mut expected: BTreeSet<String> =
                    collect_stage_entries(src, &ignore, Some(&filter), None)
                        .unwrap()
                        .into_iter()
                        .map(|(_, dest)| dest)
                        .collect();
                expected.insert("hpm.toml".to_string());

                let image = dev_copy_image(src, &manifest).unwrap();
                let dests: Vec<&String> = image.entries.iter().map(|(_, d)| d).collect();
                prop_assert!(dests.windows(2).all(|w| w[0] < w[1]), "entries sorted and unique");
                let got: BTreeSet<String> = dests.into_iter().cloned().collect();
                prop_assert_eq!(&got, &expected);
                prop_assert!(is_hash_dir_name(&image.hash));

                // Grow every file: excluded ones must not move the hash, staged
                // ones (other than hpm.toml, which is always present) must.
                let staged_payload = files.iter().any(|f| got.contains(f));
                for rel in files.iter().filter(|f| !got.contains(*f)) {
                    std::fs::write(src.join(rel), format!("{rel}-rebuilt-and-longer")).unwrap();
                }
                let after_excluded = dev_copy_image(src, &manifest).unwrap();
                prop_assert_eq!(&after_excluded.hash, &image.hash, "excluded file changed the hash");

                for rel in files.iter().filter(|f| got.contains(*f)) {
                    std::fs::write(src.join(rel), format!("{rel}-rebuilt-and-longer")).unwrap();
                }
                let after_staged = dev_copy_image(src, &manifest).unwrap();
                if staged_payload {
                    prop_assert_ne!(&after_staged.hash, &image.hash, "staged file change missed");
                }
            }
        }
    }
}
