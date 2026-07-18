//! HPM command-line interface.
//!
//! Layout:
//! - [`Cli`] / [`Commands`]: clap definitions for `hpm` and its subcommands.
//! - [`run`]: entry point shared by the `hpm` binary and integration tests —
//!   parses arguments, builds the single [`Console`], dispatches through
//!   [`run_command`], and maps [`CliError`]s to exit codes.
//! - [`commands`]: one module per subcommand. Each command owns its
//!   user-facing output, including its success line; the dispatcher only
//!   loads config, gates `--output`, and delegates.
//!
//! Output contract:
//! - All human-facing output flows through the [`Console`] constructed once
//!   in [`run`]: `stdout` for result data (survives `--quiet`), `status` for
//!   supplementary lines, and `success`/`info`/`warn`/`error` for status
//!   lines. `tracing` macros are diagnostics only and always go to stderr.
//! - `--output json|json-lines|json-compact` is honored by `list`, `check`,
//!   `update`, `search`, and `pack`; every other command rejects it with an
//!   error instead of silently ignoring it.
//!
//! Exit codes: 0 success, 1 user error (config/package/network/io),
//! 2 internal error, N for a forwarded external command exit code.
use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::{Shell, generate};
use std::process::ExitCode;
use std::time::Instant;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

pub mod commands;
pub mod console;
pub mod error;
pub mod output;
pub mod progress;
pub mod script_sink;
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
    color: Option<ColorChoice>,

    /// Output format
    #[arg(long, value_enum)]
    output: Option<OutputFormat>,

    /// Directory to run command in (defaults to current directory)
    #[arg(short = 'C', long)]
    directory: Option<std::path::PathBuf>,

    #[command(subcommand)]
    command: Commands,
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
        #[arg(short, long)]
        manifest: Option<std::path::PathBuf>,

        /// Mark dependencies as optional
        #[arg(long)]
        optional: bool,

        /// Resolve from this configured registry only, and record the pin in
        /// hpm.toml so later installs and updates keep using it. Incompatible
        /// with --path.
        #[arg(long, conflicts_with = "path")]
        registry: Option<String>,
    },
    /// Remove a package dependency
    Remove {
        /// Package name to remove
        package: String,

        /// Path to directory containing hpm.toml or direct path to hpm.toml file
        #[arg(short, long)]
        manifest: Option<std::path::PathBuf>,
    },
    /// Update packages to latest versions
    Update {
        /// Only update specific packages
        #[arg(value_name = "PACKAGE")]
        packages: Vec<String>,

        /// Path to directory containing hpm.toml or direct path to hpm.toml file
        #[arg(short, long)]
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
        #[arg(short, long)]
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
        /// Path to directory containing hpm.toml or direct path to hpm.toml file
        #[arg(short, long)]
        manifest: Option<std::path::PathBuf>,

        /// Fail if lock file is missing or would change (for CI reproducibility)
        #[arg(long)]
        frozen_lockfile: bool,
    },
    /// Validate package configuration
    Check,
    /// Materialise the install image into a directory (defaults to
    /// `[stage].output_dir`, typically `dist/`).
    ///
    /// Pass `--output <dir>` to stage into a different location — useful
    /// when running multiple Houdini sessions side by side, each pointing
    /// its `HOUDINI_PACKAGE_PATH` at its own freshly built directory.
    Build {
        /// Path to directory containing hpm.toml or direct path to hpm.toml file
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
        /// Houdini major versions this build should target, space-separated
        /// (e.g. `--houdini-majors "21 22"`). Forwarded verbatim to prepack
        /// scripts as `HPM_HOUDINI_MAJORS` so a package that builds one native
        /// artifact per major can restrict the matrix. Unset = no restriction
        /// (an inherited `HPM_HOUDINI_MAJORS` still passes through).
        #[arg(long)]
        houdini_majors: Option<String>,
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
        /// Output result as JSON (for CI integration); equivalent to the
        /// global `--output json`
        #[arg(long)]
        json: bool,
        /// Target platform (defaults to host platform when `[compat].platforms` is declared)
        #[arg(long)]
        platform: Option<String>,
        /// Fail the pack if any `[[operators]]` `source` is missing from the
        /// produced archive (default: warn only).
        #[arg(long)]
        verify_assets: bool,
    },
    /// Run security audit on dependencies
    Audit {
        /// Path to directory containing hpm.toml or direct path to hpm.toml file
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
            _ => Verbosity::Verbose,
        }
    };

    let color_choice = cli.color.unwrap_or(ColorChoice::Auto);
    let output_format = cli.output.unwrap_or(OutputFormat::Human);

    let mut console = Console::with_settings(verbosity, color_choice);

    // Initialize logging based on verbosity
    init_logging(verbosity);

    // Execute command and handle errors
    let result = match run_command(cli.command, &mut console, output_format, cli.directory).await {
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

/// Fail fast when a command has no machine-readable form: silently ignoring
/// `--output json` would leave automation parsing human text.
fn require_human(command: &str, output: OutputFormat) -> CliResult<()> {
    if output.is_json() {
        return Err(CliError::config(
            anyhow::anyhow!("the '{command}' command does not support --output {output}"),
            None,
        ));
    }
    Ok(())
}

/// Dispatch a parsed command. Takes `Commands` by value so each arm moves its
/// fields into the command implementation instead of cloning them. Every arm
/// is a thin delegation; the command modules own all user-facing output.
async fn run_command(
    command: Commands,
    console: &mut Console,
    output: OutputFormat,
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
            require_human("init", output)?;
            let options = commands::init::InitOptions {
                name_or_path: name,
                description,
                author,
                version,
                license,
                houdini,
                bare,
                vcs,
                base_dir: directory,
            };
            commands::init::init_package(options, console)
                .await
                .cli_package("init")?;
        }
        Commands::Add {
            packages,
            path,
            link,
            manifest,
            optional,
            registry,
        } => {
            require_human("add", output)?;
            let config = load_cli_config()?;
            commands::add::add_packages(
                &config,
                packages,
                path,
                link,
                manifest,
                optional,
                registry.as_deref(),
                console,
            )
            .await
            .cli_package("add")?;
        }
        Commands::Remove { package, manifest } => {
            require_human("remove", output)?;
            let config = load_cli_config()?;
            commands::remove::remove_package(&config, &package, manifest, console)
                .await
                .cli_package("remove")?;
        }
        Commands::Update {
            packages,
            manifest,
            dry_run,
            yes,
        } => {
            let config = load_cli_config()?;
            let options = commands::update::UpdateOptions {
                manifest,
                packages,
                dry_run,
                yes,
                output,
            };
            commands::update::update_packages(&config, options, console)
                .await
                .cli_package("update")?;
        }
        Commands::List { manifest, tree } => {
            commands::list::list_dependencies(manifest, tree, console, output)
                .await
                .cli_package("list")?;
        }
        Commands::Search { query } => {
            let config = load_cli_config()?;
            commands::search::search_packages(&config, &query, console, output)
                .await
                .cli_network("search")?;
        }
        Commands::Run { script, args } => {
            require_human("run", output)?;
            let exit_code = commands::run::run_script(&script, &args, directory, console)
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
            require_human("install", output)?;
            let config = load_cli_config()?;
            commands::install::execute(&config, manifest, frozen_lockfile, console)
                .await
                .cli_package("install")?;
        }
        Commands::Check => {
            commands::check::check_package(directory, console, output)
                .await
                .cli_package("check")?;
        }
        Commands::Build {
            manifest,
            output: build_output,
            platform,
            profile,
            houdini_majors,
            no_prepack,
            no_clean,
        } => {
            require_human("build", output)?;
            let options = commands::build::BuildOptions {
                manifest: manifest.or(directory),
                output: build_output,
                platform,
                profile,
                houdini_majors,
                no_prepack,
                clean: !no_clean,
            };
            commands::build::build(options, console)
                .await
                .cli_package("build")?;
        }
        Commands::Pack {
            key,
            output: pack_output,
            json,
            platform,
            verify_assets,
        } => {
            let config = load_cli_config()?;
            // `--json` and the global `--output json*` are equivalent here:
            // pack emits its established single-line JSON payload for CI.
            let json = json || output.is_json();
            commands::pack::execute(
                &config,
                directory,
                key,
                pack_output,
                json,
                platform,
                verify_assets,
                console,
            )
            .await
            .cli_package("pack")?;
        }
        Commands::Audit { manifest } => {
            require_human("audit", output)?;
            let config = load_cli_config()?;
            commands::audit::audit_packages(&config, manifest, console)
                .await
                .cli_package("audit")?;
        }
        Commands::Registry { action } => {
            require_human("registry", output)?;
            let config = load_cli_config()?;
            match action {
                RegistryAction::Add {
                    url,
                    name,
                    registry_type,
                    if_not_exists,
                } => {
                    commands::registry::add_registry(
                        url,
                        name,
                        registry_type,
                        if_not_exists,
                        console,
                    )
                    .await
                    .cli_config("registry add")?;
                }
                RegistryAction::List => {
                    commands::registry::list_registries(&config, console)
                        .await
                        .map_err(|e| CliError::config(e, None))?;
                }
                RegistryAction::Remove { name } => {
                    commands::registry::remove_registry(&name, console)
                        .await
                        .cli_config("registry remove")?;
                }
                RegistryAction::Update => {
                    commands::registry::update_registries(&config, console)
                        .await
                        .map_err(|e| {
                            CliError::network(
                                e,
                                Some(
                                    "Check your internet connection and registry URLs.".to_string(),
                                ),
                            )
                        })?;
                }
            }
        }
        Commands::Clean(args) => {
            require_human("clean", output)?;
            let config = load_cli_config()?;
            commands::clean::execute_clean(&config, &args, console)
                .await
                .cli_package("clean")?;
        }
        Commands::Completions { shell } => {
            require_human("completions", output)?;
            let mut cmd = Cli::command();
            generate(shell, &mut cmd, "hpm", &mut std::io::stdout());
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
        // Diagnostics go to stderr, never stdout. This keeps stdout clean for
        // machine-readable output — `hpm pack --json` (and any other
        // `--output json*` command) emits only its JSON payload on stdout,
        // while progress logs stay on stderr where consumers can ignore them.
        .with(
            tracing_subscriber::fmt::layer()
                .with_target(false)
                .with_writer(std::io::stderr),
        )
        .init();
}
