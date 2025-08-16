# CLAUDE.md

This document provides development guidelines for the HPM (Houdini Package Manager) repository.

## Language Standards
- **Professional**: No use of colorful language, no emojis, etc.
- **Concise**: Brief, to the point, no fluff or fibbing.
- **Formal**: No buzzwords, no marketing language or boasting.

## Project Overview

HPM is a Rust-based package management system for SideFX Houdini, providing modern package management capabilities.

### Current Project Status
**Development Stage**: Core functionality implemented with comprehensive testing infrastructure
- **Core Features**: Package initialization, dependency management, cleanup systems working
- **Registry System**: QUIC transport and gRPC API implementation complete
- **Python Integration**: Virtual environment support with content-addressable sharing

### Core Commands Status
**✅ Implemented**: `init`, `add`, `remove`, `install`, `list`, `check`, `clean`
**📋 Planned**: `update`, `search`, `publish`, `run`

## Technology Stack
- **Language**: Rust (stable), Tokio async runtime
- **CLI**: Clap (derive API), TOML configuration with Serde
- **Registry**: QUIC transport, gRPC with Protocol Buffers
- **Dependency Resolution**: PubGrub-inspired solver with conflict learning
- **Storage**: Trait-based abstraction (Memory, PostgreSQL, S3)
- **Python Integration**: Bundled UV for dependency resolution
- **Console**: Professional styling with JSON output support

## MCP Configuration
**Global**: awesome-claude-code (best practices)
**Local**: postgres (registry development)

Claude Code built-in tools handle filesystem, GitHub, and IDE integration.

## Development Workflow

### Monthly Maintenance
```bash
# MCP server health and redundancy check
claude mcp list
claude mcp remove <redundant-server> -s local

# Configuration validation
claude --version
jq empty ~/.claude.json
```

### Quality Gates
```bash
cargo fmt --check
cargo clippy --all-features -- -D warnings
cargo test --workspace
cargo-machete  # Check unused dependencies
```

## Architecture Lessons Learned

### Dependency Management
- Use cargo-machete to identify unused dependencies
- Only add dependencies when implementing actual functionality
- Reduced dependencies from 35+ to 17, achieving 50% faster build times

### Crate Architecture
- Avoid circular dependencies - Core crate should not depend on all others
- Each crate should have single, well-defined responsibility
- Use trait boundaries and explicit interfaces between crates

### Testing Strategy
- Always use `tempfile::TempDir` for filesystem tests
- Unit tests first, integration tests after basic functionality exists
- Property-based testing for complex algorithms


## Development Commands

```bash
# Build and test
cargo build                         # Standard build
cargo test --workspace            # Execute all tests
cargo test -p hpm-<crate>         # Test specific crate

# Quality
cargo fmt --check
cargo clippy --all-features -- -D warnings
cargo-machete                     # Check unused dependencies

# CLI testing  
cargo run -- init test-package
cargo run -- add utility-nodes --version "^2.1.0"
cargo run -- install
cargo run -- list
cargo run -- clean --dry-run

# Debug logging
RUST_LOG=debug cargo run -- <command>
```

## Workspace Architecture

### Crate Structure
- **hpm-cli**: Command-line interface
- **hpm-core**: Storage, discovery, cleanup systems  
- **hpm-config**: Configuration management
- **hpm-registry**: QUIC/gRPC registry implementation
- **hpm-resolver**: Dependency resolution engine
- **hpm-package**: Manifest processing and Houdini integration
- **hpm-python**: Virtual environment isolation
- **hpm-error**: Error handling infrastructure

### Storage Architecture
```
~/.hpm/
├── packages/                     # Global package storage
├── venvs/                        # Python virtual environments (content-addressable)
└── cache/                        # Download cache

project/
├── .hpm/packages/                # Project-specific package links
├── hpm.toml                      # Project manifest
└── hpm.lock                      # Dependency lock file
```

## CLI Design

### Error Handling
- **Structured errors**: Config, Package, Network, I/O, Internal, External
- **Exit codes**: 0 (success), 1 (user error), 2 (internal error)
- **Styled output**: Colored symbols (✓✗⚠ℹ) with accessibility support

### Output Modes
```bash
hpm --quiet <command>           # Silent mode
hpm --verbose <command>         # Detailed output  
hpm --output json <command>     # JSON output
hpm --color never <command>     # Disable colors
```

## Core Package Management

### HPM Package Manifest (hpm.toml)
```toml
[package]
name = "my-houdini-tool"
version = "1.0.0"
description = "Custom Houdini digital assets and tools"

[houdini]
min_version = "19.5"
max_version = "20.5"

[dependencies]
utility-nodes = "^2.1.0"
material-library = { version = "1.5", optional = true }

[python_dependencies]
numpy = ">=1.20.0"
requests = { version = ">=2.25.0", extras = ["security"] }
```

### Standard Package Structure
```
package/
├── hpm.toml          # HPM package manifest
├── package.json      # Generated Houdini package file
├── otls/            # Digital assets (.hda, .otl files)
├── python/          # Python modules
├── scripts/         # Shelf tools and scripts
└── presets/         # Node presets
```

## Python Dependency Management

HPM provides virtual environment isolation for Python dependencies:

### Features
- **Content-addressable environments**: Packages with identical dependencies share virtual environments
- **UV-powered resolution**: High-performance dependency resolution using bundled UV
- **Houdini integration**: Seamless PYTHONPATH injection via generated package.json files
- **Intelligent cleanup**: Orphaned virtual environment detection and removal

### Houdini Version Mapping
| Houdini Version | Python Version |
|----------------|----------------|
| 19.0 - 19.5    | Python 3.7     |
| 20.0           | Python 3.9     |
| 20.5           | Python 3.10    |
| 21.x           | Python 3.11    |

### Cleanup Operations
```bash
hpm clean --python-only --dry-run    # Preview Python cleanup
hpm clean --comprehensive            # Clean packages + Python environments
```

## Testing Standards

### File System Testing
- Always use `tempfile::TempDir` for temporary operations
- Use absolute paths with `base_dir` parameter instead of changing working directory
- Validate both file existence AND content correctness

### Error Handling
```rust
// Domain-specific errors using thiserror
#[derive(Debug, thiserror::Error)]
pub enum InstallError {
    #[error("Package not found: {name}")]
    PackageNotFound { name: String },
    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),
}

// Application-level errors using anyhow
use anyhow::{Context, Result};
pub fn install_package(name: &str) -> Result<()> {
    download_package(name).context("Package download failed")?;
    Ok(())
}
```

