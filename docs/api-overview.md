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
  │     ├── hpm-package
  │     └── hpm-assets
  ├── hpm-config       layered config loading and merging
  ├── hpm-package      manifest parsing, Houdini integration, deps  (leaf)
  └── hpm-assets       operator asset-index model for `hpm pack`     (leaf)
```

Python tooling (bundled `uv`, content-addressable venvs, Houdini→Python
mapping) lives in the `hpm_core::python` submodule. It was a separate
crate through 0.16 but collapsed into `hpm-core` since it had no
external consumers.

Leaves are `hpm-config`, `hpm-package`, and `hpm-assets` — none depend on
anything in the workspace, so they can be embedded by external tools without
dragging in the rest of HPM.

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
| `LockError` | `Read`/`Parse`/`Write`/`Serialize` plus `ChecksumMismatch` and `PackageMissing { package, expected_dir }`. |
| `PackageSource` | URL-only struct `{ url, version }` — what `ArchiveFetcher` downloads. Path deps bypass the fetcher and use `LockedSource::Path` in the lockfile. |
| `cas_install_dir(packages_dir, name, version)` | Canonical install path `<packages_dir>/<slug>@<version>` for a lockfile dep name. Used by `LockFile::verify_checksums` and any consumer that needs to find an installed package off the lockfile alone. |
| `fetcher_install_dir(packages_dir, name, version)` | Staging path `<packages_dir>/<safe_name>-<version>` used by `ArchiveFetcher` while extracting; the result is then copied into the canonical CAS via `install_into_cas`. |
| `Registry` (async trait) | Registry abstraction. `ApiRegistry` and `GitRegistry` implement it. |
| `RegistrySet` | Composite that fans requests out to every configured registry. |
| `ArchiveFetcher` | Downloads and extracts registry-hosted archives. |
| `fetch_manifest` | Free function: returns the parsed `PackageManifest` for `(name, version)` without project context. CAS hit reads from disk; CAS miss resolves+fetches+installs. Pass `""` or `"latest"` to pick the highest semver. |
| `PackageRunEnv` | Resolved runtime environment for a `package-env` script, produced by `ProjectManager::resolve_package_env(extra_requirements)`: the merged venv (`venv_bin`, `virtual_env`) plus `python_paths` (each package's `python/` then the venv `site-packages`) for `PYTHONPATH`. Read-only — built from `hpm.lock` + the global store, mirroring what `sync_dependencies` resolves for Houdini. `hpm run` applies it for `package-env = true` scripts. |
| `script_run` | The shared `[scripts]` runner — the single place the script-env contract (`HPM_PACKAGE_ROOT`, caller `extra_env`, and per-script-venv or `package-env` `PATH`/`VIRTUAL_ENV`/`PYTHONPATH`) is composed. Embedders implement `ScriptSink` (spawn + diagnostics) and drive `run_prepack(manifest, names, package_root, extra_env, sink)` (a `[stage].prepack` sequence, fail-fast) or `run_script(manifest, name, package_root, extra_args, extra_env, sink)` (one entry, returns the exit code). `prepare_script(...)` exposes just the env+command-line composition as a `PreparedScript { command_line, working_dir, env }` for embedders that own their spawn loop. `hpm run` and `hpm build`'s prepack both route through here, so a manifest feature picked up by one is picked up by all — no per-embedder drift. **Prefer this over re-composing `prepare_script_env` by hand:** the lower-level handle is easy to half-wire (e.g. forgetting to put the venv interpreter on `PATH`). |
| `packer` | Produces signed/unsigned `.zip` archives for `hpm pack`. Public helpers: `pack`, `create_archive`, `compute_archive_checksum`/`compute_bytes_checksum` (SHA-256), `sign_archive`/`sign_bytes` (Ed25519, returns `(base64_signature, hex_key_id)`), `load_signing_key` / `load_signing_key_from_pem`. The byte-based variants are for tooling that mutates archive bytes after pack (e.g. third-party hosting that requires reshaping) and needs to recompute the hash + re-sign without round-tripping through disk. `SigningKey` is re-exported so callers don't need a direct `ed25519-dalek` dep. |
| `collect_assets`, `AssetIndex`, `AssetIndexError` | Builds the `hpm pack` operator asset index from a manifest's `[[operators]]` declarations for a target platform (`asset_index` module). Maps each declaration to an `hpm_assets::Asset` (deriving namespace/version from the type name), resolving per-platform `source` tables to the target platform and dropping operators not shipped there. Verifies each resolved `source` exists in the produced archive — any that don't are returned in `AssetIndex::missing_sources` for the caller to warn on or, with `hpm pack --verify-assets`, treat as fatal. |

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
| `DependencySpec` | Enum: `Registry {..} \| Url {..} \| Path {..}`. The bare-string shorthand `pkg = "1.0.0"` round-trips as `Registry { registry: None, optional: false }`. |
| `PythonDependencySpec` | Untagged enum: `Simple(String) \| Detailed {..}`. |
| `ManifestEnvEntry`, `EnvMethod` | `[runtime]` entries (method, optional value, `required` flag) and methods (`set`/`prepend`/`append`). `value` is `Option<EnvValue>` — flat string or ordered variant list. `lower(substitutions, is_dev)` is the single emit path; install-source-gated variants are filtered before lowering. |
| `EnvValue`, `EnvValueBranch`, `Condition` | Conditional `[runtime]` value support. Variants compile to Houdini's expression-object array via `compile_condition` / `lower_conditional`. Axes: `houdini` (Cargo-style req), `os`, `python` (all runtime-evaluated by Houdini), plus `install_source` (typed `InstallSource`: `dev` / `registry`, filtered by hpm at install time). `os` is a typed `OsKey`; unknown axis values fail at manifest load. |
| `PackageScripts` | `[scripts]` table. `commands: IndexMap<String, ScriptEntry>` — one entry per script, with per-host variation expressed inside `ScriptEntry::cmd` via the same `when`-grammar that `[runtime]` uses. |
| `ScriptEntry`, `ScriptEnv` | Untagged enum: `Plain(String)` for the shorthand form, `WithEnv { cmd: EnvValue, python?, requirements?, label?, description?, package_env }` for the table form. `resolve_cmd(host_os)` picks the matching variant when `cmd` is conditional. Accessors `python()`, `requirements()`, `label()`, `description()`, `needs_venv()`, `uses_package_env()` work on both arms. `package_env` (`package-env` in TOML) opts the script into the project's full resolved environment — see `ProjectManager::resolve_package_env`. `label`/`description` are optional, consumer-agnostic display metadata that HPM itself never acts on. |
| `StageConfig`, `PlatformStaging`, `StagePlatformRules`, `PlaceRule`, `ProfileStaging`, `StageProfileRules` | `[stage]` table. `output_dir` (default `"dist"`), `prepack` script list, workspace `include` / `exclude` globs, and per-platform `place = [{ from, to }]` rules. `from` is a workspace-relative glob; `to` is either a directory (ends with `/`) or a literal archive path. `[stage.profile.<name>]` tables (`StageProfileRules`) layer per-profile `prepack`/`include`/`exclude`/`place` overrides onto the base; `StageConfig::resolved_for_profile(name)` returns the merged config a build uses. |
| `Platform` | Canonical platform enum, mirroring the TumbleTrove API: `LinuxX86_64`, `LinuxAarch64`, `MacosX86_64`, `MacosAarch64`, `WindowsX86_64`, `WindowsAarch64`, `Universal`. `os_key()` returns `Option<&str>` (`None` for `Universal`). |
| `RegistryConfig`, `RegistryType` | `[[registries]]` entries in manifests. |
| `OperatorDecl`, `OperatorKind`, `OperatorSource`, `SourceResolution` | `[[operators]]` entries — the operators (node types) a package bundles, declared by the author so `hpm pack` can emit a searchable asset index. `kind` (`Hda`/`Dso`), `type_name`, and `category` are required; `label`, `tab_submenu`, `icon`, and `source` are optional. `OperatorSource` is either a single archive path or a per-platform table; `OperatorDecl::resolved_source(platform)` resolves it to a `SourceResolution` (`Path` / `Unspecified` / `NotForPlatform`). |
| `PackageTemplate` | Scaffolding for `hpm init` (standard and `--bare`). |
| `HoudiniPackage`, `HoudiniNativePackage`, `HoudiniEnvValue` | Houdini `package.json` output types. |

### hpm-assets

| Type | Purpose |
|------|---------|
| `Asset`, `AssetKind` | The wire model `hpm pack --json` emits in its `assets` array. One flat object per bundled operator with a `kind` discriminator (`hda_operator`/`dso_operator`); `None` fields are omitted. Built from `[[operators]]` declarations by `hpm_core::collect_assets`. |
| `split_type_name(type_name)` | Best-effort split of a namespaced operator type name into `(namespace, base, op_version)` following Houdini's `namespace::name::version` grammar (version detected as digits-and-dots). Used to populate an `Asset`'s `namespace`/`op_version` from the declared `type_name`. |

### hpm-core::python

| Type | Purpose |
|------|---------|
| `VenvManager` | Creates and reuses content-addressable venvs. |
| `PythonVersion` | Houdini-to-Python version value. |
| `ResolvedDependencies`, `ResolvedDependencySet`, `ResolvedPackage` | UV-resolved dependency sets with content hash. |
| `PythonCleanupAnalyzer` | Detects orphan venvs for `hpm clean --python-only`. |
| `prepare_script_env(entry)` | Per-script-venv building block: bootstraps bundled uv, materializes the venv if needed, and returns a `ScriptEnvHandle` carrying the env-var mutations to apply before spawning. Plain string entries return a default (no-op) handle. Covers only the `python`/`requirements` venv path — not `package-env`, `HPM_PACKAGE_ROOT`, or the command line. Most embedders should reach for `hpm_core::script_run` (above), which wires this plus the rest of the contract; use `prepare_script_env` directly only when you specifically need just the venv handle. |
| `ScriptEnvHandle` | Spawn-strategy-agnostic env-var bundle (`path_prepend`, `env`). `apply_to(&mut HashMap<String,String>)` folds it into a caller-staged env map ready for `Command::envs` or any other spawn primitive. |
| `ensure_script_venv(python, requirements)` | Lower-level free function: resolves raw PEP-508 requirement strings via `uv pip compile` and defers to `VenvManager`, returning the venv root. Prefer `prepare_script_env` for the full flow. |
| `venv_bin_dir(path)` | Returns the executable directory inside a uv-created venv (`bin/` on Unix, `Scripts/` on Windows). `prepare_script_env` already prepends this to `PATH`; use directly only when bypassing the handle. |
| `DEFAULT_SCRIPT_PYTHON` | `"3.11"`. The Python version `ensure_script_venv` uses when a script omits `python`. |

### hpm-config

| Type | Purpose |
|------|---------|
| `Config` | Top-level config (defaults < `~/.hpm/config.toml` < project config < env < flags). |
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
