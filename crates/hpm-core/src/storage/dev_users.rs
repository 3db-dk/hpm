//! Who uses a content-addressed dev copy, and reclaiming the copies nobody
//! uses.
//!
//! A dev install lands at `_dev/<slug>@<version>/<hash>/`, a new `<hash>` each
//! time a shipped file changes (a rebuilt DSO, a saved HDA). Before this
//! module nothing reclaimed the superseded ones unless the user ran
//! `hpm clean`, and one machine collected ~490 of them, about 500 GB.
//!
//! A copy is kept while something uses it, and each user is recorded in a
//! file *beside* the copy, under `<container>/.users/<hash>/`, never inside
//! it: an embedder copies the dev copy onwards (into a per-session tree), and a
//! record inside would travel with it. Two kinds of user:
//!
//! - **A project**, recorded when its generated Houdini manifest is written
//!   with `hpath` pointing at the copy. It uses the copy while that manifest
//!   still points there; once a later sync repoints it, the record is dead
//!   without anyone having to remove it.
//! - **A process** (hython, a package script, an install in flight), recorded
//!   with its PID and OS start time and judged by
//!   [`ProcessRecord::is_live`](crate::process_users::ProcessRecord::is_live).
//!   Embedders record the processes they spawn against a copy with
//!   [`record_process_user`]; the install records itself between committing a
//!   copy and the project's manifest pointing at it ([`DevCopyHold`]), which
//!   is the window in which nothing else names the new copy.
//!
//! [`reclaim_dev_copies`] removes every hash with no live user, renaming it
//! aside first: on Windows a directory with a file open inside cannot be
//! renamed, so a copy some unrecorded process still reads is left whole
//! rather than half-deleted.

use crate::process_users::{ProcessRecord, ProcessState};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use tracing::{debug, info, warn};

#[cfg(test)]
use super::dev_install::dev_copy_is_complete;
use super::dev_install::{is_hash_dir_name, is_link_entry};

/// Directory under a dev container holding one record directory per hash.
/// Hidden and not hash-shaped, so nothing that walks hash copies sees it.
pub const USERS_DIR: &str = ".users";

/// Prefix of a hash directory renamed aside for deletion. A tombstone left by
/// a deletion that failed is removed by the next reclaim.
const TOMBSTONE_PREFIX: &str = ".reclaim-";

const PROCESS_PREFIX: &str = "process-";
const PROJECT_PREFIX: &str = "project-";

/// Split a dev copy's install path into its container and hash, or `None`
/// when `install_path` is not a content copy (a link install, a registry
/// install), which has no copies to protect.
fn split(install_path: &Path) -> Option<(&Path, &str)> {
    let hash = install_path.file_name()?.to_str()?;
    if !is_hash_dir_name(hash) {
        return None;
    }
    Some((install_path.parent()?, hash))
}

/// The container of the content copy at `install_path`, or `None` when it is
/// not a content copy.
pub(crate) fn container_of(install_path: &Path) -> Option<&Path> {
    split(install_path).map(|(container, _)| container)
}

fn records_dir(container: &Path, hash: &str) -> PathBuf {
    container.join(USERS_DIR).join(hash)
}

fn write_record(dir: &Path, name: &str, content: &[u8]) -> io::Result<()> {
    std::fs::create_dir_all(dir)?;
    hpm_package::atomic_write(&dir.join(name), content).map_err(io::Error::other)
}

/// Record `user` as a process reading the dev copy at `install_path`, so
/// [`reclaim_dev_copies`] keeps the copy while that process runs. Returns
/// `false` without writing anything when `install_path` is not a content
/// copy. Call [`release_process_user`] when the process exits; a record left
/// behind by a crash is harmless, since a dead process uses nothing.
pub fn record_process_user(install_path: &Path, user: ProcessRecord) -> io::Result<bool> {
    let Some((container, hash)) = split(install_path) else {
        return Ok(false);
    };
    let json = serde_json::to_vec(&user).map_err(io::Error::other)?;
    write_record(
        &records_dir(container, hash),
        &format!("{PROCESS_PREFIX}{}.json", user.pid),
        &json,
    )?;
    Ok(true)
}

/// Remove the record [`record_process_user`] wrote for `pid`. Missing records
/// and non-copy paths are not errors.
pub fn release_process_user(install_path: &Path, pid: u32) -> io::Result<()> {
    let Some((container, hash)) = split(install_path) else {
        return Ok(());
    };
    let path = records_dir(container, hash).join(format!("{PROCESS_PREFIX}{pid}.json"));
    match std::fs::remove_file(path) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

/// Record that the Houdini manifest at `manifest_path` points at the dev copy
/// at `install_path`. The record names the manifest, never the copy: the
/// copy is in use exactly while that file says so ([`project_record_is_live`]).
pub(crate) fn record_project_user(install_path: &Path, manifest_path: &Path) -> io::Result<bool> {
    let Some((container, hash)) = split(install_path) else {
        return Ok(false);
    };
    let manifest = std::path::absolute(manifest_path)?;
    let key = {
        let digest = Sha256::digest(manifest.to_string_lossy().as_bytes());
        digest
            .iter()
            .take(8)
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    };
    let json = serde_json::to_vec(&serde_json::json!({ "manifest": manifest }))
        .map_err(io::Error::other)?;
    write_record(
        &records_dir(container, hash),
        &format!("{PROJECT_PREFIX}{key}.json"),
        &json,
    )?;
    Ok(true)
}

/// Whether the manifest a project record names still points `hpath` at
/// `copy`. A manifest that is gone (the project or the package was removed)
/// uses nothing; one that cannot be read or parsed is kept, since a copy kept
/// too long costs disk and one removed too early costs a project.
fn project_record_is_live(record: &serde_json::Value, copy: &Path) -> bool {
    let Some(manifest) = record.get("manifest").and_then(|m| m.as_str()) else {
        return true;
    };
    let text = match std::fs::read_to_string(manifest) {
        Ok(t) => t,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return false,
        Err(_) => return true,
    };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) else {
        return true;
    };
    let points_here =
        |v: &serde_json::Value| v.as_str().is_some_and(|s| same_path(Path::new(s), copy));
    match json.get("hpath") {
        Some(serde_json::Value::Array(items)) => items.iter().any(points_here),
        Some(v) => points_here(v),
        None => false,
    }
}

