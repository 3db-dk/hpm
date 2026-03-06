//! Package source definitions for URL-based and path-based dependencies.
//!
//! This module defines the types for specifying where packages come from.
//! HPM resolves packages through a registry which provides direct download URLs,
//! with local path dependencies for development.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use thiserror::Error;

/// Errors that can occur when working with package sources.
#[derive(Debug, Error)]
pub enum PackageSourceError {
    #[error("Invalid URL: {0}")]
    InvalidUrl(String),

    #[error("Invalid version: {0}")]
    InvalidVersion(String),

    #[error("Path does not exist: {0}")]
    PathNotFound(PathBuf),
}

/// Represents where a package comes from.
///
/// HPM supports two package sources:
/// - Direct URL downloads (registry-resolved archives)
/// - Local filesystem paths (for development)
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum PackageSource {
    /// A direct URL download.
    ///
    /// The package archive is downloaded directly from the given URL without
    /// any URL reconstruction. Used for registry-hosted archives.
    Url {
        /// The direct download URL for the archive
        url: String,
        /// The version (e.g., "1.0.0")
        version: String,
    },

    /// A local filesystem path.
    ///
    /// Used during development to reference packages without publishing.
    Path {
        /// Path to the package directory (absolute or relative to manifest)
        path: PathBuf,
    },
}

impl PackageSource {
    /// Create a new direct URL source.
    ///
    /// # Arguments
    /// * `url` - The direct download URL for the archive
    /// * `version` - The package version
    ///
    /// # Errors
    /// Returns an error if the URL or version is invalid.
    pub fn url(
        url: impl Into<String>,
        version: impl Into<String>,
    ) -> Result<Self, PackageSourceError> {
        let url = url.into();
        let version = version.into();

        if !url.starts_with("https://") && !url.starts_with("http://") {
            return Err(PackageSourceError::InvalidUrl(format!(
                "URL must start with https:// or http://: {}",
                url
            )));
        }

        if version.is_empty() {
            return Err(PackageSourceError::InvalidVersion(
                "Version cannot be empty".to_string(),
            ));
        }

        if version.starts_with('.') || version.ends_with('.') {
            return Err(PackageSourceError::InvalidVersion(format!(
                "Version has invalid format: {}",
                version
            )));
        }

        Ok(PackageSource::Url { url, version })
    }

    /// Create a new path source.
    ///
    /// # Arguments
    /// * `path` - Path to the package directory
    pub fn path(path: impl Into<PathBuf>) -> Self {
        PackageSource::Path { path: path.into() }
    }

    /// Check if this is a URL source.
    pub fn is_url(&self) -> bool {
        matches!(self, PackageSource::Url { .. })
    }

    /// Check if this is a path source.
    pub fn is_path(&self) -> bool {
        matches!(self, PackageSource::Path { .. })
    }

    /// Get the path if this is a path source.
    pub fn local_path(&self) -> Option<&PathBuf> {
        match self {
            PackageSource::Path { path } => Some(path),
            _ => None,
        }
    }

    /// Returns true if the source uses secure transport (HTTPS).
    pub fn is_secure(&self) -> bool {
        match self {
            PackageSource::Url { url, .. } => url.starts_with("https://"),
            PackageSource::Path { .. } => true,
        }
    }

    /// Returns a security warning message if the source uses insecure transport.
    pub fn security_warning(&self) -> Option<&'static str> {
        if !self.is_secure() {
            Some("Using insecure HTTP. Consider HTTPS for better security.")
        } else {
            None
        }
    }

    /// Generate a unique cache key for this source.
    pub fn cache_key(&self) -> String {
        match self {
            PackageSource::Url { url, version } => {
                let url_hash = seahash_simple(url);
                format!("url-{:x}-{}", url_hash, version)
            }
            PackageSource::Path { path } => {
                let path_str = path.to_string_lossy();
                format!("local-{:x}", seahash_simple(&path_str))
            }
        }
    }
}

/// Simple hash function for cache keys (no external dependency needed).
fn seahash_simple(s: &str) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325; // FNV offset basis
    for byte in s.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3); // FNV prime
    }
    hash
}

impl std::fmt::Display for PackageSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PackageSource::Url { url, version } => {
                write!(f, "url:{}@{}", url, version)
            }
            PackageSource::Path { path } => {
                write!(f, "path:{}", path.display())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// Strategy to generate valid URLs
    fn url_strategy() -> impl Strategy<Value = String> {
        prop_oneof![
            Just("https://pkg.example.com/packages/test/1.0.0/test-1.0.0.zip".to_string()),
            Just("https://api.3db.dk/v1/registry/packages/test/1.0.0/download".to_string()),
            r"https://[a-z]+\.[a-z]+/[a-z]+/[a-z]+".prop_filter("valid url", |s| s.len() < 100),
        ]
    }

    /// Strategy to generate valid versions
    fn version_strategy() -> impl Strategy<Value = String> {
        (0u32..100, 0u32..100, 0u32..1000)
            .prop_map(|(major, minor, patch)| format!("{}.{}.{}", major, minor, patch))
    }

    proptest! {
        /// Property: Source display contains key info
        #[test]
        fn prop_source_display_contains_info(
            url in url_strategy(),
            version in version_strategy()
        ) {
            if let Ok(url_source) = PackageSource::url(&url, &version) {
                let display = format!("{}", url_source);
                prop_assert!(display.contains(&url), "Display should contain URL");
                prop_assert!(display.contains(&version), "Display should contain version");
            }

            let path_source = PackageSource::path("/test/path");
            let display = format!("{}", path_source);
            prop_assert!(display.contains("path:"), "Path display should have path: prefix");
        }

        /// Property: Cache keys are unique for different sources
        #[test]
        fn prop_cache_key_uniqueness(
            url1 in url_strategy(),
            url2 in url_strategy(),
            v1 in version_strategy(),
            v2 in version_strategy()
        ) {
            if let (Ok(s1), Ok(s2)) = (
                PackageSource::url(&url1, &v1),
                PackageSource::url(&url2, &v2)
            ) {
                if url1 != url2 || v1 != v2 {
                    prop_assert_ne!(s1.cache_key(), s2.cache_key());
                }
            }
        }

        /// Property: Source type detection is exclusive
        #[test]
        fn prop_source_type_exclusive(url in url_strategy(), version in version_strategy()) {
            if let Ok(url_source) = PackageSource::url(&url, &version) {
                prop_assert!(url_source.is_url());
                prop_assert!(!url_source.is_path());
            }

            let path_source = PackageSource::path("/some/path");
            prop_assert!(path_source.is_path());
            prop_assert!(!path_source.is_url());
        }
    }

    #[test]
    fn test_is_secure_https() {
        let source = PackageSource::url("https://example.com/pkg.zip", "1.0.0").unwrap();
        assert!(source.is_secure());
        assert!(source.security_warning().is_none());
    }

    #[test]
    fn test_is_secure_http() {
        let source = PackageSource::url("http://example.com/pkg.zip", "1.0.0").unwrap();
        assert!(!source.is_secure());
        assert!(source.security_warning().is_some());
        assert!(source.security_warning().unwrap().contains("insecure"));
    }

    #[test]
    fn test_is_secure_path() {
        let source = PackageSource::path("/local/path");
        assert!(source.is_secure());
        assert!(source.security_warning().is_none());
    }
}
