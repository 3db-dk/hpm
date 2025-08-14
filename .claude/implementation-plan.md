# HPM Package Storage Implementation Plan

## Overview

This document outlines the detailed implementation strategy for HPM's package storage and linking system, building on the architecture analysis and configuration foundation.

## Implementation Phases

### Phase 1: Global Storage Management

#### 1.1 Storage Manager Implementation

**Location**: `crates/hpm-core/src/storage.rs`

**Key Components**:
- `StorageManager` struct for managing global package storage
- Package installation and extraction
- Version management and conflict resolution
- Cache management and cleanup

**API Design**:
```rust
pub struct StorageManager {
    config: StorageConfig,
}

impl StorageManager {
    pub fn new(config: StorageConfig) -> Self;
    pub async fn install_package(&self, spec: &PackageSpec) -> Result<InstalledPackage>;
    pub async fn remove_package(&self, name: &str, version: &str) -> Result<()>;
    pub fn list_installed(&self) -> Vec<InstalledPackage>;
    pub fn package_exists(&self, name: &str, version: &str) -> bool;
    pub fn get_package_path(&self, name: &str, version: &str) -> PathBuf;
    pub async fn cleanup_unused(&self) -> Result<Vec<String>>;
}
```

#### 1.2 Package Installation Types

**Location**: `crates/hpm-core/src/storage/types.rs`

**Key Types**:
```rust
#[derive(Debug, Clone)]
pub struct InstalledPackage {
    pub name: String,
    pub version: String,
    pub manifest: PackageManifest,
    pub install_path: PathBuf,
    pub installed_at: SystemTime,
}

#[derive(Debug, Clone)]
pub struct PackageSpec {
    pub name: String,
    pub version_req: VersionReq,
    pub registry: Option<String>,
}
```

#### 1.3 Directory Initialization

**Implementation Steps**:
1. Create `~/.hpm` directory structure on first run
2. Initialize cache directories
3. Create registry index cache
4. Set up package storage directories

### Phase 2: Project Integration System

#### 2.1 Project Manager Implementation

**Location**: `crates/hpm-core/src/project.rs`

**Key Components**:
- `ProjectManager` struct for managing project-specific packages
- Houdini package.json generation
- Dependency resolution and linking
- Lock file management

**API Design**:
```rust
pub struct ProjectManager {
    project_config: ProjectConfig,
    storage_manager: Arc<StorageManager>,
}

impl ProjectManager {
    pub fn new(project_root: PathBuf, storage_manager: Arc<StorageManager>) -> Self;
    pub async fn add_dependency(&self, spec: &PackageSpec) -> Result<()>;
    pub async fn remove_dependency(&self, name: &str) -> Result<()>;
    pub async fn sync_dependencies(&self) -> Result<()>;
    pub fn generate_houdini_manifests(&self) -> Result<()>;
    pub fn list_dependencies(&self) -> Vec<ProjectDependency>;
}
```

#### 2.2 Houdini Package Generation

**Location**: `crates/hpm-package/src/houdini.rs`

**Key Functions**:
```rust
pub fn generate_houdini_package_json(
    package: &InstalledPackage,
    storage_path: &PathBuf,
) -> Result<HoudiniPackage>;

pub fn write_package_manifest(
    manifest_path: &PathBuf,
    houdini_package: &HoudiniPackage,
) -> Result<()>;
```

**Generated Package.json Structure**:
```json
{
    "name": "hpm-package-name",
    "description": "HPM managed package: package-name v1.0.0",
    "hpath": "$HOME/.hpm/packages/package-name@1.0.0/otls",
    "env": [
        {
            "PYTHONPATH": {
                "method": "prepend",
                "value": "$HOME/.hpm/packages/package-name@1.0.0/python"
            }
        },
        {
            "HOUDINI_SCRIPT_PATH": {
                "method": "prepend", 
                "value": "$HOME/.hpm/packages/package-name@1.0.0/scripts"
            }
        }
    ],
    "load_package_once": true
}
```

#### 2.3 Lock File Management

