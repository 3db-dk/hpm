# Rust Coding Conventions for HPM Package Manager

## Code Style and Formatting

### General Principles
- Follow standard Rust conventions using `rustfmt`
- Use `cargo clippy` for additional linting and best practices
- Prioritize readability and maintainability
- Embrace Rust idioms and zero-cost abstractions

### Naming Conventions
```rust
// Module names: snake_case
mod package_manager;
mod registry_client;

// Function names: snake_case  
fn install_package() {}
fn resolve_dependencies() {}

// Variable names: snake_case
let package_name = "example";
let registry_url = "https://registry.example.com";

// Constant names: SCREAMING_SNAKE_CASE
const DEFAULT_REGISTRY_URL: &str = "https://hpm.registry.com";
const MAX_CONCURRENT_DOWNLOADS: usize = 8;

// Type names: PascalCase
struct PackageManager {}
enum InstallationError {}
trait RegistryClient {}

// Generic type parameters: single uppercase letter
fn process<T, E>(item: T) -> Result<T, E> {}
```

### Module Organization
```rust
// Prefer explicit module declarations
pub mod cli;
pub mod config;
pub mod error;
pub mod installer;
pub mod registry;
pub mod resolver;

// Use clear module hierarchies
pub mod registry {
    pub mod client;
    pub mod metadata;
    pub mod auth;
}
```

## Error Handling

### Error Types
```rust
// Use thiserror for error definition
#[derive(Debug, thiserror::Error)]
pub enum InstallError {
    #[error("Package not found: {name}")]
    PackageNotFound { name: String },
    
    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),
    
    #[error("File system error: {0}")]
    FileSystem(#[from] std::io::Error),
}

// Use anyhow for application-level error handling
use anyhow::{Context, Result};

pub fn install_package(name: &str) -> Result<()> {
    download_package(name)
        .context("Failed to download package")?;
    
    extract_package(name)
        .context("Failed to extract package files")?;
    
    Ok(())
}
```

### Result Handling
```rust
// Prefer ? operator for error propagation
fn process_package() -> Result<Package, Error> {
    let metadata = fetch_metadata()?;
    let dependencies = resolve_dependencies(&metadata)?;
    Ok(Package::new(metadata, dependencies))
}

// Use match for explicit error handling when needed
match install_package("example") {
    Ok(_) => println!("Installation successful"),
    Err(InstallError::PackageNotFound { name }) => {
        eprintln!("Package '{}' not found in registry", name);
    }
    Err(e) => eprintln!("Installation failed: {}", e),
}
```

## Async Programming

### Async Patterns
```rust
// Use tokio for async runtime
#[tokio::main]
async fn main() -> Result<()> {
    let client = RegistryClient::new().await?;
    client.sync_packages().await?;
    Ok(())
}

// Prefer async/await over manual Future implementations
pub async fn download_package(url: &str) -> Result<Vec<u8>, reqwest::Error> {
    let response = reqwest::get(url).await?;
    let bytes = response.bytes().await?;
    Ok(bytes.to_vec())
}

// Use concurrent operations where appropriate
pub async fn download_packages(urls: &[String]) -> Vec<Result<Package, Error>> {
    let futures = urls.iter().map(|url| download_package(url));
    futures::future::join_all(futures).await
}
```

### Stream Processing
```rust
use futures::stream::{Stream, StreamExt};
use tokio_stream;

// Use streams for processing sequences of data
pub async fn process_package_stream<S>(packages: S) -> Result<()> 
where
    S: Stream<Item = Package> + Unpin,
{
    packages
        .for_each(|package| async move {
            if let Err(e) = install_package(&package).await {
                eprintln!("Failed to install {}: {}", package.name(), e);
            }
        })
        .await;
    
    Ok(())
}
```

## Testing Conventions

