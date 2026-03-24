//! Package manifest types and implementation.
//!
//! This module defines the core `PackageManifest` type that represents an `hpm.toml` file,
//! along with related configuration types for package metadata and Houdini integration.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::dependency::DependencySpec;
use crate::houdini::{HoudiniEnvValue, HoudiniPackage};
use crate::platform::Platform;
use crate::python::PythonDependencySpec;

/// The type of registry backend.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RegistryType {
    Api,
    Git,
}

/// Method for applying an environment variable value.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum EnvMethod {
    Set,
    Prepend,
    Append,
}

/// An environment variable entry declared in `[env]`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestEnvEntry {
    pub method: EnvMethod,
    pub value: String,
}

/// Native platform configuration for multi-architecture packaging.
///
/// Declares which files belong to which platform, enabling per-platform archives.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NativeConfig {
    /// Declared platforms this package supports.
    pub platforms: Vec<String>,
    /// Per-platform file glob patterns, keyed by platform string.
    /// Deserialized from `[native.linux-x86_64]` etc.
    #[serde(flatten)]
    pub platform_files: IndexMap<String, NativePlatformFiles>,
}

/// Files for a single platform.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NativePlatformFiles {
    /// Glob patterns matching files belonging to this platform.
    pub files: Vec<String>,
}

/// A registry declared in hpm.toml's `[[registries]]` array.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryConfig {
    pub name: String,
    pub url: String,
    #[serde(rename = "type")]
    pub registry_type: RegistryType,
}

/// HPM package manifest (hpm.toml)
///
/// Uses `IndexMap` for dependencies and python_dependencies to preserve
/// insertion order during serialization, ensuring deterministic TOML output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageManifest {
    pub package: PackageInfo,
    pub houdini: Option<HoudiniConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native: Option<NativeConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub registries: Option<Vec<RegistryConfig>>,
    pub dependencies: Option<IndexMap<String, DependencySpec>>,
    pub python_dependencies: Option<IndexMap<String, PythonDependencySpec>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env: Option<IndexMap<String, ManifestEnvEntry>>,
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

impl ManifestEnvEntry {
    /// Convert to a Houdini environment variable value.
    pub fn to_houdini_env_value(&self) -> HoudiniEnvValue {
        let method = match self.method {
            EnvMethod::Set => "set",
            EnvMethod::Prepend => "prepend",
            EnvMethod::Append => "append",
        };
        HoudiniEnvValue::Detailed {
            method: method.to_string(),
            value: self.value.clone(),
        }
    }
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
            native: None,
            registries: None,
            dependencies: None,
            python_dependencies: None,
            env: None,
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

