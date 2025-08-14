use anyhow::Result;
use clap::{Parser, Subcommand};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

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
        name: String,
    },
    /// Install a package
    Install {
        /// Package name
        package: Option<String>,
    },
    /// Uninstall a package
    Uninstall {
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
        Commands::Init { name } => {
            println!("Initializing package: {}", name);
        }
        Commands::Install { package } => match package {
            Some(pkg) => println!("Installing package: {}", pkg),
            None => println!("Installing dependencies from hpm.toml"),
        },
        Commands::Uninstall { package } => {
            println!("Uninstalling package: {}", package);
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
