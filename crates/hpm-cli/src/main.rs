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
    /// Add a package
    Add {
        /// Package name
        package: Option<String>,
    },
    /// Remove a package
    Remove {
        /// Package name
        package: String,
    },
    /// Update packages
    Update,
    /// List installed packages
    List,
    /// Search for packages
    Search {
        /// Search query
        query: String,
    },
    /// Publish a package
    Publish,
    /// Show package information
    Info {
        /// Package name
        package: String,
    },
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
            };

            init_package(options).await?;
        }
        Commands::Add { package } => match package {
            Some(pkg) => println!("Adding package: {}", pkg),
            None => println!("Adding dependencies from hpm.toml"),
        },
        Commands::Remove { package } => {
            println!("Removing package: {}", package);
        }
        Commands::Update => {
            println!("Updating packages");
        }
        Commands::List => {
            println!("Listing installed packages");
        }
        Commands::Search { query } => {
            println!("Searching for: {}", query);
        }
        Commands::Publish => {
            println!("Publishing package");
        }
        Commands::Info { package } => {
            println!("Package info for: {}", package);
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
