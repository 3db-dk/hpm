# HPM (Houdini Package Manager) Architecture

## System Overview

HPM is a Rust-based package management system designed specifically for SideFX Houdini. It brings modern package management capabilities to the Houdini ecosystem, similar to what npm does for Node.js or uv does for Python. HPM enhances Houdini's native package system with dependency resolution, version management, and centralized distribution.

## Core Architecture Principles

### Performance and Efficiency
- **Rust Native**: High-performance systems programming language
- **Concurrent Operations**: Async/await patterns for network operations
- **Memory Safety**: Zero-cost abstractions with compile-time guarantees
- **Cross-Platform**: Windows, macOS, and Linux compatibility

### Package Management Design
- **Semantic Versioning**: Full semver support for Houdini package versions
- **Dependency Resolution**: Robust dependency graph management for HDAs and tools
- **Registry System**: Centralized distribution of Houdini packages
- **Houdini Integration**: Compatible with native Houdini package system
- **Asset Types**: Support for HDAs, Python modules, scripts, and configurations

### Security and Reliability
- **Cryptographic Verification**: Package signing and integrity validation
- **Sandboxed Installation**: Isolated package environments
- **Rollback Support**: Safe package installation and removal
- **Audit Logging**: Comprehensive operation tracking

## System Components

### 1. CLI Interface (`src/main.rs`, `src/cli/`)

**Primary Responsibilities:**
- Command-line argument parsing and validation
- User interaction and feedback
- Operation orchestration and error handling
- Configuration management

**Key Commands:**
```bash
hpm init        # Initialize new Houdini package
hpm build       # Build package and generate package.json
hpm install     # Install packages and dependencies
hpm remove      # Remove installed packages
hpm update      # Update packages to latest versions
hpm search      # Search package registries
hpm publish     # Publish packages to registry
hpm info        # Display package information
hpm list        # List installed packages
```

### 2. Package Registry (`src/registry/`)

**Architecture Pattern:** Client-Server with REST API
- **Client**: HTTP client for registry communication
- **Protocol**: RESTful API with JSON payloads
- **Authentication**: Token-based authentication
- **Caching**: Local metadata caching for performance

**Registry Operations:**
- Package search and discovery
- Metadata retrieval and caching
- Version resolution and compatibility
- Download management and verification

### 3. Package Resolution (`src/resolver/`)

**Dependency Management:**
- **Graph Resolution**: Topological sorting of dependencies
- **Version Constraints**: Semver range satisfaction
- **Conflict Resolution**: Multiple version handling strategies
- **Feature Flags**: Optional package features and compilation

**Resolution Strategy:**
```
Package Request → Dependency Graph → Version Selection → Installation Plan
       ↓               ↓                    ↓                   ↓
   User Command → graph_builder → version_resolver → install_planner
```

### 4. Installation System (`src/installer/`)

**Installation Process:**
1. Package download and verification
2. Dependency installation order determination
3. File extraction and placement
4. System integration (library registration, path setup)
5. Post-install script execution

**File Management:**
- **Package Storage**: Versioned package storage
- **Library Installation**: Binary and library placement
- **Configuration**: System path and environment setup
- **Cleanup**: Uninstallation and cleanup procedures

### 5. Configuration Management (`src/config/`)

**Configuration Hierarchy:**
```
~/.hpm/config.toml              # Global HPM configuration
$PROJECT/.hpm/hpm.toml          # Project-specific settings  
$SYSTEM/packages.json           # System package integration
```

**Configuration Elements:**
- Registry URLs and authentication
- Installation paths and preferences
- Proxy and network settings
- Default package sources

## Package Structure

### Package Manifest (`hpm.toml`)
```toml
[package]
name = "example-lib"
version = "1.0.0"
description = "Example library package"
authors = ["Author Name <author@example.com>"]
license = "MIT"
keywords = ["library", "utilities", "tools"]

[dependencies]
utility-crate = "^2.1.0"
config-lib = { version = "1.5", optional = true }

[system]
min_version = "1.0"
platforms = ["windows", "linux", "macos"]
categories = ["utilities", "libraries"]

[[binaries]]
name = "example_tool"
path = "bin/example_tool"
type = "executable"
platforms = ["all"]
```

### Package Types

**1. Binary Packages**
- Executable applications and tools
- Native libraries and shared objects
- Platform-specific implementations

**2. Library Packages**
- Software libraries and frameworks
- Development tools and utilities
- Integration libraries and APIs

