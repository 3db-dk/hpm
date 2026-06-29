//! Build the package asset index from the manifest's `[[operators]]`.
//!
//! `hpm pack` emits a searchable index of the operators a package bundles so
//! the registry can offer node-level search without ever opening the archive.
//! The index is built entirely from the author's `[[operators]]` declarations
//! — HPM does not parse the package's HDA/DSO files (the HDA format is
//! undocumented and unstable, and DSOs do not expose operator names offline).
//!
//! Each operator's `source` is resolved for the platform being packed (a
//! single path applies everywhere; a per-platform table resolves to its entry),
//! and operators not shipped for that platform are dropped from its index.
//!
//! As a guard against drift between what's declared and what actually ships,
//! [`collect_assets`] checks each resolved `source` against the produced archive
//! and reports any that are missing (the CLI warns, or fails under
//! `--verify-assets`).

use std::path::Path;

use hpm_assets::asset::{Asset, AssetKind, split_type_name};
use hpm_package::IoOp;
use hpm_package::Platform;
use hpm_package::manifest::{OperatorDecl, OperatorKind, SourceResolution};

/// Errors from building the asset index.
#[derive(Debug, thiserror::Error)]
pub enum AssetIndexError {
    #[error(transparent)]
    Io(#[from] IoOp),

    #[error("Zip error reading archive for indexing: {0}")]
    Zip(#[from] zip::result::ZipError),
}

/// Map a declared `[[operators]]` entry to an index [`Asset`], filling
/// `namespace`/`op_version` by best-effort parse of the type name and stamping
/// the source path resolved for the target platform.
fn to_asset(op: &OperatorDecl, source_file: Option<String>) -> Asset {
    let kind = match op.kind {
        OperatorKind::Hda => AssetKind::HdaOperator,
        OperatorKind::Dso => AssetKind::DsoOperator,
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
        source_file,
    }
}

/// Build the asset index from declared operators for a target `platform`
/// (`None` for a universal, platform-less pack).
///
/// Each operator's `source` is resolved against `platform`: a single source
/// applies everywhere, a per-platform source resolves to the entry for the
/// target platform, and an operator whose per-platform source does not cover
/// the target platform is omitted from this platform's index (its file is not
/// in this package). Every resolved source is then checked against the produced
/// archive; paths not found are returned in [`AssetIndex::missing_sources`] for
/// the caller to warn on or treat as fatal.
///
/// Short-circuits the archive read when no operator resolves to a source path.
pub fn collect_assets(
    archive_path: &Path,
    operators: &[OperatorDecl],
    platform: Option<&Platform>,
) -> Result<AssetIndex, AssetIndexError> {
    let mut assets = Vec::new();
    let mut to_verify: Vec<String> = Vec::new();

    for op in operators {
        let source_file = match op.resolved_source(platform) {
            SourceResolution::Path(path) => {
                to_verify.push(path.to_string());
                Some(path.to_string())
            }
            SourceResolution::Unspecified => None,
            // Platform-specific source that doesn't cover this platform: the
            // operator isn't shipped here, so drop it from this index.
            SourceResolution::NotForPlatform => continue,
        };
        assets.push(to_asset(op, source_file));
    }

    let missing_sources = if to_verify.is_empty() {
        Vec::new()
    } else {
        let present = archive_entry_names(archive_path)?;
        to_verify
            .into_iter()
            .filter(|src| !present.contains(src))
            .collect()
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
    use hpm_package::manifest::OperatorSource;
    use indexmap::IndexMap;
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
            source: source.map(|s| OperatorSource::Single(s.to_string())),
        }
    }

    fn per_platform_op(type_name: &str, pairs: &[(&str, &str)]) -> OperatorDecl {
        let mut map = IndexMap::new();
        for (plat, path) in pairs {
            map.insert(plat.to_string(), path.to_string());
        }
        OperatorDecl {
            kind: OperatorKind::Dso,
            type_name: type_name.to_string(),
            category: "Sop".to_string(),
            label: None,
            tab_submenu: None,
            icon: None,
            source: Some(OperatorSource::PerPlatform(map)),
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
                OperatorKind::Dso,
                "studio::fast_scatter",
                "Sop",
                Some("dso/scatter.so"),
            ),
        ];

        let index = collect_assets(&archive, &ops, None).unwrap();
        assert_eq!(index.assets.len(), 2);
        assert!(index.missing_sources.is_empty());

        let rbd = &index.assets[0];
        assert_eq!(rbd.kind, AssetKind::HdaOperator);
        assert_eq!(rbd.namespace.as_deref(), Some("studio"));
        assert_eq!(rbd.op_version.as_deref(), Some("2.0"));
        assert_eq!(rbd.source_file.as_deref(), Some("otls/rbd.hda"));

        let scatter = &index.assets[1];
        assert_eq!(scatter.kind, AssetKind::DsoOperator);
        assert_eq!(scatter.namespace.as_deref(), Some("studio"));
        assert_eq!(scatter.op_version, None);
    }

