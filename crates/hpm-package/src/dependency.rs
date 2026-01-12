//! HPM dependency specification types.
//!
//! This module defines how dependencies are specified in `hpm.toml` files,
//! supporting Git-based and local path dependencies.

use serde::{Deserialize, Serialize};

/// Dependency specification for HPM packages.
///
/// HPM uses Git archive-based dependencies with explicit commit hashes for
/// reproducibility and security. Tags are not supported because they can be
/// redefined, which poses a security risk.
///
/// # Examples
///
/// ```toml
/// [dependencies]
/// # Git dependency with commit hash (recommended)
/// geometry-tools = { git = "https://github.com/studio/geometry-tools", commit = "abc123def456" }
///
/// # Local path dependency (for development)
/// my-local-pkg = { path = "../my-local-package" }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DependencySpec {
    /// Git repository with explicit commit hash
    Git {
        /// The Git repository URL
        git: String,
        /// The full commit hash (40 hex characters recommended)
        commit: String,
        /// Whether this dependency is optional
        #[serde(default)]
        optional: bool,
    },

    /// Local filesystem path
    Path {
        /// Path to the package directory (absolute or relative to manifest)
        path: String,
        /// Whether this dependency is optional
        #[serde(default)]
        optional: bool,
    },
}

impl DependencySpec {
    /// Create a new Git dependency.
    pub fn git(url: impl Into<String>, commit: impl Into<String>) -> Self {
        DependencySpec::Git {
            git: url.into(),
            commit: commit.into(),
            optional: false,
        }
    }

    /// Create a new path dependency.
    pub fn path(path: impl Into<String>) -> Self {
        DependencySpec::Path {
            path: path.into(),
            optional: false,
        }
    }

    /// Check if this is a Git dependency.
    pub fn is_git(&self) -> bool {
        matches!(self, DependencySpec::Git { .. })
    }

    /// Check if this is a path dependency.
    pub fn is_path(&self) -> bool {
        matches!(self, DependencySpec::Path { .. })
    }

    /// Check if this dependency is optional.
    pub fn is_optional(&self) -> bool {
        match self {
            DependencySpec::Git { optional, .. } => *optional,
            DependencySpec::Path { optional, .. } => *optional,
        }
    }

    /// Get the Git URL if this is a Git dependency.
    pub fn git_url(&self) -> Option<&str> {
        match self {
            DependencySpec::Git { git, .. } => Some(git),
            _ => None,
        }
    }

    /// Get the commit hash if this is a Git dependency.
    pub fn git_commit(&self) -> Option<&str> {
        match self {
            DependencySpec::Git { commit, .. } => Some(commit),
            _ => None,
        }
    }

    /// Get the path if this is a path dependency.
    pub fn local_path(&self) -> Option<&str> {
        match self {
            DependencySpec::Path { path, .. } => Some(path),
            _ => None,
        }
    }

    /// Validate the dependency spec.
    pub fn validate(&self) -> Result<(), String> {
        match self {
            DependencySpec::Git { git, commit, .. } => {
                if git.is_empty() {
                    return Err("Git URL cannot be empty".to_string());
                }
                if !git.starts_with("https://") && !git.starts_with("http://") {
                    return Err(format!(
                        "Git URL must start with https:// or http://: {}",
                        git
                    ));
                }
                if commit.is_empty() {
                    return Err("Commit hash cannot be empty".to_string());
                }
                if !commit.chars().all(|c| c.is_ascii_hexdigit()) {
                    return Err(format!("Commit hash must be hexadecimal: {}", commit));
                }
                Ok(())
            }
            DependencySpec::Path { path, .. } => {
                if path.is_empty() {
                    return Err("Path cannot be empty".to_string());
                }
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dependency_spec_git_serialization() {
        let spec = DependencySpec::Git {
            git: "https://github.com/owner/repo".to_string(),
            commit: "abc123def456789012345678901234567890abcd".to_string(),
            optional: false,
        };
        let json = serde_json::to_string(&spec).unwrap();
        assert!(json.contains("github.com/owner/repo"));
        assert!(json.contains("abc123def456789012345678901234567890abcd"));
    }

    #[test]
    fn dependency_spec_path_serialization() {
        let spec = DependencySpec::Path {
            path: "../local-package".to_string(),
            optional: true,
        };
        let json = serde_json::to_string(&spec).unwrap();
        assert!(json.contains("../local-package"));
        assert!(json.contains("true"));
    }

    #[test]
    fn git_dependency_validation() {
        let valid = DependencySpec::git(
            "https://github.com/test/repo",
            "abc123def456789012345678901234567890abcd",
        );
        assert!(valid.validate().is_ok());

        let empty_url = DependencySpec::Git {
            git: "".to_string(),
            commit: "abc123".to_string(),
            optional: false,
        };
        assert!(empty_url.validate().is_err());

        let invalid_url = DependencySpec::Git {
            git: "not-a-url".to_string(),
            commit: "abc123".to_string(),
            optional: false,
        };
        assert!(invalid_url.validate().is_err());

        let empty_commit = DependencySpec::Git {
            git: "https://github.com/test/repo".to_string(),
            commit: "".to_string(),
            optional: false,
        };
        assert!(empty_commit.validate().is_err());

        let invalid_commit = DependencySpec::Git {
            git: "https://github.com/test/repo".to_string(),
            commit: "not-hex!".to_string(),
            optional: false,
        };
        assert!(invalid_commit.validate().is_err());
    }

    #[test]
    fn path_dependency_validation() {
        let valid = DependencySpec::path("../local-package");
        assert!(valid.validate().is_ok());

        let empty = DependencySpec::Path {
            path: "".to_string(),
            optional: false,
        };
        assert!(empty.validate().is_err());
    }

    #[test]
    fn dependency_helper_methods() {
        let git_dep = DependencySpec::git("https://github.com/test/repo", "abc123");
        assert!(git_dep.is_git());
        assert!(!git_dep.is_path());
        assert!(!git_dep.is_optional());
        assert_eq!(git_dep.git_url(), Some("https://github.com/test/repo"));
        assert_eq!(git_dep.git_commit(), Some("abc123"));
        assert_eq!(git_dep.local_path(), None);

        let path_dep = DependencySpec::path("../local");
        assert!(!path_dep.is_git());
        assert!(path_dep.is_path());
        assert!(!path_dep.is_optional());
        assert_eq!(path_dep.git_url(), None);
        assert_eq!(path_dep.local_path(), Some("../local"));
    }
}
