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

use anyhow::{bail, Result};
use console::style;
use hpm_config::{Config, RegistrySourceConfig, RegistryType};
use hpm_core::registry::Registry;
use tracing::info;

/// Add a new registry.
pub async fn add_registry(
    url: String,
    name: Option<String>,
    registry_type: Option<String>,
) -> Result<()> {
    let mut config = Config::load().unwrap_or_default();

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

    if !config.add_registry(registry_config) {
        bail!(
            "A registry named '{}' already exists. Remove it first or choose a different name.",
            display_name
        );
    }

    config.save_user_config()?;

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
pub async fn list_registries() -> Result<()> {
    let config = Config::load().unwrap_or_default();

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

/// Remove a registry by name.
pub async fn remove_registry(name: String) -> Result<()> {
    let mut config = Config::load().unwrap_or_default();

    if !config.remove_registry(&name) {
        bail!("Registry '{}' not found.", name);
    }

    config.save_user_config()?;

    println!(
        "{} Removed registry '{}'",
        style("[-]").red().bold(),
        style(&name).cyan()
    );

    Ok(())
}

/// Update (refresh) all registry caches.
pub async fn update_registries() -> Result<()> {
    let config = Config::load().unwrap_or_default();

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

/// Build a RegistrySet from the current configuration.
pub fn build_registry_set(config: &Config) -> hpm_core::registry::RegistrySet {
    let mut set = hpm_core::registry::RegistrySet::new();

    for reg in &config.registries {
        match reg.registry_type {
            RegistryType::Api => {
                if let Ok(api_reg) = hpm_core::registry::ApiRegistry::new(&reg.name, &reg.url) {
                    set.add(Box::new(api_reg));
                }
            }
            RegistryType::Git => {
                let cache_dir = config.registry_cache_path(&reg.name);
                let git_reg = hpm_core::registry::GitRegistry::new(&reg.name, &reg.url, &cache_dir);
                set.add(Box::new(git_reg));
            }
        }
    }

    set
}
