//! Deterministic SHA-256 content hash of a directory tree.
//!
//! This is the digest recorded in `hpm.lock` at install time and recomputed
//! by `LockFile::verify_checksums` — the two sides must agree byte-for-byte,
//! so there is exactly one implementation.
//!
//! Two formats exist. [`hash_tree`] writes `v2:<hex>`, which frames every
//! path and every file's contents with their lengths. The first format, a
//! bare `<hex>`, concatenated paths and contents with no boundary, so two
//! different trees could feed the hash the same bytes (file `a` holding `bc`
//! and file `ab` holding `c`). Lockfiles written before v2 still hold it, so
//! [`hash_tree_as`] recomputes a tree in whichever format a lock recorded.

use hpm_package::IoOp;
use hpm_package::path_util::relative_path_to_forward_slash;
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};

/// Name of the checksum cache file older hpm versions wrote into package
/// directories. Skipped so a stray leftover can't perturb the digest.
pub const CHECKSUM_CACHE_FILE: &str = ".hpm-checksum";

/// Prefix of a [`hash_tree`] digest.
pub const V2_PREFIX: &str = "v2:";

/// The digest new lockfile entries record: `v2:` and the hex SHA-256 over
/// a format tag, then every file in sorted order as its forward-slash
/// relative path and its contents, each preceded by its length. Walk and
/// read errors propagate — a digest computed over a silently truncated tree
/// would misreport the package hash.
pub fn hash_tree(dir: &Path) -> Result<String, IoOp> {
    let mut hasher = Sha256::new();
    hasher.update(b"hpm-tree-v2\0");
    for path in files(dir)? {
        let relative = relative_path_to_forward_slash(path.strip_prefix(dir).unwrap_or(&path));
        hasher.update((relative.len() as u64).to_le_bytes());
        hasher.update(relative.as_bytes());
        let len = std::fs::metadata(&path)
            .map_err(|e| IoOp::wrap("stat file for checksum", &path, e))?
            .len();
        hasher.update(len.to_le_bytes());
        let read = feed(&mut hasher, &path)?;
        if read != len {
            return Err(IoOp::wrap(
                "read file for checksum",
                &path,
                std::io::Error::other("the file changed size while it was hashed"),
            ));
        }
    }
    Ok(format!("{V2_PREFIX}{}", crate::digest::finish_hex(hasher)))
}

/// `dir`'s digest in the format `recorded` uses: [`hash_tree`] for a `v2:`
/// digest, the first, unframed format for a bare hex one, so a lock written
/// before v2 still verifies.
pub fn hash_tree_as(dir: &Path, recorded: &str) -> Result<String, IoOp> {
    if recorded.starts_with(V2_PREFIX) {
        hash_tree(dir)
    } else {
        hash_tree_unframed(dir)
    }
}

/// The first format: paths and contents concatenated without boundaries.
/// Only for verifying lock entries that recorded it.
fn hash_tree_unframed(dir: &Path) -> Result<String, IoOp> {
    let mut hasher = Sha256::new();
    for path in files(dir)? {
        let relative = path.strip_prefix(dir).unwrap_or(&path);
        hasher.update(relative_path_to_forward_slash(relative).as_bytes());
        feed(&mut hasher, &path)?;
    }
    Ok(crate::digest::finish_hex(hasher))
}

/// Every file under `dir` except the checksum cache, sorted.
fn files(dir: &Path) -> Result<Vec<PathBuf>, IoOp> {
    let mut entries = Vec::new();
    for entry in walkdir::WalkDir::new(dir).sort_by_file_name() {
        let entry = entry.map_err(|e| {
            let path = e
                .path()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| dir.to_path_buf());
            IoOp::wrap(
                "walk directory for checksum",
                &path,
                e.into_io_error()
                    .unwrap_or_else(|| std::io::Error::other("walk error")),
            )
        })?;
        if !entry.file_type().is_file() || entry.file_name() == CHECKSUM_CACHE_FILE {
            continue;
        }
        entries.push(entry.path().to_path_buf());
    }
    entries.sort();
    Ok(entries)
}

