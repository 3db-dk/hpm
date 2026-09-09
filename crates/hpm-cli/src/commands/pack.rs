use anyhow::{Context, Result, bail};
use hpm_config::Config;
use hpm_core::packer;
use hpm_package::{NativePackageTarget, PackageManifest, Platform};
use std::path::{Path, PathBuf};

use crate::console::Console;

/// Inputs to one `hpm pack` run.
pub struct PackOptions {
    /// Package directory (the one holding `hpm.toml`). None = cwd.
    pub directory: Option<PathBuf>,
    /// Ed25519 signing key (PKCS#8 PEM), overriding config and
    /// `HPM_SIGNING_KEY`. None falls back to those.
    pub key: Option<PathBuf>,
    /// Where to write the archive. None = the package directory.
    pub output: Option<PathBuf>,
    /// Emit the single-line JSON payload CI consumes instead of prose.
    pub json: bool,
    /// Target platform; defaults to host when `[compat].platforms` is declared.
    pub platform: Option<String>,
    /// Treat a declared `[[operators]]` `source` missing from the archive as
    /// fatal rather than a warning.
    pub verify_assets: bool,
    /// Shape the bundled `{slug}.json` for SideFX's hpackage repository, so
    /// the archive can be uploaded as packed rather than rewritten and
    /// re-zipped. Fails the pack when the manifest declares something
    /// hpackage cannot represent.
    pub sidefx: bool,
}

/// How a directory's entries match a filename we need by exact spelling.
enum NameMatch {
    /// An entry is spelled exactly as asked.
    Exact,
    /// No exact entry, but one differs only by case. Carries its real name.
    CaseDiffers(String),
    /// Nothing resembling the name is there.
    Missing,
}

/// Look `name` up among `dir`'s entries by exact spelling.
///
/// `Path::exists` asks the filesystem, and APFS and NTFS answer
/// case-insensitively: a repo holding `FLOPs.json` reports `flops.json` as
/// present, while the same tree on Linux reports it absent. Anything keyed
/// off that answer produces a different archive depending on who built it.
/// Houdini looks the descriptor up by exact name and the archive entry is
/// written under the exact name, so a case-folded match is a different file.
/// Comparing directory entries gives the same answer everywhere.
fn find_exact_entry(dir: &Path, name: &str) -> NameMatch {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return NameMatch::Missing;
    };
    let mut case_differs = None;
    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let Some(found) = file_name.to_str() else {
            continue;
        };
        if found == name {
            return NameMatch::Exact;
        }
        if case_differs.is_none() && found.eq_ignore_ascii_case(name) {
            case_differs = Some(found.to_string());
        }
    }
    match case_differs {
        Some(found) => NameMatch::CaseDiffers(found),
        None => NameMatch::Missing,
    }
}

