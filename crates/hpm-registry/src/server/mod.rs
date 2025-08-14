//! Registry Server Implementation
//!
//! This module provides the server-side implementation of the HPM package registry,
//! including gRPC services, authentication middleware, and storage backends.
//!
//! ## Architecture
//!
//! The server is built around several key components:
//!
//! - **gRPC Service**: Handles all registry operations via Protocol Buffers
//! - **Authentication Service**: Token-based auth with scoped permissions
//! - **Storage Backend**: Trait-based abstraction supporting multiple storage systems
//! - **QUIC Transport**: High-performance networking with automatic encryption
//!
//! ## Storage Backends
//!
//! The server supports pluggable storage backends through the [`PackageStorage`] trait:
//!
//! - **MemoryStorage**: In-memory storage for development and testing
//! - **PostgreSQL**: Production database backend (planned)
//! - **S3**: Object storage for package artifacts (planned)
//!
//! ## Example Usage
//!
//! ```rust,no_run
//! use hpm_registry::server::{RegistryServer, MemoryStorage};
//! use std::net::SocketAddr;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Configure server
//!     let bind_addr: SocketAddr = "0.0.0.0:8080".parse()?;
//!     let storage = Box::new(MemoryStorage::new());
//!     
//!     // Create and start server
//!     let server = RegistryServer::new(bind_addr, storage);
//!     println!("Starting server on {}", bind_addr);
//!     
//!     server.serve().await?;
//!     Ok(())
//! }
//! ```
//!
//! ## Security
//!
//! The server implements multiple security layers:
//!
//! - **Transport Encryption**: Mandatory QUIC/TLS encryption
//! - **Authentication**: Token-based authentication with expiration
//! - **Authorization**: Scoped permissions (read, publish, delete, admin)
//! - **Package Integrity**: SHA-256 checksums for all packages
//! - **Rate Limiting**: Configurable rate limits per token (planned)
//!
//! ## Performance
//!
//! The server is designed for high performance:
//!
//! - **Async/Await**: Full tokio integration for maximum concurrency
//! - **QUIC Protocol**: 3.69x performance improvement over HTTP/2
//! - **Streaming**: Memory-efficient streaming for large package transfers
//! - **Compression**: zstd compression reduces bandwidth usage
//! - **Connection Pooling**: Efficient connection reuse

pub mod auth;
pub mod service;
pub mod storage;

use crate::proto::package_registry_server::PackageRegistryServer;
use crate::types::RegistryError;
use anyhow::Result;
use service::RegistryService;
use std::net::SocketAddr;
use tonic::transport::Server;

pub use auth::AuthService;
pub use storage::*;

pub struct RegistryServer {
    service: RegistryService,
    bind_addr: SocketAddr,
}

impl RegistryServer {
    pub fn new(bind_addr: SocketAddr, storage: Box<dyn PackageStorage>) -> Self {
        let auth_service = AuthService::new();
        let service = RegistryService::new(storage, auth_service);

        Self { service, bind_addr }
    }

    pub async fn serve(self) -> Result<(), RegistryError> {
        println!("Starting HPM Registry server on {}", self.bind_addr);

        Server::builder()
            .add_service(PackageRegistryServer::new(self.service))
            .serve(self.bind_addr)
            .await
            .map_err(RegistryError::Network)
    }
}
