use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub mod template;

pub use template::PackageTemplate;

/// HPM package manifest (hpm.toml)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageManifest {
    pub package: PackageInfo,
    pub houdini: Option<HoudiniConfig>,
    pub dependencies: Option<HashMap<String, DependencySpec>>,
    pub scripts: Option<HashMap<String, String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageInfo {
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub authors: Option<Vec<String>>,
    pub license: Option<String>,
    pub readme: Option<String>,
    pub homepage: Option<String>,
    pub repository: Option<String>,
    pub documentation: Option<String>,
    pub keywords: Option<Vec<String>>,
    pub categories: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HoudiniConfig {
    pub min_version: Option<String>,
    pub max_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DependencySpec {
    Simple(String),
    Detailed {
        version: Option<String>,
        git: Option<String>,
        tag: Option<String>,
        branch: Option<String>,
        optional: Option<bool>,
        registry: Option<String>,
    },
}

/// Houdini package.json structure for generation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HoudiniPackage {
    pub hpath: Option<Vec<String>>,
    pub env: Option<Vec<HashMap<String, HoudiniEnvValue>>>,
    pub enable: Option<String>,
    pub requires: Option<Vec<String>>,
    pub recommends: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum HoudiniEnvValue {
    Simple(String),
    Detailed { method: String, value: String },
}

impl PackageManifest {
    /// Create a new package manifest with default values
    pub fn new(
        name: String,
        version: String,
        description: Option<String>,
        authors: Option<Vec<String>>,
        license: Option<String>,
    ) -> Self {
        Self {
            package: PackageInfo {
                name,
                version,
                description,
                authors,
                license,
                readme: Some("README.md".to_string()),
                homepage: None,
                repository: None,
                documentation: None,
                keywords: Some(vec!["houdini".to_string()]),
                categories: None,
            },
            houdini: Some(HoudiniConfig {
                min_version: Some("19.5".to_string()),
                max_version: None,
            }),
            dependencies: None,
            scripts: None,
        }
    }

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

        // Validate package name (kebab-case recommended)
        if !self.is_valid_package_name(&self.package.name) {
            return Err("Package name should be kebab-case (lowercase with hyphens)".to_string());
        }

        Ok(())
    }

    /// Generate Houdini package.json from manifest
    pub fn generate_houdini_package(&self) -> HoudiniPackage {
        let mut hpath = vec![];
        let mut env = vec![];

        // Add common paths
        hpath.push("$HPM_PACKAGE_ROOT/otls".to_string());

        // Python path environment
        let mut python_env = HashMap::new();
        python_env.insert(
            "PYTHONPATH".to_string(),
            HoudiniEnvValue::Detailed {
                method: "prepend".to_string(),
                value: "$HPM_PACKAGE_ROOT/python".to_string(),
            },
        );
        env.push(python_env);

        // Scripts path environment
        let mut scripts_env = HashMap::new();
        scripts_env.insert(
            "HOUDINI_SCRIPT_PATH".to_string(),
            HoudiniEnvValue::Detailed {
                method: "prepend".to_string(),
                value: "$HPM_PACKAGE_ROOT/scripts".to_string(),
            },
        );
        env.push(scripts_env);

        // Generate version constraint
        let enable = if let Some(houdini_config) = &self.houdini {
            let mut conditions = vec![];

            if let Some(min_version) = &houdini_config.min_version {
                conditions.push(format!("houdini_version >= '{}'", min_version));
            }

            if let Some(max_version) = &houdini_config.max_version {
                conditions.push(format!("houdini_version <= '{}'", max_version));
            }

            if conditions.is_empty() {
                None
            } else {
                Some(conditions.join(" and "))
            }
        } else {
            None
        };

        HoudiniPackage {
            hpath: Some(hpath),
            env: Some(env),
            enable,
            requires: None,
            recommends: None,
        }
    }

    fn is_valid_semver(&self, version: &str) -> bool {
        // Basic semver pattern: major.minor.patch
        let parts: Vec<&str> = version.split('.').collect();
        if parts.len() != 3 {
            return false;
        }

        parts.iter().all(|part| part.parse::<u32>().is_ok())
    }

    fn is_valid_package_name(&self, name: &str) -> bool {
        // Basic validation for package name
        !name.is_empty()
            && name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c == '-' || c.is_ascii_digit())
            && !name.starts_with('-')
            && !name.ends_with('-')
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
                readme: None,
                homepage: None,
                repository: None,
                documentation: None,
                keywords: Some(vec!["houdini".to_string(), "test".to_string()]),
                categories: None,
            },
            houdini: Some(HoudiniConfig {
                min_version: Some("20.0".to_string()),
                max_version: Some("21.0".to_string()),
            }),
            dependencies: None,
            scripts: None,
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
                readme: None,
                homepage: None,
                repository: None,
                documentation: None,
                keywords: None,
                categories: None,
            },
            houdini: None,
            dependencies: None,
            scripts: None,
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
                readme: None,
                homepage: None,
                repository: None,
                documentation: None,
                keywords: None,
                categories: None,
            },
            houdini: None,
            dependencies: None,
            scripts: None,
        };

        assert!(manifest.validate().is_err());
    }

    #[test]
    fn dependency_spec_serialization() {
        let simple = DependencySpec::Simple("^1.0.0".to_string());
        let detailed = DependencySpec::Detailed {
            version: Some("1.5".to_string()),
            git: None,
            tag: None,
            branch: None,
            optional: Some(true),
            registry: None,
        };

        let simple_json = serde_json::to_string(&simple).unwrap();
        let detailed_json = serde_json::to_string(&detailed).unwrap();

        assert_eq!(simple_json, r#""^1.0.0""#);
        assert!(detailed_json.contains("version"));
        assert!(detailed_json.contains("optional"));
    }

    #[test]
    fn package_manifest_creation() {
        let manifest = PackageManifest::new(
            "test-package".to_string(),
            "1.0.0".to_string(),
            Some("A test package".to_string()),
            Some(vec!["Author <author@example.com>".to_string()]),
            Some("MIT".to_string()),
        );

        assert_eq!(manifest.package.name, "test-package");
        assert_eq!(manifest.package.version, "1.0.0");
        assert!(manifest.houdini.is_some());
        assert!(manifest.validate().is_ok());
    }

    #[test]
    fn houdini_package_generation() {
        let manifest = PackageManifest::new(
            "test-package".to_string(),
            "1.0.0".to_string(),
            None,
            None,
            None,
        );

        let houdini_pkg = manifest.generate_houdini_package();

        assert!(houdini_pkg.hpath.is_some());
        assert!(houdini_pkg.env.is_some());
        assert!(houdini_pkg.enable.is_some());
    }

    #[test]
    fn package_name_validation() {
        let valid_manifest = PackageManifest::new(
            "my-package".to_string(),
            "1.0.0".to_string(),
            None,
            None,
            None,
        );
        assert!(valid_manifest.validate().is_ok());

        let invalid_manifest = PackageManifest::new(
            "My_Package".to_string(),
            "1.0.0".to_string(),
            None,
            None,
            None,
        );
        assert!(invalid_manifest.validate().is_err());
    }
}
