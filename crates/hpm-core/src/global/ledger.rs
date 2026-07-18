//! Record of what `hpm global` installed, per Houdini version.
//!
//! The ledger is the authority for global installs. It exists because the
//! target directory does not belong to hpm: Houdini's user `packages/`
//! directory holds files written by SideFX, by other tools, and by the user
//! by hand. The project-side installer can sweep its output directory and
//! delete anything it doesn't recognise, because it owns that directory
//! outright. Doing the same here would delete other people's files.
//!
//! So every global operation is driven off this file rather than off a
//! directory scan: `list` reads it, `remove` deletes exactly the manifest it
//! names, and `hpm clean` treats its entries as GC roots so the store bytes
//! behind a global install are not collected as unreferenced.

use hpm_package::{IoOp, PackagePath, atomic_write};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use thiserror::Error;

use crate::houdini_prefs::HoudiniVersion;

#[derive(Debug, Error)]
pub enum LedgerError {
    #[error(transparent)]
    Io(#[from] IoOp),

    #[error("Global ledger at {path} is corrupt: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },

    #[error("Failed to serialize the global ledger: {0}")]
    Serialize(#[source] serde_json::Error),
}

/// One globally installed package.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GlobalEntry {
    /// Resolved exact version.
    pub version: String,
    /// Registry the package was resolved from, when it was pinned or known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub registry: Option<String>,
    /// Filename written into the Houdini packages directory. Stored rather
    /// than recomputed so that a naming-scheme change in a later hpm still
    /// removes the file this install actually created.
    pub manifest_file: String,
    /// CAS directory backing this install, used as a `hpm clean` GC root.
    pub install_path: PathBuf,
}

/// Contents of one `~/.hpm/global/houdini-<X.Y>.json`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Ledger {
    /// Keyed by scoped `creator/slug`, so one package cannot be globally
    /// installed twice at different versions for the same Houdini.
    #[serde(default)]
    pub packages: BTreeMap<String, GlobalEntry>,
}

impl Ledger {
    /// Path of the ledger for `version` under `hpm_home` (`~/.hpm`).
    pub fn path_for(hpm_home: &Path, version: HoudiniVersion) -> PathBuf {
        hpm_home
            .join("global")
            .join(format!("houdini-{}.json", version.as_dir_component()))
    }

    /// Load the ledger, treating a missing file as empty — nothing has been
    /// installed globally for this Houdini version yet.
    pub fn load(path: &Path) -> Result<Self, LedgerError> {
        let raw = match std::fs::read_to_string(path) {
            Ok(raw) => raw,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(IoOp::wrap("read global ledger", path, e).into()),
        };
        serde_json::from_str(&raw).map_err(|source| LedgerError::Parse {
            path: path.to_path_buf(),
            source,
        })
    }

    pub fn save(&self, path: &Path) -> Result<(), LedgerError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| IoOp::wrap("create global ledger directory", parent, e))?;
        }
        let json = serde_json::to_vec_pretty(self).map_err(LedgerError::Serialize)?;
        atomic_write(path, json)?;
        Ok(())
    }

    pub fn get(&self, package: &PackagePath) -> Option<&GlobalEntry> {
        self.packages.get(package.as_str())
    }

    pub fn insert(&mut self, package: &PackagePath, entry: GlobalEntry) -> Option<GlobalEntry> {
        self.packages.insert(package.as_str().to_string(), entry)
    }

    pub fn remove(&mut self, package: &PackagePath) -> Option<GlobalEntry> {
        self.packages.remove(package.as_str())
    }

    pub fn is_empty(&self) -> bool {
        self.packages.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &GlobalEntry)> {
        self.packages.iter()
    }
}