    #[test]
    fn missing_source_is_reported_not_fatal() {
        let dir = TempDir::new().unwrap();
        let archive = write_zip(dir.path(), &[("README.md", b"hi")]);

        let ops = vec![op(
            OperatorKind::Dso,
            "studio::ghost",
            "Sop",
            Some("dso/ghost.so"),
        )];

        let index = collect_assets(&archive, &ops, None).unwrap();
        assert_eq!(index.assets.len(), 1);
        assert_eq!(index.missing_sources, vec!["dso/ghost.so".to_string()]);
    }

    #[test]
    fn no_operators_skips_archive_read() {
        // A nonexistent archive path must not error when there's nothing to
        // index — the archive is only opened to check declared sources.
        let index = collect_assets(Path::new("/no/such/archive.zip"), &[], None).unwrap();
        assert!(index.assets.is_empty());
        assert!(index.missing_sources.is_empty());
    }

    #[test]
    fn no_declared_sources_skips_archive_read() {
        let ops = vec![op(OperatorKind::Dso, "studio::x", "Sop", None)];
        let index = collect_assets(Path::new("/no/such/archive.zip"), &ops, None).unwrap();
        assert_eq!(index.assets.len(), 1);
        assert!(index.missing_sources.is_empty());
    }

    #[test]
    fn per_platform_source_resolves_to_target_platform() {
        let dir = TempDir::new().unwrap();
        // A linux archive containing only the linux binary.
        let archive = write_zip(dir.path(), &[("dso/linux-x86_64/scatter.so", b"x")]);

        let ops = vec![per_platform_op(
            "studio::fast_scatter",
            &[
                ("linux-x86_64", "dso/linux-x86_64/scatter.so"),
                ("macos-aarch64", "dso/macos-aarch64/scatter.dylib"),
            ],
        )];

        let index = collect_assets(&archive, &ops, Some(&Platform::LinuxX86_64)).unwrap();
        assert_eq!(index.assets.len(), 1);
        assert_eq!(
            index.assets[0].source_file.as_deref(),
            Some("dso/linux-x86_64/scatter.so")
        );
        assert!(index.missing_sources.is_empty());
    }

    #[test]
    fn per_platform_operator_omitted_when_platform_not_covered() {
        let dir = TempDir::new().unwrap();
        let archive = write_zip(dir.path(), &[("dso/linux-x86_64/scatter.so", b"x")]);

        // Operator only ships a linux binary; packing for macOS drops it.
        let ops = vec![per_platform_op(
            "studio::fast_scatter",
            &[("linux-x86_64", "dso/linux-x86_64/scatter.so")],
        )];

        let index = collect_assets(&archive, &ops, Some(&Platform::MacosAarch64)).unwrap();
        assert!(index.assets.is_empty());
        assert!(index.missing_sources.is_empty());
    }

    #[test]
    fn per_platform_missing_file_is_reported() {
        let dir = TempDir::new().unwrap();
        // Archive is missing the linux binary the operator claims.
        let archive = write_zip(dir.path(), &[("README.md", b"hi")]);

        let ops = vec![per_platform_op(
            "studio::fast_scatter",
            &[("linux-x86_64", "dso/linux-x86_64/scatter.so")],
        )];

        let index = collect_assets(&archive, &ops, Some(&Platform::LinuxX86_64)).unwrap();
        assert_eq!(index.assets.len(), 1);
        assert_eq!(
            index.missing_sources,
            vec!["dso/linux-x86_64/scatter.so".to_string()]
        );
    }
}
