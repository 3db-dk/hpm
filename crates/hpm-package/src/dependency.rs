//! HPM dependency specification types.
//!
//! This module defines how dependencies are specified in `hpm.toml` files,
//! supporting registry-resolved, URL-based, and local path dependencies.

use serde::{Deserialize, Serialize};

/// Dependency specification for HPM packages.
///
/// HPM resolves packages through a registry at install/sync time.
/// URL and local path dependencies are also supported.
///
/// # Examples
///
/// ```toml
/// [dependencies]
/// # Registry-resolved dependency (version shorthand)
/// my-package = "1.0.0"
///
/// # Registry-resolved with options
/// my-package = { version = "1.0.0", registry = "houdinihub", optional = true }
///
/// # Direct URL download (legacy/explicit)
/// my-package = { url = "https://pkg.example.com/packages/my-package/1.0.0/my-package-1.0.0.zip", version = "1.0.0" }
///
/// # Local path dependency (for development)
/// my-local-pkg = { path = "../my-local-package" }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DependencySpec {
    /// Bare version string shorthand (e.g., `"1.0.0"`)
    ///
    /// Resolved through the configured registries at install time.
    Simple(String),

    /// Direct URL download (for registry-hosted archives)
    Url {
        /// Direct download URL for the archive
        url: String,
        /// The version (e.g., "1.0.0")
        version: String,
        /// Whether this dependency is optional
        #[serde(default)]
        optional: bool,
    },

    /// Local filesystem path
    Path {
        /// Path to the package directory (absolute or relative to manifest)
        path: String,
        /// Whether this dependency is optional
        #[serde(default)]
        optional: bool,
    },

    /// Registry-resolved dependency with options
    ///
    /// Resolved through the configured registries (or a specific one) at install time.
    Registry {
        /// The version (e.g., "1.0.0")
        version: String,
        /// Optional registry name to resolve from
        #[serde(default, skip_serializing_if = "Option::is_none")]
        registry: Option<String>,
        /// Whether this dependency is optional
        #[serde(default)]
        optional: bool,
    },
}

impl DependencySpec {
    /// Create a new direct URL dependency.
    pub fn url(url: impl Into<String>, version: impl Into<String>) -> Self {
        DependencySpec::Url {
            url: url.into(),
            version: version.into(),
            optional: false,
        }
    }

    /// Create a new path dependency.
    pub fn path(path: impl Into<String>) -> Self {
        DependencySpec::Path {
            path: path.into(),
            optional: false,
        }
    }

    /// Create a new registry dependency.
    pub fn registry(version: impl Into<String>, registry: Option<String>) -> Self {
        DependencySpec::Registry {
            version: version.into(),
            registry,
            optional: false,
        }
    }

    /// Check if this is a URL dependency.
    pub fn is_url(&self) -> bool {
        matches!(self, DependencySpec::Url { .. })
    }

    /// Check if this is a path dependency.
    pub fn is_path(&self) -> bool {
        matches!(self, DependencySpec::Path { .. })
    }

    /// Check if this is a registry-resolved dependency (Simple or Registry).
    pub fn is_registry(&self) -> bool {
        matches!(
            self,
            DependencySpec::Simple(_) | DependencySpec::Registry { .. }
        )
    }

    /// Check if this dependency is optional.
    pub fn is_optional(&self) -> bool {
        match self {
            DependencySpec::Simple(_) => false,
            DependencySpec::Url { optional, .. }
            | DependencySpec::Path { optional, .. }
            | DependencySpec::Registry { optional, .. } => *optional,
        }
    }

    /// Get the version if available.
    pub fn version(&self) -> Option<&str> {
        match self {
            DependencySpec::Simple(version)
            | DependencySpec::Url { version, .. }
            | DependencySpec::Registry { version, .. } => Some(version),
            DependencySpec::Path { .. } => None,
        }
    }

