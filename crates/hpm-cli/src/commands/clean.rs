use clap::Args;
use hpm_config::Config;
use hpm_core::StorageManager;
use std::io::{self, Write};
use tracing::info;

#[derive(Args, Debug)]
pub struct CleanArgs {
    /// Perform a dry run without actually removing packages
    #[arg(long, short = 'n')]
    pub dry_run: bool,

    /// Remove packages without asking for confirmation
    #[arg(long, short)]
    pub yes: bool,

    /// Target specific package patterns
    #[arg(long)]
    pub package: Option<Vec<String>>,
}

pub async fn execute_clean(args: &CleanArgs) -> anyhow::Result<()> {
    info!("Starting package cleanup");

    // Load configuration
    let config = Config::default(); // TODO: Load from actual config file

    // Initialize storage manager
    let storage_manager = StorageManager::new(config.storage.clone())?;

    // Perform cleanup based on arguments
    if args.dry_run {
        execute_dry_run_cleanup(&storage_manager, &config).await
    } else if args.yes {
        execute_automated_cleanup(&storage_manager, &config).await
    } else {
        execute_interactive_cleanup(&storage_manager, &config).await
    }
}

async fn execute_dry_run_cleanup(
    storage_manager: &StorageManager,
    config: &Config,
) -> anyhow::Result<()> {
    println!("Analyzing packages for cleanup (dry run)...");

    let would_remove = storage_manager
        .cleanup_unused_dry_run(&config.projects)
        .await?;

    if would_remove.is_empty() {
        println!("No orphaned packages found - cleanup not needed");
    } else {
        println!(
            "Found {} orphaned packages that would be removed:",
            would_remove.len()
        );
        for package in &would_remove {
            println!("  - {}", package);
        }
        println!();
        println!("Run 'hpm clean' to remove these packages");
        println!("Run 'hpm clean --yes' to remove without confirmation");
    }

    Ok(())
}

async fn execute_automated_cleanup(
    storage_manager: &StorageManager,
    config: &Config,
) -> anyhow::Result<()> {
    println!("Removing orphaned packages...");

    let removed = storage_manager.cleanup_unused(&config.projects).await?;

    if removed.is_empty() {
        println!("No orphaned packages found - cleanup not needed");
    } else {
        println!("Successfully removed {} orphaned packages:", removed.len());
        for package in &removed {
            println!("  - {}", package);
        }

        // Calculate approximate space saved (rough estimate)
        let estimated_space_mb = removed.len() * 10; // Rough estimate of 10MB per package
        println!();
        println!("Estimated disk space freed: ~{}MB", estimated_space_mb);
    }

    Ok(())
}

async fn execute_interactive_cleanup(
    storage_manager: &StorageManager,
    config: &Config,
) -> anyhow::Result<()> {
    println!("Analyzing packages for cleanup...");

    let would_remove = storage_manager
        .cleanup_unused_dry_run(&config.projects)
        .await?;

    if would_remove.is_empty() {
        println!("No orphaned packages found - cleanup not needed");
        return Ok(());
    }

    println!("Found {} orphaned packages:", would_remove.len());
    for package in &would_remove {
        println!("  - {}", package);
    }

    println!();
    print!("Remove these packages? [y/N]: ");
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;

    let response = input.trim().to_lowercase();
    if response == "y" || response == "yes" {
        println!("Removing packages...");

        let removed = storage_manager.cleanup_unused(&config.projects).await?;

        println!("Successfully removed {} packages:", removed.len());
        for package in &removed {
            println!("  - {}", package);
        }

        let estimated_space_mb = removed.len() * 10;
        println!();
        println!("Estimated disk space freed: ~{}MB", estimated_space_mb);
    } else {
        println!("Cleanup cancelled");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_args_parsing() {
        let args = CleanArgs {
            dry_run: true,
            yes: false,
            package: None,
        };

        assert!(args.dry_run);
        assert!(!args.yes);
        assert!(args.package.is_none());
    }

    #[test]
    fn clean_args_with_packages() {
        let args = CleanArgs {
            dry_run: false,
            yes: true,
            package: Some(vec![
                "test-package".to_string(),
                "another-package".to_string(),
            ]),
        };

        assert!(!args.dry_run);
        assert!(args.yes);
        assert_eq!(args.package.as_ref().unwrap().len(), 2);
    }
}
