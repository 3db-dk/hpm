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
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PythonDependencySpec {
    /// Simple version constraint string
    Simple(String),

    /// Detailed specification with version, extras, and optional flag
    Detailed {
        /// Version constraint (e.g., ">=1.0.0", "^2.0", "~3.1")
        version: Option<String>,
        /// Whether this dependency is optional
        optional: Option<bool>,
        /// Python extras to install (e.g., ["dev", "test"])
        extras: Option<Vec<String>>,
    },
}

impl PythonDependencySpec {
    /// Create a simple Python dependency with just a version constraint.
    pub fn simple(version: impl Into<String>) -> Self {
        PythonDependencySpec::Simple(version.into())
    }

    /// Create a detailed Python dependency.
    pub fn detailed(
        version: Option<String>,
        extras: Option<Vec<String>>,
        optional: bool,
    ) -> Self {
        PythonDependencySpec::Detailed {
            version,
            extras,
            optional: Some(optional),
        }
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
            PythonDependencySpec::Detailed { optional, .. } => optional.unwrap_or(false),
        }
    }

    /// Get the extras for this dependency.
    pub fn extras(&self) -> Option<&[String]> {
        match self {
            PythonDependencySpec::Simple(_) => None,
            PythonDependencySpec::Detailed { extras, .. } => extras.as_deref(),
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
        assert!(dep.extras().is_none());
    }

    #[test]
    fn detailed_python_dependency() {
        let dep = PythonDependencySpec::detailed(
            Some(">=1.0.0".to_string()),
            Some(vec!["dev".to_string(), "test".to_string()]),
            true,
        );
        assert_eq!(dep.version(), Some(">=1.0.0"));
        assert!(dep.is_optional());
        assert_eq!(dep.extras(), Some(&["dev".to_string(), "test".to_string()][..]));
    }

    #[test]
    fn python_dependency_serialization() {
        let simple = PythonDependencySpec::simple(">=1.0.0");
        let json = serde_json::to_string(&simple).unwrap();
        assert_eq!(json, r#"">=1.0.0""#);

        let detailed = PythonDependencySpec::Detailed {
            version: Some(">=1.0.0".to_string()),
            optional: Some(true),
            extras: Some(vec!["dev".to_string()]),
        };
        let json = serde_json::to_string(&detailed).unwrap();
        assert!(json.contains(">=1.0.0"));
        assert!(json.contains("dev"));
    }
}
