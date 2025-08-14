//! HPM Registry Server executable

use hpm_registry::server::{MemoryStorage, RegistryServer};
use std::net::SocketAddr;
use tracing::info;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    let bind_addr: SocketAddr = "127.0.0.1:8080".parse()?;

    info!("Starting HPM Registry server on {}", bind_addr);

    let storage = Box::new(MemoryStorage::new());
    let server = RegistryServer::new(bind_addr, storage);

    server.serve().await?;

    Ok(())
}
