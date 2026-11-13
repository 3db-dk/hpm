//! Archive fetcher for downloading and extracting package archives.
//!
//! This module handles downloading and extracting package archives from
//! direct URLs provided by the registry.

use crate::package_source::{PackageSource, PackageSourceError};
use std::path::{Path, PathBuf};
use thiserror::Error;
use tokio::io::AsyncWriteExt;
use tracing::{debug, info, warn};

mod extract;

use extract::extract_archive_sync;

/// Per-process sequence for private download and staging names, so two
/// fetches of one version (in this process or another) never share a file.
static PRIVATE_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// `<key>.<pid>-<seq>`: a name only this fetch uses. The fetch-staging sweep
/// removes such entries once they are old enough to belong to no live fetch.
fn private_name(key: &str) -> String {
    let seq = PRIVATE_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    format!("{key}.{}-{seq}", std::process::id())
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
    /// The fetched tree differs from the checksum the lock recorded.
    #[error(
        "{name}@{version} does not match its locked checksum (expected {expected}, got {actual})"
    )]
    LockedChecksumMismatch {
        name: String,
        version: String,
        expected: String,
        actual: String,
    },

    /// A package name or version that cannot name a directory.
    #[error(transparent)]
    InvalidCoordinate(#[from] crate::storage::layout::InvalidCoordinate),

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
    /// Tree hash of the extracted package ([`crate::tree_hash::hash_tree`]),
    /// the value hpm.lock records — not the archive's SHA-256
    pub checksum: String,
    /// Whether the archive came from the download cache rather than the
    /// network
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
    /// Directory downloads are extracted into before they are copied into
    /// the store
    staging_dir: PathBuf,
}

impl ArchiveFetcher {
    /// The fetcher for a store: archives cached in `[storage] cache_dir`,
    /// extracted under `<home_dir>/fetch` (see [`crate::storage::StoreLayout`]).
    pub fn for_storage(config: &hpm_config::StorageConfig) -> Result<Self, FetchError> {
        let layout = crate::storage::StoreLayout::new(config);
        Self::new(
            layout.cache_dir().to_path_buf(),
            layout.fetch_dir().to_path_buf(),
        )
    }

    /// Create an archive fetcher caching archives in `cache_dir` and
    /// extracting into `staging_dir`. Production code uses
    /// [`Self::for_storage`].
    pub fn new(cache_dir: PathBuf, staging_dir: PathBuf) -> Result<Self, FetchError> {
        std::fs::create_dir_all(&cache_dir).map_err(FetchError::CacheDirectoryError)?;
        std::fs::create_dir_all(&staging_dir).map_err(FetchError::CacheDirectoryError)?;

        // 5 min timeout for large packages
        let http_client =
            crate::http::client_builder(std::time::Duration::from_secs(300)).build()?;

        Ok(Self {
            http_client,
            cache_dir,
            staging_dir,
        })
    }

