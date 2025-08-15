# HPM - Houdini Package Manager

A modern, Rust-based package management system for SideFX Houdini, providing industry-standard dependency management capabilities equivalent to npm for Node.js or cargo for Rust.

## 🚀 Features

### ✅ Fully Implemented
- **Package Initialization** - Create new Houdini packages with standardized structure
- **Dependency Management** - Add, remove, and list package dependencies with semantic versioning
- **Python Integration** - Full virtual environment support with content-addressable sharing
- **Package Installation** - Install dependencies from hpm.toml manifests
- **Project Cleanup** - Intelligent orphan package detection and removal
- **Configuration Validation** - Validate package configuration and Houdini compatibility

### 🔧 In Development
- **Registry System** - Complete QUIC/gRPC implementation (CLI integration pending)
- **Package Search** - Find packages in the registry
- **Package Publishing** - Publish packages to the registry
- **Package Updates** - Update packages to latest versions
- **Script Execution** - Execute package scripts

## 📦 Installation

### Prerequisites
- Rust 1.70 or later
- SideFX Houdini (19.5+)

### Build from Source
```bash
git clone https://github.com/hpm-org/hpm.git
cd hpm
cargo build --release
```

The `hpm` binary will be available at `target/release/hpm`.

## 🎯 Quick Start

### Initialize a New Package
```bash
# Create a standard Houdini package
hpm init my-houdini-tools --description "My custom Houdini tools"

# Create a minimal package (only hpm.toml)
hpm init my-package --bare
```

### Manage Dependencies
```bash
# Add a dependency
hpm add utility-nodes --version "^2.1.0"

# Add an optional dependency
hpm add material-library --optional

# Remove a dependency
hpm remove old-package

# Install all dependencies
hpm install

# List current dependencies
hpm list
```

### Package Cleanup
```bash
# Preview cleanup operations
hpm clean --dry-run

# Clean orphaned packages
hpm clean

# Clean only Python virtual environments
hpm clean --python-only
```

### Package Validation
```bash
# Validate current package configuration
hpm check
```

## 📁 Package Structure

HPM creates standardized Houdini package structures:

```
my-package/
├── hpm.toml           # Package manifest
├── package.json       # Generated Houdini package file
├── README.md          # Package documentation
├── otls/             # Digital assets (.hda, .otl files)
├── python/           # Python modules
│   └── __init__.py
├── scripts/          # Shelf tools and scripts
├── presets/          # Node presets
├── config/           # Configuration files
└── tests/            # Test files
```

## ⚙️ Configuration

### Package Manifest (hpm.toml)
```toml
[package]
name = "my-houdini-tool"
version = "1.0.0"
description = "Custom Houdini digital assets and tools"
authors = ["Your Name <email@example.com>"]
license = "MIT"

[houdini]
min_version = "19.5"
max_version = "21.0"

[dependencies]
utility-nodes = "^2.1.0"
material-library = { version = "1.5", optional = true }

[python_dependencies]
numpy = ">=1.20.0"
requests = { version = ">=2.25.0", extras = ["security"] }
```

### Global Configuration (~/.hpm/config.toml)
```toml
[registry]
default = "https://packages.houdini.org"

[install]
parallel_downloads = 8

[projects]
search_roots = ["/Users/username/houdini-projects"]
max_search_depth = 3
```

## 🏗️ Architecture

HPM is built with a modular architecture:

- **`hpm-cli`** - Command-line interface
- **`hpm-core`** - Core functionality (storage, discovery, cleanup)
- **`hpm-config`** - Configuration management
- **`hpm-package`** - Package manifest processing
- **`hpm-python`** - Python dependency management
- **`hpm-registry`** - QUIC/gRPC package registry
- **`hpm-error`** - Error handling infrastructure

## 🧪 Development

### Running Tests
```bash
# Run all tests
cargo test --workspace

# Run tests for specific crate
cargo test -p hpm-core

# Run integration tests
cargo test --test integration_tests

# Run with debug output
RUST_LOG=debug cargo test
```

### Code Quality
```bash
# Format code
cargo fmt

# Run linter
cargo clippy --workspace --all-features -- -D warnings

# Check for unused dependencies
cargo machete
```

### Development Commands
```bash
# Build all crates
cargo build --workspace

# Build release version
cargo build --release

# Run HPM CLI
cargo run --bin hpm -- --help

# Run registry server
cargo run -p hpm-registry --bin registry-server
```

## 📊 Project Status

**Development Stage**: Core functionality implemented with comprehensive testing infrastructure

- **Test Coverage**: 90% pass rate (53/59 tests) with isolated, reliable tests
- **Architecture**: Clean, modular design with proper separation of concerns  
- **Core Features**: Package initialization, dependency management, and cleanup systems working
- **Registry System**: Complete implementation with QUIC transport and gRPC API
- **Python Integration**: Full virtual environment support with content-addressable sharing

## 🤝 Contributing

1. Fork the repository
2. Create a feature branch (`git checkout -b feature/amazing-feature`)
3. Make your changes following the project standards
4. Add tests for new functionality
5. Run the test suite (`cargo test --workspace`)
6. Commit your changes (`git commit -m 'Add amazing feature'`)
7. Push to the branch (`git push origin feature/amazing-feature`)
8. Open a Pull Request

### Development Guidelines

- Follow Rust best practices and idioms
- Add comprehensive tests for new functionality
- Update documentation for API changes
- Use `cargo fmt` and `cargo clippy` before committing
- Follow semantic versioning for releases

## 📄 License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## 🔗 Links

- **Documentation**: Comprehensive development guidelines in [CLAUDE.md](CLAUDE.md)
- **Registry Architecture**: [docs/registry-architecture.md](docs/registry-architecture.md)  
- **Python Integration**: [docs/python-dependency-management.md](docs/python-dependency-management.md)
- **Cleanup System**: [docs/cleanup-system.md](docs/cleanup-system.md)

## 🆘 Support

- **Issues**: [GitHub Issues](https://github.com/hpm-org/hpm/issues)
- **Discussions**: [GitHub Discussions](https://github.com/hpm-org/hpm/discussions)
- **Documentation**: [CLAUDE.md](CLAUDE.md) for development guidelines