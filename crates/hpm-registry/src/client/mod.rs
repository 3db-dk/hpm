//! Registry Client Implementation
//!
//! This module provides a high-level client interface for connecting to HPM package registries.
//! The client handles QUIC connections, authentication, and provides methods for all registry
//! operations including package publishing, downloading, and searching.
//!
//! ## Features
//!
//! - **Async/Await**: Full tokio integration for non-blocking operations
//! - **Connection Management**: Automatic connection pooling and reconnection
//! - **Authentication**: Token-based authentication with automatic header injection
//! - **Streaming**: Efficient streaming for large package uploads and downloads
//! - **Error Handling**: Comprehensive error types with context information
//!
//! ## Example Usage
//!
//! ```rust,no_run
//! use hpm_registry::client::{RegistryClient, RegistryClientConfig};
//! use std::path::Path;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Configure client
//!     let config = RegistryClientConfig {
//!         endpoint: "https://registry.hpm.dev".to_string(),
//!         connect_timeout: std::time::Duration::from_secs(10),
//!         request_timeout: std::time::Duration::from_secs(30),
//!         ..Default::default()
//!     };
//!     
//!     // Connect to registry
//!     let mut client = RegistryClient::connect(config).await?;
//!     client.set_auth_token("your-token-here".to_string());
//!     
//!     // Publish a package
//!     let response = client.publish_package(Path::new("./my-package")).await?;
//!     println!("Published package: {}", response.package_id);
//!     
//!     // Search for packages
//!     let results = client.search_packages("geometry tools", Some(10), None).await?;
//!     for package in results.packages {
//!         println!("Found: {} v{}", package.name, package.version);
//!     }
//!     
//!     // Download a package
//!     client.download_package("geometry-tools", "1.0.0", Path::new("./downloads")).await?;
//!     
//!     Ok(())
//! }
//! ```

pub mod auth;
pub mod connection;
pub mod operations;

use crate::proto::{
    package_registry_client::PackageRegistryClient, PackageInfo, PublishResponse, SearchResponse,
};
use crate::types::RegistryError;
use anyhow::Result;
use std::path::Path;
use tonic::transport::{Channel, ClientTlsConfig, Endpoint};

pub use auth::AuthManager;
pub use connection::ConnectionManager;
pub use operations::*;

#[derive(Debug, Clone)]
pub struct RegistryClientConfig {
    pub endpoint: String,
    pub tls_config: Option<ClientTlsConfig>,
    pub connect_timeout: std::time::Duration,
    pub request_timeout: std::time::Duration,
}

impl Default for RegistryClientConfig {
    fn default() -> Self {
        Self {
            endpoint: "https://registry.hpm.dev".to_string(),
            tls_config: None,
            connect_timeout: std::time::Duration::from_secs(10),
            request_timeout: std::time::Duration::from_secs(30),
        }
    }
}

pub struct RegistryClient {
    client: PackageRegistryClient<Channel>,
    auth_manager: AuthManager,
    #[allow(dead_code)] // Will be used for connection management in future
    config: RegistryClientConfig,
}

impl RegistryClient {
    pub async fn connect(config: RegistryClientConfig) -> Result<Self, RegistryError> {
        let endpoint = Endpoint::from_shared(config.endpoint.clone())
            .map_err(RegistryError::Network)?
            .connect_timeout(config.connect_timeout)
            .timeout(config.request_timeout);

        let endpoint = if let Some(tls_config) = &config.tls_config {
            endpoint
                .tls_config(tls_config.clone())
                .map_err(RegistryError::Network)?
        } else {
            endpoint
        };

        let channel = endpoint.connect().await?;
        let client = PackageRegistryClient::new(channel);

        Ok(Self {
            client,
            auth_manager: AuthManager::new(),
            config,
        })
    }

    pub fn set_auth_token(&mut self, token: String) {
        self.auth_manager.set_token(token);
    }

    pub async fn publish_package<P: AsRef<Path>>(
        &mut self,
        package_path: P,
    ) -> Result<PublishResponse, RegistryError> {
        publish_package(&mut self.client, &self.auth_manager, package_path).await
    }

    pub async fn download_package(
        &mut self,
        name: &str,
        version: &str,
        output_path: &Path,
    ) -> Result<(), RegistryError> {
        download_package(&mut self.client, name, version, output_path).await
    }

    pub async fn search_packages(
        &mut self,
        query: &str,
        limit: Option<i32>,
        offset: Option<i32>,
    ) -> Result<SearchResponse, RegistryError> {
        search_packages(&mut self.client, query, limit, offset).await
    }

    pub async fn get_package_info(
        &mut self,
        name: &str,
        version: Option<&str>,
    ) -> Result<PackageInfo, RegistryError> {
        get_package_info(&mut self.client, name, version).await
    }

    pub async fn health_check(&mut self) -> Result<bool, RegistryError> {
        health_check(&mut self.client).await
    }
}
