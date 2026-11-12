//! Characterization of the native package descriptor (`{slug}.json`) that
//! `hpm pack` ships, for both targets, over a fixture corpus covering every
//! `[runtime]` form, platform-specific packages, dependencies and scripts.

use hpm_package::{NativePackageTarget, PackageManifest};
use std::path::Path;

const CORPUS: &[&str] = &["minimal", "runtime", "platforms", "deps_scripts"];

fn render(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/emit")
        .join(format!("{name}.toml"));
    let manifest = PackageManifest::from_path(&path).unwrap();
    let mut out = String::new();
    for target in [
        NativePackageTarget::Generic,
        NativePackageTarget::SideFxHpackage,
    ] {
        out += &format!("--- {target:?}\n");
        match manifest.generate_houdini_native_package_for(target) {
            Ok((file, pkg)) => {
                out += &format!(
                    "file: {file}\n{}\n",
                    serde_json::to_string_pretty(&pkg).unwrap()
                );
            }
            Err(e) => out += &format!("error: {e}\n"),
        }
    }
    out
}

#[test]
fn native_package_descriptors() {
    for name in CORPUS {
        insta::assert_snapshot!(format!("native_{name}"), render(name));
    }
}
