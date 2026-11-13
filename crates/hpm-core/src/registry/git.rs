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
use super::{Registry, RegistryError, select_build_for_host};
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
                crate::process_util::hide_console_std(&mut cmd);
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
                crate::process_util::hide_console_std(&mut cmd);
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
        // A version may have several per-platform build entries. Mirror
        // ApiRegistry: host-platform match first, then universal, else
        // NoCompatibleBuild — never just the first version-string match.
        let builds: Vec<RegistryEntry> = entries
            .into_iter()
            .filter(|e| e.version == version)
            .collect();
        if builds.is_empty() {
            return Err(RegistryError::VersionNotFound {
                name: name.to_string(),
                version: version.to_string(),
            });
        }
        select_build_for_host(&builds, name, version).cloned()
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

    /// A git index serving several per-platform builds of the same version
    /// must resolve to the host's build, exactly like ApiRegistry — not the
    /// first version-string match in file order.
    #[tokio::test]
    async fn get_version_selects_host_platform_build() {
        let tmp = TempDir::new().unwrap();
        let reg = GitRegistry::new("test", "https://example.com", tmp.path());
        // Mark the cache as populated so get_versions doesn't try to clone.
        std::fs::write(tmp.path().join("config.json"), "{}").unwrap();

        let host = hpm_package::Platform::current().expect("host platform detectable");
        // A tagged non-host build first in the file: first-match would pick it.
        let other = if host == hpm_package::Platform::LinuxX86_64 {
            hpm_package::Platform::WindowsX86_64
        } else {
            hpm_package::Platform::LinuxX86_64
        };

        let path = reg.index_path("acme/native").unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let line = |platform: &str, dl: &str| {
            format!(
                r#"{{"name":"acme/native","vers":"1.0.0","deps":[],"dl":"{dl}","platform":"{platform}","yanked":false}}"#
            )
        };
        std::fs::write(
            &path,
            format!(
                "{}\n{}\n",
                line(other.as_str(), "https://example.com/other.zip"),
                line(host.as_str(), "https://example.com/host.zip"),
            ),
        )
        .unwrap();

        let entry = reg.get_version("acme/native", "1.0.0").await.unwrap();
        assert_eq!(entry.dl, "https://example.com/host.zip");
    }

    /// Same-version builds that are all tagged for foreign platforms (and no
    /// universal entry) must fail with NoCompatibleBuild, not silently pick one.
    #[tokio::test]
    async fn get_version_errors_when_no_compatible_build() {
        let tmp = TempDir::new().unwrap();
        let reg = GitRegistry::new("test", "https://example.com", tmp.path());
        std::fs::write(tmp.path().join("config.json"), "{}").unwrap();

        let path = reg.index_path("acme/native").unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            r#"{"name":"acme/native","vers":"1.0.0","deps":[],"dl":"https://example.com/a.zip","platform":"plan9-amd64","yanked":false}"#,
        )
        .unwrap();

        let result = reg.get_version("acme/native", "1.0.0").await;
        assert!(matches!(
            result,
            Err(RegistryError::NoCompatibleBuild { .. })
        ));
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

/// Git registries driven end to end through real `git` against local bare
/// repositories: the inputs that must keep resolving.
#[cfg(test)]
mod remote_tests {
    use super::super::RegistrySet;
    use std::path::Path;
    use std::process::Command;
    use tempfile::TempDir;

    fn git(dir: &Path, args: &[&str]) {
        let status = Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
            .status()
            .expect("git runs");
        assert!(status.success(), "git {args:?} failed");
    }

    /// A bare index repository holding `acme/tools` at the given versions.
    pub(crate) fn bare_index(root: &Path, name: &str, versions: &[&str]) -> std::path::PathBuf {
        let work = root.join(format!("{name}-work"));
        std::fs::create_dir_all(work.join("acme")).unwrap();
        std::fs::write(work.join("config.json"), "{}").unwrap();
        let lines: Vec<String> = versions
            .iter()
            .map(|v| {
                format!(
                    r#"{{"name":"acme/tools","vers":"{v}","deps":[],"dl":"https://example.invalid/tools-{v}.zip","yanked":false}}"#
                )
            })
            .collect();
        std::fs::write(work.join("acme/tools.json"), lines.join("\n") + "\n").unwrap();
        git(&work, &["init", "-q", "-b", "main"]);
        git(&work, &["add", "."]);
        git(&work, &["commit", "-q", "-m", "index"]);
        let bare = root.join(format!("{name}.git"));
        git(
            root,
            &[
                "clone",
                "-q",
                "--bare",
                work.to_str().unwrap(),
                bare.to_str().unwrap(),
            ],
        );
        bare
    }

    fn git_source(name: &str, url: &str) -> hpm_config::RegistrySourceConfig {
        hpm_config::RegistrySourceConfig {
            name: name.into(),
            url: url.into(),
            registry_type: hpm_config::RegistryType::Git,
        }
    }

    /// A studio index on a local or network path resolves, under a display
    /// name with capitals and dots, and a refresh pulls it again.
    #[tokio::test]
    async fn a_local_path_git_registry_resolves_and_refreshes() {
        let tmp = TempDir::new().unwrap();
        let bare = bare_index(tmp.path(), "studio", &["1.0.0", "1.2.0"]);
        let cache = tmp.path().join("registry-cache");
        let set =
            RegistrySet::from_configs(&[git_source("Studio.Reg", bare.to_str().unwrap())], &cache)
                .unwrap();

        let entry = set.resolve_in("acme/tools", "^1", None).await.unwrap();
        assert_eq!(entry.version, "1.2.0");
        for (name, result) in set.refresh_all().await {
            result.unwrap_or_else(|e| panic!("refresh {name}: {e}"));
        }
        let again = set.resolve_in("acme/tools", "=1.0.0", None).await.unwrap();
        assert_eq!(again.version, "1.0.0");
    }

    /// A `file://` URL to the same kind of index resolves too.
    #[tokio::test]
    async fn a_file_url_git_registry_resolves() {
        let tmp = TempDir::new().unwrap();
        let bare = bare_index(tmp.path(), "nas", &["2.0.0"]);
        let url = format!("file://{}", bare.to_str().unwrap().replace('\\', "/"));
        let set = RegistrySet::from_configs(&[git_source("nas", &url)], &tmp.path().join("cache"))
            .unwrap();
        let entry = set.resolve_in("acme/tools", "^2", None).await.unwrap();
        assert_eq!(entry.version, "2.0.0");
    }

    /// The usual remote URL forms build a registry without touching the
    /// network (construction never clones).
    #[test]
    fn remote_url_forms_build_a_registry() {
        let tmp = TempDir::new().unwrap();
        for url in [
            "https://github.com/studio/registry.git",
            "ssh://git@git.example.invalid/studio/registry.git",
            "git@github.com:studio/registry.git",
        ] {
            RegistrySet::from_configs(&[git_source("studio", url)], tmp.path())
                .unwrap_or_else(|e| panic!("{url}: {e}"));
        }
    }
}
