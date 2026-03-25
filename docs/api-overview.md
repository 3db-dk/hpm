# HPM API Overview

This document provides a high-level overview of HPM's crate structure and key public types. For full API documentation with function signatures, examples, and detailed type information, generate rustdoc locally:

```bash
cargo doc --workspace --all-features --no-deps --open
```

## Crate Structure

```text
┌──────────────┐
│   hpm-cli    │  Command-line interface (clap)
└──────┬───────┘
       │
┌──────▼───────┐
│   hpm-core   │  Storage, discovery, dependency analysis, cleanup
├──────────────┤
│  hpm-package │  Manifest processing, templates, Houdini integration
├──────────────┤
│  hpm-python  │  Python venv management, UV integration
├──────────────┤
│ hpm-resolver │  PubGrub dependency resolution engine
└──────┬───────┘
       │
┌──────▼───────┐
│  hpm-config  │  Configuration management
├──────────────┤
│  hpm-error   │  Structured error types
└──────────────┘
```

## Key Types by Crate

### hpm-core

| Type | Purpose |
|------|---------|
| `StorageManager` | Global package storage operations (install, remove, list, cleanup) |
| `ProjectDiscovery` | Filesystem scanning to find HPM-managed projects |
| `GlobalDependencyGraph` | Dependency graph for orphan detection during cleanup |
| `PackageManager` | High-level orchestration of package operations |

### hpm-package

| Type | Purpose |
|------|---------|
| `PackageManifest` | Parsed `hpm.toml` with all sections (package, houdini, dependencies, env, etc.) |
| `PackageTemplate` | Standard and bare package scaffolding |
| `Platform` | Target platform enum (linux-x86_64, windows-x86_64, macos-x86_64, macos-aarch64) |
| `NativeConfig` | Platform-specific file declarations for `[native]` builds |

### hpm-python

| Type | Purpose |
|------|---------|
| `VenvManager` | Content-addressable virtual environment creation and sharing |
| `PythonVersion` | Houdini-to-Python version mapping |
| `ResolvedDependencies` | UV-resolved dependency set with content hash |

### hpm-resolver

| Type | Purpose |
|------|---------|
| `Resolver` | PubGrub-inspired incremental dependency solver |
| `VersionRequirement` | Version constraint types (exact, caret, tilde, range, union) |
| `ResolutionState` | In-progress resolution with conflict learning |

### hpm-config

| Type | Purpose |
|------|---------|
| `Config` | Hierarchical configuration (defaults < global < project < env < CLI) |
| `StorageConfig` | Package storage paths and settings |
| `ProjectsConfig` | Project discovery paths and search roots |

### hpm-error

| Type | Purpose |
|------|---------|
| `HpmError` | Top-level error enum with structured variants |
| Exit codes | 0 = success, 1 = user error, 2 = internal error |

## Design Principles

- **Minimal coupling**: Each crate has a single responsibility; dependencies flow downward
- **Async by default**: Core operations use async/await with Tokio
- **Content-addressable storage**: Both packages and Python venvs use hash-based deduplication
- **Safety guarantees**: Cleanup never removes packages needed by active projects
