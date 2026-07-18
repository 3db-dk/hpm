use hpm_package::PackageManifest;
use semver::{Version, VersionReq as SemverVersionReq};
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

#[derive(Debug, Clone)]
pub struct PackageSpec {
    pub name: String,
    pub version_req: VersionReq,
}

/// Version requirement with proper semantic versioning support.
/// Supports: exact versions, ^, ~, >=, <=, >, <, and * (any)
#[derive(Debug, Clone)]
pub struct VersionReq {
    requirement: String,
    parsed: SemverVersionReq,
}

impl VersionReq {
    /// Parse a version requirement. Returns `Err` for empty/whitespace input
    /// or any string `semver::VersionReq::parse` rejects — there is no silent
    /// fallback to string equality, so a malformed requirement in `hpm.toml`
    /// fails loudly at load time instead of matching an equally malformed
    /// version later.
    pub fn new(requirement: &str) -> Result<Self, String> {
        let trimmed = requirement.trim();
        if trimmed.is_empty() {
            return Err("Version requirement cannot be empty or whitespace-only".to_string());
        }

        let parsed = SemverVersionReq::parse(trimmed)
            .map_err(|e| format!("Invalid version requirement '{}': {}", trimmed, e))?;

        Ok(Self {
            requirement: trimmed.to_string(),
            parsed,
        })
    }

    pub fn as_str(&self) -> &str {
        &self.requirement
    }

    /// Check if a `version` string matches this requirement. Returns `false`
    /// for unparseable `version` input — a malformed version is not a match
    /// against any well-formed requirement.
    pub fn matches(&self, version: &str) -> bool {
        match Version::parse(version) {
            Ok(ver) => self.parsed.matches(&ver),
            Err(_) => false,
        }
    }
}

impl std::fmt::Display for VersionReq {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.requirement)
    }
}

impl PackageSpec {
    pub fn new(name: String, version_req: VersionReq) -> Self {
        Self { name, version_req }
    }

