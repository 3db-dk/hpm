//! `hpm global` — install packages into Houdini's user preferences.
//!
//! A global install loads in every session of one Houdini version, with no
//! project and no launcher wiring:
//!
//! ```bash
//! hpm global add studio/utility-nodes --houdini 21.0
//! hpm global list --houdini 21.0
//! hpm global remove studio/utility-nodes --houdini 21.0
//! ```
//!
//! Unlike a project install, the target directory is not hpm's. Nothing here
//! scans Houdini's packages directory to decide what to delete — removal is
//! driven off the ledger and touches only files hpm recorded writing.

use crate::console::Console;
use anyhow::{Context, Result};
use console::style;
use hpm_config::Config;
use hpm_core::global;
use hpm_core::houdini_prefs::HoudiniVersion;
use hpm_core::registry::RegistrySet;
use hpm_core::{ArchiveFetcher, StorageManager};
use hpm_package::PackagePath;

/// Split `creator/slug@version` into its parts. A bare name resolves to the
/// highest non-yanked version, matching `hpm add`.
fn parse_spec(spec: &str) -> (&str, &str) {
    match spec.split_once('@') {
        Some((name, version)) if !version.is_empty() => (name, version),
        _ => (spec, "*"),
    }
}

fn parse_package(name: &str) -> Result<PackagePath> {
    PackagePath::new(name).with_context(|| {
        format!("'{name}' is not a package identifier. Use the scoped 'creator/slug' form.")
    })
}

fn parse_houdini(input: &str) -> Result<HoudiniVersion> {
    Ok(HoudiniVersion::parse(input)?)
}

pub async fn add_package(
    config: &Config,
    spec: &str,
    houdini: &str,
    registry: Option<&str>,
    console: &mut Console,
) -> Result<()> {
    let version = parse_houdini(houdini)?;
    let (name, version_req) = parse_spec(spec);
    parse_package(name)?;

    let storage = StorageManager::new(config.storage.clone())?;
    let fetcher = ArchiveFetcher::new(
        config.storage.home_dir.join("cache"),
        config.storage.home_dir.join("fetch"),
    )?;
    let registries = RegistrySet::from_config(config)?;

    let installed = global::add(
        config,
        &storage,
        &fetcher,
        &registries,
        version,
        name,
        version_req,
        registry,
    )
    .await?;

    console.success(format!(
        "Installed {} {} for Houdini {}",
        style(&installed.package).cyan().bold(),
        installed.version,
        version
    ));
    console.status(format!(
        "  manifest: {}",
        style(installed.manifest_path.display()).dim()
    ));
    console.status(format!(
        "  loads in every Houdini {} session; no project needed",
        version
    ));
    Ok(())
}

pub async fn list_packages(config: &Config, houdini: &str, console: &mut Console) -> Result<()> {
    let version = parse_houdini(houdini)?;
    let installs = global::list(&config.storage.home_dir, version)?;

    if installs.is_empty() {
        console.stdout(
            style(format!(
                "Nothing installed globally for Houdini {}.",
                version
            ))
            .dim()
            .to_string(),
        );
        console.status("");
        console.status("Install one with:");
        console.status(format!(
            "  {} {} {}",
            style("hpm global add").cyan(),
            style("<creator/slug>").dim(),
            style(format!("--houdini {}", version)).dim()
        ));
        return Ok(());
    }

    console.stdout(format!(
        "{} installed for Houdini {}:",
        style(format!("{} package(s)", installs.len())).bold(),
        version
    ));
    console.stdout("");

    for install in &installs {
        console.stdout(format!(
            "  {} {} {}",
            style("*").dim(),
            style(&install.package).cyan().bold(),
            style(&install.version).dim()
        ));
        if let Some(registry) = &install.registry {
            console.stdout(format!("    registry: {}", style(registry).dim()));
        }
        // The ledger says hpm wrote this file; if it is gone, something
        // outside hpm removed it and the package is no longer loading.
        if !install.manifest_present {
            console.stdout(format!(
                "    {}",
                style(format!(
                    "manifest missing at {} — re-run `hpm global add` to restore",
                    install.manifest_path.display()
                ))
                .yellow()
            ));
        }
    }

    Ok(())
}

pub async fn remove_package(
    config: &Config,
    name: &str,
    houdini: &str,
    console: &mut Console,
) -> Result<()> {
    let version = parse_houdini(houdini)?;
    let package = parse_package(name)?;

    let manifest_path = global::remove(&config.storage.home_dir, version, &package)?;

    console.success(format!(
        "Removed {} from Houdini {}",
        style(package.as_str()).cyan().bold(),
        version
    ));
    console.status(format!(
        "  deleted: {}",
        style(manifest_path.display()).dim()
    ));
    console.status(format!(
        "  {}",
        style("package files remain in the store; run `hpm clean` to reclaim space").dim()
    ));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_name_resolves_to_any_version() {
        assert_eq!(parse_spec("acme/tools"), ("acme/tools", "*"));
    }

    #[test]
    fn pinned_version_is_split_off() {
        assert_eq!(parse_spec("acme/tools@1.2.3"), ("acme/tools", "1.2.3"));
    }

    /// A trailing `@` is a typo, not a request for an empty version — treat
    /// it as unpinned rather than trying to resolve version "".
    #[test]
    fn trailing_at_is_treated_as_unpinned() {
        assert_eq!(parse_spec("acme/tools@"), ("acme/tools@", "*"));
    }

    #[test]
    fn unscoped_names_are_rejected() {
        assert!(parse_package("tools").is_err());
        assert!(parse_package("acme/tools").is_ok());
    }

    #[test]
    fn houdini_version_must_parse() {
        assert!(parse_houdini("21.0").is_ok());
        assert!(parse_houdini("houdini21").is_err());
    }
}
