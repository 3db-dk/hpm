//! Archive fetcher for downloading and extracting package archives.
//!
//! This module handles downloading and extracting package archives from
//! direct URLs provided by the registry.

use crate::package_source::{PackageSource, PackageSourceError};
use flate2::read::GzDecoder;
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};
use thiserror::Error;
use tokio::io::AsyncWriteExt;
use tracing::{debug, info, warn};

/// Name of the checksum cache file stored in package directories.
const CHECKSUM_CACHE_FILE: &str = ".hpm-checksum";

/// Compute the on-disk directory where `ArchiveFetcher` extracts a fetched
/// package keyed by `(name, version)`. Slashes in `name` (e.g. scoped
/// `creator/slug`) are replaced with `-` so the result is a single
/// directory name.
///
/// **Don't reinvent this formula** — `lock.rs::verify_checksums` and any
/// other consumer that needs to read a fetched package off disk must call
/// through here. The original verify_checksums hand-rolled
/// `format!("{name}@{version}")` instead and silently no-op'd because the
/// path it computed never existed.
pub fn fetcher_install_dir(packages_dir: &Path, name: &str, version: &str) -> PathBuf {
    let safe_name = name.replace('/', "-");
    packages_dir.join(format!("{}-{}", safe_name, version))
}

/// Detected archive container format.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum ArchiveFormat {
    Zip,
    TarGz,
}

/// Sniff the archive format from the file's leading bytes.
///
/// We never trust the URL or filename extension — registry URLs frequently
/// disagree with their actual payload. ZIP starts with the local-file-header
/// magic `50 4B 03 04`; gzip (and therefore tar.gz) starts with `1F 8B`.
fn detect_archive_format(archive_path: &Path) -> Result<ArchiveFormat, FetchError> {
    let mut file = std::fs::File::open(archive_path)?;
    let mut magic = [0u8; 4];
    let n = file.read(&mut magic)?;
    if n >= 4 && &magic[0..4] == b"PK\x03\x04" {
        return Ok(ArchiveFormat::Zip);
    }
    if n >= 2 && magic[0] == 0x1F && magic[1] == 0x8B {
        return Ok(ArchiveFormat::TarGz);
    }
    Err(FetchError::ExtractionError(format!(
        "Unrecognized archive format (magic bytes: {:02X?}); expected ZIP (PK..) or gzip/tar.gz (1F 8B)",
        &magic[..n]
    )))
}

/// Extract an archive to the target directory (blocking operation).
///
/// Dispatches to the ZIP or tar.gz extractor based on magic bytes — registry
/// URLs lie about formats, so the file's content is the source of truth.
/// This is a standalone function designed to be called from `spawn_blocking`.
fn extract_archive_sync(archive_path: &Path, target_dir: &Path) -> Result<(), FetchError> {
    match detect_archive_format(archive_path)? {
        ArchiveFormat::Zip => extract_zip_sync(archive_path, target_dir),
        ArchiveFormat::TarGz => extract_tar_gz_sync(archive_path, target_dir),
    }
}

/// Extract a ZIP archive to the target directory (blocking operation).
fn extract_zip_sync(archive_path: &Path, target_dir: &Path) -> Result<(), FetchError> {
    let file = std::fs::File::open(archive_path)?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| FetchError::ExtractionError(e.to_string()))?;

    // Find common prefix (Git archives typically have a single root directory)
    let common_prefix = find_archive_prefix_sync(&archive)?;

    // Create target directory
    std::fs::create_dir_all(target_dir)?;

    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
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
        validate_path_safety_sync(&relative_path)?;

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

/// Find the common prefix in a zip archive (blocking operation).
fn find_archive_prefix_sync(
    archive: &zip::ZipArchive<std::fs::File>,
) -> Result<Option<PathBuf>, FetchError> {
    if archive.is_empty() {
        return Ok(None);
    }

    let mut names = Vec::with_capacity(archive.len());
    for i in 0..archive.len() {
        let name = archive
            .name_for_index(i)
            .ok_or_else(|| FetchError::ExtractionError("Invalid archive entry".to_string()))?;
        names.push(name.to_string());
    }
    Ok(find_common_root_prefix(&names))
}