**Location**: `crates/hpm-core/src/lockfile.rs`

**Lock File Format** (`hpm.lock`):
```toml
[[package]]
name = "utility-nodes"
version = "2.1.0"
registry = "https://packages.houdini.org"
checksum = "sha256:abc123..."

[[package]]
name = "material-library"
version = "1.5.0"
registry = "https://packages.houdini.org"
checksum = "sha256:def456..."
```

### Phase 3: CLI Integration

#### 3.1 Command Enhancements

**Add Command** (`crates/hpm-cli/src/commands/add.rs`):
```rust
pub async fn execute_add(
    package_specs: Vec<String>,
    options: AddOptions,
) -> Result<()> {
    // 1. Parse package specifications
    // 2. Resolve dependencies
    // 3. Install to global storage
    // 4. Generate project manifests
    // 5. Update lock file
}
```

**Install Command** (`crates/hpm-cli/src/commands/install.rs`):
```rust
pub async fn execute_install(options: InstallOptions) -> Result<()> {
    // 1. Read hpm.toml and hpm.lock
    // 2. Install missing packages to global storage
    // 3. Generate project manifests
    // 4. Sync project dependencies
}
```

#### 3.2 Environment Variable Management

**Integration with Houdini**:
- Set `HOUDINI_PACKAGE_PATH` to include project's `.hpm/packages/`
- Provide shell script generation for environment setup
- Integration with Houdini startup scripts

### Phase 4: Advanced Features

#### 4.1 Version Conflict Resolution

**Strategy**:
1. Allow multiple versions in global storage
2. Use dependency resolution to select compatible versions
3. Warn about conflicts in project dependencies
4. Support version overrides in project configuration

#### 4.2 Package Update Management

**Implementation**:
- Track package update timestamps
- Check for updates from registry
- Handle breaking changes and migration
- Provide rollback capabilities

#### 4.3 Cleanup and Maintenance

**Features**:
- Remove unused packages from global storage
- Clean up orphaned project manifests
- Registry cache refresh
- Disk space management

## Implementation Order

### Week 1: Foundation
1. ✅ Extend configuration system with storage types
2. Create storage manager structure
3. Implement directory initialization
4. Basic package installation framework

### Week 2: Core Storage
1. Package download and extraction
2. Version management system
3. Global storage operations
4. Basic CLI integration

### Week 3: Project Integration
1. Project manager implementation
2. Houdini package.json generation
3. Dependency linking system
4. Lock file management

### Week 4: CLI and Testing
1. Enhanced CLI commands (add, install)
2. Environment variable management
3. Comprehensive testing suite
4. Documentation and examples

### Week 5: Advanced Features
1. Version conflict resolution
2. Update and cleanup operations
3. Error handling improvements
4. Performance optimizations

## Key Considerations

### Error Handling
- Graceful degradation when storage is unavailable
- Clear error messages for common failures
- Recovery from partially failed operations
- Rollback capabilities for interrupted operations

### Performance
- Concurrent package downloads
- Efficient dependency resolution
- Minimal filesystem operations
- Caching of registry metadata

### Security
- Package integrity verification
- Path traversal protection
- Safe extraction of archives
- Validation of package manifests

### Compatibility
- Support for existing Houdini packages
- Backward compatibility with package.json format
- Integration with Houdini's package loading system
- Cross-platform directory handling

## Testing Strategy

### Unit Tests
- Configuration management
- Storage operations
- Package manifest generation
- Dependency resolution logic

### Integration Tests
- End-to-end package installation
- Project setup and linking
- CLI command workflows
- Houdini integration scenarios

### File System Tests
- Use `tempfile::TempDir` for isolation
- Validate directory structure creation
- Test package extraction and linking
- Cleanup verification

## Next Steps

1. ✅ Complete configuration system extensions
2. Begin StorageManager implementation
3. Create basic package installation workflow
4. Implement project manifest generation
5. Add CLI command integration
6. Comprehensive testing and validation

This implementation plan provides a structured approach to building HPM's package management system while maintaining compatibility with Houdini's existing package system and following modern package manager best practices.