use hpm_package::PackageManifest;
use std::path::PathBuf;

/// A package present in the global CAS. The bare slug and full path live on
/// `manifest.package.path` — call `.slug()` / `.identifier()` rather than
/// duplicating them on the wrapper type.
///
/// `is_dev` records whether this entry was installed from a local path
/// dependency (either copied or symlinked under `_dev/`). It feeds the
/// `install_source` axis on `[runtime]` conditional variants at Houdini
/// manifest generation time, so a variant gated `install_source = "dev"`
/// only fires for path-installed packages and a variant gated
/// `install_source = "registry"` only fires for registry/URL installs.
#[derive(Debug, Clone)]
pub struct InstalledPackage {
    pub version: String,
    pub manifest: PackageManifest,
    pub install_path: PathBuf,
    pub is_dev: bool,
}

impl InstalledPackage {
    /// Identity used to record venv ownership: `creator/slug@version`.
    ///
    /// Scoped rather than bare-slug — two creators may publish the same slug,
    /// and a bare-slug reference would let one package's venv look live
    /// because an unrelated package happens to share its slug. Venv creation
    /// and venv cleanup must both go through this so the two cannot drift.
    pub fn venv_ref(&self) -> String {
        format!("{}@{}", self.manifest.package.identifier(), self.version)
    }
}

impl InstalledPackage {
    /// Bare slug — the kebab segment after the `/` in the package path.
    pub fn slug(&self) -> &str {
        self.manifest.package.slug()
    }

    pub fn identifier(&self) -> String {
        format!("{}@{}", self.slug(), self.version)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// Strategy to generate valid package names
    /// Package names must be lowercase, alphanumeric with hyphens
    fn package_name_strategy() -> impl Strategy<Value = String> {
        prop::string::string_regex("[a-z][a-z0-9-]{1,50}")
            .unwrap()
            .prop_filter("Package name must not end with hyphen", |name| {
                !name.ends_with('-') && name.len() >= 2 && name.len() <= 50
            })
    }

    /// Strategy to generate semantic version strings
    fn version_strategy() -> impl Strategy<Value = String> {
        (0u32..100, 0u32..100, 0u32..100)
            .prop_map(|(major, minor, patch)| format!("{}.{}.{}", major, minor, patch))
    }

    /// Strategy to generate file paths
    fn file_path_strategy() -> impl Strategy<Value = PathBuf> {
        prop::collection::vec("[a-zA-Z][a-zA-Z0-9_-]{1,20}", 1..=5).prop_map(|parts| {
            let mut path = PathBuf::from("/");
            for part in parts {
                path.push(part);
            }
            path
        })
    }

    proptest! {
        #[test]
        fn prop_installed_package_identity(
            name in package_name_strategy(),
            version in version_strategy(),
            path in file_path_strategy()
        ) {
            let manifest = hpm_package::PackageManifest::new(
                hpm_package::PackagePath::new(format!("studio/{}", name)).unwrap(),
                "Test Package".to_string(),
                version.clone(),
                None,
                Vec::new(),
                None,
            );

            let package = InstalledPackage {
                version: version.clone(),
                manifest,
                install_path: path,
                is_dev: false,
            };

            // Identifier uses the slug from the package path: `studio/<name>` → `<name>`.
            let expected_identifier = format!("{}@{}", name, version);
            prop_assert_eq!(package.identifier(), expected_identifier);
        }
    }
}
