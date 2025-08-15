# Contributing to HPM

Thank you for your interest in contributing to HPM (Houdini Package Manager)! This document provides guidelines and information for contributors.

## 🚀 Getting Started

### Prerequisites
- Rust 1.70 or later
- SideFX Houdini (19.5+) for testing integration features
- Git for version control

### Development Setup
```bash
# Clone the repository
git clone https://github.com/hpm-org/hpm.git
cd hpm

# Build the workspace
cargo build --workspace

# Run tests
cargo test --workspace

# Run formatting and linting
cargo fmt
cargo clippy --workspace --all-features -- -D warnings
```

## 📋 Development Guidelines

### Code Standards
- **Rust Best Practices**: Follow idiomatic Rust patterns and conventions
- **Documentation**: All public APIs must have comprehensive documentation
- **Testing**: New functionality requires comprehensive tests
- **Error Handling**: Use appropriate error types (`thiserror` for domain errors, `anyhow` for applications)
- **Formatting**: Use `cargo fmt` before committing
- **Linting**: Address all `cargo clippy` warnings

### Architecture Principles
- **Modular Design**: Keep crates focused on single responsibilities
- **Async Operations**: Use Tokio for all I/O operations
- **Proper Abstraction**: Use traits for testability and modularity
- **Configuration Management**: Hierarchical configuration (global, project, runtime)

### Testing Standards

#### File System Testing
- Always use `tempfile::TempDir` for temporary operations
- Use absolute paths with `base_dir` parameters instead of changing working directory
- Ensure proper cleanup and isolation between tests
- Validate both existence AND content of created files

```rust
#[tokio::test]
async fn test_functionality() {
    let temp_dir = TempDir::new().unwrap();
    
    // Use absolute paths, avoid working directory changes
    let options = SomeOptions {
        base_dir: Some(temp_dir.path().to_path_buf()),
        // ... other options
    };
    
    let result = some_function(options).await;
    assert!(result.is_ok());
    
    // Validate results
    let created_file = temp_dir.path().join("expected-file");
    assert!(created_file.exists());
    let content = fs::read_to_string(&created_file).unwrap();
    assert!(content.contains("expected content"));
    
    // TempDir automatically cleans up
}
```

#### Integration Testing
- Use `cargo test --test integration_tests` for end-to-end CLI testing
- Execute actual CLI binaries using `env!("CARGO_BIN_EXE_hpm")`
- Test complete user workflows, not just individual functions
- Include error scenarios and edge cases

## 🏗️ Project Structure

### Workspace Organization
```
hpm/
├── src/lib.rs                    # Workspace documentation
├── crates/
│   ├── hpm-cli/                  # Command-line interface
│   ├── hpm-core/                 # Core functionality (storage, discovery, cleanup)
│   ├── hpm-config/               # Configuration management
│   ├── hpm-package/              # Package manifest processing
│   ├── hpm-python/               # Python dependency management
│   ├── hpm-registry/             # QUIC/gRPC package registry
│   └── hpm-error/                # Error handling infrastructure
├── docs/                         # Technical documentation
├── scripts/                      # Development and maintenance scripts
└── tests/                        # Workspace-level integration tests
```

### Crate Responsibilities
- **hpm-cli**: User interface, command parsing, workflow orchestration
- **hpm-core**: Storage management, project discovery, dependency analysis
- **hpm-config**: Configuration loading, validation, and management
- **hpm-package**: Manifest parsing, package templates, Houdini integration
- **hpm-python**: Virtual environments, dependency resolution, Python integration
- **hpm-registry**: Network protocol, authentication, package distribution
- **hpm-error**: Structured error types and error handling utilities

## 🧪 Testing

### Running Tests
```bash
# All tests
cargo test --workspace

# Specific crate
cargo test -p hpm-core

# Integration tests
cargo test --test integration_tests

# With debug logging
RUST_LOG=debug cargo test

# Single-threaded (for working directory tests)
cargo test -- --test-threads=1
```

