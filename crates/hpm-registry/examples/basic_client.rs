//! Basic Registry Client Example
//!
//! This example demonstrates how to connect to an HPM registry and perform
//! basic operations like searching for packages and retrieving package information.

use hpm_registry::client::{RegistryClient, RegistryClientConfig};
use hpm_registry::types::RegistryError;
use std::time::Duration;
use tracing::{error, info, Level};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt().with_max_level(Level::INFO).init();

    info!("HPM Registry Client Example");

    // Configure client connection
    let config = RegistryClientConfig {
        endpoint: "http://127.0.0.1:8080".to_string(),
        tls_config: None, // No TLS for local development
        connect_timeout: Duration::from_secs(10),
        request_timeout: Duration::from_secs(30),
    };

    // Connect to the registry
    info!("Connecting to registry at {}", config.endpoint);
    let mut client = match RegistryClient::connect(config).await {
        Ok(client) => {
            info!("Successfully connected to registry");
            client
        }
        Err(e) => {
            error!("Failed to connect to registry: {}", e);
            return Err(e.into());
        }
    };

    // Perform a health check
    info!("Performing health check...");
    match client.health_check().await {
        Ok(healthy) => {
            if healthy {
                info!("Registry is healthy");
            } else {
                error!("Registry is not healthy");
                return Ok(());
            }
        }
        Err(e) => {
            error!("Health check failed: {}", e);
            return Ok(());
        }
    }

    // Search for packages
    info!("Searching for packages...");
    match client.search_packages("geometry", Some(10), Some(0)).await {
        Ok(results) => {
            info!("Found {} packages", results.total_count);
            for package in results.packages {
                info!(
                    "  - {} v{}: {}",
                    package.name, package.version, package.description
                );
            }
        }
        Err(RegistryError::PackageNotFound { .. }) => {
            info!("No packages found matching search criteria");
        }
        Err(e) => {
            error!("Search failed: {}", e);
        }
    }

    // Try to get information about a specific package
    info!("Getting package info for 'test-package'...");
    match client.get_package_info("test-package", None).await {
        Ok(package_info) => {
            info!("Package found:");
            info!("  Name: {}", package_info.name);
            info!("  Version: {}", package_info.version);
            info!("  Description: {}", package_info.description);
            info!("  Authors: {:?}", package_info.authors);
            info!("  Keywords: {:?}", package_info.keywords);
        }
        Err(RegistryError::PackageNotFound { name }) => {
            info!("Package '{}' not found in registry", name);
        }
        Err(e) => {
            error!("Failed to get package info: {}", e);
        }
    }

    info!("Client example completed");
    Ok(())
}
