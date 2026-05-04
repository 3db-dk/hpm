//! Houdini package.json types for integration.
//!
//! This module defines the output types for generating Houdini-compatible
//! `package.json` files from HPM manifests.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Houdini package.json structure for generation
///
/// This structure represents the format expected by Houdini's package system.
/// It's generated from an HPM manifest to enable seamless Houdini integration.
///
/// # Example Output
///
/// ```json
/// {
///   "hpath": ["$HPM_PACKAGE_ROOT"],
///   "env": [
///     {"PYTHONPATH": {"method": "prepend", "value": "$HPM_PACKAGE_ROOT/python"}}
///   ],
///   "enable": "houdini_version >= '20.5'"
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HoudiniPackage {
    /// Houdini path entries (for HOUDINI_OTLSCAN_PATH, etc.)
    pub hpath: Option<Vec<String>>,
    /// Environment variable definitions
    pub env: Option<Vec<HashMap<String, HoudiniEnvValue>>>,
    /// Conditional enable expression
    pub enable: Option<String>,
    /// Required packages
    pub requires: Option<Vec<String>>,
    /// Recommended packages
    pub recommends: Option<Vec<String>>,
}

/// Environment variable value in Houdini package.json
///
/// Supports three formats:
/// - Simple: direct string value
/// - Detailed: method (prepend/append/set) with a flat string value
/// - DetailedConditional: method plus an ordered list of `{ "<expr>": "<v>" }`
///   maps; Houdini picks the first whose expression matches.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum HoudiniEnvValue {
    /// Simple string value (sets the variable directly)
    Simple(String),
    /// Detailed value with method specification
    Detailed {
        /// How to apply the value: "prepend", "append", or "set"
        method: String,
        /// The value to apply
        value: String,
    },
    /// Detailed value where the value is a Houdini conditional-object array.
    /// Each map has a single entry `"<houdini-expression>": "<value>"`;
    /// Houdini evaluates the expressions in order and applies the first match.
    DetailedConditional {
        method: String,
        value: Vec<HashMap<String, String>>,
    },
}

impl HoudiniEnvValue {
    /// Create a simple environment value.
    pub fn simple(value: impl Into<String>) -> Self {
        HoudiniEnvValue::Simple(value.into())
    }

    /// Create a prepend environment value.
    pub fn prepend(value: impl Into<String>) -> Self {
        HoudiniEnvValue::Detailed {
            method: "prepend".to_string(),
            value: value.into(),
        }
    }

    /// Create an append environment value.
    pub fn append(value: impl Into<String>) -> Self {
        HoudiniEnvValue::Detailed {
            method: "append".to_string(),
            value: value.into(),
        }
    }

    /// Create a set environment value.
    pub fn set(value: impl Into<String>) -> Self {
        HoudiniEnvValue::Detailed {
            method: "set".to_string(),
            value: value.into(),
        }
    }

    /// Create a conditional environment value with the given method.
    pub fn conditional(method: &str, value: Vec<HashMap<String, String>>) -> Self {
        HoudiniEnvValue::DetailedConditional {
            method: method.to_string(),
            value,
        }
    }
}

/// Houdini-native package.json for direct use by Houdini's package system.
///
/// Unlike `HoudiniPackage` (which uses `$HPM_PACKAGE_ROOT` for HPM runtime),
/// this uses `$HOUDINI_PACKAGE_PATH/{slug}` so the archive works directly
/// with Houdini's built-in package loading.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HoudiniNativePackage {
    /// Package slug name
    pub name: String,
    /// Houdini package path
    pub hpath: String,
    /// Load this package only once
    pub load_package_once: bool,
    /// Show in Houdini's package browser
    pub show: bool,
    /// Conditional enable expression
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enable: Option<String>,
    /// Environment variable definitions
    pub env: Vec<HashMap<String, HoudiniEnvValue>>,
    /// Required packages
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requires: Option<Vec<String>>,
    /// Package metadata
    pub hpackage: HpackageMetadata,
}

/// Metadata block within a Houdini native package.json.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HpackageMetadata {
    /// Package version string
    pub version: String,
}

impl HoudiniPackage {
    /// Create an empty Houdini package.
    pub fn new() -> Self {
        Self {
            hpath: None,
            env: None,
            enable: None,
            requires: None,
            recommends: None,
        }
    }

    /// Add an hpath entry.
    pub fn add_hpath(&mut self, path: impl Into<String>) {
        self.hpath.get_or_insert_with(Vec::new).push(path.into());
    }

