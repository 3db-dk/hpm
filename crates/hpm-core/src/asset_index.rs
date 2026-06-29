//! Build the package asset index from the manifest's `[[operators]]`.
//!
//! `hpm pack` emits a searchable index of the operators a package bundles so
//! the registry can offer node-level search without ever opening the archive.
//! The index is built entirely from the author's `[[operators]]` declarations
//! — HPM does not parse the package's HDA/DSO files (the HDA format is
//! undocumented and unstable, and DSOs do not expose operator names offline).
//!
//! As a light guard against drift between what's declared and what actually
//! ships, [`collect_assets`] checks each declared `source` against the produced
//! archive and reports any that are missing.

use std::path::Path;

use hpm_assets::asset::{Asset, AssetKind, split_type_name};
use hpm_package::IoOp;
use hpm_package::manifest::{OperatorDecl, OperatorKind};

/// Errors from building the asset index.
#[derive(Debug, thiserror::Error)]
pub enum AssetIndexError {
    #[error(transparent)]
    Io(#[from] IoOp),

    #[error("Zip error reading archive for indexing: {0}")]
    Zip(#[from] zip::result::ZipError),
}

/// Map a declared `[[operators]]` entry to an index [`Asset`], filling
/// `namespace`/`op_version` by best-effort parse of the type name.
fn to_asset(op: &OperatorDecl) -> Asset {
    let kind = match op.kind {
        OperatorKind::Hda => AssetKind::HdaOperator,
        OperatorKind::Hdk => AssetKind::HdkOperator,
    };
    let (namespace, _base, op_version) = split_type_name(&op.type_name);
    Asset {
        kind,
        type_name: op.type_name.clone(),
        category: op.category.clone(),
        label: op.label.clone(),
        namespace,
        op_version,
        tab_submenu: op.tab_submenu.clone(),
        icon: op.icon.clone(),
        source_file: op.source.clone(),
    }
}

/// Build the asset index from declared operators.
///
/// Maps every `[[operators]]` declaration to an [`Asset`], then verifies each
/// declared `source` exists in `archive_path`; sources not found are returned
/// in [`AssetIndex::missing_sources`] so the caller can warn (the index is
/// still emitted — a missing source is an author mistake, not a pack failure).
///
/// Passing no operators short-circuits without opening the archive.
pub fn collect_assets(
    archive_path: &Path,
    operators: &[OperatorDecl],
) -> Result<AssetIndex, AssetIndexError> {
    let assets: Vec<Asset> = operators.iter().map(to_asset).collect();

    let missing_sources = if operators.iter().any(|op| op.source.is_some()) {
        let present = archive_entry_names(archive_path)?;
        operators
            .iter()
            .filter_map(|op| op.source.clone())
            .filter(|src| !present.contains(src))
            .collect()
    } else {
        Vec::new()
    };

    Ok(AssetIndex {
        assets,
        missing_sources,
    })
}

/// Collect the set of entry paths in a zip archive.
fn archive_entry_names(
    archive_path: &Path,
) -> Result<std::collections::HashSet<String>, AssetIndexError> {
    let file = std::fs::File::open(archive_path)
        .map_err(|e| IoOp::wrap("open archive for indexing", archive_path, e))?;
    let mut zip = zip::ZipArchive::new(file)?;
    let mut names = std::collections::HashSet::with_capacity(zip.len());
    for i in 0..zip.len() {
        if let Ok(entry) = zip.by_index(i) {
            names.insert(entry.name().to_string());
        }
    }
    Ok(names)
}

/// The result of indexing a packed archive.
#[derive(Debug)]
pub struct AssetIndex {
    /// All indexed operators, in manifest declaration order.
    pub assets: Vec<Asset>,
    /// Declared `source` files that were not found in the produced archive.
    /// Empty on a clean run.
    pub missing_sources: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;
    use zip::write::SimpleFileOptions;

    fn write_zip(dir: &Path, entries: &[(&str, &[u8])]) -> std::path::PathBuf {
        let path = dir.join("pkg.zip");
        let file = std::fs::File::create(&path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let opts = SimpleFileOptions::default();
        for (name, content) in entries {
            zip.start_file(*name, opts).unwrap();
            zip.write_all(content).unwrap();
        }
        zip.finish().unwrap();
        path
    }

    fn op(
        kind: OperatorKind,
        type_name: &str,
        category: &str,
        source: Option<&str>,
    ) -> OperatorDecl {
        OperatorDecl {
            kind,
            type_name: type_name.to_string(),
            category: category.to_string(),
            label: None,
            tab_submenu: None,
            icon: None,
            source: source.map(|s| s.to_string()),
        }
    }

    #[test]
    fn maps_declarations_to_assets_with_split_type_name() {
        let dir = TempDir::new().unwrap();
        let archive = write_zip(
            dir.path(),
            &[("otls/rbd.hda", b"x"), ("dso/scatter.so", b"y")],
        );

        let ops = vec![
            op(
                OperatorKind::Hda,
                "studio::rbd_configure::2.0",
                "Sop",
                Some("otls/rbd.hda"),
            ),
            op(
                OperatorKind::Hdk,
                "studio::fast_scatter",
                "Sop",
                Some("dso/scatter.so"),
            ),
        ];

        let index = collect_assets(&archive, &ops).unwrap();
        assert_eq!(index.assets.len(), 2);
        assert!(index.missing_sources.is_empty());

        let rbd = &index.assets[0];
        assert_eq!(rbd.kind, AssetKind::HdaOperator);
        assert_eq!(rbd.namespace.as_deref(), Some("studio"));
        assert_eq!(rbd.op_version.as_deref(), Some("2.0"));
        assert_eq!(rbd.source_file.as_deref(), Some("otls/rbd.hda"));

        let scatter = &index.assets[1];
        assert_eq!(scatter.kind, AssetKind::HdkOperator);
        assert_eq!(scatter.namespace.as_deref(), Some("studio"));
        assert_eq!(scatter.op_version, None);
    }

    #[test]
    fn missing_source_is_reported_not_fatal() {
        let dir = TempDir::new().unwrap();
        let archive = write_zip(dir.path(), &[("README.md", b"hi")]);

        let ops = vec![op(
            OperatorKind::Hdk,
            "studio::ghost",
            "Sop",
            Some("dso/ghost.so"),
        )];

        let index = collect_assets(&archive, &ops).unwrap();
        assert_eq!(index.assets.len(), 1);
        assert_eq!(index.missing_sources, vec!["dso/ghost.so".to_string()]);
    }

    #[test]
    fn no_operators_skips_archive_read() {
        // A nonexistent archive path must not error when there's nothing to
        // index — the archive is only opened to check declared sources.
        let index = collect_assets(Path::new("/no/such/archive.zip"), &[]).unwrap();
        assert!(index.assets.is_empty());
        assert!(index.missing_sources.is_empty());
    }

    #[test]
    fn no_declared_sources_skips_archive_read() {
        let ops = vec![op(OperatorKind::Hdk, "studio::x", "Sop", None)];
        let index = collect_assets(Path::new("/no/such/archive.zip"), &ops).unwrap();
        assert_eq!(index.assets.len(), 1);
        assert!(index.missing_sources.is_empty());
    }
}
