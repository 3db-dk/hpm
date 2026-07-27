//! Archive fetcher for downloading and extracting package archives.
//!
//! This module handles downloading and extracting package archives from
//! direct URLs provided by the registry.

use crate::package_source::{PackageSource, PackageSourceError};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};
use thiserror::Error;
use tokio::io::AsyncWriteExt;
use tracing::{debug, info, warn};

mod extract;

use extract::extract_archive_sync;

/// Compute the on-disk directory where `ArchiveFetcher` extracts a fetched
/// package keyed by `(name, version)`. Slashes in `name` (e.g. scoped
/// `creator/slug`) are replaced with `-` so the result is a single
/// directory name.
///
/// This is the **staging** path — `hpm install` and friends fetch into
/// `<.hpm>/fetch/<safe_name>-<version>/`, then copy into the canonical
/// `StorageManager` CAS via `install_into_cas`. To find a package's
/// canonical install location instead, see [`cas_install_dir`].
pub fn fetcher_install_dir(packages_dir: &Path, name: &str, version: &str) -> PathBuf {
    let safe_name = name.replace('/', "-");
    packages_dir.join(format!("{}-{}", safe_name, version))
}

/// Compute the canonical `StorageManager` CAS path for a dependency
/// referenced by `name` (the dependency key, possibly scoped as
/// `creator/slug`) and `version`. The CAS keys by **bare slug**: scoped
/// names are reduced to their last `/`-segment so the layout matches
/// what `StorageManager::install_into_cas` writes.
///
/// Used by `LockFile::verify_checksums` and any consumer that needs to
/// locate an installed package off the lockfile alone.
pub fn cas_install_dir(packages_dir: &Path, name: &str, version: &str) -> PathBuf {
    let slug = name.rsplit('/').next().unwrap_or(name);
    packages_dir.join(format!("{}@{}", slug, version))
}

/// SHA-256 of a file's bytes, streamed (blocking operation). Used to verify
/// a downloaded archive against the registry entry's `cksum`.
fn compute_file_sha256_sync(path: &Path) -> Result<String, FetchError> {
    let mut hasher = Sha256::new();
    let mut file = std::fs::File::open(path)?;
    let mut buffer = [0u8; 8192];
    loop {
        let bytes_read = file.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }
    let result = hasher.finalize();
    Ok(result.iter().map(|b| format!("{:02x}", b)).collect())
}

