//! UV binary management with download-on-first-use
//!
//! UV is downloaded on first use and cached under `<hpm home>/tools/`. All UV
//! invocations run against HPM's isolated cache and config — we never fall
//! back to a system UV.

use super::PythonHome;
use super::error::PythonError;
use hpm_package::IoOp;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tokio::fs;
use tracing::{debug, info, warn};

/// The uv release hpm runs. Bumping it means updating [`uv_archive`]'s
/// digests from that release's `<archive>.sha256` files.
const UV_VERSION: &str = "0.13.6";

/// This platform's uv release archive name and the SHA-256 it must hash to.
/// The download is checked against this pinned digest before anything is
/// extracted, so neither a replaced release asset nor anything between hpm
/// and GitHub can change the binary every `hpm install` runs.
fn uv_archive() -> Option<(&'static str, &'static str)> {
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    {
        Some((
            "uv-x86_64-pc-windows-msvc.zip",
            "a2a8820d5eea32f08ef467ef3b7f66883a9b076d5737a2fc2fbb5f458d1bb50f",
        ))
    }
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    {
        Some((
            "uv-x86_64-apple-darwin.tar.gz",
            "63ba17fdda55ad1a230d4b37fc6017ffa10c23688c83818bd3bfdf914c652a02",
        ))
    }
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        Some((
            "uv-aarch64-apple-darwin.tar.gz",
            "60fbd5c8a1ed681845eea28d9d622fcde03c51e91a43465b64a1e17db1ed3472",
        ))
    }
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        Some((
            "uv-x86_64-unknown-linux-gnu.tar.gz",
            "f29c3e94ffabd7e6f3ba521dba169a9b7ec12100bf3257087f0f24f6123f3645",
        ))
    }
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    {
        Some((
            "uv-aarch64-unknown-linux-gnu.tar.gz",
            "229e194255c7958d86d56e8790d969a6f74736592133f0dfd0076f754c642c1d",
        ))
    }
    #[cfg(not(any(
        all(target_os = "windows", target_arch = "x86_64"),
        all(target_os = "macos", target_arch = "x86_64"),
        all(target_os = "macos", target_arch = "aarch64"),
        all(target_os = "linux", target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "aarch64"),
    )))]
    {
        None
    }
}

fn uv_download_url() -> Option<String> {
    uv_archive().map(|(name, _)| {
        format!("https://github.com/astral-sh/uv/releases/download/{UV_VERSION}/{name}")
    })
}

fn uv_binary_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "uv.exe"
    } else {
        "uv"
    }
}

