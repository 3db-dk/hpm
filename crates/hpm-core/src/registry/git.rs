//! Git-hosted registry index client.
//!
//! Implements a Cargo-style git-based package index where package metadata
//! is stored as JSON-lines files in a directory structure based on package name.
//!
//! ## Index structure
//!
//! ```text
//! registry-repo/
//!   config.json
//!   1/
//!     a.json          # 1-char package names
//!   2/
//!     ab.json         # 2-char package names
//!   3/
//!     a/
//!       abc.json      # 3-char package names (first char prefix)
//!   pa/
//!     ck/
//!       package-name.json   # 4+ char names (2-char prefix directories)
//! ```
//!
//! Each `.json` file contains one JSON object per line (one per version).

use super::types::{RegistryConfig, RegistryEntry, SearchResults};
use super::{Registry, RegistryError};
use async_trait::async_trait;
use std::path::PathBuf;
use tracing::{info, warn};

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

    /// Compute the index file path for a package name.
    ///
    /// Follows Cargo's convention:
    /// - 1 char: `1/<name>.json`
    /// - 2 chars: `2/<name>.json`
    /// - 3 chars: `3/<first-char>/<name>.json`
    /// - 4+ chars: `<first-2>/<next-2>/<name>.json`
    fn index_path(&self, name: &str) -> PathBuf {
        let lower = name.to_lowercase();
        let relative = match lower.len() {
            0 => unreachable!("package name cannot be empty"),
            1 => PathBuf::from("1").join(format!("{}.json", lower)),
            2 => PathBuf::from("2").join(format!("{}.json", lower)),
            3 => {
                let first = &lower[..1];
                PathBuf::from("3")
                    .join(first)
                    .join(format!("{}.json", lower))
            }
            _ => {
                let prefix1 = &lower[..2];
                let prefix2 = &lower[2..4.min(lower.len())];
                PathBuf::from(prefix1)
                    .join(prefix2)
                    .join(format!("{}.json", lower))
            }
        };
        self.cache_dir.join(relative)
    }

    /// Parse all entries from a package's index file.
    fn read_entries(&self, name: &str) -> Result<Vec<RegistryEntry>, RegistryError> {
        let path = self.index_path(name);
        if !path.exists() {
            return Err(RegistryError::PackageNotFound {
                name: name.to_string(),
            });
        }

        let content = std::fs::read_to_string(&path)?;
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
                let output = std::process::Command::new("git")
                    .args(["pull", "--ff-only", "-q"])
                    .current_dir(&cache_dir)
                    .output()
                    .map_err(|e| {
                        RegistryError::GitError(format!("Failed to run git pull: {}", e))
                    })?;

                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    warn!("Git pull failed for '{}': {}", display_name, stderr);
                    // Non-fatal: we can still use the cached version
                }
            } else {
                // Clone fresh
                info!(
                    "Cloning registry index '{}' from {}...",
                    display_name, remote_url
                );
                if let Some(parent) = cache_dir.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                let output = std::process::Command::new("git")
                    .args(["clone", "--depth=1", "-q", &remote_url])
                    .arg(&cache_dir)
                    .output()
                    .map_err(|e| {
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

        // Walk the cache directory looking for .json files
        for entry in walkdir::WalkDir::new(&self.cache_dir)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            if path.extension().map_or(true, |ext| ext != "json") {
                continue;
            }
            // Skip config.json
            if path.file_name().is_some_and(|n| n == "config.json") {
                continue;
            }

            // Read entries from this file
            if let Ok(content) = std::fs::read_to_string(path) {
                let mut latest: Option<RegistryEntry> = None;
                for line in content.lines() {
                    let line = line.trim();
                    if line.is_empty() {
                        continue;
                    }
                    if let Ok(entry) = serde_json::from_str::<RegistryEntry>(line) {
                        if entry.name.to_lowercase().contains(&query_lower)
                            || entry
                                .description
                                .as_deref()
                                .is_some_and(|d| d.to_lowercase().contains(&query_lower))
                        {
                            // Keep latest version
                            if latest.as_ref().map_or(true, |l| entry.version > l.version) {
                                latest = Some(entry);
                            }
                        }
                    }
                }
                if let Some(entry) = latest {
                    results.push(entry);
                }
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

    async fn config(&self) -> Result<RegistryConfig, RegistryError> {
        if !self.is_cached() {
            self.update_cache().await?;
        }
        let config_path = self.cache_dir.join("config.json");
        if !config_path.exists() {
            return Ok(RegistryConfig {
                name: Some(self.display_name.clone()),
                api: None,
                public_keys_url: None,
            });
        }
        let content = std::fs::read_to_string(&config_path)?;
        let config: RegistryConfig =
            serde_json::from_str(&content).map_err(|e| RegistryError::ParseError(e.to_string()))?;
        Ok(config)
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
    fn test_index_path_1_char() {
        let tmp = TempDir::new().unwrap();
        let reg = GitRegistry::new("test", "https://example.com", tmp.path());
        let path = reg.index_path("a");
        assert!(path.ends_with("1/a.json"));
    }

    #[test]
    fn test_index_path_2_chars() {
        let tmp = TempDir::new().unwrap();
        let reg = GitRegistry::new("test", "https://example.com", tmp.path());
        let path = reg.index_path("ab");
        assert!(path.ends_with("2/ab.json"));
    }

    #[test]
    fn test_index_path_3_chars() {
        let tmp = TempDir::new().unwrap();
        let reg = GitRegistry::new("test", "https://example.com", tmp.path());
        let path = reg.index_path("abc");
        assert!(path.ends_with("3/a/abc.json"));
    }

    #[test]
    fn test_index_path_long_name() {
        let tmp = TempDir::new().unwrap();
        let reg = GitRegistry::new("test", "https://example.com", tmp.path());
        let path = reg.index_path("package-name");
        assert!(path.ends_with("pa/ck/package-name.json"));
    }

    #[test]
    fn test_index_path_case_insensitive() {
        let tmp = TempDir::new().unwrap();
        let reg = GitRegistry::new("test", "https://example.com", tmp.path());
        let path1 = reg.index_path("MyPackage");
        let path2 = reg.index_path("mypackage");
        assert_eq!(path1, path2);
    }

    #[test]
    fn test_read_entries_not_found() {
        let tmp = TempDir::new().unwrap();
        let reg = GitRegistry::new("test", "https://example.com", tmp.path());
        let result = reg.read_entries("nonexistent");
        assert!(matches!(result, Err(RegistryError::PackageNotFound { .. })));
    }

    #[test]
    fn test_read_entries_parses_json_lines() {
        let tmp = TempDir::new().unwrap();
        let reg = GitRegistry::new("test", "https://example.com", tmp.path());

        // Create index file
        let path = reg.index_path("mops");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let line1 = r#"{"name":"mops","vers":"1.0.0","deps":[],"dl":"https://example.com/mops-1.0.0.tar.gz","yanked":false}"#;
        let line2 = r#"{"name":"mops","vers":"2.0.0","deps":[],"dl":"https://example.com/mops-2.0.0.tar.gz","yanked":false}"#;
        std::fs::write(&path, format!("{}\n{}\n", line1, line2)).unwrap();

        let entries = reg.read_entries("mops").unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].version, "1.0.0");
        assert_eq!(entries[1].version, "2.0.0");
    }
}
