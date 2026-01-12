# HPM Developer Documentation

This comprehensive guide provides everything developers need to understand, contribute to, and extend the HPM (Houdini Package Manager) codebase. It covers architecture, API references, development workflows, testing strategies, and contribution guidelines.

## Table of Contents

1. [Development Environment Setup](#development-environment-setup)
2. [Codebase Architecture](#codebase-architecture)
3. [API Reference](#api-reference)
4. [Development Workflows](#development-workflows)
5. [Testing Strategy](#testing-strategy)
6. [Code Standards and Guidelines](#code-standards-and-guidelines)
7. [Extension and Plugin Development](#extension-and-plugin-development)
8. [Contribution Guidelines](#contribution-guidelines)
9. [Troubleshooting and Debugging](#troubleshooting-and-debugging)
10. [Release and Deployment](#release-and-deployment)

## Development Environment Setup

### Prerequisites

#### Required Tools
- **Rust 1.70 or later** - The project requires modern Rust features
- **SideFX Houdini 19.5+** - For testing integration features and package compatibility
- **Git** - Version control and contribution workflow
- **Protocol Buffers Compiler (protoc)** - For gRPC code generation in the registry module

#### Optional Tools
- **cargo-machete** - Detect unused dependencies
- **cargo-tarpaulin** - Code coverage analysis
- **cargo-audit** - Security vulnerability scanning
- **hyperfine** - Performance benchmarking
- **pre-commit** - Git hooks for quality assurance

### Environment Setup

#### 1. Clone and Build
```bash
# Clone the repository
git clone https://github.com/hpm-org/hpm.git
cd hpm

# Build the entire workspace
cargo build --workspace

# Verify build success
cargo build --release
```

#### 2. Development Tools Installation
```bash
# Install additional cargo tools
cargo install cargo-machete cargo-audit cargo-tarpaulin

# Install pre-commit (Python tool)
pip install pre-commit
pre-commit install

# Install Protocol Buffers compiler (for registry development)
# macOS
brew install protobuf
# Ubuntu/Debian
sudo apt install protobuf-compiler
# Windows
# Download from https://github.com/protocolbuffers/protobuf/releases
```

#### 3. IDE Configuration

##### VS Code Setup
```json
// .vscode/settings.json
{
  "rust-analyzer.cargo.features": "all",
  "rust-analyzer.check.command": "clippy",
  "rust-analyzer.check.extraArgs": [
    "--workspace", 
    "--all-features", 
    "--", 
    "-D", 
    "warnings"
  ],
  "rust-analyzer.test.extraArgs": ["--", "--nocapture"],
  "rust-analyzer.cargo.extraEnv": {
    "RUST_LOG": "debug"
  }
}
```

##### Recommended VS Code Extensions
- **rust-analyzer** - Rust language server
- **CodeLLDB** - Debugging support
- **crates** - Cargo.toml management
- **Error Lens** - Inline error display
- **GitLens** - Git integration enhancement

#### 4. Environment Variables
```bash
# Development environment configuration
export RUST_LOG=debug                    # Enable debug logging
export HPM_DEV=1                         # Development mode flag
export PROPTEST_CASES=100               # Faster property tests during development

# Add to your shell configuration (.bashrc, .zshrc, etc.)
echo 'export PATH="$HOME/.cargo/bin:$PATH"' >> ~/.bashrc
```

## Codebase Architecture

### Workspace Structure

HPM uses a modular workspace architecture with clear separation of concerns:

```text
hpm/
├── src/lib.rs                          # Workspace documentation crate
├── Cargo.toml                          # Workspace manifest
├── crates/                             # Individual crates
│   ├── hpm-cli/                       # Command-line interface
│   │   ├── src/
│   │   │   ├── main.rs                # CLI entry point
│   │   │   ├── commands/              # Command implementations
│   │   │   ├── console.rs             # Output formatting
│   │   │   ├── error.rs               # CLI error handling
│   │   │   └── output.rs              # Machine-readable output
│   │   └── tests/                     # Integration tests
│   ├── hpm-core/                      # Core package management
│   │   ├── src/
│   │   │   ├── lib.rs                 # Public API
│   │   │   ├── storage.rs             # Package storage management
│   │   │   ├── discovery.rs           # Project discovery
│   │   │   ├── dependency.rs          # Dependency analysis
│   │   │   ├── manager.rs             # High-level operations
│   │   │   └── project.rs             # Project management
│   │   └── proptest-regressions/      # Property test regression files
│   ├── hpm-package/                   # Package manifest processing
│   │   └── src/
│   │       ├── lib.rs                 # Manifest parsing and validation
│   │       └── template.rs            # Package templates
│   ├── hpm-config/                    # Configuration management
│   │   └── src/lib.rs                 # Hierarchical configuration
│   ├── hpm-python/                    # Python dependency management
│   │   ├── src/
│   │   │   ├── lib.rs                 # Public API
│   │   │   ├── venv.rs                # Virtual environment management
│   │   │   ├── resolver.rs            # UV-powered dependency resolution
│   │   │   ├── integration.rs         # Houdini integration
│   │   │   ├── cleanup.rs             # Python cleanup operations
│   │   │   └── types.rs               # Python-specific types
│   │   └── proptest-regressions/      # Property test regression files
│   ├── hpm-registry/                  # QUIC/gRPC registry
│   │   ├── src/
│   │   │   ├── lib.rs                 # Public API
│   │   │   ├── client/                # Registry client
│   │   │   ├── server/                # Registry server
│   │   │   ├── proto/                 # Protocol buffer definitions
│   │   │   ├── types/                 # Authentication and error types
│   │   │   └── utils/                 # Compression and validation
│   │   ├── proto/                     # .proto source files
│   │   └── examples/                  # Usage examples
│   ├── hpm-resolver/                  # Dependency resolution engine
│   │   └── src/
│   │       ├── lib.rs                 # Public API
│   │       ├── solver.rs              # PubGrub-inspired solver
│   │       └── version.rs             # Version constraint handling
│   └── hpm-error/                     # Error handling infrastructure
│       └── src/lib.rs                 # Structured error types
├── docs/                              # Technical documentation
│   ├── user-guide.md                  # User-facing documentation
│   ├── technical-architecture.md      # System architecture
│   ├── developer-documentation.md     # This document
│   └── ...                           # Additional documentation
├── scripts/                           # Development scripts
│   ├── check-emojis.py               # Enforce no-emoji policy
│   └── install-git-hooks.sh          # Git hooks installation
└── tests/                            # Workspace-level integration tests
```

### Crate Responsibilities

#### Core Infrastructure Crates

**hpm-error**: Centralized error handling infrastructure
- Structured error types with contextual information
- Domain-specific error categorization
- Error propagation and conversion utilities
- Exit code standardization

**hpm-config**: Hierarchical configuration management
- TOML-based configuration parsing
- Environment variable integration
- Configuration validation and defaults
- Project discovery settings

#### Business Logic Crates

**hpm-core**: Core package management functionality
- Global package storage management
- Project discovery and validation
- Dependency graph analysis and cleanup
- High-level package operations

**hpm-package**: Package manifest processing
- HPM manifest (`hpm.toml`) parsing and validation
- Package template generation
- Houdini integration file generation
- Package metadata management

**hpm-python**: Python dependency management
- Content-addressable virtual environment sharing
- UV-powered dependency resolution with complete isolation
- Houdini Python path integration
- Python package cleanup operations

**hpm-resolver**: Advanced dependency resolution
- PubGrub-inspired incremental solver
- Version constraint handling and conflict detection
- Performance optimization for large dependency graphs
- Integration with HPM and Python ecosystems

#### Interface and Integration Crates

**hpm-cli**: Command-line user interface
- Argument parsing and validation
- Professional error reporting and help
- Multiple output format support
- Command orchestration and workflow management

**hpm-registry**: High-performance package registry
- QUIC transport with s2n-quic for performance
- gRPC protocol with Protocol Buffers
- Pluggable storage backends (Memory, PostgreSQL, S3)
- Authentication and security features

## API Reference

### Core Types and Traits

#### Package Management Core (hpm-core)

##### StorageManager
The central component for package storage operations.

```rust
pub struct StorageManager {
    storage_path: PathBuf,
    cache_path: PathBuf,
}

impl StorageManager {
    /// Create new storage manager with default paths
    pub fn new(config: StorageConfig) -> Result<Self, StorageError>;
    
    /// Check if package version exists in storage
    pub fn package_exists(&self, name: &str, version: &str) -> bool;
    
    /// List all installed packages
    pub async fn list_installed(&self) -> Result<Vec<InstalledPackage>, StorageError>;
    
    /// Install package to global storage
    pub async fn install_package(&self, spec: &PackageSpec) -> Result<InstallResult, StorageError>;
    
    /// Remove specific package version
    pub async fn remove_package(&self, name: &str, version: &str) -> Result<(), StorageError>;
    
    /// Clean orphaned packages (project-aware)
    pub async fn cleanup_unused(&self, projects: &ProjectsConfig) -> Result<CleanupResult, StorageError>;
    
    /// Comprehensive cleanup (packages + Python environments)
    pub async fn cleanup_comprehensive(&self, projects: &ProjectsConfig) -> Result<ComprehensiveCleanupResult, StorageError>;
}
```

##### ProjectDiscovery
Filesystem scanning and project validation.

```rust
pub struct ProjectDiscovery {
    config: ProjectsConfig,
}

impl ProjectDiscovery {
    /// Create project discovery with configuration
    pub fn new(config: ProjectsConfig) -> Self;
    
    /// Find all HPM-managed projects
    pub fn find_projects(&self) -> Result<Vec<DiscoveredProject>, DiscoveryError>;
    
    /// Discover single project at path
    pub fn discover_project(&self, path: &Path) -> Result<Option<DiscoveredProject>, DiscoveryError>;
    
    /// Validate project manifest
    pub fn validate_project(&self, project: &DiscoveredProject) -> Result<(), DiscoveryError>;
}

#[derive(Debug, Clone)]
pub struct DiscoveredProject {
    pub path: PathBuf,
    pub manifest: PackageManifest,
    pub dependencies: Vec<DependencySpec>,
    pub python_dependencies: Option<Vec<PythonDependencySpec>>,
}
```

##### DependencyGraph
Complete dependency analysis including cycle detection.

```rust
pub struct DependencyGraph {
    nodes: HashMap<PackageId, PackageNode>,
    edges: HashMap<PackageId, HashSet<PackageId>>,
}

impl DependencyGraph {
    /// Build dependency graph from discovered projects
    pub async fn build_from_projects(projects: &[DiscoveredProject]) -> Result<Self, DependencyError>;
    
    /// Add dependency relationship
    pub fn add_dependency(&mut self, from: &PackageId, to: &DependencySpec) -> Result<(), DependencyError>;
    
    /// Resolve transitive dependencies
    pub async fn resolve_transitive_dependencies(&mut self) -> Result<(), DependencyError>;
    
    /// Detect circular dependencies
    pub fn detect_cycles(&self) -> Option<Vec<PackageId>>;
    
    /// Get all packages reachable from root packages
    pub fn get_reachable_packages(&self, roots: &[PackageId]) -> HashSet<PackageId>;
    
    /// Calculate packages that would be orphaned if roots were removed
    pub fn calculate_orphans(&self, active_projects: &[DiscoveredProject]) -> HashSet<PackageId>;
}
```

#### Python Integration (hpm-python)

##### VenvManager
Content-addressable virtual environment management.

```rust
pub struct VenvManager {
    venvs_dir: PathBuf,
    uv_path: PathBuf,
}

impl VenvManager {
    /// Create virtual environment manager
    pub fn new() -> Result<Self, PythonError>;
    
    /// Create or reuse virtual environment for resolved dependencies
    pub async fn ensure_virtual_environment(&self, resolved: &ResolvedDependencies) -> Result<PathBuf, PythonError>;
    
    /// List all virtual environments
    pub async fn list_virtual_environments(&self) -> Result<Vec<VirtualEnvironment>, PythonError>;
    
    /// Remove virtual environment by hash
    pub async fn remove_virtual_environment(&self, venv_hash: &str) -> Result<(), PythonError>;
    
    /// Validate virtual environment integrity
    pub async fn validate_environment(&self, venv_path: &Path, expected: &ResolvedDependencies) -> Result<bool, PythonError>;
    
    /// Calculate content hash for dependency set
    pub fn calculate_content_hash(&self, resolved: &ResolvedDependencies) -> Result<String, PythonError>;
}

#[derive(Debug, Clone)]
pub struct ResolvedDependencies {
    pub python_version: String,
    pub packages: BTreeMap<String, ResolvedPackage>,
    pub resolution_time: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct ResolvedPackage {
    pub name: String,
    pub version: String,
    pub extras: Option<Vec<String>>,
    pub source: PackageSource,
}
```

##### Dependency Collection and Resolution
```rust
/// Collect Python dependencies from package manifests
pub async fn collect_python_dependencies(packages: &[PackageManifest]) -> Result<PythonDependencies, PythonError>;

/// Resolve dependencies using UV with conflict detection
pub async fn resolve_dependencies(collected: &PythonDependencies) -> Result<ResolvedDependencies, PythonError>;

/// Map Houdini version to Python version
pub fn houdini_to_python_version(houdini_version: &str) -> String;

#[derive(Debug, Default)]
pub struct PythonDependencies {
    pub requirements: BTreeMap<String, PythonDependencySpec>,
    pub houdini_version: Option<String>,
    pub conflicts: Vec<DependencyConflict>,
}

#[derive(Debug, Clone)]
pub struct PythonDependencySpec {
    pub name: String,
    pub version_spec: String,
    pub extras: Option<Vec<String>>,
    pub optional: bool,
    pub source_packages: Vec<String>, // Which HPM packages require this
}
```

#### Registry System (hpm-registry)

##### RegistryClient
High-performance QUIC-based registry client.

```rust
pub struct RegistryClient {
    connection: QuicConnection,
    auth_token: Option<AuthToken>,
    config: RegistryClientConfig,
}

impl RegistryClient {
    /// Connect to registry server
    pub async fn connect(config: RegistryClientConfig) -> Result<Self, RegistryError>;
    
    /// Set authentication token
    pub fn set_auth_token(&mut self, token: AuthToken);
    
    /// Search packages by query
    pub async fn search_packages(
        &mut self, 
        query: &str, 
        limit: Option<u32>, 
        offset: Option<u32>
    ) -> Result<SearchResults, RegistryError>;
    
    /// Download package by name and version
    pub async fn download_package(&mut self, name: &str, version: &str) -> Result<DownloadResult, RegistryError>;
    
    /// Publish package to registry
    pub async fn publish_package(
        &mut self, 
        name: &str, 
        version: &str, 
        package_data: Vec<u8>,
        description: Option<String>
    ) -> Result<PublishResult, RegistryError>;
    
    /// Verify package integrity
    pub fn verify_package_integrity(&self, data: &[u8], expected_checksum: &str) -> Result<bool, RegistryError>;
}

#[derive(Debug, Clone)]
pub struct RegistryClientConfig {
    pub server_address: String,
    pub timeout: Duration,
    pub max_retries: u32,
    pub compression: bool,
}
```

##### RegistryServer
Scalable registry server with pluggable storage.

```rust
pub struct RegistryServer {
    bind_address: SocketAddr,
    storage: Box<dyn Storage>,
    config: ServerConfig,
}

impl RegistryServer {
    /// Create server with storage backend
    pub fn new(bind_address: SocketAddr, storage: Box<dyn Storage>) -> Self;
    
    /// Start serving requests
    pub async fn serve(self) -> Result<(), RegistryError>;
    
    /// Configure server settings
    pub fn with_config(mut self, config: ServerConfig) -> Self;
}

#[async_trait]
pub trait Storage: Send + Sync {
    /// Store package data
    async fn store_package(&self, package: &PackageData) -> Result<String, StorageError>;
    
    /// Retrieve package by ID
    async fn get_package(&self, package_id: &str) -> Result<Option<PackageData>, StorageError>;
    
    /// Search packages
    async fn search_packages(&self, query: &SearchQuery) -> Result<Vec<PackageMetadata>, StorageError>;
    
    /// List package versions
    async fn list_versions(&self, name: &str) -> Result<Vec<String>, StorageError>;
    
    /// Delete package (admin operation)
    async fn delete_package(&self, package_id: &str) -> Result<(), StorageError>;
}
```

#### CLI System (hpm-cli)

##### Console Output and Error Handling
```rust
pub struct Console {
    verbosity: Verbosity,
    color_choice: ColorChoice,
    stdout: Box<dyn Write + Send>,
    stderr: Box<dyn Write + Send>,
}

impl Console {
    /// Create console with settings
    pub fn with_settings(verbosity: Verbosity, color_choice: ColorChoice) -> Self;
    
    /// Print success message
    pub fn success<S: AsRef<str>>(&mut self, message: S);
    
    /// Print error message
    pub fn error<S: AsRef<str>>(&mut self, message: S);
    
    /// Print warning message
    pub fn warn<S: AsRef<str>>(&mut self, message: S);
    
    /// Print info message
    pub fn info<S: AsRef<str>>(&mut self, message: S);
}

#[derive(Debug, thiserror::Error)]
pub enum CliError {
    #[error("Configuration error: {source}")]
    Config { 
        source: anyhow::Error,
        help: Option<String>
    },
    
    #[error("Package error: {source}")]
    Package {
        source: anyhow::Error,
        help: Option<String>
    },
    
    #[error("Network error: {source}")]
    Network {
        source: anyhow::Error,
        help: Option<String>
    },
    
    #[error("I/O error: {source}")]
    Io {
        source: anyhow::Error,
        help: Option<String>
    },
    
    #[error("Internal error: {source}")]
    Internal {
        source: anyhow::Error,
        help: Option<String>
    },
}

impl CliError {
    /// Create error with helpful context
    pub fn package(source: anyhow::Error, help: Option<String>) -> Self;
    pub fn config(source: anyhow::Error, help: Option<String>) -> Self;
    pub fn network(source: anyhow::Error, help: Option<String>) -> Self;
    pub fn io(source: anyhow::Error, help: Option<String>) -> Self;
    pub fn internal(source: anyhow::Error, help: Option<String>) -> Self;
    
    /// Print error with full context
    pub fn print_error(&self);
    
    /// Print simple error message
    pub fn print_simple(&self);
}
```

## Development Workflows

### Daily Development Workflow

#### 1. Feature Development
```bash
# Start new feature branch
git checkout -b feature/dependency-resolver-optimization

# Write failing tests first (TDD approach)
cargo test dependency_resolution_performance --lib

# Implement feature
# Edit crates/hpm-resolver/src/solver.rs

# Run tests frequently during development
cargo test -p hpm-resolver

# Run property tests to check for edge cases
PROPTEST_CASES=100 cargo test prop_ -p hpm-resolver

# Check code quality
cargo fmt
cargo clippy -p hpm-resolver -- -D warnings
```

#### 2. Integration Testing
```bash
# Test CLI integration
cargo test --test integration_tests

# Test specific command workflows
cargo run -- init test-package --description "Test package"
cargo run -- add utility-nodes --version "^2.1.0"
cargo run -- install
cargo run -- list
cargo run -- clean --dry-run

# Debug with logging
RUST_LOG=debug cargo run -- install --verbose
```

#### 3. Cross-Crate Testing
```bash
# Test entire workspace
cargo test --workspace --all-features

# Test specific interactions
cargo test -p hpm-core integration_test
cargo test -p hpm-python integration_tests

# Performance testing
hyperfine 'cargo run -- install' --warmup 3 --runs 10
```

### Quality Assurance Workflow

#### 1. Code Quality Checks
```bash
# Format code
cargo fmt --all

# Check for issues
cargo clippy --workspace --all-features -- -D warnings

# Security audit
cargo audit

# Check for unused dependencies
cargo machete

# Check for emoji usage (enforced policy)
python3 scripts/check-emojis.py
```

#### 2. Testing Levels
```bash
# Unit tests (fast, during development)
cargo test --lib --workspace

# Integration tests (slower, before commit)
cargo test --test integration_tests

# Property tests (thorough, periodic)
PROPTEST_CASES=1000 cargo test prop_ --workspace --all-features

# All tests (comprehensive, before PR)
cargo test --workspace --all-features
```

#### 3. Documentation Maintenance
```bash
# Generate and check documentation
cargo doc --workspace --all-features --no-deps --open

# Test documentation examples
cargo test --doc --workspace

# Update README examples if needed
```

### Testing Strategy

#### Property-Based Testing with Proptest

HPM uses property-based testing extensively for business logic validation:

```rust
use proptest::prelude::*;

#[cfg(test)]
mod tests {
    use super::*;

    proptest! {
        #[test]
        fn prop_version_parsing_roundtrip(
            major in 0u32..100,
            minor in 0u32..100, 
            patch in 0u32..100
        ) {
            let version = format!("{}.{}.{}", major, minor, patch);
            let parsed = Version::parse(&version).unwrap();
            let formatted = parsed.to_string();
            prop_assert_eq!(version, formatted);
        }

        #[test]
        fn prop_storage_operations_are_idempotent(
            package_name in "[a-z][a-z0-9-]{2,20}",
            version in r"[0-9]+\.[0-9]+\.[0-9]+"
        ) {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                let temp_dir = TempDir::new().unwrap();
                let storage = StorageManager::new(StorageConfig {
                    path: temp_dir.path().to_path_buf(),
                }).unwrap();

                // Install package twice - should be idempotent
                let spec = PackageSpec {
                    name: package_name.clone(),
                    version: version.clone(),
                };

                let result1 = storage.install_package(&spec).await;
                let result2 = storage.install_package(&spec).await;

                prop_assert!(result1.is_ok());
                prop_assert!(result2.is_ok());
                prop_assert!(storage.package_exists(&package_name, &version));
            });
        }
    }
}
```

#### Test Configuration
```bash
# Property test environment variables
export PROPTEST_CASES=256              # Number of test cases (default: 256)
export PROPTEST_MAX_SHRINK_ITERS=1024  # Shrinking iterations (default: 1024)  
export PROPTEST_TIMEOUT=5000           # Timeout per case in ms
export PROPTEST_VERBOSE=1              # Enable verbose output

# Development (faster)
export PROPTEST_CASES=100

# CI/CD (balanced)
export PROPTEST_CASES=512

# Comprehensive (thorough)
export PROPTEST_CASES=2000
```

#### Integration Testing Patterns

```rust
#[tokio::test]
async fn test_complete_package_workflow() {
    let temp_dir = TempDir::new().unwrap();
    let project_dir = temp_dir.path().join("test-project");
    
    // Test complete workflow from initialization to cleanup
    // 1. Initialize package
    let init_result = commands::init::init_package(InitOptions {
        name: Some("test-project".to_string()),
        base_dir: Some(temp_dir.path().to_path_buf()),
        ..Default::default()
    }).await;
    assert!(init_result.is_ok());
    
    // 2. Add dependency
    let add_result = commands::add::add_package(
        "utility-nodes".to_string(),
        Some("^2.1.0".to_string()),
        Some(project_dir.clone()),
        false
    ).await;
    assert!(add_result.is_ok());
    
    // 3. Install dependencies
    let install_result = commands::install::install_dependencies(Some(project_dir.clone())).await;
    assert!(install_result.is_ok());
    
    // 4. Verify installation
    let storage = StorageManager::new(StorageConfig::default()).unwrap();
    assert!(storage.package_exists("utility-nodes", "2.1.0"));
    
    // 5. Clean up (should not remove active packages)
    let cleanup_result = storage.cleanup_unused(&ProjectsConfig {
        explicit_paths: vec![project_dir],
        ..Default::default()
    }).await;
    assert!(cleanup_result.is_ok());
    assert_eq!(cleanup_result.unwrap().removed.len(), 0); // No packages should be removed
}
```

### Performance and Benchmarking

#### Micro-benchmarks
```rust
#[cfg(test)]
mod benches {
    use super::*;
    use std::time::Instant;
    
    #[tokio::test]
    async fn bench_dependency_resolution() {
        let projects = create_test_projects(100); // 100 projects with complex dependencies
        
        let start = Instant::now();
        let graph = DependencyGraph::build_from_projects(&projects).await.unwrap();
        let resolution_time = start.elapsed();
        
        println!("Dependency resolution for {} projects: {:?}", projects.len(), resolution_time);
        assert!(resolution_time < Duration::from_secs(5), "Dependency resolution too slow");
        
        let start = Instant::now();
        let orphans = graph.calculate_orphans(&projects);
        let cleanup_time = start.elapsed();
        
        println!("Cleanup analysis time: {:?}", cleanup_time);
        assert!(cleanup_time < Duration::from_secs(1), "Cleanup analysis too slow");
    }
}
```

#### System-level Benchmarks
```bash
# Benchmark complete workflows
hyperfine --warmup 3 --runs 10 \
  'cargo run -- init benchmark-package' \
  'cargo run -- add numpy --version ">=1.20.0"' \
  'cargo run -- install' \
  'cargo run -- clean --dry-run'

# Memory usage profiling
/usr/bin/time -v cargo run -- install --manifest large-project/hpm.toml

# Build time optimization
cargo build --timings
```

## Code Standards and Guidelines

### Rust Code Standards

#### 1. Error Handling Patterns
```rust
// ✅ Good: Domain-specific errors with context
#[derive(Debug, thiserror::Error)]
pub enum PackageError {
    #[error("Package not found: {name}@{version}")]
    NotFound { name: String, version: String },
    
    #[error("Invalid version specification: {spec}")]
    InvalidVersion { spec: String, source: semver::Error },
    
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

// ✅ Good: Application-level error handling
pub fn install_package(spec: &PackageSpec) -> Result<InstallResult> {
    let package_data = download_package(spec)
        .context("Failed to download package from registry")?;
        
    validate_package(&package_data)
        .with_context(|| format!("Package validation failed for {}", spec.name))?;
        
    Ok(InstallResult { /* ... */ })
}

// ❌ Bad: Generic errors without context
pub fn bad_function() -> Result<(), Box<dyn std::error::Error>> {
    // No context about what failed or why
}
```

#### 2. Async/Await Patterns
```rust
// ✅ Good: Proper async error handling and concurrency
pub async fn install_multiple_packages(specs: &[PackageSpec]) -> Result<Vec<InstallResult>> {
    let tasks: Vec<_> = specs.iter()
        .map(|spec| {
            let spec = spec.clone();
            tokio::spawn(async move {
                install_single_package(&spec).await
                    .with_context(|| format!("Failed to install {}", spec.name))
            })
        })
        .collect();
    
    let mut results = Vec::new();
    for task in tasks {
        results.push(task.await??);
    }
    
    Ok(results)
}

// ❌ Bad: Blocking operations in async context
pub async fn bad_async_function() -> Result<String> {
    // This blocks the entire async runtime
    let content = std::fs::read_to_string("file.txt")?;
    Ok(content)
}

// ✅ Good: Non-blocking async I/O
pub async fn good_async_function() -> Result<String> {
    let content = tokio::fs::read_to_string("file.txt").await?;
    Ok(content)
}
```

#### 3. Testing Patterns
```rust
// ✅ Good: Isolated, deterministic tests
#[tokio::test]
async fn test_storage_manager_cleanup() {
    let temp_dir = TempDir::new().unwrap();
    let storage = StorageManager::new(StorageConfig {
        path: temp_dir.path().to_path_buf(),
    }).unwrap();
    
    // Set up known test state
    storage.install_package(&test_package_spec()).await.unwrap();
    
    // Test cleanup logic
    let result = storage.cleanup_unused(&empty_projects_config()).await.unwrap();
    
    // Verify expected behavior
    assert_eq!(result.removed.len(), 1);
    assert!(!storage.package_exists("test-package", "1.0.0"));
    
    // TempDir automatically cleans up
}

// ❌ Bad: Tests with external dependencies
#[tokio::test]
async fn bad_test() {
    // Don't depend on external services
    let client = RegistryClient::connect("https://real-registry.com").await.unwrap();
    
    // Don't use global state
    std::env::set_current_dir("/tmp/test").unwrap();
    
    // Don't use hardcoded paths that might not exist
    let result = std::fs::read("/Users/developer/test-file.txt");
}
```

### Documentation Standards

#### 1. API Documentation
```rust
/// Manages the global package storage system with project-aware cleanup capabilities.
///
/// The `StorageManager` implements HPM's dual-storage architecture where packages are
/// stored globally in `~/.hpm/packages/` but referenced by individual projects through
/// lightweight manifest files. This design enables efficient disk usage through
/// deduplication while maintaining project isolation.
///
/// # Safety Guarantees
///
/// The storage manager provides strong safety guarantees:
/// - **No false positives**: Never removes packages needed by active projects
/// - **Atomic operations**: Package installations either complete fully or leave no traces  
/// - **Corruption resistance**: Validates package integrity before and after operations
///
/// # Examples
///
/// ```rust,no_run
/// use hpm_core::{StorageManager, StorageConfig};
/// 
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let config = StorageConfig::default();
/// let storage = StorageManager::new(config)?;
///
/// // Check if package exists
/// if storage.package_exists("utility-nodes", "2.1.0") {
///     println!("Package already installed");
/// }
///
/// // List all installed packages
/// let packages = storage.list_installed().await?;
/// for package in packages {
///     println!("Installed: {} v{}", package.name, package.version);
/// }
/// # Ok(())
/// # }
/// ```
///
/// # Error Handling
///
/// All operations return [`Result`] types with specific error variants:
/// - [`StorageError::PackageNotFound`] - Requested package doesn't exist
/// - [`StorageError::PermissionDenied`] - Insufficient file system permissions
/// - [`StorageError::CorruptedData`] - Package data validation failed
/// - [`StorageError::DiskFull`] - Insufficient disk space for operations
pub struct StorageManager {
    // ...
}
```

#### 2. Module Documentation
```rust
//! # HPM Python Dependency Management
//!
//! This module provides comprehensive Python dependency management for HPM packages,
//! solving the fundamental challenge of conflicting Python dependencies through
//! advanced virtual environment isolation and content-addressable sharing.
//!
//! ## Architecture Overview  
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                    Python Architecture                      │
//! ├─────────────────────────────────────────────────────────────┤
//! │ Package Manifests → Dependency Collection → UV Resolution  │
//! │                                         ↓                   │
//! │ Content-Addressable Virtual Environments ← Houdini Setup   │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use hpm_python::{initialize, collect_python_dependencies, resolve_dependencies};
//!
//! # async fn example() -> anyhow::Result<()> {
//! // Initialize Python dependency system
//! initialize().await?;
//!
//! // Collect and resolve dependencies  
//! let packages = vec![/* package manifests */];
//! let collected = collect_python_dependencies(&packages).await?;
//! let resolved = resolve_dependencies(&collected).await?;
//!
//! println!("Resolved {} Python packages", resolved.packages.len());
//! # Ok(())
//! # }
//! ```
```

#### 3. Error Documentation
```rust
/// Errors that can occur during package storage operations.
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    /// Package was not found in storage.
    ///
    /// This error occurs when trying to access a package that hasn't been installed
    /// or has been removed. Check the package name and version spelling, or use
    /// [`StorageManager::list_installed`] to see available packages.
    ///
    /// # Example Recovery
    ///
    /// ```rust,no_run
    /// # use hpm_core::{StorageManager, StorageError};
    /// # async fn example(storage: &StorageManager) -> Result<(), Box<dyn std::error::Error>> {
    /// match storage.get_package("nonexistent", "1.0.0").await {
    ///     Err(StorageError::PackageNotFound { name, version }) => {
    ///         println!("Package {}@{} not found. Available packages:", name, version);
    ///         let available = storage.list_installed().await?;
    ///         for pkg in available {
    ///             println!("  {}@{}", pkg.name, pkg.version);
    ///         }
    ///     }
    ///     Ok(package) => println!("Found package: {:?}", package),
    ///     Err(e) => return Err(e.into()),
    /// }
    /// # Ok(())
    /// # }
    /// ```
    #[error("Package not found: {name}@{version}")]
    PackageNotFound { name: String, version: String },
    
    /// I/O operation failed.
    ///
    /// This error wraps underlying I/O errors from file system operations.
    /// Common causes include permission denied, disk full, or corrupted file system.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    
    /// Package data is corrupted or invalid.
    ///
    /// This error indicates that package validation failed, possibly due to:
    /// - Corrupted download
    /// - Invalid package format
    /// - Checksum mismatch
    /// - Missing required files
    #[error("Package data corrupted: {reason}")]
    CorruptedData { reason: String },
}
```

### Performance Guidelines

#### 1. Memory Management
```rust
// ✅ Good: Use references to avoid unnecessary cloning
pub fn process_packages(packages: &[PackageManifest]) -> Result<ProcessingResult> {
    for package in packages {
        // Process each package by reference
        validate_package(package)?;
    }
    Ok(ProcessingResult::default())
}

// ✅ Good: Use Arc for shared data in async contexts
#[derive(Clone)]
pub struct SharedStorage {
    inner: Arc<RwLock<StorageManager>>,
}

// ❌ Bad: Unnecessary cloning
pub fn bad_process_packages(packages: Vec<PackageManifest>) -> Result<ProcessingResult> {
    for package in packages.clone() { // Unnecessary clone
        let cloned_package = package.clone(); // Another unnecessary clone
        validate_package(&cloned_package)?;
    }
    Ok(ProcessingResult::default())
}
```

#### 2. I/O Optimization
```rust
// ✅ Good: Batch operations and use async I/O
pub async fn install_packages_efficiently(specs: &[PackageSpec]) -> Result<Vec<InstallResult>> {
    // Batch downloads
    let download_tasks: Vec<_> = specs.iter()
        .map(|spec| download_package_async(spec))
        .collect();
    
    let downloaded: Result<Vec<_>, _> = try_join_all(download_tasks).await;
    let packages = downloaded?;
    
    // Batch file operations
    let mut results = Vec::with_capacity(specs.len());
    for (spec, package_data) in specs.iter().zip(packages.iter()) {
        let result = install_package_data(spec, package_data).await?;
        results.push(result);
    }
    
    Ok(results)
}

// ❌ Bad: Sequential I/O operations
pub async fn install_packages_slowly(specs: &[PackageSpec]) -> Result<Vec<InstallResult>> {
    let mut results = Vec::new();
    
    for spec in specs {
        // Downloads happen one at a time
        let package_data = download_package_async(spec).await?;
        let result = install_package_data(spec, &package_data).await?;
        results.push(result);
    }
    
    Ok(results)
}
```

## Extension and Plugin Development

### Storage Backend Extensions

Create custom storage backends by implementing the `Storage` trait:

```rust
use async_trait::async_trait;
use hpm_registry::server::Storage;

pub struct RedisStorage {
    client: redis::aio::Connection,
    compression: bool,
}

#[async_trait]
impl Storage for RedisStorage {
    async fn store_package(&self, package: &PackageData) -> Result<String, StorageError> {
        let package_id = generate_package_id(&package.name, &package.version);
        
        let data = if self.compression {
            compress_package_data(&package.data)?
        } else {
            package.data.clone()
        };
        
        self.client.set(&package_id, data).await
            .map_err(|e| StorageError::Backend(e.into()))?;
            
        // Store metadata separately for efficient queries
        let metadata_key = format!("meta:{}", package_id);
        let metadata = serde_json::to_vec(&package.metadata)?;
        self.client.set(&metadata_key, metadata).await
            .map_err(|e| StorageError::Backend(e.into()))?;
            
        Ok(package_id)
    }
    
    async fn get_package(&self, package_id: &str) -> Result<Option<PackageData>, StorageError> {
        let data: Option<Vec<u8>> = self.client.get(package_id).await
            .map_err(|e| StorageError::Backend(e.into()))?;
            
        match data {
            Some(raw_data) => {
                let decompressed_data = if self.compression {
                    decompress_package_data(&raw_data)?
                } else {
                    raw_data
                };
                
                // Get metadata
                let metadata_key = format!("meta:{}", package_id);
                let metadata_raw: Vec<u8> = self.client.get(&metadata_key).await
                    .map_err(|e| StorageError::Backend(e.into()))?;
                let metadata: PackageMetadata = serde_json::from_slice(&metadata_raw)?;
                
                Ok(Some(PackageData {
                    data: decompressed_data,
                    metadata,
                }))
            }
            None => Ok(None),
        }
    }
    
    // ... implement other required methods
}
```

### Command Extensions

Add custom CLI commands by extending the command system:

```rust
use clap::Subcommand;
use hpm_cli::{CliResult, ExitStatus};

#[derive(Subcommand)]
pub enum CustomCommands {
    /// Analyze package dependencies for security vulnerabilities
    SecurityScan {
        /// Package to scan
        #[arg(short, long)]
        package: Option<String>,
        
        /// Severity threshold  
        #[arg(long, default_value = "medium")]
        severity: String,
        
        /// Output format
        #[arg(long, value_enum)]
        format: Option<OutputFormat>,
    },
    
    /// Generate package dependency graph visualization
    GraphViz {
        /// Output file
        #[arg(short, long)]
        output: PathBuf,
        
        /// Include development dependencies
        #[arg(long)]
        include_dev: bool,
    },
}

impl CustomCommands {
    pub async fn execute(self) -> CliResult<ExitStatus> {
        match self {
            Self::SecurityScan { package, severity, format } => {
                self.execute_security_scan(package, severity, format).await
            }
            Self::GraphViz { output, include_dev } => {
                self.execute_graph_viz(output, include_dev).await
            }
        }
    }
    
    async fn execute_security_scan(
        &self, 
        package: Option<String>, 
        severity: String, 
        format: Option<OutputFormat>
    ) -> CliResult<ExitStatus> {
        // Custom security scanning implementation
        let scanner = SecurityScanner::new(&severity);
        
        let results = if let Some(pkg) = package {
            scanner.scan_package(&pkg).await?
        } else {
            scanner.scan_all_installed().await?
        };
        
        match format.unwrap_or(OutputFormat::Human) {
            OutputFormat::Human => {
                for vulnerability in results.vulnerabilities {
                    println!("🚨 {} - {}: {}", 
                        vulnerability.severity,
                        vulnerability.package,
                        vulnerability.description
                    );
                }
            }
            OutputFormat::Json => {
                println!("{}", serde_json::to_string_pretty(&results)?);
            }
            _ => {} // Handle other formats
        }
        
        Ok(ExitStatus::Success)
    }
}
```

### Plugin System (Future Architecture)

Future versions will support a formal plugin system:

```rust
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Plugin trait for extending HPM functionality
#[async_trait]
pub trait HpmPlugin: Send + Sync {
    /// Plugin metadata
    fn metadata(&self) -> PluginMetadata;
    
    /// Initialize plugin with HPM context
    async fn initialize(&mut self, context: &PluginContext) -> Result<(), PluginError>;
    
    /// Execute plugin command
    async fn execute(&self, command: &str, args: &[String]) -> Result<PluginResult, PluginError>;
    
    /// Cleanup plugin resources
    async fn cleanup(&mut self) -> Result<(), PluginError>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginMetadata {
    pub name: String,
    pub version: String,
    pub description: String,
    pub author: String,
    pub commands: Vec<PluginCommand>,
    pub dependencies: Vec<PluginDependency>,
}

#[derive(Debug, Clone)]
pub struct PluginContext {
    pub hpm_version: String,
    pub config: Config,
    pub storage: Arc<StorageManager>,
}

// Example plugin implementation
pub struct LintPlugin {
    rules: Vec<LintRule>,
}

#[async_trait]
impl HpmPlugin for LintPlugin {
    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            name: "hpm-lint".to_string(),
            version: "1.0.0".to_string(),
            description: "Package quality linting plugin".to_string(),
            author: "HPM Contributors".to_string(),
            commands: vec![
                PluginCommand {
                    name: "lint".to_string(),
                    description: "Run quality checks on packages".to_string(),
                    args: vec![],
                }
            ],
            dependencies: vec![],
        }
    }
    
    async fn initialize(&mut self, _context: &PluginContext) -> Result<(), PluginError> {
        // Load linting rules from configuration
        self.rules = load_default_lint_rules();
        Ok(())
    }
    
    async fn execute(&self, command: &str, args: &[String]) -> Result<PluginResult, PluginError> {
        match command {
            "lint" => {
                let target = args.get(0).map(|s| s.as_str()).unwrap_or(".");
                let results = self.lint_package(target).await?;
                Ok(PluginResult::success_with_data(serde_json::to_value(results)?))
            }
            _ => Err(PluginError::UnknownCommand { command: command.to_string() }),
        }
    }
    
    async fn cleanup(&mut self) -> Result<(), PluginError> {
        self.rules.clear();
        Ok(())
    }
}
```

## Contribution Guidelines

### Getting Started with Contributions

#### 1. Issue Discussion and Planning
Before starting significant work:

1. **Check existing issues** - Search for related issues or discussions
2. **Create issue for new features** - Describe the problem and proposed solution
3. **Discuss approach** - Get feedback from maintainers on the technical approach
4. **Break down large features** - Split large features into smaller, reviewable PRs

#### 2. Development Process

##### Branch Naming Convention
```bash
# Feature development
git checkout -b feature/dependency-resolver-optimization

# Bug fixes
git checkout -b fix/cleanup-false-positive-detection

# Documentation improvements  
git checkout -b docs/update-api-reference

# Infrastructure changes
git checkout -b ci/improve-test-coverage-reporting
```

##### Commit Message Format
```bash
# Format: <type>(<scope>): <description>
feat(core): implement project-aware package cleanup
fix(python): resolve virtual environment hash collision
docs(readme): update installation instructions
test(cli): add integration tests for update command
refactor(storage): optimize package existence checking
```

#### 3. Pull Request Process

##### PR Description Template
```markdown
## Description
Brief summary of changes and motivation.

## Changes Made
- [ ] Added new feature X
- [ ] Fixed bug in component Y
- [ ] Updated documentation for Z

## Testing
- [ ] Added unit tests for new functionality
- [ ] Added integration tests where appropriate
- [ ] All tests pass locally
- [ ] Property tests pass with increased case count

## Breaking Changes
- [ ] No breaking changes
- [ ] Breaking changes documented below

List any breaking changes and migration path.

## Documentation  
- [ ] API documentation updated
- [ ] User guide updated if needed
- [ ] CHANGELOG.md updated

## Checklist
- [ ] Code follows project style guidelines
- [ ] Self-review completed
- [ ] Tests added/updated appropriately
- [ ] Documentation updated
```

##### Quality Checklist for Contributors
```bash
# Before pushing, ensure all checks pass
cargo fmt --all
cargo clippy --workspace --all-features -- -D warnings
cargo test --workspace --all-features
PROPTEST_CASES=500 cargo test prop_ --workspace --all-features
cargo audit
cargo machete
python3 scripts/check-emojis.py
```

#### 4. Code Review Guidelines

##### For Contributors
- **Self-review** - Review your own changes before requesting review
- **Small PRs** - Keep changes focused and reviewable (< 500 lines when possible)
- **Clear descriptions** - Explain the why, not just the what
- **Respond promptly** - Address review feedback in a timely manner
- **Ask questions** - Don't hesitate to ask for clarification on feedback

##### For Reviewers
- **Constructive feedback** - Focus on code improvement, not criticism
- **Explain rationale** - Explain why changes are needed
- **Suggest solutions** - Don't just point out problems, suggest improvements
- **Acknowledge good work** - Recognize well-written code and thoughtful solutions
- **Check testing** - Ensure adequate test coverage for changes

### Contribution Areas

#### High-Impact Areas (Beginner Friendly)
1. **Documentation improvements** - API docs, user guides, examples
2. **Test coverage** - Unit tests, integration tests, edge cases
3. **Error messages** - More helpful error messages and suggestions
4. **CLI usability** - Better help text, command examples, user experience

#### Intermediate Areas
1. **Performance optimizations** - Profile and optimize hot paths
2. **Feature enhancements** - Extend existing functionality
3. **Registry integration** - Connect CLI with registry system  
4. **Python integration** - Improve virtual environment management

#### Advanced Areas  
1. **Dependency resolution** - Enhance PubGrub-inspired solver
2. **Network protocols** - QUIC/gRPC performance optimizations
3. **Storage backends** - New storage implementations
4. **Plugin system** - Design and implement plugin architecture

## Troubleshooting and Debugging

### Development Environment Issues

#### Rust Toolchain Issues
```bash
# Update Rust toolchain
rustup update stable

# Check Rust version
rustc --version
# Should be 1.70 or later

# Reset toolchain if needed
rustup default stable
rustup component add rustfmt clippy

# Clear cargo cache if corrupted
cargo clean
rm -rf ~/.cargo/registry/cache
```

#### Build Issues
```bash  
# Clear build cache
cargo clean

# Update dependencies
cargo update

# Check for unused dependencies
cargo machete

# Verbose build to identify issues
cargo build --verbose
```

#### Test Issues
```bash
# Run tests with output
cargo test -- --nocapture

# Run single test
cargo test test_name -- --exact

# Debug test with logging
RUST_LOG=debug cargo test test_name -- --nocapture

# Property test debugging
PROPTEST_VERBOSE=1 PROPTEST_CASES=10 cargo test prop_failing_test -- --nocapture
```

### Runtime Debugging

#### Enable Debug Logging
```bash
# Full debug logging
export RUST_LOG=debug

# Module-specific logging
export RUST_LOG=hpm_core=debug,hpm_python=trace

# Specific operation debugging
RUST_LOG=debug cargo run -- install --verbose
RUST_LOG=debug cargo run -- clean --dry-run --verbose
```

#### Common Error Patterns

##### File System Permission Issues
```bash
# Check permissions on HPM directories
ls -la ~/.hpm/
ls -la ~/.hpm/packages/

# Fix permissions if needed
chmod -R u+w ~/.hpm/
```

##### Virtual Environment Issues
```bash
# Check UV binary
which uv
# Should show ~/.hpm/bin/uv or system installation

# Debug Python resolution  
RUST_LOG=hpm_python=debug cargo run -- install --verbose

# Check virtual environments
ls -la ~/.hpm/venvs/
```

##### Network Issues
```bash
# Test registry connectivity
curl -v https://packages.houdini.org/health

# Debug with proxy settings
export HTTPS_PROXY=http://proxy.company.com:8080
cargo run -- search test-package --verbose
```

### Performance Debugging

#### Build Performance
```bash
# Analyze build times
cargo build --timings

# Parallel compilation
export CARGO_BUILD_JOBS=8
```

#### Runtime Performance
```bash
# Profile with perf (Linux)
perf record cargo run -- install large-project
perf report

# Memory profiling with valgrind
valgrind --tool=massif cargo run -- install

# Simple timing
time cargo run -- install
```

#### Test Performance  
```bash
# Parallel test execution
cargo test --jobs 8

# Time individual test suites
time cargo test -p hpm-core
time cargo test -p hpm-python

# Property test performance
hyperfine 'PROPTEST_CASES=100 cargo test prop_version_parsing'
```

## Release and Deployment

### Release Process

#### Version Management
HPM uses semantic versioning (SemVer) with automated changelog generation.

##### Version Bumping
```bash
# Minor release (new features, backward compatible)
cargo set-version --bump minor

# Patch release (bug fixes, backward compatible)  
cargo set-version --bump patch

# Major release (breaking changes)
cargo set-version --bump major

# Specific version
cargo set-version 1.2.3
```

##### Pre-Release Checklist
```bash
# 1. Comprehensive testing
PROPTEST_CASES=2000 cargo test --workspace --all-features

# 2. Security audit
cargo audit

# 3. Dependency analysis  
cargo machete
cargo outdated

# 4. Performance validation
hyperfine 'cargo run --release -- install' --warmup 3

# 5. Documentation updates
cargo doc --workspace --all-features --no-deps
```

#### Release Build
```bash
# Optimized release build
cargo build --release --workspace

# Cross-platform builds (future)
cross build --target x86_64-unknown-linux-gnu --release
cross build --target x86_64-pc-windows-gnu --release
cross build --target x86_64-apple-darwin --release
```

### Deployment Architecture

#### Registry Deployment
```yaml
# docker-compose.yml for registry deployment
version: '3.8'
services:
  hpm-registry:
    build: 
      context: .
      dockerfile: Dockerfile.registry
    ports:
      - "8443:8443"
    environment:
      - DATABASE_URL=postgresql://hpm:password@postgres:5432/hpm_registry
      - REGISTRY_BIND_ADDR=0.0.0.0:8443
      - RUST_LOG=info
    depends_on:
      - postgres
    
  postgres:
    image: postgres:15
    environment:
      - POSTGRES_DB=hpm_registry
      - POSTGRES_USER=hpm  
      - POSTGRES_PASSWORD=password
    volumes:
      - postgres_data:/var/lib/postgresql/data
    
volumes:
  postgres_data:
```

#### Production Configuration
```toml
# Production registry configuration
[server]
bind_address = "0.0.0.0:8443"
max_connections = 1000
request_timeout = "30s"
compression = true

[storage]
backend = "postgresql"
connection_url = "${DATABASE_URL}"
pool_size = 20

[auth]
token_expiry = "30d"
require_auth_for_publish = true
require_auth_for_download = false

[security]
rate_limit = 100  # requests per minute
max_package_size = "500MB"
allowed_compression_types = ["zstd", "gzip"]

[logging]
level = "info"
format = "json"
```

This developer documentation provides comprehensive guidance for contributing to HPM. The modular architecture, comprehensive testing strategy, and clear development workflows ensure that contributions can be made safely and effectively while maintaining the project's high quality standards.