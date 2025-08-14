# CLAUDE.md

This document provides development guidelines for the HPM (Houdini Package Manager) repository.

## Language Standards
- **Professional**: No use of colorful language, no emojis, etc.
- **Concise**: Brief, to the point, no fluff or fibbing.
- **Formal**: No buzzwords, no marketing language or boasting.

## Project Overview

HPM is a Rust-based package management system for SideFX Houdini, providing modern package management capabilities equivalent to npm for Node.js, uv for Python or cargo for Rust.

### Core Functionality

HPM delivers comprehensive package management for Houdini:
- **Authoring**: Package creation with standardized structure and metadata
- **Publishing**: Registry-based package distribution
- **Installation**: Automated package installation with dependency resolution
- **Management**: Package updates, removal, and lifecycle maintenance

### Architecture Benefits

- **Modern Workflows**: Industry-standard package management patterns
- **Dependency Resolution**: Automated dependency graph management
- **Version Management**: Semantic versioning with compatibility validation
- **Performance**: Concurrent operations via Rust and Tokio
- **Compatibility**: Seamless integration with existing Houdini packages
- **Discovery**: Centralized package registry and search capabilities

## Technology Stack

- **Language**: Rust (stable channel)
- **Build System**: Cargo
- **Runtime**: Tokio async runtime
- **CLI Framework**: Clap (derive API)
- **Configuration**: TOML format with Serde
- **Testing**: Built-in Rust testing + tokio-test for async

## MCP Integration

HPM is integrated with Model Context Protocol (MCP) servers for enhanced development capabilities:

### Configured MCP Servers
- **Filesystem Server**: Project file operations and management
- **GitHub Server**: Repository management and API integration
- **Sequential Thinking Server**: Complex task breakdown and planning
- **PostgreSQL Server**: Database operations for registry development

### MCP Usage
- Use `@filesystem` to access project files and resources
- Use `/thinking` for structured problem-solving workflows
- Access GitHub resources for repository operations
- Database queries and schema management for package registry

For detailed MCP setup and troubleshooting, see `.claude/mcp-setup.md`.

## Project Architecture Analysis

### Critical Lessons Learned

#### Dependency Management
- **Avoid over-dependencies**: Initial setup had 18 unused dependencies across crates
- **Use cargo-machete**: Essential tool for identifying unused dependencies in workspaces
- **Principle**: Only add dependencies when implementing actual functionality
- **Result**: Reduced dependencies from 35+ to 17, achieving 50% faster build times

#### Crate Architecture
- **Avoid circular dependencies**: Core crate should not depend on all other crates
- **Separate concerns**: Each crate should have a single, well-defined responsibility
- **Minimize coupling**: Use trait boundaries and explicit interfaces between crates
- **Error handling**: Define errors in the crate where they originate, not globally

#### Testing Strategy
- **Start with tests**: Empty crates provide no confidence in code quality
- **Unit tests first**: Focus on data structures and validation logic
- **Integration tests**: Add after basic functionality exists
- **Test coverage**: Aim for meaningful tests, not just coverage percentage

### Workspace Best Practices

#### Structure Guidelines
- **Flat layout**: Prefer `crates/` directory over nested hierarchies
- **Consistent naming**: Use project prefix (hpm-*) for all workspace crates
- **Virtual manifest**: Keep root workspace as virtual manifest, avoid main crate in root
- **Centralized dependencies**: Use workspace.dependencies for version consistency

#### Development Workflow
- **Single task runner**: Choose either Makefile OR justfile, not both
- **Quality gates**: Implement comprehensive checks (fmt, clippy, tests, audit)
- **Pre-commit hooks**: Automate quality enforcement but provide fallbacks
- **Documentation**: Maintain both high-level (CLAUDE.md) and detailed guides

### Common Pitfalls

#### What Not To Do
- **Empty placeholder crates**: Create functionality before crate structure
- **Tokio everywhere**: Only add async dependencies where async is actually needed
- **Makefile + justfile**: Redundant tooling creates confusion
- **No tests**: Zero tests means zero confidence in functionality
- **Circular imports**: Core crate importing all other crates violates separation

#### Red Flags
- More than 20% unused dependencies detected by cargo-machete
- Crates with only `// TODO` comments and no real functionality
- Build times over 10 seconds for small workspaces
- Pre-commit hooks failing due to tooling issues
- Missing or outdated documentation

### Success Metrics

#### Quality Indicators
- cargo-machete reports minimal unused dependencies
- All quality checks pass consistently
- Build times under 5 seconds for clean builds
- Working unit tests with real functionality
- Documentation stays current with implementation

#### Architecture Health
- Clear crate boundaries with minimal coupling
- Each crate has a single, well-defined purpose
- Dependencies flow in one direction without cycles
- Error types defined close to their usage
- Public APIs are well-documented and tested

