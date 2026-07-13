# Testing Guide

HPM uses several layers of tests: traditional unit tests, integration
tests, property-based tests with
[proptest](https://crates.io/crates/proptest), and a Houdini conformance
test that checks generated package files against a real Houdini. This
guide covers how to run them, how to write new ones, and how to debug
failures.

## Table of contents

- [Test layers](#test-layers)
- [Running tests](#running-tests)
- [Configuration](#configuration)
- [Writing property tests](#writing-property-tests)
- [Regression files](#regression-files)
- [Debugging failures](#debugging-failures)

## Test layers

| Layer | Tools | Purpose |
|-------|-------|---------|
| **Unit** | built-in `#[test]` | Exercise a specific function or edge case. |
| **Integration** | `#[tokio::test]` + `tempfile` | End-to-end workflows: CLI command invocation, filesystem layout, manifest roundtrips. |
| **Property** | [proptest](https://proptest-rs.github.io/proptest/) | Generate inputs at random and assert invariants. Catches edge cases a human wouldn't think of. |
| **Doc** | rustdoc examples | Keep public API snippets compiling. |
| **Houdini conformance** | `hconfig` from a real Houdini install | Assert the values Houdini actually resolves from generated package files — the only layer that can catch wrong assumptions about Houdini's semantics. |

### Houdini conformance test

`hpm-core/src/houdini_conformance_tests.rs` writes package files through
the real emission path, runs `HOUDINI_PACKAGE_VERBOSE=1 $HFS/bin/hconfig`
(license-free), and asserts the merged env values from the verbose
package log. It exists because the emission layer was once validated only
against its own JSON output while Houdini silently ignored `method` on
flat-string custom variables.

- The Houdini install is auto-discovered: `$HFS` first, then the
  platform-standard locations (`/opt/hfs*`, `/Applications/Houdini/...`,
  `C:\Houdini *`).
- Without an install the test **skips** (passing, with a `SKIPPED` note).
  Set `HPM_REQUIRE_HOUDINI=1` to turn the skip into a failure — the CI
  check pipeline does, since the workers all have Houdini.

The semantics the emission layer targets are also captured as an
executable model (`hpm-core/src/houdini_env_model.rs`); a property test
(`houdini_emission_model_tests.rs`) runs randomized packages and project
overrides through the real emission code and the model, asserting that
package values survive overrides, overrides apply exactly once, and
nothing emitted uses a method or value shape Houdini rejects. When a new
question about Houdini's package semantics comes up, extend the
conformance test to settle it empirically, then encode the answer in the
model.

### Property test distribution

Property tests are concentrated in the crates with the most value-shaped
logic: manifest parsing, Python version handling, storage types. Exact
counts shift over time — run
`grep -rh "fn prop_" crates/*/src crates/*/tests | wc -l` for the
current number (the manifest strategies live in
`crates/hpm-package/tests/properties.rs` and the CLI strategies in
`crates/hpm-cli/tests/cli_validation.rs`).

| Crate | Focus |
|-------|-------|
| `hpm-cli` | Argument parsing, output format round-trips (in `tests/cli_validation.rs`). |
| `hpm-core` | Storage types, package specs, lockfile round-trips, env merge contracts, and the Houdini env emission model (`houdini_emission_model_tests.rs`). The `python` submodule covers Python versions, dependency resolution, content hashing. |
| `hpm-package` | Manifest validation, TOML round-trips, native configs (in `tests/properties.rs`). |

## Running tests

```sh
# everything: cli (single-threaded), rest of workspace, then doctests
just test

# doctests only — public-API examples in //! and /// blocks
just test-doc

# slow / external-dependency tests gated behind `#[ignore]`
# (currently: real-uv venv smoke tests in hpm-core::python)
just test-ignored

# raw cargo equivalents
cargo test --workspace
cargo test --workspace --doc

# one crate
cargo test -p hpm-core

# one test by name
cargo test prop_version_req_roundtrip

# property tests only
cargo test prop_

# sequential execution (required when tests touch shared filesystem paths;
# `just test` already does this for hpm-cli via the CwdGuard)
cargo test --workspace -- --test-threads=1

# more proptest cases (default 256)
PROPTEST_CASES=1000 cargo test prop_ --workspace
```

## Configuration

### Proptest environment variables

| Variable | Default | Purpose |
|----------|---------|---------|
| `PROPTEST_CASES` | 256 | Number of cases per property test. |
| `PROPTEST_MAX_SHRINK_ITERS` | 1024 | Shrinking budget on failure. |
| `PROPTEST_TIMEOUT` | — | Per-case timeout (ms). |
| `PROPTEST_VERBOSE` | 0 | Dump generated inputs as they run. |

### CI matrix

HPM's CI runs:

- `PROPTEST_CASES=256` on every push/PR (standard).
- `PROPTEST_CASES=2000` nightly and as a pre-release check (thorough).

```sh
PROPTEST_CASES=2000 cargo test --workspace --all-features
```

## Writing property tests

A property test defines a *strategy* for generating inputs and an invariant
that must hold for every generated input.

### Custom strategies

Constrain generators to the valid input space. Unconstrained `any::<String>()`
mostly generates nonsense and rejects it, which wastes cycles and produces
misleading shrinks.

```rust
use proptest::prelude::*;

// Good — produces valid package slugs by construction
fn slug_strategy() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9-]{1,20}"
        .prop_map(|s| s.trim_end_matches('-').to_string())
        .prop_filter("non-empty", |s| !s.is_empty())
}

fn version_strategy() -> impl Strategy<Value = String> {
    (0u32..100, 0u32..100, 0u32..100)
        .prop_map(|(maj, min, pat)| format!("{maj}.{min}.{pat}"))
}
```

### Invariants worth testing

**Roundtrip** — serialize and deserialize and compare:

```rust
proptest! {
    #[test]
    fn prop_manifest_toml_roundtrip(manifest in manifest_strategy()) {
        let toml_str = toml::to_string(&manifest).unwrap();
        let parsed: PackageManifest = toml::from_str(&toml_str).unwrap();
        prop_assert_eq!(manifest.package.path, parsed.package.path);
        prop_assert_eq!(manifest.package.version, parsed.package.version);
    }
}
```

**Validation always holds** — every value the strategy produces should pass
validation:

```rust
proptest! {
    #[test]
    fn prop_valid_manifests_validate(manifest in valid_manifest_strategy()) {
        prop_assert!(manifest.validate().is_ok());
    }
}
```

**Determinism** — running the same operation twice should produce the same
result:

```rust
proptest! {
    #[test]
    fn prop_content_hash_is_deterministic(set in resolved_set_strategy()) {
        prop_assert_eq!(set.hash(), set.hash());
    }
}
```

### Real bug caught by property tests

`VersionReq::new("   ")` (whitespace-only) was incorrectly accepted as valid.
A property test that fed `r"\s*"` into the constructor surfaced it; the fix
was to trim before the empty check. Every such bug gets a regression file
that fails until the fix stays in place.

## Regression files

Proptest persists failing cases to `crates/<crate>/proptest-regressions/`.
These are source-of-truth regression tests — commit them alongside the fix.

```
crates/hpm-package/proptest-regressions/
└── manifest.txt             # Each line is a minimized failing input.
```

Never delete these by hand unless you're *certain* the bug class is gone and
the input is no longer meaningful.

## Debugging failures

### Get a minimal failing input

Proptest automatically shrinks to the smallest input that still fails. To
see the shrink trace:

```sh
PROPTEST_VERBOSE=1 cargo test failing_prop_test -- --nocapture
```

### Reduce the case count while iterating

```sh
PROPTEST_CASES=10 cargo test failing_prop_test -- --nocapture
```

### Inspect the regression file

```sh
cat crates/hpm-package/proptest-regressions/manifest.txt
```

Each line is a serialized form of the seed that reproduced the bug. Proptest
replays these on every subsequent run, so the same minimal case is checked
deterministically until you fix it.

### Move/borrow issues in assertions

`prop_assert_eq!` moves its operands. If you want to assert on a field and
then on a predicate, clone:

```rust
prop_assert_eq!(value.field.clone(), expected);
prop_assert!(value.is_valid());
```

## Running coverage

```sh
cargo install cargo-tarpaulin
PROPTEST_CASES=500 cargo tarpaulin --workspace --out html
```

## Resources

- [Proptest book](https://proptest-rs.github.io/proptest/)
- [Hypothesis: what is property-based testing?](https://hypothesis.works/articles/what-is-property-based-testing/)
