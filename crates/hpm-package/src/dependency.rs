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
/// A bare version string (`my-package = "1.0.0"`) is shorthand for a
/// registry-resolved dependency with no options; it deserializes to
/// [`DependencySpec::Registry`] with `registry: None` and
/// `optional: false`, and that shape serializes back to the bare string.
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
///
/// # Local path dependency installed as a symlink/junction (live edits)
/// my-local-pkg = { path = "../my-local-package", link = true }
/// ```
#[derive(Debug, Clone)]
pub enum DependencySpec {
    /// Registry-resolved dependency.
    ///
    /// Resolved through the configured registries (or a specific one) at
    /// install time. The bare-string manifest shorthand maps here with
    /// `registry: None` and `optional: false`.
    Registry {
        /// The version (e.g., "1.0.0")
        version: String,
        /// Optional registry name to resolve from
        registry: Option<String>,
        /// Whether this dependency is optional
        optional: bool,
    },

    /// Direct URL download (for registry-hosted archives)
    Url {
        /// Direct download URL for the archive
        url: String,
        /// The version (e.g., "1.0.0")
        version: String,
        /// Whether this dependency is optional
        optional: bool,
    },

    /// Local filesystem path
    Path {
        /// Path to the package directory (absolute or relative to manifest)
        path: String,
        /// Whether this dependency is optional
        optional: bool,
        /// Install the package as a symlink/junction into the dev subtree
        /// instead of copying the contents. Lets working-tree edits reach a
        /// live Houdini session without re-running `hpm sync`. Opt-in: the
        /// default keeps snapshot-copy semantics.
        link: bool,
    },
}

/// Serde wire shape for [`DependencySpec`].
///
/// The bare-string shorthand is a distinct untagged variant here so
/// `my-pkg = "1.0.0"` round-trips: it deserializes into the `Registry`
/// public variant, and a `Registry` with no options serializes back to
/// the bare string.
#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum DependencySpecRepr {
    Simple(String),
    Url {
        url: String,
        version: String,
        #[serde(default)]
        optional: bool,
    },
    Path {
        path: String,
        #[serde(default)]
        optional: bool,
        // Omitted when false so existing manifests don't churn.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        link: bool,
    },
    Registry {
        version: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        registry: Option<String>,
        #[serde(default)]
        optional: bool,
    },
}

impl From<DependencySpecRepr> for DependencySpec {
    fn from(repr: DependencySpecRepr) -> Self {
        match repr {
            DependencySpecRepr::Simple(version) => DependencySpec::Registry {
                version,
                registry: None,
                optional: false,
            },
            DependencySpecRepr::Url {
                url,
                version,
                optional,
            } => DependencySpec::Url {
                url,
                version,
                optional,
            },
            DependencySpecRepr::Path {
                path,
                optional,
                link,
            } => DependencySpec::Path {
                path,
                optional,
                link,
            },
            DependencySpecRepr::Registry {
                version,
                registry,
                optional,
            } => DependencySpec::Registry {
                version,
                registry,
                optional,
            },
        }
    }
}

impl From<&DependencySpec> for DependencySpecRepr {
    fn from(spec: &DependencySpec) -> Self {
        match spec {
            DependencySpec::Registry {
                version,
                registry: None,
                optional: false,
            } => DependencySpecRepr::Simple(version.clone()),
            DependencySpec::Registry {
                version,
                registry,
                optional,
            } => DependencySpecRepr::Registry {
                version: version.clone(),
                registry: registry.clone(),
                optional: *optional,
            },
            DependencySpec::Url {
                url,
                version,
                optional,
            } => DependencySpecRepr::Url {
                url: url.clone(),
                version: version.clone(),
                optional: *optional,
            },
            DependencySpec::Path {
                path,
                optional,
                link,
            } => DependencySpecRepr::Path {
                path: path.clone(),
                optional: *optional,
                link: *link,
            },
        }
    }
}