    /// Fetch `source` and copy it into the store, then remove the extracted
    /// staging copy: once the store holds the package nothing reads staging
    /// again, and leaving it kept every installed version on disk twice.
    /// The one fetch-and-install sequence, for every caller.
    pub(crate) async fn fetch_into_store<E>(
        &self,
        storage: &crate::storage::StorageManager,
        source: &PackageSource,
        package_name: &str,
        locked_checksum: Option<&str>,
    ) -> Result<(crate::storage::InstalledPackage, String), E>
    where
        E: From<FetchError> + From<crate::storage::StorageError>,
    {
        let fetched = self.fetch(source, package_name).await?;
        // With a locked checksum, the extracted tree must be exactly the
        // locked one before anything reaches the store.
        let verified = match locked_checksum {
            Some(expected) => {
                let dir = fetched.package_path.clone();
                let recorded = expected.to_string();
                let actual = tokio::task::spawn_blocking(move || {
                    crate::tree_hash::hash_tree_as(&dir, &recorded)
                })
                .await
                .map_err(|e| FetchError::ExtractionError(format!("hash task failed: {e}")))?
                .map_err(|e| FetchError::ExtractionError(e.to_string()));
                match actual {
                    Ok(actual) if actual == expected => Ok(()),
                    Ok(actual) => Err(FetchError::LockedChecksumMismatch {
                        name: package_name.to_string(),
                        version: source.version.clone(),
                        expected: expected.to_string(),
                        actual,
                    }),
                    Err(e) => Err(e),
                }
            }
            None => Ok(()),
        };
        let installed = match verified {
            Ok(()) => {
                storage
                    .install_into_cas(&fetched.package_path, package_name, &source.version)
                    .await
            }
            Err(e) => {
                let _ = tokio::fs::remove_dir_all(&fetched.package_path).await;
                return Err(e.into());
            }
        };
        // The staging copy goes whether or not the install was accepted.
        if let Err(e) = tokio::fs::remove_dir_all(&fetched.package_path).await {
            warn!(
                "Could not remove the staging copy of {} at {:?}: {}",
                package_name, fetched.package_path, e
            );
        }
        Ok((installed?, fetched.checksum))
    }

    /// Fetch a package archive — download into the archive cache (keyed by
    /// name and version), verify it, extract it into a staging directory of
    /// its own, and report the extracted tree's checksum.
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
        let cache_key = crate::storage::layout::download_key(package_name, version)?;
        // Every fetch extracts into a directory of its own: an extraction is
        // never reused (a crash or a refused entry would leave a partial tree
        // that would be installed unverified), and two fetches of one version
        // never write into, or delete, each other's tree.
        let package_dir = self.staging_dir.join(private_name(&cache_key));

        info!("Downloading package from {}", url);

        let (archive_path, from_cache) = self.download_archive(url, &cache_key).await?;

        // Verify the archive bytes against the registry checksum before
        // anything is extracted. A corrupt or tampered cached archive is
        // removed so the next attempt re-downloads instead of failing on
        // the same bad bytes forever.
        if let Some(expected) = expected_sha256 {
            let expected = expected.to_string();
            let path_for_verify = archive_path.clone();
            let actual =
                tokio::task::spawn_blocking(move || crate::digest::sha256_file(&path_for_verify))
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
        let extracted = tokio::task::spawn_blocking(move || {
            extract_archive_sync(&archive_path_clone, &package_dir_clone)
        })
        .await
        .map_err(|e| FetchError::ExtractionError(format!("Task join error: {}", e)))
        .and_then(|r| r);
        if let Err(e) = extracted {
            if let Err(rm) = tokio::fs::remove_dir_all(&package_dir).await
                && rm.kind() != std::io::ErrorKind::NotFound
            {
                warn!(
                    "Could not remove failed extraction {:?}: {}",
                    package_dir, rm
                );
            }
            return Err(e);
        }

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
            from_cache,
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
    async fn download_archive(
        &self,
        url: &str,
        cache_key: &str,
    ) -> Result<(PathBuf, bool), FetchError> {
        let archive_path = self.cache_dir.join(cache_key);

        if tokio::fs::try_exists(&archive_path)
            .await
            .map_err(FetchError::CacheProbeError)?
        {
            debug!("Archive already cached at {:?}", archive_path);
            return Ok((archive_path, true));
        }

        let mut response = self.http_client.get(url).send().await?;

        if !response.status().is_success() {
            return Err(FetchError::DownloadFailed {
                status: response.status().as_u16(),
                message: format!("Failed to download from {}", url),
            });
        }

        // Stream the body chunk by chunk (an archive is never held whole in
        // memory) into a `.part` file only this download writes, and rename
        // it into place once complete. A download cut short leaves its
        // `.part` behind for `evict_download` and the staging sweep.
        let partial_path = self
            .cache_dir
            .join(format!("{}.part", private_name(cache_key)));
        let mut file = tokio::fs::File::create(&partial_path)
            .await
            .map_err(FetchError::WriteError)?;
        let mut downloaded: u64 = 0;
        while let Some(chunk) = response.chunk().await? {
            downloaded += chunk.len() as u64;
            file.write_all(&chunk)
                .await
                .map_err(FetchError::WriteError)?;
        }
        // `write_all` alone doesn't get the bytes to disk — flush drains
        // tokio's buffer into the OS and `sync_all` makes the file durable
        // before anyone can observe it under its final name.
        file.flush().await.map_err(FetchError::WriteError)?;
        file.sync_all().await.map_err(FetchError::WriteError)?;
        drop(file);

        if let Err(e) = tokio::fs::rename(&partial_path, &archive_path).await {
            let concurrent = tokio::fs::try_exists(&archive_path).await.unwrap_or(false);
            if let Err(rm) = tokio::fs::remove_file(&partial_path).await {
                warn!(
                    "Could not remove partial download {:?}: {}",
                    partial_path, rm
                );
            }
            // A concurrent fetch of the same package put the same archive in
            // place first, and the replace failed because something has it
            // open (Windows): use theirs.
            if concurrent {
                debug!("Archive appeared concurrently at {:?}", archive_path);
                return Ok((archive_path, false));
            }
            return Err(FetchError::WriteError(e));
        }

        info!("Downloaded {} bytes to {:?}", downloaded, archive_path);

        Ok((archive_path, false))
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

    /// Build a small in-memory zip archive with one file.
    fn make_test_zip() -> Vec<u8> {
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut buf);
            let options = zip::write::SimpleFileOptions::default();
            zip.start_file("pkg-1.0/hpm.toml", options).unwrap();
            use std::io::Write;
            zip.write_all(b"[package]\n").unwrap();
            zip.finish().unwrap();
        }
        buf.into_inner()
    }

