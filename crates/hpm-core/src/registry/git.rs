//! Git-hosted registry index client.
//!
//! Implements a Cargo-style git-based package index where package metadata
//! is stored as JSON-lines files keyed by the scoped package path.
//!
//! ## Index structure
//!
//! ```text
//! registry-repo/
//!   config.json
//!   <creator>/
//!     <slug>.json     # one file per package, keyed by scoped path
//! ```
//!
//! Each `.json` file contains one JSON object per line (one per version).

use super::types::{RegistryEntry, SearchResults};
use super::{Registry, RegistryError};
use async_trait::async_trait;
use hpm_package::IoOp;
use std::path::PathBuf;
use tracing::info;

/// A Git-hosted package registry index.
pub struct GitRegistry {
    /// Display name
    display_name: String,
    /// Remote URL of the git repository
    remote_url: String,
    /// Local cache directory for the cloned index
    cache_dir: PathBuf,
}

impl GitRegistry {
    /// Create a new Git registry.
    ///
    /// # Arguments
    /// * `name` - Display name for this registry
    /// * `remote_url` - Git remote URL (HTTPS)
    /// * `cache_dir` - Local directory to cache the cloned index
    pub fn new(
        name: impl Into<String>,
        remote_url: impl Into<String>,
        cache_dir: impl Into<PathBuf>,
    ) -> Self {
        Self {
            display_name: name.into(),
            remote_url: remote_url.into(),
            cache_dir: cache_dir.into(),
        }
    }

    /// Compute the index file path for a scoped package path
    /// (`creator/slug` -> `<creator>/<slug>.json`).
    ///
    /// Non-scoped names are rejected: the index layout is keyed on the
    /// scoped package path, and every registry entry carries one.
    fn index_path(&self, name: &str) -> Result<PathBuf, RegistryError> {
        let lower = name.to_lowercase();
        let Some((creator, slug)) = lower.split_once('/') else {
            return Err(RegistryError::ParseError(format!(
                "Package name '{}' is not a scoped path (expected 'creator/slug')",
                name
            )));
        };
        let relative = PathBuf::from(creator).join(format!("{}.json", slug));
        Ok(self.cache_dir.join(relative))
    }

    /// Parse all entries from a package's index file.
    fn read_entries(&self, name: &str) -> Result<Vec<RegistryEntry>, RegistryError> {
        let path = self.index_path(name)?;
        if !path.exists() {
            return Err(RegistryError::PackageNotFound {
                name: name.to_string(),
            });
        }

        let content = std::fs::read_to_string(&path)
            .map_err(|e| IoOp::wrap("read registry index", &path, e))?;
        let mut entries = Vec::new();
        for (line_num, line) in content.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let entry: RegistryEntry = serde_json::from_str(line).map_err(|e| {
                RegistryError::ParseError(format!(
                    "Failed to parse line {} of {}: {}",
                    line_num + 1,
                    path.display(),
                    e
                ))
            })?;
            entries.push(entry);
        }
        Ok(entries)
    }

    /// Check if the local cache exists and has been cloned.
    fn is_cached(&self) -> bool {
        self.cache_dir.join("config.json").exists()
    }

    /// Clone or pull the registry index.
    async fn update_cache(&self) -> Result<(), RegistryError> {
        let cache_dir = self.cache_dir.clone();
        let remote_url = self.remote_url.clone();
        let display_name = self.display_name.clone();

        tokio::task::spawn_blocking(move || {
            if cache_dir.join(".git").exists() {
                // Pull latest
                info!("Updating registry index '{}'...", display_name);
                let mut cmd = std::process::Command::new("git");
                cmd.args(["pull", "--ff-only", "-q"])
                    .current_dir(&cache_dir);
                #[cfg(target_os = "windows")]
                {
                    use std::os::windows::process::CommandExt;
                    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
                }
                let output = cmd.output().map_err(|e| {
                    RegistryError::GitError(format!("Failed to run git pull: {}", e))
                })?;

                if !output.status.success() {
                    // A registry that has silently stopped updating resolves
                    // to stale versions with no indication why — fail instead
                    // of falling back to the old cache.
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    return Err(RegistryError::GitError(format!(
                        "Git pull failed for '{}': {}",
                        display_name, stderr
                    )));
                }
            } else {
                // Clone fresh
                info!(
                    "Cloning registry index '{}' from {}...",
                    display_name, remote_url
                );
                if let Some(parent) = cache_dir.parent() {
                    std::fs::create_dir_all(parent)
                        .map_err(|e| IoOp::wrap("create registry cache parent", parent, e))?;
                }
                let mut cmd = std::process::Command::new("git");
                cmd.args(["clone", "--depth=1", "-q", &remote_url])
                    .arg(&cache_dir);
                #[cfg(target_os = "windows")]
                {
                    use std::os::windows::process::CommandExt;
                    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
                }
                let output = cmd.output().map_err(|e| {
                    RegistryError::GitError(format!("Failed to run git clone: {}", e))
                })?;

                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    return Err(RegistryError::GitError(format!(
                        "Git clone failed: {}",
                        stderr
                    )));
                }
            }
            Ok(())
        })
        .await
        .map_err(|e| RegistryError::GitError(format!("Task join error: {}", e)))?
    }

    /// Search by scanning all index files (brute force - only for small registries).
    fn search_local(&self, query: &str) -> Result<SearchResults, RegistryError> {
        let query_lower = query.to_lowercase();
        let mut results = Vec::new();

        if !self.cache_dir.exists() {
            return Ok(SearchResults {
                packages: vec![],
                total: 0,
            });
        }

        // Walk the cache directory looking for .json files. The cache is a
        // git checkout hpm itself manages — an unreadable file or a
        // malformed index line means the cache is corrupt, which should
        // surface, not silently shrink the search results.
        for entry in walkdir::WalkDir::new(&self.cache_dir) {
            let entry = entry.map_err(|e| {
                RegistryError::GitError(format!("Failed to walk registry cache: {}", e))
            })?;
            let path = entry.path();
            if path.extension().is_none_or(|ext| ext != "json") {
                continue;
            }
            // Skip config.json
            if path.file_name().is_some_and(|n| n == "config.json") {
                continue;
            }

            // Read entries from this file
            let content = std::fs::read_to_string(path)
                .map_err(|e| IoOp::wrap("read registry index", path, e))?;
            let mut latest: Option<RegistryEntry> = None;
            for (line_num, line) in content.lines().enumerate() {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                let entry: RegistryEntry = serde_json::from_str(line).map_err(|e| {
                    RegistryError::ParseError(format!(
                        "Failed to parse line {} of {}: {}",
                        line_num + 1,
                        path.display(),
                        e
                    ))
                })?;
                if entry.name.to_lowercase().contains(&query_lower)
                    || entry
                        .description
                        .as_deref()
                        .is_some_and(|d| d.to_lowercase().contains(&query_lower))
                {
                    // Keep latest version
                    if latest.as_ref().is_none_or(|l| entry.version > l.version) {
                        latest = Some(entry);
                    }
                }
            }
            if let Some(entry) = latest {
                results.push(entry);
            }
        }

        let total = results.len();
        Ok(SearchResults {
            packages: results,
            total,
        })
    }
}