### Future Development Guidelines

#### Before Adding New Crates
1. Verify the functionality justifies a separate crate
2. Define clear API boundaries and public interface
3. Implement basic functionality before adding to workspace
4. Add comprehensive unit tests from the beginning
5. Document the crate's purpose and integration points

#### Before Adding Dependencies
1. Check if functionality can be implemented in standard library
2. Verify the dependency is actively maintained
3. Consider the impact on build times and binary size
4. Add to workspace.dependencies for version consistency
5. Run cargo-machete regularly to catch unused dependencies

#### Development Process
1. Write tests first for new functionality
2. Use MCP servers for complex task planning
3. Maintain quality gates on every commit
4. Update documentation alongside code changes
5. Regular dependency audits and security scanning

For comprehensive analysis of architectural decisions and lessons learned, see `.claude/architecture-analysis.md`.

## Development Commands

### Build and Test
```bash
cargo build                    # Standard build
cargo build --release        # Optimized build
cargo test                   # Execute test suite
cargo test -- --nocapture   # Test with output
cargo run -- --help         # Run with help flag
```

### Code Quality
```bash
cargo fmt                            # Format source code
cargo clippy -- -D warnings         # Lint with warnings as errors
cargo clippy --all-features -- -D warnings  # Lint all features
cargo check                          # Validate without building
cargo-machete                        # Check for unused dependencies
./scripts/check-emojis.sh            # Enforce no-emoji policy
```

### Development Operations
```bash
RUST_LOG=debug cargo run -- install <package>  # Debug logging
cargo run -- init <name>                       # Initialize package
cargo test <module>::tests                     # Module-specific tests
cargo test --test integration                  # Integration tests only
cargo doc --open                               # Generate documentation
./scripts/check-emojis.sh                     # Check for emoji usage
```

### HPM CLI Testing
```bash
# Test CLI functionality
cargo run -- init test-package --description "Test package"
cargo run -- init --bare minimal-package
cargo run -- install utility-nodes
cargo run -- list
cargo run -- search "geometry tools"
```

## Project Architecture

HPM implements a modular workspace architecture optimized for package management operations.

### Workspace Structure
- **`crates/hpm-cli`** - Command-line interface and application entry point
- **`crates/hpm-core`** - Core functionality and orchestration
- **`crates/hpm-config`** - Configuration management system
- **`crates/hpm-registry`** - Package registry communication layer
- **`crates/hpm-resolver`** - Dependency resolution engine
- **`crates/hpm-installer`** - Package installation subsystem
- **`crates/hpm-package`** - Package manifest processing and validation
- **`crates/hpm-error`** - Error handling infrastructure

### Design Principles
- **Asynchronous Operations**: Tokio runtime for all I/O operations
- **Structured Error Handling**: Domain errors via `thiserror`, application errors via `anyhow`
- **Interface Abstraction**: Trait-based design for testability and modularity
- **Layered Configuration**: Hierarchical configuration management (global, project, runtime)
- **Modular Crates**: Clear separation of concerns with minimal coupling

## CLI Design and Package Management

### Command Structure

HPM provides comprehensive package management through industry-standard CLI patterns:

#### Core Commands
- `hpm init` - Initialize new Houdini packages with templates
- `hpm add` - Add packages and resolve dependencies
- `hpm remove` - Remove installed packages
- `hpm update` - Update packages to latest versions
- `hpm list` - Display installed packages and dependency tree
- `hpm search` - Search registry for packages
- `hpm publish` - Publish packages to registry
- `hpm info` - Show detailed package information
- `hpm run` - Execute package scripts
- `hpm check` - Validate package configuration and Houdini compatibility
- `hpm clean` - Clean build artifacts and caches

#### Package Templates
- **Standard** (default): Complete Houdini package with all standard directories
- **Bare**: Minimal structure with only hpm.toml for custom layouts

See `docs/cli-design.md` for comprehensive CLI specification.

## Houdini Integration

HPM extends Houdini's native package system with modern dependency management capabilities.

### HPM Package Manifest (hpm.toml)

The primary package descriptor supporting comprehensive metadata and dependency management:

```toml
[package]
name = "my-houdini-tool"
version = "1.0.0"
description = "Custom Houdini digital assets and tools"
authors = ["Author <email@example.com>"]
license = "MIT"
readme = "README.md"
keywords = ["houdini"]

[houdini]
min_version = "19.5"
max_version = "20.5"

[dependencies]
utility-nodes = "^2.1.0"
material-library = { version = "1.5", optional = true }

[scripts]
build = "python scripts/build.py"
test = "python -m pytest tests/"
```