/// Hash `path`'s contents into `hasher`; returns the number of bytes read.
fn feed(hasher: &mut Sha256, path: &Path) -> Result<u64, IoOp> {
    let mut file =
        std::fs::File::open(path).map_err(|e| IoOp::wrap("open file for checksum", path, e))?;
    let mut buffer = [0u8; 8192];
    let mut total = 0u64;
    loop {
        let n = file
            .read(&mut buffer)
            .map_err(|e| IoOp::wrap("read file for checksum", path, e))?;
        if n == 0 {
            return Ok(total);
        }
        hasher.update(&buffer[..n]);
        total += n as u64;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write(path: &Path, content: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    #[test]
    fn identical_trees_hash_identically() {
        let a = TempDir::new().unwrap();
        let b = TempDir::new().unwrap();
        for root in [a.path(), b.path()] {
            write(&root.join("hpm.toml"), "[package]");
            write(&root.join("python/mod.py"), "x = 1");
        }
        assert_eq!(hash_tree(a.path()).unwrap(), hash_tree(b.path()).unwrap());
    }

    #[test]
    fn content_change_changes_hash() {
        let dir = TempDir::new().unwrap();
        write(&dir.path().join("hpm.toml"), "[package]");
        let before = hash_tree(dir.path()).unwrap();
        write(&dir.path().join("hpm.toml"), "[package]\nchanged = true");
        assert_ne!(before, hash_tree(dir.path()).unwrap());
    }

    #[test]
    fn stray_checksum_cache_file_is_ignored() {
        let dir = TempDir::new().unwrap();
        write(&dir.path().join("hpm.toml"), "[package]");
        let before = hash_tree(dir.path()).unwrap();
        write(&dir.path().join(CHECKSUM_CACHE_FILE), "deadbeef");
        assert_eq!(before, hash_tree(dir.path()).unwrap());
    }

    /// A tree as a map of relative path to contents, written into `root`.
    fn write_tree(root: &Path, tree: &std::collections::BTreeMap<String, Vec<u8>>) {
        for (path, contents) in tree {
            let full = root.join(path);
            std::fs::create_dir_all(full.parent().unwrap()).unwrap();
            std::fs::write(full, contents).unwrap();
        }
    }

    /// Trees over a tiny alphabet, so paths and contents that differ only in
    /// where one ends and the next begins come up often.
    fn tree()
    -> impl proptest::strategy::Strategy<Value = std::collections::BTreeMap<String, Vec<u8>>> {
        use proptest::prelude::*;
        prop::collection::btree_map(
            prop_oneof!["[ab]{1,3}", "[ab]{1,2}/[ab]{1,2}"],
            prop::collection::vec(prop_oneof![Just(b'a'), Just(b'b'), Just(b'/')], 0..4),
            0..4,
        )
        .prop_filter("a path cannot be both a file and a directory", |tree| {
            !tree
                .keys()
                .any(|p| tree.keys().any(|q| q.starts_with(&format!("{p}/"))))
        })
    }

    proptest::proptest! {
        #![proptest_config(proptest::prelude::ProptestConfig::with_cases(128))]

        /// The digest a lock records identifies the tree: different trees
        /// never share a `v2:` digest, the same tree always gets the same
        /// one, and a recorded digest of either format recomputes to itself.
        #[test]
        fn the_v2_digest_identifies_the_tree(one in tree(), two in tree()) {
            let (a, b) = (TempDir::new().unwrap(), TempDir::new().unwrap());
            write_tree(a.path(), &one);
            write_tree(b.path(), &two);
            let (ha, hb) = (hash_tree(a.path()).unwrap(), hash_tree(b.path()).unwrap());
            proptest::prop_assert_eq!(ha == hb, one == two, "{:?} vs {:?}", one, two);
            proptest::prop_assert_eq!(&hash_tree_as(a.path(), &ha).unwrap(), &ha);
            let unframed = hash_tree_unframed(a.path()).unwrap();
            proptest::prop_assert_eq!(hash_tree_as(a.path(), &unframed).unwrap(), unframed);
        }

        /// Moving the boundary between a file's path and its contents (file
        /// `a` holding `bc` against file `ab` holding `c`) feeds an unframed
        /// hash the same bytes; the `v2:` digest must still tell the trees
        /// apart, alongside any other files.
        #[test]
        fn moving_the_path_contents_boundary_changes_the_digest(
            path in "[ab]{1,3}",
            contents in "[ab]{1,4}",
            split in 1usize..4,
            rest in tree(),
        ) {
            let split = split.min(contents.len());
            let moved_path = format!("{path}{}", &contents[..split]);
            let moved_contents = &contents[split..];
            let mut one = rest.clone();
            let mut two = rest;
            one.retain(|p, _| !p.starts_with(&path) && !path.starts_with(p.as_str()));
            two.retain(|p, _| !p.starts_with(&path) && !path.starts_with(p.as_str()));
            one.insert(path.clone(), contents.clone().into_bytes());
            two.insert(moved_path, moved_contents.as_bytes().to_vec());
            let (a, b) = (TempDir::new().unwrap(), TempDir::new().unwrap());
            write_tree(a.path(), &one);
            write_tree(b.path(), &two);
            proptest::prop_assert_ne!(hash_tree(a.path()).unwrap(), hash_tree(b.path()).unwrap());
        }
    }

    /// File `a` holding `bc` and file `ab` holding `c` are different trees,
    /// but concatenating path and contents with no boundary feeds the hash
    /// `abc` for both. The digest a lock records must tell them apart.
    #[test]
    fn trees_differing_only_in_a_path_content_boundary_hash_differently() {
        let one = TempDir::new().unwrap();
        let two = TempDir::new().unwrap();
        write(&one.path().join("a"), "bc");
        write(&two.path().join("ab"), "c");
        assert_ne!(
            hash_tree(one.path()).unwrap(),
            hash_tree(two.path()).unwrap()
        );
    }

    /// Both digest formats are stored in hpm.lock, so their exact bytes are
    /// pinned for a fixed tree (nested dirs, an empty file, non-ASCII names,
    /// binary content, and the excluded checksum cache). The unframed digest
    /// is the one every lockfile before v2 recorded, and must not move.
    #[test]
    fn hash_tree_golden_digest() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        write(&root.join("hpm.toml"), "[package]\n");
        write(&root.join("otls/rig.hda"), "alpha");
        write(&root.join("python/ø/日本.py"), "print('hi')\n");
        write(&root.join("empty.txt"), "");
        std::fs::write(root.join("bin.dat"), [0u8, 1, 2, 255, 254]).unwrap();
        write(&root.join(CHECKSUM_CACHE_FILE), "ignored");
        assert_eq!(
            hash_tree_unframed(root).unwrap(),
            "9d7e3976ec4826a559d46712856488be47a99d6f6779676efb4839232c00a9b4"
        );
        assert_eq!(
            hash_tree(root).unwrap(),
            "v2:984be423ea7d3485b38277415fe431323af9bbc88319a5dcf036455c63da6b25"
        );
        // A lock entry is recomputed in the format it recorded.
        assert_eq!(
            hash_tree_as(
                root,
                "9d7e3976ec4826a559d46712856488be47a99d6f6779676efb4839232c00a9b4"
            )
            .unwrap(),
            "9d7e3976ec4826a559d46712856488be47a99d6f6779676efb4839232c00a9b4"
        );
        assert_eq!(
            hash_tree_as(root, "v2:0").unwrap(),
            hash_tree(root).unwrap()
        );
    }
}
