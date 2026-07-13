//! Search command implementation.
//!
//! Searches configured registries for packages matching a query.

use crate::console::Console;
use crate::output::OutputFormat;
use anyhow::Result;
use console::style;
use hpm_config::Config;
use hpm_core::registry::RegistrySet;

/// Execute the search command.
///
/// Searches all configured registries for packages matching the query. With
/// `--output json` the result is the established pretty-printed array of
/// packages; `json-lines` streams one package per line and `json-compact`
/// minifies the array. Falls back to a helpful message (or an empty array)
/// if no registries are configured.
pub async fn search_packages(
    config: &Config,
    query: &str,
    console: &mut Console,
    output: OutputFormat,
) -> Result<()> {
    if config.registries.is_empty() {
        if output.is_json() {
            // Empty result set in the same shape as a real search.
            print_packages_json(&[], output, console)?;
        } else {
            console.stdout(format!(
                "{} No registries configured.",
                style("Note:").yellow().bold()
            ));
            console.status("");
            console.status("Add a registry first:");
            console.status(format!(
                "  {} {} {} {}",
                style("hpm registry add").cyan(),
                style("<url>").dim(),
                style("--name").green(),
                style("<alias>").dim()
            ));
        }
        return Ok(());
    }

    let registry_set = RegistrySet::from_config(config)?;

    let results = registry_set
        .search(query)
        .await
        .map_err(|e| anyhow::anyhow!("Registry search failed: {}", e))?;

    for (name, err) in &results.unavailable {
        console.warn(format!(
            "Registry '{}' is unreachable ({}); results may be incomplete.",
            style(name).cyan(),
            err
        ));
    }

    if output.is_json() {
        print_packages_json(&results.packages, output, console)?;
        return Ok(());
    }

    if results.packages.is_empty() {
        console.stdout(format!(
            "No packages found matching '{}'.",
            style(query).yellow()
        ));
        return Ok(());
    }

    console.stdout(format!(
        "Found {} packages matching '{}':",
        style(results.packages.len()).bold(),
        style(query).yellow()
    ));
    console.stdout("");

    for entry in &results.packages {
        let yanked = if entry.yanked {
            format!(" {}", style("(yanked)").red())
        } else {
            String::new()
        };

        console.stdout(format!(
            "  {} {}{}",
            style(&entry.name).cyan().bold(),
            style(&entry.version).green(),
            yanked,
        ));

        if let Some(ref desc) = entry.description {
            console.stdout(format!("    {}", style(desc).dim()));
        }

        if let Some(ref compat) = entry.houdini_compat {
            console.stdout(format!("    Houdini: {}", style(compat).dim()));
        }
    }

    Ok(())
}

/// Emit the package array for `--output json*`. `json` keeps the historical
/// pretty-printed array shape; `json-lines` streams entries one per line.
fn print_packages_json(
    packages: &[hpm_core::registry::RegistryEntry],
    output: OutputFormat,
    console: &mut Console,
) -> Result<()> {
    match output {
        OutputFormat::JsonLines => {
            for package in packages {
                console.stdout(serde_json::to_string(package)?);
            }
        }
        _ => {
            let doc = serde_json::to_value(packages)?;
            console.stdout(output.render_json(&doc));
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
        let result =
            search_packages(&config, "test", &mut Console::new(), OutputFormat::Human).await;
        assert!(result.is_ok());
    }
}