### Standard Package Structure
```
my-package/
├── hpm.toml           # HPM package manifest
├── package.json       # Generated Houdini package file
├── README.md          # Package documentation
├── otls/             # Digital assets (.hda, .otl files)
│   └── my_node.hda
├── python/           # Python modules
│   └── my_tool.py
├── scripts/          # Shelf tools and scripts
├── presets/          # Node presets
├── config/           # Configuration files
└── tests/            # Test files
```

### Package.json Generation
HPM automatically generates standard Houdini `package.json` files from `hpm.toml` configuration, ensuring seamless integration with existing Houdini workflows.

### Supported Asset Types
- **Digital Assets**: Houdini Digital Assets (.hda, .otl)
- **Python Modules**: Libraries and tools for Houdini Python environment
- **Scripts**: Shelf tools, event handlers, and automation scripts
- **Presets**: Node parameter presets and configurations
- **Configuration**: Environment and pipeline configuration files

## Development Standards

### Code Style
- Adhere to standard Rust formatting (`rustfmt`)
- Apply comprehensive linting (`cargo clippy`)
- Implement explicit error handling (avoid panics)
- Document all public APIs with doc comments

### Testing Framework

#### Core Testing Principles
- **Unit tests**: Module-level tests using `#[cfg(test)]`
- **Integration tests**: End-to-end testing in `tests/` directory
- **Mock implementations**: External dependency abstraction
- **Property-based testing**: Complex algorithm verification

#### File System Testing Standards
For functionality that creates files and directories (like `hpm init`):

**Test Fixtures and Cleanup**:
- Always use `tempfile::TempDir` for temporary file system operations
- Never rely on global file system state that could affect other tests
- Restore working directory after tests that change it

```rust
#[tokio::test]
async fn test_init_package_standard() {
    let temp_dir = TempDir::new().unwrap();
    let original_dir = env::current_dir().unwrap();
    
    env::set_current_dir(temp_dir.path()).unwrap();
    // ... test logic ...
    env::set_current_dir(original_dir).unwrap();
    
    // TempDir automatically cleans up when dropped
}
```

**Content Validation Requirements**:
- Verify both file/directory existence AND content correctness
- Test all expected files and directories, not just a subset
- Validate generated content matches expected structure and values
- Test edge cases with special characters, missing optional fields

```rust
// Validate file existence
assert!(package_path.join("hpm.toml").exists());
assert!(package_path.join("python").is_dir());

// Validate file content
let hpm_toml_content = fs::read_to_string(package_path.join("hpm.toml")).unwrap();
assert!(hpm_toml_content.contains("name = \"test-package\""));
assert!(hpm_toml_content.contains("version = \"1.0.0\""));
```

**Error Case Testing**:
- Test failure scenarios (directory already exists, invalid input)
- Verify error messages are helpful and accurate
- Ensure partial failures are handled gracefully

#### Test Organization
- Group related tests in modules using `#[cfg(test)]`
- Use descriptive test names that clearly indicate what is being tested
- Include helper functions for common validation patterns
- Run tests with `--test-threads=1` when tests modify working directory

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
    download_package(name)
        .context("Package download failed")?;
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

## System Integration

HPM integrates with Houdini through standardized mechanisms:
- **Package Discovery**: Installation to Houdini package directories
- **Manifest Translation**: Generation of `package.json` from `hpm.toml`
- **Path Management**: Configuration of `hpath`, `HOUDINI_PATH`, and environment variables
- **Version Compatibility**: Enforcement of Houdini version constraints
- **Asset Registration**: Automated registration of digital assets and Python modules

### Installation Paths
Package installation follows Houdini conventions:
- `$HOUDINI_USER_PREF_DIR/packages/` - User-specific packages
- `$HOUDINI_PACKAGE_DIR` - Project-specific installations
- `~/.hpm/` - HPM registry cache and metadata

## Security Framework

- **Package Verification**: Cryptographic signature validation for integrity assurance
- **Sandboxed Installation**: Isolated package extraction and installation processes
- **Path Validation**: Directory traversal attack prevention
- **Dependency Auditing**: Automated vulnerability scanning for package dependencies

## Contributing

### Contribution Process
1. **Repository Setup**: Fork repository and create feature branches
2. **Development**: Implement changes following project standards
3. **Testing**: Ensure comprehensive test coverage and validation
4. **Documentation**: Update documentation for API modifications
5. **Review**: Submit changes for peer review and approval

### Common Issues

| Issue | Resolution |
|-------|------------|
| Build Failures | Verify current Rust toolchain installation |
| Network Errors | Validate proxy configuration and registry connectivity |
| Permission Errors | Confirm write access to target installation directories |
| Version Conflicts | Analyze dependency tree using `cargo tree` |