/// Env vars every UV invocation must run under so that UV never touches
/// system caches or configuration.
fn uv_env(home: &PythonHome) -> [(&'static str, PathBuf); 6] {
    let hpm = home.root();
    [
        ("UV_CACHE_DIR", hpm.join("uv-cache")),
        ("UV_CONFIG_FILE", hpm.join("uv-config/uv.toml")),
        ("UV_NO_SYNC", PathBuf::from("1")),
        ("UV_SYSTEM_PYTHON", PathBuf::from("0")),
        // Keep managed CPython downloads inside HPM's tree instead of UV's
        // default per-user data dir, so cleanup/uninstall stays contained.
        ("UV_PYTHON_INSTALL_DIR", hpm.join("uv-python")),
        // Allow UV to download a managed CPython on demand. This is the
        // upstream default in 0.5.x but pinning it here means a future UV
        // upgrade or a stray system config can't disable it under us — and
        // without it, `pip compile` hard-fails on machines with no Python
        // anywhere (clean Windows installs).
        ("UV_PYTHON_DOWNLOADS", PathBuf::from("automatic")),
    ]
}

/// Ensure the bundled UV binary is downloaded and isolation configured.
///
/// Returns the path to the bundled UV binary. Errors if UV cannot be
/// downloaded or the current platform is unsupported — no system-UV fallback.
pub async fn ensure_uv_binary(home: &PythonHome) -> Result<PathBuf, PythonError> {
    let hpm_dir = home.root();
    let tools_dir = hpm_dir.join("tools");
    // Each release lives in a directory of its own, so a version bump
    // installs the new binary instead of reusing whatever is already there.
    let uv_path = tools_dir
        .join(format!("uv-{UV_VERSION}"))
        .join(uv_binary_name());

    // Older hpm versions kept an unpinned, unversioned `tools/uv`; it is
    // never run again.
    let legacy = tools_dir.join(uv_binary_name());
    if std::fs::symlink_metadata(&legacy).is_ok()
        && let Err(e) = std::fs::remove_file(&legacy)
    {
        warn!(
            "Could not remove the old unpinned uv at {:?}: {}",
            legacy, e
        );
    }

    if !uv_path.exists() {
        info!("Downloading UV {} for the first time...", UV_VERSION);
        download_uv(&uv_path).await?;
    } else {
        debug!("UV binary already exists at {:?}", uv_path);
    }

    setup_uv_isolation(hpm_dir).await?;
    Ok(uv_path)
}

async fn download_uv(target_path: &Path) -> Result<(), PythonError> {
    let download_url = uv_download_url().ok_or(PythonError::UnsupportedPlatform)?;

    info!("Downloading UV from {}", download_url);

    if let Some(parent) = target_path.parent() {
        fs::create_dir_all(parent)
            .await
            .map_err(|e| IoOp::wrap("create directory", parent, e))?;
    }

    let uv_download = |source: reqwest::Error| PythonError::UvDownload {
        url: download_url.clone(),
        source,
    };
    let client = crate::http::download_client_builder()
        .build()
        .map_err(uv_download)?;
    let response = client
        .get(&download_url)
        .send()
        .await
        .map_err(uv_download)?;

    if !response.status().is_success() {
        return Err(PythonError::UvDownloadStatus {
            url: download_url,
            status: response.status(),
        });
    }

    let archive_data = response.bytes().await.map_err(uv_download)?;
    info!("Downloaded {} bytes", archive_data.len());
    let (_, expected) = uv_archive().ok_or(PythonError::UnsupportedPlatform)?;
    verify_uv_digest(&archive_data, expected, &download_url)?;

    if download_url.ends_with(".zip") {
        extract_zip(&archive_data, target_path).await?;
    } else if download_url.ends_with(".tar.gz") {
        extract_tar_gz(&archive_data, target_path).await?;
    } else {
        return Err(PythonError::UnknownArchiveFormat { url: download_url });
    }

    if !target_path.exists() {
        return Err(PythonError::UvBinaryMissing {
            path: target_path.to_path_buf(),
        });
    }

    info!("UV binary installed at {:?}", target_path);
    Ok(())
}

/// Refuse a downloaded uv archive whose SHA-256 is not the pinned one.
fn verify_uv_digest(archive: &[u8], expected: &str, url: &str) -> Result<(), PythonError> {
    use sha2::{Digest, Sha256};
    let actual: String = Sha256::digest(archive)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    if actual == expected {
        Ok(())
    } else {
        Err(PythonError::UvDigestMismatch {
            url: url.to_string(),
            expected: expected.to_string(),
            actual,
        })
    }
}

async fn extract_zip(archive_data: &[u8], target_path: &Path) -> Result<(), PythonError> {
    let uv_binary_name = uv_binary_name();
    let target_dir = target_path.parent().unwrap().to_path_buf();
    let target_path = target_path.to_path_buf();
    let archive_data = archive_data.to_vec();

    let read_target = target_path.clone();
    let contents = tokio::task::spawn_blocking(move || -> Result<Vec<u8>, PythonError> {
        use std::io::{Cursor, Read};

        let cursor = Cursor::new(archive_data);
        let mut archive = zip::ZipArchive::new(cursor)?;

        for i in 0..archive.len() {
            let mut file = archive.by_index(i)?;
            let name = file.name()?.into_owned();

            if name.ends_with(uv_binary_name) || name == uv_binary_name {
                let mut contents = Vec::new();
                file.read_to_end(&mut contents)
                    .map_err(|e| IoOp::wrap("read UV binary from archive for", read_target, e))?;
                return Ok(contents);
            }
        }

        Err(PythonError::UvBinaryNotInArchive)
    })
    .await??;

    fs::create_dir_all(&target_dir)
        .await
        .map_err(|e| IoOp::wrap("create directory", &target_dir, e))?;
    fs::write(&target_path, &contents)
        .await
        .map_err(|e| IoOp::wrap("write", &target_path, e))?;

    info!("Extracted UV to {:?}", target_path);
    Ok(())
}

async fn extract_tar_gz(archive_data: &[u8], target_path: &Path) -> Result<(), PythonError> {
    let uv_binary_name = uv_binary_name();
    let target_dir = target_path.parent().unwrap().to_path_buf();
    let target_path = target_path.to_path_buf();
    let archive_data = archive_data.to_vec();

    fs::create_dir_all(&target_dir)
        .await
        .map_err(|e| IoOp::wrap("create directory", &target_dir, e))?;

    tokio::task::spawn_blocking(move || -> Result<(), PythonError> {
        use flate2::read::GzDecoder;
        use std::io::Cursor;
        use tar::Archive;

        let cursor = Cursor::new(archive_data);
        let gz = GzDecoder::new(cursor);
        let mut archive = Archive::new(gz);

        // Tar/gzip failures surface as io::Error without a filesystem path;
        // report them against the extraction target so the message stays
        // actionable.
        let read_err = |e: std::io::Error| IoOp::wrap("read UV tar archive for", &target_path, e);
        for entry in archive.entries().map_err(read_err)? {
            let mut entry = entry.map_err(read_err)?;
            let path_str = entry
                .path()
                .map_err(read_err)?
                .to_string_lossy()
                .to_string();

            if path_str.ends_with(uv_binary_name) {
                // Only a regular file becomes the binary: a link entry
                // would make `tools/.../uv` point wherever it says.
                if !entry.header().entry_type().is_file() {
                    return Err(PythonError::UvBinaryNotInArchive);
                }
                entry
                    .unpack(&target_path)
                    .map_err(|e| IoOp::wrap("unpack UV binary to", &target_path, e))?;

                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let mut perms = std::fs::metadata(&target_path)
                        .map_err(|e| IoOp::wrap("stat", &target_path, e))?
                        .permissions();
                    perms.set_mode(0o755);
                    std::fs::set_permissions(&target_path, perms)
                        .map_err(|e| IoOp::wrap("set permissions on", &target_path, e))?;
                }

                info!("Extracted {} to {:?}", path_str, target_path);
                return Ok(());
            }
        }

        Err(PythonError::UvBinaryNotInArchive)
    })
    .await??;

    Ok(())
}

