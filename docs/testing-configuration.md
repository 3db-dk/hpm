# Testing Configuration for HPM

This document describes the comprehensive testing setup for HPM, including property-based testing integration, CI/CD configuration, and development workflow integration.

## Testing Architecture Overview

HPM employs a multi-tiered testing strategy:

```
┌─────────────────────────────────────────────────────────────┐
│                     HPM Testing Strategy                     │
├─────────────────────────────────────────────────────────────┤
│ 1. Property-Based Tests (proptest)                         │
│    - Automated edge case discovery                         │
│    - Business logic invariant verification                 │
│    - Regression prevention                                 │
├─────────────────────────────────────────────────────────────┤
│ 2. Traditional Unit Tests                                  │
│    - Specific edge cases                                   │
│    - Known bug reproduction                                │
│    - API contract verification                             │
├─────────────────────────────────────────────────────────────┤
│ 3. Integration Tests                                       │
│    - End-to-end workflows                                  │
│    - CLI command testing                                   │
│    - Cross-module functionality                            │
├─────────────────────────────────────────────────────────────┤
│ 4. Performance Tests                                       │
│    - Build time validation                                 │
│    - Memory usage verification                             │
│    - Dependency analysis                                   │
└─────────────────────────────────────────────────────────────┘
```

## Test Distribution

| Test Type | Count | Coverage |
|-----------|-------|----------|
| Property-based Tests | 23 | Core business logic invariants |
| Traditional Unit Tests | 150+ | Specific scenarios and edge cases |
| Integration Tests | 25+ | End-to-end workflows |
| Doc Tests | 15+ | API documentation examples |

## Property-Based Testing Configuration

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

### Development Workflow Integration

#### Local Development
```bash
# Quick property test run (default settings)
cargo test prop_

# Thorough property test run
PROPTEST_CASES=1000 cargo test prop_

# Debug specific property test
PROPTEST_VERBOSE=1 cargo test prop_version_roundtrip -- --nocapture

# Run property tests for specific crate
cargo test -p hpm-core prop_
```

#### Pre-commit Integration
```bash
# Included in pre-commit hooks
cargo test --workspace --all-features -- --test-threads=1
```

### CI/CD Configuration

#### GitHub Actions Configuration

```yaml
# .github/workflows/test.yml
name: Comprehensive Testing

on: [push, pull_request]

env:
  CARGO_TERM_COLOR: always
  # Property testing configuration
  PROPTEST_CASES: 512  # Balanced between coverage and speed
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
        run: |
          echo "Running property-based tests with $PROPTEST_CASES cases"
          cargo test prop_ --workspace --all-features -- --nocapture

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
      
      - name: Run tests with ${{ matrix.test-config.cases }} property test cases
        env:
          PROPTEST_CASES: ${{ matrix.test-config.cases }}
        run: |
          cargo test --workspace --all-features
          
  regression-tests:
    name: Regression Tests
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      
      - name: Verify regression files exist
        run: |
          find . -name "proptest-regressions" -type d | while read dir; do
            if [ -z "$(ls -A "$dir" 2>/dev/null)" ]; then
              echo "Warning: Empty regression directory: $dir"
            else
              echo "Found regression files in: $dir"
              ls -la "$dir"
            fi
          done
      
      - name: Run regression tests
        run: cargo test --workspace --all-features
```

#### Pre-commit Hook Configuration

```yaml
# .pre-commit-config.yaml additions
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

      - id: regression-tests
        name: Regression test verification
        entry: bash
        args:
          - -c
          - |
            # Ensure regression files are committed
            git diff --cached --name-only | grep -E "proptest-regressions.*\.txt$" || {
              echo "No regression files staged. Checking if any exist..."
              find . -name "*.txt" -path "*/proptest-regressions/*" -newer .git/HEAD 2>/dev/null && {
                echo "Error: New regression files found but not staged."
                echo "Please add them to your commit:"
                find . -name "*.txt" -path "*/proptest-regressions/*" -newer .git/HEAD 2>/dev/null
                exit 1
              } || echo "No new regression files found."
            }
        language: system
        types: [rust]
        pass_filenames: false
```

### Makefile Integration

```makefile
# Test targets with property testing support

# Quick tests (development)
.PHONY: test-quick
test-quick:
	PROPTEST_CASES=100 cargo test --workspace --all-features

# Standard tests (CI default)  
.PHONY: test
test:
	PROPTEST_CASES=256 cargo test --workspace --all-features

# Thorough tests (nightly CI)
.PHONY: test-thorough  
test-thorough:
	PROPTEST_CASES=1000 cargo test --workspace --all-features

# Property tests only
.PHONY: test-property
test-property:
	PROPTEST_CASES=500 cargo test prop_ --workspace --all-features -- --nocapture

# Property tests with verbose output
.PHONY: test-property-verbose
test-property-verbose:
	PROPTEST_VERBOSE=1 PROPTEST_CASES=100 cargo test prop_ --workspace --all-features -- --nocapture

# Regression tests
.PHONY: test-regression
test-regression:
	@echo "Checking for regression files..."
	@find . -name "proptest-regressions" -type d -exec echo "Found regression directory: {}" \;
	cargo test --workspace --all-features

# Performance benchmarking with property tests
.PHONY: bench-property
bench-property:
	@echo "Benchmarking property test performance..."
	@time PROPTEST_CASES=1000 cargo test prop_ --workspace --all-features --release
```

