# Contributing to HPM

Thank you for your interest in contributing to HPM (Houdini Package Manager).

## Getting Started

### Prerequisites

#### Required Tools
- **Rust 1.74 or later** - The project requires modern Rust features
- **SideFX Houdini 19.5+** - For testing integration features and package compatibility
- **Git** - Version control and contribution workflow

#### Optional Tools
- **cargo-machete** - Detect unused dependencies
- **cargo-tarpaulin** - Code coverage analysis
- **cargo-audit** - Security vulnerability scanning
- **hyperfine** - Performance benchmarking

### Development Setup

```bash
git clone https://github.com/3db-dk/hpm.git
cd hpm

cargo build --workspace
cargo test --workspace
cargo fmt
cargo clippy --workspace --all-targets -- -D warnings
```

#### Additional Tools Installation
```bash
cargo install cargo-machete cargo-audit cargo-tarpaulin
```

#### IDE Configuration (VS Code)
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

Recommended extensions: rust-analyzer, CodeLLDB, crates, Error Lens.

#### Environment Variables
```bash
export RUST_LOG=debug              # Enable debug logging
export HPM_DEV=1                   # Development mode flag
export PROPTEST_CASES=100          # Faster property tests during development
```

## Project Structure

```
crates/
  hpm-cli/       CLI frontend (clap)
  hpm-core/      Storage, installation, lock files, project discovery
  hpm-config/    Configuration management
  hpm-resolver/  PubGrub dependency resolver
  hpm-package/   Package manifest parsing, Houdini integration
  hpm-python/    Python venv management (uv integration)
  hpm-error/     Shared error types
```

## Development Guidelines

### Code Standards
- Follow idiomatic Rust patterns and conventions
- All public APIs must have documentation
- New functionality requires tests
- Use `thiserror` for domain errors, `anyhow` for application errors
- Run `cargo fmt` and `cargo clippy` before committing

### Error Handling Patterns
```rust
// Good: Domain-specific errors with context
#[derive(Debug, thiserror::Error)]
pub enum PackageError {
    #[error("Package not found: {name}@{version}")]
    NotFound { name: String, version: String },

    #[error("Invalid version specification: {spec}")]
    InvalidVersion { spec: String, source: semver::Error },

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

// Good: Application-level error handling with context
pub fn install_package(spec: &PackageSpec) -> Result<InstallResult> {
    let package_data = download_package(spec)
        .context("Failed to download package from registry")?;

    validate_package(&package_data)
        .with_context(|| format!("Package validation failed for {}", spec.name))?;

    Ok(InstallResult { /* ... */ })
}
```

### Async/Await Patterns
```rust
// Good: Non-blocking async I/O
pub async fn read_config() -> Result<String> {
    let content = tokio::fs::read_to_string("file.txt").await?;
    Ok(content)
}

// Bad: Blocking operations in async context
pub async fn bad_read() -> Result<String> {
    let content = std::fs::read_to_string("file.txt")?; // blocks the runtime
    Ok(content)
}
```

### Testing Standards

Use `tempfile::TempDir` for filesystem tests. Use absolute paths instead of changing the working directory.

```rust
#[tokio::test]
async fn test_functionality() {
    let temp_dir = TempDir::new().unwrap();
    let options = SomeOptions {
        base_dir: Some(temp_dir.path().to_path_buf()),
    };
    let result = some_function(options).await;
    assert!(result.is_ok());
}
```

See the [Testing Guide](docs/testing.md) for property-based testing details.

## Contribution Workflow

1. Create a feature branch: `git checkout -b feature/your-feature`
2. Make changes, add tests, update documentation
3. Run quality checks: `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
4. Commit with a clear message: `git commit -m "feat: add your feature"`
5. Push and create a pull request

### Pull Request Guidelines
- Explain what changed and why
- Include tests for new functionality
- Keep PRs focused and reviewable
- Update CHANGELOG.md if applicable

## Release Process

### Version Management
HPM uses semantic versioning (SemVer).

```bash
# Minor release (new features, backward compatible)
cargo set-version --bump minor

# Patch release (bug fixes)
cargo set-version --bump patch
```

### Pre-Release Checklist
```bash
PROPTEST_CASES=2000 cargo test --workspace --all-features  # Comprehensive testing
cargo audit                                                  # Security audit
cargo machete                                                # Unused dependencies
cargo doc --workspace --all-features --no-deps               # Documentation
```

### Release Build
```bash
cargo build --release --workspace

# Cross-platform builds
cross build --target x86_64-unknown-linux-gnu --release
cross build --target x86_64-pc-windows-gnu --release
cross build --target x86_64-apple-darwin --release
```

## License

By contributing to HPM, you agree that your contributions will be licensed under the MIT License.