async fn setup_uv_isolation(hpm_dir: &Path) -> Result<(), PythonError> {
    debug!("Setting up UV isolation in {:?}", hpm_dir);

    let cache_dir = hpm_dir.join("uv-cache");
    let config_dir = hpm_dir.join("uv-config");

    fs::create_dir_all(&cache_dir)
        .await
        .map_err(|e| IoOp::wrap("create directory", &cache_dir, e))?;
    fs::create_dir_all(&config_dir)
        .await
        .map_err(|e| IoOp::wrap("create directory", &config_dir, e))?;

    // Forward slashes keep the TOML cross-platform (Windows backslashes trigger unicode escapes).
    let cache_dir_str = cache_dir.to_string_lossy().replace('\\', "/");
    let uv_config = format!(
        r#"# HPM UV isolation configuration
cache-dir = "{cache_dir_str}"
"#
    );

    let config_path = config_dir.join("uv.toml");
    fs::write(&config_path, uv_config)
        .await
        .map_err(|e| IoOp::wrap("write", &config_path, e))?;

    info!("UV isolation configured");
    Ok(())
}

/// In-process cache of Python versions we've already asked UV to install.
/// `uv python install` is idempotent but does its own filesystem probing,
/// which costs ~100ms per call — pointless when we hit it twice for every
/// resolution + venv create.
fn installed_python_versions() -> &'static Mutex<HashSet<String>> {
    static CACHE: std::sync::OnceLock<Mutex<HashSet<String>>> = std::sync::OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashSet::new()))
}