## Test Performance Optimization

### Build Time Optimization

```toml
# Cargo.toml test profile optimization
[profile.test]
debug = 1  # Reduced debug info for faster builds
incremental = true
codegen-units = 16  # Parallel codegen
```

### Parallel Test Execution

```bash
# Parallel execution (default)
cargo test --workspace --all-features

# Sequential execution (when needed for file system tests)
cargo test --workspace --all-features -- --test-threads=1

# Per-crate parallel execution
cargo test -p hpm-core & \
cargo test -p hpm-package & \
cargo test -p hpm-python & \
wait
```

### Memory Usage Optimization

```bash
# Monitor memory usage during tests
/usr/bin/time -v cargo test prop_ --workspace --all-features

# Limit memory usage (if needed)
ulimit -v 4194304  # 4GB virtual memory limit
cargo test prop_ --workspace --all-features
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
  
  // Property testing specific
  "rust-analyzer.cargo.extraEnv": {
    "PROPTEST_CASES": "100",  // Faster IDE testing
    "PROPTEST_VERBOSE": "0"
  },
  
  // Test discovery
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
      "presentation": {
        "echo": true,
        "reveal": "always",
        "focus": false,
        "panel": "shared",
        "showReuseMessage": true,
        "clear": false
      },
      "problemMatcher": "$rustc"
    },
    {
      "label": "test-property-verbose",
      "type": "shell",
      "command": "cargo", 
      "args": ["test", "prop_", "--workspace", "--all-features", "--", "--nocapture"],
      "env": {
        "PROPTEST_CASES": "50",
        "PROPTEST_VERBOSE": "1"
      },
      "group": "test"
    }
  ]
}
```

## Monitoring and Metrics

### Test Coverage Tracking

```bash
# Install coverage tools
cargo install cargo-tarpaulin

# Run coverage with property tests
PROPTEST_CASES=500 cargo tarpaulin --workspace --all-features --out html

# Property test specific coverage
PROPTEST_CASES=1000 cargo tarpaulin --workspace --all-features \
  --run-types tests --engine llvm --out html \
  -- prop_
```

### Performance Metrics

```bash
# Track test execution time
hyperfine 'PROPTEST_CASES=256 cargo test prop_ --workspace --all-features'

# Memory usage profiling
valgrind --tool=massif cargo test prop_ --workspace --all-features
```

### Quality Metrics

```bash
# Property test distribution analysis
cargo test prop_ --workspace --all-features -- --list | \
  awk '/prop_/ {count++} END {print "Property tests:", count}'

# Traditional test distribution
cargo test --workspace --all-features -- --list | \
  awk '!/prop_/ && /test/ {count++} END {print "Traditional tests:", count}'
```

## Troubleshooting

### Common Configuration Issues

#### Slow Test Execution
```bash
# Problem: Tests taking too long
# Solution: Reduce PROPTEST_CASES for development
export PROPTEST_CASES=50

# Or use quick test target
make test-quick
```

#### Memory Issues
```bash
# Problem: Out of memory during property tests
# Solution: Run tests sequentially  
cargo test prop_ --workspace --all-features -- --test-threads=1

# Or limit memory usage
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

```bash
# Enable verbose output
PROPTEST_VERBOSE=1 cargo test failing_prop_test -- --nocapture

# Run with fewer cases to isolate issue
PROPTEST_CASES=10 cargo test failing_prop_test -- --nocapture  

# Check regression files
ls -la crates/*/proptest-regressions/
cat crates/hpm-core/proptest-regressions/storage/types.txt
```

## Development Workflow

### Daily Development

1. **Write code** with property tests for new business logic
2. **Run quick tests** during development: `make test-quick`
3. **Full test run** before committing: `make test`
4. **Commit regression files** when property tests find new edge cases

### Code Review

1. **Review property test coverage** for new features
2. **Check regression files** for unexpected changes
3. **Validate test strategy design** follows established patterns
4. **Ensure comprehensive coverage** of business logic invariants

### Release Process

1. **Comprehensive test run**: `make test-thorough`
2. **Performance validation**: `make bench-property` 
3. **Regression verification**: `make test-regression`
4. **Documentation update**: Update testing guides if needed

## Future Enhancements

### Planned Improvements

1. **Fuzz Testing Integration**: Combine property tests with fuzzing
2. **Performance Property Tests**: Add performance invariant testing  
3. **Cross-platform Testing**: Extended CI matrix for property tests
4. **Custom Shrinking**: Domain-specific shrinking strategies

### Monitoring Dashboard

Future integration with testing dashboards to track:
- Property test coverage trends
- Regression file growth
- Test execution time trends
- Bug discovery rate through property testing

## Conclusion

This comprehensive testing configuration ensures HPM maintains high quality through:

- **Automated edge case discovery** via property-based testing
- **Continuous integration** with appropriate test coverage
- **Developer-friendly workflows** for local development
- **Performance optimization** for efficient testing cycles
- **Comprehensive monitoring** of test quality and coverage

The configuration balances thorough testing with development velocity, providing confidence in code quality while maintaining fast feedback loops for developers.