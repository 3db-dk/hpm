//! UV binary management with download-on-first-use
//!
//! UV is downloaded on first use and cached under `~/.hpm/tools/`. All UV
//! invocations run against HPM's isolated cache and config — we never fall
//! back to a system UV.

use anyhow::{Context, Result};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tokio::fs;
use tracing::{debug, info};

const UV_VERSION: &str = "0.5.9";

fn hpm_dir() -> Result<PathBuf> {
    super::hpm_root()
}

/// Platform-specific UV release archive name (filename only).
fn uv_archive_name() -> Option<&'static str> {
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    {
        Some("uv-x86_64-pc-windows-msvc.zip")
    }
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    {
        Some("uv-x86_64-apple-darwin.tar.gz")
    }
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        Some("uv-aarch64-apple-darwin.tar.gz")
    }
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        Some("uv-x86_64-unknown-linux-gnu.tar.gz")
    }
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    {
        Some("uv-aarch64-unknown-linux-gnu.tar.gz")
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
    uv_archive_name().map(|name| {
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
fn uv_env() -> Result<[(&'static str, PathBuf); 6]> {
    let hpm = hpm_dir()?;
    Ok([
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
    ])
}

/// Ensure the bundled UV binary is downloaded and isolation configured.
///
/// Returns the path to the bundled UV binary. Errors if UV cannot be
/// downloaded or the current platform is unsupported — no system-UV fallback.
pub async fn ensure_uv_binary() -> Result<PathBuf> {
    let hpm_dir = hpm_dir()?;
    let tools_dir = hpm_dir.join("tools");
    let uv_path = tools_dir.join(uv_binary_name());

    if !uv_path.exists() {
        info!("Downloading UV {} for the first time...", UV_VERSION);
        download_uv(&uv_path).await?;
    } else {
        debug!("UV binary already exists at {:?}", uv_path);
    }

    setup_uv_isolation(&hpm_dir).await?;
    Ok(uv_path)
}

async fn download_uv(target_path: &Path) -> Result<()> {
    let download_url = uv_download_url()
        .ok_or_else(|| anyhow::anyhow!("UV is not available for this platform"))?;

    info!("Downloading UV from {}", download_url);

    if let Some(parent) = target_path.parent() {
        fs::create_dir_all(parent).await?;
    }

    let client = crate::http::client_builder(std::time::Duration::from_secs(300)).build()?;
    let response = client
        .get(&download_url)
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

    if download_url.ends_with(".zip") {
        extract_zip(&archive_data, target_path).await?;
    } else if download_url.ends_with(".tar.gz") {
        extract_tar_gz(&archive_data, target_path).await?;
    } else {
        return Err(anyhow::anyhow!("Unknown archive format"));
    }

    if !target_path.exists() {
        return Err(anyhow::anyhow!(
            "UV binary not found after extraction at {:?}",
            target_path
        ));
    }

    info!("UV binary installed at {:?}", target_path);
    Ok(())
}

async fn extract_zip(archive_data: &[u8], target_path: &Path) -> Result<()> {
    let uv_binary_name = uv_binary_name();
    let target_dir = target_path.parent().unwrap().to_path_buf();
    let target_path = target_path.to_path_buf();
    let archive_data = archive_data.to_vec();

    let contents = tokio::task::spawn_blocking(move || -> Result<Vec<u8>> {
        use std::io::{Cursor, Read};

        let cursor = Cursor::new(archive_data);
        let mut archive = zip::ZipArchive::new(cursor)?;

        for i in 0..archive.len() {
            let mut file = archive.by_index(i)?;
            let name = file.name().to_string();

            if name.ends_with(uv_binary_name) || name == uv_binary_name {
                let mut contents = Vec::new();
                file.read_to_end(&mut contents)?;
                return Ok(contents);
            }
        }

        Err(anyhow::anyhow!("UV binary not found in archive"))
    })
    .await??;

    fs::create_dir_all(&target_dir).await?;
    fs::write(&target_path, &contents).await?;

    info!("Extracted UV to {:?}", target_path);
    Ok(())
}

async fn extract_tar_gz(archive_data: &[u8], target_path: &Path) -> Result<()> {
    let uv_binary_name = uv_binary_name();
    let target_dir = target_path.parent().unwrap().to_path_buf();
    let target_path = target_path.to_path_buf();
    let archive_data = archive_data.to_vec();

    fs::create_dir_all(&target_dir).await?;

    tokio::task::spawn_blocking(move || -> Result<()> {
        use flate2::read::GzDecoder;
        use std::io::Cursor;
        use tar::Archive;

        let cursor = Cursor::new(archive_data);
        let gz = GzDecoder::new(cursor);
        let mut archive = Archive::new(gz);

        for entry in archive.entries()? {
            let mut entry = entry?;
            let path_str = entry.path()?.to_string_lossy().to_string();

            if path_str.ends_with(uv_binary_name) {
                entry.unpack(&target_path)?;

                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let mut perms = std::fs::metadata(&target_path)?.permissions();
                    perms.set_mode(0o755);
                    std::fs::set_permissions(&target_path, perms)?;
                }

                info!("Extracted {} to {:?}", path_str, target_path);
                return Ok(());
            }
        }

        Err(anyhow::anyhow!("UV binary not found in archive"))
    })
    .await??;

    Ok(())
}

async fn setup_uv_isolation(hpm_dir: &Path) -> Result<()> {
    debug!("Setting up UV isolation in {:?}", hpm_dir);

    let cache_dir = hpm_dir.join("uv-cache");
    let config_dir = hpm_dir.join("uv-config");

    fs::create_dir_all(&cache_dir).await?;
    fs::create_dir_all(&config_dir).await?;

    // Forward slashes keep the TOML cross-platform (Windows backslashes trigger unicode escapes).
    let cache_dir_str = cache_dir.to_string_lossy().replace('\\', "/");
    let uv_config = format!(
        r#"# HPM UV isolation configuration
cache-dir = "{cache_dir_str}"
"#
    );

    fs::write(config_dir.join("uv.toml"), uv_config).await?;

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
pub async fn ensure_managed_python(version: &str) -> Result<()> {
    {
        let cache = installed_python_versions().lock().unwrap();
        if cache.contains(version) {
            return Ok(());
        }
    }

    debug!("Ensuring managed Python {} is available", version);
    run_uv_command(&["python", "install", version])
        .await
        .with_context(|| format!("Failed to install managed Python {}", version))?;

    installed_python_versions()
        .lock()
        .unwrap()
        .insert(version.to_string());
    Ok(())
}

/// Run UV with HPM's isolated cache and config applied per-invocation.
pub async fn run_uv_command(args: &[&str]) -> Result<std::process::Output> {
    let uv_path = ensure_uv_binary().await?;

    debug!("Running UV command: {:?} {:?}", uv_path, args);

    let mut cmd = tokio::process::Command::new(uv_path);
    cmd.args(args);
    for (key, value) in uv_env()? {
        cmd.env(key, value);
    }

    // Suppress the brief console window flash that UV (a CLI tool) would
    // otherwise show when spawned from a GUI parent on Windows.
    crate::process_util::hide_console_tokio(&mut cmd);

    let output = cmd.output().await?;

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
    fn test_uv_env() {
        let env = uv_env().expect("home dir resolves under test env");
        let keys: Vec<_> = env.iter().map(|(k, _)| *k).collect();
        assert!(keys.contains(&"UV_CACHE_DIR"));
        assert!(keys.contains(&"UV_CONFIG_FILE"));
        assert!(keys.contains(&"UV_NO_SYNC"));
        assert!(keys.contains(&"UV_SYSTEM_PYTHON"));
        assert!(keys.contains(&"UV_PYTHON_INSTALL_DIR"));
        assert!(keys.contains(&"UV_PYTHON_DOWNLOADS"));
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
