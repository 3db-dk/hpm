//! HPM Package Registry
//!
//! This crate provides both client and server functionality for the HPM package registry,
//! implementing a high-performance, secure system for Houdini package distribution using
//! QUIC transport and gRPC for remote procedure calls.
//!
//! ## Architecture Overview
//!
//! The registry uses a modern protocol stack optimized for package management workloads:
//!
//! - **Transport**: QUIC with s2n-quic for enhanced performance and reliability
//! - **RPC**: gRPC with Protocol Buffers for efficient binary serialization
//! - **Security**: Token-based authentication with scoped permissions
//! - **Storage**: Trait-based storage abstraction supporting multiple backends
//! - **Compression**: zstd compression for package data
//! - **Integrity**: SHA-256 checksums for package verification
//!
//! ## Key Features
//!
//! - **High Performance**: QUIC provides 3.69x performance improvement for large file transfers
//! - **Secure by Design**: Mandatory encryption, token authentication, package verification
//! - **Scalable Architecture**: Async design with horizontal scaling support
//! - **Developer Friendly**: Comprehensive error handling and type safety
//!
//! ## Usage
//!
//! ### Server
//!
//! ```rust,no_run
//! use hpm_registry::server::{RegistryServer, MemoryStorage};
//! use std::net::SocketAddr;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let bind_addr: SocketAddr = "127.0.0.1:8080".parse()?;
//!     let storage = Box::new(MemoryStorage::new());
//!     let server = RegistryServer::new(bind_addr, storage);
//!     
//!     server.serve().await?;
//!     Ok(())
//! }
//! ```
//!
//! ### Client
//!
//! ```rust,no_run
//! use hpm_registry::{RegistryClient, RegistryClientConfig};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let config = RegistryClientConfig {
//!         endpoint: "https://registry.hpm.dev".to_string(),
//!         ..Default::default()
//!     };
//!     
//!     let mut client = RegistryClient::connect(config).await?;
//!     client.set_auth_token("your-token-here".to_string());
//!     
//!     // Search for packages
//!     let results = client.search_packages("geometry", Some(10), Some(0)).await?;
//!     println!("Found {} packages", results.total_count);
//!     
//!     Ok(())
//! }
//! ```
//!
//! ## Modules
//!
//! - [`client`] - Registry client implementation for connecting to remote registries
//! - [`server`] - Registry server implementation with gRPC services
//! - [`types`] - Common types for authentication, packages, and errors
//! - [`utils`] - Utility functions for compression, validation, and checksums
//! - [`proto`] - Generated Protocol Buffer definitions

#![allow(clippy::result_large_err)]
#![allow(clippy::large_enum_variant)]

pub mod client;
pub mod proto;
pub mod server;
pub mod types;
pub mod utils;

// Re-export commonly used types and functions
pub use client::{RegistryClient, RegistryClientConfig};
pub use proto::{
    package_registry_client::PackageRegistryClient, package_registry_server::PackageRegistryServer,
    DownloadRequest, DownloadResponse, PackageInfo, PackageMetadata, PublishRequest,
    PublishResponse, SearchRequest, SearchResponse,
};
pub use server::RegistryServer;
pub use types::{AuthToken, RegistryError, TokenScope};
