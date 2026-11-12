//! Python dependency specification types.
//!
//! This module defines how Python dependencies are specified in `hpm.toml` files,
//! supporting both simple version strings and detailed specifications with extras.

use serde::{Deserialize, Serialize};

/// Python dependency specification
///
/// Supports two formats:
/// - Simple: Just a version constraint string (e.g., ">=1.0.0")
/// - Detailed: Full specification with version, extras, and optional flag
///
/// # Examples
///
/// ```toml
/// [python_dependencies]
/// # Simple version constraint
/// numpy = ">=1.20.0"
///
/// # Detailed specification with extras
/// scipy = { version = ">=1.7.0", extras = ["sparse", "linalg"] }
///
/// # Optional dependency
/// matplotlib = { version = ">=3.5.0", optional = true }
/// ```
#[derive(Debug, PartialEq, Eq, Clone, Serialize)]
#[serde(untagged)]
pub enum PythonDependencySpec {
    /// Simple version constraint string
    Simple(String),

    /// Detailed specification with version, extras, and optional flag. A
    /// table gives `version`, `extras` or both; any other key is an error.
    Detailed {
        /// Version constraint (e.g., ">=1.0.0", "^2.0", "~3.1"); any version
        /// when absent.
        #[serde(skip_serializing_if = "Option::is_none")]
        version: Option<String>,
        /// Whether this dependency is optional
        #[serde(skip_serializing_if = "std::ops::Not::not")]
        optional: bool,
        /// Python extras to install (e.g., ["dev", "test"])
        #[serde(skip_serializing_if = "Vec::is_empty")]
        extras: Vec<String>,
    },
}

/// The keys a Python dependency table may carry.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PythonDependencyTable {
    version: Option<String>,
    #[serde(default)]
    optional: bool,
    #[serde(default)]
    extras: Vec<String>,
}

impl<'de> Deserialize<'de> for PythonDependencySpec {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct SpecVisitor;

        impl<'de> serde::de::Visitor<'de> for SpecVisitor {
            type Value = PythonDependencySpec;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a version string or a table with `version` and/or `extras`")
            }

            fn visit_str<E: serde::de::Error>(self, version: &str) -> Result<Self::Value, E> {
                Ok(PythonDependencySpec::Simple(version.to_string()))
            }

            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                map: A,
            ) -> Result<Self::Value, A::Error> {
                let table = PythonDependencyTable::deserialize(
                    serde::de::value::MapAccessDeserializer::new(map),
                )?;
                if table.version.is_none() && table.extras.is_empty() {
                    return Err(serde::de::Error::custom(
                        "a Python dependency table needs `version` or `extras`",
                    ));
                }
                Ok(PythonDependencySpec::Detailed {
                    version: table.version,
                    optional: table.optional,
                    extras: table.extras,
                })
            }
        }

        deserializer.deserialize_any(SpecVisitor)
    }
}

impl PythonDependencySpec {
    /// The version requirement, `"*"` when the spec gives none.
    pub fn version_req(&self) -> &str {
        match self {
            Self::Simple(version) => version,
            Self::Detailed { version, .. } => version.as_deref().unwrap_or("*"),
        }
    }

    /// Create a simple Python dependency with just a version constraint.
    pub fn simple(version: impl Into<String>) -> Self {
        PythonDependencySpec::Simple(version.into())
    }

    /// Get the version constraint string.
    pub fn version(&self) -> Option<&str> {
        match self {
            PythonDependencySpec::Simple(v) => Some(v),
            PythonDependencySpec::Detailed { version, .. } => version.as_deref(),
        }
    }

    /// Check if this dependency is optional.
    pub fn is_optional(&self) -> bool {
        match self {
            PythonDependencySpec::Simple(_) => false,
            PythonDependencySpec::Detailed { optional, .. } => *optional,
        }
    }

    /// The extras requested for this dependency (none for a simple spec).
    pub fn extras(&self) -> &[String] {
        match self {
            PythonDependencySpec::Simple(_) => &[],
            PythonDependencySpec::Detailed { extras, .. } => extras,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_python_dependency() {
        let dep = PythonDependencySpec::simple(">=1.0.0");
        assert_eq!(dep.version(), Some(">=1.0.0"));
        assert!(!dep.is_optional());
        assert!(dep.extras().is_empty());
    }

    #[test]
    fn detailed_python_dependency() {
        let dep = PythonDependencySpec::Detailed {
            version: Some(">=1.0.0".to_string()),
            extras: vec!["dev".to_string(), "test".to_string()],
            optional: true,
        };
        assert_eq!(dep.version(), Some(">=1.0.0"));
        assert!(dep.is_optional());
        assert_eq!(dep.extras(), ["dev".to_string(), "test".to_string()]);
    }

    #[test]
    fn python_dependency_serialization() {
        let simple = PythonDependencySpec::simple(">=1.0.0");
        let json = serde_json::to_string(&simple).unwrap();
        assert_eq!(json, r#"">=1.0.0""#);

        let detailed = PythonDependencySpec::Detailed {
            version: Some(">=1.0.0".to_string()),
            optional: true,
            extras: vec!["dev".to_string()],
        };
        let json = serde_json::to_string(&detailed).unwrap();
        assert!(json.contains(">=1.0.0"));
        assert!(json.contains("dev"));
    }

    #[test]
    fn version_req_is_the_requirement_or_any() {
        assert_eq!(
            PythonDependencySpec::Simple(">=1.20".into()).version_req(),
            ">=1.20"
        );
        let detailed = |version: Option<&str>| PythonDependencySpec::Detailed {
            version: version.map(str::to_string),
            optional: false,
            extras: Vec::new(),
        };
        assert_eq!(detailed(Some("^3.5")).version_req(), "^3.5");
        assert_eq!(detailed(None).version_req(), "*");
    }

    #[derive(Debug, Deserialize)]
    struct Deps {
        deps: std::collections::BTreeMap<String, PythonDependencySpec>,
    }

    /// A Python dependency table gives `version` and/or `extras`; any other
    /// key, or neither, is refused.
    #[test]
    fn misshapen_python_tables_are_refused() {
        let parse = |table: &str| toml::from_str::<Deps>(&format!("[deps]\nx = {table}\n"));
        assert!(
            parse(r#"{ version = ">=1", extra = ["a"] }"#)
                .unwrap_err()
                .to_string()
                .contains("extra")
        );
        assert!(
            parse("{}")
                .unwrap_err()
                .to_string()
                .contains("needs `version` or `extras`")
        );
        assert!(parse("{ optional = true }").is_err());
        assert_eq!(
            parse(r#"{ extras = ["socks"] }"#).unwrap().deps["x"],
            PythonDependencySpec::Detailed {
                version: None,
                optional: false,
                extras: vec!["socks".into()]
            }
        );
    }
}
