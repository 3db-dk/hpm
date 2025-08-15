# Property-Based Testing Guide for HPM

This guide provides comprehensive information about property-based testing in HPM, covering concepts, implementation patterns, and best practices for contributors.

## Table of Contents

- [Overview](#overview)
- [Why Property-Based Testing](#why-property-based-testing)
- [HPM's Property Testing Architecture](#hpms-property-testing-architecture)
- [Writing Property-Based Tests](#writing-property-based-tests)
- [Testing Strategies and Patterns](#testing-strategies-and-patterns)
- [Best Practices](#best-practices)
- [Examples](#examples)
- [Troubleshooting](#troubleshooting)

## Overview

HPM uses [proptest](https://crates.io/crates/proptest), a Rust property-based testing framework that automatically generates test inputs to verify that invariant properties hold across a wide range of scenarios.

Property-based testing complements traditional unit tests by:
- **Automatically discovering edge cases** that developers might not consider
- **Testing business logic invariants** that should always hold
- **Providing better test coverage** through random input generation
- **Finding bugs early** in the development cycle

## Why Property-Based Testing

### Problems with Traditional Unit Testing

```rust
// Traditional unit test - limited scope
#[test]
fn test_version_parsing() {
    assert_eq!("1.0.0".parse::<Version>().unwrap().to_string(), "1.0.0");
    assert_eq!("2.1.3".parse::<Version>().unwrap().to_string(), "2.1.3");
    // What about edge cases? Unicode? Very long strings? Invalid formats?
}
```

### Property-Based Testing Advantages

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

### Real Bug Discovery in HPM

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

## HPM's Property Testing Architecture

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

## Writing Property-Based Tests

### 1. Define Custom Strategies

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

### 2. Write Property Tests

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

### 3. Test Categories

#### Roundtrip Testing
Verify that operations are reversible:

```rust
#[test]
fn prop_manifest_serialization_roundtrip(manifest in package_manifest_strategy()) {
    let toml_str = toml::to_string(&manifest).unwrap();
    let deserialized: PackageManifest = toml::from_str(&toml_str).unwrap();
    
    prop_assert_eq!(manifest.package.name, deserialized.package.name);
    prop_assert_eq!(manifest.package.version, deserialized.package.version);
}
```

#### Invariant Testing
Verify that business rules always hold:

```rust
#[test]
fn prop_valid_manifests_pass_validation(manifest in package_manifest_strategy()) {
    prop_assert!(manifest.validate().is_ok());
}
```

#### Consistency Testing
Verify that operations produce consistent results:

```rust
#[test]
fn prop_resolved_set_hash_consistency(resolved in resolved_dependency_set_strategy()) {
    let hash1 = resolved.hash();
    let hash2 = resolved.hash();
    prop_assert_eq!(hash1, hash2, "Hash should be consistent");
}
```

## Testing Strategies and Patterns

### Common Strategy Patterns

#### 1. Constrained Random Generation
```rust
fn python_version_strategy() -> impl Strategy<Value = PythonVersion> {
    (2u8..=3, 6u8..=12, prop::option::of(0u8..=20))
        .prop_map(|(major, minor, patch)| PythonVersion::new(major, minor, patch))
}
```

#### 2. Enum and Union Types
```rust
fn dependency_spec_strategy() -> impl Strategy<Value = DependencySpec> {
    prop_oneof![
        version_req_strategy().prop_map(DependencySpec::Simple),
        // ... detailed variant generation
    ]
}
```

#### 3. Complex Structure Generation
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

### Advanced Patterns

#### Conditional Testing
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

#### Error Condition Testing
```rust
#[test]
fn prop_version_req_invalid(whitespace in r"\s*") {
    let result = VersionReq::new(&whitespace);
    prop_assert!(result.is_err());
}
```

## Best Practices

### 1. Strategy Design

**✅ Good**:
```rust
fn package_name_strategy() -> impl Strategy<Value = String> {
    prop::string::string_regex("[a-z][a-z0-9-]{1,50}")
        .unwrap()
        .prop_filter("Valid package name", |name| {
            !name.ends_with('-') && name.len() >= 2
        })
}
```

**❌ Avoid**:
```rust
// Too broad - generates mostly invalid inputs
fn bad_name_strategy() -> impl Strategy<Value = String> {
    any::<String>() // Will generate Unicode, empty strings, etc.
}
```

### 2. Property Assertions

**✅ Good**:
```rust
// Clear, specific assertions
prop_assert_eq!(result.name, expected_name);
prop_assert!(result.is_valid(), "Result should be valid");
```

**❌ Avoid**:
```rust
// Vague assertions
prop_assert!(something_happened); // What happened?
```

### 3. Test Organization

**✅ Good Structure**:
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

### 4. Performance Considerations

- **Default test count**: Proptest runs 256 tests by default
- **Configure test count**: Use environment variables or test attributes
- **Strategy efficiency**: Avoid expensive filters when possible

```rust
// Efficient strategy
(0u32..100).prop_map(|n| format!("package-{}", n))

// Less efficient - many rejections
any::<String>().prop_filter("valid", |s| s.starts_with("package-"))
```

### 5. Regression Testing

Proptest automatically saves failing test cases to regression files:

```
crates/hpm-core/proptest-regressions/storage/types.txt
```

**Always commit regression files** - they ensure previously found bugs don't reappear.

## Examples

### Basic Property Test

```rust
proptest! {
    #[test]
    fn prop_python_version_roundtrip(version in python_version_strategy()) {
        let version_str = version.to_string();
        let parsed: PythonVersion = version_str.parse().unwrap();
        prop_assert_eq!(version, parsed);
    }
}
```

### Complex Business Logic Test

```rust
proptest! {
    #[test]
    fn prop_houdini_package_generation(manifest in package_manifest_strategy()) {
        let houdini_pkg = manifest.generate_houdini_package();
        
        // Generated package should always have required fields
        prop_assert!(houdini_pkg.hpath.is_some());
        prop_assert!(houdini_pkg.env.is_some());
        
        // hpath should contain otls directory
        let hpath = houdini_pkg.hpath.unwrap();
        prop_assert!(hpath.iter().any(|path| path.contains("otls")));
    }
}
```

### Error Handling Test

```rust
proptest! {
    #[test]
    fn prop_package_spec_invalid_input(invalid_input in r"[^a-zA-Z0-9@.\-_]{1,10}") {
        // Skip inputs that might accidentally be valid
        if invalid_input.chars().all(|c| c.is_alphanumeric() || c == '-') {
            return Ok(());
        }
        
        let result = PackageSpec::parse(&invalid_input);
        // Should either parse successfully or fail gracefully
        if let Ok(spec) = result {
            prop_assert!(!spec.name.is_empty());
        }
    }
}
```

## Troubleshooting

### Common Issues

#### 1. Test Failures Due to Invalid Inputs

**Problem**: Strategy generates invalid inputs that cause panics

**Solution**: Use `prop_filter` or adjust regex patterns

```rust
// Before: generates invalid names
any::<String>()

// After: constrained generation
prop::string::string_regex("[a-z][a-z0-9-]{1,50}")
    .unwrap()
    .prop_filter("Valid name", |name| !name.ends_with('-'))
```

#### 2. Move/Borrow Issues in Property Tests

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

#### 3. Slow Test Execution

**Problem**: Tests take too long due to complex strategies

**Solutions**:
- Reduce test count: `PROPTEST_CASES=100 cargo test`
- Simplify strategies: avoid expensive filters
- Use more targeted generation

### Debugging Property Test Failures

1. **Check regression files**: Failed cases are saved automatically
2. **Add debug prints**: Use `println!` in test bodies (with `--nocapture`)
3. **Simplify strategies**: Temporarily reduce complexity to isolate issues
4. **Use shrinking**: Proptest finds minimal failing examples

### Integration with CI

Property tests run in CI with default settings. For longer runs:

```yaml
# In CI configuration
env:
  PROPTEST_CASES: 1000  # More thorough testing
```

## Running Property Tests

### Local Development
```bash
# Run all property tests
cargo test prop_

# Run specific crate property tests
cargo test -p hpm-core prop_

# Run with more test cases
PROPTEST_CASES=1000 cargo test prop_

# Debug failing tests
cargo test prop_test_name -- --nocapture
```

### Integration with HPM Development Workflow

Property tests are integrated into HPM's quality gates:
- **Pre-commit hooks**: Run property tests before commits
- **CI pipeline**: Extended property test runs
- **Release testing**: Comprehensive property test execution

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

## Resources

- [Proptest Documentation](https://docs.rs/proptest/)
- [Property-Based Testing Concepts](https://hypothesis.works/articles/what-is-property-based-testing/)
- [HPM Testing Architecture](../CLAUDE.md#testing-framework)

## Conclusion

Property-based testing is a powerful technique that has already proven its value in HPM by discovering real bugs and providing comprehensive test coverage. By following these guidelines and patterns, contributors can write effective property tests that improve code quality and reliability.

The investment in property-based testing pays dividends through:
- **Early bug detection** before code reaches production
- **Increased confidence** in refactoring and changes  
- **Better documentation** of business logic through executable specifications
- **Comprehensive edge case coverage** that traditional tests might miss

Continue building upon HPM's property testing foundation to maintain high code quality and reliability.