/// Find a common single-component root directory across a set of archive entry names.
///
/// Returns `Some(prefix)` only if every entry starts with the same first path
/// component — matches Git/SideFX archive convention where everything sits
/// under a single `pkg-name-version/` directory.
fn find_common_root_prefix(names: &[String]) -> Option<PathBuf> {
    let first = names.first()?;
    let first_component = PathBuf::from(first)
        .components()
        .next()?
        .as_os_str()
        .to_owned();
    let prefix = PathBuf::from(&first_component);
    let prefix_str = prefix.to_str()?;
    for name in names {
        if !name.starts_with(prefix_str) {
            return None;
        }
    }
    Some(prefix)
}

/// Extract a gzipped tar archive to the target directory (blocking operation).
fn extract_tar_gz_sync(archive_path: &Path, target_dir: &Path) -> Result<(), FetchError> {
    // Pass 1: enumerate entry names so we can detect a common root prefix.
    // The tar crate's `Archive` is single-pass, so we open it twice.
    let names = {
        let file = std::fs::File::open(archive_path)?;
        let gz = GzDecoder::new(file);
        let mut archive = tar::Archive::new(gz);
        let mut names = Vec::new();
        for entry in archive
            .entries()
            .map_err(|e| FetchError::ExtractionError(e.to_string()))?
        {
            let entry = entry.map_err(|e| FetchError::ExtractionError(e.to_string()))?;
            let path = entry
                .path()
                .map_err(|e| FetchError::ExtractionError(e.to_string()))?;
            names.push(path.to_string_lossy().into_owned());
        }
        names
    };
    let common_prefix = find_common_root_prefix(&names);

    std::fs::create_dir_all(target_dir)?;

    // Pass 2: extract.
    let file = std::fs::File::open(archive_path)?;
    let gz = GzDecoder::new(file);
    let mut archive = tar::Archive::new(gz);
    archive.set_preserve_permissions(true);
    archive.set_overwrite(true);

    for entry in archive
        .entries()
        .map_err(|e| FetchError::ExtractionError(e.to_string()))?
    {
        let mut entry = entry.map_err(|e| FetchError::ExtractionError(e.to_string()))?;
        let raw_path = entry
            .path()
            .map_err(|e| FetchError::ExtractionError(e.to_string()))?
            .into_owned();

        let relative_path = if let Some(ref prefix) = common_prefix {
            match raw_path.strip_prefix(prefix) {
                Ok(p) => p.to_path_buf(),
                Err(_) => raw_path,
            }
        } else {
            raw_path
        };

        if relative_path.as_os_str().is_empty() {
            continue;
        }

        validate_path_safety_sync(&relative_path)?;

        let target_path = target_dir.join(&relative_path);
        let entry_type = entry.header().entry_type();

        if entry_type.is_dir() {
            std::fs::create_dir_all(&target_path)?;
        } else if entry_type.is_file() {
            if let Some(parent) = target_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut outfile = std::fs::File::create(&target_path)?;
            std::io::copy(&mut entry, &mut outfile)?;

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if let Ok(mode) = entry.header().mode() {
                    std::fs::set_permissions(&target_path, std::fs::Permissions::from_mode(mode))?;
                }
            }
        } else {
            // Symlinks, hardlinks, devices, etc. — skipped intentionally to
            // keep the same security posture as the ZIP path (which doesn't
            // honor symlinks either).
            debug!(
                "Skipping non-regular tar entry: {} ({:?})",
                relative_path.display(),
                entry_type
            );
        }
    }

    Ok(())
}

