use hpm_package::PackageManifest;
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

#[derive(Debug, Clone)]
pub struct VersionReq {
    requirement: String,
}

impl VersionReq {
    pub fn new(requirement: &str) -> Result<Self, String> {
        // Basic validation for now
        if requirement.is_empty() {
            return Err("Version requirement cannot be empty".to_string());
        }

        Ok(Self {
            requirement: requirement.to_string(),
        })
    }

    pub fn parse(input: &str) -> Result<Self, String> {
        Self::new(input)
    }

    pub fn as_str(&self) -> &str {
        &self.requirement
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

    pub fn is_compatible_with(&self, version_req: &VersionReq) -> bool {
        // TODO: Implement proper version matching
        // For now, just do exact match or wildcard
        match version_req.as_str() {
            "*" => true,
            req if req == self.version => true,
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_req_creation() {
        let version_req = VersionReq::new("1.0.0").unwrap();
        assert_eq!(version_req.as_str(), "1.0.0");
        assert_eq!(version_req.to_string(), "1.0.0");
    }

    #[test]
    fn version_req_empty_fails() {
        let result = VersionReq::new("");
        assert!(result.is_err());
    }

    #[test]
    fn package_spec_parsing() {
        let spec = PackageSpec::parse("test-package").unwrap();
        assert_eq!(spec.name, "test-package");
        assert_eq!(spec.version_req.as_str(), "*");
        assert!(spec.registry.is_none());

        let spec = PackageSpec::parse("test-package@1.0.0").unwrap();
        assert_eq!(spec.name, "test-package");
        assert_eq!(spec.version_req.as_str(), "1.0.0");
        assert!(spec.registry.is_none());
    }

    #[test]
    fn package_spec_invalid_parsing() {
        let result = PackageSpec::parse("test-package@1.0.0@extra");
        assert!(result.is_err());
    }

    #[test]
    fn package_spec_with_registry() {
        let version_req = VersionReq::new("1.0.0").unwrap();
        let spec = PackageSpec::with_registry(
            "test-package".to_string(),
            version_req,
            "https://custom.registry".to_string(),
        );

        assert_eq!(spec.name, "test-package");
        assert_eq!(spec.version_req.as_str(), "1.0.0");
        assert_eq!(spec.registry, Some("https://custom.registry".to_string()));
    }

    #[test]
    fn installed_package_identifier() {
        let manifest = hpm_package::PackageManifest::new(
            "test-package".to_string(),
            "1.0.0".to_string(),
            None,
            None,
            None,
        );

        let package = InstalledPackage {
            name: "test-package".to_string(),
            version: "1.0.0".to_string(),
            manifest,
            install_path: PathBuf::from("/test/path"),
            installed_at: SystemTime::now(),
        };

        assert_eq!(package.identifier(), "test-package@1.0.0");
    }

    #[test]
    fn installed_package_compatibility() {
        let manifest = hpm_package::PackageManifest::new(
            "test-package".to_string(),
            "1.0.0".to_string(),
            None,
            None,
            None,
        );

        let package = InstalledPackage {
            name: "test-package".to_string(),
            version: "1.0.0".to_string(),
            manifest,
            install_path: PathBuf::from("/test/path"),
            installed_at: SystemTime::now(),
        };

        let wildcard_req = VersionReq::new("*").unwrap();
        assert!(package.is_compatible_with(&wildcard_req));

        let exact_req = VersionReq::new("1.0.0").unwrap();
        assert!(package.is_compatible_with(&exact_req));

        let different_req = VersionReq::new("2.0.0").unwrap();
        assert!(!package.is_compatible_with(&different_req));
    }
}
