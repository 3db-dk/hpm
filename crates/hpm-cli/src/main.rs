//! # HPM CLI - Houdini Package Manager Command Line Interface
//!
//! The HPM CLI provides a comprehensive command-line interface for managing Houdini packages,
//! dependencies, and project workflows.
//!
//! ## Available Commands
//!
//! ### Fully Implemented
//! - `init` - Initialize new Houdini packages with templates
//! - `add` - Add package dependencies with version specifications
//! - `remove` - Remove package dependencies from manifests
//! - `install` - Install dependencies from hpm.toml with Python support
//! - `list` - Display package information and dependencies
//! - `check` - Validate package configuration and Houdini compatibility
//! - `clean` - Project-aware package cleanup with orphan detection
//!
//! ### Planned for Future Implementation
//! - `update` - Update packages to latest versions
//! - `search` - Search registry for packages
//! - `publish` - Publish packages to registry
//! - `run` - Execute package scripts
//!
//! The CLI is built using [clap](https://docs.rs/clap/) for argument parsing and provides
//! comprehensive help information for all commands and options.

use anyhow::Result;
use clap::{Parser, Subcommand};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod commands;
use commands::init_package;

#[derive(Parser)]
#[command(name = "hpm", version, about = "HPM - Houdini Package Manager")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize a new HPM package
    Init {
        /// Package name
        name: Option<String>,

        /// Package description
        #[arg(long)]
        description: Option<String>,

        /// Package author
        #[arg(long)]
        author: Option<String>,

        /// Initial version
        #[arg(long, default_value = "0.1.0")]
        version: String,

        /// License identifier
        #[arg(long, default_value = "MIT")]
        license: String,

        /// Minimum Houdini version
        #[arg(long = "houdini-min")]
        houdini_min: Option<String>,

        /// Maximum Houdini version
        #[arg(long = "houdini-max")]
        houdini_max: Option<String>,

        /// Create minimal package structure (only hpm.toml)
        #[arg(long)]
        bare: bool,

        /// Initialize version control (git, none)
        #[arg(long, default_value = "git")]
        vcs: String,
    },
    /// Add a package dependency
    Add {
        /// Package name to add
        package: String,

        /// Version specification (e.g., "^1.0.0", "latest")
        #[arg(short, long)]
        version: Option<String>,

        /// Path to directory containing hpm.toml or direct path to hpm.toml file
        #[arg(short = 'p', long = "package")]
        manifest: Option<std::path::PathBuf>,

        /// Mark dependency as optional
        #[arg(long)]
        optional: bool,
    },
    /// Remove a package dependency
    Remove {
        /// Package name to remove
        package: String,

        /// Path to directory containing hpm.toml or direct path to hpm.toml file
        #[arg(short = 'p', long = "package")]
        manifest: Option<std::path::PathBuf>,
    },
    /// Update packages
    Update,
    /// Display package information and dependencies
    List {
        /// Path to directory containing hpm.toml or direct path to hpm.toml file
        #[arg(short = 'p', long = "package")]
        manifest: Option<std::path::PathBuf>,
    },
    /// Search for packages
    Search {
        /// Search query
        query: String,
    },
    /// Publish a package
    Publish,
    /// Execute package scripts
    Run {
        /// Script name to execute
        script: String,
        /// Additional arguments to pass to the script
        #[arg(trailing_var_arg = true)]
        args: Vec<String>,
    },
    /// Install dependencies from hpm.toml
    Install {
        /// Path to hpm.toml file (defaults to current directory)
        #[arg(short, long)]
        manifest: Option<std::path::PathBuf>,
    },
    /// Validate package configuration
    Check,
    /// Clean orphaned packages
    Clean(commands::clean::CleanArgs),
}

#[tokio::main]
async fn main() -> Result<()> {
    init_logging();

    let cli = Cli::parse();

    match cli.command {
        Commands::Init {
            name,
            description,
            author,
            version,
            license,
            houdini_min,
            houdini_max,
            bare,
            vcs,
        } => {
            let options = commands::init::InitOptions {
                name,
                description,
                author,
                version,
                license,
                houdini_min,
                houdini_max,
                bare,
                vcs,
                base_dir: None, // Use current working directory for CLI usage
            };

            init_package(options).await?;
        }
        Commands::Add {
            package,
            version,
            manifest,
            optional,
        } => {
            commands::add::add_package(package, version, manifest, optional).await?;
        }
        Commands::Remove { package, manifest } => {
            commands::remove::remove_package(package, manifest).await?;
        }
        Commands::Update => {
            println!("Update command not yet implemented");
            println!("   This feature is planned for a future release");
        }
        Commands::List { manifest } => {
            commands::list::list_dependencies(manifest).await?;
        }
        Commands::Search { query: _ } => {
            println!("Search command not yet implemented");
            println!("   This feature is planned for a future release");
        }
        Commands::Publish => {
            println!("Publish command not yet implemented");
            println!("   This feature is planned for a future release");
        }
        Commands::Run { script, args: _ } => {
            println!("Run command not yet implemented");
            println!(
                "   Script '{}' execution is planned for a future release",
                script
            );
        }
        Commands::Install { manifest } => {
            commands::install::install_dependencies(manifest).await?;
        }
        Commands::Check => {
            commands::check::check_package().await?;
        }
        Commands::Clean(args) => {
            commands::clean::execute_clean(&args).await?;
        }
    }

    Ok(())
}

fn init_logging() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "hpm=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();
}
