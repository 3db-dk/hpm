use anyhow::{Context, Result, bail};
use hpm_config::Config;
use hpm_core::packer;
use hpm_package::{PackageManifest, Platform};
use std::path::{Path, PathBuf};

use crate::console::Console;

pub async fn execute(
    directory: Option<PathBuf>,
    key: Option<PathBuf>,
    output: Option<PathBuf>,
    json: bool,
    platform_arg: Option<String>,
    console: &mut Console,
) -> Result<()> {
    let package_dir = match directory {
        Some(dir) => dir,
        None => std::env::current_dir().context("Failed to get current directory")?,
    };

    // Validate package first
    super::check::check_package(Some(package_dir.clone())).await?;

    // Read manifest to get name and version
    let manifest_path = package_dir.join("hpm.toml");
    let content = std::fs::read_to_string(&manifest_path).context("Failed to read hpm.toml")?;
    let manifest: PackageManifest = toml::from_str(&content).context("Failed to parse hpm.toml")?;

    let name = &manifest.package.name;
    let version = &manifest.package.version;

    // Resolve platform
    let platform = match (&platform_arg, &manifest.native) {
        (Some(_), None) => {
            bail!("--platform was specified but package has no [native] section");
        }
        (Some(p), Some(_)) => Some(p.parse::<Platform>().map_err(|e| anyhow::anyhow!(e))?),
        (None, Some(_)) => {
            // Auto-detect host platform
            let detected = Platform::current()
                .context("Could not detect host platform; use --platform to specify explicitly")?;
            Some(detected)
        }
        (None, None) => None,
    };

    // Validate platform is in native.platforms
    if let (Some(p), Some(native)) = (&platform, &manifest.native) {
        if !native.platforms.contains(&p.to_string()) {
            bail!(
                "Platform '{}' is not declared in [native] platforms: {:?}",
                p,
                native.platforms
            );
        }
    }

    // Resolve signing key: CLI flag → HPM_SIGNING_KEY env (PEM content or path) → config
    let signing_key = if let Some(path) = key {
        Some(packer::load_signing_key(&path)?)
    } else if let Ok(value) = std::env::var("HPM_SIGNING_KEY") {
        if value.trim_start().starts_with("-----BEGIN") {
            Some(packer::load_signing_key_from_pem(&value)?)
        } else {
            Some(packer::load_signing_key(Path::new(&value))?)
        }
    } else {
        Config::load()
            .unwrap_or_default()
            .signing
            .key_path
            .map(|p| packer::load_signing_key(&p))
            .transpose()?
    };

    let output_dir = output.unwrap_or_else(|| package_dir.clone());

    // Generate Houdini-native package.json if not already present
    let inject_files: Vec<(String, Vec<u8>)> = match manifest.generate_houdini_native_package() {
        Ok((filename, native_pkg)) => {
            if package_dir.join(&filename).exists() {
                // User has a hand-written file; don't overwrite
                vec![]
            } else {
                let json_bytes = serde_json::to_vec_pretty(&native_pkg)
                    .context("Failed to serialize Houdini native package JSON")?;
                vec![(filename, json_bytes)]
            }
        }
        Err(_) => vec![],
    };

    // Run pack on blocking thread (zip I/O)
    let native_config = manifest.native.clone();
    let result = tokio::task::spawn_blocking({
        let package_dir = package_dir.clone();
        let name = name.clone();
        let version = version.clone();
        let output_dir = output_dir.clone();
        move || {
            packer::pack(
                &package_dir,
                &name,
                &version,
                &output_dir,
                signing_key.as_ref(),
                platform.as_ref(),
                native_config.as_ref(),
                &inject_files,
            )
        }
    })
    .await
    .context("Pack task panicked")??;

    if json {
        // Machine-readable JSON output for CI
        let json_output = serde_json::json!({
            "archive": result.archive_path.display().to_string(),
            "sha256": result.checksum,
            "signature": result.signature,
            "key_id": result.key_id,
            "platform": result.platform,
        });
        println!("{}", serde_json::to_string(&json_output).unwrap());
    } else {
        // Human-readable output
        if let Some(ref p) = result.platform {
            console.success(format!("Packed {} v{} ({})", name, version, p));
        } else {
            console.success(format!("Packed {} v{}", name, version));
        }
        println!("  archive: {}", result.archive_path.display());
        println!("  sha256:  {}", result.checksum);
        if let Some(ref p) = result.platform {
            println!("  platform: {}", p);
        }
        if let Some(ref sig) = result.signature {
            println!("  sig:     {}", sig);
        }
        if let Some(ref kid) = result.key_id {
            println!("  kid:     {}", kid);
        }
    }

    Ok(())
}
