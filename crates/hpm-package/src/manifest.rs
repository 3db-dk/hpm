//! Package manifest types and implementation.
//!
//! This module defines the core `PackageManifest` type that represents an `hpm.toml` file,
//! along with related configuration types for package metadata and Houdini integration.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::dependency::DependencySpec;
use crate::houdini::{HoudiniEnvValue, HoudiniPackage};
use crate::python::PythonDependencySpec;

/// HPM package manifest (hpm.toml)
///
/// Uses `IndexMap` for dependencies and python_dependencies to preserve
/// insertion order during serialization, ensuring deterministic TOML output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageManifest {
    pub package: PackageInfo,
    pub houdini: Option<HoudiniConfig>,
    pub dependencies: Option<IndexMap<String, DependencySpec>>,
    pub python_dependencies: Option<IndexMap<String, PythonDependencySpec>>,
    pub scripts: Option<HashMap<String, String>>,
}

/// Package metadata information
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

/// Houdini version compatibility configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HoudiniConfig {
    pub min_version: Option<String>,
    pub max_version: Option<String>,
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
            python_dependencies: None,
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
    fn empty_name_fails_validation() {
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
            python_dependencies: None,
            scripts: None,
        };
        assert!(manifest.validate().is_err());
    }

    #[test]
    fn empty_version_fails_validation() {
        let manifest = PackageManifest {
            package: PackageInfo {
                name: "test".to_string(),
                version: "".to_string(),
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
            python_dependencies: None,
            scripts: None,
        };
        assert!(manifest.validate().is_err());
    }

    #[test]
    fn houdini_package_no_version_constraints() {
        let mut manifest =
            PackageManifest::new("test".to_string(), "1.0.0".to_string(), None, None, None);

        // Remove version constraints
        manifest.houdini = Some(HoudiniConfig {
            min_version: None,
            max_version: None,
        });

        let houdini_pkg = manifest.generate_houdini_package();
        assert!(houdini_pkg.enable.is_none());
    }
}
