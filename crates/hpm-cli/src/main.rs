//! # HPM CLI - Houdini Package Manager Command Line Interface
//!
//! The HPM CLI provides a comprehensive, professional command-line interface for managing
//! Houdini packages, dependencies, and project workflows. Built with industry-standard
//! patterns and UV-inspired error handling for an optimal developer experience.
//!
//! ## CLI Architecture
//!
//! The HPM CLI implements a modular, extensible architecture designed for reliability and user experience:
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────────────┐
//! │                              HPM CLI Architecture                               │
//! ├─────────────────────────────────────────────────────────────────────────────────┤
//! │                                                                                 │
//! │  User Interface Layer                                                          │
//! │  ┌─────────────────────────────────────────────────────────────────────────┐   │
//! │  │                         Command Parser (Clap)                          │   │
//! │  │  • Argument validation and type conversion                              │   │
//! │  │  • Help generation and usage information                                │   │
//! │  │  • Subcommand routing and option handling                               │   │
//! │  └─────────────────────────────────────────────────────────────────────────┘   │
//! │                                    │                                           │
//! │                                    ▼                                           │
//! │  Output & Console Management                                                   │
//! │  ┌─────────────────────┐              ┌─────────────────────────────────────┐  │
//! │  │   Console System    │              │        Output Formats              │  │
//! │  │ • Styled output     │              │ • Human-readable (colors, icons)   │  │
//! │  │ • Color management  │ ────────────▶│ • JSON (machine-readable)          │  │
//! │  │ • Verbosity levels  │              │ • JSON Lines (streaming)           │  │
//! │  └─────────────────────┘              └─────────────────────────────────────┘  │
//! │                                    │                                           │
//! │                                    ▼                                           │
//! │  Error Handling & Reporting                                                    │
//! │  ┌─────────────────────────────────────────────────────────────────────────┐   │
//! │  │                     Structured Error System                            │   │
//! │  │  • Domain-specific error types (Config, Package, Network, etc.)        │   │
//! │  │  • Contextual help and suggestions                                     │   │
//! │  │  • Standardized exit codes                                             │   │
//! │  │  • Machine-readable error formats                                      │   │
//! │  └─────────────────────────────────────────────────────────────────────────┘   │
//! │                                    │                                           │
//! │                                    ▼                                           │
//! │  Command Implementation Layer                                                   │
//! │  ┌─────────────────────────────────────────────────────────────────────────┐   │
//! │  │  init   add   remove   install   list   clean   update   check   ...   │   │
//! │  │   │      │      │        │       │      │       │       │               │   │
//! │  │   ▼      ▼      ▼        ▼       ▼      ▼       ▼       ▼               │   │
//! │  │                  Integration with Core Modules                          │   │
//! │  │  ┌─────────┐  ┌─────────┐  ┌─────────┐  ┌─────────┐  ┌─────────┐       │   │
//! │  │  │hpm-core │  │hmp-pkg  │  │hpm-python│ │hpm-config│ │hpm-registry    │       │
//! │  │  └─────────┘  └─────────┘  └─────────┘  └─────────┘  └─────────┘       │   │
//! │  └─────────────────────────────────────────────────────────────────────────┘   │
//! └─────────────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Command Categories
//!
//! ### Project Management Commands
//! Commands for creating and managing Houdini package projects:
//!
//! - **`init`** - Initialize new Houdini packages with standardized templates
//!   - Standard template: Full package structure with all directories
//!   - Bare template: Minimal structure with only `hpm.toml`
//!   - Version control integration (Git initialization)
//!   - Configurable metadata (author, license, Houdini versions)
//!
//! - **`check`** - Validate package configuration and Houdini compatibility
//!   - Manifest validation (syntax, required fields, version constraints)
//!   - Houdini version compatibility checking
//!   - Dependency constraint validation
//!   - Package structure verification
//!
//! ### Dependency Management Commands
//! Commands for managing package dependencies and installations:
//!
//! - **`add`** - Add package dependencies with semantic versioning
//!   - Automatic dependency resolution and installation
//!   - Version specification support (^, ~, >=, exact)
//!   - Optional dependency marking
//!   - Flexible manifest targeting
//!
//! - **`remove`** - Remove package dependencies from manifests
//!   - Non-destructive removal (preserves downloaded packages)
//!   - Lock file synchronization
//!   - Validation and error handling
//!
//! - **`install`** - Install dependencies from `hpm.toml` manifests
//!   - HPM package dependency resolution
//!   - Python dependency management with virtual environments
//!   - Project structure setup and integration
//!   - Lock file generation and validation
//!
//! - **`update`** - Update packages to latest compatible versions
//!   - Intelligent dependency resolution with conflict detection
//!   - Dry-run mode for preview
//!   - Selective package updates
//!   - Multiple output formats for automation
//!
//! ### Information and Analysis Commands
//! Commands for inspecting packages and dependencies:
//!
//! - **`list`** - Display comprehensive package information
//!   - Package metadata (name, version, description, compatibility)
//!   - HPM dependency specifications with version constraints
//!   - Python dependency specifications with extras
//!   - Optional dependency indicators
//!
//! ### Maintenance Commands
//! Commands for system maintenance and optimization:
//!
//! - **`clean`** - Project-aware package cleanup with safety guarantees
//!   - Orphaned package detection and removal
//!   - Python virtual environment cleanup
//!   - Comprehensive cleanup (packages + Python environments)
//!   - Dry-run mode with detailed preview
//!   - Interactive confirmation for safety
//!
//! ### Future Commands (Planned)
//! Commands planned for future releases:
//!
//! - **`search`** - Search registry for packages with filtering
//! - **`publish`** - Publish packages to registry with validation
//! - **`run`** - Execute package scripts and workflows
//!
//! ## Error Handling Philosophy
//!
//! HPM CLI implements professional error handling inspired by UV's approach:
//!
//! ### Structured Error Types
//! ```rust
//! pub enum CliError {
//!     Config { source: anyhow::Error, help: Option<String> },    // Configuration issues
//!     Package { source: anyhow::Error, help: Option<String> },   // Package operation failures
//!     Network { source: anyhow::Error, help: Option<String> },   // Registry connectivity issues
//!     Io { source: anyhow::Error, help: Option<String> },        // File system operations
//!     Internal { source: anyhow::Error, help: Option<String> },  // Unexpected errors
//!     External { source: anyhow::Error, help: Option<String> },  // External command failures
//! }
//! ```
//!
//! ### Exit Code Standards
//! - **0**: Success - command completed successfully
//! - **1**: User error - configuration, input, or package issues
//! - **2**: Internal error - bugs or unexpected conditions
//! - **N**: External command exit code (when running external tools)
//!
//! ### User Experience Features
//! - **Contextual Help**: Error messages include suggested solutions
//! - **Progressive Verbosity**: More details available with `-v` flags
//! - **Color-Coded Output**: Success (green), warnings (yellow), errors (red)
//! - **Accessibility**: Symbols alongside colors for color-blind users
//!
//! ## Output Format Support
//!
//! HPM CLI supports multiple output formats for different use cases:
//!
//! ### Human-Readable (Default)
//! Styled output with colors, symbols, and formatting optimized for terminal use:
//! ```text
//! [SUCCESS] Package 'geometry-tools' initialized successfully
//! [WARNING] Warning: No Python dependencies specified
//! [ERROR] Error: Package 'nonexistent-package' not found
//! ```
//!
//! ### Machine-Readable Formats
//! Structured output for automation and integration:
//!
//! - **JSON**: Pretty-printed for human-readable automation
//! - **JSON Lines**: Single-line JSON for streaming and log processing
//! - **JSON Compact**: Minimal JSON for bandwidth efficiency
//!
//! ```json
//! {
//!   "success": true,
//!   "command": "install",
//!   "message": "3 packages installed",
//!   "elapsed_ms": 1250
//! }
//! ```
//!
//! ## Usage Examples
//!
//! ### Project Initialization
//! ```bash
//! # Create standard package with full structure
//! hpm init my-houdini-tools --author "Artist <artist@studio.com>"
//!
//! # Create minimal package structure
//! hpm init --bare minimal-package --houdini-min 20.0
//!
//! # Initialize with custom metadata
//! hpm init advanced-tools \
//!   --description "Advanced geometry manipulation tools" \
//!   --license Apache-2.0 \
//!   --houdini-min 19.5 \
//!   --houdini-max 21.0
//! ```
//!
//! ### Dependency Management
//! ```bash
//! # Add latest version of a package
//! hpm add utility-nodes
//!
//! # Add specific version with constraints
//! hpm add geometry-tools --version "^2.1.0"
//!
//! # Add optional dependency
//! hpm add material-library --version "1.5.0" --optional
//!
//! # Remove dependency
//! hpm remove old-package
//!
//! # Install all dependencies
//! hpm install
//! ```
//!
//! ### Package Updates
//! ```bash
//! # Preview available updates
//! hpm update --dry-run
//!
//! # Update all packages
//! hpm update
//!
//! # Update specific packages
//! hpm update numpy geometry-tools
//!
//! # Automated update with JSON output
//! hpm update --yes --output json
//! ```
//!
//! ### System Maintenance
//! ```bash
//! # Preview cleanup operations
//! hpm clean --dry-run
//!
//! # Clean orphaned packages interactively
//! hpm clean
//!
//! # Automated comprehensive cleanup
//! hpm clean --comprehensive --yes
//!
//! # Clean only Python virtual environments
//! hpm clean --python-only --dry-run
//! ```
//!
//! ### Information and Analysis
//! ```bash
//! # List dependencies from current project
//! hpm list
//!
//! # List dependencies from specific project
//! hpm list --package /path/to/project/
//!
//! # Validate package configuration
//! hpm check
//!
//! # Check specific project
//! hpm check --package /path/to/project/
//! ```
//!
//! ## Global Options
//!
//! All commands support these global options for consistent behavior:
//!
//! - **`-v, --verbose`**: Increase verbosity (can be used multiple times)
//! - **`-q, --quiet`**: Suppress output except for errors
//! - **`--color <WHEN>`**: Control color output (auto, always, never)
//! - **`--output <FORMAT>`**: Set output format (human, json, json-lines, json-compact)
//! - **`-C, --directory <DIR>`**: Run command in specified directory
//!
//! ## Integration with Core Systems
//!
//! The CLI seamlessly integrates with all HPM subsystems:
//!
//! - **HPM Core**: Package storage, project discovery, dependency analysis
//! - **HPM Python**: Python dependency management and virtual environments
//! - **HPM Registry**: Package search, download, and publishing (planned)
//! - **HPM Config**: Configuration management and project settings
//! - **HPM Package**: Manifest processing and Houdini integration

