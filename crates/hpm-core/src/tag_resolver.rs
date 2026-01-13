//! Tag resolver for converting Git tags to commit hashes.
//!
//! This module provides functionality to resolve Git tags to their corresponding
//! commit hashes using provider-specific REST APIs.

use crate::package_source::GitProvider;
use serde::Deserialize;
use thiserror::Error;
use tracing::{debug, info};

/// Errors that can occur during tag resolution.
#[derive(Error, Debug)]
pub enum TagResolveError {
    #[error("Tag '{tag}' not found in repository '{repo}'")]
    TagNotFound { tag: String, repo: String },

    #[error("Failed to connect to Git provider: {0}")]
    NetworkError(#[from] reqwest::Error),

    #[error("Tag resolution is not supported for this Git provider")]
    UnsupportedProvider,

    #[error("Failed to parse API response: {0}")]
    ParseError(String),

    #[error("Invalid repository URL: {0}")]
    InvalidUrl(String),
}

/// Resolves Git tags to commit hashes using provider REST APIs.
#[derive(Clone)]
pub struct TagResolver {
    http_client: reqwest::Client,
}

impl TagResolver {
    /// Create a new tag resolver.
    pub fn new() -> Result<Self, TagResolveError> {
        let http_client = reqwest::Client::builder()
            .user_agent("hpm/0.1.0")
            .timeout(std::time::Duration::from_secs(30))
            .build()?;

        Ok(Self { http_client })
    }

    /// Resolve a tag to its commit hash.
    ///
    /// # Arguments
    /// * `url` - The Git repository URL (e.g., "https://github.com/owner/repo")
    /// * `tag` - The tag name to resolve (e.g., "v1.0.0")
    ///
    /// # Returns
    /// The full commit hash that the tag points to.
    pub async fn resolve(&self, url: &str, tag: &str) -> Result<String, TagResolveError> {
        let provider = GitProvider::from_url(url);
        let (owner, repo) = parse_owner_repo(url)?;

        info!("Resolving tag '{}' for {}/{} via {:?}", tag, owner, repo, provider);

        match provider {
            GitProvider::GitHub => self.resolve_github(&owner, &repo, tag).await,
            GitProvider::GitLab => self.resolve_gitlab(&owner, &repo, tag).await,
            GitProvider::Bitbucket => self.resolve_bitbucket(&owner, &repo, tag).await,
            GitProvider::Unknown => Err(TagResolveError::UnsupportedProvider),
        }
    }

    /// Resolve a tag using GitHub's API.
    async fn resolve_github(
        &self,
        owner: &str,
        repo: &str,
        tag: &str,
    ) -> Result<String, TagResolveError> {
        // GitHub API: GET /repos/{owner}/{repo}/git/ref/tags/{tag}
        let api_url = format!(
            "https://api.github.com/repos/{}/{}/git/ref/tags/{}",
            owner, repo, tag
        );

        debug!("GitHub API request: {}", api_url);

        let response = self
            .http_client
            .get(&api_url)
            .header("Accept", "application/vnd.github+json")
            .send()
            .await?;

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(TagResolveError::TagNotFound {
                tag: tag.to_string(),
                repo: format!("{}/{}", owner, repo),
            });
        }

        let response = response.error_for_status()?;
        let git_ref: GitHubRef = response
            .json()
            .await
            .map_err(|e| TagResolveError::ParseError(e.to_string()))?;

        // For annotated tags, the object type is "tag" and we need to dereference
        if git_ref.object.object_type == "tag" {
            debug!("Annotated tag detected, dereferencing...");
            return self.dereference_github_tag(owner, repo, &git_ref.object.sha).await;
        }

        Ok(git_ref.object.sha)
    }

    /// Dereference an annotated tag to get the commit it points to.
    async fn dereference_github_tag(
        &self,
        owner: &str,
        repo: &str,
        tag_sha: &str,
    ) -> Result<String, TagResolveError> {
        // GitHub API: GET /repos/{owner}/{repo}/git/tags/{tag_sha}
        let api_url = format!(
            "https://api.github.com/repos/{}/{}/git/tags/{}",
            owner, repo, tag_sha
        );

        debug!("GitHub tag dereference request: {}", api_url);

        let response = self
            .http_client
            .get(&api_url)
            .header("Accept", "application/vnd.github+json")
            .send()
            .await?
            .error_for_status()?;

        let tag_object: GitHubTagObject = response
            .json()
            .await
            .map_err(|e| TagResolveError::ParseError(e.to_string()))?;

        Ok(tag_object.object.sha)
    }