    fn sha256_hex(bytes: &[u8]) -> String {
        crate::digest::sha256_bytes(bytes)
    }

    /// Installing a download leaves no copy of it in staging. Every install
    /// used to, so each version in the store was on disk twice.
    #[tokio::test]
    async fn fetch_into_store_removes_the_staging_copy() {
        let temp = TempDir::new().unwrap();
        let storage = crate::storage::StorageManager::new(hpm_config::StorageConfig {
            home_dir: temp.path().to_path_buf(),
            cache_dir: temp.path().join("cache"),
            packages_dir: temp.path().join("packages"),
            registry_cache_dir: temp.path().join("registry"),
        })
        .unwrap();
        let staging = temp.path().join("fetch");
        let fetcher = ArchiveFetcher::new(temp.path().join("cache"), staging.clone()).unwrap();

        // Pre-place the archive in the download cache so no network happens.
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            use std::io::Write;
            let mut zip = zip::ZipWriter::new(&mut buf);
            let options = zip::write::SimpleFileOptions::default();
            zip.start_file("tool-1.0.0/hpm.toml", options).unwrap();
            zip.write_all(
                b"[package]\npath = \"studio/tool\"\nname = \"tool\"\nversion = \"1.0.0\"\n",
            )
            .unwrap();
            zip.finish().unwrap();
        }
        std::fs::write(
            temp.path().join("cache").join("studio-tool-1.0.0"),
            buf.into_inner(),
        )
        .unwrap();

        let source = PackageSource::url("https://example.invalid/tool.zip", "1.0.0").unwrap();
        let (installed, _) = fetcher
            .fetch_into_store::<FetchIntoStoreTestError>(&storage, &source, "studio/tool", None)
            .await
            .unwrap();

