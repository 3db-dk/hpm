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
/// my-package = { version = "1.0.0", registry = "tumbletrove", optional = true }
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
#[derive(Debug, Clone, PartialEq, Eq)]
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
        /// live Houdini session without re-running `hpm install`. Opt-in: the
        /// default keeps snapshot-copy semantics.
        link: bool,
    },
}

/// Serde shape [`DependencySpec`] is written as. The bare-string shorthand is
/// its own variant so `my-pkg = "1.0.0"` round-trips: a `Registry` spec with
/// no options serializes back to the bare string. Options that are off are
/// not written.
#[derive(Serialize)]
#[serde(untagged)]
enum DependencySpecRepr {
    Simple(String),
    Url {
        url: String,
        version: String,
        #[serde(skip_serializing_if = "std::ops::Not::not")]
        optional: bool,
    },
    Path {
        path: String,
        #[serde(skip_serializing_if = "std::ops::Not::not")]
        optional: bool,
        #[serde(skip_serializing_if = "std::ops::Not::not")]
        link: bool,
    },
    Registry {
        version: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        registry: Option<String>,
        #[serde(skip_serializing_if = "std::ops::Not::not")]
        optional: bool,
    },
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

/// Every key a dependency table may carry. Which ones go together is
/// decided by [`DependencyTable::into_spec`], so a misplaced key is named in
/// the error rather than silently dropped.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DependencyTable {
    url: Option<String>,
    path: Option<String>,
    version: Option<String>,
    registry: Option<String>,
    #[serde(default)]
    optional: bool,
    link: Option<bool>,
}

impl DependencyTable {
    /// The spec this table describes: a `url` table (with `version`), a
    /// `path` table, or a registry table (with `version`).
    fn into_spec(self) -> Result<DependencySpec, String> {
        let refuse =
            |key: &str, kind: &str| Err(format!("`{key}` does not apply to a {kind} dependency"));
        if let Some(url) = self.url {
            if self.path.is_some() {
                return Err("a dependency has either `url` or `path`, not both".into());
            }
            if self.registry.is_some() {
                return refuse("registry", "url");
            }
            if self.link.is_some() {
                return refuse("link", "url");
            }
            let version = self.version.ok_or("a url dependency needs a `version`")?;
            // The version names the download's cache and store directories,
            // so it must be one: a semantic version, nothing path-shaped.
            semver::Version::parse(&version).map_err(|_| {
                format!("a url dependency's `version` must be a semantic version, got '{version}'")
            })?;
            return Ok(DependencySpec::Url {
                url,
                version,
                optional: self.optional,
            });
        }
        if let Some(path) = self.path {
            if self.version.is_some() {
                return Err(
                    "`version` does not apply to a path dependency (its version is the one in its hpm.toml)"
                        .into(),
                );
            }
            if self.registry.is_some() {
                return refuse("registry", "path");
            }
            return Ok(DependencySpec::Path {
                path,
                optional: self.optional,
                link: self.link.unwrap_or(false),
            });
        }
        if self.link.is_some() {
            return refuse("link", "registry");
        }
        let version = self
            .version
            .ok_or("a dependency table needs `version`, `path` or `url`")?;
        Ok(DependencySpec::Registry {
            version,
            registry: self.registry,
            optional: self.optional,
        })
    }
}

impl<'de> Deserialize<'de> for DependencySpec {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct SpecVisitor;

        impl<'de> serde::de::Visitor<'de> for SpecVisitor {
            type Value = DependencySpec;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a version string or a dependency table")
            }

