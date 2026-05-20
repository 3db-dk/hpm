use anyhow::{Result, bail};
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

    /// Clean only Python virtual environments
    #[arg(long)]
    pub python_only: bool,

    /// Enable comprehensive cleanup including Python virtual environments
    #[arg(long)]
    pub comprehensive: bool,
}

/// Cleanup scope — packages, Python venvs, or both.
#[derive(Copy, Clone)]
enum Scope {
    Packages,
    Python,
    Comprehensive,
}

/// How the cleanup interacts with the user.
#[derive(Copy, Clone)]
enum Mode {
    /// Print what would be removed; never delete.
    DryRun,
    /// Delete without confirmation.
    Automated,
    /// Print findings, prompt, then delete (or cancel).
    Interactive,
}

impl Mode {
    fn from_flags(dry_run: bool, yes: bool) -> Self {
        if dry_run {
            Self::DryRun
        } else if yes {
            Self::Automated
        } else {
            Self::Interactive
        }
    }
}

pub async fn execute_clean(config: &Config, args: &CleanArgs) -> Result<()> {
    info!("Starting package cleanup");

    let storage = StorageManager::new(config.storage.clone())?;
    let mode = Mode::from_flags(args.dry_run, args.yes);
    let scope = match (args.python_only, args.comprehensive) {
        (true, true) => bail!("Cannot specify both --python-only and --comprehensive options"),
        (true, false) => Scope::Python,
        (false, true) => Scope::Comprehensive,
        (false, false) => Scope::Packages,
    };

    match scope {
        Scope::Packages => cleanup_packages(&storage, config, mode).await,
        Scope::Python => cleanup_python(&storage, mode).await,
        Scope::Comprehensive => cleanup_comprehensive(&storage, config, mode).await,
    }
}

/// Prompt the user with `[y/N]: <label>`. Returns true on `y`/`yes`.
fn prompt_yes_no(label: &str) -> Result<bool> {
    println!();
    print!("{label} [y/N]: ");
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let response = input.trim().to_lowercase();
    Ok(response == "y" || response == "yes")
}

async fn cleanup_packages(storage: &StorageManager, config: &Config, mode: Mode) -> Result<()> {
    let would_remove_cas = storage.cleanup_unused_dry_run(&config.projects).await?;
    let would_remove_dev = storage
        .cleanup_unused_dev_installs_dry_run(&config.projects)
        .await?;
    if would_remove_cas.is_empty() && would_remove_dev.is_empty() {
        println!("No orphaned packages found - cleanup not needed");
        return Ok(());
    }

    if !would_remove_cas.is_empty() {
        println!("Found {} orphaned packages:", would_remove_cas.len());
        for package in &would_remove_cas {
            println!("  - {package}");
        }
    }
    if !would_remove_dev.is_empty() {
        println!("Found {} orphaned dev installs:", would_remove_dev.len());
        for entry in &would_remove_dev {
            println!("  - {entry}");
        }
    }

    match mode {
        Mode::DryRun => {
            println!();
            println!("Run 'hpm clean' to remove these packages");
            println!("Run 'hpm clean --yes' to remove without confirmation");
            Ok(())
        }
        Mode::Interactive if !prompt_yes_no("Remove these packages?")? => {
            println!("Cleanup cancelled");
            Ok(())
        }
        Mode::Automated | Mode::Interactive => {
            let removed_cas = storage.cleanup_unused(&config.projects).await?;
            let removed_dev = storage
                .cleanup_unused_dev_installs(&config.projects)
                .await?;
            if !removed_cas.is_empty() {
                println!(
                    "Successfully removed {} orphaned packages:",
                    removed_cas.len()
                );
                for package in &removed_cas {
                    println!("  - {package}");
                }
            }
            if !removed_dev.is_empty() {
                println!(
                    "Successfully removed {} orphaned dev installs:",
                    removed_dev.len()
                );
                for entry in &removed_dev {
                    println!("  - {entry}");
                }
            }
            Ok(())
        }
    }
}