**3. Configuration Packages**
- Environment setup and preferences
- Application configurations
- Template collections and presets

### Package Installation

**Directory Structure:**
```
~/.hpm/
├── packages/                   # HPM package storage
│   ├── bin/                    # Installed executables
│   ├── lib/                    # Library modules
│   ├── config/                 # Configuration files
│   └── metadata/               # Package metadata
├── registry/                   # Registry cache
└── temp/                       # Temporary files
```

## Data Flow Architecture

### Package Installation Workflow
```
1. Package Resolution
   ├── Registry query and metadata retrieval
   ├── Dependency graph construction
   ├── Version constraint satisfaction
   └── Installation plan generation

2. Download and Verification
   ├── Package archive download
   ├── Cryptographic signature verification
   ├── Integrity check (checksums)
   └── Malware scanning (optional)

3. Installation Process
   ├── Dependency installation (recursive)
   ├── File extraction and placement
   ├── System path registration
   └── Post-install script execution

4. Integration
   ├── Binary registration and linking
   ├── Library path updates
   ├── Environment variable setup
   └── Configuration file updates
```

### Registry Communication
```
HPM Client → HTTP/HTTPS → Package Registry → Database/Storage
     ↓           ↓              ↓                ↓
   Search → REST API → Metadata Query → Package Index
   Request   (JSON)     (Database)      (File System)
```

## Security Architecture

### Package Verification
- **Digital Signatures**: Ed25519 cryptographic signatures
- **Checksum Validation**: SHA-256 integrity verification
- **Source Verification**: Publisher identity validation
- **Sandboxed Execution**: Isolated post-install scripts

### Network Security
- **TLS Encryption**: All registry communication encrypted
- **Certificate Pinning**: Registry certificate validation
- **Proxy Support**: Corporate firewall compatibility
- **Authentication**: Token-based registry access

### File System Security
- **Permission Management**: Minimal required file permissions
- **Path Validation**: Prevention of directory traversal attacks
- **Symlink Protection**: Safe symbolic link handling
- **Cleanup Guarantees**: Complete uninstallation support

## Performance Considerations

### Optimization Strategies
- **Parallel Downloads**: Concurrent package downloads
- **Incremental Updates**: Delta downloads for package updates
- **Local Caching**: Aggressive metadata and package caching
- **Lazy Loading**: On-demand registry queries

### Scalability Factors
- **Registry Mirroring**: Multiple registry endpoints
- **CDN Integration**: Content delivery network support
- **Bandwidth Management**: Configurable download limits
- **Storage Optimization**: Deduplication and compression

## Error Handling and Recovery

### Failure Scenarios
- **Network Failures**: Retry logic with exponential backoff
- **Disk Space**: Pre-installation space validation
- **Permission Errors**: Clear error messages and solutions
- **Corruption**: Automatic package re-download and verification

### Recovery Mechanisms
- **Transaction Logging**: Atomic installation operations
- **Rollback Support**: Complete installation reversal
- **Partial Failure**: Continue with successful operations
- **State Validation**: Installation integrity checking

## Development and Testing

### Development Environment
- **Rust Toolchain**: Latest stable Rust compiler
- **Testing Framework**: Comprehensive unit and integration tests
- **CI/CD Pipeline**: Automated testing and deployment
- **Documentation**: Inline docs and external guides

### Quality Assurance
- **Code Coverage**: High test coverage requirements
- **Static Analysis**: Clippy and other linting tools
- **Security Audit**: Regular dependency vulnerability scanning
- **Performance Testing**: Benchmark suites for critical paths

## Integration Points

### System Integration
- **Package Loading**: Native package discovery and loading
- **Binary Management**: Executable discovery and linking
- **Library Modules**: Module path and import system
- **Environment Setup**: Path and configuration management

### External Services
- **Package Registries**: Public and private registry support
- **Version Control**: Git integration for source packages
- **Build Systems**: Integration with Rust and Python build tools
- **Monitoring**: Usage analytics and error reporting

## Future Architecture Evolution

### Planned Enhancements
- **Plugin System**: Extensible command and operation plugins
- **Build Integration**: Source package building and compilation
- **Enterprise Features**: Role-based access and audit trails
- **GUI Application**: Desktop application for package management

### Technology Roadmap
- **WebAssembly**: Browser-based package tools
- **Cloud Integration**: Cloud storage and synchronization
- **AI Features**: Intelligent dependency suggestions
- **Ecosystem Integration**: Third-party tool compatibility