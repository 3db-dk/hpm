//! Connection management for QUIC transport

use crate::types::RegistryError;
use std::time::Duration;
use tonic::transport::{Channel, ClientTlsConfig, Endpoint};

pub struct ConnectionManager {
    endpoint: String,
    tls_config: Option<ClientTlsConfig>,
    connect_timeout: Duration,
    connection_pool: Option<Channel>,
}

impl ConnectionManager {
    pub fn new(endpoint: String) -> Self {
        Self {
            endpoint,
            tls_config: None,
            connect_timeout: Duration::from_secs(10),
            connection_pool: None,
        }
    }

    pub fn with_tls_config(mut self, tls_config: ClientTlsConfig) -> Self {
        self.tls_config = Some(tls_config);
        self
    }

    pub fn with_connect_timeout(mut self, timeout: Duration) -> Self {
        self.connect_timeout = timeout;
        self
    }

    pub async fn get_channel(&mut self) -> Result<Channel, RegistryError> {
        if let Some(channel) = &self.connection_pool {
            // In a real implementation, we'd check if the connection is still healthy
            return Ok(channel.clone());
        }

        let channel = self.create_new_connection().await?;
        self.connection_pool = Some(channel.clone());
        Ok(channel)
    }

    async fn create_new_connection(&self) -> Result<Channel, RegistryError> {
        let endpoint = Endpoint::from_shared(self.endpoint.clone())
            .map_err(RegistryError::Network)?
            .connect_timeout(self.connect_timeout);

        let endpoint = if let Some(tls_config) = &self.tls_config {
            endpoint
                .tls_config(tls_config.clone())
                .map_err(RegistryError::Network)?
        } else {
            endpoint
        };

        endpoint.connect().await.map_err(RegistryError::Network)
    }

    pub fn close_connection(&mut self) {
        self.connection_pool = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_connection_manager_creation() {
        let manager = ConnectionManager::new("https://example.com".to_string());
        assert_eq!(manager.endpoint, "https://example.com");
        assert!(manager.tls_config.is_none());
        assert_eq!(manager.connect_timeout, Duration::from_secs(10));
    }
}