fn same_path(a: &Path, b: &Path) -> bool {
    a == b
        || matches!(
            (std::fs::canonicalize(a), std::fs::canonicalize(b)),
            (Ok(x), Ok(y)) if x == y
        )
}

/// Whether anything still uses `<container>/<hash>`. Dead records are
/// removed on the way, so a records directory only ever holds live users and
/// the ones nobody could judge.
pub(crate) fn hash_in_use(
    container: &Path,
    hash: &str,
    state_of: &impl Fn(u32) -> ProcessState,
) -> bool {
    let dir = records_dir(container, hash);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return false;
    };
    let copy = container.join(hash);
    let mut in_use = false;
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        let live = match std::fs::read(&path) {
            Ok(bytes) => {
                if name.starts_with(PROCESS_PREFIX) {
                    serde_json::from_slice::<ProcessRecord>(&bytes)
                        .map(|rec| rec.is_live(state_of))
                        .unwrap_or(true)
                } else if name.starts_with(PROJECT_PREFIX) {
                    serde_json::from_slice::<serde_json::Value>(&bytes)
                        .map(|rec| project_record_is_live(&rec, &copy))
                        .unwrap_or(true)
                } else {
                    // Written by something newer than this build: keep.
                    true
                }
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => false,
            Err(_) => true,
        };
        if live {
            in_use = true;
        } else {
            let _ = std::fs::remove_file(&path);
        }
    }
    in_use
}

/// In-process count of holds per dev copy, so two installs of the same copy
/// in one process share one process record and the first to finish does not
/// release the other's protection.
static HOLDS: Mutex<Option<HashMap<PathBuf, usize>>> = Mutex::new(None);

/// This process's claim on a dev copy it has just installed or reused, from
/// the moment the install commits (or finds) the copy until the project's
/// manifest points at it, which is when the project record takes over. In
/// between, nothing else names the copy, and a reclaim in another sync, or in
/// another process, would see a copy with no users. Dropping the hold
/// releases it; holding it for longer only keeps a copy longer.
#[derive(Debug)]
pub struct DevCopyHold {
    install_path: PathBuf,
}

impl DevCopyHold {
    /// Hold the copy at `install_path`, or `None` when it is not a content
    /// copy. A record that cannot be written is logged and the hold still
    /// returned: it then protects within this process only, which costs at
    /// worst a recopy.
    pub fn acquire(install_path: &Path) -> Option<Self> {
        split(install_path)?;
        let mut guard = HOLDS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let count = guard
            .get_or_insert_with(HashMap::new)
            .entry(install_path.to_path_buf())
            .or_insert(0);
        *count += 1;
        if *count == 1
            && let Err(e) = record_process_user(install_path, ProcessRecord::current())
        {
            warn!(
                "Could not record this process as a user of {}: {}",
                install_path.display(),
                e
            );
        }
        Some(Self {
            install_path: install_path.to_path_buf(),
        })
    }

    /// Whether this process holds `install_path` right now.
    pub(crate) fn is_held(install_path: &Path) -> bool {
        let guard = HOLDS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        guard.as_ref().is_some_and(|m| m.contains_key(install_path))
    }
}

impl Drop for DevCopyHold {
    fn drop(&mut self) {
        let mut guard = HOLDS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(map) = guard.as_mut() else { return };
        let Some(count) = map.get_mut(&self.install_path) else {
            return;
        };
        *count -= 1;
        if *count == 0 {
            map.remove(&self.install_path);
            if let Err(e) = release_process_user(&self.install_path, std::process::id()) {
                debug!("Could not release {}: {}", self.install_path.display(), e);
            }
        }
    }
}

/// Holds an install took on behalf of a project sync, waiting for that
/// project's manifest to point at the copy ([`settle`]).
static PENDING: Mutex<Vec<DevCopyHold>> = Mutex::new(Vec::new());

/// Hold `install_path` until [`settle`] is called for it. The install calls
/// this as it commits or reuses a copy; the project sync settles it once the
/// manifest pointing at the copy is written and recorded. An install that no
/// project ever settles (an embedder calling the store directly, a sync that
/// failed halfway) keeps its copy until this process exits, which costs disk
/// for as long as the process runs and never a copy someone needs.
pub(crate) fn hold_until_settled(install_path: &Path) {
    if let Some(hold) = DevCopyHold::acquire(install_path) {
        PENDING
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(hold);
    }
}

