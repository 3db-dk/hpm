//! Basic Registry Server Example
//!
//! This example demonstrates how to set up a basic HPM registry server
//! using in-memory storage for development and testing.

use hpm_registry::server::{AuthService, MemoryStorage, RegistryServer};
use hpm_registry::types::{AuthToken, TokenScope};
use std::collections::HashSet;
use std::net::SocketAddr;
use tracing::{info, Level};
use tracing_subscriber::fmt;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    fmt().with_max_level(Level::INFO).init();

    info!("Starting HPM Registry Server");

    // Configure server address
    let bind_addr: SocketAddr = "127.0.0.1:8080".parse()?;

    // Create in-memory storage backend
    let storage = Box::new(MemoryStorage::new());

    // Create server instance
    let server = RegistryServer::new(bind_addr, storage);

    info!("Server listening on {}", bind_addr);
    info!("Ready to accept connections...");

    // Start server (this will run indefinitely)
    server.serve().await?;

    Ok(())
}

// Helper function to create test tokens (would be in a separate admin tool)
#[allow(dead_code)]
async fn create_test_tokens(auth_service: &AuthService) {
    // Create a read-only token
    let mut read_scopes = HashSet::new();
    read_scopes.insert(TokenScope::Read);
    let read_token = AuthToken::new("test_user_1".to_string(), read_scopes);
    auth_service.add_token(read_token).await;

    // Create a publish token
    let mut publish_scopes = HashSet::new();
    publish_scopes.insert(TokenScope::Read);
    publish_scopes.insert(TokenScope::Publish);
    let publish_token = AuthToken::new("test_user_2".to_string(), publish_scopes);
    auth_service.add_token(publish_token).await;

    // Create an admin token
    let mut admin_scopes = HashSet::new();
    admin_scopes.insert(TokenScope::Admin);
    let admin_token = AuthToken::new("admin_user".to_string(), admin_scopes);
    auth_service.add_token(admin_token).await;
}
