//! `ArchiveFetcher` input — a URL-addressable package source.
//!
//! Path dependencies skip the fetcher entirely (they're copied straight
//! into the dev CAS via `StorageManager::install_as_dev_copy`), so this
//! type only needs to describe a remote download. The lockfile records
//! both URL and path sources via [`LockedSource`] in `lock.rs`.
//!
//! [`LockedSource`]: crate::lock::LockedSource

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Errors that can occur when constructing a [`PackageSource`].
#[derive(Debug, Error)]
pub enum PackageSourceError {
    #[error("Invalid URL: {0}")]
    InvalidUrl(String),

    #[error("Invalid version: {0}")]
    InvalidVersion(String),
}

/// Where a package archive is fetched from. URL-only by design — if you
/// need to record a local path dependency in the lockfile, use
/// [`LockedSource`].
///
/// [`LockedSource`]: crate::lock::LockedSource
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PackageSource {
    /// Direct download URL for the archive.
    pub url: String,
    /// Resolved package version (e.g. `"1.2.3"`).
    pub version: String,
}

impl PackageSource {
    /// Create a new package source. Validates that `url` carries a `http://`
    /// or `https://` scheme and that `version` is non-empty and not bracketed
    /// by stray `.` characters.
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

        Ok(Self { url, version })
    }

    /// Returns true if the source uses secure transport (HTTPS).
    pub fn is_secure(&self) -> bool {
        self.url.starts_with("https://")
    }

    /// Returns a security warning message if the source uses insecure
    /// transport (plain HTTP).
    pub fn security_warning(&self) -> Option<&'static str> {
        if self.is_secure() {
            None
        } else {
            Some("Using insecure HTTP. Consider HTTPS for better security.")
        }
    }
}

impl std::fmt::Display for PackageSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "url:{}@{}", self.url, self.version)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn url_strategy() -> impl Strategy<Value = String> {
        prop_oneof![
            Just("https://pkg.example.com/packages/test/1.0.0/test-1.0.0.zip".to_string()),
            Just("https://api.3db.dk/v1/registry/packages/test/1.0.0/download".to_string()),
            r"https://[a-z]+\.[a-z]+/[a-z]+/[a-z]+".prop_filter("valid url", |s| s.len() < 100),
        ]
    }

    fn version_strategy() -> impl Strategy<Value = String> {
        (0u32..100, 0u32..100, 0u32..1000)
            .prop_map(|(major, minor, patch)| format!("{}.{}.{}", major, minor, patch))
    }

    proptest! {
        /// Display contains both URL and version.
        #[test]
        fn prop_source_display_contains_info(
            url in url_strategy(),
            version in version_strategy()
        ) {
            if let Ok(source) = PackageSource::url(&url, &version) {
                let display = format!("{}", source);
                prop_assert!(display.contains(&url));
                prop_assert!(display.contains(&version));
            }
        }
    }

    #[test]
    fn https_is_secure() {
        let source = PackageSource::url("https://example.com/pkg.zip", "1.0.0").unwrap();
        assert!(source.is_secure());
        assert!(source.security_warning().is_none());
    }

    #[test]
    fn http_is_not_secure() {
        let source = PackageSource::url("http://example.com/pkg.zip", "1.0.0").unwrap();
        assert!(!source.is_secure());
        assert!(source.security_warning().unwrap().contains("insecure"));
    }

    #[test]
    fn rejects_unknown_scheme() {
        assert!(PackageSource::url("file:///local", "1.0.0").is_err());
        assert!(PackageSource::url("git@example.com:foo.git", "1.0.0").is_err());
    }

    #[test]
    fn rejects_empty_version() {
        assert!(PackageSource::url("https://example.com/pkg.zip", "").is_err());
    }
}