/// Release every hold [`hold_until_settled`] took for `install_path`.
///
/// All of them, not one: the caller has just recorded a project whose
/// manifest points at the copy, so the copy has a user whatever other
/// installs of it are still in flight, and a hold left by a sync that failed
/// before reaching its manifest is collected here instead of lasting until
/// the process exits.
pub(crate) fn settle(install_path: &Path) {
    let holds: Vec<DevCopyHold> = {
        let mut pending = PENDING
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (settled, kept) = std::mem::take(&mut *pending)
            .into_iter()
            .partition(|h| h.install_path == install_path);
        *pending = kept;
        settled
    };
    // Dropped outside the PENDING lock: dropping takes HOLDS.
    drop(holds);
}

static TOMBSTONE_SEQ: AtomicU64 = AtomicU64::new(0);

/// What a reclaim does with a copy that has never had a user recorded: one
/// made before records existed. Its users are unknown, not absent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Unrecorded {
    /// Leave it. For a reclaim that knows one project only (the pass after a
    /// sync): another project, not synced since records began, may point at it.
    Keep,
    /// Remove it. For a pass that has first recorded every known project's
    /// copies ([`record_project_manifests`]), so that unrecorded means unused
    /// by any of them.
    Remove,
}

/// The dev copies the Houdini manifests in `packages_dir` (a project's
/// `.hpm/packages`) point at, each with the manifest that names it. Only
/// copies that exist, directly inside a `_dev/<slug>@<version>/` container.
fn project_dev_copies(packages_dir: &Path) -> Vec<(PathBuf, PathBuf)> {
    let Ok(entries) = std::fs::read_dir(packages_dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let manifest = entry.path();
        if manifest.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&manifest) else {
            continue;
        };
        let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        let hpaths: Vec<&str> = match json.get("hpath") {
            Some(serde_json::Value::Array(items)) => {
                items.iter().filter_map(|v| v.as_str()).collect()
            }
            Some(v) => v.as_str().into_iter().collect(),
            None => Vec::new(),
        };
        for hpath in hpaths {
            let copy = Path::new(hpath);
            let in_dev = split(copy).is_some()
                && copy
                    .parent()
                    .and_then(Path::parent)
                    .and_then(Path::file_name)
                    .is_some_and(|n| n == super::dev_install::DEV_INSTALL_DIR);
            if in_dev && copy.is_dir() {
                out.push((manifest.clone(), copy.to_path_buf()));
            }
        }
    }
    out
}

/// Record every dev copy the Houdini manifests in `packages_dir` point at,
/// as that project's. This is how projects last synced before records
/// existed come to be known; it changes nothing for a project synced since.
pub(crate) fn record_project_manifests(packages_dir: &Path) {
    for (manifest, copy) in project_dev_copies(packages_dir) {
        let _ = record_project_user(&copy, &manifest);
    }
}

/// Hold, for this process, every dev copy the project at `packages_dir`
/// currently points at. For an embedder about to run something against the
/// project that it will wait for (a headless hython): the project's own
/// record protects a copy only until the project's next sync repoints its
/// manifest, and a run can outlast that. Drop the holds when the run ends.
pub fn hold_project_dev_copies(packages_dir: &Path) -> Vec<DevCopyHold> {
    project_dev_copies(packages_dir)
        .iter()
        .filter_map(|(_, copy)| DevCopyHold::acquire(copy))
        .collect()
}

/// Record `user` as a process reading every dev copy the project at
/// `packages_dir` currently points at. For a process the embedder does not
/// wait for (a detached script). Nothing needs releasing: a record of a
/// process that has exited is dead, and the next reclaim removes it.
pub fn record_project_process(packages_dir: &Path, user: ProcessRecord) {
    for (_, copy) in project_dev_copies(packages_dir) {
        if let Err(e) = record_process_user(&copy, user) {
            warn!(
                "Could not record process {} as a user of {}: {}",
                user.pid,
                copy.display(),
                e
            );
        }
    }
}

/// Remove every content copy in `container` that nothing uses, except
/// `keep` (`hpm clean` passes the current workspace's hash so an unchanged
/// relaunch does not recopy). Returns the number of copies removed.
///
/// Each removal renames the copy to a hidden tombstone and deletes that; a
/// copy that cannot be renamed (on Windows: something has a file open in it)
/// is kept whole. Tombstones a failed deletion left behind are retried here,
/// and record directories whose copy is gone are dropped. Staging
/// directories of installs in flight (`.stage-*`) are never touched.
pub(crate) fn reclaim_dev_copies(
    container: &Path,
    keep: Option<&str>,
    unrecorded: Unrecorded,
    state_of: impl Fn(u32) -> ProcessState,
) -> usize {
    reclaim_with(container, keep, unrecorded, state_of, &|_| {})
}

