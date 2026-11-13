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
use std::path::{Path, PathBuf};
use tracing::info;

/// A git registry remote that has passed [`hpm_package::validate_git_remote`].
///
/// The only way to build a [`GitRegistry`], so no unchecked string reaches a
/// `git` command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitRemote(String);

impl GitRemote {
    /// Parse a remote URL, refusing option-shaped, helper-transport and
    /// plaintext forms.
    pub fn parse(url: &str) -> Result<Self, RegistryError> {
        hpm_package::validate_git_remote(url).map_err(RegistryError::ParseError)?;
        // A relative local path names a directory relative to where hpm runs;
        // pin it to that absolute path, since git runs elsewhere (in the
        // cache) and records local origins as absolute paths.
        let is_url = url.contains("://");
        let is_scp = url
            .split_once(':')
            .is_some_and(|(host, _)| !host.contains(['/', '\\']) && host.len() > 1);
        if !is_url && !is_scp && !Path::new(url).is_absolute() {
            let cwd = std::env::current_dir().map_err(|e| {
                RegistryError::GitError(format!("cannot resolve relative registry path: {e}"))
            })?;
            return Ok(Self(cwd.join(url).to_string_lossy().into_owned()));
        }
        Ok(Self(url.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The cache directory name for this remote: a digest of the URL, so two
    /// registries share a clone only when they share a remote, and no part
    /// of a registry's name or URL becomes a path component.
    fn cache_key(&self) -> String {
        use sha2::{Digest, Sha256};
        let digest = Sha256::digest(self.0.as_bytes());
        digest[..16].iter().map(|b| format!("{b:02x}")).collect()
    }
}

/// A `git` command with hpm's fixed policy: only the https, ssh and file
/// transports, no credential prompts, and none of the inherited variables
/// that would point it at a different repository or configuration.
fn git_command(current_dir: &Path) -> std::process::Command {
    const INHERITED: [&str; 11] = [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_INDEX_FILE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_COMMON_DIR",
        "GIT_NAMESPACE",
        "GIT_CONFIG",
        "GIT_CONFIG_COUNT",
        "GIT_CONFIG_PARAMETERS",
        "GIT_EXEC_PATH",
    ];
    let mut cmd = std::process::Command::new("git");
    for var in INHERITED {
        cmd.env_remove(var);
    }
    cmd.env("GIT_ALLOW_PROTOCOL", "https:ssh:file")
        .env("GIT_TERMINAL_PROMPT", "0")
        .current_dir(current_dir);
    crate::process_util::hide_console_std(&mut cmd);
    cmd
}

/// A Git-hosted package registry index.
pub struct GitRegistry {
    /// Display name
    display_name: String,
    /// Remote of the git repository
    remote: GitRemote,
    /// Local cache directory for the cloned index
    cache_dir: PathBuf,
}

impl GitRegistry {
    /// Create a new Git registry.
    ///
    /// # Arguments
    /// * `name` - Display name for this registry (never used in a path)
    /// * `remote_url` - Git remote URL, checked by [`GitRemote::parse`]
    /// * `registry_cache_root` - Directory holding every git registry's
    ///   clone; this registry's clone lives in a subdirectory named by a
    ///   digest of its remote
    pub fn new(
        name: impl Into<String>,
        remote_url: &str,
        registry_cache_root: &Path,
    ) -> Result<Self, RegistryError> {
        let remote = GitRemote::parse(remote_url)?;
        let cache_dir = registry_cache_root.join(remote.cache_key());
        Ok(Self {
            display_name: name.into(),
            remote,
            cache_dir,
        })
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
        // Both segments become path components under the clone: only plain
        // names, never `..`, separators or drive syntax.
        let plain = |segment: &str| {
            !segment.is_empty()
                && !segment.starts_with('.')
                && segment
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
        };
        if !plain(creator) || !plain(slug) {
            return Err(RegistryError::ParseError(format!(
                "Package name '{}' is not a valid scoped path",
                name
            )));
        }
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

    /// Make sure the clone exists and is this remote's before it is read: a
    /// missing clone, or one whose origin is another remote (left by an
    /// older hpm's name-keyed layout, or planted), is cloned again.
    async fn ensure_cache(&self) -> Result<(), RegistryError> {
        if self.is_cached() {
            let cache_dir = self.cache_dir.clone();
            let remote = self.remote.clone();
            let ours = tokio::task::spawn_blocking(move || origin_matches(&cache_dir, &remote))
                .await
                .map_err(|e| RegistryError::GitError(format!("Task join error: {}", e)))??;
            if ours {
                return Ok(());
            }
        }
        self.update_cache().await
    }

    /// Check if the local cache exists and has been cloned.
    fn is_cached(&self) -> bool {
        self.cache_dir.join("config.json").exists()
    }

    /// Clone or pull the registry index.
    ///
    /// A clone whose `origin` is not this registry's remote is discarded and
    /// re-cloned rather than pulled, so a cache never follows another remote.
    async fn update_cache(&self) -> Result<(), RegistryError> {
        let cache_dir = self.cache_dir.clone();
        let remote = self.remote.clone();
        let display_name = self.display_name.clone();

        tokio::task::spawn_blocking(move || {
            if cache_dir.join(".git").exists() && !origin_matches(&cache_dir, &remote)? {
                info!(
                    "Registry index '{}' cache points at another remote; re-cloning",
                    display_name
                );
                std::fs::remove_dir_all(&cache_dir)
                    .map_err(|e| IoOp::wrap("remove stale registry cache", &cache_dir, e))?;
            }
            if !cache_dir.join(".git").exists() && cache_dir.exists() {
                // Not a clone (a partial or foreign directory): start over.
                std::fs::remove_dir_all(&cache_dir)
                    .map_err(|e| IoOp::wrap("remove stale registry cache", &cache_dir, e))?;
            }
            if cache_dir.join(".git").exists() {
                // Pull latest
                info!("Updating registry index '{}'...", display_name);
                let output = git_command(&cache_dir)
                    .args(["pull", "--ff-only", "-q"])
                    .output()
                    .map_err(|e| {
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
                    display_name,
                    remote.as_str()
                );
                let parent = cache_dir.parent().ok_or_else(|| {
                    RegistryError::GitError("Registry cache directory has no parent".to_string())
                })?;
                std::fs::create_dir_all(parent)
                    .map_err(|e| IoOp::wrap("create registry cache parent", parent, e))?;
                // `--` ends option parsing: the remote and destination are
                // only ever positional arguments.
                let output = git_command(parent)
                    .args(["clone", "--depth=1", "-q", "--", remote.as_str()])
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

/// Whether the clone at `cache_dir` was made from `remote`.
fn origin_matches(cache_dir: &Path, remote: &GitRemote) -> Result<bool, RegistryError> {
    let output = git_command(cache_dir)
        .args(["config", "--get", "remote.origin.url"])
        .output()
        .map_err(|e| RegistryError::GitError(format!("Failed to run git config: {}", e)))?;
    // A clone without an origin (exit 1) is not this remote's clone either.
    if !output.status.success() {
        return Ok(false);
    }
    let origin = String::from_utf8_lossy(&output.stdout)
        .trim_end_matches(['\r', '\n'])
        .to_string();
    if origin == remote.as_str() {
        return Ok(true);
    }
    // git may record a local path in another spelling (symlinks resolved,
    // separators normalised); the same directory is the same remote.
    Ok(
        match (
            std::fs::canonicalize(&origin),
            std::fs::canonicalize(remote.as_str()),
        ) {
            (Ok(a), Ok(b)) => a == b,
            _ => false,
        },
    )
}

#[async_trait]
impl Registry for GitRegistry {
    async fn search(&self, query: &str) -> Result<SearchResults, RegistryError> {
        self.ensure_cache().await?;
        self.search_local(query)
    }

    async fn get_versions(&self, name: &str) -> Result<Vec<RegistryEntry>, RegistryError> {
        self.ensure_cache().await?;
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

    /// Make `reg`'s cache a clone of its remote as far as hpm can tell (a
    /// git checkout whose origin is the remote, holding `config.json`), so a
    /// lookup reads it without cloning.
    fn seed_cache(reg: &GitRegistry) {
        std::fs::create_dir_all(&reg.cache_dir).unwrap();
        std::fs::write(reg.cache_dir.join("config.json"), "{}").unwrap();
        for args in [
            vec!["init", "-q"],
            vec!["remote", "add", "origin", reg.remote.as_str()],
        ] {
            let status = std::process::Command::new("git")
                .args(&args)
                .current_dir(&reg.cache_dir)
                .status()
                .unwrap();
            assert!(status.success());
        }
    }

    #[test]
    fn test_index_path_rejects_non_scoped_name() {
        let tmp = TempDir::new().unwrap();
        let reg = GitRegistry::new("test", "https://example.com", tmp.path()).unwrap();
        assert!(matches!(
            reg.index_path("package-name"),
            Err(RegistryError::ParseError(_))
        ));
    }

    #[test]
    fn test_index_path_scoped() {
        let tmp = TempDir::new().unwrap();
        let reg = GitRegistry::new("test", "https://example.com", tmp.path()).unwrap();
        let path = reg.index_path("tumblehead/tumble-rig").unwrap();
        assert!(path.ends_with("tumblehead/tumble-rig.json"));
    }

    #[test]
    fn test_index_path_scoped_case_insensitive() {
        let tmp = TempDir::new().unwrap();
        let reg = GitRegistry::new("test", "https://example.com", tmp.path()).unwrap();
        let path1 = reg.index_path("TumbleHead/Tumble-Rig").unwrap();
        let path2 = reg.index_path("tumblehead/tumble-rig").unwrap();
        assert_eq!(path1, path2);
    }

    #[test]
    fn test_read_entries_not_found() {
        let tmp = TempDir::new().unwrap();
        let reg = GitRegistry::new("test", "https://example.com", tmp.path()).unwrap();
        let result = reg.read_entries("creator/nonexistent");
        assert!(matches!(result, Err(RegistryError::PackageNotFound { .. })));
    }

    /// A git index serving several per-platform builds of the same version
    /// must resolve to the host's build, exactly like ApiRegistry — not the
    /// first version-string match in file order.
    #[tokio::test]
    async fn get_version_selects_host_platform_build() {
        let tmp = TempDir::new().unwrap();
        let reg = GitRegistry::new("test", "https://example.com", tmp.path()).unwrap();
        // Mark the cache as populated so get_versions doesn't try to clone.
        seed_cache(&reg);

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
        let reg = GitRegistry::new("test", "https://example.com", tmp.path()).unwrap();
        seed_cache(&reg);

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
        let reg = GitRegistry::new("test", "https://example.com", tmp.path()).unwrap();

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

    /// Refreshing a local-path registry pulls into the existing clone; it is
    /// not discarded and cloned again (the clone's recorded origin must be
    /// recognised as the configured path, through symlinked temp dirs too).
    #[tokio::test]
    async fn refreshing_a_local_path_registry_keeps_its_clone() {
        let tmp = TempDir::new().unwrap();
        let bare = bare_index(tmp.path(), "studio", &["1.0.0"]);
        let cache = tmp.path().join("registry-cache");
        let set =
            RegistrySet::from_configs(&[git_source("studio", bare.to_str().unwrap())], &cache)
                .unwrap();
        set.resolve_in("acme/tools", "^1", None).await.unwrap();
        let clone = std::fs::read_dir(&cache)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        std::fs::write(clone.join(".kept"), "").unwrap();
        for (name, result) in set.refresh_all().await {
            result.unwrap_or_else(|e| panic!("refresh {name}: {e}"));
        }
        set.resolve_in("acme/tools", "^1", None).await.unwrap();
        assert!(
            clone.join(".kept").exists(),
            "the clone was discarded and re-cloned"
        );
    }

    /// A relative local path names a directory relative to where hpm runs,
    /// as before; it is recorded as that absolute path.
    #[test]
    fn a_relative_local_path_is_taken_from_the_working_directory() {
        let remote = super::GitRemote::parse("shared/registry.git").unwrap();
        assert_eq!(
            Path::new(remote.as_str()),
            std::env::current_dir().unwrap().join("shared/registry.git")
        );
    }

    /// A clone whose origin is not the configured remote is never read, even
    /// before any refresh: it is cloned again from the right remote.
    #[tokio::test]
    async fn a_clone_with_another_origin_is_not_read() {
        let tmp = TempDir::new().unwrap();
        let real = bare_index(tmp.path(), "real", &["1.0.0"]);
        let planted = bare_index(tmp.path(), "planted", &["9.0.0"]);
        let cache = tmp.path().join("cache");
        let set =
            RegistrySet::from_configs(&[git_source("studio", real.to_str().unwrap())], &cache)
                .unwrap();
        // Put a clone of another index exactly where this registry's clone goes.
        let slot = cache.join(
            super::GitRemote::parse(real.to_str().unwrap())
                .unwrap()
                .cache_key(),
        );
        std::fs::create_dir_all(&cache).unwrap();
        git(
            &cache,
            &[
                "clone",
                "-q",
                planted.to_str().unwrap(),
                slot.to_str().unwrap(),
            ],
        );
        let entry = set.resolve_in("acme/tools", "*", None).await.unwrap();
        assert_eq!(entry.version, "1.0.0");
    }

    /// A `file://` URL to the same kind of index resolves too.
    #[tokio::test]
    async fn a_file_url_git_registry_resolves() {
        let tmp = TempDir::new().unwrap();
        let bare = bare_index(tmp.path(), "nas", &["2.0.0"]);
        let url = format!(
            "file:///{}",
            bare.to_str()
                .unwrap()
                .replace('\\', "/")
                .trim_start_matches('/')
        );
        let set = RegistrySet::from_configs(&[git_source("nas", &url)], &tmp.path().join("cache"))
            .unwrap();
        let entry = set.resolve_in("acme/tools", "^2", None).await.unwrap();
        assert_eq!(entry.version, "2.0.0");
    }

    /// A project's registry list must not be able to hand `git` an option in
    /// place of a URL: the option-looking URL is refused before any spawn,
    /// even when the name points the clone at an existing repository.
    #[cfg(unix)]
    #[tokio::test]
    async fn an_option_shaped_url_never_reaches_git() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = TempDir::new().unwrap();
        let bare = bare_index(tmp.path(), "a", &["1.0.0"]);
        let marker = tmp.path().join("marker");
        let hook = tmp.path().join("hook.sh");
        std::fs::write(
            &hook,
            format!(
                "#!/bin/sh\ntouch '{}'\nexec git-upload-pack \"$@\"\n",
                marker.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
        let cache = tmp.path().join("cache");
        let sources = [
            git_source("a", bare.to_str().unwrap()),
            git_source("a/.git", &format!("--upload-pack={}", hook.display())),
        ];
        // Either the set refuses to build, or no lookup through it spawns
        // the option.
        if let Ok(set) = RegistrySet::from_configs(&sources, &cache) {
            let _ = set.resolve_in("acme/tools", "^1", Some("a")).await;
            let _ = set.resolve_in("acme/tools", "^1", Some("a/.git")).await;
        }
        assert!(
            !marker.exists(),
            "git ran an option taken from a registry url"
        );
    }

    /// Option-shaped and transport-helper URLs are refused for git registries.
    #[test]
    fn option_and_helper_urls_are_refused() {
        let tmp = TempDir::new().unwrap();
        for url in [
            "--upload-pack=touch x",
            "-u touch x",
            "--template=/tmp/t",
            "-cprotocol.allow=always",
            "ext::sh -c touch% x",
            "fd::17",
            "  --upload-pack=x",
            "git://git.example.invalid/registry.git",
            "http://git.example.invalid/registry.git",
            "",
            "https://example.invalid/a\nb",
            "ssh://-oProxyCommand=touch%20x/r.git",
            "ssh://git@-oProxyCommand=x/r.git",
            "git@-oProxyCommand=x:r.git",
        ] {
            assert!(
                RegistrySet::from_configs(&[git_source("studio", url)], tmp.path()).is_err(),
                "accepted {url:?}"
            );
        }
    }

    /// A registry's display name never chooses where its index is cloned.
    #[tokio::test]
    async fn a_registry_name_never_places_the_clone() {
        let tmp = TempDir::new().unwrap();
        let bare = bare_index(tmp.path(), "src", &["1.0.0"]);
        let cache = tmp.path().join("cache");
        let absolute = tmp.path().join("absolute-target");
        for name in [
            "../escaped".to_string(),
            "..".to_string(),
            "x/../../escaped2".to_string(),
            absolute.to_str().unwrap().to_string(),
        ] {
            let set =
                RegistrySet::from_configs(&[git_source(&name, bare.to_str().unwrap())], &cache)
                    .unwrap();
            let _ = set.resolve_in("acme/tools", "^1", None).await;
        }
        assert!(!tmp.path().join("escaped").exists());
        assert!(!tmp.path().join("escaped2").exists());
        assert!(!absolute.exists());
        assert!(!tmp.path().join("config.json").exists());
        for entry in std::fs::read_dir(tmp.path()).unwrap() {
            let name = entry.unwrap().file_name();
            let name = name.to_string_lossy();
            assert!(
                ["cache", "src-work", "src.git"].contains(&name.as_ref()),
                "unexpected entry {name}"
            );
        }
    }

    /// Two registries that share a display name but not a URL never read
    /// each other's index: a project cannot pre-seed the user's registry.
    #[tokio::test]
    async fn same_named_registries_keep_separate_indexes() {
        let tmp = TempDir::new().unwrap();
        let attacker = bare_index(tmp.path(), "attacker", &["9.0.0"]);
        let real = bare_index(tmp.path(), "real", &["1.0.0"]);
        let cache = tmp.path().join("cache");

        let project =
            RegistrySet::from_configs(&[git_source("studio", attacker.to_str().unwrap())], &cache)
                .unwrap();
        project.resolve_in("acme/tools", "*", None).await.unwrap();

        let user =
            RegistrySet::from_configs(&[git_source("studio", real.to_str().unwrap())], &cache)
                .unwrap();
        let entry = user.resolve_in("acme/tools", "*", None).await.unwrap();
        assert_eq!(entry.version, "1.0.0");
    }

    /// Every git spawn carries the transport allowlist and drops inherited
    /// repository-location variables.
    #[test]
    fn git_commands_carry_the_fixed_policy() {
        let tmp = TempDir::new().unwrap();
        let cmd = super::git_command(tmp.path());
        let envs: Vec<_> = cmd.get_envs().collect();
        let value = |k: &str| {
            envs.iter()
                .find(|(key, _)| *key == std::ffi::OsStr::new(k))
                .map(|(_, v)| *v)
        };
        assert_eq!(
            value("GIT_ALLOW_PROTOCOL"),
            Some(Some(std::ffi::OsStr::new("https:ssh:file")))
        );
        assert_eq!(
            value("GIT_TERMINAL_PROMPT"),
            Some(Some(std::ffi::OsStr::new("0")))
        );
        for removed in [
            "GIT_DIR",
            "GIT_WORK_TREE",
            "GIT_CONFIG_PARAMETERS",
            "GIT_CONFIG_COUNT",
        ] {
            assert_eq!(value(removed), Some(None), "{removed} not removed");
        }
        assert_eq!(cmd.get_current_dir(), Some(tmp.path()));
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

    mod properties {
        use super::super::GitRegistry;
        use proptest::prelude::*;

        /// URLs assembled from the parts that matter to git: schemes it
        /// knows and helper syntax, user info, option-shaped hosts and
        /// paths; plus arbitrary text.
        fn hostile_url() -> impl Strategy<Value = String> {
            let scheme = prop::sample::select(vec![
                "https://",
                "HTTPS://",
                "ssh://",
                "file://",
                "git://",
                "http://",
                "ext::",
                "fd::",
                "",
                "-",
                "--upload-pack=",
            ]);
            let user = prop::sample::select(vec!["", "git@", "-oProxyCommand=x@", "u@-"]);
            let host = prop::sample::select(vec![
                "git.example.invalid",
                "-oProxyCommand=x",
                "h",
                "",
                "[::1]",
            ]);
            let sep = prop::sample::select(vec!["/", ":", "", ":22/"]);
            let path = prop::sample::select(vec![
                "org/repo.git",
                "",
                "-u x",
                "a::b",
                "r\nx",
                "./r",
                "..\\r",
            ]);
            prop_oneof![
                4 => (scheme, user, host, sep, path)
                    .prop_map(|(a, b, c, d, e)| format!("{a}{b}{c}{d}{e}")),
                1 => "[ -~\t\n]{0,24}",
            ]
        }

        /// The host git hands to ssh, or None when the URL names no host.
        fn ssh_host(url: &str) -> Option<&str> {
            let authority = match url.split_once("://") {
                Some((_, rest)) => rest.split('/').next().unwrap_or(rest),
                None => {
                    let (head, _) = url.split_once(':')?;
                    if head.contains(['/', '\\']) || head.len() <= 1 {
                        return None;
                    }
                    head
                }
            };
            Some(
                authority
                    .rsplit_once('@')
                    .map_or(authority, |(_, host)| host),
            )
        }

        proptest! {
            #![proptest_config(ProptestConfig::with_cases(512))]

            /// An accepted remote can never reach git as an option, a remote
            /// helper, a plaintext transport or an option-shaped ssh host,
            /// and its clone is always one digest-named child of the root.
            #[test]
            fn accepted_remotes_are_plain_remotes(url in hostile_url()) {
                let root = std::path::Path::new("/cache/registry");
                let Ok(reg) = GitRegistry::new("r", &url, root) else {
                    return Ok(());
                };
                let remote = reg.remote.as_str();
                prop_assert!(!remote.starts_with('-'), "{:?}", remote);
                prop_assert!(!remote.chars().any(char::is_control), "{:?}", remote);
                match remote.split_once("://") {
                    Some((scheme, _)) => prop_assert!(
                        ["https", "ssh", "file"].contains(&scheme.to_ascii_lowercase().as_str()),
                        "{:?}", remote
                    ),
                    None => prop_assert!(
                        !remote.split(['/', '\\']).next().unwrap_or("").contains("::"),
                        "{:?}", remote
                    ),
                }
                if let Some(host) = ssh_host(remote) {
                    prop_assert!(!host.starts_with('-'), "{:?} names host {:?}", remote, host);
                }
                prop_assert_eq!(reg.cache_dir.parent(), Some(root));
                let leaf = reg.cache_dir.file_name().unwrap().to_str().unwrap();
                prop_assert!(leaf.len() == 32 && leaf.bytes().all(|b| b.is_ascii_hexdigit()));
            }

            /// Remotes registries are really served from are accepted, and
            /// two remotes share a clone only when they are the same remote.
            #[test]
            fn real_remotes_are_accepted_and_kept_apart(
                host in "[a-z]{1,8}(\\.[a-z]{2,5}){1,2}",
                org in "[A-Za-z0-9_.-]{1,8}",
                repo in "[A-Za-z0-9_-]{1,8}",
                form in 0usize..5,
                other_repo in "[A-Za-z0-9_-]{1,8}",
            ) {
                let build = |repo: &str| match form {
                    0 => format!("https://{host}/{org}/{repo}.git"),
                    1 => format!("ssh://git@{host}:22/{org}/{repo}.git"),
                    2 => format!("git@{host}:{org}/{repo}.git"),
                    3 => format!("file:///srv/{org}/{repo}.git"),
                    _ => format!("/srv/git/{org}/{repo}"),
                };
                let root = std::path::Path::new("/cache/registry");
                let a = GitRegistry::new("a", &build(&repo), root);
                prop_assert!(a.is_ok(), "{:?} refused", build(&repo));
                let b = GitRegistry::new("b", &build(&other_repo), root).unwrap();
                prop_assert_eq!(a.unwrap().cache_dir == b.cache_dir, repo == other_repo);
            }

            /// Whatever a package name holds, its index file is a
            /// `<creator>/<slug>.json` inside the clone.
            #[test]
            fn index_paths_stay_inside_the_clone(name in "[a-zA-Z0-9./\\\\:_ -]{0,16}") {
                let reg = GitRegistry::new("r", "https://git.example.invalid/i.git", std::path::Path::new("/c")).unwrap();
                if let Ok(path) = reg.index_path(&name) {
                    let rest = path.strip_prefix(&reg.cache_dir).unwrap();
                    let parts: Vec<_> = rest.components().collect();
                    prop_assert_eq!(parts.len(), 2, "{:?}", path);
                    prop_assert!(
                        parts.iter().all(|c| matches!(c, std::path::Component::Normal(_))),
                        "{:?}", path
                    );
                }
            }
        }
    }
}