/// Ensure a managed CPython matching `version` is installed under
/// `UV_PYTHON_INSTALL_DIR`.
///
/// `uv pip compile` and `uv venv --python <ver>` both require an
/// interpreter, and on a fresh Windows install with no system Python they
/// fail with `No interpreter found in virtual environments, managed
/// installations, search path, or registry`. Running `uv python install`
/// up front makes that environment work the same as a developer Mac
/// without forcing every command to go via `--python-preference managed`
/// (which would also force-download Python on Linux/macOS dev boxes that
/// already have a working interpreter).
pub async fn ensure_managed_python(home: &PythonHome, version: &str) -> Result<(), PythonError> {
    {
        let cache = installed_python_versions().lock().unwrap();
        if cache.contains(version) {
            return Ok(());
        }
    }

    debug!("Ensuring managed Python {} is available", version);
    let context = || format!("Failed to install managed Python {}", version);
    match run_uv_command(home, &["python", "install", version]).await {
        Ok(_) => {}
        Err(PythonError::UvCommand { stderr, .. }) if should_reinstall(&stderr) => {
            // `uv python install` of an existing version re-checks the copy on
            // disk, so a failure here is usually a damaged install rather than
            // a missing one. A disk that filled while uv rewrote the copy's
            // `_sysconfigdata` leaves a truncated file, and every later run
            // stops on it with "Unexpected end of file". The copy lives in
            // HPM's own tree and is re-downloadable, so replace it once.
            warn!(
                "Managed Python {} failed its install check ({}); reinstalling it",
                version,
                stderr.trim()
            );
            run_uv_command(home, &["python", "install", "--reinstall", version])
                .await
                .map_err(|e| e.uv_context(context()))?;
        }
        Err(e) => return Err(e.uv_context(context())),
    }

    installed_python_versions()
        .lock()
        .unwrap()
        .insert(version.to_string());
    Ok(())
}

/// Whether a failed `uv python install` is worth one `--reinstall`.
///
/// Everything except a full disk: reinstalling there deletes a copy that may
/// be intact only to fail writing the new one, and the error the user needs
/// is the one they already have.
fn should_reinstall(stderr: &str) -> bool {
    !crate::disk_full::is_disk_full_message(stderr)
}