    /// Resolve a tag using GitLab's API.
    async fn resolve_gitlab(
        &self,
        owner: &str,
        repo: &str,
        tag: &str,
    ) -> Result<String, TagResolveError> {
        // GitLab API: GET /api/v4/projects/{owner}%2F{repo}/repository/tags/{tag}
        let project_id = format!("{}%2F{}", owner, repo);
        let api_url = format!(
            "https://gitlab.com/api/v4/projects/{}/repository/tags/{}",
            project_id, tag
        );

        debug!("GitLab API request: {}", api_url);

        let response = self.http_client.get(&api_url).send().await?;

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(TagResolveError::TagNotFound {
                tag: tag.to_string(),
                repo: format!("{}/{}", owner, repo),
            });
        }

        let response = response.error_for_status()?;
        let tag_info: GitLabTag = response
            .json()
            .await
            .map_err(|e| TagResolveError::ParseError(e.to_string()))?;

        Ok(tag_info.commit.id)
    }

    /// Resolve a tag using Bitbucket's API.
    async fn resolve_bitbucket(
        &self,
        owner: &str,
        repo: &str,
        tag: &str,
    ) -> Result<String, TagResolveError> {
        // Bitbucket API: GET /2.0/repositories/{owner}/{repo}/refs/tags/{tag}
        let api_url = format!(
            "https://api.bitbucket.org/2.0/repositories/{}/{}/refs/tags/{}",
            owner, repo, tag
        );

        debug!("Bitbucket API request: {}", api_url);

        let response = self.http_client.get(&api_url).send().await?;

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(TagResolveError::TagNotFound {
                tag: tag.to_string(),
                repo: format!("{}/{}", owner, repo),
            });
        }

        let response = response.error_for_status()?;
        let tag_info: BitbucketTag = response
            .json()
            .await
            .map_err(|e| TagResolveError::ParseError(e.to_string()))?;

        Ok(tag_info.target.hash)
    }
}

impl Default for TagResolver {
    fn default() -> Self {
        Self::new().expect("Failed to create default TagResolver")
    }
}

/// Parse owner and repository name from a Git URL.
fn parse_owner_repo(url: &str) -> Result<(String, String), TagResolveError> {
    // Normalize URL: remove trailing slashes and .git suffix
    let normalized = url
        .trim_end_matches('/')
        .trim_end_matches(".git");

    // Parse as URL to extract path
    let parsed = url::Url::parse(normalized)
        .map_err(|e| TagResolveError::InvalidUrl(e.to_string()))?;

    let path = parsed.path().trim_start_matches('/');
    let parts: Vec<&str> = path.split('/').collect();

    if parts.len() < 2 {
        return Err(TagResolveError::InvalidUrl(format!(
            "URL must contain owner/repo: {}",
            url
        )));
    }

    Ok((parts[0].to_string(), parts[1].to_string()))
}

// GitHub API response types

#[derive(Debug, Deserialize)]
struct GitHubRef {
    object: GitHubRefObject,
}

#[derive(Debug, Deserialize)]
struct GitHubRefObject {
    sha: String,
    #[serde(rename = "type")]
    object_type: String,
}

#[derive(Debug, Deserialize)]
struct GitHubTagObject {
    object: GitHubTagTarget,
}

#[derive(Debug, Deserialize)]
struct GitHubTagTarget {
    sha: String,
}

// GitLab API response types

#[derive(Debug, Deserialize)]
struct GitLabTag {
    commit: GitLabCommit,
}

#[derive(Debug, Deserialize)]
struct GitLabCommit {
    id: String,
}

// Bitbucket API response types

#[derive(Debug, Deserialize)]
struct BitbucketTag {
    target: BitbucketTarget,
}

#[derive(Debug, Deserialize)]
struct BitbucketTarget {
    hash: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_owner_repo_github() {
        let (owner, repo) = parse_owner_repo("https://github.com/owner/repo").unwrap();
        assert_eq!(owner, "owner");
        assert_eq!(repo, "repo");
    }

    #[test]
    fn test_parse_owner_repo_with_git_suffix() {
        let (owner, repo) = parse_owner_repo("https://github.com/owner/repo.git").unwrap();
        assert_eq!(owner, "owner");
        assert_eq!(repo, "repo");
    }

    #[test]
    fn test_parse_owner_repo_gitlab() {
        let (owner, repo) = parse_owner_repo("https://gitlab.com/group/project").unwrap();
        assert_eq!(owner, "group");
        assert_eq!(repo, "project");
    }

    #[test]
    fn test_parse_owner_repo_bitbucket() {
        let (owner, repo) = parse_owner_repo("https://bitbucket.org/team/repo").unwrap();
        assert_eq!(owner, "team");
        assert_eq!(repo, "repo");
    }

    #[test]
    fn test_parse_owner_repo_trailing_slash() {
        let (owner, repo) = parse_owner_repo("https://github.com/owner/repo/").unwrap();
        assert_eq!(owner, "owner");
        assert_eq!(repo, "repo");
    }

    #[test]
    fn test_parse_owner_repo_invalid() {
        let result = parse_owner_repo("https://github.com/onlyone");
        assert!(result.is_err());
    }
}
