//! Package manifest types and implementation.
//!
//! This module defines the core `PackageManifest` type that represents an `hpm.toml` file,
//! along with related configuration types for package metadata and Houdini integration.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::dependency::DependencySpec;
use crate::houdini::{HoudiniEnvValue, HoudiniNativePackage, HoudiniPackage, HpackageMetadata};
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
    /// Scoped package path: `creator-slug/package-slug` (e.g. `tumblehead/tumble-rig`)
    pub path: String,
    /// Freeform display name (e.g. `TumbleRig`)
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

impl PackageInfo {
    /// Returns the scoped path (the canonical identifier)
    pub fn identifier(&self) -> &str {
        &self.path
    }

    /// Returns the creator slug from the path (e.g. `tumblehead` from `tumblehead/tumble-rig`)
    pub fn creator(&self) -> Option<&str> {
        self.path.split_once('/').map(|(creator, _)| creator)
    }

    /// Returns the package slug from the path (e.g. `tumble-rig` from `tumblehead/tumble-rig`)
    pub fn slug(&self) -> Option<&str> {
        self.path.split_once('/').map(|(_, slug)| slug)
    }
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
        path: String,
        name: String,
        version: String,
        description: Option<String>,
        authors: Option<Vec<String>>,
        license: Option<String>,
    ) -> Self {
        Self {
            package: PackageInfo {
                path,
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
        if self.package.path.is_empty() {
            return Err("Package path cannot be empty".to_string());
        }

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

        // Validate package path (creator-slug/package-slug)
        if !Self::is_valid_package_path(&self.package.path) {
            return Err("Package path must be creator/slug format (lowercase kebab-case segments separated by /)".to_string());
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

    /// Generate a Houdini-native package.json for direct use by Houdini's package system.
    ///
    /// Returns `(filename, package)` where filename is `{slug}.json`.
    /// Uses `$HOUDINI_PACKAGE_PATH/{slug}` instead of `$HPM_PACKAGE_ROOT`.
    pub fn generate_houdini_native_package(
        &self,
    ) -> Result<(String, HoudiniNativePackage), String> {
        let slug = self
            .package
            .slug()
            .ok_or("Package path must be in creator/slug format")?
            .to_string();

        let pkg_root = format!("$HOUDINI_PACKAGE_PATH/{}", slug);

        // Build PKG_{SLUG_UPPER} env var name
        let slug_upper = slug.replace('-', "_").to_uppercase();
        let pkg_var_name = format!("PKG_{}", slug_upper);

        // First env entry: PKG_{SLUG_UPPER} pointing to package root
        let mut env = Vec::new();
        let mut pkg_env = HashMap::new();
        pkg_env.insert(pkg_var_name, HoudiniEnvValue::simple(&pkg_root));
        env.push(pkg_env);

        // User-defined env vars with $HPM_PACKAGE_ROOT replaced
        if let Some(user_env) = &self.env {
            for (key, entry) in user_env {
                let mut env_map = HashMap::new();
                let value = entry.value.replace("$HPM_PACKAGE_ROOT", &pkg_root);
                let houdini_value = match entry.method {
                    EnvMethod::Set => HoudiniEnvValue::set(value),
                    EnvMethod::Prepend => HoudiniEnvValue::prepend(value),
                    EnvMethod::Append => HoudiniEnvValue::append(value),
                };
                env_map.insert(key.clone(), houdini_value);
                env.push(env_map);
            }
        }

        // Build enable from houdini version constraints
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

        // Build requires from dependency keys (slug portion only)
        let requires = self.dependencies.as_ref().and_then(|deps| {
            let names: Vec<String> = deps
                .keys()
                .map(|key| {
                    key.split_once('/')
                        .map(|(_, s)| s)
                        .unwrap_or(key)
                        .to_string()
                })
                .collect();
            if names.is_empty() { None } else { Some(names) }
        });

        let filename = format!("{}.json", slug);
        let package = HoudiniNativePackage {
            name: slug,
            hpath: pkg_root,
            load_package_once: true,
            show: true,
            enable,
            env,
            requires,
            hpackage: HpackageMetadata {
                version: self.package.version.clone(),
            },
        };

        Ok((filename, package))
    }

    fn is_valid_semver(&self, version: &str) -> bool {
        // Basic semver pattern: major.minor.patch
        let parts: Vec<&str> = version.split('.').collect();
        if parts.len() != 3 {
            return false;
        }

        parts.iter().all(|part| part.parse::<u32>().is_ok())
    }

    /// Validate a package path in `creator/slug` format.
    /// Both segments must be kebab-case (lowercase alphanumeric + hyphens).
    pub fn is_valid_package_path(path: &str) -> bool {
        match path.split_once('/') {
            Some((creator, slug)) => Self::is_valid_slug(creator) && Self::is_valid_slug(slug),
            None => false,
        }
    }

    /// Validate a single slug segment (kebab-case).
    pub fn is_valid_slug(slug: &str) -> bool {
        !slug.is_empty()
            && slug
                .chars()
                .all(|c| c.is_ascii_lowercase() || c == '-' || c.is_ascii_digit())
            && !slug.starts_with('-')
            && !slug.ends_with('-')
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
        let mut manifest = PackageManifest::new(
            "studio/test".to_string(),
            "Test".to_string(),
            "1.0.0".to_string(),
            None,
            None,
            None,
        );

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
path = "studio/my-context"
name = "My Context"
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
"studio/test" = "0.2.0"
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
path = "studio/my-package"
name = "My Package"
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
path = "studio/my-package"
name = "My Package"
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
path = "studio/my-package"
name = "My Package"
version = "0.1.0"
"#;
        let manifest: PackageManifest = toml::from_str(toml_str).unwrap();
        assert!(manifest.env.is_none());
    }

    #[test]
    fn generate_houdini_package_includes_user_env() {
        let mut manifest = PackageManifest::new(
            "studio/test".to_string(),
            "Test".to_string(),
            "1.0.0".to_string(),
            None,
            None,
            None,
        );

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
path = "studio/my-native-pkg"
name = "My Native Pkg"
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
path = "studio/my-package"
name = "My Package"
version = "0.1.0"
"#;
        let manifest: PackageManifest = toml::from_str(toml_str).unwrap();
        assert!(manifest.native.is_none());
    }

    #[test]
    fn native_validation_unknown_platform() {
        let mut manifest = PackageManifest::new(
            "studio/test".to_string(),
            "Test".to_string(),
            "1.0.0".to_string(),
            None,
            None,
            None,
        );
        manifest.native = Some(NativeConfig {
            platforms: vec!["linux-arm64".to_string()],
            platform_files: IndexMap::new(),
        });
        assert!(manifest.validate().is_err());
    }

    #[test]
    fn native_validation_missing_files_section() {
        let mut manifest = PackageManifest::new(
            "studio/test".to_string(),
            "Test".to_string(),
            "1.0.0".to_string(),
            None,
            None,
            None,
        );
        manifest.native = Some(NativeConfig {
            platforms: vec!["linux-x86_64".to_string()],
            platform_files: IndexMap::new(),
        });
        let err = manifest.validate().unwrap_err();
        assert!(err.contains("has no [native.linux-x86_64] section"));
    }

    #[test]
    fn native_validation_empty_files() {
        let mut manifest = PackageManifest::new(
            "studio/test".to_string(),
            "Test".to_string(),
            "1.0.0".to_string(),
            None,
            None,
            None,
        );
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
        let mut manifest = PackageManifest::new(
            "studio/test".to_string(),
            "Test".to_string(),
            "1.0.0".to_string(),
            None,
            None,
            None,
        );
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
    fn valid_package_path() {
        assert!(PackageManifest::is_valid_package_path(
            "tumblehead/tumble-rig"
        ));
        assert!(PackageManifest::is_valid_package_path("studio/fire-fx"));
        assert!(PackageManifest::is_valid_package_path("a/b"));
        assert!(PackageManifest::is_valid_package_path("creator123/pkg456"));
    }

    #[test]
    fn invalid_package_path() {
        assert!(!PackageManifest::is_valid_package_path("flat-name"));
        assert!(!PackageManifest::is_valid_package_path(""));
        assert!(!PackageManifest::is_valid_package_path("a/b/c"));
        assert!(!PackageManifest::is_valid_package_path("Upper/case"));
        assert!(!PackageManifest::is_valid_package_path("creator/"));
        assert!(!PackageManifest::is_valid_package_path("/slug"));
        assert!(!PackageManifest::is_valid_package_path("-bad/slug"));
        assert!(!PackageManifest::is_valid_package_path("creator/-bad"));
    }

    #[test]
    fn package_info_helpers() {
        let info = PackageInfo {
            path: "tumblehead/tumble-rig".to_string(),
            name: "TumbleRig".to_string(),
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
        };
        assert_eq!(info.identifier(), "tumblehead/tumble-rig");
        assert_eq!(info.creator(), Some("tumblehead"));
        assert_eq!(info.slug(), Some("tumble-rig"));
    }

    #[test]
    fn manifest_toml_roundtrip_with_path() {
        let manifest = PackageManifest::new(
            "tumblehead/tumble-rig".to_string(),
            "TumbleRig".to_string(),
            "1.0.0".to_string(),
            Some("A rig tool".to_string()),
            None,
            Some("MIT".to_string()),
        );
        let toml_str = toml::to_string(&manifest).unwrap();
        assert!(toml_str.contains("path = \"tumblehead/tumble-rig\""));
        assert!(toml_str.contains("name = \"TumbleRig\""));

        let deserialized: PackageManifest = toml::from_str(&toml_str).unwrap();
        assert_eq!(deserialized.package.path, "tumblehead/tumble-rig");
        assert_eq!(deserialized.package.name, "TumbleRig");
    }

    #[test]
    fn registries_none_when_absent() {
        let toml_str = r#"
[package]
path = "studio/my-context"
name = "My Context"
version = "0.1.0"
"#;
        let manifest: PackageManifest = toml::from_str(toml_str).unwrap();
        assert!(manifest.registries.is_none());
    }

    #[test]
    fn generate_houdini_native_package_full() {
        let toml_str = r#"
[package]
path = "creator/my-tool"
name = "My Cool Tool"
version = "1.2.3"

[houdini]
min_version = "21.0"

[dependencies]
"studio/some-dep" = "1.0.0"

[env]
MY_VAR = { method = "prepend", value = "$HPM_PACKAGE_ROOT/scripts" }
"#;
        let manifest: PackageManifest = toml::from_str(toml_str).unwrap();
        let (filename, pkg) = manifest.generate_houdini_native_package().unwrap();

        assert_eq!(filename, "my-tool.json");
        assert_eq!(pkg.name, "my-tool");
        assert_eq!(pkg.hpath, "$HOUDINI_PACKAGE_PATH/my-tool");
        assert!(pkg.load_package_once);
        assert!(pkg.show);
        assert_eq!(pkg.enable.as_deref(), Some("houdini_version >= '21.0'"));
        assert_eq!(pkg.hpackage.version, "1.2.3");

        // First env entry is PKG_MY_TOOL
        let first_env = &pkg.env[0];
        assert!(first_env.contains_key("PKG_MY_TOOL"));

        // Second env entry has $HPM_PACKAGE_ROOT replaced
        let second_env = &pkg.env[1];
        let my_var = second_env.get("MY_VAR").unwrap();
        match my_var {
            crate::houdini::HoudiniEnvValue::Detailed { value, method } => {
                assert_eq!(value, "$HOUDINI_PACKAGE_PATH/my-tool/scripts");
                assert_eq!(method, "prepend");
            }
            _ => panic!("Expected Detailed variant"),
        }

        // Requires uses slug only
        assert_eq!(pkg.requires, Some(vec!["some-dep".to_string()]));
    }

    #[test]
    fn generate_houdini_native_package_no_deps_no_houdini() {
        let toml_str = r#"
[package]
path = "studio/bare-pkg"
name = "Bare Package"
version = "0.1.0"
"#;
        let manifest: PackageManifest = toml::from_str(toml_str).unwrap();
        let (filename, pkg) = manifest.generate_houdini_native_package().unwrap();

        assert_eq!(filename, "bare-pkg.json");
        assert!(pkg.enable.is_none());
        assert!(pkg.requires.is_none());
        // Only the PKG_ env entry
        assert_eq!(pkg.env.len(), 1);
        assert!(pkg.env[0].contains_key("PKG_BARE_PKG"));
    }

    #[test]
    fn generate_houdini_native_package_env_root_replacement() {
        let mut manifest = PackageManifest::new(
            "studio/test-pkg".to_string(),
            "Test".to_string(),
            "1.0.0".to_string(),
            None,
            None,
            None,
        );
        let mut env = IndexMap::new();
        env.insert(
            "PATH_A".to_string(),
            ManifestEnvEntry {
                method: EnvMethod::Set,
                value: "$HPM_PACKAGE_ROOT/a".to_string(),
            },
        );
        env.insert(
            "PATH_B".to_string(),
            ManifestEnvEntry {
                method: EnvMethod::Append,
                value: "$HPM_PACKAGE_ROOT/b:$HPM_PACKAGE_ROOT/c".to_string(),
            },
        );
        manifest.env = Some(env);

        let (_, pkg) = manifest.generate_houdini_native_package().unwrap();

        // PATH_A
        match pkg.env[1].get("PATH_A").unwrap() {
            crate::houdini::HoudiniEnvValue::Detailed { value, .. } => {
                assert_eq!(value, "$HOUDINI_PACKAGE_PATH/test-pkg/a");
            }
            _ => panic!("Expected Detailed"),
        }
        // PATH_B with multiple replacements
        match pkg.env[2].get("PATH_B").unwrap() {
            crate::houdini::HoudiniEnvValue::Detailed { value, method } => {
                assert_eq!(
                    value,
                    "$HOUDINI_PACKAGE_PATH/test-pkg/b:$HOUDINI_PACKAGE_PATH/test-pkg/c"
                );
                assert_eq!(method, "append");
            }
            _ => panic!("Expected Detailed"),
        }
    }
}
