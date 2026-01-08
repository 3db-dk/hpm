//! Search command implementation.
//!
//! Note: With Git archive-based dependencies, package search is no longer needed.
//! Packages are discovered by browsing Git repositories directly.

use anyhow::Result;
use console::style;

/// Execute the search command
///
/// With Git archive-based dependencies, this command is deprecated.
/// Users should browse Git repositories directly to discover packages.
pub async fn search_packages(query: String, _limit: Option<u32>, _json_output: bool) -> Result<()> {
    println!(
        "{} HPM uses Git archive-based dependencies.",
        style("Note:").yellow().bold()
    );
    println!();
    println!(
        "Package search is not available. To find packages, browse Git repositories directly."
    );
    println!();
    println!("To add a package, use:");
    println!(
        "  {} {} {} {}",
        style("hpm add").cyan(),
        style("--git").green(),
        style("<repository-url>").dim(),
        style("--commit <commit-hash>").dim()
    );
    println!();
    println!("Example:");
    println!(
        "  {} {} https://github.com/studio/geometry-tools {}",
        style("hpm add").cyan(),
        style("--git").green(),
        style("--commit abc123").dim()
    );
    println!();

    if !query.is_empty() {
        println!(
            "{}",
            style(format!(
                "Tip: Search for '{}' on GitHub, GitLab, or your preferred Git hosting platform.",
                query
            ))
            .dim()
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_search_command_runs() {
        // Just verify the command doesn't panic
        let result = search_packages("test".to_string(), None, false).await;
        assert!(result.is_ok());
    }
}
