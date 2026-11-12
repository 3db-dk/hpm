//! Manifests of packages published on TumbleTrove (the oldest, the newest
//! and every 25th installable version of each package, sampled 2026-11-12).
//! hpm parses the manifest inside every archive it installs, so a stricter
//! parser must still accept all of these: a published package that stops
//! parsing becomes uninstallable for everyone who depends on it.

use hpm_package::PackageManifest;
use std::path::Path;

#[test]
fn every_published_manifest_parses_and_validates() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/published");
    let mut checked = 0;
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        let manifest = PackageManifest::from_path(&path)
            .unwrap_or_else(|e| panic!("{} no longer parses: {e}", path.display()));
        manifest
            .validate()
            .unwrap_or_else(|e| panic!("{} no longer validates: {e}", path.display()));
        checked += 1;
    }
    assert_eq!(checked, 14);
}
