//! Integration Tests for HPM Registry
//!
//! These tests verify end-to-end functionality of the registry system,
//! including client-server communication and data persistence.

use hpm_registry::client::{RegistryClient, RegistryClientConfig};
use hpm_registry::server::{MemoryStorage, RegistryServer};
use hpm_registry::types::{AuthToken, RegistryError, TokenScope};
use std::collections::HashSet;
use std::net::SocketAddr;
use std::time::Duration;
use tokio::time::sleep;

/// Helper to start a test registry server in the background
async fn start_test_server() -> SocketAddr {
    let bind_addr: SocketAddr = "127.0.0.1:0".parse().unwrap(); // Use random port
    let storage = Box::new(MemoryStorage::new());
    let server = RegistryServer::new(bind_addr, storage);

    // Start server in background
    let actual_addr = bind_addr; // In real implementation, we'd get the actual bound address
    tokio::spawn(async move {
        if let Err(e) = server.serve().await {
            eprintln!("Server error: {}", e);
        }
    });

    // Give server time to start
    sleep(Duration::from_millis(100)).await;
    actual_addr
}

/// Create a test client connected to the given server address
async fn create_test_client(server_addr: SocketAddr) -> RegistryClient {
    let config = RegistryClientConfig {
        endpoint: format!("http://{}", server_addr),
        tls_config: None,
        connect_timeout: Duration::from_secs(5),
        request_timeout: Duration::from_secs(10),
    };

    RegistryClient::connect(config)
        .await
        .expect("Failed to connect test client")
}

#[tokio::test]
#[ignore] // TODO: Fix server startup in tests
async fn test_health_check() {
    let server_addr = start_test_server().await;
    let mut client = create_test_client(server_addr).await;

    let is_healthy = client.health_check().await.expect("Health check failed");
    assert!(is_healthy, "Server should be healthy");
}

#[tokio::test]
#[ignore] // TODO: Fix server startup in tests
async fn test_search_empty_registry() {
    let server_addr = start_test_server().await;
    let mut client = create_test_client(server_addr).await;

    let results = client
        .search_packages("nonexistent", Some(10), Some(0))
        .await
        .expect("Search should succeed even if no results");

    assert_eq!(
        results.total_count, 0,
        "Empty registry should return no results"
    );
    assert!(results.packages.is_empty(), "Packages list should be empty");
}

#[tokio::test]
#[ignore] // TODO: Fix server startup in tests
async fn test_get_nonexistent_package() {
    let server_addr = start_test_server().await;
    let mut client = create_test_client(server_addr).await;

    let result = client.get_package_info("nonexistent-package", None).await;

    match result {
        Err(RegistryError::PackageNotFound { name }) => {
            assert_eq!(name, "nonexistent-package");
        }
        Ok(_) => panic!("Should not find nonexistent package"),
        Err(e) => panic!("Unexpected error: {}", e),
    }
}

#[tokio::test]
async fn test_client_connection_failure() {
    // Try to connect to a non-existent server
    let config = RegistryClientConfig {
        endpoint: "http://127.0.0.1:65432".to_string(), // Unlikely to be used
        tls_config: None,
        connect_timeout: Duration::from_millis(100),
        request_timeout: Duration::from_millis(100),
    };

    let result = RegistryClient::connect(config).await;
    assert!(
        result.is_err(),
        "Should fail to connect to non-existent server"
    );
}

#[cfg(test)]
mod auth_tests {
    use super::*;
    use chrono::{Duration as ChronoDuration, Utc};
    use hpm_registry::server::AuthService;

    #[tokio::test]
    async fn test_token_validation() {
        let auth_service = AuthService::new();

        // Create a test token
        let mut scopes = HashSet::new();
        scopes.insert(TokenScope::Read);
        let token = AuthToken::new("test_user".to_string(), scopes);
        let token_str = token.token.clone();

        // Add token to auth service
        auth_service.add_token(token).await;

        // Validate token
        let validated_token = auth_service
            .validate_token(&token_str)
            .await
            .expect("Token validation should succeed");

        assert_eq!(validated_token.user_id, "test_user");
        assert!(validated_token.has_scope(&TokenScope::Read));
        assert!(!validated_token.has_scope(&TokenScope::Publish));
    }

    #[tokio::test]
    async fn test_expired_token() {
        let auth_service = AuthService::new();

        // Create an expired token
        let mut scopes = HashSet::new();
        scopes.insert(TokenScope::Read);
        let token = AuthToken::new("test_user".to_string(), scopes)
            .with_expiry(Utc::now() - ChronoDuration::hours(1)); // Expired 1 hour ago
        let token_str = token.token.clone();

        // Add token to auth service
        auth_service.add_token(token).await;

        // Try to validate expired token
        let result = auth_service.validate_token(&token_str).await;
        assert!(result.is_err(), "Expired token should not validate");
    }