/// Validate that a path doesn't contain traversal attempts.
fn validate_path_safety_sync(path: &Path) -> Result<(), FetchError> {
    // Check for backslash-based traversal (e.g. from Windows-style archive entries)
    let path_str = path.to_string_lossy();
    if path_str.contains("..\\") || path_str.contains("../") || path_str == ".." {
        return Err(FetchError::PathTraversalDetected(
            path.display().to_string(),
        ));
    }
    for component in path.components() {
        if matches!(component, std::path::Component::ParentDir) {
            return Err(FetchError::PathTraversalDetected(
                path.display().to_string(),
            ));
        }
    }
    Ok(())
}

/// Get the checksum for a directory, using cache if available (blocking operation).
fn get_directory_checksum_sync(dir: &Path) -> Result<String, FetchError> {
    // Try to read from cache first
    let checksum_file = dir.join(CHECKSUM_CACHE_FILE);
    if let Ok(cached) = std::fs::read_to_string(&checksum_file) {
        debug!("Using cached checksum for {:?}", dir);
        return Ok(cached.trim().to_string());
    }

    // Compute checksum and cache it
    let checksum = compute_directory_checksum_sync(dir)?;
    if let Err(e) = std::fs::write(&checksum_file, &checksum) {
        warn!("Failed to cache checksum: {}", e);
        // Non-fatal - we still have the checksum
    }
    Ok(checksum)
}

