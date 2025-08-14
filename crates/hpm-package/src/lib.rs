use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// HPM package manifest (hpm.toml)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageManifest {
    pub package: PackageInfo,
    pub houdini: Option<HoudiniConfig>,
    pub dependencies: Option<HashMap<String, DependencySpec>>,
    #[serde(rename = "dev-dependencies")]
    pub dev_dependencies: Option<HashMap<String, DependencySpec>>,
    pub assets: Option<Vec<Asset>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageInfo {
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub authors: Option<Vec<String>>,
    pub license: Option<String>,
    pub keywords: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HoudiniConfig {
    pub min_version: Option<String>,
    pub max_version: Option<String>,
    pub contexts: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DependencySpec {
    Simple(String),
    Detailed {
        version: String,
        optional: Option<bool>,
        registry: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Asset {
    pub name: String,
    pub path: String,
    #[serde(rename = "type")]
    pub asset_type: String,
    pub contexts: Option<Vec<String>>,
}

impl PackageManifest {
    /// Validate the package manifest for common errors
    pub fn validate(&self) -> Result<(), String> {
        if self.package.name.is_empty() {
            return Err("Package name cannot be empty".to_string());
        }

        if self.package.version.is_empty() {
            return Err("Package version cannot be empty".to_string());
        }

        // Basic semver validation
        if !self.is_valid_semver(&self.package.version) {
            return Err("Package version must be valid semantic version".to_string());
        }

        Ok(())
    }

    fn is_valid_semver(&self, version: &str) -> bool {
        // Basic semver pattern: major.minor.patch
        let parts: Vec<&str> = version.split('.').collect();
        if parts.len() != 3 {
            return false;
        }

        parts.iter().all(|part| part.parse::<u32>().is_ok())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_manifest_validation() {
        let manifest = PackageManifest {
            package: PackageInfo {
                name: "test-package".to_string(),
                version: "1.0.0".to_string(),
                description: Some("A test package".to_string()),
                authors: Some(vec!["Author <author@example.com>".to_string()]),
                license: Some("MIT".to_string()),
                keywords: Some(vec!["houdini".to_string(), "test".to_string()]),
            },
            houdini: Some(HoudiniConfig {
                min_version: Some("20.0".to_string()),
                max_version: Some("21.0".to_string()),
                contexts: Some(vec!["sop".to_string()]),
            }),
            dependencies: None,
            dev_dependencies: None,
            assets: None,
        };

        assert!(manifest.validate().is_ok());
    }

    #[test]
    fn manifest_validation_fails_empty_name() {
        let manifest = PackageManifest {
            package: PackageInfo {
                name: "".to_string(),
                version: "1.0.0".to_string(),
                description: None,
                authors: None,
                license: None,
                keywords: None,
            },
            houdini: None,
            dependencies: None,
            dev_dependencies: None,
            assets: None,
        };

        assert!(manifest.validate().is_err());
    }

    #[test]
    fn manifest_validation_fails_invalid_version() {
        let manifest = PackageManifest {
            package: PackageInfo {
                name: "test".to_string(),
                version: "invalid".to_string(),
                description: None,
                authors: None,
                license: None,
                keywords: None,
            },
            houdini: None,
            dependencies: None,
            dev_dependencies: None,
            assets: None,
        };

        assert!(manifest.validate().is_err());
    }

    #[test]
    fn dependency_spec_serialization() {
        let simple = DependencySpec::Simple("^1.0.0".to_string());
        let detailed = DependencySpec::Detailed {
            version: "1.5".to_string(),
            optional: Some(true),
            registry: None,
        };

        let simple_json = serde_json::to_string(&simple).unwrap();
        let detailed_json = serde_json::to_string(&detailed).unwrap();

        assert_eq!(simple_json, r#""^1.0.0""#);
        assert!(detailed_json.contains("version"));
        assert!(detailed_json.contains("optional"));
    }
}
