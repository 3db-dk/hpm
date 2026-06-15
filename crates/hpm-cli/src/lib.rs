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
//! │  │  ┌─────────┐  ┌─────────┐  ┌──────────┐                                │   │
//! │  │  │hpm-core │  │hpm-pkg  │  │hpm-config│                                │   │
//! │  │  └─────────┘  └─────────┘  └──────────┘                                │   │
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
//! ### Script Execution Commands
//! Commands for running package-defined workflows:
//!
//! - **`run`** - Execute a `[scripts]` entry from `hpm.toml`
//!   - Forwards trailing arguments to the script
//!   - Sets `HPM_PACKAGE_ROOT` to the manifest directory
//!   - Picks the host-matching variant from conditional `cmd` values
//!   - Materializes a uv-managed venv on demand for table-form entries
//!     with `python` / `requirements` set
//!
//! ### Future Commands (Planned)
//! Commands planned for future releases:
//!
//! - **`search`** - Search registry for packages with filtering
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
//! # Create minimal package structure (Houdini 21.x only — the default)
//! hpm init --bare minimal-package
//!
//! # Initialize with custom metadata, widening the Houdini range
//! hpm init advanced-tools \
//!   --description "Advanced geometry manipulation tools" \
//!   --license Apache-2.0 \
//!   --houdini ">=20.5, <22"
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
//! - **HPM Registry**: Package search and download
//! - **HPM Config**: Configuration management and project settings
//! - **HPM Package**: Manifest processing and Houdini integration
use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::{Shell, generate};
use std::collections::HashMap;
use std::process::ExitCode;
use std::time::Instant;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

pub mod commands;
pub mod console;
pub mod error;
pub mod output;
pub mod progress;
use commands::init_package;
pub use console::{ColorChoice, Console, Verbosity};
use error::{CliError, CliResult, CliResultExt, ExitStatus};
pub use output::OutputFormat;
#[derive(Parser)]
#[command(name = "hpm", version, about = "HPM - Houdini Package Manager")]
pub struct Cli {
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
pub enum ColorChoiceArg {
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
pub enum OutputFormatArg {
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
pub enum Commands {
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

        /// `[compat].houdini` Cargo-style range, e.g. `">=20.5"`, `"^21"`,
        /// or `">=20.5, <22"`. Defaults to the template's `>=20.5`.
        #[arg(long = "houdini")]
        houdini: Option<String>,

        /// Create minimal package structure (only hpm.toml)
        #[arg(long)]
        bare: bool,