### Test Organization
```rust
// Unit tests in same file
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_package_parsing() {
        let manifest = r#"
            [package]
            name = "test"
            version = "1.0.0"
        "#;
        
        let package = Package::from_str(manifest).unwrap();
        assert_eq!(package.name(), "test");
        assert_eq!(package.version().to_string(), "1.0.0");
    }

    #[tokio::test]
    async fn test_registry_client() {
        let client = MockRegistryClient::new();
        let result = client.search("test").await.unwrap();
        assert!(!result.packages.is_empty());
    }
}

// Integration tests in tests/ directory
// tests/integration/package_installation.rs
use hpm::{PackageManager, Registry};

#[tokio::test]
async fn test_full_installation_workflow() {
    let registry = Registry::mock();
    let manager = PackageManager::new(registry);
    
    let result = manager.install("test-package").await;
    assert!(result.is_ok());
}
```

### Mock and Test Utilities
```rust
// Create testable abstractions
#[async_trait]
pub trait RegistryClient {
    async fn search(&self, query: &str) -> Result<SearchResults>;
    async fn download(&self, package: &PackageSpec) -> Result<Vec<u8>>;
}

// Implement mocks for testing
pub struct MockRegistryClient {
    packages: HashMap<String, Package>,
}

#[async_trait]
impl RegistryClient for MockRegistryClient {
    async fn search(&self, query: &str) -> Result<SearchResults> {
        // Mock implementation
        Ok(SearchResults::empty())
    }
}
```

## Documentation

### Code Documentation
```rust
//! # HPM - Houdini Package Manager
//! 
//! A modern package manager for SideFX Houdini digital assets and tools.
//! 
//! ## Features
//! 
//! - Semantic versioning and dependency resolution
//! - Cryptographic package verification
//! - Cross-platform compatibility
//! 
//! ## Usage
//! 
//! ```rust
//! use hpm::PackageManager;
//! 
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let manager = PackageManager::new().await?;
//!     manager.install("example-package").await?;
//!     Ok(())
//! }
//! ```

/// Downloads and installs a package from the registry.
/// 
/// # Arguments
/// 
/// * `name` - The name of the package to install
/// * `version` - Optional version constraint (defaults to latest)
/// 
/// # Returns
/// 
/// Returns `Ok(())` on successful installation, or an error describing
/// what went wrong during the installation process.
/// 
/// # Examples
/// 
/// ```rust
/// # use hpm::PackageManager;
/// # tokio_test::block_on(async {
/// let manager = PackageManager::new().await?;
/// 
/// // Install latest version
/// manager.install_package("example-hda", None).await?;
/// 
/// // Install specific version
/// manager.install_package("example-hda", Some("1.2.0")).await?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// # });
/// ```
pub async fn install_package(
    &self,
    name: &str,
    version: Option<&str>,
) -> Result<(), InstallError> {
    // Implementation
}
```

### API Documentation
```rust
// Use doc comments for public APIs
/// Configuration settings for the HPM package manager.
/// 
/// This struct holds all configuration options that control
/// package manager behavior, including registry URLs,
/// authentication settings, and installation preferences.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Default registry URL for package searches and downloads
    pub registry_url: String,
    
    /// Installation directory for packages (relative to Houdini user prefs)
    pub install_path: PathBuf,
    
    /// Authentication token for private registries
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_token: Option<String>,
}

impl Config {
    /// Creates a new configuration with default settings.
    /// 
    /// # Examples
    /// 
    /// ```rust
    /// use hpm::Config;
    /// 
    /// let config = Config::default();
    /// assert_eq!(config.registry_url, "https://packages.houdini.org");
    /// ```
    pub fn default() -> Self {
        Self {
            registry_url: "https://packages.houdini.org".to_string(),
            install_path: PathBuf::from("packages/hpm"),
            auth_token: None,
        }
    }
}
```

## Dependency Management

### Cargo.toml Structure
```toml
[package]
name = "hpm"
version = "0.1.0"
edition = "2021"
description = "Houdini Package Manager"
license = "MIT OR Apache-2.0"
repository = "https://github.com/user/hpm"
keywords = ["houdini", "package-manager", "vfx"]
categories = ["command-line-utilities", "development-tools"]

[dependencies]
# CLI and configuration
clap = { version = "4.0", features = ["derive"] }
serde = { version = "1.0", features = ["derive"] }
toml = "0.8"

