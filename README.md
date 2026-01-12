# HPM - Houdini Package Manager

A modern, Rust-based package management system for SideFX Houdini, providing industry-standard dependency management capabilities equivalent to npm for Node.js, uv for Python, or cargo for Rust.

[![Rust](https://img.shields.io/badge/rust-1.70+-orange.svg)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://opensource.org/licenses/MIT)
[![Tests](https://img.shields.io/badge/tests-245%20passing-green.svg)](https://github.com/hpm-org/hpm)

## ✨ Key Features

### 🎯 Package Management
- **Package Initialization** - Create standardized Houdini packages with templates (standard/bare)
- **Dependency Management** - Add, remove, and list dependencies with semantic versioning
- **Smart Installation** - Install dependencies from hpm.toml manifests with dependency resolution
- **Project-Aware Cleanup** - Intelligent orphan detection with safety guarantees

### 🐍 Python Integration
- **Content-Addressable Virtual Environments** - Share environments between packages with identical dependencies
- **UV-Powered Resolution** - High-performance dependency resolution with complete isolation
- **Automatic Houdini Integration** - Seamless PYTHONPATH injection via package.json generation
- **Conflict Detection** - Automatic detection and reporting of dependency conflicts

### 🏗️ Advanced Architecture
- **PubGrub Dependency Resolution** - State-of-the-art algorithm with conflict learning
- **Git-Based Dependencies** - Secure dependency pinning with commit hashes
- **Professional CLI** - UV-inspired error handling with machine-readable output
- **Comprehensive Testing** - 245+ tests with property-based testing and 100% pass rate

## 🚀 Current Status

**Production Ready Core**: All essential package management functionality implemented and tested

### ✅ Fully Implemented & Tested
- **Package Creation**: `hpm init` with standard and bare templates
- **Dependency Management**: `hpm add`, `hpm remove`, `hpm list` with semantic versioning
- **Installation System**: `hpm install` with Python virtual environment support
- **Cleanup System**: `hpm clean` with project-aware orphan detection
- **Configuration Validation**: `hpm check` for package and Houdini compatibility

### 🔧 Additional Features
- **Shell Completions**: `hpm completions` for bash, zsh, fish, PowerShell
- **Multi-Package Add**: Add multiple packages in a single command
- **Tree View**: `hpm list --tree` for visual dependency display
- **Package Updates**: `hpm update` for keeping dependencies current

## 📦 Quick Installation

### Prerequisites
- Rust 1.70 or later
- SideFX Houdini 19.5+

### Build from Source
```bash
git clone https://github.com/hpm-org/hpm.git
cd hpm
cargo build --release
```

The `hpm` binary will be available at `target/release/hpm`.

## 🎯 Quick Start

### Create Your First Package
```bash
# Create a full Houdini package with all directories
hpm init my-houdini-tools --description "My custom Houdini tools"

# Create a minimal package (only hpm.toml)
hpm init my-package --bare
```

### Manage Dependencies
```bash
# Add dependencies from Git (with commit pinning for security)
hpm add utility-nodes --git https://github.com/studio/utility-nodes --commit abc1234

# Add multiple packages at once
hpm add pkg1 pkg2 --git https://github.com/studio/tools --commit def5678

# Add local path dependency
hpm add local-tools --path ../my-local-tools

# Add optional dependency
hpm add material-library --git https://github.com/studio/materials --commit 789xyz --optional

# Install all dependencies (including Python packages)
hpm install

# List current dependencies
hpm list

# View as tree
hpm list --tree

# Remove dependencies
hpm remove old-package
```

### Python Dependencies
```toml
# Python dependencies are specified in hpm.toml
[python_dependencies]
numpy = ">=1.20.0"
requests = { version = ">=2.25.0", extras = ["security"] }
matplotlib = { version = "^3.5.0", optional = true }
```

### Smart Cleanup
```bash
# Preview what would be cleaned (recommended)
hpm clean --dry-run

# Clean orphaned packages (preserves active dependencies)
hpm clean

# Clean Python virtual environments
hpm clean --python-only

# Comprehensive cleanup (packages + Python)
hpm clean --comprehensive
```

## 📁 Package Structure

HPM creates standardized Houdini package structures:

```
my-package/
├── hpm.toml           # Package manifest (HPM)
├── package.json       # Generated Houdini package file
├── README.md          # Package documentation
├── .gitignore         # Git ignore file
├── otls/              # Digital assets (.hda, .otl files)
├── python/            # Python modules
│   └── __init__.py
├── scripts/           # Shelf tools and scripts
├── presets/           # Node presets
├── config/            # Configuration files
└── tests/             # Test files
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
readme = "README.md"
keywords = ["houdini"]

[houdini]
min_version = "19.5"
max_version = "21.0"

# HPM dependencies (Git-based with commit pinning)
[dependencies]
utility-nodes = { git = "https://github.com/studio/utility-nodes", commit = "abc1234" }
material-library = { git = "https://github.com/studio/materials", commit = "def5678", optional = true }

# Python dependencies (managed in isolated virtual environments)
[python_dependencies]
numpy = ">=1.20.0"
requests = { version = ">=2.25.0", extras = ["security"] }
matplotlib = { version = "^3.5.0", optional = true }

# Package scripts
[scripts]
build = "python scripts/build.py"
test = "python -m pytest tests/"
```

## 🏗️ System Architecture

HPM implements a sophisticated modular architecture optimized for package management:

```
┌─────────────────────────────────────────────────────────────────┐
│                       HPM Architecture                          │
├─────────────────────────────────────────────────────────────────┤
│                                                                 │
│  CLI Layer (hpm-cli)                                           │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │ Professional CLI with UV-inspired error handling       │   │
│  │ • Machine-readable output (JSON)                       │   │
│  │ • Comprehensive command validation                      │   │
│  │ • Interactive prompts and confirmations                 │   │
│  └─────────────────────────────────────────────────────────┘   │
│                              │                                  │
│  Core Systems (hpm-core)     ▼                                  │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │ • Project-aware package storage                         │   │
│  │ • Intelligent cleanup with orphan detection            │   │
│  │ • Project discovery and dependency analysis             │   │
│  └─────────────────────────────────────────────────────────┘   │
│                              │                                  │
│  Dependency Resolution        ▼                                  │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │ PubGrub Algorithm (hpm-resolver)                        │   │
│  │ • Conflict learning and backtracking                    │   │
│  │ • Performance optimization                              │   │
│  │ • Comprehensive version constraint support              │   │
│  └─────────────────────────────────────────────────────────┘   │
│                              │                                  │
│  Python Integration          ▼                                  │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │ Content-Addressable Virtual Environments (hpm-python)  │   │
│  │ • UV-powered resolution with complete isolation        │   │
│  │ • Virtual environment sharing and cleanup              │   │
│  │ • Automatic Houdini integration                        │   │
│  └─────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────┘
```

### Crate Organization
- **`hpm-cli`** - Command-line interface with comprehensive commands
- **`hpm-core`** - Core functionality (storage, discovery, cleanup systems)
- **`hpm-config`** - Configuration management with project discovery
- **`hpm-package`** - Package manifest processing and Houdini integration
- **`hpm-python`** - Python dependency management with virtual environment isolation
- **`hpm-resolver`** - PubGrub-inspired dependency resolution engine
- **`hpm-error`** - Structured error handling infrastructure

## 📊 Technical Excellence

### Testing & Quality
- **245+ Tests**: Comprehensive test coverage with 100% pass rate
- **Property-Based Testing**: Advanced testing with proptest for edge case discovery
- **Integration Testing**: End-to-end CLI testing with real filesystem operations
- **Zero Dependencies Waste**: Regular cargo-machete validation ensures clean dependencies

### Performance & Reliability
- **Async Architecture**: Tokio-based async runtime for optimal performance
- **Content-Addressable Storage**: Efficient disk usage with virtual environment sharing
- **Intelligent Caching**: Smart caching strategies to avoid redundant operations
- **Error Recovery**: Comprehensive error handling with detailed context

### Code Quality
- **Modern Rust**: Leverages latest Rust features and best practices
- **Structured Errors**: Domain-specific error types with thiserror
- **Professional CLI**: Industry-standard command-line interface patterns
- **Comprehensive Documentation**: 16,000+ lines of documentation

## 🧪 Development

### Quick Development Setup
```bash
# Clone and build
git clone https://github.com/hpm-org/hpm.git
cd hpm
cargo build

# Run all tests (245+ tests)
cargo test --workspace

# Run specific crate tests
cargo test -p hpm-core
cargo test -p hpm-python

# Code quality checks
cargo fmt
cargo clippy --workspace --all-features -- -D warnings
cargo-machete  # Check for unused dependencies
```

### Development Commands
```bash
# Run HPM CLI commands during development
cargo run -- init test-package
cargo run -- add some-pkg --git https://github.com/example/repo --commit abc123
cargo run -- install
cargo run -- list --tree
cargo run -- clean --dry-run
cargo run -- completions bash

# Debug mode with logging
RUST_LOG=debug cargo run -- install
```

## 📚 Comprehensive Documentation

HPM provides enterprise-grade documentation (16,000+ lines) for all audiences:

### 👥 For Users
- **[User Guide](docs/user-guide.md)** - Complete installation, usage, and troubleshooting guide
- **[Tutorials & Examples](docs/tutorials-and-examples.md)** - Step-by-step workflows and real-world scenarios
- **[Python Integration Guide](docs/python-user-guide.md)** - Managing Python dependencies in Houdini

### 👨‍💻 For Developers
- **[Developer Guide](docs/developer-documentation.md)** - Architecture overview and contribution guidelines
- **[API Reference](docs/api-reference.md)** - Complete API documentation for all public interfaces
- **[Testing Guide](docs/testing-configuration.md)** - Comprehensive testing documentation and best practices

### 🏗️ For Architecture
- **[Technical Deep Dives](docs/system-deep-dives.md)** - Detailed explanations of complex systems
- **[Technical Architecture](docs/technical-architecture.md)** - System architecture and design patterns
- **[Cleanup System](docs/cleanup-system.md)** - Project-aware cleanup with safety guarantees

**[📖 Complete Documentation Index](docs/README.md)** - Full documentation overview and navigation

## 🤝 Contributing

We welcome contributions! HPM follows high standards for code quality and testing.

### Getting Started
1. Fork the repository
2. Create a feature branch (`git checkout -b feature/amazing-feature`)
3. Make your changes following the project standards
4. Add comprehensive tests for new functionality
5. Run the full test suite (`cargo test --workspace`)
6. Run quality checks (`cargo fmt && cargo clippy`)
7. Commit your changes (`git commit -m 'Add amazing feature'`)
8. Push to the branch (`git push origin feature/amazing-feature`)
9. Open a Pull Request

### Development Standards
- **Testing**: All new features must have comprehensive tests
- **Documentation**: Update documentation for API changes
- **Code Quality**: Use `cargo fmt` and `cargo clippy` before committing
- **Error Handling**: Use structured error types with helpful messages
- **Performance**: Consider async patterns and efficient algorithms

See **[Developer Guide](docs/developer-documentation.md)** for detailed contribution guidelines.

## 📈 Project Metrics

- **Lines of Code**: ~12,000 lines of Rust across 7 crates
- **Test Coverage**: 245+ tests with 100% pass rate
- **Documentation**: 16,000+ lines of comprehensive documentation
- **Dependencies**: Minimal, well-audited dependency tree
- **Performance**: Async architecture with intelligent caching
- **Quality**: Zero clippy warnings, comprehensive error handling

## 📄 License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## 🆘 Support & Community

- **[GitHub Issues](https://github.com/hpm-org/hpm/issues)** - Bug reports and feature requests
- **[GitHub Discussions](https://github.com/hpm-org/hpm/discussions)** - Community discussions and questions  
- **[Complete Documentation](docs/README.md)** - Comprehensive guides and references

---

**HPM** - Modern package management for the modern Houdini pipeline. Built with Rust for performance, reliability, and developer experience.