use clap::{Parser, Subcommand};
use std::process::ExitCode;
use std::time::Instant;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod commands;
mod console;
mod error;
mod output;

#[cfg(test)]
mod cli_validation_tests;

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

    /// Directory to run command in (defaults to current directory)
    #[arg(short = 'C', long)]
    directory: Option<std::path::PathBuf>,

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
    /// Update packages to latest versions
    Update {
        /// Only update specific packages
        #[arg(value_name = "PACKAGE")]
        packages: Vec<String>,

        /// Path to directory containing hpm.toml or direct path to hpm.toml file
        #[arg(short = 'p', long = "package")]
        manifest: Option<std::path::PathBuf>,

        /// Preview changes without applying them
        #[arg(long)]
        dry_run: bool,

        /// Skip confirmation prompts
        #[arg(short, long)]
        yes: bool,
    },
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
    let result = match run_command(&cli.command, &mut console, output_format, cli.directory).await {
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
    directory: Option<std::path::PathBuf>,
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
                base_dir: directory.clone(), // Use directory from CLI flag
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
        Commands::Update {
            packages,
            manifest,
            dry_run,
            yes,
        } => {
            let options = commands::update::UpdateOptions {
                package: manifest.clone(),
                packages: packages.clone(),
                dry_run: *dry_run,
                yes: *yes,
                output: output_format,
            };

            commands::update::update_packages(options)
                .await
                .map_err(|e| {
                    CliError::package(
                        e,
                        Some("Use 'hpm update --help' for usage information".to_string()),
                    )
                })?;

            if output_format == OutputFormat::Human {
                console.success("Package update completed");
            }
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
            commands::check::check_package(directory.clone())
                .await
                .map_err(|e| {
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