    #[tokio::test]
    async fn test_permission_checking() {
        let auth_service = AuthService::new();

        // Create a read-only token
        let mut read_scopes = HashSet::new();
        read_scopes.insert(TokenScope::Read);
        let read_token = AuthToken::new("read_user".to_string(), read_scopes);

        // Should have read permission
        assert!(auth_service
            .check_permission(&read_token, TokenScope::Read)
            .await
            .is_ok());

        // Should not have publish permission
        assert!(auth_service
            .check_permission(&read_token, TokenScope::Publish)
            .await
            .is_err());

        // Create admin token
        let mut admin_scopes = HashSet::new();
        admin_scopes.insert(TokenScope::Admin);
        let admin_token = AuthToken::new("admin_user".to_string(), admin_scopes);

        // Admin should have all permissions
        assert!(auth_service
            .check_permission(&admin_token, TokenScope::Read)
            .await
            .is_ok());
        assert!(auth_service
            .check_permission(&admin_token, TokenScope::Publish)
            .await
            .is_ok());
        assert!(auth_service
            .check_permission(&admin_token, TokenScope::Delete)
            .await
            .is_ok());
    }
}

#[cfg(test)]
mod storage_tests {
    use super::*;
    use chrono::Utc;
    use hpm_registry::server::PackageStorage;
    use hpm_registry::types::{HoudiniRequirements, PackageMetadata, PackageVersion};
    use std::collections::HashMap;

    fn create_test_package_version(name: &str, version: &str) -> PackageVersion {
        PackageVersion {
            version: version.to_string(),
            metadata: PackageMetadata {
                name: name.to_string(),
                version: version.to_string(),
                description: format!("Test package {}", name),
                authors: vec!["Test Author".to_string()],
                license: Some("MIT".to_string()),
                dependencies: HashMap::new(),
                houdini: HoudiniRequirements {
                    min_version: Some("19.0".to_string()),
                    max_version: Some("20.0".to_string()),
                    platforms: vec!["linux".to_string(), "windows".to_string()],
                },
                keywords: vec!["test".to_string(), "example".to_string()],
                readme: Some("Test package readme".to_string()),
                repository: Some("https://github.com/test/test".to_string()),
                homepage: Some("https://test.dev".to_string()),
            },
            published_at: Utc::now(),
            published_by: "test_user".to_string(),
            checksum: "abc123".to_string(),
            size_bytes: 1024,
        }
    }

    #[tokio::test]
    async fn test_memory_storage_roundtrip() {
        let storage = MemoryStorage::new();
        let package = create_test_package_version("test-package", "1.0.0");
        let test_data = b"test package data".to_vec();

        // Store package
        let package_id = storage
            .store_package(package.clone(), test_data.clone())
            .await
            .expect("Should store package successfully");

        assert!(!package_id.is_empty(), "Package ID should not be empty");

        // Retrieve package data
        let retrieved_data = storage
            .get_package_data("test-package", "1.0.0")
            .await
            .expect("Should retrieve package data");

        assert_eq!(
            retrieved_data, test_data,
            "Retrieved data should match stored data"
        );

        // Retrieve package info
        let retrieved_info = storage
            .get_package_info("test-package", Some("1.0.0"))
            .await
            .expect("Should retrieve package info");

        assert_eq!(retrieved_info.metadata.name, "test-package");
        assert_eq!(retrieved_info.version, "1.0.0");
        assert_eq!(
            retrieved_info.metadata.description,
            "Test package test-package"
        );
    }

    #[tokio::test]
    async fn test_storage_search_functionality() {
        let storage = MemoryStorage::new();

        // Store multiple test packages
        let package1 = create_test_package_version("geometry-tools", "1.0.0");
        let package2 = create_test_package_version("material-library", "2.1.0");

        storage
            .store_package(package1, b"data1".to_vec())
            .await
            .unwrap();
        storage
            .store_package(package2, b"data2".to_vec())
            .await
            .unwrap();

        // Search for packages
        let (results, total) = storage
            .search_packages("geometry", 10, 0)
            .await
            .expect("Search should succeed");

        assert_eq!(total, 1, "Should find one geometry package");
        assert_eq!(results[0].metadata.name, "geometry-tools");

        // Search for packages with "test" in description
        let (_results, total) = storage
            .search_packages("Test package", 10, 0)
            .await
            .expect("Search should succeed");

        assert_eq!(total, 2, "Should find both packages by description");
    }
}
