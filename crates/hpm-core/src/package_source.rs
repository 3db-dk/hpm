//! Package source definitions for Git release-based dependencies.
//!
//! This module defines the types for specifying where packages come from.
//! HPM uses Git release artifacts as the primary package distribution mechanism,
//! with local path dependencies for development.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use thiserror::Error;

/// Errors that can occur when working with package sources.
#[derive(Debug, Error)]
pub enum PackageSourceError {
    #[error("Invalid Git URL: {0}")]
    InvalidGitUrl(String),

    #[error("Invalid version: {0}")]
    InvalidVersion(String),

    #[error("Path does not exist: {0}")]
    PathNotFound(PathBuf),

    #[error("Unsupported Git provider for URL: {0}")]
    UnsupportedGitProvider(String),
}

/// Represents where a package comes from.
///
/// HPM supports two package sources:
/// - Git repositories (via release artifact download)
/// - Local filesystem paths (for development)
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum PackageSource {
    /// A Git repository with a specific version.
    ///
    /// The package will be fetched as a release artifact from the Git hosting provider.
    /// Package authors must create releases with artifacts following the naming
    /// convention `{package-name}-{version}.zip`.
    Git {
        /// The Git repository URL (e.g., "https://github.com/owner/repo")
        url: String,
        /// The version (e.g., "1.0.0", extracted from tag like "v1.0.0")
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
    /// Create a new Git source.
    ///
    /// # Arguments
    /// * `url` - The Git repository URL
    /// * `version` - The package version
    ///
    /// # Errors
    /// Returns an error if the URL or version is invalid.
    pub fn git(
        url: impl Into<String>,
        version: impl Into<String>,
    ) -> Result<Self, PackageSourceError> {
        let url = url.into();
        let version = version.into();

        // Validate URL has a reasonable format
        if !url.starts_with("https://") && !url.starts_with("http://") {
            return Err(PackageSourceError::InvalidGitUrl(format!(
                "URL must start with https:// or http://: {}",
                url
            )));
        }

        // Validate version format
        if version.is_empty() {
            return Err(PackageSourceError::InvalidVersion(
                "Version cannot be empty".to_string(),
            ));
        }

        // Version should be a valid semver-like string
        if version.starts_with('.') || version.ends_with('.') {
            return Err(PackageSourceError::InvalidVersion(format!(
                "Version has invalid format: {}",
                version
            )));
        }

        Ok(PackageSource::Git { url, version })
    }

    /// Create a new path source.
    ///
    /// # Arguments
    /// * `path` - Path to the package directory
    pub fn path(path: impl Into<PathBuf>) -> Self {
        PackageSource::Path { path: path.into() }
    }

    /// Check if this is a Git source.
    pub fn is_git(&self) -> bool {
        matches!(self, PackageSource::Git { .. })
    }

    /// Check if this is a path source.
    pub fn is_path(&self) -> bool {
        matches!(self, PackageSource::Path { .. })
    }

    /// Get the Git URL if this is a Git source.
    pub fn git_url(&self) -> Option<&str> {
        match self {
            PackageSource::Git { url, .. } => Some(url),
            _ => None,
        }
    }

    /// Get the version if this is a Git source.
    pub fn git_version(&self) -> Option<&str> {
        match self {
            PackageSource::Git { version, .. } => Some(version),
            _ => None,
        }
    }

    /// Get the path if this is a path source.
    pub fn local_path(&self) -> Option<&PathBuf> {
        match self {
            PackageSource::Path { path } => Some(path),
            _ => None,
        }
    }

    /// Returns true if the source uses secure transport (HTTPS).
    ///
    /// For Git sources, this checks if the URL uses HTTPS.
    /// Path sources are always considered secure (local filesystem).
    pub fn is_secure(&self) -> bool {
        match self {
            PackageSource::Git { url, .. } => url.starts_with("https://"),
            PackageSource::Path { .. } => true,
        }
    }

    /// Returns a security warning message if the source uses insecure transport.
    ///
    /// Returns `Some` with a warning message for HTTP URLs.
    /// Returns `None` for HTTPS URLs and local paths.
    pub fn security_warning(&self) -> Option<&'static str> {
        if !self.is_secure() {
            Some("Using insecure HTTP. Consider HTTPS for better security.")
        } else {
            None
        }
    }

    /// Generate a unique cache key for this source.
    ///
    /// For Git sources, this is based on the URL and version.
    /// For path sources, this is based on the absolute path.
    pub fn cache_key(&self) -> String {
        match self {
            PackageSource::Git { url, version } => {
                // Extract owner/repo from URL for a readable cache key
                let repo_part = url
                    .trim_end_matches(".git")
                    .rsplit('/')
                    .take(2)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect::<Vec<_>>()
                    .join("-");

                format!("{}-{}", repo_part, version)
            }
            PackageSource::Path { path } => {
                // Use path hash for local sources
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

/// Identifies the Git hosting provider for a repository URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitProvider {
    GitHub,
    GitLab,
    /// Gitea instance (including self-hosted)
    Gitea {
        host: String,
    },
    /// Codeberg (runs on Gitea)
    Codeberg,
    Bitbucket,
    /// Unknown provider
    Unknown,
}

impl GitProvider {
    /// Detect the provider from a Git URL.
    pub fn from_url(url: &str) -> Self {
        let url_lower = url.to_lowercase();
        if url_lower.contains("github.com") {
            GitProvider::GitHub
        } else if url_lower.contains("gitlab.com") {
            GitProvider::GitLab
        } else if url_lower.contains("codeberg.org") {
            GitProvider::Codeberg
        } else if url_lower.contains("bitbucket.org") {
            GitProvider::Bitbucket
        } else {
            // For unknown hosts, assume Gitea (most self-hosted instances use Gitea/Forgejo)
            // Extract host from URL
            let host = url
                .trim_start_matches("https://")
                .trim_start_matches("http://")
                .split('/')
                .next()
                .unwrap_or("unknown")
                .to_string();
            GitProvider::Gitea { host }
        }
    }

    /// Construct the release asset download URL for this provider.
    ///
    /// # Arguments
    /// * `repo_url` - The base repository URL
    /// * `tag` - The release tag (e.g., "v1.0.0")
    /// * `package_name` - The package name
    /// * `version` - The version (e.g., "1.0.0")
    ///
    /// # Returns
    /// The URL to download the release asset.
    pub fn release_asset_url(
        &self,
        repo_url: &str,
        tag: &str,
        package_name: &str,
        version: &str,
    ) -> Result<String, PackageSourceError> {
        // Normalize URL (remove trailing slashes and .git)
        let base_url = repo_url.trim_end_matches('/').trim_end_matches(".git");

        // Expected asset filename
        let asset_name = format!("{}-{}.zip", package_name, version);

        match self {
            GitProvider::GitHub => {
                // GitHub: https://github.com/{owner}/{repo}/releases/download/{tag}/{asset}
                Ok(format!(
                    "{}/releases/download/{}/{}",
                    base_url, tag, asset_name
                ))
            }
            GitProvider::GitLab => {
                // GitLab: https://gitlab.com/{owner}/{repo}/-/releases/{tag}/downloads/{asset}
                Ok(format!(
                    "{}/-/releases/{}/downloads/{}",
                    base_url, tag, asset_name
                ))
            }
            GitProvider::Gitea { .. } => {
                // Gitea: https://{host}/{owner}/{repo}/releases/download/{tag}/{asset}
                Ok(format!(
                    "{}/releases/download/{}/{}",
                    base_url, tag, asset_name
                ))
            }
            GitProvider::Codeberg => {
                // Codeberg (Gitea): https://codeberg.org/{owner}/{repo}/releases/download/{tag}/{asset}
                Ok(format!(
                    "{}/releases/download/{}/{}",
                    base_url, tag, asset_name
                ))
            }
            GitProvider::Bitbucket => {
                // Bitbucket: https://bitbucket.org/{owner}/{repo}/downloads/{asset}
                // Note: Bitbucket uses a general downloads section, not per-release
                Ok(format!("{}/downloads/{}", base_url, asset_name))
            }
            GitProvider::Unknown => {
                // For unknown providers, try Gitea-style URL as a fallback
                Ok(format!(
                    "{}/releases/download/{}/{}",
                    base_url, tag, asset_name
                ))
            }
        }
    }

    /// Construct the archive download URL for this provider (for source archives).
    ///
    /// Note: This is kept for backwards compatibility but release_asset_url should
    /// be preferred for downloading pre-built packages.
    ///
    /// # Arguments
    /// * `repo_url` - The base repository URL
    /// * `commit` - The commit hash or tag
    ///
    /// # Returns
    /// The URL to download the source archive.
    pub fn archive_url(&self, repo_url: &str, commit: &str) -> Result<String, PackageSourceError> {
        // Normalize URL (remove trailing slashes and .git)
        let base_url = repo_url.trim_end_matches('/').trim_end_matches(".git");

        match self {
            GitProvider::GitHub => {
                // GitHub: https://github.com/{owner}/{repo}/archive/{commit}.zip
                Ok(format!("{}/archive/{}.zip", base_url, commit))
            }
            GitProvider::GitLab => {
                // GitLab: https://gitlab.com/{owner}/{repo}/-/archive/{commit}/{repo}-{commit}.zip
                let repo_name = base_url.rsplit('/').next().unwrap_or("repo");
                Ok(format!(
                    "{}/-/archive/{}/{}-{}.zip",
                    base_url, commit, repo_name, commit
                ))
            }
            GitProvider::Gitea { .. } | GitProvider::Codeberg => {
                // Gitea/Codeberg: https://{host}/{owner}/{repo}/archive/{commit}.zip
                Ok(format!("{}/archive/{}.zip", base_url, commit))
            }
            GitProvider::Bitbucket => {
                // Bitbucket: https://bitbucket.org/{owner}/{repo}/get/{commit}.zip
                Ok(format!("{}/get/{}.zip", base_url, commit))
            }
            GitProvider::Unknown => {
                // For unknown providers, try GitHub-style URL as a fallback
                Ok(format!("{}/archive/{}.zip", base_url, commit))
            }
        }
    }
}

impl std::fmt::Display for PackageSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PackageSource::Git { url, version } => {
                write!(f, "{}@{}", url, version)
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

    // Property-based tests for comprehensive coverage with random inputs

    /// Strategy to generate valid Git URLs
    fn git_url_strategy() -> impl Strategy<Value = String> {
        prop_oneof![
            Just("https://github.com/owner/repo".to_string()),
            Just("https://gitlab.com/owner/repo".to_string()),
            Just("https://codeberg.org/owner/repo".to_string()),
            Just("https://bitbucket.org/owner/repo".to_string()),
            r"https://[a-z]+\.[a-z]+/[a-z]+/[a-z]+".prop_filter("valid url", |s| s.len() < 100),
        ]
    }

    /// Strategy to generate valid versions
    fn version_strategy() -> impl Strategy<Value = String> {
        (0u32..100, 0u32..100, 0u32..1000)
            .prop_map(|(major, minor, patch)| format!("{}.{}.{}", major, minor, patch))
    }

    /// Strategy to generate valid package names
    fn package_name_strategy() -> impl Strategy<Value = String> {
        r"[a-z][a-z0-9-]{1,30}".prop_filter("no double dashes", |s| !s.contains("--"))
    }

    /// Strategy to generate release tags
    fn tag_strategy() -> impl Strategy<Value = String> {
        version_strategy().prop_map(|v| format!("v{}", v))
    }

    proptest! {
        /// Property: Git provider detection is consistent (same URL always gives same provider)
        #[test]
        fn prop_git_provider_detection_consistent(url in git_url_strategy()) {
            let provider1 = GitProvider::from_url(&url);
            let provider2 = GitProvider::from_url(&url);
            prop_assert_eq!(
                std::mem::discriminant(&provider1),
                std::mem::discriminant(&provider2),
                "Provider detection should be deterministic"
            );
        }

        /// Property: Release asset URLs have correct structure
        #[test]
        fn prop_release_asset_url_structure(
            url in git_url_strategy(),
            tag in tag_strategy(),
            name in package_name_strategy(),
            version in version_strategy()
        ) {
            let provider = GitProvider::from_url(&url);
            let result = provider.release_asset_url(&url, &tag, &name, &version);

            prop_assert!(result.is_ok(), "Should generate URL for valid inputs");

            let asset_url = result.unwrap();
            prop_assert!(asset_url.contains(&name), "URL should contain package name");
            prop_assert!(asset_url.ends_with(".zip"), "URL should end with .zip");
        }

        /// Property: Source display contains key info
        #[test]
        fn prop_source_display_contains_info(
            url in git_url_strategy(),
            version in version_strategy()
        ) {
            if let Ok(git_source) = PackageSource::git(&url, &version) {
                let display = format!("{}", git_source);
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
            url1 in git_url_strategy(),
            url2 in git_url_strategy(),
            v1 in version_strategy(),
            v2 in version_strategy()
        ) {
            if let (Ok(s1), Ok(s2)) = (
                PackageSource::git(&url1, &v1),
                PackageSource::git(&url2, &v2)
            ) {
                if url1 != url2 || v1 != v2 {
                    prop_assert_ne!(s1.cache_key(), s2.cache_key());
                }
            }
        }

        /// Property: Source type detection is exclusive
        #[test]
        fn prop_source_type_exclusive(url in git_url_strategy(), version in version_strategy()) {
            if let Ok(git_source) = PackageSource::git(&url, &version) {
                prop_assert!(git_source.is_git());
                prop_assert!(!git_source.is_path());
            }

            let path_source = PackageSource::path("/some/path");
            prop_assert!(path_source.is_path());
            prop_assert!(!path_source.is_git());
        }
    }

    // Keep security tests (critical for security auditing)

    #[test]
    fn test_is_secure_https() {
        let source = PackageSource::git("https://github.com/owner/repo", "1.0.0").unwrap();
        assert!(source.is_secure());
        assert!(source.security_warning().is_none());
    }

    #[test]
    fn test_is_secure_http() {
        let source = PackageSource::git("http://github.com/owner/repo", "1.0.0").unwrap();
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