        /// Initialize version control (git, none)
        #[arg(long, default_value = "git")]
        vcs: String,
    },
    /// Add package dependencies
    Add {
        /// Package name(s) to add (use name@version for specific versions)
        #[arg(value_name = "PACKAGE", required = true)]
        packages: Vec<String>,

        /// Local path to package directory (only with single package)
        #[arg(long)]
        path: Option<std::path::PathBuf>,

        /// Install path dependency as a symlink/junction so working-tree edits
        /// reach a live Houdini session without re-running `hpm sync`.
        /// Requires --path.
        #[arg(long, requires = "path")]
        link: bool,

        /// Path to directory containing hpm.toml or direct path to hpm.toml file
        #[arg(short = 'p', long = "package")]
        manifest: Option<std::path::PathBuf>,

        /// Mark dependencies as optional
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

        /// Display dependencies as a tree
        #[arg(long)]
        tree: bool,
    },
    /// Search for packages
    Search {
        /// Search query
        query: String,
    },
    /// Execute a package script defined in `[scripts]`
    Run {
        /// Script name to execute
        script: String,
        /// Additional arguments forwarded to the script
        #[arg(trailing_var_arg = true)]
        args: Vec<String>,
    },
    /// Install dependencies from hpm.toml
    Install {
        /// Path to hpm.toml file (defaults to current directory)
        #[arg(short, long)]
        manifest: Option<std::path::PathBuf>,

        /// Fail if lock file is missing or would change (for CI reproducibility)
        #[arg(long)]
        frozen_lockfile: bool,
    },
    /// Validate package configuration
    Check,
    /// Migrate a pre-0.16 hpm.toml to the current schema.
    ///
    /// Old-format manifests are still read transparently (with a deprecation
    /// warning) until the format is removed; this rewrites the file so it is
    /// on the current schema. The lossy `[native]` -> `[stage]` conversion is
    /// best-effort and flags derived placement rules for review.
    Migrate {
        /// Path to hpm.toml or its directory (defaults to cwd).
        #[arg(short = 'p', long = "package")]
        manifest: Option<std::path::PathBuf>,
        /// Print the migrated manifest to stdout instead of writing it.
        #[arg(long)]
        stdout: bool,
        /// Only report whether migration is needed (exit non-zero if so);
        /// write nothing.
        #[arg(long)]
        check: bool,
    },
    /// Materialise the install image into a directory (defaults to
    /// `[stage].output_dir`, typically `dist/`).
    ///
    /// Pass `--output <dir>` to stage into a different location — useful
    /// when running multiple Houdini sessions side by side, each pointing
    /// its `HOUDINI_PACKAGE_PATH` at its own freshly built directory.
    Build {
        /// Path to hpm.toml or its directory (defaults to cwd).
        #[arg(short, long)]
        manifest: Option<std::path::PathBuf>,
        /// Override `[stage].output_dir`. Relative paths resolve against
        /// the manifest directory; absolute paths are used verbatim.
        #[arg(short = 'o', long)]
        output: Option<std::path::PathBuf>,
        /// Target platform; defaults to the host platform when
        /// `[compat].platforms` is declared.
        #[arg(long)]
        platform: Option<String>,
        /// Build profile to apply. Selects the matching `[stage.profile.<name>]`
        /// table (if any) and is exposed to prepack scripts as
        /// `HPM_BUILD_PROFILE`. The target platform is exposed as `HPM_PLATFORM`.
        #[arg(long, default_value = "release")]
        profile: String,
        /// Skip `[stage].prepack`. Useful in CI when the build steps already
        /// ran out-of-band.
        #[arg(long)]
        no_prepack: bool,
        /// Keep the existing output directory contents alongside the new
        /// staging output. Default behavior wipes the output dir first to
        /// avoid stale files surviving from a prior platform.
        #[arg(long)]
        no_clean: bool,
    },
    /// Create a distributable package archive
    Pack {
        /// Path to Ed25519 signing key (PKCS#8 PEM). Overrides HPM_SIGNING_KEY env var.
        #[arg(long)]
        key: Option<std::path::PathBuf>,
        /// Output directory for the archive (defaults to current directory)
        #[arg(long)]
        output: Option<std::path::PathBuf>,
        /// Output result as JSON (for CI integration)
        #[arg(long)]
        json: bool,
        /// Target platform (defaults to host platform when `[compat].platforms` is declared)
        #[arg(long)]
        platform: Option<String>,
    },
    /// Run security audit on dependencies
    Audit {
        /// Path to hpm.toml file (defaults to current directory)
        #[arg(short, long)]
        manifest: Option<std::path::PathBuf>,
    },
    /// Clean orphaned packages
    Clean(commands::clean::CleanArgs),
    /// Manage package registries
    Registry {
        #[command(subcommand)]
        action: RegistryAction,
    },
    /// Generate shell completions
    Completions {
        /// Target shell (bash, zsh, fish, powershell, elvish)
        #[arg(value_enum)]
        shell: Shell,
    },
}

#[derive(Subcommand)]
pub enum RegistryAction {
    /// Add a package registry
    Add {
        /// Registry URL (API endpoint or Git remote)
        url: String,
        /// Display name / alias for this registry
        #[arg(long)]
        name: Option<String>,
        /// Registry type: "api" or "git" (auto-detected if not specified)
        #[arg(long = "type")]
        registry_type: Option<String>,
        /// Succeed silently if a registry with the same name already exists,
        /// instead of erroring. Eases idempotent/automated provisioning.
        #[arg(long)]
        if_not_exists: bool,
    },
    /// List configured registries
    List,
    /// Remove a registry by name
    Remove {
        /// Registry name to remove
        name: String,
    },
    /// Update (refresh) all registry caches
    Update,
}

/// Entry point usable by both the `hpm` binary and any external runner.
///
/// Equivalent to the previous `#[tokio::main] async fn main`. Lives in the
/// library so integration tests (`tests/cli_validation.rs`) and embedded
/// hosts can drive the CLI without spawning the binary.
pub async fn run() -> ExitCode {
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
                    // `detail()` carries the full cause chain; `to_string()`
                    // would emit only the category label ("Package error").
                    "error": error.detail(),
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
            houdini,
            bare,
            vcs,
        } => {
            let options = commands::init::InitOptions {
                name_or_path: name.clone(),
                description: description.clone(),
                author: author.clone(),
                version: version.clone(),
                license: license.clone(),
                houdini: houdini.clone(),
                bare: *bare,
                vcs: vcs.clone(),
                base_dir: directory.clone(), // Use directory from CLI flag
            };

            let package_name = init_package(options).await.cli_package("init")?;

            if output_format == OutputFormat::Human {
                console.success(format!(
                    "Package '{}' initialized successfully",
                    package_name
                ));
            }
        }
        Commands::Add {
            packages,
            path,
            link,
            manifest,
            optional,
        } => {
            let config = load_cli_config()?;
            commands::add::add_packages(
                &config,
                packages.clone(),
                path.clone(),
                *link,
                manifest.clone(),
                *optional,
            )
            .await
            .cli_package("add")?;

            if output_format == OutputFormat::Human {
                let msg = if packages.len() == 1 {
                    format!("Added dependency '{}'", packages[0])
                } else {
                    format!("Added {} dependencies", packages.len())
                };
                console.success(msg);
            }
        }
        Commands::Remove { package, manifest } => {
            let config = load_cli_config()?;
            commands::remove::remove_package(&config, package.clone(), manifest.clone())
                .await
                .cli_package("remove")?;

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

            let config = load_cli_config()?;
            commands::update::update_packages(&config, options)
                .await
                .cli_package("update")?;

            if output_format == OutputFormat::Human {
                console.success("Package update completed");
            }
        }
        Commands::List { manifest, tree } => {
            if *tree {
                commands::list::list_dependencies_tree(manifest.clone())
                    .await
                    .cli_package("list")?;
            } else {
                commands::list::list_dependencies(manifest.clone())
                    .await
                    .cli_package("list")?;
            }
        }
        Commands::Search { query } => {
            let json_output = output_format != OutputFormat::Human;
            let config = load_cli_config()?;
            commands::search::search_packages(&config, query.clone(), None, json_output)
                .await
                .cli_network("search")?;
        }
        Commands::Run { script, args } => {
            let exit_code = commands::run::run_script(
                script,
                args,
                directory.clone(),
                &HashMap::new(),
                console,
            )
            .await
            .cli_package("run")?;
            return Ok(if exit_code == 0 {
                ExitStatus::Success
            } else {
                let truncated: u8 = exit_code.try_into().unwrap_or(1);
                ExitStatus::External(truncated)
            });
        }
        Commands::Install {
            manifest,
            frozen_lockfile,
        } => {
            let config = load_cli_config()?;
            commands::install::install_dependencies(&config, manifest.clone(), *frozen_lockfile)
                .await
                .cli_package("install")?;

            if output_format == OutputFormat::Human {
                console.success("Dependencies installed successfully");
            }
        }
        Commands::Check => {
            commands::check::check_package(directory.clone())
                .await
                .cli_package("check")?;

            if output_format == OutputFormat::Human {
                console.success("Package configuration is valid");
            }
        }
        Commands::Migrate {
            manifest,
            stdout,
            check,
        } => {
            let needs_migration =
                commands::migrate::migrate_manifest(manifest.clone(), *stdout, *check, console)
                    .await
                    .cli_package("migrate")?;
            // Under --check, a manifest that still needs migrating is a
            // non-zero exit so CI can gate on it.
            if *check && needs_migration {
                return Ok(ExitStatus::Failure);
            }
        }
        Commands::Build {
            manifest,
            output,
            platform,
            profile,
            no_prepack,
            no_clean,
        } => {
            let options = commands::build::BuildOptions {
                manifest: manifest.clone().or_else(|| directory.clone()),
                output: output.clone(),
                platform: platform.clone(),
                profile: profile.clone(),
                no_prepack: *no_prepack,
                clean: !*no_clean,
            };
            commands::build::build(options, console)
                .await
                .cli_package("build")?;
        }
        Commands::Pack {
            key,
            output,
            json,
            platform,
        } => {
            let config = load_cli_config()?;
            commands::pack::execute(
                &config,
                directory.clone(),
                key.clone(),
                output.clone(),
                *json,
                platform.clone(),
                console,
            )
            .await
            .cli_package("pack")?;
        }
        Commands::Audit { manifest } => {
            let config = load_cli_config()?;
            commands::audit::audit_packages(&config, manifest.clone())
                .await
                .cli_package("audit")?;
        }
        Commands::Registry { action } => match action {
            RegistryAction::Add {
                url,
                name,
                registry_type,
                if_not_exists,
            } => {
                let config = load_cli_config()?;
                commands::registry::add_registry(
                    config,
                    url.clone(),
                    name.clone(),
                    registry_type.clone(),
                    *if_not_exists,
                )
                .await
                .cli_config("registry add")?;
            }
            RegistryAction::List => {
                let config = load_cli_config()?;
                commands::registry::list_registries(&config)
                    .await
                    .map_err(|e| CliError::config(e, None))?;
            }
            RegistryAction::Remove { name } => {
                let config = load_cli_config()?;
                commands::registry::remove_registry(config, name.clone())
                    .await
                    .cli_config("registry remove")?;
            }
            RegistryAction::Update => {
                let config = load_cli_config()?;
                commands::registry::update_registries(&config)
                    .await
                    .map_err(|e| {
                        CliError::network(
                            e,
                            Some("Check your internet connection and registry URLs.".to_string()),
                        )
                    })?;
            }
        },
        Commands::Clean(args) => {
            let config = load_cli_config()?;
            commands::clean::execute_clean(&config, args)
                .await
                .cli_package("clean")?;

            if output_format == OutputFormat::Human {
                console.success("Cleanup completed successfully");
            }
        }
        Commands::Completions { shell } => {
            let mut cmd = Cli::command();
            generate(*shell, &mut cmd, "hpm", &mut std::io::stdout());
            return Ok(ExitStatus::Success);
        }
    }

    Ok(ExitStatus::Success)
}

/// Load HPM config once for a single command invocation.
///
/// Pulled out of each `Commands::*` arm so the same `Config` can be threaded
/// into helpers that previously each called `Config::load()` themselves —
/// `install` used to reload twice, and `add` reloaded once then handed control
/// to `install` which reloaded twice more.
fn load_cli_config() -> CliResult<hpm_config::Config> {
    hpm_config::Config::load().map_err(|e| CliError::config(e, None))
}

fn init_logging(verbosity: Verbosity) {
    let log_level = match verbosity {
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
