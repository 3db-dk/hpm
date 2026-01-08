use hpm_package::PackageManifest;
use semver::{Version, VersionReq as SemverVersionReq};
use std::path::PathBuf;
use std::time::SystemTime;

#[derive(Debug, Clone)]
pub struct InstalledPackage {
    pub name: String,
    pub version: String,
    pub manifest: PackageManifest,
    pub install_path: PathBuf,
    pub installed_at: SystemTime,
}

#[derive(Debug, Clone)]
pub struct PackageSpec {
    pub name: String,
    pub version_req: VersionReq,
    pub registry: Option<String>,
}

/// Version requirement with proper semantic versioning support.
/// Supports: exact versions, ^, ~, >=, <=, >, <, and * (any)
#[derive(Debug, Clone)]
pub struct VersionReq {
    requirement: String,
    parsed: Option<SemverVersionReq>,
}

impl VersionReq {
    pub fn new(requirement: &str) -> Result<Self, String> {
        let trimmed = requirement.trim();
        if trimmed.is_empty() {
            return Err("Version requirement cannot be empty or whitespace-only".to_string());
        }

        // Handle wildcard
        if trimmed == "*" {
            return Ok(Self {
                requirement: trimmed.to_string(),
                parsed: Some(SemverVersionReq::STAR),
            });
        }

        // Try to parse as semver requirement
        let parsed = SemverVersionReq::parse(trimmed).ok();

        Ok(Self {
            requirement: trimmed.to_string(),
            parsed,
        })
    }

    pub fn parse(input: &str) -> Result<Self, String> {
        Self::new(input)
    }

    pub fn as_str(&self) -> &str {
        &self.requirement
    }

    /// Check if a version string matches this requirement
    pub fn matches(&self, version: &str) -> bool {
        // Wildcard matches everything
        if self.requirement == "*" {
            return true;
        }

        // Try to parse the version and check against semver requirement
        if let Some(ref req) = self.parsed {
            if let Ok(ver) = Version::parse(version) {
                return req.matches(&ver);
            }
        }

        // Fallback to exact string match
        self.requirement == version
    }
}

impl std::fmt::Display for VersionReq {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.requirement)
    }
}

impl PackageSpec {
    pub fn new(name: String, version_req: VersionReq) -> Self {
        Self {
            name,
            version_req,
            registry: None,
        }
    }

    pub fn with_registry(name: String, version_req: VersionReq, registry: String) -> Self {
        Self {
            name,
            version_req,
            registry: Some(registry),
        }
    }

    pub fn parse(spec: &str) -> Result<Self, String> {
        let parts: Vec<&str> = spec.split('@').collect();

        match parts.len() {
            1 => {
                // Just package name, default to latest
                let name = parts[0].to_string();
                let version_req = VersionReq::new("*")?;
                Ok(Self::new(name, version_req))
            }
            2 => {
                // Package name and version
                let name = parts[0].to_string();
                let version_req = VersionReq::new(parts[1])?;
                Ok(Self::new(name, version_req))
            }
            _ => Err(format!("Invalid package specification: {}", spec)),
        }
    }
}

impl InstalledPackage {
    pub fn identifier(&self) -> String {
        format!("{}@{}", self.name, self.version)
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
    use std::time::SystemTime;

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

    /// Strategy to generate registry URLs
    fn registry_url_strategy() -> impl Strategy<Value = String> {
        prop_oneof![
            Just("https://packages.houdini.org".to_string()),
            Just("https://custom-registry.example.com".to_string()),
            Just("https://internal.registry".to_string()),
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
            prop_assert!(spec.registry.is_none());
        }

        /// Test that package specs with registry URLs are handled correctly
        #[test]
        fn prop_package_spec_with_registry(
            name in package_name_strategy(),
            version in version_req_strategy(),
            registry in registry_url_strategy()
        ) {
            let version_req = VersionReq::new(&version).unwrap();
            let spec = PackageSpec::with_registry(name.clone(), version_req, registry.clone());

            prop_assert_eq!(spec.name, name);
            prop_assert_eq!(spec.version_req.as_str(), version);
            prop_assert_eq!(spec.registry, Some(registry));
        }

        /// Test that installed packages maintain identity consistency
        #[test]
        fn prop_installed_package_identity(
            name in package_name_strategy(),
            version in version_strategy(),
            path in file_path_strategy()
        ) {
            let manifest = hpm_package::PackageManifest::new(
                name.clone(),
                version.clone(),
                None,
                None,
                None,
            );

            let package = InstalledPackage {
                name: name.clone(),
                version: version.clone(),
                manifest,
                install_path: path,
                installed_at: SystemTime::now(),
            };

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
                name.clone(),
                package_version.clone(),
                None,
                None,
                None,
            );

            let package = InstalledPackage {
                name,
                version: package_version.clone(),
                manifest,
                install_path: path,
                installed_at: SystemTime::now(),
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
    fn package_spec_multiple_at_signs_fails() {
        let result = PackageSpec::parse("package@1.0.0@extra");
        assert!(result.is_err());
    }

    #[test]
    fn version_compatibility_edge_cases() {
        let manifest = hpm_package::PackageManifest::new(
            "test".to_string(),
            "1.0.0".to_string(),
            None,
            None,
            None,
        );

        let package = InstalledPackage {
            name: "test".to_string(),
            version: "1.0.0".to_string(),
            manifest,
            install_path: PathBuf::from("/path"),
            installed_at: SystemTime::now(),
        };

        // Test specific edge cases that property tests might miss
        assert!(package.is_compatible_with(&VersionReq::new("*").unwrap()));
        assert!(package.is_compatible_with(&VersionReq::new("1.0.0").unwrap()));
        assert!(!package.is_compatible_with(&VersionReq::new("2.0.0").unwrap()));
    }
}
