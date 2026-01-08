//! Package source definitions for Git archive-based dependencies.
//!
//! This module defines the types for specifying where packages come from.
//! HPM uses Git archives as the primary package distribution mechanism,
//! with local path dependencies for development.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use thiserror::Error;

/// Errors that can occur when working with package sources.
#[derive(Debug, Error)]
pub enum PackageSourceError {
    #[error("Invalid Git URL: {0}")]
    InvalidGitUrl(String),

    #[error("Invalid commit hash: {0}")]
    InvalidCommitHash(String),

    #[error("Path does not exist: {0}")]
    PathNotFound(PathBuf),

    #[error("Unsupported Git provider for URL: {0}")]
    UnsupportedGitProvider(String),
}

/// Represents where a package comes from.
///
/// HPM supports two package sources:
/// - Git repositories (via archive download)
/// - Local filesystem paths (for development)
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum PackageSource {
    /// A Git repository with a specific commit hash.
    ///
    /// The package will be fetched as an archive from the Git hosting provider.
    /// Using commit hashes instead of tags ensures reproducibility, since
    /// tags can be redefined.
    Git {
        /// The Git repository URL (e.g., "https://github.com/owner/repo")
        url: String,
        /// The full commit hash (40 hex characters for SHA-1)
        commit: String,
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
    /// * `commit` - The full commit hash
    ///
    /// # Errors
    /// Returns an error if the URL or commit hash is invalid.
    pub fn git(url: impl Into<String>, commit: impl Into<String>) -> Result<Self, PackageSourceError> {
        let url = url.into();
        let commit = commit.into();

        // Validate URL has a reasonable format
        if !url.starts_with("https://") && !url.starts_with("http://") {
            return Err(PackageSourceError::InvalidGitUrl(
                format!("URL must start with https:// or http://: {}", url)
            ));
        }

        // Validate commit hash format (should be hex, typically 40 chars for full SHA-1)
        if commit.is_empty() {
            return Err(PackageSourceError::InvalidCommitHash(
                "Commit hash cannot be empty".to_string()
            ));
        }

        // Allow short hashes (7+ chars) but prefer full 40-char hashes
        if !commit.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(PackageSourceError::InvalidCommitHash(
                format!("Commit hash must be hexadecimal: {}", commit)
            ));
        }

        Ok(PackageSource::Git { url, commit })
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

    /// Get the commit hash if this is a Git source.
    pub fn git_commit(&self) -> Option<&str> {
        match self {
            PackageSource::Git { commit, .. } => Some(commit),
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

    /// Generate a unique cache key for this source.
    ///
    /// For Git sources, this is based on the URL and commit hash.
    /// For path sources, this is based on the absolute path.
    pub fn cache_key(&self) -> String {
        match self {
            PackageSource::Git { url, commit } => {
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

                // Use short commit hash for brevity
                let short_commit = &commit[..commit.len().min(12)];
                format!("{}-{}", repo_part, short_commit)
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitProvider {
    GitHub,
    GitLab,
    Bitbucket,
    /// Unknown provider - will attempt generic Git archive URL
    Unknown,
}

impl GitProvider {
    /// Detect the provider from a Git URL.
    pub fn from_url(url: &str) -> Self {
        let url_lower = url.to_lowercase();
        if url_lower.contains("github.com") {
            GitProvider::GitHub
        } else if url_lower.contains("gitlab.com") || url_lower.contains("gitlab.") {
            GitProvider::GitLab
        } else if url_lower.contains("bitbucket.org") || url_lower.contains("bitbucket.") {
            GitProvider::Bitbucket
        } else {
            GitProvider::Unknown
        }
    }

    /// Construct the archive download URL for this provider.
    ///
    /// # Arguments
    /// * `repo_url` - The base repository URL
    /// * `commit` - The commit hash
    ///
    /// # Returns
    /// The URL to download the archive, or an error if unsupported.
    pub fn archive_url(&self, repo_url: &str, commit: &str) -> Result<String, PackageSourceError> {
        // Normalize URL (remove trailing slashes and .git)
        let base_url = repo_url
            .trim_end_matches('/')
            .trim_end_matches(".git");

        match self {
            GitProvider::GitHub => {
                // GitHub: https://github.com/{owner}/{repo}/archive/{commit}.zip
                Ok(format!("{}/archive/{}.zip", base_url, commit))
            }
            GitProvider::GitLab => {
                // GitLab: https://gitlab.com/{owner}/{repo}/-/archive/{commit}/{repo}-{commit}.zip
                // Extract repo name from URL for the filename
                let repo_name = base_url
                    .rsplit('/')
                    .next()
                    .unwrap_or("repo");
                Ok(format!("{}/-/archive/{}/{}-{}.zip", base_url, commit, repo_name, commit))
            }
            GitProvider::Bitbucket => {
                // Bitbucket: https://bitbucket.org/{owner}/{repo}/get/{commit}.zip
                Ok(format!("{}/get/{}.zip", base_url, commit))
            }
            GitProvider::Unknown => {
                // For unknown providers, try GitHub-style URL as a fallback
                // This might work for many Git hosting platforms
                Ok(format!("{}/archive/{}.zip", base_url, commit))
            }
        }
    }
}

impl std::fmt::Display for PackageSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PackageSource::Git { url, commit } => {
                let short_commit = &commit[..commit.len().min(8)];
                write!(f, "{}@{}", url, short_commit)
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

    #[test]
    fn test_git_source_creation() {
        let source = PackageSource::git(
            "https://github.com/studio/geometry-tools",
            "abc123def456789"
        ).unwrap();

        assert!(source.is_git());
        assert!(!source.is_path());
        assert_eq!(source.git_url(), Some("https://github.com/studio/geometry-tools"));
        assert_eq!(source.git_commit(), Some("abc123def456789"));
    }

    #[test]
    fn test_git_source_invalid_url() {
        let result = PackageSource::git("not-a-url", "abc123");
        assert!(result.is_err());
    }

    #[test]
    fn test_git_source_invalid_commit() {
        let result = PackageSource::git("https://github.com/test/repo", "not-hex!");
        assert!(result.is_err());
    }

    #[test]
    fn test_path_source_creation() {
        let source = PackageSource::path("/path/to/package");
        assert!(source.is_path());
        assert!(!source.is_git());
        assert_eq!(source.local_path(), Some(&PathBuf::from("/path/to/package")));
    }

    #[test]
    fn test_git_provider_detection() {
        assert_eq!(
            GitProvider::from_url("https://github.com/owner/repo"),
            GitProvider::GitHub
        );
        assert_eq!(
            GitProvider::from_url("https://gitlab.com/owner/repo"),
            GitProvider::GitLab
        );
        assert_eq!(
            GitProvider::from_url("https://bitbucket.org/owner/repo"),
            GitProvider::Bitbucket
        );
        assert_eq!(
            GitProvider::from_url("https://custom.git.example.com/repo"),
            GitProvider::Unknown
        );
    }

    #[test]
    fn test_github_archive_url() {
        let url = GitProvider::GitHub.archive_url(
            "https://github.com/owner/repo",
            "abc123"
        ).unwrap();
        assert_eq!(url, "https://github.com/owner/repo/archive/abc123.zip");
    }

    #[test]
    fn test_gitlab_archive_url() {
        let url = GitProvider::GitLab.archive_url(
            "https://gitlab.com/owner/repo",
            "abc123"
        ).unwrap();
        assert_eq!(url, "https://gitlab.com/owner/repo/-/archive/abc123/repo-abc123.zip");
    }

    #[test]
    fn test_bitbucket_archive_url() {
        let url = GitProvider::Bitbucket.archive_url(
            "https://bitbucket.org/owner/repo",
            "abc123"
        ).unwrap();
        assert_eq!(url, "https://bitbucket.org/owner/repo/get/abc123.zip");
    }

    #[test]
    fn test_cache_key_uniqueness() {
        let source1 = PackageSource::git(
            "https://github.com/owner/repo",
            "abc123def456"
        ).unwrap();
        let source2 = PackageSource::git(
            "https://github.com/owner/repo",
            "def789abc012"
        ).unwrap();
        let source3 = PackageSource::git(
            "https://github.com/other/repo",
            "abc123def456"
        ).unwrap();

        assert_ne!(source1.cache_key(), source2.cache_key());
        assert_ne!(source1.cache_key(), source3.cache_key());
    }

    #[test]
    fn test_display() {
        let git_source = PackageSource::git(
            "https://github.com/owner/repo",
            "abc123def456789"
        ).unwrap();
        assert_eq!(
            format!("{}", git_source),
            "https://github.com/owner/repo@abc123de"
        );

        let path_source = PackageSource::path("/local/path");
        assert_eq!(format!("{}", path_source), "path:/local/path");
    }
}
