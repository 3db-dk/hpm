use anyhow::{Context, Result};
use hpm_config::Config;
use hpm_core::packer::{self, PackRequest};
use hpm_package::{NativePackageTarget, Platform};
use std::path::{Path, PathBuf};

use crate::console::Console;

/// Inputs to one `hpm pack` run.
pub struct PackOptions {
    /// The package's `hpm.toml`, or the directory holding it. None = cwd.
    pub manifest: Option<PathBuf>,
    /// Ed25519 signing key (PKCS#8 PEM), overriding config and
    /// `HPM_SIGNING_KEY`. None falls back to those.
    pub key: Option<PathBuf>,
    /// Where to write the archive. None = the package directory.
    pub output: Option<PathBuf>,
    /// Emit the single-line JSON payload CI consumes instead of prose.
    pub json: bool,
    /// Target platform; defaults to host when `[compat].platforms` is declared.
    pub platform: Option<Platform>,
    /// Treat a declared `[[operators]]` `source` missing from the archive as
    /// fatal rather than a warning.
    pub verify_assets: bool,
    /// Shape the bundled `{slug}.json` for SideFX's hpackage repository, so
    /// the archive can be uploaded as packed rather than rewritten and
    /// re-zipped. Fails the pack when the manifest declares something
    /// hpackage cannot represent.
    pub sidefx: bool,
}

/// CLI entry for `hpm pack`: resolve the signing key, run
/// [`packer::pack_package`], and report the archive.
pub async fn execute(config: &Config, options: PackOptions, console: &mut Console) -> Result<()> {
    let PackOptions {
        manifest,
        key,
        output,
        json,
        platform,
        verify_assets,
        sidefx,
    } = options;

    let package_dir = super::manifest_utils::package_dir(manifest)?;

    // CLI flag, then HPM_SIGNING_KEY (PEM content or a path), then config.
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

    let request = PackRequest {
        output_dir: output.unwrap_or_else(|| package_dir.clone()),
        dir: package_dir,
        key: signing_key,
        platform,
        target: if sidefx {
            NativePackageTarget::SideFxHpackage
        } else {
            NativePackageTarget::Generic
        },
        verify_assets,
    };
    // Zip I/O; keep it off the async runtime.
    let outcome = tokio::task::spawn_blocking(move || packer::pack_package(request))
        .await
        .context("Pack task panicked")??;

    // Warnings go to stderr, keeping stdout clean for --json.
    for warning in &outcome.warnings {
        console.warn(warning);
    }
    let result = &outcome.result;
    let asset_index = &outcome.asset_index;
    let (name, version) = (
        &outcome.manifest.package.name,
        &outcome.manifest.package.version,
    );

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
