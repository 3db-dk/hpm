# Contributing to HPM

Thank you for your interest in contributing to HPM (Houdini Package Manager).

## Getting Started

### Prerequisites
- Rust 1.74 or later
- SideFX Houdini (19.5+) for testing integration features
- Git

### Development Setup
```bash
git clone https://github.com/3db-dk/hpm.git
cd hpm

cargo build --workspace
cargo test --workspace
cargo fmt
cargo clippy --workspace --all-targets -- -D warnings
```

## Development Guidelines

### Code Standards
- Follow idiomatic Rust patterns and conventions
- All public APIs must have documentation
- New functionality requires tests
- Use `thiserror` for domain errors, `anyhow` for application errors
- Run `cargo fmt` and `cargo clippy` before committing

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

Run integration tests with `cargo test --test integration_tests`.

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

## License

By contributing to HPM, you agree that your contributions will be licensed under the MIT License.