        assert_eq!(installed.version, "1.0.0");
        assert!(installed.install_path.join("hpm.toml").exists());
        assert_eq!(
            std::fs::read_dir(&staging).unwrap().count(),
            0,
            "the staging copy must be gone once the store holds the package"
        );
    }

    /// Install `dep_name@version` from an archive pre-placed in the download
    /// cache (no network), whose own `hpm.toml` declares `declared_path` at
    /// `declared_version`. Returns the result and the temp root.
    async fn fetch_declared(
        dep_name: &str,
        version: &str,
        declared_path: &str,
        declared_version: &str,
    ) -> (
        Result<crate::storage::InstalledPackage, FetchIntoStoreTestError>,
        TempDir,
    ) {
        let temp = TempDir::new().unwrap();
        let storage = crate::storage::StorageManager::new(hpm_config::StorageConfig {
            home_dir: temp.path().to_path_buf(),
            cache_dir: temp.path().join("cache"),
            packages_dir: temp.path().join("packages"),
            registry_cache_dir: temp.path().join("registry"),
        })
        .unwrap();
        let fetcher =
            ArchiveFetcher::new(temp.path().join("cache"), temp.path().join("fetch")).unwrap();
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            use std::io::Write;
            let mut zip = zip::ZipWriter::new(&mut buf);
            let options = zip::write::SimpleFileOptions::default();
            zip.start_file("pkg/hpm.toml", options).unwrap();
            write!(
                zip,
                "[package]\npath = \"{declared_path}\"\nname = \"x\"\nversion = \"{declared_version}\"\n"
            )
            .unwrap();
            zip.finish().unwrap();
        }
        std::fs::create_dir_all(temp.path().join("cache")).unwrap();
        std::fs::write(
            temp.path()
                .join("cache")
                .join(crate::storage::layout::download_key(dep_name, version).unwrap()),
            buf.into_inner(),
        )
        .unwrap();
        let source = PackageSource::url("https://example.invalid/p.zip", version).unwrap();
        let result = fetcher
            .fetch_into_store::<FetchIntoStoreTestError>(&storage, &source, dep_name, None)
            .await
            .map(|(installed, _)| installed);
        (result, temp)
    }

    /// A package whose archive declares the identity it was requested under
    /// installs, including when the request differs only in ASCII case or
    /// names the bare slug.
    #[tokio::test]
    async fn an_archive_declaring_the_requested_identity_installs() {
        for (dep, version, declared, declared_version) in [
            ("studio/tool", "1.0.0", "studio/tool", "1.0.0"),
            ("Studio/Tool", "1.0.0", "studio/tool", "1.0.0"),
            ("tool", "2.1.0-beta.1", "studio/tool", "2.1.0-beta.1"),
        ] {
            let (result, _temp) = fetch_declared(dep, version, declared, declared_version).await;
            let installed = result.unwrap_or_else(|e| panic!("{dep}@{version}: {e:?}"));
            assert_eq!(installed.version, declared_version);
            assert_eq!(installed.manifest.package.path.as_str(), declared);
        }
    }

    /// An archive that declares a different package or version than the one
    /// requested never lands in the store: it would otherwise occupy that
    /// other package's slot for every project on the machine.
    #[tokio::test]
    async fn an_archive_claiming_another_identity_is_refused() {
        for (dep, version, declared, declared_version) in [
            ("evil/helper", "1.0.0", "acme/tools", "1.0.0"),
            ("evil/helper", "1.0.0", "evil/tools", "1.0.0"),
            ("acme/tools", "1.0.0", "acme/tools", "1.2.0"),
            ("acme/tools", "1.0.0", "acme/tools", "1.0.0-rc.1"),
            ("acme/tools", "1.0.0", "other/tools", "1.0.0"),
            ("helper", "1.0.0", "acme/tools", "1.0.0"),
        ] {
            let (result, temp) = fetch_declared(dep, version, declared, declared_version).await;
            assert!(
                result.is_err(),
                "{dep}@{version} installed an archive declaring {declared}@{declared_version}"
            );
            let packages = temp.path().join("packages");
            let slots: Vec<_> = std::fs::read_dir(&packages)
                .map(|d| d.filter_map(|e| e.ok()).map(|e| e.file_name()).collect())
                .unwrap_or_default();
            assert!(
                slots
                    .iter()
                    .all(|n| n.to_string_lossy().starts_with('.') || n.to_string_lossy() == "_dev"),
                "{dep}@{version}: store slots {slots:?}"
            );
            assert_eq!(
                std::fs::read_dir(temp.path().join("fetch"))
                    .map(|d| d.count())
                    .unwrap_or(0),
                0,
                "{dep}@{version}: staging left behind"
            );
        }
    }

    #[derive(Debug)]
    #[allow(dead_code)]
    enum FetchIntoStoreTestError {
        Fetch(FetchError),
        Storage(crate::storage::StorageError),
    }
    impl From<FetchError> for FetchIntoStoreTestError {
        fn from(e: FetchError) -> Self {
            Self::Fetch(e)
        }
    }
    impl From<crate::storage::StorageError> for FetchIntoStoreTestError {
        fn from(e: crate::storage::StorageError) -> Self {
            Self::Storage(e)
        }
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
        assert_eq!(
            std::fs::read_dir(&packages_dir).unwrap().count(),
            0,
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

        let (path, from_cache) = fetcher.download_archive(&url, "pkg-1.0.0").await.unwrap();
        assert!(!from_cache);

        assert_eq!(
            crate::digest::sha256_file(&path).unwrap(),
            sha256_hex(&bytes),
            "archive contents must be durable before download_archive returns"
        );
        let parts: Vec<_> = std::fs::read_dir(&cache_dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".part"))
            .collect();
        assert!(
            parts.is_empty(),
            "the .part file must be renamed away, not left behind: {parts:?}"
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

    /// An archive far larger than one network read arrives intact: every
    /// chunk of the streamed body reaches the file, so the checksum verifies
    /// and the large member extracts byte for byte.
    #[tokio::test]
    async fn test_fetch_streams_a_multi_chunk_archive() {
        let temp = TempDir::new().unwrap();
        let fetcher =
            ArchiveFetcher::new(temp.path().join("cache"), temp.path().join("packages")).unwrap();

        // 4 MiB of pseudo-random bytes, stored uncompressed.
        let mut state: u32 = 0x9e37_79b9;
        let payload: Vec<u8> = (0..4 * 1024 * 1024)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                state as u8
            })
            .collect();
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            use std::io::Write;
            let mut zip = zip::ZipWriter::new(&mut buf);
            let stored = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            zip.start_file("pkg-1.0/hpm.toml", stored).unwrap();
            zip.write_all(b"[package]\n").unwrap();
            zip.start_file("pkg-1.0/data.bin", stored).unwrap();
            zip.write_all(&payload).unwrap();
            zip.finish().unwrap();
        }
        let bytes = buf.into_inner();
        let digest = sha256_hex(&bytes);
        let url = serve_once(bytes).await;

        let result = fetcher
            .fetch_direct_url(&url, "1.0.0", "big", Some(&digest))
            .await
            .expect("a multi-chunk download must verify against its checksum");
        assert_eq!(
            std::fs::read(result.package_path.join("data.bin")).unwrap(),
            payload
        );
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

    /// Two installs of one version at once both succeed and leave one
    /// complete store copy with its record. They used to share one staging
    /// directory and copy into the store directory in place, so each could
    /// delete or overwrite the other's tree mid-copy.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_installs_of_one_version_both_succeed() {
        let temp = TempDir::new().unwrap();
        let config = hpm_config::StorageConfig {
            home_dir: temp.path().to_path_buf(),
            cache_dir: temp.path().join("cache"),
            packages_dir: temp.path().join("packages"),
            registry_cache_dir: temp.path().join("registry"),
        };
        let storage =
            std::sync::Arc::new(crate::storage::StorageManager::new(config.clone()).unwrap());
        let fetcher = ArchiveFetcher::for_storage(&config).unwrap();
        // The archive is already cached, so both fetches start at once.
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            use std::io::Write;
            let mut zip = zip::ZipWriter::new(&mut buf);
            let opts = zip::write::SimpleFileOptions::default();
            zip.start_file("tool-1.0.0/hpm.toml", opts).unwrap();
            zip.write_all(
                b"[package]\npath = \"studio/tool\"\nname = \"Tool\"\nversion = \"1.0.0\"\n",
            )
            .unwrap();
            for n in 0..200 {
                zip.start_file(format!("tool-1.0.0/data/{n}.bin"), opts)
                    .unwrap();
                zip.write_all(&vec![n as u8; 4096]).unwrap();
            }
            zip.finish().unwrap();
        }
        std::fs::write(
            temp.path().join("cache/studio-tool-1.0.0"),
            buf.into_inner(),
        )
        .unwrap();

        let source = PackageSource::url("https://example.invalid/tool.zip", "1.0.0").unwrap();
        let runs: Vec<_> = (0..4)
            .map(|_| {
                let (fetcher, storage, source) = (fetcher.clone(), storage.clone(), source.clone());
                tokio::spawn(async move {
                    fetcher
                        .fetch_into_store::<FetchIntoStoreTestError>(
                            &storage,
                            &source,
                            "studio/tool",
                            None,
                        )
                        .await
                        .map(|(installed, _)| installed.install_path)
                })
            })
            .collect();
        for run in runs {
            run.await
                .unwrap()
                .expect("every concurrent install succeeds");
        }

        let store = storage.get_package_path("tool", "1.0.0").unwrap();
        assert_eq!(std::fs::read_dir(store.join("data")).unwrap().count(), 200);
        match crate::install_manifest::verify(&store).unwrap() {
            crate::install_manifest::Verification::Checked(report) => {
                assert!(report.is_intact(), "{report:?}")
            }
            other => panic!("the store copy must carry its record, got {other:?}"),
        }
        assert_eq!(
            std::fs::read_dir(temp.path().join("fetch"))
                .unwrap()
                .count(),
            0
        );
        let hidden: Vec<_> = std::fs::read_dir(temp.path().join("packages"))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with('.'))
            .collect();
        assert!(hidden.is_empty(), "no store staging left: {hidden:?}");
    }

    /// An extraction that fails partway leaves nothing in staging, so the
    /// partial tree can never be installed. Any existing staging directory
    /// used to be taken as a finished extraction and copied into the store
    /// unchecked.
    #[tokio::test]
    async fn a_failed_extraction_leaves_no_staging_behind() {
        use flate2::Compression;
        use flate2::write::GzEncoder;

        let temp = TempDir::new().unwrap();
        let cache_dir = temp.path().join("cache");
        let staging = temp.path().join("fetch");
        let fetcher = ArchiveFetcher::new(cache_dir.clone(), staging.clone()).unwrap();

        // A valid entry, then one that escapes the target.
        let mut tar = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::default()));
        let mut ok = tar::Header::new_gnu();
        ok.set_size(9);
        ok.set_mode(0o644);
        ok.set_cksum();
        tar.append_data(&mut ok, "pkg-1.0/hpm.toml", &b"[package]"[..])
            .unwrap();
        let mut evil = tar::Header::new_old();
        let name = b"pkg-1.0/../../escaped.txt";
        evil.as_old_mut().name[..name.len()].copy_from_slice(name);
        evil.set_size(3);
        evil.set_mode(0o644);
        evil.set_cksum();
        tar.append(&evil, &b"pwn"[..]).unwrap();
        std::fs::write(
            cache_dir.join("pkg-1.0.0"),
            tar.into_inner().unwrap().finish().unwrap(),
        )
        .unwrap();

        for _ in 0..2 {
            let result = fetcher
                .fetch_direct_url("https://example.invalid/p.tgz", "1.0.0", "pkg", None)
                .await;
            assert!(
                matches!(result, Err(FetchError::PathTraversalDetected(_))),
                "{result:?}"
            );
            assert_eq!(
                std::fs::read_dir(&staging).unwrap().count(),
                0,
                "a failed extraction must not leave a tree to reuse"
            );
        }
    }
}