# Async runtime and networking
tokio = { version = "1.0", features = ["full"] }
reqwest = { version = "0.11", features = ["json", "stream"] }
futures = "0.3"

# Error handling
anyhow = "1.0"
thiserror = "1.0"

# File system and compression
tar = "0.4"
flate2 = "1.0"
walkdir = "2.0"

# Cryptography
ring = "0.16"
base64 = "0.21"

[dev-dependencies]
tokio-test = "0.4"
tempfile = "3.0"
mockito = "1.0"

[features]
default = ["registry-client"]
registry-client = ["reqwest"]
offline = []  # Disable network features for testing
```

### Feature Flags
```rust
// Conditional compilation for features
#[cfg(feature = "registry-client")]
pub mod registry {
    pub use crate::client::RegistryClient;
}

#[cfg(not(feature = "registry-client"))]
pub mod registry {
    pub struct RegistryClient;
    
    impl RegistryClient {
        pub fn new() -> Self {
            unimplemented!("Registry client disabled")
        }
    }
}
```

## Performance Best Practices

### Memory Management
```rust
// Use references to avoid unnecessary clones
pub fn process_packages(packages: &[Package]) -> Vec<InstallResult> {
    packages
        .iter()
        .map(|package| install_package(package))
        .collect()
}

// Use Cow for conditional ownership
use std::borrow::Cow;

pub fn normalize_name(name: &str) -> Cow<'_, str> {
    if name.chars().any(|c| c.is_uppercase()) {
        Cow::Owned(name.to_lowercase())
    } else {
        Cow::Borrowed(name)
    }
}

// Prefer iteration over collection when possible
pub fn find_package<'a>(
    packages: &'a [Package],
    name: &str,
) -> Option<&'a Package> {
    packages.iter().find(|pkg| pkg.name() == name)
}
```

### Concurrent Operations
```rust
// Use channels for communication between tasks
use tokio::sync::mpsc;

pub async fn parallel_downloads(
    urls: Vec<String>,
) -> Result<Vec<Package>, Error> {
    let (tx, mut rx) = mpsc::channel(urls.len());
    
    // Spawn download tasks
    for url in urls {
        let tx = tx.clone();
        tokio::spawn(async move {
            let result = download_package(&url).await;
            let _ = tx.send(result).await;
        });
    }
    
    // Collect results
    let mut packages = Vec::new();
    for _ in 0..urls.len() {
        if let Some(result) = rx.recv().await {
            packages.push(result?);
        }
    }
    
    Ok(packages)
}
```

## Security Considerations

### Input Validation
```rust
use std::path::{Path, PathBuf};

/// Safely joins paths, preventing directory traversal attacks
pub fn safe_join(base: &Path, path: &Path) -> Result<PathBuf, SecurityError> {
    let joined = base.join(path);
    let canonical = joined.canonicalize()
        .map_err(|_| SecurityError::InvalidPath)?;
    
    if !canonical.starts_with(base) {
        return Err(SecurityError::DirectoryTraversal);
    }
    
    Ok(canonical)
}

/// Validates package names to prevent injection attacks
pub fn validate_package_name(name: &str) -> Result<(), ValidationError> {
    if name.is_empty() || name.len() > 214 {
        return Err(ValidationError::InvalidLength);
    }
    
    if !name.chars().all(|c| c.is_alphanumeric() || "-_.".contains(c)) {
        return Err(ValidationError::InvalidCharacters);
    }
    
    Ok(())
}
```

### Cryptographic Operations
```rust
use ring::{digest, hmac};

/// Verifies package integrity using SHA-256 checksums
pub fn verify_package_integrity(
    data: &[u8],
    expected_hash: &str,
) -> Result<(), IntegrityError> {
    let actual = digest::digest(&digest::SHA256, data);
    let expected = hex::decode(expected_hash)
        .map_err(|_| IntegrityError::InvalidHash)?;
    
    if actual.as_ref() != expected.as_slice() {
        return Err(IntegrityError::HashMismatch);
    }
    
    Ok(())
}
```