    /// Add an environment variable.
    pub fn add_env(&mut self, key: impl Into<String>, value: HoudiniEnvValue) {
        let mut env_map = HashMap::new();
        env_map.insert(key.into(), value);
        self.env.get_or_insert_with(Vec::new).push(env_map);
    }

    /// Set the enable condition.
    pub fn set_enable(&mut self, condition: impl Into<String>) {
        self.enable = Some(condition.into());
    }
}

impl Default for HoudiniPackage {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn houdini_package_builder() {
        let mut pkg = HoudiniPackage::new();
        pkg.add_hpath("$HPM_PACKAGE_ROOT");
        pkg.add_env(
            "PYTHONPATH",
            HoudiniEnvValue::prepend("$HPM_PACKAGE_ROOT/python"),
        );
        pkg.set_enable("houdini_version >= '20.5'");

        assert!(pkg.hpath.is_some());
        assert_eq!(pkg.hpath.as_ref().unwrap().len(), 1);
        assert!(pkg.env.is_some());
        assert!(pkg.enable.is_some());
    }

    #[test]
    fn houdini_env_value_constructors() {
        let simple = HoudiniEnvValue::simple("value");
        let prepend = HoudiniEnvValue::prepend("value");
        let append = HoudiniEnvValue::append("value");
        let set = HoudiniEnvValue::set("value");

        match simple {
            HoudiniEnvValue::Simple(v) => assert_eq!(v, "value"),
            _ => panic!("Expected Simple variant"),
        }

        match prepend {
            HoudiniEnvValue::Detailed { method, value } => {
                assert_eq!(method, "prepend");
                assert_eq!(value, "value");
            }
            _ => panic!("Expected Detailed variant"),
        }

        match append {
            HoudiniEnvValue::Detailed { method, value } => {
                assert_eq!(method, "append");
                assert_eq!(value, "value");
            }
            _ => panic!("Expected Detailed variant"),
        }

        match set {
            HoudiniEnvValue::Detailed { method, value } => {
                assert_eq!(method, "set");
                assert_eq!(value, "value");
            }
            _ => panic!("Expected Detailed variant"),
        }
    }

    #[test]
    fn houdini_package_serialization() {
        let mut pkg = HoudiniPackage::new();
        pkg.add_hpath("$HPM_PACKAGE_ROOT");
        pkg.add_env(
            "PYTHONPATH",
            HoudiniEnvValue::prepend("$HPM_PACKAGE_ROOT/python"),
        );

        let json = serde_json::to_string_pretty(&pkg).unwrap();
        assert!(json.contains("hpath"));
        assert!(json.contains("PYTHONPATH"));
        assert!(json.contains("prepend"));
    }

    #[test]
    fn houdini_native_package_serialization() {
        let pkg = HoudiniNativePackage {
            name: "my-tool".to_string(),
            hpath: "$HOUDINI_PACKAGE_PATH/my-tool".to_string(),
            load_package_once: true,
            show: true,
            enable: Some("houdini_version >= '21.0'".to_string()),
            env: vec![{
                let mut m = HashMap::new();
                m.insert(
                    "PKG_MY_TOOL".to_string(),
                    HoudiniEnvValue::simple("$HOUDINI_PACKAGE_PATH/my-tool"),
                );
                m
            }],
            requires: Some(vec!["some-dep".to_string()]),
            hpackage: HpackageMetadata {
                version: "1.2.3".to_string(),
            },
        };

        let json = serde_json::to_string_pretty(&pkg).unwrap();
        assert!(json.contains("\"name\": \"my-tool\""));
        assert!(json.contains("\"load_package_once\": true"));
        assert!(json.contains("\"show\": true"));
        assert!(json.contains("\"hpath\": \"$HOUDINI_PACKAGE_PATH/my-tool\""));
        assert!(json.contains("\"version\": \"1.2.3\""));
        assert!(json.contains("\"some-dep\""));
    }

    #[test]
    fn houdini_native_package_omits_none_fields() {
        let pkg = HoudiniNativePackage {
            name: "test".to_string(),
            hpath: "$HOUDINI_PACKAGE_PATH/test".to_string(),
            load_package_once: true,
            show: true,
            enable: None,
            env: vec![],
            requires: None,
            hpackage: HpackageMetadata {
                version: "1.0.0".to_string(),
            },
        };

        let json = serde_json::to_string_pretty(&pkg).unwrap();
        assert!(!json.contains("enable"));
        assert!(!json.contains("requires"));
    }
}