    pub fn parse(spec: &str) -> Result<Self, String> {
        // Use rfind('@') so scoped paths like `creator/slug@1.0.0` are handled
        // correctly: everything before the last `@` is the package identifier,
        // everything after is the version.
        match spec.rfind('@') {
            Some(pos) => {
                let name = spec[..pos].to_string();
                let version_str = &spec[pos + 1..];
                if name.is_empty() {
                    return Err(format!("Invalid package specification: {}", spec));
                }
                let version_req = VersionReq::new(version_str)?;
                Ok(Self::new(name, version_req))
            }
            None => {
                // Just package name/path, default to latest
                let name = spec.to_string();
                let version_req = VersionReq::new("*")?;
                Ok(Self::new(name, version_req))
            }
        }
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

    /// Check if this installed package satisfies a version requirement
    pub fn is_compatible_with(&self, version_req: &VersionReq) -> bool {
        version_req.matches(&self.version)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    // Custom strategies for generating test data

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

    /// Strategy to generate version requirements
    fn version_req_strategy() -> impl Strategy<Value = String> {
        prop_oneof![
            Just("*".to_string()),
            version_strategy(),
            version_strategy().prop_map(|v| format!("^{}", v)),
            version_strategy().prop_map(|v| format!("~{}", v)),
            version_strategy().prop_map(|v| format!(">={}", v)),
        ]
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

    // Property-based tests

    proptest! {
        /// Test that valid version requirements can be created and are consistent
        #[test]
        fn prop_version_req_roundtrip(version_str in version_req_strategy()) {
            let version_req = VersionReq::new(&version_str).unwrap();
            prop_assert_eq!(version_req.as_str(), version_str.clone());
            prop_assert_eq!(version_req.to_string(), version_str);
        }

        /// Test that empty or whitespace-only strings fail to create version requirements
        #[test]
        fn prop_version_req_invalid(whitespace in r"\s*") {
            let result = VersionReq::new(&whitespace);
            prop_assert!(result.is_err());
        }

        /// Test that package specs can be parsed and maintain consistency
        #[test]
        fn prop_package_spec_parse_roundtrip(
            name in package_name_strategy(),
            version in version_req_strategy()
        ) {
            let spec_str = if version == "*" {
                name.clone()
            } else {
                format!("{}@{}", name, version)
            };

            let spec = PackageSpec::parse(&spec_str).unwrap();
            prop_assert_eq!(spec.name, name);
            prop_assert_eq!(spec.version_req.as_str(), version);
        }

        /// Test that installed packages maintain identity consistency
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

        /// Test version compatibility logic with various requirement patterns
        #[test]
        fn prop_version_compatibility(
            name in package_name_strategy(),
            package_version in version_strategy(),
            req_version in version_req_strategy(),
            path in file_path_strategy()
        ) {
            let manifest = hpm_package::PackageManifest::new(
                hpm_package::PackagePath::new(format!("studio/{}", name)).unwrap(),
                "Test Package".to_string(),
                package_version.clone(),
                None,
                Vec::new(),
                None,
            );

            let _ = name; // kept by package_name_strategy for path construction above
            let package = InstalledPackage {
                version: package_version.clone(),
                manifest,
                install_path: path,
                is_dev: false,
            };

            let version_req = VersionReq::new(&req_version).unwrap();
            let is_compatible = package.is_compatible_with(&version_req);

            // Wildcard should always match
            if req_version == "*" {
                prop_assert!(is_compatible);
            }

            // Exact version match
            if req_version == package_version {
                prop_assert!(is_compatible);
            }

            // Version compatibility is consistent with repeated calls
            prop_assert_eq!(is_compatible, package.is_compatible_with(&version_req));
        }

        /// Test that package spec parsing fails gracefully with malformed input
        #[test]
        fn prop_package_spec_invalid_input(
            invalid_input in r"[^a-zA-Z0-9@.\-_]{1,10}|.*@.*@.*"
        ) {
            // Skip inputs that might accidentally be valid
            if invalid_input.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '.') {
                return Ok(());
            }

            let result = PackageSpec::parse(&invalid_input);
            // Should either parse successfully or fail gracefully
            if let Ok(spec) = result {
                // If it parses, the name should be non-empty
                prop_assert!(!spec.name.is_empty());
            }
        }
    }

    // Traditional unit tests for edge cases and specific scenarios

    #[test]
    fn version_req_empty_string_fails() {
        let result = VersionReq::new("");
        assert!(result.is_err());
    }

    #[test]
    fn version_req_exact_operator_is_accepted() {
        // `=1.2.3` is the semver exact-match operator. It must parse as a
        // requirement (not be mistaken for a literal version string and shoved
        // into a registry URL path), matching 1.2.3 and nothing else.
        let req = VersionReq::new("=1.2.3").expect("'=1.2.3' must parse as a requirement");
        assert_eq!(req.as_str(), "=1.2.3");
        assert!(req.matches("1.2.3"));
        assert!(!req.matches("1.2.4"));
        assert!(!req.matches("1.2.2"));
    }

    #[test]
    fn package_spec_exact_operator_version() {
        let spec = PackageSpec::parse("tumblehead/fire-fx@=1.2.3")
            .expect("scoped spec with '=' operator must parse");
        assert_eq!(spec.name, "tumblehead/fire-fx");
        assert_eq!(spec.version_req.as_str(), "=1.2.3");
    }

    #[test]
    fn package_spec_multiple_at_signs_uses_last() {
        // With rfind('@'), "package@1.0.0@extra" splits into name="package@1.0.0"
        // and version="extra" — rejected because "extra" is not a valid version
        // requirement. (Earlier behavior silently accepted any string and
        // matched it via fallback equality, hiding malformed inputs.)
        let result = PackageSpec::parse("package@1.0.0@extra");
        assert!(result.is_err(), "non-semver version segment must error");
    }

    #[test]
    fn package_spec_scoped_path_with_version() {
        let result = PackageSpec::parse("tumblehead/fire-fx@1.0.0");
        assert!(result.is_ok());
        let spec = result.unwrap();
        assert_eq!(spec.name, "tumblehead/fire-fx");
        assert_eq!(spec.version_req.as_str(), "1.0.0");
    }

    #[test]
    fn package_spec_scoped_path_without_version() {
        let result = PackageSpec::parse("tumblehead/fire-fx");
        assert!(result.is_ok());
        let spec = result.unwrap();
        assert_eq!(spec.name, "tumblehead/fire-fx");
        assert_eq!(spec.version_req.as_str(), "*");
    }

    #[test]
    fn version_compatibility_edge_cases() {
        let manifest = hpm_package::PackageManifest::new(
            hpm_package::PackagePath::new("studio/test").unwrap(),
            "Test Package".to_string(),
            "1.0.0".to_string(),
            None,
            Vec::new(),
            None,
        );

        let package = InstalledPackage {
            version: "1.0.0".to_string(),
            manifest,
            install_path: PathBuf::from("/path"),
            is_dev: false,
        };

        // Test specific edge cases that property tests might miss
        assert!(package.is_compatible_with(&VersionReq::new("*").unwrap()));
        assert!(package.is_compatible_with(&VersionReq::new("1.0.0").unwrap()));
        assert!(!package.is_compatible_with(&VersionReq::new("2.0.0").unwrap()));
    }
}