async fn cleanup_python(storage: &StorageManager, mode: Mode) -> Result<()> {
    let analysis = storage.cleanup_python_only(true).await?;
    if analysis.items_that_would_be_cleaned() == 0 {
        println!("No orphaned Python virtual environments found - cleanup not needed");
        return Ok(());
    }

    println!(
        "Found {} orphaned virtual environments:",
        analysis.items_that_would_be_cleaned()
    );
    for venv in &analysis.would_remove {
        println!("  - {}", venv.display());
    }
    println!(
        "Would free approximately: {}",
        analysis.format_space_that_would_be_freed()
    );

    match mode {
        Mode::DryRun => {
            println!();
            println!("Run 'hpm clean --python-only' to remove these virtual environments");
            println!("Run 'hpm clean --python-only --yes' to remove without confirmation");
            Ok(())
        }
        Mode::Interactive if !prompt_yes_no("Remove these virtual environments?")? => {
            println!("Cleanup cancelled");
            Ok(())
        }
        Mode::Automated | Mode::Interactive => {
            let result = storage.cleanup_python_only(false).await?;
            println!(
                "Successfully removed {} virtual environments:",
                result.items_cleaned()
            );
            for venv in &result.removed {
                println!("  - {}", venv.display());
            }
            println!("Disk space freed: {}", result.format_space_freed());
            Ok(())
        }
    }
}

async fn cleanup_comprehensive(
    storage: &StorageManager,
    config: &Config,
    mode: Mode,
) -> Result<()> {
    let analysis = storage
        .cleanup_comprehensive(&config.projects, true)
        .await?;
    if analysis.total_items_that_would_be_cleaned() == 0 {
        println!("No orphaned packages or virtual environments found - cleanup not needed");
        return Ok(());
    }

    if !analysis.removed_packages.is_empty() {
        println!(
            "Found {} orphaned packages:",
            analysis.removed_packages.len()
        );
        for package in &analysis.removed_packages {
            println!("  - {package}");
        }
    }
    if !analysis.removed_dev_installs.is_empty() {
        println!(
            "Found {} orphaned dev installs:",
            analysis.removed_dev_installs.len()
        );
        for entry in &analysis.removed_dev_installs {
            println!("  - {entry}");
        }
    }
    if analysis.python_cleanup.items_that_would_be_cleaned() > 0 {
        println!(
            "Found {} orphaned virtual environments:",
            analysis.python_cleanup.items_that_would_be_cleaned()
        );
        for venv in &analysis.python_cleanup.would_remove {
            println!("  - {}", venv.display());
        }
    }

    match mode {
        Mode::DryRun => {
            println!();
            println!("Run 'hpm clean --comprehensive' to remove these items");
            println!("Run 'hpm clean --comprehensive --yes' to remove without confirmation");
            Ok(())
        }
        Mode::Interactive if !prompt_yes_no("Remove these items?")? => {
            println!("Cleanup cancelled");
            Ok(())
        }
        Mode::Automated | Mode::Interactive => {
            let result = storage
                .cleanup_comprehensive(&config.projects, false)
                .await?;
            if !result.removed_packages.is_empty() {
                println!(
                    "Successfully removed {} orphaned packages:",
                    result.removed_packages.len()
                );
                for package in &result.removed_packages {
                    println!("  - {package}");
                }
            }
            if !result.removed_dev_installs.is_empty() {
                println!(
                    "Successfully removed {} orphaned dev installs:",
                    result.removed_dev_installs.len()
                );
                for entry in &result.removed_dev_installs {
                    println!("  - {entry}");
                }
            }
            if result.python_cleanup.items_cleaned() > 0 {
                println!(
                    "Successfully removed {} orphaned virtual environments:",
                    result.python_cleanup.items_cleaned()
                );
                for venv in &result.python_cleanup.removed {
                    println!("  - {}", venv.display());
                }
                println!(
                    "Disk space freed: {}",
                    result.python_cleanup.format_space_freed()
                );
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_from_flags_picks_dry_run_first() {
        assert!(matches!(Mode::from_flags(true, false), Mode::DryRun));
        // dry_run wins even if --yes is also set — never delete in a dry run.
        assert!(matches!(Mode::from_flags(true, true), Mode::DryRun));
    }

    #[test]
    fn mode_from_flags_yes_implies_automated() {
        assert!(matches!(Mode::from_flags(false, true), Mode::Automated));
    }

    #[test]
    fn mode_from_flags_defaults_to_interactive() {
        assert!(matches!(Mode::from_flags(false, false), Mode::Interactive));
    }
}