pub async fn execute(config: &Config, options: PackOptions, console: &mut Console) -> Result<()> {
    let PackOptions {
        directory,
        key,
        output,
        json,
        platform: platform_arg,
        verify_assets,
        sidefx,
    } = options;

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

    // Generate Houdini-native package.json (or take the user's hand-written
    // one). A generation failure fails the pack: shipping an archive without
    // the package.json Houdini needs would produce a broken install.
    //
    // The json is always injected so it lands at the ARCHIVE ROOT, next to
    // the `{slug}/` content folder (Houdini's hpackage layout): the json's
    // paths are `$HOUDINI_PACKAGE_PATH/{slug}/...`, so extracting the archive
    // straight into a packages directory must yield `packages/{slug}.json` +
    // `packages/{slug}/...` for them to resolve. A hand-written file is
    // injected by content (create_archive skips the staged copy).
    //
    // `--sidefx` additionally shapes the generated json to what SideFX's
    // hpackage repository accepts on upload, so the publisher can ship the
    // archive `pack` produced rather than rewriting the json and re-zipping —
    // a repack invalidates both the checksum and the signature reported
    // below. The normalization is confined to this file, which hpm's own
    // installer skips, and refuses any rewrite that would change what the
    // manifest declared.
    let target = if sidefx {
        // hpackage's uploader reads `{package_dir}/README.md` and sends its
        // text as the package description; a missing file raises rather than
        // warning, and the name is literal. hpm's own README check accepts
        // `README.txt` and a bare `README` and only warns, so a package can
        // satisfy `hpm check` and still be refused on upload.
        match find_exact_entry(&package_dir, "README.md") {
            NameMatch::Exact => {}
            NameMatch::CaseDiffers(found) => bail!(
                "SideFX's hpackage uploader reads `README.md` by that exact name and this \
                 package has {found}. Rename it to README.md."
            ),
            NameMatch::Missing => bail!(
                "SideFX's hpackage uploader requires a README.md in the package directory and \
                 sends its text as the published description; it refuses the upload when the \
                 file is absent. Add {}.",
                package_dir.join("README.md").display()
            ),
        }
        // The uploader also refuses a README still holding the placeholder
        // its own scaffold writes.
        let readme = std::fs::read_to_string(package_dir.join("README.md"))
            .context("Failed to read README.md")?;
        if readme.contains("<enter a one-line description") {
            bail!(
                "README.md still contains the placeholder line hpackage's scaffold writes \
                 (`<enter a one-line description`), and its uploader refuses that. Write the \
                 package description there."
            );
        }
        NativePackageTarget::SideFxHpackage
    } else {
        NativePackageTarget::Generic
    };
    let (native_filename, native_pkg) = manifest
        .generate_houdini_native_package_for(target)
        .map_err(|e| anyhow::anyhow!(e))
        .context("Failed to generate the Houdini package.json for the archive")?;
    let hand_written = package_dir.join(&native_filename);
    let json_bytes = match find_exact_entry(&package_dir, &native_filename) {
        // User has a hand-written file; ship it verbatim, don't overwrite
        NameMatch::Exact => std::fs::read(&hand_written).with_context(|| {
            format!(
                "Failed to read hand-written Houdini package json {}",
                hand_written.display()
            )
        })?,
        // A file whose name differs only by case is not the descriptor
        // Houdini will look for, so it cannot be shipped as one — but the
        // author plainly meant it to be used, and silently generating over
        // it would drop whatever it declares without saying so.
        NameMatch::CaseDiffers(found) => {
            console.warn(format!(
                "Ignoring {found}: the bundled Houdini descriptor must be named exactly \
                 {native_filename} (after the package slug), so {found} is a different file. \
                 Generating the descriptor from hpm.toml instead — rename it to {native_filename} \
                 to ship it verbatim."
            ));
            serde_json::to_vec_pretty(&native_pkg)
                .context("Failed to serialize Houdini native package JSON")?
        }
        NameMatch::Missing => serde_json::to_vec_pretty(&native_pkg)
            .context("Failed to serialize Houdini native package JSON")?,
    };
    let inject_files: Vec<(String, Vec<u8>)> = vec![(native_filename, json_bytes)];
    let content_prefix = manifest.package.slug().to_string();

    // Run pack on blocking thread (zip I/O)
    let stage_config = manifest.stage.clone();
    let result = tokio::task::spawn_blocking({
        let package_dir = package_dir.clone();
        let name = name.clone();
        let version = version.clone();
        let output_dir = output_dir.clone();
        let content_prefix = content_prefix.clone();
        move || {
            packer::pack(
                &package_dir,
                &name,
                &version,
                &output_dir,
                signing_key.as_ref(),
                platform.as_ref(),
                &stage_config,
                packer::ArchiveLayout {
                    inject_files: &inject_files,
                    content_prefix: Some(&content_prefix),
                },
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
    let asset_index = hpm_core::collect_assets(
        &result.archive_path,
        &manifest.operators,
        platform.as_ref(),
        Some(&content_prefix),
    )
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression: the bundled descriptor was chosen with `Path::exists`,
    /// which APFS and NTFS answer case-insensitively. A repo holding
    /// `FLOPs.json` shipped that file as the root `flops.json` — hardcoded
    /// author paths and all — when packed on macOS or Windows, and shipped
    /// the generated descriptor when packed on Linux. It also bypassed
    /// `--sidefx`, since a hand-written file is shipped without
    /// normalization, so the flag silently did nothing on exactly the
    /// machines most authors publish from.
    #[test]
    fn exact_entry_lookup_does_not_fold_case() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("FLOPs.json"), "{}").unwrap();

        // The filesystem under this test may well answer `exists` for the
        // lowercase spelling; the lookup must not.
        match find_exact_entry(dir.path(), "flops.json") {
            NameMatch::CaseDiffers(found) => assert_eq!(found, "FLOPs.json"),
            NameMatch::Exact => panic!("case-folded match reported as exact"),
            NameMatch::Missing => panic!("the case-differing file should be reported"),
        }

        assert!(matches!(
            find_exact_entry(dir.path(), "FLOPs.json"),
            NameMatch::Exact
        ));
        assert!(matches!(
            find_exact_entry(dir.path(), "README.md"),
            NameMatch::Missing
        ));
    }

    #[test]
    fn exact_entry_lookup_handles_a_missing_directory() {
        let dir = tempfile::tempdir().unwrap();
        let absent = dir.path().join("nope");
        assert!(matches!(
            find_exact_entry(&absent, "README.md"),
            NameMatch::Missing
        ));
    }
}