    /// Get the path if this is a path dependency.
    pub fn local_path(&self) -> Option<&str> {
        match self {
            DependencySpec::Path { path, .. } => Some(path),
            _ => None,
        }
    }

    /// Get the registry name if specified.
    pub fn registry_name(&self) -> Option<&str> {
        match self {
            DependencySpec::Registry {
                registry: Some(r), ..
            } => Some(r),
            _ => None,
        }
    }

    /// Validate the dependency spec.
    pub fn validate(&self) -> Result<(), String> {
        match self {
            DependencySpec::Simple(version) | DependencySpec::Registry { version, .. } => {
                if version.is_empty() {
                    return Err("Version cannot be empty".to_string());
                }
                if version.starts_with('.') || version.ends_with('.') {
                    return Err(format!("Version has invalid format: {}", version));
                }
                Ok(())
            }
            DependencySpec::Url { url, version, .. } => {
                if url.is_empty() {
                    return Err("URL cannot be empty".to_string());
                }
                if !url.starts_with("https://") && !url.starts_with("http://") {
                    return Err(format!("URL must start with https:// or http://: {}", url));
                }
                if version.is_empty() {
                    return Err("Version cannot be empty".to_string());
                }
                if version.starts_with('.') || version.ends_with('.') {
                    return Err(format!("Version has invalid format: {}", version));
                }
                Ok(())
            }
            DependencySpec::Path { path, .. } => {
                if path.is_empty() {
                    return Err("Path cannot be empty".to_string());
                }
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dependency_spec_url_serialization() {
        let spec = DependencySpec::Url {
            url: "https://pkg.example.com/packages/test/1.0.0/test-1.0.0.zip".to_string(),
            version: "1.0.0".to_string(),
            optional: false,
        };
        let json = serde_json::to_string(&spec).unwrap();
        assert!(json.contains("pkg.example.com"));
        assert!(json.contains("1.0.0"));
    }

    #[test]
    fn dependency_spec_path_serialization() {
        let spec = DependencySpec::Path {
            path: "../local-package".to_string(),
            optional: true,
        };
        let json = serde_json::to_string(&spec).unwrap();
        assert!(json.contains("../local-package"));
        assert!(json.contains("true"));
    }

    #[test]
    fn dependency_spec_simple_toml_roundtrip() {
        // Bare string in a dependencies table
        let toml_str = r#"
[deps]
test = "1.0.0"
"#;
        #[derive(Deserialize)]
        struct Wrapper {
            deps: indexmap::IndexMap<String, DependencySpec>,
        }
        let parsed: Wrapper = toml::from_str(toml_str).unwrap();
        let spec = &parsed.deps["test"];
        assert!(matches!(spec, DependencySpec::Simple(v) if v == "1.0.0"));
        assert!(spec.is_registry());
        assert_eq!(spec.version(), Some("1.0.0"));
    }

    #[test]
    fn dependency_spec_registry_toml_roundtrip() {
        let toml_str = r#"
[deps]
test = { version = "2.0.0", registry = "houdinihub", optional = true }
"#;
        #[derive(Deserialize)]
        struct Wrapper {
            deps: indexmap::IndexMap<String, DependencySpec>,
        }
        let parsed: Wrapper = toml::from_str(toml_str).unwrap();
        let spec = &parsed.deps["test"];
        assert!(
            matches!(spec, DependencySpec::Registry { version, registry: Some(r), optional: true } if version == "2.0.0" && r == "houdinihub")
        );
        assert!(spec.is_registry());
        assert!(spec.is_optional());
        assert_eq!(spec.registry_name(), Some("houdinihub"));
    }

    #[test]
    fn dependency_spec_registry_version_only_toml() {
        // { version = "1.0.0" } without registry or optional
        let toml_str = r#"
[deps]
test = { version = "1.0.0" }
"#;
        #[derive(Deserialize)]
        struct Wrapper {
            deps: indexmap::IndexMap<String, DependencySpec>,
        }
        let parsed: Wrapper = toml::from_str(toml_str).unwrap();
        let spec = &parsed.deps["test"];
        // This should match Registry (not Url, which requires `url` field)
        assert!(matches!(spec, DependencySpec::Registry { version, .. } if version == "1.0.0"));
    }

    #[test]
    fn url_dependency_validation() {
        let valid = DependencySpec::url("https://example.com/pkg.zip", "1.0.0");
        assert!(valid.validate().is_ok());

        let empty_url = DependencySpec::Url {
            url: "".to_string(),
            version: "1.0.0".to_string(),
            optional: false,
        };
        assert!(empty_url.validate().is_err());

        let invalid_url = DependencySpec::Url {
            url: "not-a-url".to_string(),
            version: "1.0.0".to_string(),
            optional: false,
        };
        assert!(invalid_url.validate().is_err());

        let empty_version = DependencySpec::Url {
            url: "https://example.com/pkg.zip".to_string(),
            version: "".to_string(),
            optional: false,
        };
        assert!(empty_version.validate().is_err());

        let invalid_version_start = DependencySpec::Url {
            url: "https://example.com/pkg.zip".to_string(),
            version: ".1.0.0".to_string(),
            optional: false,
        };
        assert!(invalid_version_start.validate().is_err());

        let invalid_version_end = DependencySpec::Url {
            url: "https://example.com/pkg.zip".to_string(),
            version: "1.0.0.".to_string(),
            optional: false,
        };
        assert!(invalid_version_end.validate().is_err());
    }

    #[test]
    fn path_dependency_validation() {
        let valid = DependencySpec::path("../local-package");
        assert!(valid.validate().is_ok());

        let empty = DependencySpec::Path {
            path: "".to_string(),
            optional: false,
        };
        assert!(empty.validate().is_err());
    }

    #[test]
    fn registry_dependency_validation() {
        let valid = DependencySpec::Simple("1.0.0".to_string());
        assert!(valid.validate().is_ok());

        let empty = DependencySpec::Simple("".to_string());
        assert!(empty.validate().is_err());

        let invalid_start = DependencySpec::Simple(".1.0.0".to_string());
        assert!(invalid_start.validate().is_err());

        let registry = DependencySpec::registry("2.0.0", Some("houdinihub".to_string()));
        assert!(registry.validate().is_ok());
    }

    #[test]
    fn dependency_helper_methods() {
        let url_dep = DependencySpec::url("https://example.com/pkg.zip", "1.0.0");
        assert!(url_dep.is_url());
        assert!(!url_dep.is_path());
        assert!(!url_dep.is_registry());
        assert!(!url_dep.is_optional());
        assert_eq!(url_dep.version(), Some("1.0.0"));
        assert_eq!(url_dep.local_path(), None);

        let path_dep = DependencySpec::path("../local");
        assert!(!path_dep.is_url());
        assert!(path_dep.is_path());
        assert!(!path_dep.is_registry());
        assert!(!path_dep.is_optional());
        assert_eq!(path_dep.version(), None);
        assert_eq!(path_dep.local_path(), Some("../local"));

        let simple_dep = DependencySpec::Simple("1.0.0".to_string());
        assert!(!simple_dep.is_url());
        assert!(!simple_dep.is_path());
        assert!(simple_dep.is_registry());
        assert!(!simple_dep.is_optional());
        assert_eq!(simple_dep.version(), Some("1.0.0"));
        assert_eq!(simple_dep.registry_name(), None);

        let registry_dep = DependencySpec::Registry {
            version: "2.0.0".to_string(),
            registry: Some("houdinihub".to_string()),
            optional: true,
        };
        assert!(!registry_dep.is_url());
        assert!(!registry_dep.is_path());
        assert!(registry_dep.is_registry());
        assert!(registry_dep.is_optional());
        assert_eq!(registry_dep.version(), Some("2.0.0"));
        assert_eq!(registry_dep.registry_name(), Some("houdinihub"));
    }
}
