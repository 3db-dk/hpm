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

    /// Clean only Python virtual environments
    #[arg(long)]
    pub python_only: bool,

    /// Enable comprehensive cleanup including Python virtual environments
    #[arg(long)]
    pub comprehensive: bool,
}

pub async fn execute_clean(args: &CleanArgs) -> anyhow::Result<()> {
    info!("Starting package cleanup");

    // Load configuration from config files (falls back to defaults if no config exists)
    let config = Config::load().unwrap_or_else(|e| {
        tracing::warn!("Failed to load config file, using defaults: {}", e);
        Config::default()
    });

    // Initialize storage manager
    let storage_manager = StorageManager::new(config.storage.clone())?;

    // Handle different cleanup modes
    match (args.python_only, args.comprehensive) {
        (true, true) => Err(anyhow::anyhow!(
            "Cannot specify both --python-only and --comprehensive options"
        )),
        (true, false) => {
            // Python-only cleanup
            execute_python_only_cleanup(&storage_manager, args.dry_run, args.yes).await
        }
        (false, true) => {
            // Comprehensive cleanup (packages + Python)
            execute_comprehensive_cleanup(&storage_manager, &config, args.dry_run, args.yes).await
        }
        (false, false) => {
            // Traditional package-only cleanup
            if args.dry_run {
                execute_dry_run_cleanup(&storage_manager, &config).await
            } else if args.yes {
                execute_automated_cleanup(&storage_manager, &config).await
            } else {
                execute_interactive_cleanup(&storage_manager, &config).await
            }
        }
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

/// Execute Python-only cleanup
async fn execute_python_only_cleanup(
    storage_manager: &StorageManager,
    dry_run: bool,
    automated: bool,
) -> anyhow::Result<()> {
    if dry_run {
        println!("Analyzing Python virtual environments for cleanup (dry run)...");
        let result = storage_manager.cleanup_python_only(true).await?;

        if result.items_that_would_be_cleaned() == 0 {
            println!("No orphaned Python virtual environments found - cleanup not needed");
        } else {
            println!(
                "Found {} orphaned virtual environments that would be removed:",
                result.items_that_would_be_cleaned()
            );
            for venv_path in &result.would_remove {
                println!("  - {:?}", venv_path);
            }
            println!(
                "Would free approximately: {}",
                result.format_space_that_would_be_freed()
            );
            println!();
            println!("Run 'hpm clean --python-only' to remove these virtual environments");
            println!("Run 'hpm clean --python-only --yes' to remove without confirmation");
        }
    } else if automated {
        println!("Removing orphaned Python virtual environments...");
        let result = storage_manager.cleanup_python_only(false).await?;

        if result.items_cleaned() == 0 {
            println!("No orphaned Python virtual environments found - cleanup not needed");
        } else {
            println!(
                "Successfully removed {} orphaned virtual environments:",
                result.items_cleaned()
            );
            for venv_path in &result.removed {
                println!("  - {:?}", venv_path);
            }
            println!("Disk space freed: {}", result.format_space_freed());
        }
    } else {
        // Interactive cleanup
        println!("Analyzing Python virtual environments for cleanup...");
        let result = storage_manager.cleanup_python_only(true).await?;

        if result.items_that_would_be_cleaned() == 0 {
            println!("No orphaned Python virtual environments found - cleanup not needed");
            return Ok(());
        }

        println!(
            "Found {} orphaned virtual environments:",
            result.items_that_would_be_cleaned()
        );
        for venv_path in &result.would_remove {
            println!("  - {:?}", venv_path);
        }
        println!(
            "Would free approximately: {}",
            result.format_space_that_would_be_freed()
        );

        println!();
        print!("Remove these virtual environments? [y/N]: ");
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;

        let response = input.trim().to_lowercase();
        if response == "y" || response == "yes" {
            println!("Removing virtual environments...");
            let result = storage_manager.cleanup_python_only(false).await?;

            println!(
                "Successfully removed {} virtual environments:",
                result.items_cleaned()
            );
            for venv_path in &result.removed {
                println!("  - {:?}", venv_path);
            }
            println!("Disk space freed: {}", result.format_space_freed());
        } else {
            println!("Cleanup cancelled");
        }
    }

    Ok(())
}

/// Execute comprehensive cleanup (packages + Python environments)
async fn execute_comprehensive_cleanup(
    storage_manager: &StorageManager,
    config: &Config,
    dry_run: bool,
    automated: bool,
) -> anyhow::Result<()> {
    if dry_run {
        println!("Analyzing packages and Python environments for cleanup (dry run)...");
        let result = storage_manager
            .cleanup_comprehensive_dry_run(&config.projects)
            .await?;

        if result.total_items_that_would_be_cleaned() == 0 {
            println!("No orphaned packages or virtual environments found - cleanup not needed");
        } else {
            if !result.removed_packages.is_empty() {
                println!(
                    "Found {} orphaned packages that would be removed:",
                    result.removed_packages.len()
                );
                for package in &result.removed_packages {
                    println!("  - {}", package);
                }
            }

            if result.python_cleanup.items_that_would_be_cleaned() > 0 {
                println!(
                    "Found {} orphaned virtual environments that would be removed:",
                    result.python_cleanup.items_that_would_be_cleaned()
                );
                for venv_path in &result.python_cleanup.would_remove {
                    println!("  - {:?}", venv_path);
                }
            }

            println!(
                "Total would free approximately: {}",
                result.format_total_space_that_would_be_freed()
            );
            println!();
            println!("Run 'hpm clean --comprehensive' to remove these items");
            println!("Run 'hpm clean --comprehensive --yes' to remove without confirmation");
        }
    } else if automated {
        println!("Performing comprehensive cleanup (packages + Python environments)...");
        let result = storage_manager
            .cleanup_comprehensive(&config.projects)
            .await?;

        if result.total_items_cleaned() == 0 {
            println!("No orphaned packages or virtual environments found - cleanup not needed");
        } else {
            if !result.removed_packages.is_empty() {
                println!(
                    "Successfully removed {} orphaned packages:",
                    result.removed_packages.len()
                );
                for package in &result.removed_packages {
                    println!("  - {}", package);
                }
            }

            if result.python_cleanup.items_cleaned() > 0 {
                println!(
                    "Successfully removed {} orphaned virtual environments:",
                    result.python_cleanup.items_cleaned()
                );
                for venv_path in &result.python_cleanup.removed {
                    println!("  - {:?}", venv_path);
                }
            }

            println!(
                "Total disk space freed: {}",
                result.format_total_space_freed()
            );
        }
    } else {
        // Interactive cleanup
        println!("Analyzing packages and Python environments for cleanup...");
        let result = storage_manager
            .cleanup_comprehensive_dry_run(&config.projects)
            .await?;

        if result.total_items_that_would_be_cleaned() == 0 {
            println!("No orphaned packages or virtual environments found - cleanup not needed");
            return Ok(());
        }

        if !result.removed_packages.is_empty() {
            println!("Found {} orphaned packages:", result.removed_packages.len());
            for package in &result.removed_packages {
                println!("  - {}", package);
            }
        }

        if result.python_cleanup.items_that_would_be_cleaned() > 0 {
            println!(
                "Found {} orphaned virtual environments:",
                result.python_cleanup.items_that_would_be_cleaned()
            );
            for venv_path in &result.python_cleanup.would_remove {
                println!("  - {:?}", venv_path);
            }
        }

        println!(
            "Total would free approximately: {}",
            result.format_total_space_that_would_be_freed()
        );

        println!();
        print!("Remove these items? [y/N]: ");
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;

        let response = input.trim().to_lowercase();
        if response == "y" || response == "yes" {
            println!("Performing comprehensive cleanup...");
            let result = storage_manager
                .cleanup_comprehensive(&config.projects)
                .await?;

            println!(
                "Successfully cleaned up {} total items:",
                result.total_items_cleaned()
            );
            if !result.removed_packages.is_empty() {
                println!("  Packages ({}): ", result.removed_packages.len());
                for package in &result.removed_packages {
                    println!("    - {}", package);
                }
            }
            if result.python_cleanup.items_cleaned() > 0 {
                println!(
                    "  Virtual Environments ({}): ",
                    result.python_cleanup.items_cleaned()
                );
                for venv_path in &result.python_cleanup.removed {
                    println!("    - {:?}", venv_path);
                }
            }
            println!(
                "Total disk space freed: {}",
                result.format_total_space_freed()
            );
        } else {
            println!("Cleanup cancelled");
        }
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
            python_only: false,
            comprehensive: false,
        };

        assert!(args.dry_run);
        assert!(!args.yes);
        assert!(args.package.is_none());
        assert!(!args.python_only);
        assert!(!args.comprehensive);
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
            python_only: false,
            comprehensive: false,
        };

        assert!(!args.dry_run);
        assert!(args.yes);
        assert_eq!(args.package.as_ref().unwrap().len(), 2);
        assert!(!args.python_only);
        assert!(!args.comprehensive);
    }

    #[test]
    fn clean_args_python_only() {
        let args = CleanArgs {
            dry_run: false,
            yes: false,
            package: None,
            python_only: true,
            comprehensive: false,
        };

        assert!(!args.dry_run);
        assert!(!args.yes);
        assert!(args.package.is_none());
        assert!(args.python_only);
        assert!(!args.comprehensive);
    }

    #[test]
    fn clean_args_comprehensive() {
        let args = CleanArgs {
            dry_run: true,
            yes: false,
            package: None,
            python_only: false,
            comprehensive: true,
        };

        assert!(args.dry_run);
        assert!(!args.yes);
        assert!(args.package.is_none());
        assert!(!args.python_only);
        assert!(args.comprehensive);
    }
}
