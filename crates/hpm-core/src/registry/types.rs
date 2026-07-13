//! Registry type definitions.
//!
//! Types representing package entries in a registry and dependency
//! specifications within registry entries.

use serde::{Deserialize, Serialize};

/// Platform tag on a registry build entry.
///
/// Registries annotate archive variants with a platform string. Known
/// canonical tags parse to [`hpm_package::Platform`]; the `"universal"`
/// sentinel gets its own variant; anything else is preserved verbatim as
/// `Unknown` — an unrecognized future platform tag must not fail
/// deserialization, it just never matches the host.
///
/// String (de)serialization round-trips the original tag exactly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
pub enum PlatformTag {
    /// A concrete arch-suffixed platform (e.g. `windows-x86_64`).
    Platform(hpm_package::Platform),
    /// The `"universal"` sentinel: the build runs on any platform.
    Universal,
    /// An unrecognized tag, preserved verbatim. Never matches a host
    /// platform, but counts as universal when it spells `"universal"` in
    /// a different case (matching the historical case-insensitive check).
    Unknown(String),
}

impl PlatformTag {
    /// True when this build entry matches the given host platform.
    ///
    /// Only a concrete platform tag can match — `Universal` is handled
    /// separately via [`Self::is_universal`], and a bare-OS or unknown
    /// tag says nothing about the architecture, so it never matches.
    pub fn matches(&self, host: hpm_package::Platform) -> bool {
        matches!(self, PlatformTag::Platform(p) if *p == host)
    }

    /// True when this build entry counts as universal.
    pub fn is_universal(&self) -> bool {
        match self {
            PlatformTag::Universal => true,
            PlatformTag::Unknown(s) => s.eq_ignore_ascii_case("universal"),
            PlatformTag::Platform(_) => false,
        }
    }

    /// The original tag string.
    pub fn as_str(&self) -> &str {
        match self {
            PlatformTag::Platform(p) => p.as_str(),
            PlatformTag::Universal => "universal",
            PlatformTag::Unknown(s) => s,
        }
    }
}

impl From<String> for PlatformTag {
    fn from(s: String) -> Self {
        match s.parse::<hpm_package::Platform>() {
            // Route the exact "universal" spelling to its own variant so
            // Platform(Universal) never exists and `matches` stays strict.
            Ok(hpm_package::Platform::Universal) => PlatformTag::Universal,
            Ok(p) => PlatformTag::Platform(p),
            Err(_) => PlatformTag::Unknown(s),
        }
    }
}

impl From<PlatformTag> for String {
    fn from(tag: PlatformTag) -> Self {
        match tag {
            PlatformTag::Platform(p) => p.as_str().to_string(),
            PlatformTag::Universal => "universal".to_string(),
            PlatformTag::Unknown(s) => s,
        }
    }
}

impl std::fmt::Display for PlatformTag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A single version entry in the registry.
///
/// Each published version of a package has one entry. For git-based registries,
/// these are stored as one JSON object per line in the package index file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryEntry {
    /// Scoped package path (e.g. `creator/slug`), used as the unique identifier.
    pub name: String,
    /// Version string (semver)
    #[serde(rename = "vers")]
    pub version: String,
    /// SHA-256 checksum of the package archive (hex-encoded, prefixed with "sha256:")
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cksum: Option<String>,
    /// Download URL for the package archive
    pub dl: String,
    /// Ed25519 signature of the checksum (prefixed with "ed25519:")
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sig: Option<String>,
    /// Key ID used for signing
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kid: Option<String>,
    /// Houdini version compatibility range (e.g., ">=20.5,<23.0")
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub houdini_compat: Option<String>,
    /// Target platform for this archive variant (None = universal)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub platform: Option<PlatformTag>,
    /// Whether this version has been yanked
    #[serde(default)]
    pub yanked: bool,
    /// Package description (optional, mainly for search results)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Package author(s) (optional, mainly for search results)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    /// Publish timestamp of this version (ISO 8601). Populated by API
    /// registries; git registries may omit it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
}

/// Search results returned from a registry search.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResults {
    /// Matching entries (latest version per package)
    pub packages: Vec<RegistryEntry>,
    /// Total number of matches (may be more than returned)
    pub total: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use hpm_package::Platform;

    #[test]
    fn platform_tag_parses_canonical_forms() {
        assert_eq!(
            PlatformTag::from("windows-x86_64".to_string()),
            PlatformTag::Platform(Platform::WindowsX86_64)
        );
        assert_eq!(
            PlatformTag::from("linux-aarch64".to_string()),
            PlatformTag::Platform(Platform::LinuxAarch64)
        );
        assert_eq!(
            PlatformTag::from("universal".to_string()),
            PlatformTag::Universal
        );
    }

    #[test]
    fn platform_tag_bare_os_and_unknown_are_preserved() {
        // A bare OS tag says nothing about the architecture; matching it
        // to x86_64 would be a guess that misroutes ARM hosts.
        for raw in ["WINDOWS", "Linux", "macos", "plan9-amd64", "linux-arm64"] {
            let tag = PlatformTag::from(raw.to_string());
            assert_eq!(tag, PlatformTag::Unknown(raw.to_string()));
            assert!(!tag.matches(Platform::LinuxX86_64));
            assert!(!tag.matches(Platform::WindowsX86_64));
        }
    }

    #[test]
    fn platform_tag_is_universal() {
        assert!(PlatformTag::Universal.is_universal());
        // Historical behavior: the universal check is case-insensitive.
        assert!(PlatformTag::from("UNIVERSAL".to_string()).is_universal());
        assert!(!PlatformTag::from("linux-x86_64".to_string()).is_universal());
        assert!(!PlatformTag::from("plan9-amd64".to_string()).is_universal());
    }

    #[test]
    fn platform_tag_round_trips_original_string_exactly() {
        // Unknown tags (including "UNIVERSAL", which only matches the
        // universal check case-insensitively) must survive verbatim so a
        // deserialize/serialize cycle never rewrites registry data.
        for raw in [
            "windows-x86_64",
            "macos-aarch64",
            "universal",
            "UNIVERSAL",
            "plan9-amd64",
            "",
        ] {
            let json = serde_json::to_string(&raw).unwrap();
            let tag: PlatformTag = serde_json::from_str(&json).unwrap();
            assert_eq!(serde_json::to_string(&tag).unwrap(), json, "tag: {raw:?}");
        }
    }

    #[test]
    fn registry_entry_with_unknown_platform_deserializes() {
        // An unrecognized future platform tag must not fail deserialization.
        let line = r#"{"name":"acme/mops","vers":"1.0.0","deps":[],"dl":"https://example.com/a.zip","platform":"riscv64-futureos","yanked":false}"#;
        let entry: RegistryEntry = serde_json::from_str(line).unwrap();
        assert_eq!(
            entry.platform,
            Some(PlatformTag::Unknown("riscv64-futureos".to_string()))
        );
    }
}