/// Run UV with HPM's isolated cache and config applied per-invocation.
pub async fn run_uv_command(
    home: &PythonHome,
    args: &[&str],
) -> Result<std::process::Output, PythonError> {
    let uv_path = ensure_uv_binary(home).await?;

    debug!("Running UV command: {:?} {:?}", uv_path, args);

    let mut cmd = tokio::process::Command::new(&uv_path);
    cmd.args(args);
    for (key, value) in uv_env(home) {
        cmd.env(key, value);
    }

    // Suppress the brief console window flash that UV (a CLI tool) would
    // otherwise show when spawned from a GUI parent on Windows.
    crate::process_util::hide_console_tokio(&mut cmd);

    let output = cmd
        .output()
        .await
        .map_err(|e| IoOp::wrap("run", &uv_path, e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        debug!(
            "UV command failed with exit code: {:?}",
            output.status.code()
        );
        debug!("UV stderr: {}", stderr);
        // Callers replace `context` via `PythonError::uv_context` with the
        // operation they were attempting; the arg list is the fallback.
        return Err(PythonError::UvCommand {
            context: format!("UV command `uv {}` failed", args.join(" ")),
            stderr: stderr.into_owned(),
        });
    }

    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// A uv already sitting at `tools/uv` (any older hpm's download, or a
    /// file put there) is not the pinned release: hpm never runs it, and
    /// removes it.
    #[tokio::test]
    async fn an_unpinned_uv_in_tools_is_never_used() {
        let temp = TempDir::new().unwrap();
        let home = PythonHome::new(temp.path());
        let tools = temp.path().join("tools");
        std::fs::create_dir_all(&tools).unwrap();
        let legacy = tools.join(uv_binary_name());
        std::fs::write(&legacy, "old uv").unwrap();
        // The pinned release, already installed, so nothing is downloaded.
        let pinned = tools
            .join(format!("uv-{UV_VERSION}"))
            .join(uv_binary_name());
        std::fs::create_dir_all(pinned.parent().unwrap()).unwrap();
        std::fs::write(&pinned, "pinned uv").unwrap();

        let used = ensure_uv_binary(&home).await.unwrap();
        assert_ne!(used, legacy, "ran the unpinned uv");
        assert_eq!(used, pinned);
        assert!(!legacy.exists(), "the unpinned uv was left in place");
    }

    /// A download is accepted only when it hashes to the pinned digest.
    #[test]
    fn only_the_pinned_archive_is_accepted() {
        use sha2::{Digest, Sha256};
        let archive = b"pretend uv archive".to_vec();
        let digest: String = Sha256::digest(&archive)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        verify_uv_digest(&archive, &digest, "https://example.invalid/uv").unwrap();
        let mut tampered = archive.clone();
        tampered.push(0);
        assert!(matches!(
            verify_uv_digest(&tampered, &digest, "https://example.invalid/uv"),
            Err(PythonError::UvDigestMismatch { .. })
        ));
        if let Some((name, pinned)) = uv_archive() {
            assert!(name.starts_with("uv-"));
            assert_eq!(pinned.len(), 64);
            assert!(
                pinned
                    .bytes()
                    .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
            );
        }
    }

    /// A link entry named like the binary is refused, not unpacked.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_link_entry_is_not_unpacked_as_the_binary() {
        use flate2::{Compression, write::GzEncoder};
        let gz = GzEncoder::new(Vec::new(), Compression::default());
        let mut tar = tar::Builder::new(gz);
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Symlink);
        header.set_size(0);
        tar.append_link(&mut header, format!("uv-x/{}", uv_binary_name()), "/bin/sh")
            .unwrap();
        let bytes = tar.into_inner().unwrap().finish().unwrap();
        let temp = TempDir::new().unwrap();
        let target = temp.path().join("tools/uv-x").join(uv_binary_name());
        assert!(extract_tar_gz(&bytes, &target).await.is_err());
        assert!(std::fs::symlink_metadata(&target).is_err());
    }

    #[tokio::test]
    async fn test_uv_isolation_setup() {
        let temp_dir = TempDir::new().unwrap();
        let hpm_dir = temp_dir.path();

        setup_uv_isolation(hpm_dir).await.unwrap();

        assert!(hpm_dir.join("uv-cache").exists());
        assert!(hpm_dir.join("uv-config").exists());
        assert!(hpm_dir.join("uv-config/uv.toml").exists());
    }

    #[test]
    fn test_uv_env() {
        let env = uv_env(&PythonHome::new("/hpm"));
        let keys: Vec<_> = env.iter().map(|(k, _)| *k).collect();
        assert!(keys.contains(&"UV_CACHE_DIR"));
        assert!(keys.contains(&"UV_CONFIG_FILE"));
        assert!(keys.contains(&"UV_NO_SYNC"));
        assert!(keys.contains(&"UV_SYSTEM_PYTHON"));
        assert!(keys.contains(&"UV_PYTHON_INSTALL_DIR"));
        assert!(keys.contains(&"UV_PYTHON_DOWNLOADS"));
    }

    /// Everything uv keeps lives under the hpm home: its cache, its config
    /// and the managed CPython it downloads; venvs sit beside them. The
    /// default home is `~/.hpm`, and a configured `[storage] home_dir`
    /// moves all of it.
    #[test]
    fn uv_state_and_venvs_live_under_the_hpm_home() {
        let default = PythonHome::of_user().unwrap();
        assert_eq!(
            default.root(),
            hpm_package::user_home().unwrap().join(".hpm")
        );
        let storage = hpm_config::StorageConfig {
            home_dir: PathBuf::from("/studio/hpm"),
            ..Default::default()
        };
        for python_home in [default, PythonHome::for_storage(&storage)] {
            let home = python_home.root().to_path_buf();
            let env: std::collections::HashMap<_, _> = uv_env(&python_home).into_iter().collect();
            assert_eq!(env["UV_CACHE_DIR"], home.join("uv-cache"));
            assert_eq!(env["UV_CONFIG_FILE"], home.join("uv-config/uv.toml"));
            assert_eq!(env["UV_PYTHON_INSTALL_DIR"], home.join("uv-python"));
            assert_eq!(python_home.venvs_dir(), home.join("venvs"));
        }
    }

    #[test]
    fn test_uv_download_url() {
        let url = uv_download_url();
        #[cfg(any(
            all(target_os = "windows", target_arch = "x86_64"),
            all(target_os = "macos", target_arch = "x86_64"),
            all(target_os = "macos", target_arch = "aarch64"),
            all(target_os = "linux", target_arch = "x86_64"),
            all(target_os = "linux", target_arch = "aarch64"),
        ))]
        assert!(url.is_some());
    }

    #[test]
    fn test_uv_binary_name() {
        let name = uv_binary_name();
        #[cfg(target_os = "windows")]
        assert_eq!(name, "uv.exe");
        #[cfg(not(target_os = "windows"))]
        assert_eq!(name, "uv");
    }
}
