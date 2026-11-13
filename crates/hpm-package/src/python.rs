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

/// Whether `name` is a PEP 508 distribution name (also the form of an extra):
/// ASCII letters and digits, with `.`, `_` and `-` inside.
pub fn is_python_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    let edge = |b: &u8| b.is_ascii_alphanumeric();
    bytes.first().is_some_and(edge)
        && bytes.last().is_some_and(edge)
        && bytes
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

/// Check a Python version constraint before it becomes part of a requirement
/// line: only the characters of version specifiers (PEP 440 operators and
/// versions, `*`, hpm's `^`, commas and spaces). That leaves no way to start
/// a pip option (`-`), a direct URL or VCS reference (`@`, `:`, `/`), an
/// environment marker (`;`), extras (`[`), a comment (`#`) or a new line.
pub fn validate_python_version(spec: &str) -> Result<(), String> {
    let ok = spec.bytes().all(|b| {
        b.is_ascii_alphanumeric()
            || matches!(
                b,
                b' ' | b'.' | b',' | b'*' | b'+' | b'!' | b'<' | b'>' | b'=' | b'~' | b'^' | b'-'
            )
    });
    // A `-` belongs inside a version (`1.0-1`); after a space, comma or
    // operator it would start a token pip and uv read as an option
    // (`>=1 --config-settings=...`).
    let option_dash = spec.starts_with('-')
        || spec
            .as_bytes()
            .windows(2)
            .any(|pair| matches!(pair, [before, b'-'] if !before.is_ascii_alphanumeric()));
    if !ok || option_dash {
        return Err(format!(
            "'{}' is not a version constraint (letters, digits, spaces and . , * + ! < > = ~ ^ -)",
            spec.escape_debug()
        ));
    }
    Ok(())
}

/// Check one `[python_dependencies]` entry: a PEP 508 name, a version
/// constraint, and extras that are names.
pub fn validate_python_dependency(name: &str, spec: &PythonDependencySpec) -> Result<(), String> {
    if !is_python_name(name) {
        return Err(format!(
            "'{}' is not a Python package name",
            name.escape_debug()
        ));
    }
    validate_python_version(spec.version_req()).map_err(|e| format!("{name}: {e}"))?;
    if let Some(bad) = spec.extras().iter().find(|extra| !is_python_name(extra)) {
        return Err(format!(
            "{name}: extra '{}' is not a name",
            bad.escape_debug()
        ));
    }
    Ok(())
}

