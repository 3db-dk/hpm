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
    verify_assets: bool,
    console: &mut Console,
) -> Result<()> {
    let package_dir = match directory {
        Some(dir) => dir,
        None => std::env::current_dir().context("Failed to get current directory")?,
    };

    // Validate package first, keeping stdout clean for --json: warnings go
    // to stderr via the console, errors fail the pack.
    let validation = super::check::validate_package(Some(package_dir.clone())).await?;
    for warning in &validation.warnings {
        console.warn(warning);
    }
    if !validation.is_valid {
        bail!(
            "Package validation failed with {} error(s):\n  - {}",
            validation.errors.len(),
            validation.errors.join("\n  - ")
        );
    }

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

    // Generate Houdini-native package.json if not already present. A
    // generation failure fails the pack: shipping an archive without the
    // package.json Houdini needs would produce a broken install.
    let (native_filename, native_pkg) = manifest
        .generate_houdini_native_package()
        .map_err(|e| anyhow::anyhow!(e))
        .context("Failed to generate the Houdini package.json for the archive")?;
    let inject_files: Vec<(String, Vec<u8>)> = if package_dir.join(&native_filename).exists() {
        // User has a hand-written file; don't overwrite
        vec![]
    } else {
        let json_bytes = serde_json::to_vec_pretty(&native_pkg)
            .context("Failed to serialize Houdini native package JSON")?;
        vec![(native_filename, json_bytes)]
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
    // declarations, resolving each operator's source for the target platform
    // and checking it against the produced archive. An indexing failure
    // fails the pack — emitting an archive whose advertised index silently
    // dropped is a packaging bug, not a shippable artifact.
    let asset_index =
        hpm_core::collect_assets(&result.archive_path, &manifest.operators, platform.as_ref())
            .inspect_err(|_| {
                // Don't leave a half-vetted archive behind for CI to publish.
                let _ = std::fs::remove_file(&result.archive_path);
            })
            .context("Failed to build the asset index for the packed archive")?;

    // A declared operator source that isn't in the produced archive means the
    // index would advertise a file the package doesn't ship. With
    // `--verify-assets` this is fatal (and the invalid archive is removed so CI
    // can't publish it); otherwise it's a warning.
    if !asset_index.missing_sources.is_empty() {
        if verify_assets {
            let _ = std::fs::remove_file(&result.archive_path);
            let list = asset_index.missing_sources.join("\n  - ");
            bail!(
                "operator source(s) declared in [[operators]] but missing from the packed archive:\n  - {list}\n\n\
                 `source` must name the file's path inside the package (after [stage] placement). \
                 Build the package first if these are compiled artifacts, fix the path, or declare a \
                 per-platform `source` table. The archive was removed."
            );
        }
        for missing in &asset_index.missing_sources {
            console.warn(format!(
                "Declared operator source not found in archive: {missing} (pass --verify-assets to fail the pack)"
            ));
        }
    }

    if json {
        // Machine-readable JSON output for CI. Single-line payload; the
        // shape is an established contract and must not change.
        let json_output = serde_json::json!({
            "archive": result.archive_path.display().to_string(),
            "sha256": result.checksum,
            "signature": result.signature,
            "key_id": result.key_id,
            "platform": result.platform,
            "assets": asset_index.assets,
        });
        console.stdout(serde_json::to_string(&json_output).unwrap());
    } else {
        // Human-readable output
        if let Some(ref p) = result.platform {
            console.success(format!("Packed {} v{} ({})", name, version, p));
        } else {
            console.success(format!("Packed {} v{}", name, version));
        }
        console.stdout(format!("  archive: {}", result.archive_path.display()));
        console.stdout(format!("  sha256:  {}", result.checksum));
        if let Some(ref p) = result.platform {
            console.stdout(format!("  platform: {}", p));
        }
        if let Some(ref sig) = result.signature {
            console.stdout(format!("  sig:     {}", sig));
        }
        if let Some(ref kid) = result.key_id {
            console.stdout(format!("  kid:     {}", kid));
        }
        if !asset_index.assets.is_empty() {
            let hda = asset_index
                .assets
                .iter()
                .filter(|a| matches!(a.kind, hpm_assets::AssetKind::HdaOperator))
                .count();
            let dso = asset_index.assets.len() - hda;
            console.stdout(format!(
                "  assets:  {} ({} HDA, {} DSO)",
                asset_index.assets.len(),
                hda,
                dso
            ));
        }
    }

    Ok(())
}
