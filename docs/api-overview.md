# API Overview

This document maps HPM's crate layout to its key public types. For full API
docs with signatures and examples, generate rustdoc locally:

```sh
cargo doc --workspace --no-deps --open
```

## Crate graph

```text
hpm-cli         binary; clap dispatch, output formatting, exit codes
  ├── hpm-core         storage, discovery, lock, registry, fetch, pack, python
  │     ├── hpm-config
  │     └── hpm-package
  ├── hpm-config       layered config loading and merging
  └── hpm-package      manifest parsing, Houdini integration, deps  (leaf)
```

Python tooling (bundled `uv`, content-addressable venvs, Houdini→Python
mapping) lives in the `hpm_core::python` submodule. It was a separate
crate through 0.16 but collapsed into `hpm-core` since it had no
external consumers.

Leaves are `hpm-config` and `hpm-package` — both depend on nothing in the
workspace, so they can be embedded by external tools without dragging in
the rest of HPM.

Each crate defines its own error type via `thiserror` (e.g. `StorageError`,
`ConfigError`). `hpm-cli` converts these into a single `CliError` with exit
codes and help hints.

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
| `fetcher_install_dir(packages_dir, name, version)` | Staging path `<packages_dir>/<safe_name>-<version>` used by `ArchiveFetcher` while extracting; the result is then copied into the canonical CAS via `install_into_cas`. |
| `Registry` (async trait) | Registry abstraction. `ApiRegistry` and `GitRegistry` implement it. |
| `RegistrySet` | Composite that fans requests out to every configured registry. |
| `ArchiveFetcher` | Downloads and extracts registry-hosted archives. |
| `fetch_manifest` | Free function: returns the parsed `PackageManifest` for `(name, version)` without project context. CAS hit reads from disk; CAS miss resolves+fetches+installs. Pass `""` or `"latest"` to pick the highest semver. |
| `PackageRunEnv` | Resolved runtime environment for a `package-env` script, produced by `ProjectManager::resolve_package_env(extra_requirements)`: the merged venv (`venv_bin`, `virtual_env`) plus `python_paths` (each package's `python/` then the venv `site-packages`) for `PYTHONPATH`. Read-only — built from `hpm.lock` + the global store, mirroring what `sync_dependencies` resolves for Houdini. `hpm run` applies it for `package-env = true` scripts. |
| `packer` | Produces signed/unsigned `.zip` archives for `hpm pack`. Public helpers: `pack`, `create_archive`, `compute_archive_checksum`/`compute_bytes_checksum` (SHA-256), `sign_archive`/`sign_bytes` (Ed25519, returns `(base64_signature, hex_key_id)`), `load_signing_key` / `load_signing_key_from_pem`. The byte-based variants are for tooling that mutates archive bytes after pack (e.g. third-party hosting that requires reshaping) and needs to recompute the hash + re-sign without round-tripping through disk. `SigningKey` is re-exported so callers don't need a direct `ed25519-dalek` dep. |

### hpm-package

| Type | Purpose |
|------|---------|
| `PackageManifest` | Parsed `hpm.toml` (every section). `PackageManifest::from_path` reads + parses, returning `ManifestLoadError` with the offending path. `validate()` returns the first structural error; `validate_with(ValidationLevel)` returns a `ValidationReport { errors, warnings }` separating hard errors from publish-quality advisories. |
| `ValidationLevel`, `ValidationReport` | `Strict` runs structural checks only; `Publish` adds advisory warnings on missing description / authors / keywords / `[compat].houdini`. `hpm check` consumes the report; a future `hpm publish` can promote warnings to errors. |
| `ManifestLoadError` | `NotFound { path }` / `Read { path, source }` / `Parse { path, source }`. Re-exported by `hpm-core`'s `StorageError`, `ProjectError`, `DiscoveryError`, and `FetchManifestError`. |
| `IoOp` | Shared IO-error shape `{ op: &'static str, path: PathBuf, source: io::Error }` used as the single `Io(IoOp)` variant in `StorageError`, `ProjectError`, and `DiscoveryError`. Construct via `IoOp::wrap("read directory", &path, e)` at call sites. |
| `PackagePath`, `PackagePathError` | Validated `creator/slug` newtype. Kebab-case enforced at deserialization, so `creator()` and `slug()` return `&str` — no `Option`. |
| `PackageInfo` | Contents of `[package]`. `path: PackagePath` is the canonical identifier; `name`, `version`, etc. are user-facing metadata. |
| `CompatConfig` | Contents of `[compat]`. `houdini: Option<HoudiniRange>` parses and validates at deserialize time; `houdini_min()` extracts its lower bound for Python ABI selection. `platforms: Vec<Platform>` declares the native platforms this package supports — unknown identifiers fail at TOML parse via `Platform::TryFrom<String>`. Must reference platforms used in `[stage.platform.*]` entries. |
| `DependencySpec` | Untagged enum: `Simple(String) \| Url {..} \| Path {..} \| Registry {..}`. |
| `PythonDependencySpec` | Untagged enum: `Simple(String) \| Detailed {..}`. |
| `ManifestEnvEntry`, `EnvMethod` | `[runtime]` entries (method, optional value, `required` flag) and methods (`set`/`prepend`/`append`). `value` is `Option<EnvValue>` — flat string or ordered variant list. `lower(substitutions, is_dev)` is the single emit path; install-source-gated variants are filtered before lowering. |
| `EnvValue`, `EnvValueBranch`, `Condition` | Conditional `[runtime]` value support. Variants compile to Houdini's expression-object array via `compile_condition` / `lower_conditional`. Axes: `houdini` (Cargo-style req), `os`, `python` (all runtime-evaluated by Houdini), plus `install_source` (`"dev"` / `"registry"`, filtered by hpm at install time). |
| `PackageScripts` | `[scripts]` table. `commands: IndexMap<String, ScriptEntry>` — one entry per script, with per-host variation expressed inside `ScriptEntry::cmd` via the same `when`-grammar that `[runtime]` uses. |
| `ScriptEntry`, `ScriptEnv` | Untagged enum: `Plain(String)` for the shorthand form, `WithEnv { cmd: EnvValue, python?, requirements?, label?, description?, package_env }` for the table form. `resolve_cmd(host_os)` picks the matching variant when `cmd` is conditional. Accessors `python()`, `requirements()`, `label()`, `description()`, `needs_venv()`, `uses_package_env()` work on both arms. `package_env` (`package-env` in TOML) opts the script into the project's full resolved environment — see `ProjectManager::resolve_package_env`. `label`/`description` are optional, consumer-agnostic display metadata that HPM itself never acts on. |
| `StageConfig`, `PlatformStaging`, `StagePlatformRules`, `PlaceRule`, `ProfileStaging`, `StageProfileRules` | `[stage]` table. `output_dir` (default `"dist"`), `prepack` script list, workspace `include` / `exclude` globs, and per-platform `place = [{ from, to }]` rules. `from` is a workspace-relative glob; `to` is either a directory (ends with `/`) or a literal archive path. `[stage.profile.<name>]` tables (`StageProfileRules`) layer per-profile `prepack`/`include`/`exclude`/`place` overrides onto the base; `StageConfig::resolved_for_profile(name)` returns the merged config a build uses. |
| `Platform` | Canonical platform enum, mirroring the TumbleTrove API: `LinuxX86_64`, `LinuxAarch64`, `MacosX86_64`, `MacosAarch64`, `WindowsX86_64`, `WindowsAarch64`, `Universal`. `os_key()` returns `Option<&str>` (`None` for `Universal`). |
| `RegistryConfig`, `RegistryType` | `[[registries]]` entries in manifests. |
| `PackageTemplate` | Scaffolding for `hpm init` (standard and `--bare`). |
| `HoudiniPackage`, `HoudiniNativePackage`, `HoudiniEnvValue` | Houdini `package.json` output types. |

### hpm-core::python

| Type | Purpose |
|------|---------|
| `VenvManager` | Creates and reuses content-addressable venvs. |
| `PythonVersion` | Houdini-to-Python version value. |
| `ResolvedDependencies`, `ResolvedDependencySet`, `ResolvedPackage` | UV-resolved dependency sets with content hash. |
| `PythonCleanupAnalyzer` | Detects orphan venvs for `hpm clean --python-only`. |
| `prepare_script_env(entry)` | Canonical entry point for table-form `[scripts]`. Bootstraps bundled uv, materializes the venv if needed, and returns a `ScriptEnvHandle` carrying the env-var mutations to apply before spawning. Plain string entries return a default (no-op) handle. Shared by `hpm run` and outside embedders (e.g. tumbletrove-desktop's hook runner). |
| `ScriptEnvHandle` | Spawn-strategy-agnostic env-var bundle (`path_prepend`, `env`). `apply_to(&mut HashMap<String,String>)` folds it into a caller-staged env map ready for `Command::envs` or any other spawn primitive. |
| `ensure_script_venv(python, requirements)` | Lower-level free function: resolves raw PEP-508 requirement strings via `uv pip compile` and defers to `VenvManager`, returning the venv root. Prefer `prepare_script_env` for the full flow. |
| `venv_bin_dir(path)` | Returns the executable directory inside a uv-created venv (`bin/` on Unix, `Scripts/` on Windows). `prepare_script_env` already prepends this to `PATH`; use directly only when bypassing the handle. |
| `DEFAULT_SCRIPT_PYTHON` | `"3.11"`. The Python version `ensure_script_venv` uses when a script omits `python`. |

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
| `ProjectPaths` | Per-project paths (`.hpm/packages/`, `hpm.lock`, `hpm.toml`). |

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
