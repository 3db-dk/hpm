# API Overview

This document maps HPM's crate layout to its key public types. For full API
docs with signatures and examples, generate rustdoc locally:

```sh
cargo doc --workspace --no-deps --open
```

## Crate graph

```text
┌──────────────┐
│   hpm-cli    │   Command-line frontend (clap). Binary.
└──────┬───────┘
       │ depends on everything below
┌──────▼───────┐
│   hpm-core   │   Storage, discovery, lock file, registry trait, archive I/O
├──────────────┤
│  hpm-package │   Manifest parsing, Houdini integration, dependency types
├──────────────┤
│  hpm-python  │   Venv management, bundled uv, Houdini→Python mapping
├──────────────┤
│ hpm-resolver │   PubGrub-style solver
└──────┬───────┘
       │
┌──────▼───────┐
│  hpm-config  │   Configuration loading and merging
└──────────────┘
```

Each crate defines its own error type via `thiserror` (e.g. `StorageError`,
`ResolverError`, `ConfigError`). `hpm-cli` converts these into a single
`CliError` with exit codes and help hints.

## Key types by crate

### hpm-core

| Type | Purpose |
|------|---------|
| `StorageManager` | Global package install/remove/list. Backed by `~/.hpm/packages/`. |
| `ProjectDiscovery` | Scans `[projects]` paths to enumerate active HPM projects. |
| `GlobalDependencyGraph` | Reachability-based orphan detection for `hpm clean`. |
| `LockFile`, `LockedDependency`, `LockedPythonDependency`, `LockMetadata` | `hpm.lock` types. |
| `LockedSource` | Origin recorded in the lockfile for each dep — `Url { url, version }` or `Path { path }`. |
| `LockError` | `Read`/`Parse`/`Write`/`Serialize` plus `ChecksumMismatch`, `PackageMissing { package, expected_dir }`, and `UnsupportedVersion`. |
| `PackageSource` | URL-only struct `{ url, version }` — what `ArchiveFetcher` downloads. Path deps bypass the fetcher and use `LockedSource::Path` in the lockfile. |
| `cas_install_dir(packages_dir, name, version)` | Canonical install path `<packages_dir>/<slug>@<version>` for a lockfile dep name. Used by `LockFile::verify_checksums` and any consumer that needs to find an installed package off the lockfile alone. |
| `fetcher_install_dir(packages_dir, name, version)` | Staging path `<packages_dir>/<safe_name>-<version>` used by `ArchiveFetcher` while extracting; the result is then copied into the canonical CAS via `install_from_path`. |
| `Registry` (async trait) | Registry abstraction. `ApiRegistry` and `GitRegistry` implement it. |
| `RegistrySet` | Composite that fans requests out to every configured registry. |
| `ArchiveFetcher` | Downloads and extracts registry-hosted archives. |
| `fetch_manifest` | Free function: returns the parsed `PackageManifest` for `(name, version)` without project context. CAS hit reads from disk; CAS miss resolves+fetches+installs. Pass `""` or `"latest"` to pick the highest semver. |
| `packer` | Produces signed/unsigned `.zip` archives for `hpm pack`. |

### hpm-package

