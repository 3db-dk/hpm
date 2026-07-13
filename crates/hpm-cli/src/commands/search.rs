//! Search command implementation.
//!
//! Searches configured registries for packages matching a query.

use anyhow::Result;
use console::style;
use hpm_config::Config;
use hpm_core::registry::RegistrySet;

/// Execute the search command.
///
/// Searches all configured registries for packages matching the query.
/// Falls back to a helpful message if no registries are configured.
pub async fn search_packages(
    config: &Config,
    query: String,
    _limit: Option<u32>,
    json_output: bool,
) -> Result<()> {
    if config.registries.is_empty() {
        println!(
            "{} No registries configured.",
            style("Note:").yellow().bold()
        );
        println!();
        println!("Add a registry first:");
        println!(
            "  {} {} {} {}",
            style("hpm registry add").cyan(),
            style("<url>").dim(),
            style("--name").green(),
            style("<alias>").dim()
        );
        return Ok(());
    }

    let registry_set = RegistrySet::from_config(config)?;

    let results = registry_set
        .search(&query)
        .await
        .map_err(|e| anyhow::anyhow!("Registry search failed: {}", e))?;

    for (name, err) in &results.unavailable {
        eprintln!(
            "{} Registry '{}' is unreachable ({}); results may be incomplete.",
            style("Warning:").yellow().bold(),
            style(name).cyan(),
            err
        );
    }

    if json_output {
        let json = serde_json::to_string_pretty(&results.packages)?;
        println!("{}", json);
        return Ok(());
    }

    if results.packages.is_empty() {
        println!("No packages found matching '{}'.", style(&query).yellow());
        return Ok(());
    }

    println!(
        "Found {} packages matching '{}':",
        style(results.packages.len()).bold(),
        style(&query).yellow()
    );
    println!();

    for entry in &results.packages {
        let yanked = if entry.yanked {
            format!(" {}", style("(yanked)").red())
        } else {
            String::new()
        };

        println!(
            "  {} {}{}",
            style(&entry.name).cyan().bold(),
            style(&entry.version).green(),
            yanked,
        );

        if let Some(ref desc) = entry.description {
            println!("    {}", style(desc).dim());
        }

        if let Some(ref compat) = entry.houdini_compat {
            println!("    Houdini: {}", style(compat).dim());
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_search_no_registries() {
        // Should not panic when no registries are configured
        let config = Config::default();
        let result = search_packages(&config, "test".to_string(), None, false).await;
        assert!(result.is_ok());
    }
}