/// [`reclaim_dev_copies`], with `after_decision` called for each copy judged
/// unused, before it is moved aside: the window in which an install may
/// claim it. Tests put a claim there; production passes nothing.
fn reclaim_with(
    container: &Path,
    keep: Option<&str>,
    unrecorded: Unrecorded,
    state_of: impl Fn(u32) -> ProcessState,
    after_decision: &dyn Fn(&Path),
) -> usize {
    // A container that is a link (a `DevLink` install) points into the
    // user's workspace: never walk it, let alone delete from it.
    match std::fs::symlink_metadata(container) {
        Ok(meta) if meta.is_dir() && matches!(is_link_entry(&meta, container), Ok(false)) => {}
        _ => return 0,
    }
    let Ok(entries) = std::fs::read_dir(container) else {
        return 0;
    };
    let pid = std::process::id();
    let mut removed = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with(TOMBSTONE_PREFIX) {
            let _ = remove_tree(&path);
            continue;
        }
        if !is_hash_dir_name(&name) || Some(name.as_str()) == keep {
            continue;
        }
        // Copies are directories; anything else with a hash-shaped name is
        // not ours to judge.
        if !entry.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        if unrecorded == Unrecorded::Keep && !records_dir(container, &name).is_dir() {
            continue;
        }
        if DevCopyHold::is_held(&path) || hash_in_use(container, &name, &state_of) {
            continue;
        }
        after_decision(&path);
        let seq = TOMBSTONE_SEQ.fetch_add(1, Ordering::Relaxed);
        let tombstone = container.join(format!("{TOMBSTONE_PREFIX}{name}-{pid}-{seq}"));
        if let Err(e) = std::fs::rename(&path, &tombstone) {
            if e.kind() != io::ErrorKind::NotFound {
                warn!(
                    "Kept dev copy {}: it could not be moved aside, so something still has it open: {}",
                    path.display(),
                    e
                );
            }
            continue;
        }
        // Deciding and moving aside are two steps, and an install may claim
        // the copy between them: it takes its hold, then checks the copy is
        // complete, and reuses it. Its hold is in place before that check,
        // so asking again now sees every claim made before the copy moved;
        // a claimed copy goes back where the install found it.
        if DevCopyHold::is_held(&path) || hash_in_use(container, &name, &state_of) {
            if let Err(e) = std::fs::rename(&tombstone, &path) {
                warn!(
                    "Dev copy {} was claimed while being reclaimed and could not be put back: {}",
                    path.display(),
                    e
                );
            }
            continue;
        }
        let _ = std::fs::remove_dir_all(records_dir(container, &name));
        match remove_tree(&tombstone) {
            Ok(()) => removed += 1,
            Err(e) => warn!(
                "Failed to delete superseded dev copy {} (retried on the next reclaim): {}",
                tombstone.display(),
                e
            ),
        }
    }
    drop_orphan_records(container, &state_of);
    if removed > 0 {
        info!(
            "Reclaimed {} superseded dev {} in {}",
            removed,
            if removed == 1 { "copy" } else { "copies" },
            container.display()
        );
    }
    removed
}

