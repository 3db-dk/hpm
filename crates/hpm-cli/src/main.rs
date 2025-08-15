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

use clap::{Parser, Subcommand};
use std::process::ExitCode;
use std::time::Instant;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod commands;
mod console;
mod error;
mod output;

use commands::init_package;
use console::{ColorChoice, Console, Verbosity};
use error::{CliError, CliResult, ExitStatus};
use output::OutputFormat;

#[derive(Parser)]
#[command(name = "hpm", version, about = "HPM - Houdini Package Manager")]
struct Cli {
    /// Verbosity level
    #[arg(short, long, action = clap::ArgAction::Count)]
    verbose: u8,

    /// Suppress output
    #[arg(short, long)]
    quiet: bool,

    /// Force colors
    #[arg(long, value_enum)]
    color: Option<ColorChoiceArg>,

    /// Output format
    #[arg(long, value_enum)]
    output: Option<OutputFormatArg>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(clap::ValueEnum, Clone, Debug)]
enum ColorChoiceArg {
    Auto,
    Always,
    Never,
}

impl From<ColorChoiceArg> for ColorChoice {
    fn from(choice: ColorChoiceArg) -> Self {
        match choice {
            ColorChoiceArg::Auto => ColorChoice::Auto,
            ColorChoiceArg::Always => ColorChoice::Always,
            ColorChoiceArg::Never => ColorChoice::Never,
        }
    }
}

#[derive(clap::ValueEnum, Clone, Debug)]
enum OutputFormatArg {
    Human,
    Json,
    JsonLines,
    JsonCompact,
}

impl From<OutputFormatArg> for OutputFormat {
    fn from(format: OutputFormatArg) -> Self {
        match format {
            OutputFormatArg::Human => OutputFormat::Human,
            OutputFormatArg::Json => OutputFormat::Json,
            OutputFormatArg::JsonLines => OutputFormat::JsonLines,
            OutputFormatArg::JsonCompact => OutputFormat::JsonCompact,
        }
    }
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
async fn main() -> ExitCode {
    let start_time = Instant::now();
    let cli = Cli::parse();

    // Set up console output
    let verbosity = if cli.quiet {
        Verbosity::Quiet
    } else {
        match cli.verbose {
            0 => Verbosity::Normal,
            1 => Verbosity::Verbose,
            _ => Verbosity::Verbose,
        }
    };

    let color_choice = cli
        .color
        .map(ColorChoice::from)
        .unwrap_or(ColorChoice::Auto);
    let output_format = cli
        .output
        .map(OutputFormat::from)
        .unwrap_or(OutputFormat::Human);

    let mut console = Console::with_settings(verbosity, color_choice);

    // Initialize logging based on verbosity
    init_logging(verbosity);

    // Execute command and handle errors
    let result = match run_command(&cli.command, &mut console, output_format).await {
        Ok(status) => status,
        Err(error) => {
            // Print the error using our structured error system
            if output_format == OutputFormat::Human {
                if verbosity >= Verbosity::Verbose {
                    error.print_error();
                } else {
                    error.print_simple();
                }
            } else {
                // For machine-readable formats, print JSON error
                let error_json = serde_json::json!({
                    "success": false,
                    "error": error.to_string(),
                    "error_type": match &error {
                        CliError::Config { .. } => "config",
                        CliError::Package { .. } => "package",
                        CliError::Network { .. } => "network",
                        CliError::Io { .. } => "io",
                        CliError::Internal { .. } => "internal",
                        CliError::External { .. } => "external",
                    },
                    "elapsed_ms": start_time.elapsed().as_millis()
                });
                eprintln!("{}", serde_json::to_string_pretty(&error_json).unwrap());
            }
            ExitStatus::from(&error)
        }
    };

    result.into()
}

async fn run_command(
    command: &Commands,
    console: &mut Console,
    output_format: OutputFormat,
) -> CliResult<ExitStatus> {
    match command {
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
                name: name.clone(),
                description: description.clone(),
                author: author.clone(),
                version: version.clone(),
                license: license.clone(),
                houdini_min: houdini_min.clone(),
                houdini_max: houdini_max.clone(),
                bare: *bare,
                vcs: vcs.clone(),
                base_dir: None, // Use current working directory for CLI usage
            };

            init_package(options).await.map_err(|e| {
                CliError::package(
                    e,
                    Some("Try 'hpm init --help' for usage information".to_string()),
                )
            })?;

            if output_format == OutputFormat::Human {
                console.success(format!(
                    "Package '{}' initialized successfully",
                    name.as_deref().unwrap_or("new-package")
                ));
            }
        }
        Commands::Add {
            package,
            version,
            manifest,
            optional,
        } => {
            commands::add::add_package(
                package.clone(),
                version.clone(),
                manifest.clone(),
                *optional,
            )
            .await
            .map_err(|e| {
                CliError::package(
                    e,
                    Some("Use 'hpm add --help' for usage information".to_string()),
                )
            })?;

            if output_format == OutputFormat::Human {
                console.success(format!("Added dependency '{}'", package));
            }
        }
        Commands::Remove { package, manifest } => {
            commands::remove::remove_package(package.clone(), manifest.clone())
                .await
                .map_err(|e| {
                    CliError::package(
                        e,
                        Some("Use 'hpm remove --help' for usage information".to_string()),
                    )
                })?;

            if output_format == OutputFormat::Human {
                console.success(format!("Removed dependency '{}'", package));
            }
        }
        Commands::Update => {
            console.warn("Update command not yet implemented");
            console.info("This feature is planned for a future release");
        }
        Commands::List { manifest } => {
            commands::list::list_dependencies(manifest.clone())
                .await
                .map_err(|e| {
                    CliError::package(
                        e,
                        Some("Use 'hpm list --help' for usage information".to_string()),
                    )
                })?;
        }
        Commands::Search { query: _ } => {
            console.warn("Search command not yet implemented");
            console.info("This feature is planned for a future release");
        }
        Commands::Publish => {
            console.warn("Publish command not yet implemented");
            console.info("This feature is planned for a future release");
        }
        Commands::Run { script, args: _ } => {
            console.warn("Run command not yet implemented");
            console.info(format!(
                "Script '{}' execution is planned for a future release",
                script
            ));
        }
        Commands::Install { manifest } => {
            commands::install::install_dependencies(manifest.clone())
                .await
                .map_err(|e| {
                    CliError::package(
                        e,
                        Some("Use 'hpm install --help' for usage information".to_string()),
                    )
                })?;

            if output_format == OutputFormat::Human {
                console.success("Dependencies installed successfully");
            }
        }
        Commands::Check => {
            commands::check::check_package().await.map_err(|e| {
                CliError::package(
                    e,
                    Some("Use 'hpm check --help' for usage information".to_string()),
                )
            })?;

            if output_format == OutputFormat::Human {
                console.success("Package configuration is valid");
            }
        }
        Commands::Clean(args) => {
            commands::clean::execute_clean(args).await.map_err(|e| {
                CliError::package(
                    e,
                    Some("Use 'hpm clean --help' for usage information".to_string()),
                )
            })?;

            if output_format == OutputFormat::Human {
                console.success("Cleanup completed successfully");
            }
        }
    }

    Ok(ExitStatus::Success)
}

fn init_logging(verbosity: Verbosity) {
    let log_level = match verbosity {
        Verbosity::Silent => "hpm=error",
        Verbosity::Quiet => "hpm=warn",
        Verbosity::Normal => "hpm=info",
        Verbosity::Verbose => "hpm=debug",
    };

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| log_level.into()),
        )
        .with(tracing_subscriber::fmt::layer().with_target(false))
        .init();
}
