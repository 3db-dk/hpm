//! Archive fetcher for downloading packages from Git hosting providers.
//!
//! This module handles downloading and extracting package archives from
//! Git hosting platforms like GitHub, GitLab, and Bitbucket.

use crate::package_source::{GitProvider, PackageSource, PackageSourceError};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use thiserror::Error;
use tracing::{debug, info, warn};

/// Errors that can occur during archive fetching.
#[derive(Debug, Error)]
pub enum FetchError {
    #[error("HTTP request failed: {0}")]
    HttpError(#[from] reqwest::Error),

    #[error("Failed to download archive: HTTP {status} - {message}")]
    DownloadFailed { status: u16, message: String },

    #[error("Failed to create cache directory: {0}")]
    CacheDirectoryError(std::io::Error),

    #[error("Failed to write archive to disk: {0}")]
    WriteError(std::io::Error),

    #[error("Failed to extract archive: {0}")]
    ExtractionError(String),

    #[error("Archive contains path traversal attempt: {0}")]
    PathTraversalDetected(String),

    #[error("Invalid package source: {0}")]
    InvalidSource(#[from] PackageSourceError),

    #[error("Path source not found: {0}")]
    PathNotFound(PathBuf),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Checksum mismatch: expected {expected}, got {actual}")]
    ChecksumMismatch { expected: String, actual: String },
}

/// Result of a successful package fetch operation.
#[derive(Debug)]
pub struct FetchResult {
    /// Path to the extracted package directory
    pub package_path: PathBuf,
    /// SHA-256 checksum of the package contents
    pub checksum: String,
    /// Whether the package was fetched from cache
    pub from_cache: bool,
}

/// Fetches and extracts packages from Git archives or local paths.
pub struct ArchiveFetcher {
    /// HTTP client for downloading archives
    http_client: reqwest::Client,
    /// Directory for caching downloaded archives
    cache_dir: PathBuf,
    /// Directory for extracted packages
    packages_dir: PathBuf,
}

impl ArchiveFetcher {
    /// Create a new archive fetcher.
    ///
    /// # Arguments
    /// * `cache_dir` - Directory for caching downloaded archives
    /// * `packages_dir` - Directory for extracted packages
    pub fn new(cache_dir: PathBuf, packages_dir: PathBuf) -> Result<Self, FetchError> {
        // Ensure directories exist
        std::fs::create_dir_all(&cache_dir)
            .map_err(FetchError::CacheDirectoryError)?;
        std::fs::create_dir_all(&packages_dir)
            .map_err(FetchError::CacheDirectoryError)?;

        let http_client = reqwest::Client::builder()
            .user_agent("hpm/0.1.0")
            .timeout(std::time::Duration::from_secs(300)) // 5 min timeout for large packages
            .build()?;

        Ok(Self {
            http_client,
            cache_dir,
            packages_dir,
        })
    }

    /// Fetch a package from the given source.
    ///
    /// For Git sources, downloads and extracts the archive.
    /// For path sources, validates the path exists.
    ///
    /// # Arguments
    /// * `source` - The package source
    /// * `package_name` - Name of the package (for directory naming)
    pub async fn fetch(
        &self,
        source: &PackageSource,
        package_name: &str,
    ) -> Result<FetchResult, FetchError> {
        match source {
            PackageSource::Git { url, commit } => {
                self.fetch_git_archive(url, commit, package_name).await
            }
            PackageSource::Path { path } => {
                self.fetch_from_path(path, package_name).await
            }
        }
    }

    /// Fetch a package from a Git archive.
    async fn fetch_git_archive(
        &self,
        url: &str,
        commit: &str,
        package_name: &str,
    ) -> Result<FetchResult, FetchError> {
        let cache_key = format!("{}-{}", package_name, &commit[..commit.len().min(12)]);
        let package_dir = self.packages_dir.join(&cache_key);

        // Check if already extracted
        if package_dir.exists() {
            info!("Package {} already cached at {:?}", cache_key, package_dir);
            let checksum = self.compute_directory_checksum(&package_dir)?;
            return Ok(FetchResult {
                package_path: package_dir,
                checksum,
                from_cache: true,
            });
        }

        // Determine archive URL based on provider
        let provider = GitProvider::from_url(url);
        let archive_url = provider.archive_url(url, commit)?;

        info!("Downloading package from {}", archive_url);

        // Download the archive
        let archive_path = self.download_archive(&archive_url, &cache_key).await?;

        // Extract the archive
        info!("Extracting package to {:?}", package_dir);
        self.extract_archive(&archive_path, &package_dir)?;

        // Compute checksum of extracted contents
        let checksum = self.compute_directory_checksum(&package_dir)?;

        // Clean up the archive file
        if let Err(e) = std::fs::remove_file(&archive_path) {
            warn!("Failed to clean up archive file: {}", e);
        }

        Ok(FetchResult {
            package_path: package_dir,
            checksum,
            from_cache: false,
        })
    }

    /// Fetch a package from a local path.
    async fn fetch_from_path(
        &self,
        path: &Path,
        _package_name: &str,
    ) -> Result<FetchResult, FetchError> {
        if !path.exists() {
            return Err(FetchError::PathNotFound(path.to_path_buf()));
        }

        // For path dependencies, we use the path directly without copying
        let checksum = self.compute_directory_checksum(path)?;

        Ok(FetchResult {
            package_path: path.to_path_buf(),
            checksum,
            from_cache: true, // Path dependencies are always "cached"
        })
    }

    /// Download an archive from a URL.
    async fn download_archive(
        &self,
        url: &str,
        cache_key: &str,
    ) -> Result<PathBuf, FetchError> {
        let archive_path = self.cache_dir.join(format!("{}.zip", cache_key));

        // Check if already downloaded
        if archive_path.exists() {
            debug!("Archive already cached at {:?}", archive_path);
            return Ok(archive_path);
        }

        let response = self.http_client.get(url).send().await?;

        if !response.status().is_success() {
            return Err(FetchError::DownloadFailed {
                status: response.status().as_u16(),
                message: format!("Failed to download from {}", url),
            });
        }

        // Stream the response to a file
        let bytes = response.bytes().await?;

        let mut file = std::fs::File::create(&archive_path)
            .map_err(FetchError::WriteError)?;
        file.write_all(&bytes)
            .map_err(FetchError::WriteError)?;

        info!("Downloaded {} bytes to {:?}", bytes.len(), archive_path);

        Ok(archive_path)
    }

    /// Extract a zip archive to the target directory.
    fn extract_archive(
        &self,
        archive_path: &Path,
        target_dir: &Path,
    ) -> Result<(), FetchError> {
        let file = std::fs::File::open(archive_path)?;
        let mut archive = zip::ZipArchive::new(file)
            .map_err(|e| FetchError::ExtractionError(e.to_string()))?;

        // Git archives typically have a single root directory
        // e.g., "repo-abc123/" for GitHub archives
        // We need to strip this prefix when extracting

        // First, find the common prefix (if any)
        let common_prefix = self.find_archive_prefix(&archive)?;

        // Create target directory
        std::fs::create_dir_all(target_dir)?;

        for i in 0..archive.len() {
            let mut file = archive.by_index(i)
                .map_err(|e| FetchError::ExtractionError(e.to_string()))?;

            let raw_path = match file.enclosed_name() {
                Some(p) => p.to_path_buf(),
                None => {
                    warn!("Skipping file with invalid path in archive");
                    continue;
                }
            };

            // Strip the common prefix
            let relative_path = if let Some(ref prefix) = common_prefix {
                match raw_path.strip_prefix(prefix) {
                    Ok(p) => p.to_path_buf(),
                    Err(_) => raw_path,
                }
            } else {
                raw_path
            };

            // Skip empty paths (the root directory itself after stripping prefix)
            if relative_path.as_os_str().is_empty() {
                continue;
            }

            // Security check: ensure no path traversal
            self.validate_path_safety(&relative_path)?;

            let target_path = target_dir.join(&relative_path);

            if file.is_dir() {
                std::fs::create_dir_all(&target_path)?;
            } else {
                // Ensure parent directory exists
                if let Some(parent) = target_path.parent() {
                    std::fs::create_dir_all(parent)?;
                }

                let mut outfile = std::fs::File::create(&target_path)?;
                std::io::copy(&mut file, &mut outfile)?;

                // Set permissions on Unix
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    if let Some(mode) = file.unix_mode() {
                        std::fs::set_permissions(&target_path, std::fs::Permissions::from_mode(mode))?;
                    }
                }
            }
        }

        Ok(())
    }

    /// Find the common prefix in a zip archive (e.g., "repo-abc123/").
    fn find_archive_prefix(&self, archive: &zip::ZipArchive<std::fs::File>) -> Result<Option<PathBuf>, FetchError> {
        if archive.is_empty() {
            return Ok(None);
        }

        // Get the first entry
        let first_name = archive.name_for_index(0)
            .ok_or_else(|| FetchError::ExtractionError("Empty archive".to_string()))?;

        // Check if it looks like a root directory (ends with /)
        let first_path = PathBuf::from(first_name);
        if let Some(first_component) = first_path.components().next() {
            let prefix = PathBuf::from(first_component.as_os_str());

            // Verify all entries start with this prefix
            for i in 0..archive.len() {
                let name = archive.name_for_index(i)
                    .ok_or_else(|| FetchError::ExtractionError("Invalid archive entry".to_string()))?;
                if !name.starts_with(prefix.to_str().unwrap_or("")) {
                    return Ok(None);
                }
            }

            return Ok(Some(prefix));
        }

        Ok(None)
    }

    /// Validate that a path doesn't contain traversal attempts.
    fn validate_path_safety(&self, path: &Path) -> Result<(), FetchError> {
        for component in path.components() {
            match component {
                std::path::Component::ParentDir => {
                    return Err(FetchError::PathTraversalDetected(
                        path.display().to_string()
                    ));
                }
                std::path::Component::Normal(s) => {
                    let s_str = s.to_string_lossy();
                    // Also check for Windows-style parent refs
                    if s_str == ".." {
                        return Err(FetchError::PathTraversalDetected(
                            path.display().to_string()
                        ));
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Compute a SHA-256 checksum of a directory's contents.
    fn compute_directory_checksum(&self, dir: &Path) -> Result<String, FetchError> {
        let mut hasher = Sha256::new();

        // Walk directory in sorted order for deterministic checksums
        let mut entries: Vec<_> = walkdir::WalkDir::new(dir)
            .sort_by_file_name()
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
            .collect();

        entries.sort_by(|a, b| a.path().cmp(b.path()));

        for entry in entries {
            // Include relative path in hash
            let relative_path = entry.path().strip_prefix(dir).unwrap_or(entry.path());
            hasher.update(relative_path.to_string_lossy().as_bytes());

            // Include file contents in hash
            let mut file = std::fs::File::open(entry.path())?;
            let mut buffer = [0u8; 8192];
            loop {
                let bytes_read = file.read(&mut buffer)?;
                if bytes_read == 0 {
                    break;
                }
                hasher.update(&buffer[..bytes_read]);
            }
        }

        let result = hasher.finalize();
        Ok(format!("{:x}", result))
    }

    /// Check if a package is already cached.
    pub fn is_cached(&self, source: &PackageSource, package_name: &str) -> bool {
        match source {
            PackageSource::Git { commit, .. } => {
                let cache_key = format!("{}-{}", package_name, &commit[..commit.len().min(12)]);
                self.packages_dir.join(cache_key).exists()
            }
            PackageSource::Path { path } => path.exists(),
        }
    }

    /// Get the cache path for a package.
    pub fn cache_path(&self, source: &PackageSource, package_name: &str) -> Option<PathBuf> {
        match source {
            PackageSource::Git { commit, .. } => {
                let cache_key = format!("{}-{}", package_name, &commit[..commit.len().min(12)]);
                let path = self.packages_dir.join(cache_key);
                if path.exists() {
                    Some(path)
                } else {
                    None
                }
            }
            PackageSource::Path { path } => {
                if path.exists() {
                    Some(path.clone())
                } else {
                    None
                }
            }
        }
    }

    /// Remove a cached package.
    pub fn remove_cached(&self, source: &PackageSource, package_name: &str) -> Result<bool, FetchError> {
        if let Some(path) = self.cache_path(source, package_name) {
            if matches!(source, PackageSource::Git { .. }) {
                std::fs::remove_dir_all(&path)?;
                return Ok(true);
            }
        }
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_validate_path_safety() {
        let temp_dir = TempDir::new().unwrap();
        let fetcher = ArchiveFetcher::new(
            temp_dir.path().join("cache"),
            temp_dir.path().join("packages"),
        ).unwrap();

        // Safe paths
        assert!(fetcher.validate_path_safety(Path::new("foo/bar/baz.txt")).is_ok());
        assert!(fetcher.validate_path_safety(Path::new("src/lib.rs")).is_ok());

        // Unsafe paths
        assert!(fetcher.validate_path_safety(Path::new("../etc/passwd")).is_err());
        assert!(fetcher.validate_path_safety(Path::new("foo/../../etc/passwd")).is_err());
    }

    #[test]
    fn test_cache_key_generation() {
        let source = PackageSource::git(
            "https://github.com/owner/repo",
            "abc123def456789"
        ).unwrap();

        assert!(source.cache_key().contains("abc123def456"));
    }

    #[tokio::test]
    async fn test_fetcher_creation() {
        let temp_dir = TempDir::new().unwrap();
        let fetcher = ArchiveFetcher::new(
            temp_dir.path().join("cache"),
            temp_dir.path().join("packages"),
        );
        assert!(fetcher.is_ok());
    }

    #[tokio::test]
    async fn test_path_source_validation() {
        let temp_dir = TempDir::new().unwrap();
        let fetcher = ArchiveFetcher::new(
            temp_dir.path().join("cache"),
            temp_dir.path().join("packages"),
        ).unwrap();

        // Create a test package directory
        let pkg_dir = temp_dir.path().join("my-package");
        std::fs::create_dir_all(&pkg_dir).unwrap();
        std::fs::write(pkg_dir.join("hpm.toml"), "[package]\nname = \"test\"").unwrap();

        let source = PackageSource::path(&pkg_dir);
        let result = fetcher.fetch(&source, "my-package").await;
        assert!(result.is_ok());

        let fetch_result = result.unwrap();
        assert_eq!(fetch_result.package_path, pkg_dir);
        assert!(fetch_result.from_cache);
    }

    #[tokio::test]
    async fn test_nonexistent_path_source() {
        let temp_dir = TempDir::new().unwrap();
        let fetcher = ArchiveFetcher::new(
            temp_dir.path().join("cache"),
            temp_dir.path().join("packages"),
        ).unwrap();

        let source = PackageSource::path("/nonexistent/path");
        let result = fetcher.fetch(&source, "my-package").await;
        assert!(result.is_err());
    }
}