            fn visit_str<E: serde::de::Error>(self, version: &str) -> Result<Self::Value, E> {
                Ok(DependencySpec::Registry {
                    version: version.to_string(),
                    registry: None,
                    optional: false,
                })
            }

            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                map: A,
            ) -> Result<Self::Value, A::Error> {
                let table = DependencyTable::deserialize(
                    serde::de::value::MapAccessDeserializer::new(map),
                )?;
                table.into_spec().map_err(serde::de::Error::custom)
            }
        }

        deserializer.deserialize_any(SpecVisitor)
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

    #[derive(Serialize, Deserialize, Debug)]
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
            registry: Some("tumbletrove".to_string()),
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
test = { version = "2.0.0", registry = "tumbletrove", optional = true }
"#;
        let parsed: Wrapper = toml::from_str(toml_str).unwrap();
        let spec = &parsed.deps["test"];
        assert!(
            matches!(spec, DependencySpec::Registry { version, registry: Some(r), optional: true } if version == "2.0.0" && r == "tumbletrove")
        );
        assert!(spec.is_registry());
        assert!(spec.is_optional());
        assert_eq!(spec.registry_name(), Some("tumbletrove"));

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
            registry: Some("tumbletrove".to_string()),
            optional: true,
        };
        assert!(!registry_dep.is_url());
        assert!(!registry_dep.is_path());
        assert!(registry_dep.is_registry());
        assert!(registry_dep.is_optional());
        assert_eq!(registry_dep.version(), Some("2.0.0"));
        assert_eq!(registry_dep.registry_name(), Some("tumbletrove"));
    }

    /// Typos and misplaced keys in a dependency table are errors that name
    /// the problem; they used to be dropped silently (`registy` and
    /// `optinal` gave an unpinned, required dependency; a path dependency's
    /// `version` vanished; `{}` parsed).
    #[test]
    fn misshapen_dependency_tables_are_refused_by_name() {
        let parse = |table: &str| {
            toml::from_str::<Wrapper>(&format!("[deps]\nx = {table}\n"))
                .map(|w| w.deps["x"].clone())
        };
        for (table, says) in [
            (
                r#"{ version = "1.0.0", registy = "tumbletrove" }"#,
                "registy",
            ),
            (r#"{ version = "1.0.0", optinal = true }"#, "optinal"),
            (
                r#"{ path = "../z", version = "2.0.0" }"#,
                "`version` does not apply to a path",
            ),
            (
                r#"{ path = "../z", url = "https://x/z.zip" }"#,
                "either `url` or `path`",
            ),
            (r#"{ url = "https://x/z.zip" }"#, "needs a `version`"),
            (
                r#"{ url = "https://x/z.zip", version = "1.0.0", link = true }"#,
                "`link` does not apply to a url",
            ),
            (
                r#"{ version = "1.0.0", link = true }"#,
                "`link` does not apply to a registry",
            ),
            (
                r#"{ path = "../z", registry = "r" }"#,
                "`registry` does not apply to a path",
            ),
            ("{}", "needs `version`, `path` or `url`"),
        ] {
            let err = parse(table).unwrap_err().to_string();
            assert!(err.contains(says), "{table}: {err}");
        }
        assert_eq!(
            parse(r#"{ path = "../z", link = true, optional = true }"#).unwrap(),
            DependencySpec::Path {
                path: "../z".into(),
                optional: true,
                link: true
            }
        );
    }

    mod strictness {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            /// Any key outside a dependency table's vocabulary is refused,
            /// whatever else the table holds.
            #[test]
            fn an_unknown_key_is_always_refused(
                key in "[a-z_]{1,10}".prop_filter("not a known key", |k| {
                    !["url", "path", "version", "registry", "optional", "link"].contains(&k.as_str())
                }),
                base in prop::sample::select(vec![
                    r#"version = "1.0.0""#,
                    r#"path = "../z""#,
                    r#"url = "https://x/z.zip", version = "1.0.0""#,
                ]),
            ) {
                let doc = format!("[deps]\nx = {{ {base}, {key} = true }}\n");
                let err = toml::from_str::<Wrapper>(&doc).unwrap_err().to_string();
                prop_assert!(err.contains(&key), "{}", err);
            }
        }
    }
}
