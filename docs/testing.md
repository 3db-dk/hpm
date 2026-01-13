# HPM Testing Guide

This comprehensive guide covers HPM's testing strategy, including property-based testing, configuration, CI/CD integration, and best practices for contributors.

## Table of Contents

- [Testing Architecture Overview](#testing-architecture-overview)
- [Test Distribution](#test-distribution)
- [Property-Based Testing](#property-based-testing)
  - [Why Property-Based Testing](#why-property-based-testing)
  - [Writing Property Tests](#writing-property-tests)
  - [Testing Strategies and Patterns](#testing-strategies-and-patterns)
- [Fuzzing](#fuzzing)
  - [Running Fuzz Tests](#running-fuzz-tests)
  - [Fuzz Targets](#fuzz-targets)
  - [Adding New Fuzz Tests](#adding-new-fuzz-tests)
- [Running Tests](#running-tests)
- [Configuration](#configuration)
- [CI/CD Integration](#cicd-integration)
- [IDE Integration](#ide-integration)
- [Best Practices](#best-practices)
- [Troubleshooting](#troubleshooting)
- [Contributing Guidelines](#contributing-guidelines)

## Testing Architecture Overview

HPM employs a multi-tiered testing strategy:

```
+-------------------------------------------------------------+
|                     HPM Testing Strategy                     |
+-------------------------------------------------------------+
| 1. Property-Based Tests (proptest)                          |
|    - Automated edge case discovery                          |
|    - Business logic invariant verification                  |
|    - Regression prevention                                  |
+-------------------------------------------------------------+
| 2. Traditional Unit Tests                                   |
|    - Specific edge cases                                    |
|    - Known bug reproduction                                 |
|    - API contract verification                              |
+-------------------------------------------------------------+
| 3. Integration Tests                                        |
|    - End-to-end workflows                                   |
|    - CLI command testing                                    |
|    - Cross-module functionality                             |
+-------------------------------------------------------------+
| 4. Performance Tests                                        |
|    - Build time validation                                  |
|    - Memory usage verification                              |
|    - Dependency analysis                                    |
+-------------------------------------------------------------+
```

## Test Distribution

| Test Type | Count | Coverage |
|-----------|-------|----------|
| Property-based Tests | 23 | Core business logic invariants |
| Traditional Unit Tests | 150+ | Specific scenarios and edge cases |
| Integration Tests | 25+ | End-to-end workflows |
| Doc Tests | 15+ | API documentation examples |

### Crate Coverage

| Crate | Property Tests | Focus Area |
|-------|---------------|------------|
| `hpm-core` | 7 tests | Storage types, package specs, version compatibility |
| `hpm-package` | 7 tests | Manifest validation, serialization, Houdini integration |
| `hpm-python` | 9 tests | Python versions, dependencies, virtual environments |

### Test Organization Structure

```
crates/
├── hpm-core/
│   ├── src/storage/types.rs
│   │   └── tests module with property tests
│   └── proptest-regressions/
│       └── storage/types.txt    # Regression test cases
├── hpm-package/
│   ├── src/lib.rs
│   │   └── tests module with property tests
│   └── proptest-regressions/    # Generated automatically
└── hpm-python/
    ├── src/types.rs
    │   └── tests module with property tests
    └── proptest-regressions/
```

## Property-Based Testing

HPM uses [proptest](https://crates.io/crates/proptest), a Rust property-based testing framework that automatically generates test inputs to verify that invariant properties hold across a wide range of scenarios.

### Why Property-Based Testing

Property-based testing complements traditional unit tests by:
- **Automatically discovering edge cases** that developers might not consider
- **Testing business logic invariants** that should always hold
- **Providing better test coverage** through random input generation
- **Finding bugs early** in the development cycle

#### Traditional vs Property-Based Testing

Traditional unit tests have limited scope:

```rust
// Traditional unit test - limited scope
#[test]
fn test_version_parsing() {
    assert_eq!("1.0.0".parse::<Version>().unwrap().to_string(), "1.0.0");
    assert_eq!("2.1.3".parse::<Version>().unwrap().to_string(), "2.1.3");
    // What about edge cases? Unicode? Very long strings? Invalid formats?
}
```

Property-based tests provide comprehensive coverage:

```rust
// Property test - comprehensive coverage
proptest! {
    #[test]
    fn prop_version_roundtrip(version in version_strategy()) {
        let version_str = version.to_string();
        let parsed: Version = version_str.parse().unwrap();
        prop_assert_eq!(version, parsed); // Tests hundreds of random inputs
    }
}
```

#### Real Bug Discovery in HPM

Property-based testing discovered a real bug in HPM's VersionReq validation:

**Bug Found**: `VersionReq::new(" ")` (whitespace-only string) was incorrectly accepted as valid

**Property Test That Found It**:
```rust
#[test]
fn prop_version_req_invalid(whitespace in r"\s*") {
    let result = VersionReq::new(&whitespace);
    prop_assert!(result.is_err());
}
```

**Fix Applied**: Changed validation from `requirement.is_empty()` to `requirement.trim().is_empty()`

### Writing Property Tests

#### 1. Define Custom Strategies

Strategies define how proptest generates random test inputs:

```rust
/// Strategy to generate valid package names
fn package_name_strategy() -> impl Strategy<Value = String> {
    prop::string::string_regex("[a-z][a-z0-9-]{1,50}")
        .unwrap()
        .prop_filter("Package name must not end with hyphen", |name| {
            !name.ends_with('-') && name.len() >= 2 && name.len() <= 50
        })
}

/// Strategy to generate semantic versions
fn version_strategy() -> impl Strategy<Value = String> {
    (0u32..100, 0u32..100, 0u32..100)
        .prop_map(|(major, minor, patch)| format!("{}.{}.{}", major, minor, patch))
}
```

#### 2. Write Property Tests

Property tests use the `proptest!` macro:

```rust
proptest! {
    /// Test that package specs can be parsed and maintain consistency
    #[test]
    fn prop_package_spec_parse_roundtrip(
        name in package_name_strategy(),
        version in version_req_strategy()
    ) {
        let spec_str = format!("{}@{}", name, version);
        let spec = PackageSpec::parse(&spec_str).unwrap();

        prop_assert_eq!(spec.name, name);
        prop_assert_eq!(spec.version_req.as_str(), version);
    }
}
```

#### 3. Test Categories

**Roundtrip Testing** - Verify that operations are reversible:

```rust
#[test]
fn prop_manifest_serialization_roundtrip(manifest in package_manifest_strategy()) {
    let toml_str = toml::to_string(&manifest).unwrap();
    let deserialized: PackageManifest = toml::from_str(&toml_str).unwrap();

    prop_assert_eq!(manifest.package.name, deserialized.package.name);
    prop_assert_eq!(manifest.package.version, deserialized.package.version);
}
```

**Invariant Testing** - Verify that business rules always hold:

```rust
#[test]
fn prop_valid_manifests_pass_validation(manifest in package_manifest_strategy()) {
    prop_assert!(manifest.validate().is_ok());
}
```

**Consistency Testing** - Verify that operations produce consistent results:

```rust
#[test]
fn prop_resolved_set_hash_consistency(resolved in resolved_dependency_set_strategy()) {
    let hash1 = resolved.hash();
    let hash2 = resolved.hash();
    prop_assert_eq!(hash1, hash2, "Hash should be consistent");
}
```

### Testing Strategies and Patterns

#### Common Strategy Patterns

**Constrained Random Generation**:
```rust
fn python_version_strategy() -> impl Strategy<Value = PythonVersion> {
    (2u8..=3, 6u8..=12, prop::option::of(0u8..=20))
        .prop_map(|(major, minor, patch)| PythonVersion::new(major, minor, patch))
}
```

**Enum and Union Types**:
```rust
fn dependency_spec_strategy() -> impl Strategy<Value = DependencySpec> {
    prop_oneof![
        version_req_strategy().prop_map(DependencySpec::Simple),
        // ... detailed variant generation
    ]
}
```

**Complex Structure Generation**:
```rust
fn package_manifest_strategy() -> impl Strategy<Value = PackageManifest> {
    (
        package_name_strategy(),
        version_strategy(),
        prop::option::of(description_strategy()),
        // ... more fields
    ).prop_map(|(name, version, description, ...)| {
        PackageManifest {
            package: PackageInfo {
                name,
                version,
                description,
                // ... rest of structure
            },
            // ... rest of manifest
        }
    })
}
```

#### Advanced Patterns

**Conditional Testing**:
```rust
#[test]
fn prop_dependency_merge_compatible(deps1 in deps_strategy(), deps2 in deps_strategy()) {
    // Only test if there are no conflicts
    if !has_conflicts(&deps1, &deps2) {
        let result = deps1.merge(&deps2);
        prop_assert!(result.is_ok());
    }
}
```

**Error Condition Testing**:
```rust
#[test]
fn prop_version_req_invalid(whitespace in r"\s*") {
    let result = VersionReq::new(&whitespace);
    prop_assert!(result.is_err());
}
```

## Fuzzing

HPM uses fuzzcheck-rs for structure-aware fuzzing of security-sensitive parsing code. Unlike cargo-fuzz, fuzzcheck works on all platforms including Windows.

### Running Fuzz Tests

Fuzz tests require the Rust nightly toolchain:

```bash
# Install nightly if not already installed
rustup install nightly

# Run fuzz tests for a specific package
cargo +nightly test fuzz_ --release -p hpm-package --ignored

# Run fuzz tests for all packages
cargo +nightly test fuzz_ --release --workspace --ignored -- --test-threads=1
```

The `--ignored` flag is required because fuzz tests are marked as ignored by default to prevent them from running during normal test runs.

### Fuzz Targets

| Crate | Target | Description |
|-------|--------|-------------|
| `hpm-package` | `fuzz_manifest_parsing` | TOML manifest parsing |
| `hpm-package` | `fuzz_dependency_spec_json` | Dependency spec JSON parsing |
| `hpm-package` | `fuzz_python_dependency_spec` | Python dependency parsing |
| `hpm-resolver` | `fuzz_version_req_parsing` | Version requirement parsing |
| `hpm-resolver` | `fuzz_version_parsing` | Version string parsing |
| `hpm-core` | `fuzz_lock_file_parsing` | Lock file TOML parsing |
| `hpm-core` | `fuzz_package_spec_parsing` | Package spec parsing |

### Adding New Fuzz Tests

Fuzz tests live in `fuzz_tests.rs` modules within each crate:

```rust
// crates/<crate>/src/fuzz_tests.rs

#[cfg(test)]
mod fuzz {
    use fuzzcheck::fuzz_test;

    #[test]
    #[ignore]
    fn fuzz_my_parser() {
        let result = fuzz_test(|input: &String| {
            // Parser should never panic on any input
            let _ = my_parser(input);
        })
        .default_mutator()
        .serde_serializer()
        .default_sensor_and_pool()
        .arguments_from_cargo_fuzzcheck()
        .stop_after_first_test_failure(true)
        .launch();

        assert!(!result.found_test_failure, "Fuzzing found a failure");
    }
}
```

Key points:
- Mark tests with `#[ignore]` so they don't run during normal test execution
- Use `.stop_after_first_test_failure(true)` to stop immediately when a bug is found
- Fuzz tests should only test that parsing doesn't panic - they shouldn't validate correctness

### CI/CD Fuzzing

Fuzz tests run automatically in CI via the `.github/workflows/fuzz.yml` workflow:
- Runs on PRs that modify crate source files
- Runs weekly for extended fuzzing
- Uses nightly Rust with coverage instrumentation

## Running Tests

### Local Development

```bash
# Run all tests
cargo test --workspace

# Run property tests only
cargo test prop_

# Run specific crate tests
cargo test -p hpm-core
cargo test -p hpm-package
cargo test -p hpm-python

# Run with more test cases
PROPTEST_CASES=1000 cargo test prop_

# Debug failing tests
cargo test prop_test_name -- --nocapture
```

### Test Commands Summary

```bash
# Quick tests (development)
PROPTEST_CASES=100 cargo test --workspace --all-features

# Standard tests (CI default)
PROPTEST_CASES=256 cargo test --workspace --all-features

# Thorough tests (nightly CI)
PROPTEST_CASES=1000 cargo test --workspace --all-features

# Property tests with verbose output
PROPTEST_VERBOSE=1 PROPTEST_CASES=100 cargo test prop_ --workspace --all-features -- --nocapture

# Sequential execution (when needed for file system tests)
cargo test --workspace --all-features -- --test-threads=1
```

## Configuration

### Environment Variables

Property-based tests can be configured via environment variables:

```bash
# Number of test cases to generate (default: 256)
export PROPTEST_CASES=1000

# Maximum shrinking iterations (default: 1024)
export PROPTEST_MAX_SHRINK_ITERS=10000

# Timeout per test case in milliseconds (default: none)
export PROPTEST_TIMEOUT=5000

# Enable verbose output
export PROPTEST_VERBOSE=1

# Disable regression file saving (not recommended)
export PROPTEST_DISABLE_FAILURE_PERSISTENCE=0
```

### Cargo.toml Test Profile

```toml
[profile.test]
debug = 1  # Reduced debug info for faster builds
incremental = true
codegen-units = 16  # Parallel codegen
```

## CI/CD Integration

### GitHub Actions Configuration

```yaml
# .github/workflows/test.yml
name: Comprehensive Testing

on: [push, pull_request]

env:
  CARGO_TERM_COLOR: always
  PROPTEST_CASES: 512
  PROPTEST_MAX_SHRINK_ITERS: 2048
  PROPTEST_TIMEOUT: 10000

jobs:
  property-tests:
    name: Property-Based Tests
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable

      - name: Cache dependencies
        uses: actions/cache@v3
        with:
          path: |
            ~/.cargo/registry
            ~/.cargo/git
            target/
          key: ${{ runner.os }}-cargo-${{ hashFiles('**/Cargo.lock') }}

      - name: Run property tests
        run: cargo test prop_ --workspace --all-features -- --nocapture

  comprehensive-tests:
    name: Full Test Suite
    runs-on: ubuntu-latest
    strategy:
      matrix:
        test-config:
          - { cases: 100, name: "quick" }
          - { cases: 1000, name: "thorough" }
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable

      - name: Run tests
        env:
          PROPTEST_CASES: ${{ matrix.test-config.cases }}
        run: cargo test --workspace --all-features
```

### Pre-commit Hook Configuration

```yaml
# .pre-commit-config.yaml
repos:
  - repo: local
    hooks:
      - id: property-tests
        name: Property-based tests
        entry: bash
        args:
          - -c
          - |
            export PROPTEST_CASES=256
            cargo test prop_ --workspace --all-features -- --test-threads=1
        language: system
        types: [rust]
        pass_filenames: false
```

## IDE Integration

### VS Code Configuration

```json
// .vscode/settings.json
{
  "rust-analyzer.cargo.features": "all",
  "rust-analyzer.check.command": "test",
  "rust-analyzer.check.extraArgs": ["--workspace", "--all-features"],
  "rust-analyzer.test.extraArgs": ["--", "--nocapture"],

  "rust-analyzer.cargo.extraEnv": {
    "PROPTEST_CASES": "100",
    "PROPTEST_VERBOSE": "0"
  },

  "rust-analyzer.lens.enable": true,
  "rust-analyzer.lens.methodReferences": true,
  "rust-analyzer.lens.references": true
}
```

### Test Tasks Configuration

```json
// .vscode/tasks.json
{
  "version": "2.0.0",
  "tasks": [
    {
      "label": "test-property",
      "type": "shell",
      "command": "cargo",
      "args": ["test", "prop_", "--workspace", "--all-features"],
      "env": {
        "PROPTEST_CASES": "256",
        "PROPTEST_VERBOSE": "1"
      },
      "group": "test",
      "problemMatcher": "$rustc"
    }
  ]
}
```

## Best Practices

### Strategy Design

**Good** - Constrained generation:
```rust
fn package_name_strategy() -> impl Strategy<Value = String> {
    prop::string::string_regex("[a-z][a-z0-9-]{1,50}")
        .unwrap()
        .prop_filter("Valid package name", |name| {
            !name.ends_with('-') && name.len() >= 2
        })
}
```

**Avoid** - Too broad generation:
```rust
// Generates mostly invalid inputs
fn bad_name_strategy() -> impl Strategy<Value = String> {
    any::<String>() // Will generate Unicode, empty strings, etc.
}
```

### Property Assertions

**Good** - Clear, specific assertions:
```rust
prop_assert_eq!(result.name, expected_name);
prop_assert!(result.is_valid(), "Result should be valid");
```

**Avoid** - Vague assertions:
```rust
prop_assert!(something_happened); // What happened?
```

### Test Organization

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    // Custom strategies grouped at top
    fn strategy_a() -> impl Strategy<Value = TypeA> { ... }
    fn strategy_b() -> impl Strategy<Value = TypeB> { ... }

    // Property tests
    proptest! {
        #[test]
        fn prop_test_name(input in strategy_a()) {
            // Test logic
        }
    }

    // Traditional unit tests for specific edge cases
    #[test]
    fn specific_edge_case() {
        // Traditional test
    }
}
```

### Performance Considerations

- **Default test count**: Proptest runs 256 tests by default
- **Configure test count**: Use environment variables or test attributes
- **Strategy efficiency**: Avoid expensive filters when possible

```rust
// Efficient strategy
(0u32..100).prop_map(|n| format!("package-{}", n))

// Less efficient - many rejections
any::<String>().prop_filter("valid", |s| s.starts_with("package-"))
```

### Regression Testing

Proptest automatically saves failing test cases to regression files:

```
crates/hpm-core/proptest-regressions/storage/types.txt
```

**Always commit regression files** - they ensure previously found bugs don't reappear.

## Troubleshooting

### Common Issues

#### Slow Test Execution

```bash
# Problem: Tests taking too long
# Solution: Reduce PROPTEST_CASES for development
export PROPTEST_CASES=50

# Or run quick tests
PROPTEST_CASES=100 cargo test --workspace
```

#### Memory Issues

```bash
# Problem: Out of memory during property tests
# Solution: Run tests sequentially
cargo test prop_ --workspace --all-features -- --test-threads=1

# Or limit memory usage (Linux/macOS)
ulimit -v 2097152  # 2GB limit
```

#### Flaky Tests

```bash
# Problem: Property tests occasionally fail
# Solution: Increase shrinking iterations
export PROPTEST_MAX_SHRINK_ITERS=5000

# Or add timeout
export PROPTEST_TIMEOUT=30000  # 30 seconds
```

### Debugging Property Test Failures

1. **Check regression files**: Failed cases are saved automatically
2. **Add debug prints**: Use `println!` in test bodies (with `--nocapture`)
3. **Simplify strategies**: Temporarily reduce complexity to isolate issues
4. **Use shrinking**: Proptest finds minimal failing examples

```bash
# Enable verbose output
PROPTEST_VERBOSE=1 cargo test failing_prop_test -- --nocapture

# Run with fewer cases to isolate issue
PROPTEST_CASES=10 cargo test failing_prop_test -- --nocapture

# Check regression files
ls -la crates/*/proptest-regressions/
```

### Move/Borrow Issues in Property Tests

**Problem**: Values moved in assertions

```rust
prop_assert_eq!(value.field, other); // value moved here
prop_assert!(value.is_valid()); // Error: value used after move
```

**Solution**: Clone values when needed

```rust
prop_assert_eq!(value.field.clone(), other);
prop_assert!(value.is_valid());
```

## Contributing Guidelines

When adding new property tests to HPM:

1. **Add tests for new business logic**: Any new validation or transformation logic should have property tests
2. **Use existing strategy patterns**: Follow established patterns for consistency
3. **Document custom strategies**: Add clear comments explaining strategy purposes
4. **Test edge cases**: Include tests for error conditions and boundary cases
5. **Commit regression files**: Always include generated regression test files

### Review Checklist

- [ ] Property tests cover the main business logic invariants
- [ ] Custom strategies generate realistic, constrained inputs
- [ ] Tests include both positive and negative cases
- [ ] Property assertions are clear and specific
- [ ] Regression files are included in commits
- [ ] Tests run efficiently (not too slow)

## Development Workflow

### Daily Development

1. **Write code** with property tests for new business logic
2. **Run quick tests** during development: `PROPTEST_CASES=100 cargo test`
3. **Full test run** before committing: `cargo test --workspace`
4. **Commit regression files** when property tests find new edge cases

### Code Review

1. **Review property test coverage** for new features
2. **Check regression files** for unexpected changes
3. **Validate test strategy design** follows established patterns
4. **Ensure comprehensive coverage** of business logic invariants

### Release Process

1. **Comprehensive test run**: `PROPTEST_CASES=1000 cargo test --workspace`
2. **Regression verification**: Ensure all regression tests pass
3. **Documentation update**: Update testing guides if needed

## Test Coverage Tracking

```bash
# Install coverage tools
cargo install cargo-tarpaulin

# Run coverage with property tests
PROPTEST_CASES=500 cargo tarpaulin --workspace --all-features --out html

# Property test specific coverage
PROPTEST_CASES=1000 cargo tarpaulin --workspace --all-features \
  --run-types tests --engine llvm --out html -- prop_
```

## Resources

- [Proptest Documentation](https://docs.rs/proptest/)
- [Property-Based Testing Concepts](https://hypothesis.works/articles/what-is-property-based-testing/)

## Conclusion

HPM's comprehensive testing strategy combines property-based testing with traditional unit and integration tests to ensure high code quality and reliability. Property-based testing has already proven its value by discovering real bugs and providing comprehensive test coverage.

The investment in property-based testing pays dividends through:
- **Early bug detection** before code reaches production
- **Increased confidence** in refactoring and changes
- **Better documentation** of business logic through executable specifications
- **Comprehensive edge case coverage** that traditional tests might miss
