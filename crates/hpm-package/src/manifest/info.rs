//! `[package]` section: identifying metadata for the manifest.

use crate::package_path::PackagePath;
use serde::{Deserialize, Serialize};

fn semantic_version<'de, D: serde::Deserializer<'de>>(de: D) -> Result<String, D::Error> {
    let version = String::deserialize(de)?;
    semver::Version::parse(&version).map_err(|e| {
        serde::de::Error::custom(format!(
            "package version `{version}` is not a semantic version (MAJOR.MINOR.PATCH): {e}"
        ))
    })?;
    Ok(version)
}

/// Package metadata information
#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageInfo {
    /// Scoped package path: `creator-slug/package-slug` (e.g. `tumblehead/tumble-rig`).
    /// Validated kebab-case at deserialization — see [`PackagePath`].
    pub path: PackagePath,
    /// Freeform display name (e.g. `TumbleRig`)
    pub name: String,
    /// Semantic version (`MAJOR.MINOR.PATCH`, with optional pre-release and
    /// build metadata), checked when the manifest is parsed.
    #[serde(deserialize_with = "semantic_version")]
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub authors: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub readme: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub documentation: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keywords: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub categories: Vec<String>,
}

impl PackageInfo {
    /// Returns the scoped path (the canonical identifier).
    pub fn identifier(&self) -> &str {
        self.path.as_str()
    }

    /// Returns the creator segment, e.g. `tumblehead`.
    pub fn creator(&self) -> &str {
        self.path.creator()
    }

    /// Returns the slug segment, e.g. `tumble-rig`.
    pub fn slug(&self) -> &str {
        self.path.slug()
    }
}
