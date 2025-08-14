//! Bundled UV binary management with complete isolation

use anyhow::{Context, Result};
use std::env;
use std::path::{Path, PathBuf};
use tokio::fs;
use tracing::{debug, info, warn};

/// Get the HPM directory
fn get_hpm_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".hpm")
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
    let uv_path = hpm_dir.join("tools").join("uv");

    // Add platform-specific extension on Windows
    #[cfg(target_os = "windows")]
    let uv_path = uv_path.with_extension("exe");

    if !uv_path.exists() {
        info!("Extracting bundled UV binary to {:?}", uv_path);
        extract_uv_binary(&uv_path)
            .await
            .context("Failed to extract UV binary")?;
        setup_uv_isolation(&hpm_dir)
            .await
            .context("Failed to setup UV isolation")?;
    }

    // Always setup environment variables
    setup_uv_environment();

    Ok(uv_path)
}

/// Extract the bundled UV binary for the current platform
async fn extract_uv_binary(target_path: &Path) -> Result<()> {
    // Create parent directory
    if let Some(parent) = target_path.parent() {
        fs::create_dir_all(parent).await?;
    }

    // Get the appropriate binary for current platform
    let binary_data = get_platform_binary()
        .ok_or_else(|| anyhow::anyhow!("UV binary not available for current platform"))?;

    // Write binary to target path
    fs::write(target_path, binary_data).await?;

    // Set executable permissions on Unix-like systems
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(target_path).await?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(target_path, perms).await?;
    }

    info!("UV binary extracted to {:?}", target_path);
    Ok(())
}

/// Get the UV binary for the current platform
fn get_platform_binary() -> Option<&'static [u8]> {
    // TODO: Actually embed UV binaries here
    // For now, we'll use placeholder logic
    warn!("UV binary embedding not yet implemented - using placeholder");

    #[cfg(target_os = "windows")]
    {
        // TODO: Include actual UV binary for Windows
        // static UV_BINARY_WINDOWS: &[u8] = include_bytes!("../resources/uv-x86_64-pc-windows-msvc.exe");
        // Some(UV_BINARY_WINDOWS)
        None
    }

    #[cfg(target_os = "macos")]
    {
        // TODO: Include actual UV binary for macOS
        // static UV_BINARY_MACOS: &[u8] = include_bytes!("../resources/uv-x86_64-apple-darwin");
        // Some(UV_BINARY_MACOS)
        None
    }

    #[cfg(target_os = "linux")]
    {
        // TODO: Include actual UV binary for Linux
        // static UV_BINARY_LINUX: &[u8] = include_bytes!("../resources/uv-x86_64-unknown-linux-gnu");
        // Some(UV_BINARY_LINUX)
        None
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        None
    }
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
    let uv_config = format!(
        r#"[cache]
dir = "{cache_dir}"

[global]
# Prevent UV from interfering with system installations
no-cache-dir = false
no-python-downloads = false
system-python = false

[install]
# Ensure packages are installed in isolated environments only
user = false
break-system-packages = false
"#,
        cache_dir = cache_dir.to_string_lossy()
    );

    fs::write(config_dir.join("uv.toml"), uv_config).await?;

    info!("UV isolation configured");
    Ok(())
}

/// Check if UV binary is available (either bundled or system)
pub async fn check_uv_availability() -> Result<PathBuf> {
    // First try to use our bundled version
    match ensure_uv_binary().await {
        Ok(path) => Ok(path),
        Err(e) => {
            warn!("Failed to setup bundled UV: {}", e);

            // For development/testing, try to find system UV as fallback
            // TODO: Remove this fallback in production
            if let Ok(system_uv) = which::which("uv") {
                warn!("Using system UV as fallback: {:?}", system_uv);
                setup_uv_environment(); // Still setup isolation
                Ok(system_uv)
            } else {
                Err(anyhow::anyhow!(
                    "Neither bundled nor system UV available. UV binary support not yet implemented."
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
}
