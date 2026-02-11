//! UV binary management with download-on-first-use
//!
//! This module manages the UV binary used for Python dependency resolution.
//! UV is downloaded on first use and cached for subsequent operations.

use anyhow::{Context, Result};
use std::env;
use std::path::{Path, PathBuf};
use tokio::fs;
use tracing::{debug, info, warn};

/// UV version to download
const UV_VERSION: &str = "0.5.9";

/// Get the HPM directory
fn get_hpm_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".hpm")
}

/// Get platform-specific UV download URL
fn get_uv_download_url() -> Option<&'static str> {
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    {
        Some(concat!(
            "https://github.com/astral-sh/uv/releases/download/",
            "0.5.9",
            "/uv-x86_64-pc-windows-msvc.zip"
        ))
    }

    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    {
        Some(concat!(
            "https://github.com/astral-sh/uv/releases/download/",
            "0.5.9",
            "/uv-x86_64-apple-darwin.tar.gz"
        ))
    }

    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        Some(concat!(
            "https://github.com/astral-sh/uv/releases/download/",
            "0.5.9",
            "/uv-aarch64-apple-darwin.tar.gz"
        ))
    }

    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        Some(concat!(
            "https://github.com/astral-sh/uv/releases/download/",
            "0.5.9",
            "/uv-x86_64-unknown-linux-gnu.tar.gz"
        ))
    }

    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    {
        Some(concat!(
            "https://github.com/astral-sh/uv/releases/download/",
            "0.5.9",
            "/uv-aarch64-unknown-linux-gnu.tar.gz"
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

/// Get the expected UV binary name for the current platform
fn get_uv_binary_name() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        "uv.exe"
    }

    #[cfg(not(target_os = "windows"))]
    {
        "uv"
    }
}

/// Setup UV environment variables for complete isolation
pub fn setup_uv_environment() {
    let hpm_dir = get_hpm_dir();

    env::set_var("UV_CACHE_DIR", hpm_dir.join("uv-cache"));
    env::set_var("UV_CONFIG_FILE", hpm_dir.join("uv-config/uv.toml"));
    env::set_var("UV_NO_SYNC", "1");
    env::set_var("UV_SYSTEM_PYTHON", "0");

    debug!("UV environment configured for isolation");
}

/// Ensure UV binary is available and properly configured
pub async fn ensure_uv_binary() -> Result<PathBuf> {
    let hpm_dir = get_hpm_dir();
    let tools_dir = hpm_dir.join("tools");
    let uv_path = tools_dir.join(get_uv_binary_name());

    // Check if UV is already downloaded
    if uv_path.exists() {
        debug!("UV binary already exists at {:?}", uv_path);
        // Ensure isolation config exists (may have been deleted or never created)
        setup_uv_isolation(&hpm_dir).await?;
        setup_uv_environment();
        return Ok(uv_path);
    }

    // Download UV
    info!("Downloading UV {} for the first time...", UV_VERSION);
    download_uv(&uv_path).await?;

    // Setup isolation
    setup_uv_isolation(&hpm_dir).await?;

    // Setup environment variables
    setup_uv_environment();

    Ok(uv_path)
}

/// Download UV binary from GitHub releases
async fn download_uv(target_path: &Path) -> Result<()> {
    let download_url = get_uv_download_url()
        .ok_or_else(|| anyhow::anyhow!("UV is not available for this platform"))?;

    info!("Downloading UV from {}", download_url);

    // Create parent directory
    if let Some(parent) = target_path.parent() {
        fs::create_dir_all(parent).await?;
    }

    // Download the archive
    let client = reqwest::Client::builder().user_agent("hpm").build()?;

    let response = client
        .get(download_url)
        .send()
        .await
        .context("Failed to download UV")?;

    if !response.status().is_success() {
        return Err(anyhow::anyhow!(
            "Failed to download UV: HTTP {}",
            response.status()
        ));
    }

    let archive_data = response.bytes().await?;
    info!("Downloaded {} bytes", archive_data.len());

    // Extract the binary based on archive type
    if download_url.ends_with(".zip") {
        extract_zip(&archive_data, target_path).await?;
    } else if download_url.ends_with(".tar.gz") {
        extract_tar_gz(&archive_data, target_path).await?;
    } else {
        return Err(anyhow::anyhow!("Unknown archive format"));
    }

    // Verify the binary exists
    if !target_path.exists() {
        return Err(anyhow::anyhow!(
            "UV binary not found after extraction at {:?}",
            target_path
        ));
    }

    info!("UV binary installed at {:?}", target_path);
    Ok(())
}

/// Extract UV from a .zip archive (Windows)
async fn extract_zip(archive_data: &[u8], target_path: &Path) -> Result<()> {
    use std::io::{Cursor, Read};

    let cursor = Cursor::new(archive_data);
    let mut archive = zip::ZipArchive::new(cursor)?;

    let uv_binary_name = get_uv_binary_name();
    let target_dir = target_path.parent().unwrap();

    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let name = file.name().to_string();

        // Look for the uv binary
        if name.ends_with(uv_binary_name) || name == uv_binary_name {
            let mut contents = Vec::new();
            file.read_to_end(&mut contents)?;

            fs::create_dir_all(target_dir).await?;
            fs::write(target_path, &contents).await?;

            info!("Extracted {} to {:?}", name, target_path);
            return Ok(());
        }
    }

    Err(anyhow::anyhow!("UV binary not found in archive"))
}