/// Remove record directories whose copy no longer exists and that hold no
/// live user. A live process record for a missing copy is an install in
/// flight: it holds the hash before committing the copy, so the copy is
/// protected from the moment it appears.
fn drop_orphan_records(container: &Path, state_of: &impl Fn(u32) -> ProcessState) {
    let users = container.join(USERS_DIR);
    let Ok(entries) = std::fs::read_dir(&users) else {
        return;
    };
    for entry in entries.flatten() {
        let hash = entry.file_name().to_string_lossy().into_owned();
        if !container.join(&hash).exists() && !hash_in_use(container, &hash, state_of) {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
    let _ = std::fs::remove_dir(&users);
}

/// Whether `container` holds nothing but records: no copy, no install in
/// flight, no tombstone. Such a container can go with its package.
pub(crate) fn container_is_empty(container: &Path) -> bool {
    std::fs::read_dir(container)
        .is_ok_and(|entries| entries.flatten().all(|e| e.file_name() == USERS_DIR))
}

/// `remove_dir_all`, retried once with every entry made writable. Windows
/// refuses to delete a read-only file (git's object files are read-only), and
/// a dev copy made before copies were filtered carried a whole `.git/`.
pub(crate) fn remove_tree(path: &Path) -> io::Result<()> {
    match std::fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(first) => {
            for entry in walkdir::WalkDir::new(path)
                .follow_links(false)
                .into_iter()
                .flatten()
            {
                if let Ok(meta) = entry.path().symlink_metadata() {
                    let mut perms = meta.permissions();
                    if perms.readonly() && !meta.file_type().is_symlink() {
                        #[allow(clippy::permissions_set_readonly_false)]
                        perms.set_readonly(false);
                        let _ = std::fs::set_permissions(entry.path(), perms);
                    }
                }
            }
            std::fs::remove_dir_all(path).map_err(|_| first)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process_users::process_state;
    use tempfile::TempDir;

    fn hash(c: char) -> String {
        std::iter::repeat_n(c, 64).collect()
    }

    fn make_copy(container: &Path, h: &str) -> PathBuf {
        let copy = container.join(h);
        std::fs::create_dir_all(copy.join("otls")).unwrap();
        std::fs::write(copy.join("hpm.toml"), "x").unwrap();
        std::fs::write(copy.join("otls/a.hda"), "x").unwrap();
        copy
    }

    fn write_manifest(path: &Path, hpath: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let json = serde_json::json!({ "hpath": [hpath.to_string_lossy()] });
        std::fs::write(path, serde_json::to_vec(&json).unwrap()).unwrap();
    }

    /// The whole keep rule over one container: a copy goes exactly when no
    /// user is live and it is not `keep`. Users: a live process, a dead
    /// process, a recycled PID, a project whose manifest points here, a
    /// project whose manifest moved on, a project whose manifest is gone,
    /// and no user at all.
    #[test]
    fn reclaim_keeps_exactly_the_copies_in_use() {
        let tmp = TempDir::new().unwrap();
        let c = tmp.path().join("_dev/rig@1.0.0");
        const LIVE: u32 = 10;
        const DEAD: u32 = 20;
        const RECYCLED: u32 = 30;
        const T: u64 = 1_758_000_000;
        let state_of = |pid: u32| match pid {
            LIVE => ProcessState::Alive {
                started_at: Some(T),
            },
            RECYCLED => ProcessState::Alive {
                started_at: Some(T + 3600),
            },
            _ => ProcessState::Gone,
        };

        let live_proc = make_copy(&c, &hash('1'));
        record_process_user(
            &live_proc,
            ProcessRecord {
                pid: LIVE,
                started_at: Some(T),
            },
        )
        .unwrap();
        let dead_proc = make_copy(&c, &hash('2'));
        record_process_user(
            &dead_proc,
            ProcessRecord {
                pid: DEAD,
                started_at: Some(T),
            },
        )
        .unwrap();
        let recycled = make_copy(&c, &hash('3'));
        record_process_user(
            &recycled,
            ProcessRecord {
                pid: RECYCLED,
                started_at: Some(T),
            },
        )
        .unwrap();

        let pointed = make_copy(&c, &hash('4'));
        let m1 = tmp.path().join("p1/.hpm/packages/s.rig.json");
        write_manifest(&m1, &pointed);
        record_project_user(&pointed, &m1).unwrap();

        let moved_on = make_copy(&c, &hash('5'));
        let m2 = tmp.path().join("p2/.hpm/packages/s.rig.json");
        record_project_user(&moved_on, &m2).unwrap();
        write_manifest(&m2, &pointed);

        let removed_project = make_copy(&c, &hash('6'));
        let m3 = tmp.path().join("p3/.hpm/packages/s.rig.json");
        write_manifest(&m3, &removed_project);
        record_project_user(&removed_project, &m3).unwrap();
        std::fs::remove_file(&m3).unwrap();

        let unused = make_copy(&c, &hash('7'));
        let kept_by_caller = make_copy(&c, &hash('8'));
        std::fs::create_dir_all(c.join(".stage-1-0/otls")).unwrap();
        std::fs::create_dir_all(c.join(".reclaim-old-1-0/otls")).unwrap();

        let removed = reclaim_dev_copies(&c, Some(&hash('8')), Unrecorded::Remove, state_of);

        assert!(live_proc.is_dir());
        assert!(!dead_proc.exists());
        assert!(!recycled.exists());
        assert!(pointed.is_dir());
        assert!(!moved_on.exists());
        assert!(!removed_project.exists());
        assert!(!unused.exists());
        assert!(kept_by_caller.is_dir());
        assert_eq!(removed, 5);
        assert!(
            c.join(".stage-1-0").is_dir(),
            "an install in flight is never touched"
        );
        assert!(
            !c.join(".reclaim-old-1-0").exists(),
            "leftover tombstones are retried"
        );
        let records: Vec<_> = std::fs::read_dir(c.join(USERS_DIR))
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        let mut records = records;
        records.sort();
        assert_eq!(
            records,
            vec![hash('1'), hash('4')],
            "only live copies keep records"
        );
    }

    /// A hold protects a copy across the gap between its install and the
    /// project's manifest, also against a reclaim that judges records only,
    /// and two holds in one process release it only when both are gone.
    #[test]
    fn holds_protect_until_the_last_is_dropped() {
        let tmp = TempDir::new().unwrap();
        let c = tmp.path().join("_dev/rig@1.0.0");
        let copy = make_copy(&c, &hash('a'));

        let first = DevCopyHold::acquire(&copy).unwrap();
        let second = DevCopyHold::acquire(&copy).unwrap();
        assert!(
            hash_in_use(&c, &hash('a'), &process_state),
            "the record names this process"
        );
        drop(first);
        reclaim_dev_copies(&c, None, Unrecorded::Remove, process_state);
        assert!(copy.is_dir(), "one hold left");
        drop(second);
        assert!(
            !hash_in_use(&c, &hash('a'), &process_state),
            "released with the last hold"
        );
        reclaim_dev_copies(&c, None, Unrecorded::Remove, process_state);
        assert!(!copy.exists());
    }

    /// An install's hold lasts until the project settles it, and settling a
    /// copy nobody holds is harmless.
    #[test]
    fn pending_hold_lasts_until_settled() {
        let tmp = TempDir::new().unwrap();
        let c = tmp.path().join("_dev/rig@1.0.0");
        let copy = make_copy(&c, &hash('b'));
        hold_until_settled(&copy);
        reclaim_dev_copies(&c, None, Unrecorded::Remove, process_state);
        assert!(copy.is_dir());
        settle(&copy);
        settle(&copy);
        reclaim_dev_copies(&c, None, Unrecorded::Remove, process_state);
        assert!(!copy.exists());
    }

    /// A copy from before records existed is kept by a one-project reclaim
    /// and, once every project's manifests are recorded, removed exactly
    /// when none of them points at it.
    #[test]
    fn unrecorded_copies_wait_for_a_pass_that_knows_every_project() {
        let tmp = TempDir::new().unwrap();
        let c = tmp.path().join("_dev/rig@1.0.0");
        let pointed = make_copy(&c, &hash('1'));
        let unused = make_copy(&c, &hash('2'));
        let packages = tmp.path().join("p/.hpm/packages");
        write_manifest(&packages.join("s.rig.json"), &pointed);

        assert_eq!(
            reclaim_dev_copies(&c, None, Unrecorded::Keep, process_state),
            0
        );
        assert!(pointed.is_dir() && unused.is_dir());

        record_project_manifests(&packages);
        assert_eq!(
            reclaim_dev_copies(&c, None, Unrecorded::Remove, process_state),
            1
        );
        assert!(pointed.is_dir());
        assert!(!unused.exists());
    }

    /// A run against a project outlives the project's next sync: the copy
    /// it started on is kept by the hold (a run the embedder waits for) or
    /// by the process record (a detached one), not by the manifest.
    #[test]
    fn a_run_keeps_its_copy_after_the_project_moves_on() {
        let tmp = TempDir::new().unwrap();
        let c = tmp.path().join("_dev/rig@1.0.0");
        let old = make_copy(&c, &hash('1'));
        let new = make_copy(&c, &hash('2'));
        let packages = tmp.path().join("p/.hpm/packages");
        let manifest = packages.join("s.rig.json");
        write_manifest(&manifest, &old);
        record_project_user(&old, &manifest).unwrap();

        // The run's own process, distinct from this one (which holds).
        const RUN: u32 = 10;
        let run_alive = std::cell::Cell::new(true);
        let state_of = |pid: u32| match pid {
            RUN if run_alive.get() => ProcessState::Alive { started_at: None },
            RUN => ProcessState::Gone,
            other => process_state(other),
        };
        let holds = hold_project_dev_copies(&packages);
        assert_eq!(holds.len(), 1);
        record_project_process(
            &packages,
            ProcessRecord {
                pid: RUN,
                started_at: None,
            },
        );

        // The project syncs onto the new copy.
        write_manifest(&manifest, &new);
        record_project_user(&new, &manifest).unwrap();
        reclaim_dev_copies(&c, None, Unrecorded::Remove, state_of);
        assert!(old.is_dir(), "held");
        drop(holds);
        reclaim_dev_copies(&c, None, Unrecorded::Remove, state_of);
        assert!(old.is_dir(), "the recorded process still runs");
        run_alive.set(false);
        reclaim_dev_copies(&c, None, Unrecorded::Remove, state_of);
        assert!(!old.exists());
        assert!(new.is_dir());
    }

    /// An install that reuses a copy takes its hold and then checks the copy
    /// is complete; a reclaim decides a copy is unused and then moves it
    /// aside. Interleaved (decide, hold, check, move) the install points a
    /// project at a copy that is then deleted. The reclaim must notice the
    /// claim that arrived after its decision and leave the copy in place.
    #[test]
    fn a_copy_claimed_after_the_decision_is_not_removed() {
        let tmp = TempDir::new().unwrap();
        let c = tmp.path().join("_dev/rig@1.0.0");
        let copy = make_copy(&c, &hash('e'));
        let claimed = std::cell::RefCell::new(Vec::new());
        let removed = reclaim_with(&c, None, Unrecorded::Remove, process_state, &|p| {
            // The install, between the reclaim's decision and its rename.
            claimed.borrow_mut().push(DevCopyHold::acquire(p).unwrap());
            assert!(dev_copy_is_complete(p));
        });
        assert_eq!(removed, 0);
        assert!(
            copy.join("otls/a.hda").is_file(),
            "the claimed copy is intact"
        );
        assert!(!claimed.borrow().is_empty());
    }

    /// The same window, with the claim made by another process: only its
    /// record shows, no in-process hold.
    #[test]
    fn a_copy_claimed_by_another_process_after_the_decision_is_not_removed() {
        let tmp = TempDir::new().unwrap();
        let c = tmp.path().join("_dev/rig@1.0.0");
        let copy = make_copy(&c, &hash('f'));
        const OTHER: u32 = 77;
        let state_of = |pid: u32| match pid {
            OTHER => ProcessState::Alive { started_at: None },
            _ => ProcessState::Gone,
        };
        reclaim_with(&c, None, Unrecorded::Remove, state_of, &|p| {
            record_process_user(
                p,
                ProcessRecord {
                    pid: OTHER,
                    started_at: None,
                },
            )
            .unwrap();
        });
        assert!(copy.join("otls/a.hda").is_file());
    }

    #[test]
    fn non_copy_paths_are_not_recorded() {
        let tmp = TempDir::new().unwrap();
        let link = tmp.path().join("_dev/rig@1.0.0");
        assert!(!record_process_user(&link, ProcessRecord::current()).unwrap());
        assert!(DevCopyHold::acquire(&link).is_none());
        release_process_user(&link, 1).unwrap();
    }

    /// A link container points into a workspace, where a 64-hex directory
    /// is the user's own: reclaim walks nothing behind a link.
    #[cfg(unix)]
    #[test]
    fn reclaim_never_follows_a_link_container() {
        let tmp = TempDir::new().unwrap();
        let workspace = tmp.path().join("workspace");
        let theirs = make_copy(&workspace, &hash('c'));
        std::fs::create_dir_all(tmp.path().join("_dev")).unwrap();
        let link = tmp.path().join("_dev/rig@1.0.0");
        std::os::unix::fs::symlink(&workspace, &link).unwrap();
        assert_eq!(
            reclaim_dev_copies(&link, None, Unrecorded::Remove, process_state),
            0
        );
        assert!(theirs.is_dir());
    }

    /// An install holds its hash before committing the copy; a reclaim in
    /// that window must keep the record, so the copy is protected the moment
    /// it appears.
    #[test]
    fn a_hold_taken_before_the_copy_exists_survives_a_reclaim() {
        let tmp = TempDir::new().unwrap();
        let c = tmp.path().join("_dev/rig@1.0.0");
        std::fs::create_dir_all(&c).unwrap();
        let target = c.join(hash('d'));
        let hold = DevCopyHold::acquire(&target).unwrap();
        // Judged by records only, as another process would.
        reclaim_dev_copies(&c, None, Unrecorded::Remove, process_state);
        assert!(records_dir(&c, &hash('d')).is_dir());
        make_copy(&c, &hash('d'));
        assert!(hash_in_use(&c, &hash('d'), &process_state));
        drop(hold);
    }

    #[test]
    fn remove_tree_deletes_read_only_files() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("t");
        std::fs::create_dir_all(dir.join(".git/objects/ab")).unwrap();
        let obj = dir.join(".git/objects/ab/cdef");
        std::fs::write(&obj, "x").unwrap();
        let mut perms = std::fs::metadata(&obj).unwrap().permissions();
        perms.set_readonly(true);
        std::fs::set_permissions(&obj, perms).unwrap();
        remove_tree(&dir).unwrap();
        assert!(!dir.exists());
    }

    #[test]
    fn empty_container_means_records_only() {
        let tmp = TempDir::new().unwrap();
        let c = tmp.path().join("_dev/rig@1.0.0");
        std::fs::create_dir_all(c.join(USERS_DIR)).unwrap();
        assert!(container_is_empty(&c));
        std::fs::create_dir_all(c.join(".stage-1-0")).unwrap();
        assert!(!container_is_empty(&c));
    }

    mod props {
        use super::*;
        use proptest::prelude::*;
        use std::collections::{BTreeMap, BTreeSet};

        /// What the OS says about a recorded PID.
        #[derive(Debug, Clone, Copy)]
        enum Os {
            Gone,
            Unknown,
            Same,
            Recycled,
        }

        /// One user of one copy.
        #[derive(Debug, Clone)]
        enum User {
            /// A recorded process, judged through the shared PID table.
            Process(u32),
            /// A project record; its manifest may point here or elsewhere,
            /// or be gone.
            Project {
                points_here: bool,
                manifest_exists: bool,
            },
            /// A record nobody can parse: judged live.
            Garbage,
        }

        #[derive(Debug, Clone)]
        struct Case {
            /// Per copy, its users (empty: never recorded).
            copies: Vec<Vec<User>>,
            pids: BTreeMap<u32, Os>,
            keep: Option<usize>,
            unrecorded: bool,
            hash_named_file: bool,
        }

        const T: u64 = 1_758_000_000;

        fn user() -> impl Strategy<Value = User> {
            prop_oneof![
                (1u32..5).prop_map(User::Process),
                (any::<bool>(), any::<bool>()).prop_map(|(points_here, manifest_exists)| {
                    User::Project {
                        points_here,
                        manifest_exists,
                    }
                }),
                Just(User::Garbage),
            ]
        }

        fn case() -> impl Strategy<Value = Case> {
            (
                prop::collection::vec(prop::collection::vec(user(), 0..4), 1..7),
                prop::collection::btree_map(
                    1u32..5,
                    prop_oneof![
                        Just(Os::Gone),
                        Just(Os::Unknown),
                        Just(Os::Same),
                        Just(Os::Recycled)
                    ],
                    0..5,
                ),
                any::<Option<prop::sample::Index>>(),
                any::<bool>(),
                any::<bool>(),
            )
                .prop_map(|(copies, pids, keep, unrecorded, hash_named_file)| {
                    let keep = keep.map(|i| i.index(copies.len()));
                    Case {
                        copies,
                        pids,
                        keep,
                        unrecorded,
                        hash_named_file,
                    }
                })
        }

        fn copy_hash(i: usize) -> String {
            format!("{:064x}", i + 1)
        }

        /// The rule, stated independently of the code: a copy is kept when it
        /// is `keep`, or never recorded under `Unrecorded::Keep`, or has a
        /// live user.
        fn kept_by_model(c: &Case) -> BTreeSet<usize> {
            let os = |pid: u32| c.pids.get(&pid).copied().unwrap_or(Os::Gone);
            (0..c.copies.len())
                .filter(|&i| {
                    let users = &c.copies[i];
                    Some(i) == c.keep
                        || (c.unrecorded && users.is_empty())
                        || users.iter().any(|u| match u {
                            User::Process(pid) => matches!(os(*pid), Os::Unknown | Os::Same),
                            User::Project {
                                points_here,
                                manifest_exists,
                            } => *points_here && *manifest_exists,
                            User::Garbage => true,
                        })
                })
                .collect()
        }

        proptest! {
            #![proptest_config(ProptestConfig::with_cases(128))]

            /// Reclaim removes exactly the copies the rule says nobody uses,
            /// and changes nothing else: kept copies keep their content, an
            /// install in flight and a hash-named file are untouched, nothing
            /// outside the container is touched, no tombstone is left, and
            /// no removed copy leaves records behind.
            #[test]
            fn prop_reclaim_removes_exactly_the_unused_copies(c in case()) {
                let tmp = TempDir::new().unwrap();
                let c_dir = tmp.path().join("_dev/rig@1.0.0");
                let outside = tmp.path().join("outside.txt");
                std::fs::write(&outside, "mine").unwrap();
                let elsewhere = tmp.path().join("elsewhere");
                for (i, users) in c.copies.iter().enumerate() {
                    let copy = make_copy(&c_dir, &copy_hash(i));
                    std::fs::write(copy.join("id"), i.to_string()).unwrap();
                    for (n, u) in users.iter().enumerate() {
                        match u {
                            User::Process(pid) => {
                                record_process_user(&copy, ProcessRecord { pid: *pid, started_at: Some(T) })
                                    .unwrap();
                            }
                            User::Project { points_here, manifest_exists } => {
                                let m = tmp.path().join(format!("p{i}-{n}/.hpm/packages/s.rig.json"));
                                write_manifest(&m, if *points_here { &copy } else { &elsewhere });
                                record_project_user(&copy, &m).unwrap();
                                if !manifest_exists {
                                    std::fs::remove_file(&m).unwrap();
                                }
                            }
                            User::Garbage => {
                                let dir = records_dir(&c_dir, &copy_hash(i));
                                std::fs::create_dir_all(&dir).unwrap();
                                std::fs::write(dir.join(format!("process-garbage{n}.json")), "{").unwrap();
                            }
                        }
                    }
                }
                std::fs::create_dir_all(c_dir.join(".stage-9-9/otls")).unwrap();
                let file_hash = format!("{:064x}", 0xfeedu32);
                if c.hash_named_file {
                    std::fs::write(c_dir.join(&file_hash), "not a copy").unwrap();
                }
                let pids = c.pids.clone();
                let state_of = move |pid: u32| match pids.get(&pid).copied().unwrap_or(Os::Gone) {
                    Os::Gone => ProcessState::Gone,
                    Os::Unknown => ProcessState::Unknown,
                    Os::Same => ProcessState::Alive { started_at: Some(T) },
                    Os::Recycled => ProcessState::Alive { started_at: Some(T + 60) },
                };
                let keep_hash = c.keep.map(copy_hash);
                let mode = if c.unrecorded { Unrecorded::Keep } else { Unrecorded::Remove };

                let removed = reclaim_dev_copies(&c_dir, keep_hash.as_deref(), mode, state_of);

                let want = kept_by_model(&c);
                let got: BTreeSet<usize> =
                    (0..c.copies.len()).filter(|&i| c_dir.join(copy_hash(i)).exists()).collect();
                prop_assert_eq!(&got, &want);
                prop_assert_eq!(removed, c.copies.len() - want.len());
                for i in &got {
                    let copy = c_dir.join(copy_hash(*i));
                    prop_assert_eq!(std::fs::read_to_string(copy.join("id")).unwrap(), i.to_string());
                    prop_assert!(copy.join("otls/a.hda").is_file());
                }
                for i in (0..c.copies.len()).filter(|i| !got.contains(i)) {
                    prop_assert!(!records_dir(&c_dir, &copy_hash(i)).exists());
                }
                prop_assert!(c_dir.join(".stage-9-9/otls").is_dir());
                if c.hash_named_file {
                    prop_assert_eq!(std::fs::read_to_string(c_dir.join(&file_hash)).unwrap(), "not a copy");
                }
                prop_assert_eq!(std::fs::read_to_string(&outside).unwrap(), "mine");
                let tombstones = std::fs::read_dir(&c_dir)
                    .unwrap()
                    .flatten()
                    .filter(|e| e.file_name().to_string_lossy().starts_with(TOMBSTONE_PREFIX))
                    .count();
                prop_assert_eq!(tombstones, 0);
            }
        }

        #[derive(Debug, Clone, Copy)]
        enum Op {
            Acquire(usize),
            DropOne(usize),
            HoldUntilSettled(usize),
            Settle(usize),
        }

        fn op() -> impl Strategy<Value = Op> {
            prop_oneof![
                (0usize..3).prop_map(Op::Acquire),
                (0usize..3).prop_map(Op::DropOne),
                (0usize..3).prop_map(Op::HoldUntilSettled),
                (0usize..3).prop_map(Op::Settle),
            ]
        }

        proptest! {
            #![proptest_config(ProptestConfig::with_cases(128))]

            /// Over any sequence of holds and releases on a few copies, a
            /// copy's record names this process exactly while some hold on
            /// it is outstanding, direct or pending: one release never frees
            /// another holder's protection, and the last release always does.
            #[test]
            fn prop_record_exists_exactly_while_held(ops in prop::collection::vec(op(), 0..40)) {
                let tmp = TempDir::new().unwrap();
                let c = tmp.path().join("_dev/rig@1.0.0");
                let copies: Vec<PathBuf> = (0..3).map(|i| make_copy(&c, &copy_hash(i))).collect();
                let mut direct: Vec<Vec<DevCopyHold>> = vec![Vec::new(), Vec::new(), Vec::new()];
                let mut pending = [0usize; 3];
                let record = |i: usize| {
                    records_dir(&c, &copy_hash(i)).join(format!("{PROCESS_PREFIX}{}.json", std::process::id()))
                };
                for op in ops {
                    match op {
                        Op::Acquire(i) => direct[i].push(DevCopyHold::acquire(&copies[i]).unwrap()),
                        Op::DropOne(i) => drop(direct[i].pop()),
                        Op::HoldUntilSettled(i) => {
                            hold_until_settled(&copies[i]);
                            pending[i] += 1;
                        }
                        Op::Settle(i) => {
                            settle(&copies[i]);
                            pending[i] = 0;
                        }
                    }
                    for i in 0..3 {
                        let held = !direct[i].is_empty() || pending[i] > 0;
                        prop_assert_eq!(record(i).is_file(), held, "copy {} after {:?}", i, op);
                        prop_assert_eq!(DevCopyHold::is_held(&copies[i]), held);
                    }
                }
                for (i, p) in copies.iter().enumerate() {
                    if pending[i] > 0 {
                        settle(p);
                    }
                }
            }
        }
    }
}