/// Deserialize `[python_dependencies]`, checking every entry with
/// [`validate_python_dependency`].
pub fn deserialize_python_dependencies<'de, D>(
    deserializer: D,
) -> Result<indexmap::IndexMap<String, PythonDependencySpec>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let deps = indexmap::IndexMap::<String, PythonDependencySpec>::deserialize(deserializer)?;
    for (name, spec) in &deps {
        validate_python_dependency(name, spec).map_err(serde::de::Error::custom)?;
    }
    Ok(deps)
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

    mod properties {
        use super::*;
        use proptest::prelude::*;

        /// The line uv reads for an accepted dependency, rendered the way the
        /// resolver renders it.
        fn requirement_line(name: &str, spec: &PythonDependencySpec) -> String {
            let version = match spec.version_req() {
                "*" | "" => "",
                v => v,
            };
            if spec.extras().is_empty() {
                format!("{name}{version}")
            } else {
                format!("{name}[{}]{version}", spec.extras().join(","))
            }
        }

        fn hostile_text() -> impl Strategy<Value = String> {
            let alphabet = prop::sample::select(vec![
                'a', 'e', 'Z', '1', '.', '_', '-', ' ', '\t', '=', '>', '<', '~', '^', '*', ',',
                '!', '+', ';', '@', '#', '/', ':', '[', ']', '\n', '\r', '"', '\\',
            ]);
            // Mostly characters a constraint may hold, so accepted values
            // are common and the rest probe the edges.
            let allowed = prop::sample::select(vec![
                'a', 'e', '1', '0', '.', '-', ' ', '=', '>', '<', '~', '^', '*', ',', '!', '+',
            ]);
            prop_oneof![
                prop::collection::vec(alphabet, 0..14),
                prop::collection::vec(allowed, 0..14),
            ]
            .prop_map(|c| c.into_iter().collect())
        }

        fn hostile_name() -> impl Strategy<Value = String> {
            prop_oneof!["[a-z0-9]([a-z0-9._-]{0,6}[a-z0-9])?", hostile_text(),]
        }

        fn hostile_spec() -> impl Strategy<Value = PythonDependencySpec> {
            prop_oneof![
                hostile_text().prop_map(PythonDependencySpec::Simple),
                (
                    prop::option::of(hostile_text()),
                    any::<bool>(),
                    prop::collection::vec(hostile_name(), 0..3)
                )
                    .prop_map(|(version, optional, extras)| {
                        PythonDependencySpec::Detailed {
                            version,
                            optional,
                            extras,
                        }
                    }),
            ]
        }

        /// PEP 440 specifiers as projects write them.
        fn pep440_spec() -> impl Strategy<Value = String> {
            let version = prop_oneof![
                "[0-9]{1,3}(\\.[0-9]{1,3}){0,3}",
                "[0-9]{1,2}\\.[0-9]{1,2}(a|b|rc|\\.post|\\.dev)[0-9]{1,2}",
                "[0-9]{1,2}\\.[0-9]{1,2}-[0-9]",
                "[0-9]\\![0-9]\\.[0-9]",
                "[0-9]\\.[0-9]\\+[a-z0-9]{1,4}(\\.[0-9])?",
                "[0-9]\\.[0-9]\\.\\*",
            ];
            let clause = (
                prop::sample::select(vec![
                    "==", "!=", ">=", "<=", ">", "<", "~=", "===", "^", "~", "",
                ]),
                version,
            )
                .prop_map(|(op, v)| format!("{op}{v}"));
            prop_oneof![
                Just("*".to_string()),
                (
                    prop::collection::vec(clause, 1..4),
                    prop::sample::select(vec![",", ", ", " , "])
                )
                    .prop_map(|(clauses, sep)| clauses.join(sep)),
            ]
        }

        proptest! {
            #![proptest_config(ProptestConfig::with_cases(1024))]

            /// An accepted dependency renders to exactly one requirement line
            /// that is a name, optional extras and a version constraint: no
            /// line break, marker, URL, comment, quote, or pip option.
            #[test]
            fn accepted_dependencies_render_one_plain_requirement(
                name in hostile_name(),
                spec in hostile_spec(),
            ) {
                if validate_python_dependency(&name, &spec).is_ok() {
                    let line = requirement_line(&name, &spec);
                    prop_assert!(
                        !line.chars().any(|c| c.is_control() || ";@#/:\"\\'".contains(c)),
                        "{:?}", line
                    );
                    prop_assert!(
                        line.split_whitespace().all(|token| !token.starts_with('-')),
                        "{:?} carries an option", line
                    );
                    prop_assert_eq!(line.matches('[').count(), usize::from(!spec.extras().is_empty()));
                }
            }

            /// Real PEP 440 constraints, names and extras are accepted.
            #[test]
            fn pep440_constraints_are_accepted(
                name in "[A-Za-z0-9]([A-Za-z0-9._-]{0,10}[A-Za-z0-9])?",
                version in pep440_spec(),
                extras in prop::collection::vec("[a-z][a-z0-9_-]{0,6}[a-z0-9]", 0..3),
            ) {
                let spec = PythonDependencySpec::Detailed { version: Some(version), optional: false, extras };
                let result = validate_python_dependency(&name, &spec);
                prop_assert!(result.is_ok(), "{:?} {:?}: {:?}", name, spec, result);
            }
        }
    }
}