| Type | Purpose |
|------|---------|
| `PackageManifest` | Parsed `hpm.toml` (every section). `PackageManifest::from_path` reads + parses, returning `ManifestLoadError` with the offending path. |
| `ManifestLoadError` | `NotFound { path }` / `Read { path, source }` / `Parse { path, source }`. Re-exported by `hpm-core`'s `StorageError`, `ProjectError`, `DiscoveryError`, and `FetchManifestError`. |
| `PackagePath`, `PackagePathError` | Validated `creator/slug` newtype. Kebab-case enforced at deserialization, so `creator()` and `slug()` return `&str` — no `Option`. |
| `PackageInfo` | Contents of `[package]`. `path: PackagePath` is the canonical identifier; `name`, `version`, etc. are user-facing metadata. |
| `HoudiniConfig` | Contents of `[houdini]`. |
| `DependencySpec` | Untagged enum: `Simple(String) \| Url {..} \| Path {..} \| Registry {..}`. |
| `PythonDependencySpec` | Untagged enum: `Simple(String) \| Detailed {..}`. |
| `ManifestEnvEntry`, `EnvMethod` | `[env]` entries (method, optional value, `required` flag) and methods (`set`/`prepend`/`append`). `value` is `Option<EnvValueSpec>` — flat string or ordered variant list. `lower(substitutions)` is the single emit path. |
| `EnvValueSpec`, `EnvValueVariant`, `WhenSelector` | Conditional `[env]` value support. Variants compile to Houdini's expression-object array via `compile_when` / `lower_conditional`. Axes: `houdini` (Cargo-style req), `os`, `python`. |
| `NativeConfig`, `NativePlatformFiles` | `[native]` and per-platform file globs. |
| `Platform` | Canonical platform enum: `LinuxX86_64`, `MacosUniversal`, `WindowsX86_64`. |
| `RegistryConfig`, `RegistryType` | `[[registries]]` entries in manifests. |
| `PackageTemplate` | Scaffolding for `hpm init` (standard and `--bare`). |
| `HoudiniPackage`, `HoudiniNativePackage`, `HoudiniEnvValue` | Houdini `package.json` output types. |

### hpm-python

| Type | Purpose |
|------|---------|
| `VenvManager` | Creates and reuses content-addressable venvs. |
| `PythonVersion` | Houdini-to-Python version value. |
| `ResolvedDependencies`, `ResolvedDependencySet`, `ResolvedPackage` | UV-resolved dependency sets with content hash. |
| `PythonCleanupAnalyzer` | Detects orphan venvs for `hpm clean --python-only`. |

### hpm-resolver

| Type | Purpose |
|------|---------|
| `Resolver` | PubGrub-style incremental solver with conflict learning. |
| `VersionReq` | Parsed version requirement. |
| `ResolutionState`, `DecisionPoint` | Solver state for backtracking. |

### hpm-config

| Type | Purpose |
|------|---------|
| `Config` | Top-level config (defaults < `~/.hpm/config.toml` < project config < env < flags). |
| `ConfigBuilder` | In-memory `Config` construction for library consumers. |
| `StorageConfig` | `home_dir`, `cache_dir`, `packages_dir`, `registry_cache_dir`. |
| `InstallConfig` | `path`, `parallel_downloads`. |
| `ProjectsConfig` | `explicit_paths`, `search_roots`, `max_search_depth`, `ignore_patterns`. |
| `RegistrySourceConfig`, `RegistryType` | Registry entries from `[[registries]]`. |
| `SigningConfig` | `key_path` fallback for `hpm pack`. |
| `ProjectConfig` | Per-project paths (`.hpm/packages/`, `hpm.lock`, `hpm.toml`). |

### hpm-cli

| Type | Purpose |
|------|---------|
| `CliError` | CLI-facing error enum with categorised variants (`Config`, `Package`, `Network`, `Io`, `Internal`, `External`). Carries optional help text. |
| `ExitStatus` | Process-exit code abstraction; converts `&CliError -> ExitStatus -> ExitCode`. |

Exit codes used by `hpm-cli`:

| Code | Meaning |
|------|---------|
| 0 | Success. |
| 1 | User error — bad configuration, bad input, missing manifest, etc. |
| 2 | Internal error — unexpected condition or bug. |
| _N_ | Pass-through exit code from an invoked external tool. |

## Design principles

- **Minimal coupling.** Each crate owns one concern; no upward dependencies.
- **Async by default.** Everything I/O-bound runs on Tokio.
- **Content-addressable storage.** Both HPM packages (`<slug>@<version>` under `~/.hpm/packages/`, with path-installed packages isolated in `_dev/`) and Python venvs (SHA-256 over the resolved set) deduplicate globally.
- **Safety by construction.** Cleanup never removes a package an active project needs; lock file checksums are verified before every install; signing is opt-in but standard when used.
- **TOML-first configuration.** Every persistent config — manifest, lock file, global config — is TOML and hand-editable.
