//! End-to-end: pack a package, then build the asset index from the archive.

use hpm_assets::AssetKind;
use hpm_core::{collect_assets, packer};
use hpm_package::manifest::{OperatorDecl, OperatorKind, OperatorSource, StageConfig};
use std::fs;
use tempfile::TempDir;

fn op(kind: OperatorKind, type_name: &str, category: &str, source: Option<&str>) -> OperatorDecl {
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

#[test]
fn pack_then_index_reports_present_and_missing_sources() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("hpm.toml"),
        "[package]\npath = \"studio/test\"\nname = \"test\"\nversion = \"1.0.0\"\n",
    )
    .unwrap();
    fs::create_dir_all(dir.path().join("otls")).unwrap();
    fs::write(dir.path().join("otls/rbd.hda"), b"hda-bytes").unwrap();

    let out = TempDir::new().unwrap();
    let result = packer::pack(
        dir.path(),
        "test",
        "1.0.0",
        out.path(),
        None,
        None,
        &StageConfig::default(),
        &[],
    )
    .unwrap();

    let operators = vec![
        // present in the archive
        op(
            OperatorKind::Hda,
            "studio::rbd_configure::2.0",
            "Sop",
            Some("otls/rbd.hda"),
        ),
        // declared but not shipped -> reported as missing
        op(
            OperatorKind::Dso,
            "studio::fast_scatter",
            "Sop",
            Some("dso/scatter.so"),
        ),
    ];

    let index = collect_assets(&result.archive_path, &operators, None).unwrap();

    assert_eq!(index.assets.len(), 2);
    assert_eq!(index.assets[0].kind, AssetKind::HdaOperator);
    assert_eq!(index.assets[0].namespace.as_deref(), Some("studio"));
    assert_eq!(index.assets[0].op_version.as_deref(), Some("2.0"));
    assert_eq!(index.assets[1].kind, AssetKind::DsoOperator);

    assert_eq!(index.missing_sources, vec!["dso/scatter.so".to_string()]);
}
