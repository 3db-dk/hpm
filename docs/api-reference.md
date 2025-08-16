# HPM API Reference

This document provides a comprehensive reference for all public APIs, types, functions, and modules in the HPM (Houdini Package Manager) system. It serves as the authoritative source for developers building with or extending HPM.

## Table of Contents

1. [Module Overview](#module-overview)
2. [Core Package Management (hpm-core)](#core-package-management-hpm-core)
3. [Python Integration (hpm-python)](#python-integration-hpm-python)
4. [Registry System (hpm-registry)](#registry-system-hpm-registry)
5. [Package Processing (hpm-package)](#package-processing-hpm-package)
6. [Configuration Management (hpm-config)](#configuration-management-hpm-config)
7. [Dependency Resolution (hpm-resolver)](#dependency-resolution-hpm-resolver)
8. [CLI System (hpm-cli)](#cli-system-hpm-cli)
9. [Error Handling (hpm-error)](#error-handling-hpm-error)
10. [Type Reference](#type-reference)
11. [Trait Reference](#trait-reference)

## Module Overview

HPM is organized as a modular workspace where each crate provides specific functionality with minimal coupling:

```text
┌─────────────────────────────────────────────────────────────────────────────────┐
│                              HPM Module Structure                               │
├─────────────────────────────────────────────────────────────────────────────────┤
│                                                                                 │
│  User Interface                                                                 │
│  ┌─────────────────────────────────────────────────────────────────────────┐   │
│  │  hpm-cli        │ Command-line interface, error handling, output       │   │
│  └─────────────────────────────────────────────────────────────────────────┘   │
│                                    │                                           │
│                                    ▼                                           │
│  Core Functionality                                                            │
│  ┌─────────────────────────────────────────────────────────────────────────┐   │
│  │  hmp-core       │ Storage, discovery, dependency analysis, cleanup     │   │
│  │  hpm-package    │ Manifest processing, templates, Houdini integration  │   │
│  │  hpm-python     │ Python venv management, UV integration, cleanup      │   │
│  │  hpm-resolver   │ PubGrub dependency resolution engine                 │   │
│  └─────────────────────────────────────────────────────────────────────────┘   │
│                                    │                                           │
│                                    ▼                                           │
│  Infrastructure                                                                │
│  ┌─────────────────────────────────────────────────────────────────────────┐   │
│  │  hpm-registry   │ QUIC/gRPC registry client/server, authentication     │   │
│  │  hpm-config     │ Configuration management, project discovery settings │   │
│  │  hpm-error      │ Structured error types, error handling utilities     │   │
│  └─────────────────────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────────────────────┘
```

## Core Package Management (hpm-core)

The `hpm-core` crate provides the fundamental package management functionality including storage, project discovery, and dependency analysis.

### Storage Management

#### StorageManager

Central component for managing the global package storage system.

```rust
pub struct StorageManager {
    // Private fields
}

impl StorageManager {
    /// Create storage manager with configuration
    pub fn new(config: StorageConfig) -> Result<Self, StorageError>
    
    /// Check if package version exists in storage  
    pub fn package_exists(&self, name: &str, version: &str) -> bool
    
    /// Get path to package in storage
    pub fn get_package_path(&self, name: &str, version: &str) -> PathBuf
    
    /// List all installed packages
    pub async fn list_installed(&self) -> Result<Vec<InstalledPackage>, StorageError>
    
    /// Install package to global storage
    pub async fn install_package(&self, spec: &PackageSpec) -> Result<InstallResult, StorageError>
    
    /// Remove specific package version  
    pub async fn remove_package(&self, name: &str, version: &str) -> Result<(), StorageError>
    
    /// Clean orphaned packages (dry run)
    pub async fn cleanup_unused_dry_run(
        &self, 
        projects: &ProjectsConfig
    ) -> Result<Vec<PackageId>, StorageError>
    
    /// Clean orphaned packages 
    pub async fn cleanup_unused(&self, projects: &ProjectsConfig) -> Result<CleanupResult, StorageError>
    
    /// Comprehensive cleanup (packages + Python environments)
    pub async fn cleanup_comprehensive(
        &self, 
        projects: &ProjectsConfig
    ) -> Result<ComprehensiveCleanupResult, StorageError>
}
```

#### StorageConfig

Configuration for the storage manager.

```rust
#[derive(Debug, Clone)]
pub struct StorageConfig {
    /// Root directory for package storage (default: ~/.hpm)
    pub storage_path: PathBuf,
    
    /// Cache directory for downloads (default: ~/.hpm/cache)
    pub cache_path: PathBuf,
    
    /// Enable compression for package storage
    pub compression_enabled: bool,
    
    /// Maximum parallel download operations
    pub max_parallel_downloads: usize,
}

impl Default for StorageConfig {
    fn default() -> Self
}
```

#### Package Types

```rust
#[derive(Debug, Clone)]
pub struct PackageSpec {
    pub name: String,
    pub version: String,
    pub source: PackageSource,
}

#[derive(Debug, Clone)]
pub enum PackageSource {
    Registry { url: String },
    Git { url: String, reference: GitReference },
    Local { path: PathBuf },
}

#[derive(Debug, Clone)]
pub struct InstalledPackage {
    pub name: String,
    pub version: String,
    pub install_date: DateTime<Utc>,
    pub storage_path: PathBuf,
    pub metadata: PackageMetadata,
}

#[derive(Debug)]
pub struct InstallResult {
    pub package_id: PackageId,
    pub install_path: PathBuf,
    pub files_created: Vec<PathBuf>,
    pub install_time: Duration,
}

#[derive(Debug)]
pub struct CleanupResult {
    pub removed: Vec<PackageId>,
    pub space_freed: u64,
    pub cleanup_time: Duration,
}

#[derive(Debug)]
pub struct ComprehensiveCleanupResult {
    pub removed_packages: Vec<PackageId>,
    pub python_cleanup: PythonCleanupResult,
    pub total_space_freed: u64,
    pub cleanup_time: Duration,
}

impl ComprehensiveCleanupResult {
    pub fn format_total_space_freed(&self) -> String
}
```

### Project Discovery

#### ProjectDiscovery

Filesystem scanning and project validation system.

```rust
pub struct ProjectDiscovery {
    // Private fields
}

impl ProjectDiscovery {
    /// Create project discovery with configuration
    pub fn new(config: ProjectsConfig) -> Self
    
    /// Find all HPM-managed projects using configuration
    pub fn find_projects(&self) -> Result<Vec<DiscoveredProject>, DiscoveryError>
    
    /// Discover single project at specific path
    pub fn discover_project(&self, path: &Path) -> Result<Option<DiscoveredProject>, DiscoveryError>
    
    /// Validate project configuration
    pub fn validate_project(&self, project: &DiscoveredProject) -> Result<(), DiscoveryError>
    
    /// Search for projects recursively in directory
    pub fn search_recursive(
        &self, 
        root: &Path, 
        current_depth: usize, 
        found_projects: &mut Vec<DiscoveredProject>
    ) -> Result<(), DiscoveryError>
}
```

#### Project Types

```rust
#[derive(Debug, Clone)]
pub struct DiscoveredProject {
    /// Path to project directory containing hpm.toml
    pub path: PathBuf,
    
    /// Parsed package manifest
    pub manifest: PackageManifest,
    
    /// HPM package dependencies
    pub dependencies: Vec<DependencySpec>,
    
    /// Python dependencies (if any)
    pub python_dependencies: Option<Vec<PythonDependencySpec>>,
    
    /// Project discovery metadata
    pub discovery_metadata: DiscoveryMetadata,
}

#[derive(Debug, Clone)]
pub struct DiscoveryMetadata {
    pub discovered_at: DateTime<Utc>,
    pub manifest_path: PathBuf,
    pub manifest_size: u64,
    pub manifest_modified: SystemTime,
}
```

### Dependency Analysis

#### DependencyGraph

Complete dependency modeling with cycle detection and reachability analysis.

```rust
pub struct DependencyGraph {
    // Private fields
}

impl DependencyGraph {
    /// Create empty dependency graph
    pub fn new() -> Self
    
    /// Build dependency graph from discovered projects
    pub async fn build_from_projects(
        projects: &[DiscoveredProject]
    ) -> Result<Self, DependencyError>
    
    /// Add package node to graph
    pub fn add_package(&mut self, package_id: PackageId, metadata: PackageNode) -> Result<(), DependencyError>
    
    /// Add dependency edge between packages  
    pub fn add_dependency(
        &mut self, 
        from: &PackageId, 
        to: &DependencySpec
    ) -> Result<(), DependencyError>
    
    /// Resolve all transitive dependencies
    pub async fn resolve_transitive_dependencies(&mut self) -> Result<(), DependencyError>
    
    /// Detect circular dependencies
    pub fn detect_cycles(&self) -> Option<Vec<PackageId>>
    
    /// Get packages reachable from given roots
    pub fn get_reachable_packages(&self, roots: &[PackageId]) -> HashSet<PackageId>
    
    /// Calculate packages that would be orphaned
    pub fn calculate_orphans(&self, active_projects: &[DiscoveredProject]) -> HashSet<PackageId>
    
    /// Get package metadata
    pub fn get_package(&self, id: &PackageId) -> Option<&PackageNode>
    
    /// Get dependencies of package
    pub fn get_dependencies(&self, id: &PackageId) -> Vec<&PackageId>
    
    /// Get packages that depend on given package
    pub fn get_dependents(&self, id: &PackageId) -> Vec<&PackageId>
}
```

#### Dependency Types

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PackageId {
    pub name: String,
    pub version: String,
}

impl std::fmt::Display for PackageId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result
}

impl std::str::FromStr for PackageId {
    type Err = DependencyError;
    fn from_str(s: &str) -> Result<Self, Self::Err>
}

#[derive(Debug, Clone)]
pub struct PackageNode {
    pub id: PackageId,
    pub metadata: PackageMetadata,
    pub install_status: InstallStatus,
    pub discovery_source: DiscoverySource,
}

#[derive(Debug, Clone)]
pub enum InstallStatus {
    Installed { path: PathBuf, install_time: DateTime<Utc> },
    Missing,
    Corrupted { reason: String },
}

#[derive(Debug, Clone)]
pub enum DiscoverySource {
    Project { project_path: PathBuf },
    Transitive { required_by: Vec<PackageId> },
    Explicit,
}

#[derive(Debug, Clone)]
pub struct DependencySpec {
    pub name: String,
    pub version_requirement: String,
    pub optional: bool,
    pub source: Option<PackageSource>,
    pub features: Option<Vec<String>>,
}
```

### High-Level Management

#### PackageManager

High-level package operations that orchestrate the core subsystems.

```rust
pub struct PackageManager {
    storage: StorageManager,
    discovery: ProjectDiscovery,
    python_manager: Option<Arc<VenvManager>>,
}

impl PackageManager {
    /// Create package manager with configuration
    pub fn new(config: Config) -> Result<Self, PackageError>
    
    /// Initialize new package with template
    pub async fn init_package(&self, options: InitOptions) -> Result<InitResult, PackageError>
    
    /// Install package and all dependencies
    pub async fn install_package(&self, spec: &PackageSpec) -> Result<InstallResult, PackageError>
    
    /// Install dependencies from manifest
    pub async fn install_from_manifest(&self, manifest_path: &Path) -> Result<InstallResult, PackageError>
    
    /// Update package to latest compatible version
    pub async fn update_package(&self, name: &str, constraints: Option<&str>) -> Result<UpdateResult, PackageError>
    
    /// Remove package and clean up unused dependencies
    pub async fn remove_package(&self, name: &str, version: &str) -> Result<RemoveResult, PackageError>
    
    /// List installed packages with metadata
    pub async fn list_packages(&self, filter: Option<PackageFilter>) -> Result<Vec<PackageInfo>, PackageError>
    
    /// Perform comprehensive system cleanup
    pub async fn cleanup_system(&self, options: CleanupOptions) -> Result<CleanupResult, PackageError>
    
    /// Validate package configuration
    pub async fn validate_package(&self, path: &Path) -> Result<ValidationResult, PackageError>
}
```

### Error Types

```rust
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("Package not found: {name}@{version}")]
    PackageNotFound { name: String, version: String },
    
    #[error("Package already exists: {name}@{version}")]
    PackageAlreadyExists { name: String, version: String },
    
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    
    #[error("Permission denied: {operation}")]
    PermissionDenied { operation: String },
    
    #[error("Disk full: required {required} bytes, available {available} bytes")]
    DiskFull { required: u64, available: u64 },
    
    #[error("Package corrupted: {reason}")]
    CorruptedData { reason: String },
    
    #[error("Configuration error: {0}")]
    Config(#[from] ConfigError),
}

#[derive(Debug, thiserror::Error)]
pub enum DiscoveryError {
    #[error("Invalid project structure at {path}: {reason}")]
    InvalidProjectStructure { path: PathBuf, reason: String },
    
    #[error("Manifest parsing failed: {0}")]
    ManifestParse(#[from] toml::de::Error),
    
    #[error("I/O error during discovery: {0}")]
    Io(#[from] std::io::Error),
    
    #[error("Permission denied accessing {path}")]
    PermissionDenied { path: PathBuf },
    
    #[error("Search depth limit exceeded: {max_depth}")]
    MaxDepthExceeded { max_depth: usize },
}

#[derive(Debug, thiserror::Error)]
pub enum DependencyError {
    #[error("Circular dependency detected: {cycle:?}")]
    CircularDependency { cycle: Vec<PackageId> },
    
    #[error("Dependency conflict: {package} requires {requirement1} and {requirement2}")]
    VersionConflict { 
        package: String, 
        requirement1: String, 
        requirement2: String 
    },
    
    #[error("Package not found in dependency resolution: {package}")]
    PackageNotFound { package: PackageId },
    
    #[error("Invalid version requirement: {requirement}")]
    InvalidVersionRequirement { requirement: String },
    
    #[error("Resolution timeout after {duration:?}")]
    ResolutionTimeout { duration: Duration },
}
```

## Python Integration (hpm-python)

The `hpm-python` crate provides comprehensive Python dependency management with content-addressable virtual environment sharing.

### Virtual Environment Management

#### VenvManager

Content-addressable virtual environment management system.

```rust
pub struct VenvManager {
    // Private fields
}

impl VenvManager {
    /// Create virtual environment manager
    pub fn new() -> Result<Self, PythonError>
    
    /// Create virtual environment manager with custom paths
    pub fn with_paths(venvs_dir: PathBuf, uv_path: PathBuf) -> Result<Self, PythonError>
    
    /// Create or reuse virtual environment for resolved dependencies
    pub async fn ensure_virtual_environment(
        &self, 
        resolved: &ResolvedDependencies
    ) -> Result<PathBuf, PythonError>
    
    /// List all virtual environments
    pub async fn list_virtual_environments(&self) -> Result<Vec<VirtualEnvironment>, PythonError>
    
    /// Get virtual environment by content hash
    pub async fn get_virtual_environment(
        &self, 
        content_hash: &str
    ) -> Result<Option<VirtualEnvironment>, PythonError>
    
    /// Remove virtual environment by hash
    pub async fn remove_virtual_environment(&self, venv_hash: &str) -> Result<(), PythonError>
    
    /// Validate virtual environment integrity
    pub async fn validate_environment(
        &self, 
        venv_path: &Path, 
        expected: &ResolvedDependencies
    ) -> Result<bool, PythonError>
    
    /// Calculate deterministic content hash
    pub fn calculate_content_hash(&self, resolved: &ResolvedDependencies) -> Result<String, PythonError>
    
    /// Cleanup orphaned virtual environments
    pub async fn cleanup_orphaned_venvs(
        &self, 
        active_packages: &[String]
    ) -> Result<PythonCleanupResult, PythonError>
}
```

#### Virtual Environment Types

```rust
#[derive(Debug, Clone)]
pub struct VirtualEnvironment {
    pub hash: String,
    pub path: PathBuf,
    pub python_version: String,
    pub packages: BTreeMap<String, String>,
    pub created_at: DateTime<Utc>,
    pub last_used: DateTime<Utc>,
    pub metadata: VenvMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VenvMetadata {
    pub hpm_version: String,
    pub content_hash: String,
    pub python_version: String,
    pub resolved_packages: BTreeMap<String, ResolvedPackage>,
    pub creation_time: DateTime<Utc>,
    pub source_manifests: Vec<String>,
}

#[derive(Debug)]
pub struct PythonCleanupResult {
    removed_venvs: Vec<String>,
    space_freed: u64,
    cleanup_time: Duration,
}

impl PythonCleanupResult {
    pub fn items_cleaned(&self) -> usize
    pub fn items_that_would_be_cleaned(&self) -> usize
    pub fn format_space_freed(&self) -> String
    pub fn format_space_that_would_be_freed(&self) -> String
}
```

### Dependency Collection and Resolution

#### Core Functions

```rust
/// Initialize Python dependency management system
pub async fn initialize() -> Result<(), PythonError>

/// Collect Python dependencies from package manifests
pub async fn collect_python_dependencies(
    packages: &[PackageManifest]
) -> Result<PythonDependencies, PythonError>

/// Resolve dependencies using UV with conflict detection
pub async fn resolve_dependencies(
    collected: &PythonDependencies
) -> Result<ResolvedDependencies, PythonError>

/// Map Houdini version to compatible Python version
pub fn houdini_to_python_version(houdini_version: &str) -> String

/// Get HPM Python cache directory
pub fn get_python_cache_dir() -> PathBuf

/// Get HPM Python configuration directory  
pub fn get_python_config_dir() -> PathBuf

/// Get HPM virtual environments directory
pub fn get_venvs_dir() -> PathBuf
```

#### Python Dependency Types

```rust
#[derive(Debug, Default)]
pub struct PythonDependencies {
    /// Python package requirements mapped by name
    pub requirements: BTreeMap<String, PythonDependencySpec>,
    
    /// Target Houdini version (affects Python version selection)
    pub houdini_version: Option<String>,
    
    /// Detected dependency conflicts
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

#[derive(Debug, Clone)]
pub struct ResolvedDependencies {
    /// Target Python version
    pub python_version: String,
    
    /// Resolved package versions
    pub packages: BTreeMap<String, ResolvedPackage>,
    
    /// When resolution was performed
    pub resolution_time: DateTime<Utc>,
    
    /// Hash for content-addressable storage
    pub content_hash: String,
}

#[derive(Debug, Clone)]
pub struct ResolvedPackage {
    pub name: String,
    pub version: String,
    pub extras: Option<Vec<String>>,
    pub source: PackageSource,
    pub dependencies: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct DependencyConflict {
    pub package_name: String,
    pub conflicting_requirements: Vec<ConflictingRequirement>,
    pub resolution_suggestions: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ConflictingRequirement {
    pub requirement: String,
    pub source_package: String,
    pub source_manifest: PathBuf,
}
```

### Houdini Integration

#### Integration Functions

```rust
/// Generate Houdini package.json with Python environment integration
pub fn generate_houdini_manifest(
    package_name: &str,
    package_path: &Path,
    python_venv: Option<&Path>
) -> Result<HoudiniManifest, PythonError>

/// Update existing Houdini manifest with Python paths
pub fn update_houdini_manifest_python_paths(
    manifest: &mut HoudiniManifest,
    python_venv: &Path
) -> Result<(), PythonError>

/// Extract Python paths from virtual environment
pub fn extract_python_paths(venv_path: &Path) -> Result<PythonPaths, PythonError>
```

#### Houdini Integration Types

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HoudiniManifest {
    /// Package root path
    pub path: String,
    
    /// Whether to load package only once
    pub load_package_once: Option<bool>,
    
    /// Environment variables for Houdini
    pub env: Vec<EnvVar>,
    
    /// HPM management metadata
    pub hpm_managed: Option<bool>,
    pub hpm_package: Option<String>,
    pub hpm_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvVar {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone)]
pub struct PythonPaths {
    pub site_packages: PathBuf,
    pub python_executable: PathBuf,
    pub virtual_env_path: PathBuf,
}
```

### UV Integration

#### UV Management

```rust
/// UV binary management and execution
pub mod bundled {
    /// Ensure UV binary is available and ready
    pub async fn ensure_uv_binary() -> Result<PathBuf, PythonError>
    
    /// Execute UV command with HPM-specific environment
    pub async fn execute_uv_command(
        args: &[String],
        working_dir: Option<&Path>
    ) -> Result<UvResult, PythonError>
    
    /// Get UV binary path
    pub fn get_uv_binary_path() -> PathBuf
    
    /// Check if UV binary exists and is executable
    pub fn is_uv_available() -> bool
}

#[derive(Debug)]
pub struct UvResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub execution_time: Duration,
}
```

### Cleanup System

#### Python Cleanup

```rust
/// Python cleanup analysis and operations
pub mod cleanup {
    /// Analyze orphaned Python virtual environments
    pub struct PythonCleanupAnalyzer {
        // Private fields
    }
    
    impl PythonCleanupAnalyzer {
        pub fn new() -> Self
        
        /// Find virtual environments not used by any active packages
        pub async fn analyze_orphaned_venvs(
            &self,
            active_packages: &[String]
        ) -> Result<Vec<OrphanedVenv>, PythonError>
        
        /// Clean up orphaned virtual environments
        pub async fn cleanup_orphaned_venvs(
            &self,
            orphaned_venvs: &[OrphanedVenv],
            dry_run: bool
        ) -> Result<PythonCleanupResult, PythonError>
        
        /// Calculate disk space used by virtual environments
        pub async fn calculate_venv_disk_usage(&self) -> Result<VenvDiskUsage, PythonError>
    }
    
    #[derive(Debug, Clone)]
    pub struct OrphanedVenv {
        pub hash: String,
        pub path: PathBuf,
        pub last_used: Option<DateTime<Utc>>,
        pub disk_usage: u64,
    }
    
    #[derive(Debug)]
    pub struct VenvDiskUsage {
        pub total_size: u64,
        pub venv_count: usize,
        pub average_size: u64,
        pub largest_venv: Option<(String, u64)>,
    }
}
```

### Error Types

```rust
#[derive(Debug, thiserror::Error)]
pub enum PythonError {
    #[error("UV binary not available: {reason}")]
    UvNotAvailable { reason: String },
    
    #[error("Dependency resolution failed: {reason}")]
    ResolutionFailed { reason: String },
    
    #[error("Virtual environment creation failed: {path}")]
    VenvCreationFailed { path: PathBuf },
    
    #[error("Virtual environment corrupted: {path}")]
    VenvCorrupted { path: PathBuf },
    
    #[error("Python version not supported: {version}")]
    UnsupportedPythonVersion { version: String },
    
    #[error("Dependency conflict: {details}")]
    DependencyConflict { details: String },
    
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    
    #[error("UV execution failed: {exit_code}")]
    UvExecutionFailed { exit_code: i32, stderr: String },
}
```

## Registry System (hpm-registry)

The `hpm-registry` crate provides a high-performance QUIC/gRPC-based package registry system.

### Registry Client

#### RegistryClient

High-performance registry client with QUIC transport.

```rust
pub struct RegistryClient {
    // Private fields
}

impl RegistryClient {
    /// Connect to registry server
    pub async fn connect(config: RegistryClientConfig) -> Result<Self, RegistryError>
    
    /// Disconnect from registry server
    pub async fn disconnect(self) -> Result<(), RegistryError>
    
    /// Set authentication token for operations requiring authorization
    pub fn set_auth_token(&mut self, token: AuthToken)
    
    /// Clear authentication token
    pub fn clear_auth_token(&mut self)
    
    /// Search packages by query string
    pub async fn search_packages(
        &mut self,
        query: &str,
        limit: Option<u32>,
        offset: Option<u32>
    ) -> Result<SearchResults, RegistryError>
    
    /// Get package metadata
    pub async fn get_package_metadata(
        &mut self,
        name: &str,
        version: Option<&str>
    ) -> Result<PackageMetadata, RegistryError>
    
    /// Download package by name and version
    pub async fn download_package(
        &mut self,
        name: &str,
        version: &str
    ) -> Result<DownloadResult, RegistryError>
    
    /// Publish package to registry (requires authentication)
    pub async fn publish_package(
        &mut self,
        name: &str,
        version: &str,
        package_data: Vec<u8>,
        metadata: PublishMetadata
    ) -> Result<PublishResult, RegistryError>
    
    /// List versions for a package
    pub async fn list_versions(&mut self, name: &str) -> Result<Vec<String>, RegistryError>
    
    /// Verify package integrity using checksum
    pub fn verify_package_integrity(&self, data: &[u8], expected_checksum: &str) -> Result<bool, RegistryError>
    
    /// Get client connection statistics
    pub fn get_connection_stats(&self) -> ConnectionStats
}
```

#### Client Configuration and Types

```rust
#[derive(Debug, Clone)]
pub struct RegistryClientConfig {
    /// Registry server address (e.g., "registry.hpm.dev:8443")
    pub server_address: String,
    
    /// Connection timeout
    pub timeout: Duration,
    
    /// Maximum number of retry attempts
    pub max_retries: u32,
    
    /// Enable response compression
    pub compression: bool,
    
    /// Client certificate for mTLS (optional)
    pub client_cert: Option<ClientCertificate>,
    
    /// Custom CA certificates (optional)
    pub ca_certs: Option<Vec<Certificate>>,
}

impl Default for RegistryClientConfig {
    fn default() -> Self
}

#[derive(Debug)]
pub struct SearchResults {
    pub packages: Vec<PackageSearchResult>,
    pub total_count: u32,
    pub limit: u32,
    pub offset: u32,
}

#[derive(Debug, Clone)]
pub struct PackageSearchResult {
    pub name: String,
    pub latest_version: String,
    pub description: Option<String>,
    pub keywords: Vec<String>,
    pub download_count: u64,
    pub last_updated: DateTime<Utc>,
}

#[derive(Debug)]
pub struct DownloadResult {
    pub package_data: Vec<u8>,
    pub checksum: String,
    pub metadata: PackageMetadata,
    pub download_time: Duration,
}

#[derive(Debug)]
pub struct PublishResult {
    pub package_id: String,
    pub checksum: String,
    pub publish_time: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct PublishMetadata {
    pub description: Option<String>,
    pub keywords: Vec<String>,
    pub license: Option<String>,
    pub homepage: Option<String>,
    pub repository: Option<String>,
}

#[derive(Debug)]
pub struct ConnectionStats {
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub requests_sent: u32,
    pub connection_time: Duration,
    pub last_activity: DateTime<Utc>,
}
```

### Registry Server

#### RegistryServer

Scalable registry server with pluggable storage backends.

```rust
pub struct RegistryServer {
    // Private fields
}

impl RegistryServer {
    /// Create registry server with storage backend
    pub fn new(bind_address: SocketAddr, storage: Box<dyn Storage>) -> Self
    
    /// Configure server with custom settings
    pub fn with_config(self, config: ServerConfig) -> Self
    
    /// Add authentication provider
    pub fn with_auth_provider(self, provider: Box<dyn AuthProvider>) -> Self
    
    /// Start serving requests (blocks until shutdown)
    pub async fn serve(self) -> Result<(), RegistryError>
    
    /// Get server statistics
    pub fn get_server_stats(&self) -> ServerStats
}
```

#### Server Configuration

```rust
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// Maximum number of concurrent connections
    pub max_connections: u32,
    
    /// Request timeout
    pub request_timeout: Duration,
    
    /// Enable response compression
    pub compression: bool,
    
    /// Rate limiting (requests per minute per client)
    pub rate_limit: Option<u32>,
    
    /// Maximum package size in bytes
    pub max_package_size: u64,
    
    /// TLS configuration
    pub tls_config: TlsConfig,
    
    /// Logging configuration
    pub logging: LoggingConfig,
}

impl Default for ServerConfig {
    fn default() -> Self
}

#[derive(Debug, Clone)]
pub struct TlsConfig {
    pub cert_path: PathBuf,
    pub key_path: PathBuf,
    pub require_client_cert: bool,
}

#[derive(Debug, Clone)]
pub struct LoggingConfig {
    pub level: String,
    pub format: LogFormat,
    pub access_log: bool,
}

#[derive(Debug, Clone)]
pub enum LogFormat {
    Json,
    Text,
}
```

### Storage Backend Abstraction

#### Storage Trait

Pluggable storage backend for the registry server.

```rust
#[async_trait]
pub trait Storage: Send + Sync {
    /// Store package data and return package ID
    async fn store_package(&self, package: &PackageData) -> Result<String, StorageError>
    
    /// Retrieve package by ID
    async fn get_package(&self, package_id: &str) -> Result<Option<PackageData>, StorageError>
    
    /// Get package by name and version
    async fn get_package_by_name_version(
        &self, 
        name: &str, 
        version: &str
    ) -> Result<Option<PackageData>, StorageError>
    
    /// Search packages by query
    async fn search_packages(&self, query: &SearchQuery) -> Result<Vec<PackageMetadata>, StorageError>
    
    /// List all versions for a package
    async fn list_versions(&self, name: &str) -> Result<Vec<String>, StorageError>
    
    /// Delete package (admin operation)
    async fn delete_package(&self, package_id: &str) -> Result<(), StorageError>
    
    /// Check if package exists
    async fn package_exists(&self, name: &str, version: &str) -> Result<bool, StorageError>
    
    /// Get storage statistics
    async fn get_statistics(&self) -> Result<StorageStatistics, StorageError>
}
```

#### Storage Implementations

```rust
/// In-memory storage for development and testing
pub struct MemoryStorage {
    // Private fields
}

impl MemoryStorage {
    pub fn new() -> Self
    pub fn with_capacity(capacity: usize) -> Self
}

/// PostgreSQL storage for production deployments
pub struct PostgreSqlStorage {
    // Private fields  
}

impl PostgreSqlStorage {
    pub async fn connect(database_url: &str) -> Result<Self, StorageError>
    pub async fn connect_with_config(config: PostgreSqlConfig) -> Result<Self, StorageError>
    pub async fn migrate(&self) -> Result<(), StorageError>
}

/// S3-compatible storage for cloud deployments
pub struct S3Storage {
    // Private fields
}

impl S3Storage {
    pub async fn new(config: S3Config) -> Result<Self, StorageError>
    pub async fn with_credentials(config: S3Config, credentials: Credentials) -> Result<Self, StorageError>
}

#[derive(Debug, Clone)]
pub struct PostgreSqlConfig {
    pub database_url: String,
    pub pool_size: u32,
    pub connection_timeout: Duration,
    pub idle_timeout: Duration,
}

#[derive(Debug, Clone)]
pub struct S3Config {
    pub bucket: String,
    pub region: String,
    pub endpoint: Option<String>, // For S3-compatible services
    pub prefix: Option<String>,   // Key prefix for organization
}
```

### Authentication System

#### Authentication Types

```rust
#[derive(Debug, Clone)]
pub struct AuthToken {
    pub token: String,
    pub scopes: Vec<TokenScope>,
    pub expires_at: Option<DateTime<Utc>>,
    pub user_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TokenScope {
    /// Read packages and metadata
    Read,
    /// Publish packages
    Write,
    /// Administrative operations
    Admin,
}

impl AuthToken {
    pub fn new(token: String, scopes: Vec<TokenScope>) -> Self
    pub fn with_expiry(self, expires_at: DateTime<Utc>) -> Self
    pub fn with_user_id(self, user_id: String) -> Self
    pub fn is_expired(&self) -> bool
    pub fn has_scope(&self, scope: &TokenScope) -> bool
}
```

#### AuthProvider Trait

```rust
#[async_trait]
pub trait AuthProvider: Send + Sync {
    /// Validate authentication token
    async fn validate_token(&self, token: &str) -> Result<AuthToken, AuthError>
    
    /// Create new token (admin operation)
    async fn create_token(&self, user_id: &str, scopes: Vec<TokenScope>) -> Result<AuthToken, AuthError>
    
    /// Revoke token (admin operation)
    async fn revoke_token(&self, token: &str) -> Result<(), AuthError>
    
    /// List tokens for user (admin operation)
    async fn list_user_tokens(&self, user_id: &str) -> Result<Vec<AuthToken>, AuthError>
}
```

### Protocol Buffer Definitions

#### Generated Types

```rust
/// Package metadata for search and listing
#[derive(Debug, Clone, prost::Message)]
pub struct PackageInfo {
    #[prost(string, tag = "1")]
    pub name: String,
    #[prost(string, tag = "2")]
    pub version: String,
    #[prost(string, optional, tag = "3")]
    pub description: Option<String>,
    #[prost(string, repeated, tag = "4")]
    pub keywords: Vec<String>,
    #[prost(uint64, tag = "5")]
    pub size: u64,
    #[prost(string, tag = "6")]
    pub checksum: String,
}

/// Search request parameters
#[derive(Debug, Clone, prost::Message)]
pub struct SearchRequest {
    #[prost(string, tag = "1")]
    pub query: String,
    #[prost(uint32, tag = "2")]
    pub limit: u32,
    #[prost(uint32, tag = "3")]
    pub offset: u32,
}

/// Search response with results
#[derive(Debug, Clone, prost::Message)]
pub struct SearchResponse {
    #[prost(message, repeated, tag = "1")]
    pub packages: Vec<PackageInfo>,
    #[prost(uint32, tag = "2")]
    pub total_count: u32,
}

/// Package download request
#[derive(Debug, Clone, prost::Message)]
pub struct DownloadRequest {
    #[prost(string, tag = "1")]
    pub name: String,
    #[prost(string, tag = "2")]
    pub version: String,
}

/// Package download response (streamed)
#[derive(Debug, Clone, prost::Message)]
pub struct DownloadResponse {
    #[prost(bytes = "vec", tag = "1")]
    pub data: Vec<u8>,
    #[prost(string, tag = "2")]
    pub checksum: String,
    #[prost(message, optional, tag = "3")]
    pub metadata: Option<PackageInfo>,
}

/// Package publish request
#[derive(Debug, Clone, prost::Message)]
pub struct PublishRequest {
    #[prost(string, tag = "1")]
    pub name: String,
    #[prost(string, tag = "2")]
    pub version: String,
    #[prost(bytes = "vec", tag = "3")]
    pub data: Vec<u8>,
    #[prost(string, optional, tag = "4")]
    pub description: Option<String>,
    #[prost(string, repeated, tag = "5")]
    pub keywords: Vec<String>,
}

/// Package publish response
#[derive(Debug, Clone, prost::Message)]
pub struct PublishResponse {
    #[prost(string, tag = "1")]
    pub package_id: String,
    #[prost(string, tag = "2")]  
    pub checksum: String,
}
```

### Error Types

```rust
#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    #[error("Connection failed: {reason}")]
    ConnectionFailed { reason: String },
    
    #[error("Authentication failed")]
    AuthenticationFailed,
    
    #[error("Package not found: {name}@{version}")]
    PackageNotFound { name: String, version: String },
    
    #[error("Package already exists: {name}@{version}")]
    PackageAlreadyExists { name: String, version: String },
    
    #[error("Invalid package data: {reason}")]
    InvalidPackageData { reason: String },
    
    #[error("Network error: {0}")]
    Network(#[from] Box<dyn std::error::Error + Send + Sync>),
    
    #[error("Storage error: {0}")]
    Storage(#[from] StorageError),
    
    #[error("Serialization error: {0}")]
    Serialization(#[from] prost::DecodeError),
    
    #[error("Rate limit exceeded: {retry_after}s")]
    RateLimitExceeded { retry_after: u32 },
    
    #[error("Server error: {status_code}")]
    ServerError { status_code: u16 },
}

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("Package not found: {id}")]
    NotFound { id: String },
    
    #[error("Storage backend error: {details}")]
    BackendError { details: String },
    
    #[error("Serialization failed: {0}")]
    Serialization(#[from] serde_json::Error),
    
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),
    
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}
```

## Package Processing (hpm-package)

The `hpm-package` crate handles package manifest processing, template generation, and Houdini integration.

### Package Manifest

#### PackageManifest

Core package manifest structure representing `hpm.toml` files.

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageManifest {
    pub package: PackageMetadata,
    pub houdini: Option<HoudiniCompatibility>,
    pub dependencies: BTreeMap<String, DependencySpec>,
    pub dev_dependencies: Option<BTreeMap<String, DependencySpec>>,
    pub python_dependencies: Option<BTreeMap<String, PythonDependencySpec>>,
    pub scripts: Option<BTreeMap<String, String>>,
}

impl PackageManifest {
    /// Parse manifest from TOML string
    pub fn from_toml_str(content: &str) -> Result<Self, PackageError>
    
    /// Load manifest from file
    pub fn from_file(path: &Path) -> Result<Self, PackageError>
    
    /// Save manifest to file with formatting
    pub fn to_file(&self, path: &Path) -> Result<(), PackageError>
    
    /// Convert to TOML string with formatting
    pub fn to_toml_string(&self) -> Result<String, PackageError>
    
    /// Validate manifest structure and constraints
    pub fn validate(&self) -> Result<(), ValidationError>
    
    /// Get all dependencies (regular + dev)
    pub fn all_dependencies(&self) -> BTreeMap<String, &DependencySpec>
    
    /// Add dependency to manifest
    pub fn add_dependency(&mut self, name: String, spec: DependencySpec)
    
    /// Remove dependency from manifest
    pub fn remove_dependency(&mut self, name: &str) -> Option<DependencySpec>
    
    /// Check if dependency exists
    pub fn has_dependency(&self, name: &str) -> bool
}
```

#### Package Metadata

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageMetadata {
    /// Package name (must be unique in registry)
    pub name: String,
    
    /// Semantic version
    pub version: String,
    
    /// Brief package description
    pub description: Option<String>,
    
    /// Package authors
    pub authors: Option<Vec<String>>,
    
    /// License identifier (SPDX format recommended)
    pub license: Option<String>,
    
    /// README file path
    pub readme: Option<String>,
    
    /// Homepage URL
    pub homepage: Option<String>,
    
    /// Repository URL
    pub repository: Option<String>,
    
    /// Keywords for package discovery
    pub keywords: Option<Vec<String>>,
    
    /// Package categories
    pub categories: Option<Vec<String>>,
}

impl PackageMetadata {
    pub fn new(name: String, version: String) -> Self
    pub fn with_description(mut self, description: String) -> Self
    pub fn with_authors(mut self, authors: Vec<String>) -> Self
    pub fn with_license(mut self, license: String) -> Self
    
    /// Validate package name format
    pub fn validate_name(&self) -> Result<(), ValidationError>
    
    /// Validate semantic version
    pub fn validate_version(&self) -> Result<(), ValidationError>
}
```

#### Houdini Compatibility

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HoudiniCompatibility {
    /// Minimum Houdini version
    pub min_version: Option<String>,
    
    /// Maximum Houdini version
    pub max_version: Option<String>,
}

impl HoudiniCompatibility {
    pub fn new() -> Self
    pub fn with_min_version(mut self, version: String) -> Self
    pub fn with_max_version(mut self, version: String) -> Self
    
    /// Check if Houdini version is compatible
    pub fn is_compatible(&self, houdini_version: &str) -> bool
    
    /// Get compatible Python version for Houdini version
    pub fn get_python_version(&self) -> Option<String>
}
```

### Package Templates

#### Template System

```rust
/// Package template types
#[derive(Debug, Clone)]
pub enum PackageTemplate {
    /// Full package structure with all directories
    Standard {
        include_python: bool,
        include_tests: bool,
        vcs: Option<VcsType>,
    },
    
    /// Minimal structure with only hmp.toml
    Bare {
        vcs: Option<VcsType>,
    },
}

#[derive(Debug, Clone)]
pub enum VcsType {
    Git,
    None,
}

/// Template generation options
#[derive(Debug, Clone)]
pub struct TemplateOptions {
    pub name: String,
    pub description: Option<String>,
    pub author: Option<String>,
    pub license: String,
    pub version: String,
    pub houdini_min: Option<String>,
    pub houdini_max: Option<String>,
    pub template: PackageTemplate,
    pub base_dir: Option<PathBuf>,
}

impl Default for TemplateOptions {
    fn default() -> Self
}
```

#### Template Generation

```rust
/// Generate package from template
pub async fn generate_package_template(options: TemplateOptions) -> Result<GeneratedPackage, PackageError>

/// Generate hpm.toml manifest file
pub fn generate_manifest(options: &TemplateOptions) -> Result<PackageManifest, PackageError>

/// Generate README.md file
pub fn generate_readme(options: &TemplateOptions) -> Result<String, PackageError>

/// Generate .gitignore for Houdini packages
pub fn generate_gitignore() -> String

/// Initialize version control if requested
pub async fn initialize_vcs(path: &Path, vcs_type: VcsType) -> Result<(), PackageError>

#[derive(Debug)]
pub struct GeneratedPackage {
    pub path: PathBuf,
    pub manifest: PackageManifest,
    pub files_created: Vec<PathBuf>,
    pub directories_created: Vec<PathBuf>,
}
```

### Houdini Integration

#### Houdini Package Generation

```rust
/// Generate Houdini package.json from HPM manifest
pub fn generate_houdini_package_json(
    manifest: &PackageManifest,
    package_path: &Path,
    python_venv: Option<&Path>
) -> Result<HoudiniPackageJson, PackageError>

/// Update existing package.json with HPM metadata
pub fn update_houdini_package_json(
    package_json_path: &Path,
    hpm_metadata: HpmIntegrationMetadata
) -> Result<(), PackageError>

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HoudiniPackageJson {
    /// Package root path (typically "$HPM_PACKAGE_ROOT")
    pub path: String,
    
    /// Load package only once
    #[serde(skip_serializing_if = "Option::is_none")]
    pub load_package_once: Option<bool>,
    
    /// Environment variables
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub env: Vec<EnvironmentVariable>,
    
    /// HPM metadata
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hpm_managed: Option<bool>,
    
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hpm_package: Option<String>,
    
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hpm_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvironmentVariable {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone)]
pub struct HpmIntegrationMetadata {
    pub hmp_managed: bool,
    pub hmp_package: String,
    pub hpm_version: String,
    pub python_environment: Option<PathBuf>,
}
```

### Validation System

#### Manifest Validation

```rust
/// Validate package manifest comprehensively
pub fn validate_package_manifest(manifest: &PackageManifest) -> Result<ValidationResult, PackageError>

/// Validate dependency specifications
pub fn validate_dependencies(deps: &BTreeMap<String, DependencySpec>) -> Result<(), ValidationError>

/// Validate Python dependencies
pub fn validate_python_dependencies(
    deps: &BTreeMap<String, PythonDependencySpec>
) -> Result<(), ValidationError>

/// Validate Houdini version compatibility
pub fn validate_houdini_compatibility(compat: &HoudiniCompatibility) -> Result<(), ValidationError>

#[derive(Debug)]
pub struct ValidationResult {
    pub is_valid: bool,
    pub errors: Vec<ValidationError>,
    pub warnings: Vec<ValidationWarning>,
}

#[derive(Debug, Clone)]
pub struct ValidationWarning {
    pub field: String,
    pub message: String,
    pub suggestion: Option<String>,
}
```

### Error Types

```rust
#[derive(Debug, thiserror::Error)]
pub enum PackageError {
    #[error("Invalid package manifest: {reason}")]
    InvalidManifest { reason: String },
    
    #[error("Validation failed: {0}")]
    Validation(#[from] ValidationError),
    
    #[error("TOML parsing error: {0}")]
    TomlParse(#[from] toml::de::Error),
    
    #[error("TOML serialization error: {0}")]
    TomlSerialize(#[from] toml::ser::Error),
    
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    
    #[error("Template generation failed: {reason}")]
    TemplateGeneration { reason: String },
    
    #[error("Version control initialization failed: {reason}")]
    VcsInitialization { reason: String },
}

#[derive(Debug, thiserror::Error)]
pub enum ValidationError {
    #[error("Invalid package name: {name}")]
    InvalidPackageName { name: String },
    
    #[error("Invalid version: {version}")]
    InvalidVersion { version: String },
    
    #[error("Invalid dependency specification: {spec}")]
    InvalidDependencySpec { spec: String },
    
    #[error("Invalid Houdini version: {version}")]
    InvalidHoudiniVersion { version: String },
    
    #[error("Required field missing: {field}")]
    MissingRequiredField { field: String },
    
    #[error("Field validation failed: {field} - {reason}")]
    FieldValidation { field: String, reason: String },
}
```

## Configuration Management (hpm-config)

The `hpm-config` crate provides hierarchical configuration management with project discovery settings.

### Configuration System

#### Config

Main configuration structure with hierarchical loading.

```rust
#[derive(Debug, Clone)]
pub struct Config {
    pub registry: RegistryConfig,
    pub storage: StorageConfig,
    pub projects: ProjectsConfig,
    pub python: PythonConfig,
    pub ui: UiConfig,
}

impl Config {
    /// Load configuration with defaults
    pub fn load() -> Result<Self, ConfigError>
    
    /// Load from specific file
    pub fn load_from_file(path: &Path) -> Result<Self, ConfigError>
    
    /// Load with custom search paths
    pub fn load_with_paths(search_paths: &[PathBuf]) -> Result<Self, ConfigError>
    
    /// Save configuration to file
    pub fn save_to_file(&self, path: &Path) -> Result<(), ConfigError>
    
    /// Merge with another configuration (other takes precedence)
    pub fn merge(self, other: Config) -> Self
    
    /// Apply environment variable overrides
    pub fn apply_env_overrides(self) -> Self
}

impl Default for Config {
    fn default() -> Self
}
```

#### Registry Configuration

```rust
#[derive(Debug, Clone)]
pub struct RegistryConfig {
    /// Default registry URL
    pub default_url: String,
    
    /// Additional registry sources
    pub sources: BTreeMap<String, RegistrySource>,
    
    /// Authentication token
    pub auth_token: Option<String>,
    
    /// Connection timeout
    pub timeout: Duration,
    
    /// Maximum retry attempts
    pub max_retries: u32,
    
    /// Enable compression
    pub compression: bool,
}

#[derive(Debug, Clone)]
pub struct RegistrySource {
    pub url: String,
    pub auth_token: Option<String>,
    pub priority: u32,
}

impl Default for RegistryConfig {
    fn default() -> Self
}
```

#### Storage Configuration

```rust
#[derive(Debug, Clone)]
pub struct StorageConfig {
    /// Root storage directory (default: ~/.hpm)
    pub root_path: PathBuf,
    
    /// Package storage subdirectory
    pub packages_dir: String,
    
    /// Cache subdirectory
    pub cache_dir: String,
    
    /// Temporary files subdirectory
    pub temp_dir: String,
    
    /// Enable storage compression
    pub compression: bool,
    
    /// Maximum parallel operations
    pub max_parallel_operations: usize,
    
    /// Cache retention period
    pub cache_retention: Duration,
}

impl Default for StorageConfig {
    fn default() -> Self
}
```

#### Projects Configuration

```rust
#[derive(Debug, Clone)]
pub struct ProjectsConfig {
    /// Explicit project paths (always monitored)
    pub explicit_paths: Vec<PathBuf>,
    
    /// Root directories to search recursively
    pub search_roots: Vec<PathBuf>,
    
    /// Maximum search depth
    pub max_search_depth: usize,
    
    /// Directory patterns to ignore
    pub ignore_patterns: Vec<String>,
    
    /// Enable project caching
    pub enable_caching: bool,
    
    /// Cache validity duration
    pub cache_duration: Duration,
}

impl ProjectsConfig {
    pub fn new() -> Self
    pub fn add_explicit_path(mut self, path: PathBuf) -> Self
    pub fn add_search_root(mut self, root: PathBuf) -> Self
    pub fn with_max_depth(mut self, depth: usize) -> Self
    pub fn add_ignore_pattern(mut self, pattern: String) -> Self
}

impl Default for ProjectsConfig {
    fn default() -> Self
}
```

#### Python Configuration

```rust
#[derive(Debug, Clone)]
pub struct PythonConfig {
    /// Python virtual environments directory
    pub venvs_dir: PathBuf,
    
    /// UV cache directory  
    pub uv_cache_dir: PathBuf,
    
    /// UV configuration directory
    pub uv_config_dir: PathBuf,
    
    /// Maximum number of virtual environments to keep
    pub max_venvs: Option<usize>,
    
    /// Virtual environment cleanup threshold (days)
    pub cleanup_threshold_days: u32,
    
    /// Default Python version for new environments
    pub default_python_version: Option<String>,
}

impl Default for PythonConfig {
    fn default() -> Self
}
```

#### UI Configuration

```rust
#[derive(Debug, Clone)]
pub struct UiConfig {
    /// Color output preference
    pub color: ColorChoice,
    
    /// Default output format
    pub output_format: OutputFormat,
    
    /// Default verbosity level
    pub verbosity: Verbosity,
    
    /// Show progress bars
    pub progress_bars: bool,
    
    /// Confirm destructive operations
    pub confirm_destructive: bool,
    
    /// Emoji usage in output
    pub use_emojis: bool,
}

#[derive(Debug, Clone)]
pub enum ColorChoice {
    Auto,
    Always,
    Never,
}

#[derive(Debug, Clone)]
pub enum OutputFormat {
    Human,
    Json,
    JsonLines,
    JsonCompact,
}

#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub enum Verbosity {
    Silent,
    Quiet,
    Normal,
    Verbose,
}

impl Default for UiConfig {
    fn default() -> Self
}
```

### Configuration Loading

#### Configuration Sources

Configuration is loaded hierarchically from multiple sources:

1. **Built-in defaults** - Sensible defaults for all settings
2. **Global config file** - `~/.hpm/config.toml`
3. **Project config file** - `.hpm/config.toml` in project root
4. **Environment variables** - `HPM_*` prefixed variables
5. **Command-line arguments** - Highest priority overrides

```rust
/// Configuration loading utilities
pub mod loader {
    /// Load configuration from all sources
    pub fn load_full_config() -> Result<Config, ConfigError>
    
    /// Load configuration from specific file
    pub fn load_from_toml_file(path: &Path) -> Result<Config, ConfigError>
    
    /// Load configuration from TOML string
    pub fn load_from_toml_str(content: &str) -> Result<Config, ConfigError>
    
    /// Apply environment variable overrides
    pub fn apply_env_overrides(mut config: Config) -> Config
    
    /// Find configuration files in search paths
    pub fn find_config_files(search_paths: &[PathBuf]) -> Vec<PathBuf>
}
```

#### Environment Variables

```rust
/// Environment variable configuration
pub mod env {
    /// Apply all HPM_* environment variables to configuration
    pub fn apply_env_vars(config: &mut Config)
    
    /// Get environment variable with HPM prefix
    pub fn get_hpm_env(key: &str) -> Option<String>
    
    /// Set environment variable with HPM prefix
    pub fn set_hpm_env(key: &str, value: &str)
    
    /// Environment variable mappings
    pub const REGISTRY_URL: &str = "HPM_REGISTRY_URL";
    pub const STORAGE_PATH: &str = "HPM_STORAGE_PATH";
    pub const AUTH_TOKEN: &str = "HPM_AUTH_TOKEN";
    pub const LOG_LEVEL: &str = "HPM_LOG_LEVEL";
    pub const MAX_PARALLEL: &str = "HPM_MAX_PARALLEL";
}
```

### Error Types

```rust
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("Configuration file not found: {path}")]
    FileNotFound { path: PathBuf },
    
    #[error("Permission denied reading config: {path}")]
    PermissionDenied { path: PathBuf },
    
    #[error("TOML parsing error: {0}")]
    TomlParse(#[from] toml::de::Error),
    
    #[error("TOML serialization error: {0}")]
    TomlSerialize(#[from] toml::ser::Error),
    
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    
    #[error("Invalid configuration value: {field} = {value}")]
    InvalidValue { field: String, value: String },
    
    #[error("Required configuration missing: {field}")]
    MissingRequired { field: String },
    
    #[error("Environment variable error: {var}")]
    EnvVar { var: String },
}
```

## Dependency Resolution (hpm-resolver)

The `hpm-resolver` crate provides advanced dependency resolution using a PubGrub-inspired algorithm.

### Dependency Solver

#### Resolver

Main dependency resolution engine with conflict detection and backtracking.

```rust
pub struct Resolver {
    // Private fields
}

impl Resolver {
    /// Create resolver with default configuration
    pub fn new() -> Self
    
    /// Create resolver with custom configuration
    pub fn with_config(config: ResolverConfig) -> Self
    
    /// Resolve dependencies for a set of requirements
    pub async fn resolve(
        &mut self, 
        requirements: &[DependencyRequirement]
    ) -> Result<ResolutionResult, ResolutionError>
    
    /// Resolve dependencies incrementally (for interactive use)
    pub async fn resolve_incremental(
        &mut self,
        current_solution: Option<&ResolutionResult>,
        new_requirements: &[DependencyRequirement]
    ) -> Result<ResolutionResult, ResolutionError>
    
    /// Check if requirements can be satisfied
    pub async fn can_resolve(
        &mut self, 
        requirements: &[DependencyRequirement]
    ) -> Result<bool, ResolutionError>
    
    /// Find conflicts between requirements
    pub fn find_conflicts(
        &self, 
        requirements: &[DependencyRequirement]
    ) -> Vec<DependencyConflict>
    
    /// Get resolution statistics
    pub fn get_stats(&self) -> ResolutionStats
}
```

#### Resolution Configuration

```rust
#[derive(Debug, Clone)]
pub struct ResolverConfig {
    /// Maximum number of backtracking attempts
    pub max_iterations: u32,
    
    /// Resolution timeout
    pub timeout: Duration,
    
    /// Enable prerelease versions
    pub allow_prereleases: bool,
    
    /// Conflict resolution strategy
    pub conflict_strategy: ConflictStrategy,
    
    /// Package source priority
    pub source_priority: Vec<SourceType>,
    
    /// Enable resolution caching
    pub enable_caching: bool,
}

#[derive(Debug, Clone)]
pub enum ConflictStrategy {
    /// Fail immediately on conflicts
    Strict,
    
    /// Attempt automatic conflict resolution
    Resolve,
    
    /// Use highest version that satisfies constraints
    HighestCompatible,
}

#[derive(Debug, Clone)]
pub enum SourceType {
    Registry,
    Git,
    Local,
}

impl Default for ResolverConfig {
    fn default() -> Self
}
```

### Version Handling

#### Version Types

```rust
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    // Private fields
}

impl Version {
    /// Parse semantic version string
    pub fn parse(version: &str) -> Result<Self, VersionError>
    
    /// Create version from components
    pub fn new(major: u32, minor: u32, patch: u32) -> Self
    
    /// Create prerelease version
    pub fn with_prerelease(self, prerelease: String) -> Self
    
    /// Create version with build metadata
    pub fn with_build(self, build: String) -> Self
    
    /// Check if version is prerelease
    pub fn is_prerelease(&self) -> bool
    
    /// Get major version
    pub fn major(&self) -> u32
    
    /// Get minor version
    pub fn minor(&self) -> u32
    
    /// Get patch version
    pub fn patch(&self) -> u32
    
    /// Get prerelease identifier
    pub fn prerelease(&self) -> Option<&str>
    
    /// Get build metadata
    pub fn build(&self) -> Option<&str>
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result
}

impl std::str::FromStr for Version {
    type Err = VersionError;
    fn from_str(s: &str) -> Result<Self, Self::Err>
}
```

#### Version Requirements

```rust
#[derive(Debug, Clone)]
pub struct VersionRequirement {
    // Private fields
}

impl VersionRequirement {
    /// Parse version requirement string (e.g., "^1.0.0", "~2.1", ">=1.5.0")
    pub fn parse(requirement: &str) -> Result<Self, VersionError>
    
    /// Create exact version requirement
    pub fn exact(version: Version) -> Self
    
    /// Create caret requirement (^1.0.0)
    pub fn caret(version: Version) -> Self
    
    /// Create tilde requirement (~1.0.0)
    pub fn tilde(version: Version) -> Self
    
    /// Create range requirement (>=1.0.0, <2.0.0)
    pub fn range(min: Option<Version>, max: Option<Version>) -> Self
    
    /// Check if version satisfies requirement
    pub fn satisfies(&self, version: &Version) -> bool
    
    /// Get all versions that satisfy requirement
    pub fn filter_versions(&self, versions: &[Version]) -> Vec<&Version>
    
    /// Find intersection with another requirement
    pub fn intersect(&self, other: &Self) -> Option<Self>
    
    /// Check if requirement is compatible with another
    pub fn is_compatible(&self, other: &Self) -> bool
}

impl std::fmt::Display for VersionRequirement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result
}

impl std::str::FromStr for VersionRequirement {
    type Err = VersionError;
    fn from_str(s: &str) -> Result<Self, Self::Err>
}
```

### Resolution Types

#### Resolution Result

```rust
#[derive(Debug, Clone)]
pub struct ResolutionResult {
    /// Resolved packages with exact versions
    pub packages: BTreeMap<String, ResolvedPackage>,
    
    /// Resolution metadata
    pub metadata: ResolutionMetadata,
}

impl ResolutionResult {
    /// Get package by name
    pub fn get_package(&self, name: &str) -> Option<&ResolvedPackage>
    
    /// Check if package is included
    pub fn contains_package(&self, name: &str) -> bool
    
    /// Get all package names
    pub fn package_names(&self) -> Vec<&String>
    
    /// Convert to lock file format
    pub fn to_lock_file(&self) -> LockFile
    
    /// Validate resolution consistency
    pub fn validate(&self) -> Result<(), ResolutionError>
}

#[derive(Debug, Clone)]
pub struct ResolvedPackage {
    pub name: String,
    pub version: Version,
    pub source: PackageSource,
    pub dependencies: Vec<DependencyRequirement>,
    pub features: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ResolutionMetadata {
    pub resolution_time: Duration,
    pub iterations: u32,
    pub conflicts_resolved: u32,
    pub packages_considered: u32,
    pub resolver_version: String,
}
```

#### Resolution Requirements

```rust
#[derive(Debug, Clone)]
pub struct DependencyRequirement {
    pub name: String,
    pub version_requirement: VersionRequirement,
    pub source: Option<PackageSource>,
    pub optional: bool,
    pub features: Vec<String>,
}

impl DependencyRequirement {
    pub fn new(name: String, version_requirement: VersionRequirement) -> Self
    pub fn with_source(mut self, source: PackageSource) -> Self
    pub fn with_features(mut self, features: Vec<String>) -> Self
    pub fn optional(mut self) -> Self
    
    /// Check if requirement is satisfied by resolved package
    pub fn is_satisfied_by(&self, package: &ResolvedPackage) -> bool
}
```

### Conflict Resolution

#### Conflict Types

```rust
#[derive(Debug, Clone)]
pub struct DependencyConflict {
    pub package_name: String,
    pub conflicting_requirements: Vec<ConflictingRequirement>,
    pub resolution_suggestions: Vec<ResolutionSuggestion>,
}

#[derive(Debug, Clone)]
pub struct ConflictingRequirement {
    pub requirement: VersionRequirement,
    pub source: ConflictSource,
    pub optional: bool,
}

#[derive(Debug, Clone)]
pub enum ConflictSource {
    RootRequirement,
    Dependency { package: String, version: Version },
}

#[derive(Debug, Clone)]
pub struct ResolutionSuggestion {
    pub suggestion_type: SuggestionType,
    pub description: String,
    pub impact: ImpactLevel,
}

#[derive(Debug, Clone)]
pub enum SuggestionType {
    UpdateRequirement { package: String, new_requirement: String },
    MakeOptional { package: String },
    UseAlternative { instead_of: String, use_package: String },
    RemovePackage { package: String },
}

#[derive(Debug, Clone)]
pub enum ImpactLevel {
    Low,
    Medium,
    High,
}
```

### Error Types

```rust
#[derive(Debug, thiserror::Error)]
pub enum ResolutionError {
    #[error("No solution found for requirements")]
    NoSolution { conflicts: Vec<DependencyConflict> },
    
    #[error("Resolution timeout after {duration:?}")]
    Timeout { duration: Duration },
    
    #[error("Maximum iterations exceeded: {max_iterations}")]
    MaxIterationsExceeded { max_iterations: u32 },
    
    #[error("Package not found: {package}")]
    PackageNotFound { package: String },
    
    #[error("Invalid dependency specification: {spec}")]
    InvalidDependency { spec: String },
    
    #[error("Circular dependency detected: {cycle:?}")]
    CircularDependency { cycle: Vec<String> },
}

#[derive(Debug, thiserror::Error)]
pub enum VersionError {
    #[error("Invalid version format: {version}")]
    InvalidFormat { version: String },
    
    #[error("Invalid version requirement: {requirement}")]
    InvalidRequirement { requirement: String },
    
    #[error("Version component out of range: {component}")]
    ComponentOutOfRange { component: String },
}
```

## CLI System (hpm-cli)

The `hpm-cli` crate provides the command-line interface with professional error handling and multiple output formats.

### Command System

#### Command Traits

```rust
/// Trait for CLI command execution
#[async_trait]
pub trait Command {
    /// Execute the command
    async fn execute(&self, context: &CommandContext) -> CliResult<ExitStatus>
    
    /// Get command name
    fn name(&self) -> &str
    
    /// Get command description
    fn description(&self) -> &str
    
    /// Validate command arguments
    fn validate(&self) -> Result<(), ValidationError>
}

/// Command execution context
#[derive(Debug)]
pub struct CommandContext {
    pub config: Config,
    pub console: Console,
    pub output_format: OutputFormat,
    pub working_directory: PathBuf,
}
```

#### Exit Status

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum ExitStatus {
    Success,
    UserError,
    InternalError,
    ExternalCommandError { exit_code: i32 },
}

impl From<ExitStatus> for std::process::ExitCode {
    fn from(status: ExitStatus) -> Self
}

impl From<&CliError> for ExitStatus {
    fn from(error: &CliError) -> Self
}
```

### Console System

#### Console

Terminal output management with styling and formatting.

```rust
pub struct Console {
    // Private fields
}

impl Console {
    /// Create console with default settings
    pub fn new() -> Self
    
    /// Create console with custom settings
    pub fn with_settings(verbosity: Verbosity, color_choice: ColorChoice) -> Self
    
    /// Print success message with styling
    pub fn success<S: AsRef<str>>(&mut self, message: S)
    
    /// Print error message with styling
    pub fn error<S: AsRef<str>>(&mut self, message: S)
    
    /// Print warning message with styling
    pub fn warn<S: AsRef<str>>(&mut self, message: S)
    
    /// Print info message with styling
    pub fn info<S: AsRef<str>>(&mut self, message: S)
    
    /// Print debug message (only if verbose)
    pub fn debug<S: AsRef<str>>(&mut self, message: S)
    
    /// Print message without styling
    pub fn println<S: AsRef<str>>(&mut self, message: S)
    
    /// Print progress indicator
    pub fn progress<S: AsRef<str>>(&mut self, message: S)
    
    /// Ask user for confirmation
    pub fn confirm<S: AsRef<str>>(&mut self, prompt: S) -> bool
    
    /// Set verbosity level
    pub fn set_verbosity(&mut self, verbosity: Verbosity)
    
    /// Set color choice
    pub fn set_color_choice(&mut self, choice: ColorChoice)
}
```

#### Output Formatting

```rust
/// Machine-readable output formatting
pub mod output {
    /// Format command result as JSON
    pub fn format_json<T: Serialize>(result: &T, pretty: bool) -> Result<String, OutputError>
    
    /// Format command result as JSON Lines
    pub fn format_json_lines<T: Serialize>(results: &[T]) -> Result<String, OutputError>
    
    /// Format command result as compact JSON
    pub fn format_json_compact<T: Serialize>(result: &T) -> Result<String, OutputError>
    
    /// Output result in specified format
    pub fn output_result<T: Serialize>(
        result: &T, 
        format: OutputFormat, 
        writer: &mut dyn Write
    ) -> Result<(), OutputError>
}

#[derive(Debug)]
pub struct CommandOutput<T> {
    pub success: bool,
    pub command: String,
    pub data: T,
    pub elapsed_ms: u64,
}

#[derive(Debug)]
pub struct ErrorOutput {
    pub success: bool,
    pub error: String,
    pub error_type: String,
    pub help: Option<String>,
    pub elapsed_ms: u64,
}
```

### Error Handling

#### CLI Error Types

```rust
#[derive(Debug, thiserror::Error)]
pub enum CliError {
    #[error("Configuration error: {source}")]
    Config { 
        #[source] source: anyhow::Error,
        help: Option<String>
    },
    
    #[error("Package error: {source}")]
    Package {
        #[source] source: anyhow::Error,
        help: Option<String>
    },
    
    #[error("Network error: {source}")]
    Network {
        #[source] source: anyhow::Error,
        help: Option<String>
    },
    
    #[error("I/O error: {source}")]
    Io {
        #[source] source: anyhow::Error,
        help: Option<String>
    },
    
    #[error("Internal error: {source}")]
    Internal {
        #[source] source: anyhow::Error,
        help: Option<String>
    },
    
    #[error("External command failed: {source}")]
    External {
        #[source] source: anyhow::Error,
        help: Option<String>
    },
}

impl CliError {
    /// Create configuration error with context
    pub fn config(source: anyhow::Error, help: Option<String>) -> Self
    
    /// Create package error with context
    pub fn package(source: anyhow::Error, help: Option<String>) -> Self
    
    /// Create network error with context
    pub fn network(source: anyhow::Error, help: Option<String>) -> Self
    
    /// Create I/O error with context
    pub fn io(source: anyhow::Error, help: Option<String>) -> Self
    
    /// Create internal error with context
    pub fn internal(source: anyhow::Error, help: Option<String>) -> Self
    
    /// Create external command error with context
    pub fn external(source: anyhow::Error, help: Option<String>) -> Self
    
    /// Print detailed error information
    pub fn print_error(&self)
    
    /// Print simple error message
    pub fn print_simple(&self)
    
    /// Get help message if available
    pub fn help_message(&self) -> Option<&String>
}
```

### Command Implementations

#### Command Options

```rust
/// Options for package initialization
#[derive(Debug, Clone)]
pub struct InitOptions {
    pub name: Option<String>,
    pub description: Option<String>,
    pub author: Option<String>,
    pub version: String,
    pub license: String,
    pub houdini_min: Option<String>,
    pub houdini_max: Option<String>,
    pub bare: bool,
    pub vcs: String,
    pub base_dir: Option<PathBuf>,
}

/// Options for package updates
#[derive(Debug, Clone)]
pub struct UpdateOptions {
    pub package: Option<PathBuf>,
    pub packages: Vec<String>,
    pub dry_run: bool,
    pub yes: bool,
    pub output: OutputFormat,
}

/// Options for cleanup operations
#[derive(Debug, Clone)]
pub struct CleanupOptions {
    pub dry_run: bool,
    pub yes: bool,
    pub python_only: bool,
    pub comprehensive: bool,
    pub output: OutputFormat,
}
```

## Error Handling (hpm-error)

The `hpm-error` crate provides structured error handling infrastructure for the entire HPM system.

### Error Categories

#### Core Error Types

```rust
/// Base error trait for all HPM errors
pub trait HpmError: std::error::Error + Send + Sync + 'static {
    /// Get error category
    fn category(&self) -> ErrorCategory;
    
    /// Get error severity
    fn severity(&self) -> ErrorSeverity;
    
    /// Get help message if available
    fn help_message(&self) -> Option<String>;
    
    /// Get error code for programmatic handling
    fn error_code(&self) -> Option<&str>;
}

#[derive(Debug, Clone, PartialEq)]
pub enum ErrorCategory {
    Configuration,
    Package,
    Network,
    Io,
    Internal,
    External,
    Python,
    Registry,
    Resolution,
}

#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub enum ErrorSeverity {
    Info,
    Warning,
    Error,
    Critical,
}
```

## Type Reference

### Common Types

#### Package Identification

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PackageId {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageSpec {
    pub name: String,
    pub version: String,
    pub source: PackageSource,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PackageSource {
    Registry { url: String },
    Git { url: String, reference: GitReference },
    Local { path: PathBuf },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GitReference {
    Branch(String),
    Tag(String),
    Commit(String),
}
```

#### Time and Duration Types

```rust
// Re-exported from chrono
pub use chrono::{DateTime, Utc, Duration as ChronoDuration};

// Duration for timeouts and measurements
pub use std::time::{Duration, SystemTime, Instant};
```

#### Path Types

```rust
// Standard path types
pub use std::path::{Path, PathBuf};

// Path utilities
pub mod path_utils {
    /// Normalize path for cross-platform compatibility
    pub fn normalize_path(path: &Path) -> PathBuf
    
    /// Get relative path between two paths
    pub fn relative_path(from: &Path, to: &Path) -> Option<PathBuf>
    
    /// Check if path is within another path
    pub fn is_subpath(child: &Path, parent: &Path) -> bool
    
    /// Expand user home directory in path
    pub fn expand_tilde(path: &Path) -> PathBuf
}
```

## Trait Reference

### Storage Traits

```rust
/// Pluggable storage backend trait
#[async_trait]
pub trait Storage: Send + Sync {
    async fn store_package(&self, package: &PackageData) -> Result<String, StorageError>;
    async fn get_package(&self, package_id: &str) -> Result<Option<PackageData>, StorageError>;
    async fn search_packages(&self, query: &SearchQuery) -> Result<Vec<PackageMetadata>, StorageError>;
    async fn list_versions(&self, name: &str) -> Result<Vec<String>, StorageError>;
    async fn delete_package(&self, package_id: &str) -> Result<(), StorageError>;
}
```

### Authentication Traits

```rust
/// Authentication provider trait
#[async_trait]
pub trait AuthProvider: Send + Sync {
    async fn validate_token(&self, token: &str) -> Result<AuthToken, AuthError>;
    async fn create_token(&self, user_id: &str, scopes: Vec<TokenScope>) -> Result<AuthToken, AuthError>;
    async fn revoke_token(&self, token: &str) -> Result<(), AuthError>;
    async fn list_user_tokens(&self, user_id: &str) -> Result<Vec<AuthToken>, AuthError>;
}
```

### Extension Traits

```rust
/// Extension points for custom functionality
pub trait PackageValidator: Send + Sync {
    fn validate(&self, manifest: &PackageManifest) -> Result<(), ValidationError>;
    fn validator_name(&self) -> &str;
}

pub trait PackageTransformer: Send + Sync {
    fn transform(&self, manifest: &mut PackageManifest) -> Result<(), TransformError>;
    fn transformer_name(&self) -> &str;
}
```

This comprehensive API reference provides detailed information about all public interfaces in the HPM system. For usage examples and workflows, refer to the [User Guide](user-guide.md) and [Developer Documentation](developer-documentation.md).