### Test Categories
1. **Unit Tests**: Test individual functions and modules in isolation
2. **Integration Tests**: Test interactions between crates and components  
3. **CLI Integration Tests**: Test complete user workflows via CLI binary
4. **Property Tests**: Test complex algorithms with generated inputs

### Current Test Status
- **Overall**: 91% pass rate (54/59 tests)
- **Core Modules**: All tests passing (hpm-core, hpm-python, hpm-registry)
- **CLI Tests**: Some working directory concurrency issues (being addressed)

## 🔄 Contribution Workflow

### 1. Issue Discussion
- For new features, create an issue to discuss the approach
- Reference existing issues in your contributions
- Follow issue templates when available

### 2. Development Process
```bash
# Create feature branch
git checkout -b feature/your-feature-name

# Make changes following coding standards
# Add comprehensive tests
# Update documentation

# Run quality checks
cargo fmt
cargo clippy --workspace --all-features -- -D warnings
cargo test --workspace

# Commit with clear messages
git commit -m "feat: add your feature description"

# Push and create PR
git push origin feature/your-feature-name
```

### 3. Pull Request Guidelines
- **Clear Description**: Explain what changes were made and why
- **Test Coverage**: Include tests for new functionality
- **Documentation Updates**: Update docs for API changes
- **Breaking Changes**: Clearly document any breaking changes
- **Small Focused PRs**: Keep changes focused and reviewable

### 4. Review Process
- All PRs require review before merging
- Address feedback promptly and thoroughly
- Maintain discussion focus on code quality and project goals
- Be respectful and constructive in all interactions

## 📚 Documentation

### Types of Documentation
1. **API Documentation**: Rust doc comments for all public interfaces
2. **User Guides**: Usage examples and tutorials in README.md
3. **Development Guidelines**: Technical documentation in CLAUDE.md
4. **Architecture Documentation**: System design in docs/ directory

### Documentation Standards
- Use clear, concise language
- Include practical examples
- Document error conditions and edge cases
- Keep documentation current with code changes

## 🐛 Bug Reports

### Before Reporting
- Search existing issues for duplicates
- Try to reproduce with the latest version
- Gather system information (OS, Rust version, Houdini version)

### Bug Report Template
```markdown
**Description**
Clear description of the bug.

**Steps to Reproduce**
1. Run command `hpm init test-package`
2. See error

**Expected Behavior**
What should have happened.

**Actual Behavior**
What actually happened.

**Environment**
- OS: macOS/Linux/Windows
- Rust Version: 1.70
- Houdini Version: 20.0
- HPM Version: 0.1.0

**Additional Context**
Logs, screenshots, or other relevant information.
```

## 💡 Feature Requests

### Enhancement Process
1. **Create Issue**: Use feature request template
2. **Discussion**: Community discussion on approach
3. **Design**: Technical design if complex
4. **Implementation**: Follow development process
5. **Documentation**: Update user and developer docs

### Implementation Priority
**Current Focus Areas:**
- Registry CLI integration (search, publish, update)
- Package script execution (`hpm run`)
- Enhanced error handling and user experience
- Performance optimizations

## 🎯 Code Review Checklist

### For Authors
- [ ] Tests added for new functionality
- [ ] Documentation updated
- [ ] Code formatted (`cargo fmt`)
- [ ] Linting passes (`cargo clippy`)
- [ ] All tests pass
- [ ] CHANGELOG.md updated if applicable

### For Reviewers
- [ ] Code follows project standards
- [ ] Tests are comprehensive and appropriate
- [ ] Documentation is clear and complete
- [ ] Error handling is appropriate
- [ ] Performance impact considered
- [ ] Security implications reviewed

## 📞 Getting Help

- **Questions**: Use GitHub Discussions for general questions
- **Bugs**: Use GitHub Issues for bug reports
- **Development**: Check CLAUDE.md for technical guidelines
- **Features**: Discuss new features in GitHub Issues

## 📄 License

By contributing to HPM, you agree that your contributions will be licensed under the MIT License.