/// Compute a SHA-256 checksum of a directory's contents (blocking operation).
fn compute_directory_checksum_sync(dir: &Path) -> Result<String, FetchError> {
    let mut hasher = Sha256::new();

    // Walk directory in sorted order for deterministic checksums
    let mut entries: Vec<_> = walkdir::WalkDir::new(dir)
        .sort_by_file_name()
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        // Skip the checksum cache file itself
        .filter(|e| e.file_name() != CHECKSUM_CACHE_FILE)
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
    Ok(result.iter().map(|b| format!("{:02x}", b)).collect())
}

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
///
/// This struct is cheaply cloneable and can be shared across async tasks
/// for parallel package fetching.
#[derive(Clone, Debug)]
pub struct ArchiveFetcher {
    /// HTTP client for downloading archives (internally Arc-ed by reqwest)
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
        std::fs::create_dir_all(&cache_dir).map_err(FetchError::CacheDirectoryError)?;
        std::fs::create_dir_all(&packages_dir).map_err(FetchError::CacheDirectoryError)?;

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
            PackageSource::Url { url, version } => {
                self.fetch_direct_url(url, version, package_name).await
            }
            PackageSource::Path { path } => self.fetch_from_path(path, package_name).await,
        }
    }

    /// Fetch a package from a direct URL.
    async fn fetch_direct_url(
        &self,
        url: &str,
        version: &str,
        package_name: &str,
    ) -> Result<FetchResult, FetchError> {
        let package_dir = fetcher_install_dir(&self.packages_dir, package_name, version);
        // The download cache file (a sibling, with the archive bytes) is
        // keyed off the same dir name minus the parent.
        let cache_key = package_dir
            .file_name()
            .expect("fetcher_install_dir always yields a filename")
            .to_string_lossy()
            .into_owned();

        // Check if already extracted
        if tokio::fs::try_exists(&package_dir).await.unwrap_or(false) {
            info!("Package {} already cached at {:?}", cache_key, package_dir);
            let dir_for_checksum = package_dir.clone();
            let checksum =
                tokio::task::spawn_blocking(move || get_directory_checksum_sync(&dir_for_checksum))
                    .await
                    .map_err(|e| {
                        FetchError::ExtractionError(format!("Checksum task join error: {}", e))
                    })??;
            return Ok(FetchResult {
                package_path: package_dir,
                checksum,
                from_cache: true,
            });
        }

        info!("Downloading package from {}", url);

        // Download the archive directly from the URL
        let archive_path = self.download_archive(url, &cache_key).await?;

        // Extract the archive
        info!("Extracting package to {:?}", package_dir);
        let archive_path_clone = archive_path.clone();
        let package_dir_clone = package_dir.clone();
        tokio::task::spawn_blocking(move || {
            extract_archive_sync(&archive_path_clone, &package_dir_clone)
        })
        .await
        .map_err(|e| FetchError::ExtractionError(format!("Task join error: {}", e)))??;

        // Compute checksum
        let package_dir_for_checksum = package_dir.clone();
        let checksum = tokio::task::spawn_blocking(move || {
            get_directory_checksum_sync(&package_dir_for_checksum)
        })
        .await
        .map_err(|e| FetchError::ExtractionError(format!("Checksum task join error: {}", e)))??;

        // Clean up the archive file
        if let Err(e) = tokio::fs::remove_file(&archive_path).await {
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
        if !tokio::fs::try_exists(path).await.unwrap_or(false) {
            return Err(FetchError::PathNotFound(path.to_path_buf()));
        }

        // For path dependencies, we use the path directly without copying
        // Note: We don't cache checksums for path dependencies since the source
        // may change without our knowledge (it's a local directory)
        let path_for_checksum = path.to_path_buf();
        let checksum = tokio::task::spawn_blocking(move || {
            compute_directory_checksum_sync(&path_for_checksum)
        })
        .await
        .map_err(|e| FetchError::ExtractionError(format!("Checksum task join error: {}", e)))??;

        Ok(FetchResult {
            package_path: path.to_path_buf(),
            checksum,
            from_cache: true, // Path dependencies are always "cached"
        })
    }

    /// Download an archive from a URL.
    ///
    /// The cache file has no extension — format is sniffed from magic bytes
    /// at extraction time, so the on-disk name is purely an identifier.
    async fn download_archive(&self, url: &str, cache_key: &str) -> Result<PathBuf, FetchError> {
        let archive_path = self.cache_dir.join(cache_key);

        // Check if already downloaded
        if tokio::fs::try_exists(&archive_path).await.unwrap_or(false) {
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

        let mut file = tokio::fs::File::create(&archive_path)
            .await
            .map_err(FetchError::WriteError)?;
        file.write_all(&bytes)
            .await
            .map_err(FetchError::WriteError)?;

        info!("Downloaded {} bytes to {:?}", bytes.len(), archive_path);

        Ok(archive_path)
    }

    /// Check if a package is already cached.
    pub fn is_cached(&self, source: &PackageSource, package_name: &str) -> bool {
        match source {
            PackageSource::Url { version, .. } => {
                fetcher_install_dir(&self.packages_dir, package_name, version).exists()
            }
            PackageSource::Path { path } => path.exists(),
        }
    }

    /// Get the cache path for a package.
    pub fn cache_path(&self, source: &PackageSource, package_name: &str) -> Option<PathBuf> {
        match source {
            PackageSource::Url { version, .. } => {
                let path = fetcher_install_dir(&self.packages_dir, package_name, version);
                if path.exists() { Some(path) } else { None }
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
    pub fn remove_cached(
        &self,
        source: &PackageSource,
        package_name: &str,
    ) -> Result<bool, FetchError> {
        if let Some(path) = self.cache_path(source, package_name) {
            if matches!(source, PackageSource::Url { .. }) {
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

    // Security tests for path traversal - CRITICAL, never delete
    // These tests validate archive extraction safety

    #[test]
    fn test_validate_path_safety() {
        // Safe paths
        assert!(validate_path_safety_sync(Path::new("foo/bar/baz.txt")).is_ok());
        assert!(validate_path_safety_sync(Path::new("src/lib.rs")).is_ok());

        // Unsafe paths
        assert!(validate_path_safety_sync(Path::new("../etc/passwd")).is_err());
        assert!(validate_path_safety_sync(Path::new("foo/../../etc/passwd")).is_err());
    }

    #[test]
    fn test_path_traversal_parent_directory() {
        // Path traversal with parent directory reference
        assert!(validate_path_safety_sync(Path::new("../secret")).is_err());
    }

    #[test]
    fn test_path_traversal_embedded() {
        // Path traversal embedded in path
        assert!(validate_path_safety_sync(Path::new("foo/../../../etc/passwd")).is_err());
    }

    #[test]
    fn test_path_traversal_windows_style() {
        // Windows-style path separators shouldn't bypass checks
        assert!(validate_path_safety_sync(Path::new("..\\secret")).is_err());
    }

    // Async integration tests - require real I/O

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
        )
        .unwrap();

        // Create a test package directory
        let pkg_dir = temp_dir.path().join("my-package");
        std::fs::create_dir_all(&pkg_dir).unwrap();
        std::fs::write(
            pkg_dir.join("hpm.toml"),
            "[package]\npath = \"studio/test\"\nname = \"Test\"\nversion = \"1.0.0\"",
        )
        .unwrap();

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
        )
        .unwrap();

        let source = PackageSource::path("/nonexistent/path");
        let result = fetcher.fetch(&source, "my-package").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_fetch_returns_path_not_found_error() {
        let temp_dir = TempDir::new().unwrap();
        let fetcher = ArchiveFetcher::new(
            temp_dir.path().join("cache"),
            temp_dir.path().join("packages"),
        )
        .unwrap();

        let source = PackageSource::path("/definitely/does/not/exist/anywhere");
        let result = fetcher.fetch(&source, "test-pkg").await;

        match result {
            Err(FetchError::PathNotFound(path)) => {
                assert!(path.to_string_lossy().contains("definitely"));
            }
            _ => panic!("Expected PathNotFound error"),
        }
    }

    // Unit tests for PackageSource git/version validation removed -
    // covered by prop_release_asset_url_structure, prop_cache_key_uniqueness,
    // and prop_source_type_exclusive in package_source.rs

    // --- Archive format detection + tar.gz extraction ---
    //
    // Regression coverage for the `tumblehead/nodepilot` incident: the desktop
    // hardcoded ZIP extraction and silently failed with "Could not find EOCD"
    // on tar.gz uploads. These tests pin both formats end-to-end so the same
    // class of regression can't ship undetected.

    use flate2::Compression;
    use flate2::write::GzEncoder;

    /// Build an in-memory tar.gz with `entries` = (relative path, contents)
    /// nested under a single root directory `root_dir`. Mirrors the layout
    /// `pkg-name-version/...` produced by `tar -czf` in package CI.
    fn build_test_tar_gz(root_dir: &str, entries: &[(&str, &[u8])]) -> Vec<u8> {
        let buf = Vec::new();
        let gz = GzEncoder::new(buf, Compression::default());
        let mut tar = tar::Builder::new(gz);
        for (rel_path, contents) in entries {
            let full_path = format!("{}/{}", root_dir, rel_path);
            let mut header = tar::Header::new_gnu();
            header.set_size(contents.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            tar.append_data(&mut header, &full_path, *contents).unwrap();
        }
        tar.into_inner().unwrap().finish().unwrap()
    }

    #[test]
    fn test_detect_archive_format_zip() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("a.bin");
        // Minimal ZIP magic header — enough for sniffing, not enough to extract.
        std::fs::write(&path, b"PK\x03\x04rest").unwrap();
        assert_eq!(detect_archive_format(&path).unwrap(), ArchiveFormat::Zip);
    }

    #[test]
    fn test_detect_archive_format_tar_gz() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("a.bin");
        let bytes = build_test_tar_gz("pkg-1.0", &[("hpm.toml", b"[package]")]);
        std::fs::write(&path, &bytes).unwrap();
        assert_eq!(detect_archive_format(&path).unwrap(), ArchiveFormat::TarGz);
    }

    #[test]
    fn test_detect_archive_format_unknown() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("a.bin");
        std::fs::write(&path, b"not an archive").unwrap();
        match detect_archive_format(&path) {
            Err(FetchError::ExtractionError(msg)) => {
                assert!(msg.contains("Unrecognized archive format"));
            }
            other => panic!("Expected ExtractionError, got {:?}", other),
        }
    }

    #[test]
    fn test_extract_tar_gz_strips_common_prefix() {
        let temp = TempDir::new().unwrap();
        let archive_path = temp.path().join("pkg.tar.gz");
        let extract_dir = temp.path().join("out");

        let bytes = build_test_tar_gz(
            "nodepilot-1.2.0",
            &[
                ("hpm.toml", b"[package]\nname = \"nodepilot\"\n"),
                ("python/main.py", b"print('hi')\n"),
            ],
        );
        std::fs::write(&archive_path, &bytes).unwrap();

        extract_archive_sync(&archive_path, &extract_dir).unwrap();

        // Common root should be stripped — files land directly under extract_dir,
        // not under nodepilot-1.2.0/.
        assert!(extract_dir.join("hpm.toml").exists());
        assert!(extract_dir.join("python/main.py").exists());
        assert!(!extract_dir.join("nodepilot-1.2.0").exists());

        let manifest = std::fs::read_to_string(extract_dir.join("hpm.toml")).unwrap();
        assert!(manifest.contains("nodepilot"));
    }

    #[test]
    fn test_extract_dispatches_on_magic_not_extension() {
        // Even if the file is named `.zip`, a tar.gz payload should extract
        // successfully — content is the source of truth, not the filename.
        let temp = TempDir::new().unwrap();
        let archive_path = temp.path().join("misnamed.zip");
        let extract_dir = temp.path().join("out");

        let bytes = build_test_tar_gz("pkg-1.0", &[("data.txt", b"hello")]);
        std::fs::write(&archive_path, &bytes).unwrap();

        extract_archive_sync(&archive_path, &extract_dir).unwrap();
        assert_eq!(
            std::fs::read_to_string(extract_dir.join("data.txt")).unwrap(),
            "hello"
        );
    }

    #[test]
    fn test_extract_tar_gz_rejects_path_traversal() {
        let temp = TempDir::new().unwrap();
        let archive_path = temp.path().join("evil.tar.gz");
        let extract_dir = temp.path().join("out");

        // The `tar::Builder` API rejects `..` paths at write time, so we
        // forge the entry by writing the malicious name straight into the
        // header's raw `name` bytes — matches what a hostile packager could
        // produce with a custom tar implementation.
        let buf = Vec::new();
        let gz = GzEncoder::new(buf, Compression::default());
        let mut tar = tar::Builder::new(gz);
        // Use a name that survives common-prefix stripping: prefix `pkg-1.0`
        // is identified and removed, leaving `../../escaped.txt` for the
        // safety validator to catch. This is the real attack shape — a
        // single-entry `../escape` would get its `..` eaten as the prefix.
        let mut header = tar::Header::new_old();
        let evil_name = b"pkg-1.0/../../escaped.txt";
        header.as_old_mut().name[..evil_name.len()].copy_from_slice(evil_name);
        let payload = b"pwn";
        header.set_size(payload.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        tar.append(&header, &payload[..]).unwrap();
        let bytes = tar.into_inner().unwrap().finish().unwrap();
        std::fs::write(&archive_path, &bytes).unwrap();

        match extract_archive_sync(&archive_path, &extract_dir) {
            Err(FetchError::PathTraversalDetected(_)) => {}
            other => panic!("Expected PathTraversalDetected, got {:?}", other),
        }
    }

    #[test]
    fn test_find_common_root_prefix_no_shared_root() {
        let names = vec!["a/x".to_string(), "b/y".to_string()];
        assert!(find_common_root_prefix(&names).is_none());
    }

    #[test]
    fn test_find_common_root_prefix_single_root() {
        let names = vec!["pkg-1.0/a".to_string(), "pkg-1.0/b/c".to_string()];
        assert_eq!(
            find_common_root_prefix(&names),
            Some(PathBuf::from("pkg-1.0"))
        );
    }
}