#[async_trait]
impl Registry for GitRegistry {
    async fn search(&self, query: &str) -> Result<SearchResults, RegistryError> {
        if !self.is_cached() {
            self.update_cache().await?;
        }
        self.search_local(query)
    }

    async fn get_versions(&self, name: &str) -> Result<Vec<RegistryEntry>, RegistryError> {
        if !self.is_cached() {
            self.update_cache().await?;
        }
        self.read_entries(name)
    }

    async fn get_version(&self, name: &str, version: &str) -> Result<RegistryEntry, RegistryError> {
        let entries = self.get_versions(name).await?;
        entries
            .into_iter()
            .find(|e| e.version == version)
            .ok_or_else(|| RegistryError::VersionNotFound {
                name: name.to_string(),
                version: version.to_string(),
            })
    }

    async fn refresh(&self) -> Result<(), RegistryError> {
        self.update_cache().await
    }

    fn name(&self) -> &str {
        &self.display_name
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_index_path_rejects_non_scoped_name() {
        let tmp = TempDir::new().unwrap();
        let reg = GitRegistry::new("test", "https://example.com", tmp.path());
        assert!(matches!(
            reg.index_path("package-name"),
            Err(RegistryError::ParseError(_))
        ));
    }

    #[test]
    fn test_index_path_scoped() {
        let tmp = TempDir::new().unwrap();
        let reg = GitRegistry::new("test", "https://example.com", tmp.path());
        let path = reg.index_path("tumblehead/tumble-rig").unwrap();
        assert!(path.ends_with("tumblehead/tumble-rig.json"));
    }

    #[test]
    fn test_index_path_scoped_case_insensitive() {
        let tmp = TempDir::new().unwrap();
        let reg = GitRegistry::new("test", "https://example.com", tmp.path());
        let path1 = reg.index_path("TumbleHead/Tumble-Rig").unwrap();
        let path2 = reg.index_path("tumblehead/tumble-rig").unwrap();
        assert_eq!(path1, path2);
    }

    #[test]
    fn test_read_entries_not_found() {
        let tmp = TempDir::new().unwrap();
        let reg = GitRegistry::new("test", "https://example.com", tmp.path());
        let result = reg.read_entries("creator/nonexistent");
        assert!(matches!(result, Err(RegistryError::PackageNotFound { .. })));
    }

    #[test]
    fn test_read_entries_parses_json_lines() {
        let tmp = TempDir::new().unwrap();
        let reg = GitRegistry::new("test", "https://example.com", tmp.path());

        // Create index file for a scoped package
        let path = reg.index_path("acme/mops").unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let line1 = r#"{"name":"acme/mops","vers":"1.0.0","deps":[],"dl":"https://example.com/mops-1.0.0.tar.gz","yanked":false}"#;
        let line2 = r#"{"name":"acme/mops","vers":"2.0.0","deps":[],"dl":"https://example.com/mops-2.0.0.tar.gz","yanked":false}"#;
        std::fs::write(&path, format!("{}\n{}\n", line1, line2)).unwrap();

        let entries = reg.read_entries("acme/mops").unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].version, "1.0.0");
        assert_eq!(entries[1].version, "2.0.0");
    }
}