/// Compute a SHA-256 checksum of a directory's contents (blocking operation)
/// via the shared [`crate::tree_hash::hash_tree`].
///
/// Always computed from the actual tree — never cached on disk. A stored
/// checksum can go stale without anything invalidating it, silently
/// misreporting the package hash that feeds lockfile verification.
fn compute_directory_checksum_sync(dir: &Path) -> Result<String, FetchError> {
    crate::tree_hash::hash_tree(dir).map_err(|e| FetchError::IoError(std::io::Error::other(e)))
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

    #[error("Failed to probe cache path: {0}")]
    CacheProbeError(std::io::Error),

    #[error("Failed to write archive to disk: {0}")]
    WriteError(std::io::Error),

    #[error("Failed to extract archive: {0}")]
    ExtractionError(String),

    #[error("Archive contains path traversal attempt: {0}")]
    PathTraversalDetected(String),

    #[error("Invalid package source: {0}")]
    InvalidSource(#[from] PackageSourceError),

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

        // 5 min timeout for large packages
        let http_client =
            crate::http::client_builder(std::time::Duration::from_secs(300)).build()?;

        Ok(Self {
            http_client,
            cache_dir,
            packages_dir,
        })
    }

    /// Fetch a package archive — download to the URL-keyed cache file,
    /// extract into the staging dir, and report the on-disk checksum.
    ///
    /// Path dependencies don't go through here; they're copied straight
    /// into the dev CAS via `StorageManager::install_as_dev_copy`.
    pub async fn fetch(
        &self,
        source: &PackageSource,
        package_name: &str,
    ) -> Result<FetchResult, FetchError> {
        self.fetch_direct_url(
            &source.url,
            &source.version,
            package_name,
            source.expected_sha256.as_deref(),
        )
        .await
    }

    /// Fetch a package from a direct URL, verifying the archive bytes
    /// against `expected_sha256` (when known) before extraction.
    async fn fetch_direct_url(
        &self,
        url: &str,
        version: &str,
        package_name: &str,
        expected_sha256: Option<&str>,
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
        if tokio::fs::try_exists(&package_dir)
            .await
            .map_err(FetchError::CacheProbeError)?
        {
            info!("Package {} already cached at {:?}", cache_key, package_dir);
            let dir_for_checksum = package_dir.clone();
            let checksum = tokio::task::spawn_blocking(move || {
                compute_directory_checksum_sync(&dir_for_checksum)
            })
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

        // Verify the archive bytes against the registry checksum before
        // anything is extracted. A corrupt or tampered cached archive is
        // removed so the next attempt re-downloads instead of failing on
        // the same bad bytes forever.
        if let Some(expected) = expected_sha256 {
            let expected = expected.to_string();
            let path_for_verify = archive_path.clone();
            let actual =
                tokio::task::spawn_blocking(move || compute_file_sha256_sync(&path_for_verify))
                    .await
                    .map_err(|e| {
                        FetchError::ExtractionError(format!("Checksum task join error: {}", e))
                    })??;
            if actual != expected {
                if let Err(e) = tokio::fs::remove_file(&archive_path).await {
                    warn!("Failed to remove corrupt archive {:?}: {}", archive_path, e);
                }
                return Err(FetchError::ChecksumMismatch { expected, actual });
            }
            debug!("Archive checksum verified for {}", cache_key);
        } else {
            warn!(
                "No registry checksum for {}; installing archive unverified",
                cache_key
            );
        }

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
            compute_directory_checksum_sync(&package_dir_for_checksum)
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

    /// Download an archive from a URL.
    ///
    /// The cache file has no extension — format is sniffed from magic bytes
    /// at extraction time, so the on-disk name is purely an identifier.
    ///
    /// Bytes land in a sibling `<cache_key>.part` first and are renamed into
    /// place only once they're flushed and fsync'd. Two reasons the direct
    /// write was wrong: `tokio::fs::File::write_all` only queues the bytes
    /// for the blocking pool, so the very next reader (the checksum verify in
    /// `fetch_direct_url`) could open a still-empty file and compute the
    /// SHA-256 of zero bytes; and a download interrupted partway left a
    /// truncated file that the `try_exists` probe above would happily serve
    /// as a complete cache hit forever after.
    async fn download_archive(&self, url: &str, cache_key: &str) -> Result<PathBuf, FetchError> {
        let archive_path = self.cache_dir.join(cache_key);

        // Check if already downloaded
        if tokio::fs::try_exists(&archive_path)
            .await
            .map_err(FetchError::CacheProbeError)?
        {
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

        let partial_path = self.cache_dir.join(format!("{}.part", cache_key));
        let mut file = tokio::fs::File::create(&partial_path)
            .await
            .map_err(FetchError::WriteError)?;
        file.write_all(&bytes)
            .await
            .map_err(FetchError::WriteError)?;
        // `write_all` alone doesn't get the bytes to disk — flush drains
        // tokio's buffer into the OS and `sync_all` makes the file durable
        // before anyone can observe it under its final name.
        file.flush().await.map_err(FetchError::WriteError)?;
        file.sync_all().await.map_err(FetchError::WriteError)?;
        drop(file);

        if let Err(e) = tokio::fs::rename(&partial_path, &archive_path).await {
            // A concurrent fetch of the same package may have won the race.
            // Windows' rename refuses to clobber an existing file, so treat
            // "destination is now there" as success and drop our copy.
            if tokio::fs::try_exists(&archive_path).await.unwrap_or(false) {
                debug!("Archive appeared concurrently at {:?}", archive_path);
                let _ = tokio::fs::remove_file(&partial_path).await;
                return Ok(archive_path);
            }
            let _ = tokio::fs::remove_file(&partial_path).await;
            return Err(FetchError::WriteError(e));
        }

        info!("Downloaded {} bytes to {:?}", bytes.len(), archive_path);

        Ok(archive_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

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

    // Path-source tests removed: path dependencies bypass the fetcher
    // entirely and copy straight into the dev CAS via
    // `StorageManager::install_as_dev_copy`.

    // Unit tests for PackageSource git/version validation removed -
    // covered by prop_release_asset_url_structure, prop_cache_key_uniqueness,
    // and prop_source_type_exclusive in package_source.rs

    /// Build a small in-memory zip archive with one file.
    fn make_test_zip() -> Vec<u8> {
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut buf);
            let options: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default();
            zip.start_file("pkg-1.0/hpm.toml", options).unwrap();
            use std::io::Write;
            zip.write_all(b"[package]\n").unwrap();
            zip.finish().unwrap();
        }
        buf.into_inner()
    }

    fn sha256_hex(bytes: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        hasher
            .finalize()
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect()
    }

    /// A checksum mismatch must abort before extraction and remove the
    /// corrupt cached archive so a later attempt re-downloads.
    #[tokio::test]
    async fn test_fetch_rejects_checksum_mismatch() {
        let temp = TempDir::new().unwrap();
        let cache_dir = temp.path().join("cache");
        let packages_dir = temp.path().join("packages");
        let fetcher = ArchiveFetcher::new(cache_dir.clone(), packages_dir.clone()).unwrap();

        // Pre-place the archive in the download cache so no network happens.
        let archive_path = cache_dir.join("pkg-1.0.0");
        std::fs::write(&archive_path, make_test_zip()).unwrap();

        let wrong = "0".repeat(64);
        let result = fetcher
            .fetch_direct_url(
                "https://example.invalid/pkg.zip",
                "1.0.0",
                "pkg",
                Some(&wrong),
            )
            .await;

        match result {
            Err(FetchError::ChecksumMismatch { expected, .. }) => assert_eq!(expected, wrong),
            other => panic!("Expected ChecksumMismatch, got {:?}", other),
        }
        assert!(
            !archive_path.exists(),
            "corrupt cached archive must be removed"
        );
        assert!(
            !fetcher_install_dir(&packages_dir, "pkg", "1.0.0").exists(),
            "nothing may be extracted from an unverified archive"
        );
    }

    /// Serve `body` once over HTTP on an ephemeral loopback port and return
    /// the URL. Hand-rolled rather than pulling in a mock-server dependency —
    /// the download path only needs a status line, a length, and the bytes.
    async fn serve_once(body: Vec<u8>) -> String {
        use tokio::io::AsyncWriteExt as _;

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            // Drain the request line/headers so the client isn't writing into
            // a closed pipe while we reply.
            let mut discard = [0u8; 1024];
            let _ = tokio::io::AsyncReadExt::read(&mut socket, &mut discard).await;
            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            socket.write_all(header.as_bytes()).await.unwrap();
            socket.write_all(&body).await.unwrap();
            socket.shutdown().await.unwrap();
        });
        format!("http://{}/pkg.zip", addr)
    }

    /// The bytes must be on disk by the time `download_archive` returns.
    ///
    /// Regression: `write_all` on a `tokio::fs::File` only queues the write,
    /// so the checksum verify that runs immediately afterwards could open a
    /// still-empty file and hash zero bytes — surfacing as a bogus "Checksum
    /// mismatch: ... got e3b0c442..." (the SHA-256 of the empty input) on a
    /// download that had actually succeeded.
    #[tokio::test]
    async fn test_downloaded_archive_is_readable_immediately() {
        let temp = TempDir::new().unwrap();
        let cache_dir = temp.path().join("cache");
        let packages_dir = temp.path().join("packages");
        let fetcher = ArchiveFetcher::new(cache_dir.clone(), packages_dir).unwrap();

        let bytes = make_test_zip();
        let url = serve_once(bytes.clone()).await;

        let path = fetcher.download_archive(&url, "pkg-1.0.0").await.unwrap();

        assert_eq!(
            compute_file_sha256_sync(&path).unwrap(),
            sha256_hex(&bytes),
            "archive contents must be durable before download_archive returns"
        );
        assert!(
            !cache_dir.join("pkg-1.0.0.part").exists(),
            "the staging file must be renamed away, not left behind"
        );
    }

    /// A verified download extracts end-to-end, exercising the same
    /// download → flush → checksum → extract sequence the failure hit.
    #[tokio::test]
    async fn test_fetch_downloads_and_verifies_end_to_end() {
        let temp = TempDir::new().unwrap();
        let fetcher =
            ArchiveFetcher::new(temp.path().join("cache"), temp.path().join("packages")).unwrap();

        let bytes = make_test_zip();
        let digest = sha256_hex(&bytes);
        let url = serve_once(bytes).await;

        let result = fetcher
            .fetch_direct_url(&url, "1.0.0", "pkg", Some(&digest))
            .await
            .expect("a freshly downloaded archive must verify against its checksum");
        assert!(result.package_path.join("hpm.toml").exists());
        assert!(!result.from_cache);
    }

    /// A matching checksum extracts normally.
    #[tokio::test]
    async fn test_fetch_accepts_matching_checksum() {
        let temp = TempDir::new().unwrap();
        let cache_dir = temp.path().join("cache");
        let packages_dir = temp.path().join("packages");
        let fetcher = ArchiveFetcher::new(cache_dir.clone(), packages_dir.clone()).unwrap();

        let bytes = make_test_zip();
        let digest = sha256_hex(&bytes);
        std::fs::write(cache_dir.join("pkg-1.0.0"), &bytes).unwrap();

        let result = fetcher
            .fetch_direct_url(
                "https://example.invalid/pkg.zip",
                "1.0.0",
                "pkg",
                Some(&digest),
            )
            .await
            .expect("verified archive should extract");
        assert!(result.package_path.join("hpm.toml").exists());
    }
}