impl Serialize for DependencySpec {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        DependencySpecRepr::from(self).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for DependencySpec {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        DependencySpecRepr::deserialize(deserializer).map(DependencySpec::from)
    }
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
            link: false,
        }
    }

    /// Whether this path dependency requests link-mode install.
    pub fn is_link(&self) -> bool {
        matches!(self, DependencySpec::Path { link: true, .. })
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

    /// Check if this is a registry-resolved dependency.
    pub fn is_registry(&self) -> bool {
        matches!(self, DependencySpec::Registry { .. })
    }

    /// Check if this dependency is optional.
    pub fn is_optional(&self) -> bool {
        match self {
            DependencySpec::Url { optional, .. }
            | DependencySpec::Path { optional, .. }
            | DependencySpec::Registry { optional, .. } => *optional,
        }
    }

    /// Get the version if available.
    pub fn version(&self) -> Option<&str> {
        match self {
            DependencySpec::Url { version, .. } | DependencySpec::Registry { version, .. } => {
                Some(version)
            }
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
            DependencySpec::Registry { version, .. } => {
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
            link: false,
        };
        let json = serde_json::to_string(&spec).unwrap();
        assert!(json.contains("../local-package"));
        assert!(json.contains("true"));
    }

    /// `link` is omitted from serialized output when false so we don't churn
    /// existing manifests, and round-trips intact when true.
    #[test]
    fn dependency_spec_path_link_roundtrip() {
        let default_link = DependencySpec::Path {
            path: "../local-package".to_string(),
            optional: false,
            link: false,
        };
        let json = serde_json::to_string(&default_link).unwrap();
        assert!(
            !json.contains("link"),
            "default link=false must not serialize: {json}"
        );

        let with_link = DependencySpec::Path {
            path: "../local-package".to_string(),
            optional: false,
            link: true,
        };
        let toml_str = toml::to_string(&with_link).unwrap();
        assert!(toml_str.contains("link"), "link=true must serialize");

        // Backward compat: manifests without `link` parse to link=false.
        let legacy_toml = r#"
[deps]
my-pkg = { path = "../local" }
"#;
        #[derive(Deserialize)]
        struct Wrapper {
            deps: indexmap::IndexMap<String, DependencySpec>,
        }
        let parsed: Wrapper = toml::from_str(legacy_toml).unwrap();
        match &parsed.deps["my-pkg"] {
            DependencySpec::Path { link, .. } => assert!(!link),
            other => panic!("expected Path, got {other:?}"),
        }
    }

    #[derive(Serialize, Deserialize)]
    struct Wrapper {
        deps: indexmap::IndexMap<String, DependencySpec>,
    }

    #[test]
    fn dependency_spec_bare_string_deserializes_to_registry() {
        // Bare string in a dependencies table is registry shorthand.
        let toml_str = r#"
[deps]
test = "1.0.0"
"#;
        let parsed: Wrapper = toml::from_str(toml_str).unwrap();
        let spec = &parsed.deps["test"];
        assert!(matches!(
            spec,
            DependencySpec::Registry {
                version,
                registry: None,
                optional: false,
            } if version == "1.0.0"
        ));
        assert!(spec.is_registry());
        assert_eq!(spec.version(), Some("1.0.0"));
    }

    #[test]
    fn dependency_spec_bare_string_toml_roundtrip() {
        // A no-options Registry spec serializes back to the bare string,
        // so round-tripped manifests keep the idiomatic shorthand.
        let toml_str = "[deps]\ntest = \"1.0.0\"\n";
        let parsed: Wrapper = toml::from_str(toml_str).unwrap();
        let rendered = toml::to_string(&parsed).unwrap();
        assert_eq!(rendered, toml_str);
    }

    #[test]
    fn dependency_spec_no_option_registry_serializes_bare() {
        let spec = DependencySpec::registry("1.0.0", None);
        let json = serde_json::to_string(&spec).unwrap();
        assert_eq!(json, "\"1.0.0\"");
    }

    #[test]
    fn dependency_spec_registry_with_options_serializes_table() {
        let spec = DependencySpec::Registry {
            version: "1.0.0".to_string(),
            registry: Some("houdinihub".to_string()),
            optional: false,
        };
        let json = serde_json::to_string(&spec).unwrap();
        assert!(json.contains("\"version\""));
        assert!(json.contains("\"registry\""));

        let optional_only = DependencySpec::Registry {
            version: "1.0.0".to_string(),
            registry: None,
            optional: true,
        };
        let json = serde_json::to_string(&optional_only).unwrap();
        assert!(json.contains("\"optional\":true"));
        assert!(!json.contains("registry"));
    }

    #[test]
    fn dependency_spec_registry_toml_roundtrip() {
        let toml_str = r#"
[deps]
test = { version = "2.0.0", registry = "houdinihub", optional = true }
"#;
        let parsed: Wrapper = toml::from_str(toml_str).unwrap();
        let spec = &parsed.deps["test"];
        assert!(
            matches!(spec, DependencySpec::Registry { version, registry: Some(r), optional: true } if version == "2.0.0" && r == "houdinihub")
        );
        assert!(spec.is_registry());
        assert!(spec.is_optional());
        assert_eq!(spec.registry_name(), Some("houdinihub"));

        // Options present: the table form survives re-serialization.
        let rendered = toml::to_string(&parsed).unwrap();
        let reparsed: Wrapper = toml::from_str(&rendered).unwrap();
        assert!(matches!(
            &reparsed.deps["test"],
            DependencySpec::Registry {
                registry: Some(_),
                optional: true,
                ..
            }
        ));
    }

    #[test]
    fn dependency_spec_registry_version_only_toml() {
        // { version = "1.0.0" } without registry or optional
        let toml_str = r#"
[deps]
test = { version = "1.0.0" }
"#;
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
            link: false,
        };
        assert!(empty.validate().is_err());
    }

    #[test]
    fn registry_dependency_validation() {
        let valid = DependencySpec::registry("1.0.0", None);
        assert!(valid.validate().is_ok());

        let empty = DependencySpec::registry("", None);
        assert!(empty.validate().is_err());

        let invalid_start = DependencySpec::registry(".1.0.0", None);
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

        let bare_dep = DependencySpec::registry("1.0.0", None);
        assert!(!bare_dep.is_url());
        assert!(!bare_dep.is_path());
        assert!(bare_dep.is_registry());
        assert!(!bare_dep.is_optional());
        assert_eq!(bare_dep.version(), Some("1.0.0"));
        assert_eq!(bare_dep.registry_name(), None);

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