/// Extract UV from a .tar.gz archive (macOS/Linux)
async fn extract_tar_gz(archive_data: &[u8], target_path: &Path) -> Result<()> {
    use flate2::read::GzDecoder;
    use std::io::Cursor;
    use tar::Archive;

    let cursor = Cursor::new(archive_data);
    let gz = GzDecoder::new(cursor);
    let mut archive = Archive::new(gz);

    let uv_binary_name = get_uv_binary_name();
    let target_dir = target_path.parent().unwrap();

    fs::create_dir_all(target_dir).await?;

    for entry in archive.entries()? {
        let mut entry = entry?;
        let path_str = entry.path()?.to_string_lossy().to_string();

        // Look for the uv binary (might be in a subdirectory like uv-x86_64-unknown-linux-gnu/uv)
        if path_str.ends_with(uv_binary_name) {
            entry.unpack(target_path)?;

            // Set executable permissions on Unix
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mut perms = std::fs::metadata(target_path)?.permissions();
                perms.set_mode(0o755);
                std::fs::set_permissions(target_path, perms)?;
            }

            info!("Extracted {} to {:?}", path_str, target_path);
            return Ok(());
        }
    }

    Err(anyhow::anyhow!("UV binary not found in archive"))
}

/// Setup UV isolation directories and configuration
async fn setup_uv_isolation(hpm_dir: &Path) -> Result<()> {
    debug!("Setting up UV isolation in {:?}", hpm_dir);

    // Create isolated UV directories
    let cache_dir = hpm_dir.join("uv-cache");
    let config_dir = hpm_dir.join("uv-config");

    fs::create_dir_all(&cache_dir).await?;
    fs::create_dir_all(&config_dir).await?;

    // Create isolated UV configuration
    // Use forward slashes for TOML compatibility (Windows backslashes cause unicode escape issues)
    let cache_dir_str = cache_dir.to_string_lossy().replace('\\', "/");
    let uv_config = format!(
        r#"# HPM UV isolation configuration
cache-dir = "{cache_dir}"
"#,
        cache_dir = cache_dir_str
    );

    fs::write(config_dir.join("uv.toml"), uv_config).await?;

    info!("UV isolation configured");
    Ok(())
}

/// Check if UV binary is available (either downloaded or system)
pub async fn check_uv_availability() -> Result<PathBuf> {
    // First try to use our downloaded version
    match ensure_uv_binary().await {
        Ok(path) => Ok(path),
        Err(e) => {
            warn!("Failed to setup downloaded UV: {}", e);

            // Try to find system UV as fallback
            if let Ok(system_uv) = which::which("uv") {
                warn!("Using system UV as fallback: {:?}", system_uv);
                setup_uv_environment(); // Still setup isolation
                Ok(system_uv)
            } else {
                Err(anyhow::anyhow!(
                    "UV is not available. Failed to download: {}. Please install UV manually (https://docs.astral.sh/uv/)",
                    e
                ))
            }
        }
    }
}

/// Run UV command with proper isolation
pub async fn run_uv_command(args: &[&str]) -> Result<std::process::Output> {
    let uv_path = check_uv_availability().await?;

    debug!("Running UV command: {:?} {:?}", uv_path, args);

    let output = tokio::process::Command::new(uv_path)
        .args(args)
        .env("UV_CACHE_DIR", get_hpm_dir().join("uv-cache"))
        .env("UV_CONFIG_FILE", get_hpm_dir().join("uv-config/uv.toml"))
        .env("UV_NO_SYNC", "1")
        .env("UV_SYSTEM_PYTHON", "0")
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        debug!(
            "UV command failed with exit code: {:?}",
            output.status.code()
        );
        debug!("UV stderr: {}", stderr);
        return Err(anyhow::anyhow!("UV command failed: {}", stderr));
    }

    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

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
    fn test_uv_environment_setup() {
        setup_uv_environment();

        // Check that environment variables are set
        assert!(env::var("UV_CACHE_DIR").is_ok());
        assert!(env::var("UV_CONFIG_FILE").is_ok());
        assert_eq!(env::var("UV_NO_SYNC").unwrap(), "1");
        assert_eq!(env::var("UV_SYSTEM_PYTHON").unwrap(), "0");
    }

    #[test]
    fn test_get_uv_download_url() {
        // This should return Some on supported platforms
        let url = get_uv_download_url();
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
    fn test_get_uv_binary_name() {
        let name = get_uv_binary_name();
        #[cfg(target_os = "windows")]
        assert_eq!(name, "uv.exe");
        #[cfg(not(target_os = "windows"))]
        assert_eq!(name, "uv");
    }
}
