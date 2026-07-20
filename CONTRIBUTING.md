# Contributing to HPM

Thank you for your interest in contributing to HPM (Houdini Package Manager).

## Getting Started

### Prerequisites

#### Required Tools
- **Rust 1.85 or later** - The project requires modern Rust features
- **SideFX Houdini 20.5+** - For testing integration features and package compatibility
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

#### Git hooks

A versioned pre-commit hook lives in `.githooks/`. Point git at it once per
clone:

```bash
just install-hooks   # runs 'git config core.hooksPath .githooks'
```

The hook runs `just pre-commit` (fmt + clippy). Commit messages follow
[Conventional Commits](https://www.conventionalcommits.org/) by convention,
not enforcement.

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
  hpm-core/      Storage, installation, lock files, project discovery,
                 Python venv management (`python` submodule, bundled uv)
  hpm-config/    Configuration management
  hpm-package/   Package manifest parsing, Houdini integration
  hpm-assets/    Operator asset-index model emitted by `hpm pack`
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
PROPTEST_CASES=2000 cargo test --workspace   # Comprehensive testing
cargo audit                                  # Security audit
cargo machete                                # Unused dependencies
cargo doc --workspace --no-deps              # Documentation
```

No crate in the workspace declares a `[features]` table, so `--all-features`
is a no-op here.

### Cutting a release

Releases are built by Woodpecker CI, not locally. The process is:

1. Bump the workspace version and update `CHANGELOG.md`.
2. Push a `vX.Y.Z` tag.

Every pipeline in `.woodpecker/` is gated on `event: tag` /
`ref: refs/tags/v*`, so pushing the tag is what triggers the build:

- `check.yml` runs `cargo fmt --check`, clippy, and `cargo test --workspace`
  with `HPM_REQUIRE_HOUDINI=1` on a worker that has Houdini installed.
- `build-linux.yml`, `build-macos.yml`, and `build-windows.yml` each
  `depends_on: check`, so a failing check skips every platform build — the
  tag lands but publishes no binaries.

Artifacts are uploaded to GitHub Releases on `3db-dk/hpm` as
`hpm-v<version>-linux-x86_64`, `hpm-v<version>-darwin-universal` (a lipo of
`x86_64` and `aarch64`), and `hpm-v<version>-windows-x86_64.exe`. The builds
run natively on per-platform workers; `cross` is not used.

The upload itself lives in `ci/release-upload.sh` (linux, macOS) and
`ci/release-upload.ps1` (Windows) rather than inline in the pipeline YAML.
Each takes a source binary and an asset suffix, derives the version from
`CI_COMMIT_TAG`, creates the release if it does not exist, and uploads the
asset.

Re-running a tag pipeline is safe: the upload **replaces** an existing asset
of the same name rather than skipping it, so a rerun after amending the tag
publishes the new binary. The scripts fail loudly on any unexpected HTTP
status and verify after uploading that the asset is present, in state
`uploaded`, and the expected size — a red upload step means the release is
genuinely wrong, not merely re-run.

All three platform workflows race to create the release; exactly one gets
HTTP 201 and the others get 422, which is tolerated by design.

For a local release binary:

```bash
cargo build --release --workspace
```

## License

By contributing to HPM, you agree that your contributions will be licensed under the MIT License.
