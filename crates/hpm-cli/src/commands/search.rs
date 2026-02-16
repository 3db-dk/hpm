//! Search command implementation.
//!
//! Searches configured registries for packages matching a query.

use super::registry::build_registry_set;
use anyhow::Result;
use console::style;
use hpm_config::Config;

/// Execute the search command.
///
/// Searches all configured registries for packages matching the query.
/// Falls back to a helpful message if no registries are configured.
pub async fn search_packages(query: String, _limit: Option<u32>, json_output: bool) -> Result<()> {
    let config = Config::load().unwrap_or_default();

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
        println!();
        println!("Or add packages directly from Git:");
        println!(
            "  {} {} {} {}",
            style("hpm add").cyan(),
            style("--git").green(),
            style("<repository-url>").dim(),
            style("--tag <release-tag>").dim()
        );
        return Ok(());
    }

    let registry_set = build_registry_set(&config);

    let results = registry_set
        .search(&query)
        .await
        .map_err(|e| anyhow::anyhow!("Registry search failed: {}", e))?;

    if json_output {
        let json = serde_json::to_string_pretty(&results)?;
        println!("{}", json);
        return Ok(());
    }

    if results.packages.is_empty() {
        println!("No packages found matching '{}'.", style(&query).yellow());
        return Ok(());
    }

    println!(
        "Found {} packages matching '{}':",
        style(results.total).bold(),
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
        let result = search_packages("test".to_string(), None, false).await;
        assert!(result.is_ok());
    }
}
