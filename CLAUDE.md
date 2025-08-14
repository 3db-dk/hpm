# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

This is the **HPM (Houdini Package Manager)** repository - a Rust-based package management system for SideFX Houdini. HPM brings modern package management capabilities to the Houdini ecosystem, similar to what npm does for Node.js or uv does for Python.

### Goal

HPM provides comprehensive tooling for Houdini package workflows:
- **Authoring**: Create new Houdini packages with proper structure and metadata
- **Publishing**: Distribute packages to registries for community sharing
- **Installing**: Download and install packages with dependency resolution
- **Managing**: Update, remove, and maintain installed packages

### Key Benefits

- **Modern Workflows**: Cargo/npm-style package management for Houdini
- **Dependency Resolution**: Automatic handling of package dependencies
- **Version Management**: Semantic versioning and compatibility checking  
- **Performance**: Fast Rust implementation with parallel operations
- **Compatibility**: Works with existing Houdini package system
- **Discovery**: Centralized registry for package distribution and discovery

## Technology Stack

- **Language**: Rust (stable channel)
- **Build System**: Cargo
- **Runtime**: Tokio async runtime
- **CLI Framework**: Clap (derive API)
- **Configuration**: TOML format with Serde
- **Testing**: Built-in Rust testing + tokio-test for async

## Development Commands

### Build and Test
```bash
# Build the project
cargo build

# Run all tests
cargo test

# Run tests with output
cargo test -- --nocapture

# Build and run
cargo run -- --help

# Build release version
cargo build --release
```

### Code Quality
```bash
# Format code
cargo fmt

# Run clippy linter
cargo clippy -- -D warnings

# Run clippy with all features
cargo clippy --all-features -- -D warnings

# Check without building
cargo check
```

### Development Workflow
```bash
# Run in development mode with logging
RUST_LOG=debug cargo run -- install example-package

# Test package operations
cargo run -- init my-package
cargo run -- build
cargo run -- publish

# Run specific test module  
cargo test resolver::tests

# Run integration tests only
cargo test --test integration

# Generate documentation
cargo doc --open
```

## Project Architecture

HPM follows a modular Rust architecture optimized for package management:

### Core Modules
- **`src/main.rs`** - CLI entry point and command orchestration
- **`src/cli/`** - Command-line interface and argument parsing  
- **`src/config/`** - Configuration management and persistence
- **`src/registry/`** - Package registry client and communication
- **`src/resolver/`** - Dependency resolution and version management
- **`src/installer/`** - Package installation and file management
- **`src/package/`** - Package manifest and metadata handling
- **`src/error/`** - Error types and handling utilities

### Key Design Patterns
- **Async/Await**: All I/O operations use Tokio async runtime
- **Error Propagation**: Consistent error handling with `anyhow` and `thiserror`
- **Trait Abstractions**: Testable interfaces for registry and file system operations
- **Configuration Layers**: Global, project, and runtime configuration management

## Houdini Package System

HPM works with Houdini's native package system while adding modern package management features.

### Houdini Package Basics

Houdini packages are JSON files that define:
- Environment variables and paths
- Houdini path modifications (`hpath`)
- Conditional loading based on version/OS
- Package dependencies and load order

### HPM Enhancement

HPM adds a manifest file (`hpm.toml`) alongside the standard Houdini `package.json`:

```toml
[package]
name = "my-houdini-tool"
version = "1.0.0"
description = "Custom Houdini digital assets and tools"
authors = ["Author <email@example.com>"]
license = "MIT"
keywords = ["houdini", "modeling", "vfx"]

[houdini]
min_version = "20.0"
max_version = "21.0"
contexts = ["sop", "lop", "cop"]

[dependencies]
utility-nodes = "^2.1.0"
material-library = { version = "1.5", optional = true }

[[assets]]
name = "my_custom_node"
path = "otls/my_custom_node.hda"
type = "hda"
contexts = ["sop"]
```

### Package Structure
```
my-package/
├── hpm.toml           # HPM package manifest
├── package.json       # Standard Houdini package file
├── otls/             # Digital assets (.hda, .otl files)
│   └── my_node.hda
├── python/           # Python modules
│   └── my_tool.py
├── scripts/          # Shelf tools and scripts
├── presets/          # Node presets
└── config/           # Configuration files
```

### Supported Asset Types
- **HDAs**: Houdini Digital Assets (.hda, .otl files)
- **Python**: Python libraries and modules for Houdini
- **Scripts**: Shelf tools, event scripts, and automation
- **Presets**: Node presets and configurations
- **Config**: Environment and pipeline configurations

## Development Conventions

### Code Style
- Follow standard Rust conventions (`rustfmt`)
- Use `cargo clippy` for additional linting
- Prefer explicit error handling over panics
- Document public APIs with doc comments

### Testing Strategy
- Unit tests in `src/` modules using `#[cfg(test)]`
- Integration tests in `tests/` directory
- Mock implementations for external dependencies
- Property-based testing for complex algorithms

### Error Handling
```rust
// Use thiserror for domain errors
#[derive(Debug, thiserror::Error)]
pub enum InstallError {
    #[error("Package not found: {name}")]
    PackageNotFound { name: String },
    
    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),
}

// Use anyhow for application errors
use anyhow::{Context, Result};

pub fn install_package(name: &str) -> Result<()> {
    download_package(name)
        .context("Failed to download package")?;
    Ok(())
}
```

## Configuration

### Global Configuration (`~/.hpm/config.toml`)
```toml
[registry]
default = "https://packages.houdini.org"

[install] 
path = "packages/hpm"
parallel_downloads = 8

[auth]
token = "your-registry-token"
```

### Project Configuration (`project/.hpm/hpm.toml`)
```toml
[dependencies]
utility-nodes = "^2.1.0"
material-library = { version = "1.5", optional = true }

[dev-dependencies]
test-assets = "0.1.0"
```

## Houdini Integration

HPM integrates with Houdini through:
- **Package Discovery**: Installs to Houdini's package directories
- **JSON Generation**: Creates compatible `package.json` files from `hpm.toml`
- **Path Management**: Manages `hpath`, `HOUDINI_PATH`, and environment variables
- **Version Compatibility**: Ensures packages work with specified Houdini versions
- **Asset Registration**: Automatic OTL and Python module registration

### Installation Locations
HPM installs packages to standard Houdini locations:
- `$HOUDINI_USER_PREF_DIR/packages/` - User packages
- Project-specific locations via `HOUDINI_PACKAGE_DIR`
- Custom registry and cache in `~/.hpm/`

## Security Considerations

- **Package Verification**: Cryptographic signatures for package integrity
- **Sandboxed Installation**: Safe package extraction and installation
- **Path Validation**: Prevention of directory traversal attacks
- **Dependency Auditing**: Security vulnerability scanning for dependencies

## Contributing Guidelines

1. **Fork and Clone**: Standard GitHub workflow
2. **Feature Branches**: Create branches for new features
3. **Testing**: Ensure all tests pass before submitting
4. **Documentation**: Update docs for public API changes
5. **Code Review**: All changes require review before merge

## Troubleshooting

### Common Issues
- **Build Failures**: Ensure Rust toolchain is up to date
- **Network Errors**: Check proxy settings and registry connectivity
- **Permission Errors**: Verify write access to installation directories
- **Version Conflicts**: Use `cargo tree` to debug dependency issues