        // Validate [native] section
        if let Some(native) = &self.native {
            for platform_str in &native.platforms {
                platform_str
                    .parse::<Platform>()
                    .map_err(|e| e.to_string())?;
            }
            for key in native.platform_files.keys() {
                if !native.platforms.contains(key) {
                    return Err(format!(
                        "[native.{}] declared but '{}' not listed in native.platforms",
                        key, key
                    ));
                }
            }
            for platform_str in &native.platforms {
                if !native.platform_files.contains_key(platform_str) {
                    return Err(format!(
                        "Platform '{}' listed in native.platforms but has no [native.{}] section",
                        platform_str, platform_str
                    ));
                }
            }
            for (platform_str, files) in &native.platform_files {
                if files.files.is_empty() {
                    return Err(format!(
                        "[native.{}] files array must not be empty",
                        platform_str
                    ));
                }
            }
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

        // Append user-defined env vars from [env] section
        if let Some(user_env) = &self.env {
            for (key, entry) in user_env {
                let mut env_map = HashMap::new();
                env_map.insert(key.clone(), entry.to_houdini_env_value());
                env.push(env_map);
            }
        }

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

    // empty_name/empty_version validation tests removed - covered by
    // prop_malformed_package_names_rejected and prop_malformed_versions_rejected
    // in proptest_helpers.rs which test validation with randomized inputs

    #[test]
    fn houdini_package_no_version_constraints() {
        // Edge case: Houdini package generation without version constraints
        let mut manifest =
            PackageManifest::new("test".to_string(), "1.0.0".to_string(), None, None, None);

        manifest.houdini = Some(HoudiniConfig {
            min_version: None,
            max_version: None,
        });

        let houdini_pkg = manifest.generate_houdini_package();
        assert!(houdini_pkg.enable.is_none());
    }

    #[test]
    fn registries_deserialize_from_toml() {
        let toml_str = r#"
[package]
name = "my-context"
version = "0.1.0"

[[registries]]
name = "houdinihub"
url = "https://api.3db.dk/v1/registry"
type = "api"

[[registries]]
name = "studio-internal"
url = "https://packages.studio.com/v1/registry"
type = "git"

[dependencies]
test = "0.2.0"
"#;
        let manifest: PackageManifest = toml::from_str(toml_str).unwrap();
        let registries = manifest.registries.unwrap();
        assert_eq!(registries.len(), 2);
        assert_eq!(registries[0].name, "houdinihub");
        assert_eq!(registries[0].registry_type, RegistryType::Api);
        assert_eq!(registries[1].name, "studio-internal");
        assert_eq!(registries[1].registry_type, RegistryType::Git);
    }

    #[test]
    fn env_deserialize_from_toml() {
        let toml_str = r#"
[package]
name = "my-package"
version = "0.1.0"

[env]
MY_PLUGIN_ROOT = { method = "set", value = "$HPM_PACKAGE_ROOT/config" }
HOUDINI_TOOLBAR_PATH = { method = "prepend", value = "$HPM_PACKAGE_ROOT/toolbar" }
"#;
        let manifest: PackageManifest = toml::from_str(toml_str).unwrap();
        let env = manifest.env.unwrap();
        assert_eq!(env.len(), 2);
        assert_eq!(env["MY_PLUGIN_ROOT"].method, EnvMethod::Set);
        assert_eq!(env["MY_PLUGIN_ROOT"].value, "$HPM_PACKAGE_ROOT/config");
        assert_eq!(env["HOUDINI_TOOLBAR_PATH"].method, EnvMethod::Prepend);
    }

    #[test]
    fn env_invalid_method_rejected() {
        let toml_str = r#"
[package]
name = "my-package"
version = "0.1.0"

[env]
MY_VAR = { method = "invalid", value = "foo" }
"#;
        let result: Result<PackageManifest, _> = toml::from_str(toml_str);
        assert!(result.is_err());
    }

    #[test]
    fn env_none_when_absent() {
        let toml_str = r#"
[package]
name = "my-package"
version = "0.1.0"
"#;
        let manifest: PackageManifest = toml::from_str(toml_str).unwrap();
        assert!(manifest.env.is_none());
    }

    #[test]
    fn generate_houdini_package_includes_user_env() {
        let mut manifest =
            PackageManifest::new("test".to_string(), "1.0.0".to_string(), None, None, None);

        let mut env = IndexMap::new();
        env.insert(
            "MY_VAR".to_string(),
            ManifestEnvEntry {
                method: EnvMethod::Set,
                value: "$HPM_PACKAGE_ROOT/data".to_string(),
            },
        );
        manifest.env = Some(env);

        let houdini_pkg = manifest.generate_houdini_package();
        let env_list = houdini_pkg.env.unwrap();
        // 2 hardcoded (PYTHONPATH, HOUDINI_SCRIPT_PATH) + 1 user-defined
        assert_eq!(env_list.len(), 3);
        let last = &env_list[2];
        let val = last.get("MY_VAR").unwrap();
        match val {
            HoudiniEnvValue::Detailed { method, value } => {
                assert_eq!(method, "set");
                assert_eq!(value, "$HPM_PACKAGE_ROOT/data");
            }
            _ => panic!("Expected Detailed variant"),
        }
    }

    #[test]
    fn native_deserialize_from_toml() {
        let toml_str = r#"
[package]
name = "my-native-pkg"
version = "1.0.0"

[native]
platforms = ["linux-x86_64", "macos-universal"]

[native.linux-x86_64]
files = ["lib/linux-x86_64/*"]

[native.macos-universal]
files = ["lib/macos-universal/*"]
"#;
        let manifest: PackageManifest = toml::from_str(toml_str).unwrap();
        let native = manifest.native.unwrap();
        assert_eq!(native.platforms.len(), 2);
        assert_eq!(native.platform_files.len(), 2);
        assert_eq!(
            native.platform_files["linux-x86_64"].files,
            vec!["lib/linux-x86_64/*"]
        );
    }

    #[test]
    fn native_none_when_absent() {
        let toml_str = r#"
[package]
name = "my-package"
version = "0.1.0"
"#;
        let manifest: PackageManifest = toml::from_str(toml_str).unwrap();
        assert!(manifest.native.is_none());
    }

    #[test]
    fn native_validation_unknown_platform() {
        let mut manifest =
            PackageManifest::new("test".to_string(), "1.0.0".to_string(), None, None, None);
        manifest.native = Some(NativeConfig {
            platforms: vec!["linux-arm64".to_string()],
            platform_files: IndexMap::new(),
        });
        assert!(manifest.validate().is_err());
    }

    #[test]
    fn native_validation_missing_files_section() {
        let mut manifest =
            PackageManifest::new("test".to_string(), "1.0.0".to_string(), None, None, None);
        manifest.native = Some(NativeConfig {
            platforms: vec!["linux-x86_64".to_string()],
            platform_files: IndexMap::new(),
        });
        let err = manifest.validate().unwrap_err();
        assert!(err.contains("has no [native.linux-x86_64] section"));
    }

    #[test]
    fn native_validation_empty_files() {
        let mut manifest =
            PackageManifest::new("test".to_string(), "1.0.0".to_string(), None, None, None);
        let mut platform_files = IndexMap::new();
        platform_files.insert(
            "linux-x86_64".to_string(),
            NativePlatformFiles { files: vec![] },
        );
        manifest.native = Some(NativeConfig {
            platforms: vec!["linux-x86_64".to_string()],
            platform_files,
        });
        let err = manifest.validate().unwrap_err();
        assert!(err.contains("files array must not be empty"));
    }

    #[test]
    fn native_validation_extra_platform_files() {
        let mut manifest =
            PackageManifest::new("test".to_string(), "1.0.0".to_string(), None, None, None);
        let mut platform_files = IndexMap::new();
        platform_files.insert(
            "windows-x86_64".to_string(),
            NativePlatformFiles {
                files: vec!["lib/*".to_string()],
            },
        );
        manifest.native = Some(NativeConfig {
            platforms: vec!["linux-x86_64".to_string()],
            platform_files,
        });
        let err = manifest.validate().unwrap_err();
        assert!(err.contains("not listed in native.platforms"));
    }

    #[test]
    fn registries_none_when_absent() {
        let toml_str = r#"
[package]
name = "my-context"
version = "0.1.0"
"#;
        let manifest: PackageManifest = toml::from_str(toml_str).unwrap();
        assert!(manifest.registries.is_none());
    }
}
