use anyhow::{Context, Result, bail};
use hpm_config::Config;
use hpm_core::packer;
use hpm_package::{PackageManifest, Platform};
use std::path::{Path, PathBuf};

use crate::console::Console;

pub async fn execute(
    config: &Config,
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
    let manifest = PackageManifest::from_path(&manifest_path)?;

    let name = &manifest.package.name;
    let version = &manifest.package.version;

    // Resolve target platform. A package targets per-platform builds when
    // it declares `[compat].platforms`; pure-data / pure-Python packages
    // omit that and produce a single common archive.
    let declared_platforms = &manifest.compat.platforms;
    let has_platforms = !declared_platforms.is_empty();

    let platform = match (&platform_arg, has_platforms) {
        (Some(_), false) => {
            bail!("--platform was specified but package has no [compat].platforms");
        }
        (Some(p), true) => Some(p.parse::<Platform>().map_err(|e| anyhow::anyhow!(e))?),
        (None, true) => {
            // Auto-detect host platform
            let detected = Platform::current()
                .context("Could not detect host platform; use --platform to specify explicitly")?;
            Some(detected)
        }
        (None, false) => None,
    };

    // Validate platform is declared in [compat].platforms
    if let Some(p) = &platform
        && !declared_platforms.contains(p)
    {
        bail!(
            "Platform '{}' is not declared in [compat].platforms: {:?}",
            p,
            declared_platforms
        );
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
        config
            .signing
            .key_path
            .as_ref()
            .map(|p| packer::load_signing_key(p))
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
    let stage_config = manifest.stage.clone();
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
                &stage_config,
                &inject_files,
            )
        }
    })
    .await
    .context("Pack task panicked")??;

    // Build the searchable asset index from the manifest's [[operators]]
    // declarations, checking each declared source against the produced archive.
    // Indexing must not fail the pack itself, so a hard error here degrades to
    // an empty index with a warning.
    let asset_index = match hpm_core::collect_assets(&result.archive_path, &manifest.operators) {
        Ok(index) => index,
        Err(e) => {
            console.warn(format!("Could not build asset index: {e}"));
            hpm_core::AssetIndex {
                assets: Vec::new(),
                missing_sources: Vec::new(),
            }
        }
    };
    for missing in &asset_index.missing_sources {
        console.warn(format!(
            "Declared operator source not found in archive: {missing}"
        ));
    }

    if json {
        // Machine-readable JSON output for CI
        let json_output = serde_json::json!({
            "archive": result.archive_path.display().to_string(),
            "sha256": result.checksum,
            "signature": result.signature,
            "key_id": result.key_id,
            "platform": result.platform,
            "assets": asset_index.assets,
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
        if !asset_index.assets.is_empty() {
            let hda = asset_index
                .assets
                .iter()
                .filter(|a| matches!(a.kind, hpm_assets::AssetKind::HdaOperator))
                .count();
            let hdk = asset_index.assets.len() - hda;
            println!(
                "  assets:  {} ({} HDA, {} HDK)",
                asset_index.assets.len(),
                hda,
                hdk
            );
        }
    }

    Ok(())
}
