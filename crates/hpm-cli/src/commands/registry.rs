//! Registry management commands.
//!
//! Commands for adding, listing, removing, and updating package registries.
//!
//! ```bash
//! hpm registry add https://api.3db.dk/v1/registry --name 3db
//! hpm registry add https://github.com/houdinihub/registry.git --name community --type git
//! hpm registry list
//! hpm registry remove 3db
//! hpm registry update
//! ```

use anyhow::{Result, bail};
use console::style;
use hpm_config::{Config, ConfigOverlay, RegistrySourceConfig, RegistryType};
use hpm_core::registry::Registry;
use tracing::info;

// `RegistrySet` construction lives on the type itself
// (`RegistrySet::from_config(&Config)` in hpm-core); this module owns the
// imperative `hpm registry …` subcommands only.

/// Load the user config file as an overlay for editing.
///
/// Registry add/remove edit the user config *file* rather than re-saving the
/// resolved [`Config`]: dumping the resolved config would bake the current
/// defaults (and any project-layer values) into the user file, pinning them
/// forever.
fn load_user_overlay() -> Result<ConfigOverlay> {
    let path = Config::user_config_path();
    if path.exists() {
        Ok(ConfigOverlay::load(&path)?)
    } else {
        Ok(ConfigOverlay::default())
    }
}

/// Add a new registry to the user config file.
pub async fn add_registry(
    url: String,
    name: Option<String>,
    registry_type: Option<String>,
    if_not_exists: bool,
) -> Result<()> {
    // Infer registry type from URL if not specified
    let reg_type = match registry_type.as_deref() {
        Some("api") => RegistryType::Api,
        Some("git") => RegistryType::Git,
        Some(other) => bail!("Unknown registry type '{}'. Use 'api' or 'git'.", other),
        None => {
            if url.ends_with(".git") || url.contains("github.com") || url.contains("gitea") {
                RegistryType::Git
            } else {
                RegistryType::Api
            }
        }
    };

    // Derive name from URL if not specified
    let display_name = name.unwrap_or_else(|| {
        url.trim_end_matches('/')
            .trim_end_matches(".git")
            .rsplit('/')
            .next()
            .unwrap_or("registry")
            .to_string()
    });

    let registry_config = RegistrySourceConfig {
        name: display_name.clone(),
        url: url.clone(),
        registry_type: reg_type.clone(),
    };

    let mut overlay = load_user_overlay()?;
    let registries = overlay.registries.get_or_insert_with(Vec::new);
    if registries.iter().any(|r| r.name == display_name) {
        if if_not_exists {
            // Idempotent path: a registry by this name is already present, so
            // there is nothing to do. Report the no-op rather than erroring so
            // automated provisioning can re-run `hpm registry add … --if-not-exists`
            // safely.
            info!(
                "Registry '{}' already exists; left unchanged (--if-not-exists)",
                display_name
            );
            println!(
                "{} Registry '{}' already exists, left unchanged",
                style("[=]").dim(),
                style(&display_name).cyan()
            );
            return Ok(());
        }
        bail!(
            "A registry named '{}' already exists. Remove it first or choose a different name.",
            display_name
        );
    }
    registries.push(registry_config);
    overlay.save(&Config::user_config_path())?;

    info!("Added registry '{}' at {}", display_name, url);
    println!(
        "{} Added registry '{}' ({})",
        style("[+]").green().bold(),
        style(&display_name).cyan(),
        match reg_type {
            RegistryType::Api => "API",
            RegistryType::Git => "Git",
        }
    );
    println!("    URL: {}", style(&url).dim());

    Ok(())
}

/// List configured registries.
pub async fn list_registries(config: &Config) -> Result<()> {
    if config.registries.is_empty() {
        println!("{}", style("No registries configured.").dim());
        println!();
        println!("Add a registry with:");
        println!(
            "  {} {} {}",
            style("hpm registry add").cyan(),
            style("<url>").dim(),
            style("--name <alias>").dim()
        );
        return Ok(());
    }

    println!(
        "{} {} configured:",
        style("Registries").bold(),
        config.registries.len()
    );
    println!();

    for reg in &config.registries {
        let type_badge = match reg.registry_type {
            RegistryType::Api => style("API").green(),
            RegistryType::Git => style("Git").blue(),
        };
        println!(
            "  {} {} [{}]",
            style("*").dim(),
            style(&reg.name).cyan().bold(),
            type_badge,
        );
        println!("    {}", style(&reg.url).dim());
    }

    Ok(())
}

/// Remove a registry by name from the user config file.
pub async fn remove_registry(name: String) -> Result<()> {
    let mut overlay = load_user_overlay()?;
    let registries = overlay.registries.get_or_insert_with(Vec::new);
    let before = registries.len();
    registries.retain(|r| r.name != name);
    if registries.len() == before {
        bail!("Registry '{}' not found.", name);
    }
    overlay.save(&Config::user_config_path())?;

    println!(
        "{} Removed registry '{}'",
        style("[-]").red().bold(),
        style(&name).cyan()
    );

    Ok(())
}

/// Update (refresh) all registry caches.
pub async fn update_registries(config: &Config) -> Result<()> {
    if config.registries.is_empty() {
        println!("{}", style("No registries configured.").dim());
        return Ok(());
    }

    println!(
        "{} Updating {} registries...",
        style("[~]").yellow().bold(),
        config.registries.len()
    );

    for reg in &config.registries {
        print!("  {} {}... ", style("*").dim(), style(&reg.name).cyan());
        match reg.registry_type {
            RegistryType::Api => {
                // API registries don't need cache update
                println!("{}", style("OK (live)").green());
            }
            RegistryType::Git => {
                let cache_dir = config.registry_cache_path(&reg.name);
                let git_reg =
                    hpm_core::registry::git::GitRegistry::new(&reg.name, &reg.url, &cache_dir);
                match git_reg.refresh().await {
                    Ok(()) => println!("{}", style("OK").green()),
                    Err(e) => println!("{} {}", style("FAILED").red(), style(e).dim()),
                }
            }
        }
    }

    println!();
    println!("{}", style("Registry update complete.").green().bold());
    Ok(())
}