/// Every global entry across every Houdini version's ledger under `hpm_home`.
///
/// `hpm clean` uses this: a globally installed package is referenced by no
/// project, so without these roots the mark-and-sweep would classify its CAS
/// directory as unreferenced and delete the bytes out from under a manifest
/// that Houdini still loads.
///
/// A ledger that fails to parse is skipped with the error returned alongside,
/// never treated as empty — "no roots" and "unreadable roots" must not look
/// the same to a garbage collector.
pub fn all_global_entries(hpm_home: &Path) -> (Vec<GlobalEntry>, Vec<(PathBuf, LedgerError)>) {
    let dir = hpm_home.join("global");
    let mut entries = Vec::new();
    let mut failures = Vec::new();

    let read_dir = match std::fs::read_dir(&dir) {
        Ok(rd) => rd,
        // No global directory means nothing was ever installed globally.
        Err(_) => return (entries, failures),
    };

    for dir_entry in read_dir.flatten() {
        let path = dir_entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        match Ledger::load(&path) {
            Ok(ledger) => entries.extend(ledger.packages.into_values()),
            Err(e) => failures.push((path, e)),
        }
    }

    (entries, failures)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn entry(version: &str, file: &str) -> GlobalEntry {
        GlobalEntry {
            version: version.to_string(),
            registry: None,
            manifest_file: file.to_string(),
            install_path: PathBuf::from("/store").join(file),
        }
    }

    fn pkg(s: &str) -> PackagePath {
        PackagePath::new(s).unwrap()
    }

    #[test]
    fn ledger_path_is_per_houdini_version() {
        let home = Path::new("/home/u/.hpm");
        let a = Ledger::path_for(home, HoudiniVersion::parse("21.0").unwrap());
        let b = Ledger::path_for(home, HoudiniVersion::parse("22.0").unwrap());
        assert!(a.ends_with("global/houdini-21.0.json"));
        assert!(b.ends_with("global/houdini-22.0.json"));
        assert_ne!(a, b);
    }

    #[test]
    fn missing_ledger_loads_as_empty() {
        let tmp = TempDir::new().unwrap();
        let ledger = Ledger::load(&tmp.path().join("nope.json")).unwrap();
        assert!(ledger.is_empty());
    }

    #[test]
    fn round_trips_through_disk() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("global").join("houdini-21.0.json");

        let mut ledger = Ledger::default();
        ledger.insert(&pkg("acme/tools"), entry("1.2.3", "hpm-acme.tools.json"));
        ledger.save(&path).unwrap();

        let loaded = Ledger::load(&path).unwrap();
        assert_eq!(loaded.get(&pkg("acme/tools")).unwrap().version, "1.2.3");
    }

    /// A corrupt ledger must not read as "nothing is installed" — that would
    /// let `hpm clean` collect every globally installed package.
    #[test]
    fn corrupt_ledger_is_an_error_not_an_empty_ledger() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("houdini-21.0.json");
        std::fs::write(&path, b"{ not json").unwrap();

        assert!(matches!(
            Ledger::load(&path),
            Err(LedgerError::Parse { .. })
        ));
    }

    #[test]
    fn all_global_entries_spans_every_houdini_version() {
        let tmp = TempDir::new().unwrap();
        let home = tmp.path();

        for (version, slug) in [("21.0", "a"), ("22.0", "b")] {
            let mut ledger = Ledger::default();
            ledger.insert(
                &pkg(&format!("acme/{slug}")),
                entry("1.0.0", &format!("hpm-acme.{slug}.json")),
            );
            ledger
                .save(&Ledger::path_for(
                    home,
                    HoudiniVersion::parse(version).unwrap(),
                ))
                .unwrap();
        }

        let (entries, failures) = all_global_entries(home);
        assert_eq!(entries.len(), 2);
        assert!(failures.is_empty());
    }

    /// An unreadable ledger is reported, not silently dropped — a GC that
    /// cannot read its roots must not conclude there are none.
    #[test]
    fn all_global_entries_reports_unreadable_ledgers() {
        let tmp = TempDir::new().unwrap();
        let home = tmp.path();
        std::fs::create_dir_all(home.join("global")).unwrap();
        std::fs::write(home.join("global").join("houdini-21.0.json"), b"broken").unwrap();

        let (entries, failures) = all_global_entries(home);
        assert!(entries.is_empty());
        assert_eq!(failures.len(), 1);
    }

    #[test]
    fn no_global_dir_yields_no_entries_and_no_failures() {
        let tmp = TempDir::new().unwrap();
        let (entries, failures) = all_global_entries(tmp.path());
        assert!(entries.is_empty() && failures.is_empty());
    }
}
