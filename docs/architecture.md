# Architecture

Technical overview of HPM — system design, dependency resolution, cleanup,
storage, and Python integration. Intended for contributors and integrators.

## Table of contents

1. [System overview](#system-overview)
2. [Crate layout](#crate-layout)
3. [Core types](#core-types)
4. [Dependency resolution](#dependency-resolution)
5. [Install flow](#install-flow)
6. [Project-aware cleanup](#project-aware-cleanup)
7. [Storage architecture](#storage-architecture)
8. [Python integration](#python-integration)
9. [Security and performance](#security-and-performance)
10. [Extension points](#extension-points)

## System overview

HPM is a modular monolith. A single binary (`hpm-cli`) drives a handful of
focused library crates. Responsibilities are split along stable seams:
manifest parsing, configuration, resolution, storage, Python, and errors all
live in their own crates so they can be reused (e.g., a desktop app can
depend on the library crates and skip the CLI entirely).

```text
┌────────────────────────────────────────────────────────────────────────┐
│ CLI (hpm-cli)                                                          │
│   • subcommands: init, add, remove, install, update, list, search,    │
│     check, pack, audit, clean, registry, completions                  │
│   • output formats: human, json, json-lines, json-compact              │
│   • shell completions: bash, zsh, fish, powershell, elvish             │
└───────────────────────────────────┬────────────────────────────────────┘
                                    │
┌───────────────────────────────────▼────────────────────────────────────┐
│ Core orchestration (hpm-core)                                          │
│   • StorageManager: global package install/remove/list                 │
│   • ProjectDiscovery: scan configured paths for active projects        │
│   • LockFile / LockedDependency / PackageSource                        │
│   • Registry trait + ApiRegistry, GitRegistry                          │
│   • ArchiveFetcher, packer                                             │
└─────────┬──────────────────┬──────────────────┬──────────────────┬─────┘
          │                  │                  │                  │
┌─────────▼───────┐ ┌────────▼────────┐ ┌───────▼──────┐ ┌─────────▼─────┐
│ hpm-package     │ │ hpm-python      │ │ hpm-resolver │ │ hpm-config    │
│   PackageManifest│ │   VenvManager   │ │   Resolver   │ │   Config      │
│   DependencySpec │ │   PythonVersion │ │   VersionReq │ │   Storage/    │
│   NativeConfig   │ │   ResolvedSet   │ │              │ │   Projects/   │
│   Platform       │ │   bundled uv    │ │              │ │   Signing     │
│   HoudiniPackage │ └─────────────────┘ └──────────────┘ └───────────────┘
└──────────────────┘
```

Each crate defines its own error type (e.g. `StorageError`, `ResolverError`,
`ConfigError`) via `thiserror`. Errors surface to the user through `CliError`
in `hpm-cli`, which converts them to exit codes and help hints.

Key non-functional properties:

- **Language**: Rust, edition 2024, MSRV 1.85.
- **Async runtime**: Tokio, multi-thread.
- **Configuration**: TOML throughout (manifests, lock file, global config).
- **Testing**: unit, integration, and property-based (proptest) tests.

## Crate layout

| Crate | Responsibility |
|-------|----------------|
| `hpm-cli` | Command-line frontend (clap). Turns command-line invocations into calls on the library crates. |
| `hpm-core` | Storage, project discovery, lock file, registry trait + two implementations, archive fetching/packing. |
| `hpm-package` | `hpm.toml` parsing and validation, dependency/Python dependency types, Houdini `package.json` generation, platform enum. |
| `hpm-python` | Bundled `uv`, content-addressable venv management, Houdini→Python version mapping, venv cleanup analysis. |
| `hpm-resolver` | PubGrub-style dependency solver. |
| `hpm-config` | Global and project configuration loading and merging. |

Dependencies flow downward: `hpm-cli` depends on everything else, `hpm-core`
depends on package/python/resolver/config, and so on. No crate depends
upward. Domain errors (`StorageError`, `ResolverError`, `ConfigError`, ...)
live in the crate that produces them.

## Core types

Selected public types. See `cargo doc --workspace --no-deps --open` for the
full list.

### hpm-package

```rust
pub struct PackageManifest {
    pub package: PackageInfo,
    pub houdini: Option<HoudiniConfig>,
    pub native: Option<NativeConfig>,
    pub registries: Option<Vec<RegistryConfig>>,
    pub dependencies: Option<IndexMap<String, DependencySpec>>,
    pub python_dependencies: Option<IndexMap<String, PythonDependencySpec>>,
    pub env: Option<IndexMap<String, ManifestEnvEntry>>,
    pub scripts: Option<PackageScripts>,
}

pub struct PackageScripts {
    pub platform: Option<PlatformScripts>,        // [scripts.platform.<os>]
    pub commands: IndexMap<String, ScriptEntry>,  // flat entries under [scripts]
}

pub struct PlatformScripts {
    pub linux: Option<IndexMap<String, ScriptEntry>>,
    pub macos: Option<IndexMap<String, ScriptEntry>>,
    pub windows: Option<IndexMap<String, ScriptEntry>>,
}

// Untagged: serialises as either a bare string ("cargo build") or a table
// form { cmd, python?, requirements? }. The table form opts the script into
// a uv-managed venv resolved on demand by `hpm run`.
pub enum ScriptEntry {
    Plain(String),
    WithEnv(ScriptEnv),
}

pub struct ScriptEnv {
    pub cmd: String,
    pub python: Option<String>,        // e.g. "3.11"
    pub requirements: Vec<String>,     // e.g. ["PySide6>=6.6"]
}

pub struct PackageInfo {
    pub path: String,              // "creator/slug"
    pub name: String,              // freeform display name
    pub version: String,           // semver
    pub description: Option<String>,
    pub authors: Option<Vec<String>>,
    pub license: Option<String>,
    pub readme: Option<String>,
    pub homepage: Option<String>,
    pub repository: Option<String>,
    pub documentation: Option<String>,
    pub keywords: Option<Vec<String>>,
    pub categories: Option<Vec<String>>,
}

pub enum DependencySpec {
    Simple(String),                                          // "1.0.0"
    Url { url: String, version: String, optional: bool },
    Path { path: String, optional: bool },
    Registry { version: String, registry: Option<String>, optional: bool },
}

pub enum Platform {
    LinuxX86_64,      // "linux-x86_64"
    MacosUniversal,   // "macos-universal"
    WindowsX86_64,    // "windows-x86_64"
}
```

### hpm-core

```rust
pub struct LockFile {
    pub version: u32,
    pub package: LockPackageInfo,
    pub dependencies: BTreeMap<String, LockedDependency>,
    pub python_dependencies: BTreeMap<String, LockedPythonDependency>,
    pub metadata: Option<LockMetadata>,
}

pub struct LockedDependency {
    pub version: String,
    pub checksum: Option<String>,    // "sha256:<hex>"
    pub source: LockedSource,        // Url { url, version } | Path { path }
    pub dependencies: Vec<String>,   // transitive names
}

#[async_trait]
pub trait Registry: Send + Sync {
    async fn search(&self, query: &str) -> Result<SearchResults, RegistryError>;
    async fn get_versions(&self, name: &str) -> Result<Vec<RegistryEntry>, RegistryError>;
    async fn get_version(&self, name: &str, version: &str) -> Result<RegistryEntry, RegistryError>;
    async fn refresh(&self) -> Result<(), RegistryError>;
    async fn config(&self) -> Result<RegistryConfig, RegistryError>;
    fn name(&self) -> &str;
}
```

### hpm-python

```rust
pub struct PythonVersion {
    pub major: u8,
    pub minor: u8,
    pub patch: Option<u8>,
}

pub struct ResolvedDependencySet {
    pub python_version: PythonVersion,
    pub packages: BTreeMap<String, ResolvedPackage>,
}

pub struct VenvManager { /* … */ }

impl VenvManager {
    pub async fn ensure_virtual_environment(
        &self,
        resolved: &ResolvedDependencies
    ) -> Result<PathBuf>;
}
```

### hpm-config

```rust
pub struct Config {
    pub install: InstallConfig,
    pub storage: StorageConfig,
    pub projects: ProjectsConfig,
    pub registries: Vec<RegistrySourceConfig>,
    pub signing: SigningConfig,
}

pub struct StorageConfig {
    pub home_dir: PathBuf,            // default: $HOME/.hpm
    pub cache_dir: PathBuf,
    pub packages_dir: PathBuf,
    pub registry_cache_dir: PathBuf,
}
```

Configuration merges in this order: defaults → `~/.hpm/config.toml` →
`<cwd>/.hpm/config.toml` → environment → CLI flags.

## Dependency resolution

HPM's resolver (`hpm-resolver`) is PubGrub-inspired: the same family of
algorithms `uv`, Dart's `pub`, and Swift PM use. It models resolution as a
constraint-satisfaction problem.

```text
Given:
  P = { p₁, …, pₙ }        packages
  V(pᵢ) = { v₁, …, vₖ }    available versions
  C = { c₁, …, cₘ }        version constraints

Find an assignment A: P → V such that ∀c ∈ C, c(A) holds.
```

The solver:

1. Starts with the root manifest's direct dependencies.
2. Picks the highest version that satisfies all current constraints for a candidate package.
3. Recursively adds that version's transitive dependencies.
4. On conflict, learns a minimal explanation of why the current path is infeasible and backtracks.
5. Continues until an assignment exists or failure is final.

### Version requirement grammar

HPM manifest values accept the usual semver constraint operators:

| Form | Meaning |
|------|---------|
| `=1.2.3`, `1.2.3` | Exact. |
| `^1.2.3` | Compatible with the given major (`>=1.2.3, <2.0.0`). |
| `~1.2.3` | Patch updates allowed (`>=1.2.3, <1.3.0`). |
| `>=1.0.0, <2.0.0` | Explicit range. |

## Install flow

```text
 ┌──────────────────────────────────────────────────────────────────────┐
 │ hpm install                                                          │
 ├──────────────────────────────────────────────────────────────────────┤
 │  1. Load hpm.toml                                                    │
 │  2. If hpm.lock exists:                                              │
 │       verify cached packages against stored checksums                │
 │       warn if metadata.generated_at > 90 days                        │
 │  3. Resolve HPM dependencies                                         │
 │       query configured registries                                    │
 │       backtrack on conflict                                          │
 │  4. Fetch + install in parallel (one task per dep):                  │
 │       URL/registry → ArchiveFetcher → ~/.hpm/fetch/                  │
 │                    → install_from_path → ~/.hpm/packages/<slug>@<v>/ │
 │       Path         → install_from_path_dev                           │
 │                    → ~/.hpm/packages/_dev/<slug>@<v>/                │
 │  5. Merge [python_dependencies] from root + every dep manifest       │
 │     Python ABI = root manifest's [houdini].min_version (NOT per-dep) │
 │  6. Ensure managed CPython installed under ~/.hpm/uv-python/         │
 │     (uv python install <ver>) — auto-downloads on clean machines     │
 │  7. Resolve with bundled uv, hash the resolved set, pick or          │
 │     rebuild ~/.hpm/venvs/<hash>/                                     │
 │  8. uv pip install --python <venv>/bin/python                        │
 │  9. Write <project>/.hpm/packages/{name}.json per dep                │
 │ 10. Sweep stale <project>/.hpm/packages/ entries                     │
 │ 11. Write hpm.lock                                                   │
 └──────────────────────────────────────────────────────────────────────┘
```

`--frozen-lockfile` inserts a check between steps 2 and 3 that aborts if
the solver's output would require a different lock than the existing one.

## Project-aware cleanup

`hpm clean` removes packages and venvs no active project depends on. The
identity of "active projects" comes from the `[projects]` config section.

```text
  I = installed packages in ~/.hpm/packages/
  P = active projects discovered via ProjectDiscovery
  D(p) = direct deps declared by project p
  T(I) = transitive closure over the dependency graph

  orphans = I \ T( ⋃ D(p) )
```

Implementation outline:

```rust
pub struct GlobalDependencyGraph {
    packages: HashMap<PackageId, PackageNode>,
    edges: HashMap<PackageId, HashSet<PackageId>>,
    roots: HashSet<PackageId>,
}

impl GlobalDependencyGraph {
    pub fn reachable(&self) -> HashSet<PackageId> {
        let mut seen = HashSet::new();
        let mut stack: Vec<_> = self.roots.iter().cloned().collect();
        while let Some(id) = stack.pop() {
            if seen.insert(id.clone()) {
                if let Some(deps) = self.edges.get(&id) {
                    stack.extend(deps.iter().cloned());
                }
            }
        }
        seen
    }

    pub fn orphans(&self, installed: &HashSet<PackageId>) -> Vec<PackageId> {
        let reachable = self.reachable();
        installed.difference(&reachable).cloned().collect()
    }
}
```

Project discovery scans `explicit_paths` plus recursive walks of
`search_roots`, respecting `max_search_depth` and skipping directory names
that match `ignore_patterns`. Only directories containing an `hpm.toml` are
considered projects.

Python venv cleanup uses the same principle against the `metadata.json` files
in each venv, which list the HPM packages using that venv.

## Storage architecture

HPM stores everything under `~/.hpm/` on every supported platform. The
layout is content-addressable where it helps:

```text
~/.hpm/
├── config.toml
├── packages/                       # canonical CAS — written by StorageManager
│   ├── slug@1.0.0/                 # registry / URL installs
│   │   ├── hpm.toml
│   │   ├── (package sources)
│   │   └── … (Houdini convention subdirs)
│   └── _dev/                       # path-installed (dev) packages
│       └── slug@1.0.0/             # never substituted for a registry hit
│           └── …                   # at the same (slug, version)
├── fetch/                          # ArchiveFetcher staging — extracted
│   └── creator-slug-1.0.0/         # archives live here briefly before
│                                   # install_from_path copies into packages/
├── venvs/
│   └── <12-char hash>/             # hash of resolved set + Python version
│       ├── pyvenv.cfg
│       ├── bin/                    # or Scripts/ on Windows
│       ├── lib/pythonX.Y/site-packages/
│       └── metadata.json
├── cache/                          # download archive cache
├── registry/                       # one subdir per registry
│   └── <registry name>/
├── tools/                          # bundled uv binary
├── uv-cache/                       # isolated uv cache
├── uv-config/
├── uv-python/                      # managed CPython installs (UV_PYTHON_INSTALL_DIR)
└── logs/
```

Both `hpm install` and `hpm sync` route URL/registry deps through the same
two-step flow: `ArchiveFetcher` downloads + extracts into `~/.hpm/fetch/`,
then `StorageManager::install_from_path` copies into the canonical CAS at
`~/.hpm/packages/<slug>@<version>/`. Path deps skip the fetcher entirely
and go straight to `~/.hpm/packages/_dev/<slug>@<version>/` via
`install_from_path_dev`.

Per-project:

```text
<project>/
├── hpm.toml
├── hpm.lock
└── .hpm/
    ├── config.toml                 # optional
    └── packages/
        ├── utility-nodes.json      # generated Houdini manifests, one per dep
        └── material-library.json   # absolute paths into the global CAS
```

Global packages are shared across projects; each project holds only the
per-dependency Houdini manifest and a lockfile. This keeps disk usage sane
across a studio's worth of projects. The Houdini JSON manifests carry
absolute CAS paths, so the project tree contains no symlinks pointing into
the global storage.

`sync_dependencies` sweeps stale per-package manifests at the end of every
sync: any `<slug>.json` in `<project>/.hpm/packages/` whose slug is no longer
in the resolved dependency set is removed. Without this, a manifest written
by a previous sync (e.g. for a path dependency that has since been removed)
would keep loading the package on Houdini launch even though `hpm.toml` no
longer asks for it.

Path dependencies install into `~/.hpm/packages/_dev/<slug>@<version>/`
rather than the registry CAS at `~/.hpm/packages/<slug>@<version>/`.
The `_dev` subtree is invisible to `list_installed`, so a dev install of
`foo@1.0.0` cannot be served as the cached install for a registry
resolution at the same coordinate from a different project.

## Python integration

The Python layer runs on four ideas, in descending order of importance:

1. **Content-addressable venvs.** Hash the resolved dependency set, use it as the venv directory name. Same hash → same venv → shared across packages.
2. **Bundled uv.** A copy of `uv` ships with HPM and lives at `~/.hpm/tools/uv`. Its cache (`~/.hpm/uv-cache/`), config (`~/.hpm/uv-config/`), and managed CPython installs (`~/.hpm/uv-python/`, pinned via `UV_PYTHON_INSTALL_DIR`) are isolated from any system `uv` so HPM never perturbs your other Python work.
3. **Self-bootstrapping Python.** `uv pip compile` and `uv venv` need an interpreter. Before invoking either, HPM runs `uv python install <ver>` to ensure a managed CPython matching the project's Houdini ABI exists. This is what makes a clean Windows install (no system Python anywhere) Just Work — without it `pip compile` errors with `No interpreter found in virtual environments, managed installations, search path, or registry`. The ensure step is process-cached, so it costs one fast filesystem probe per resolution.
4. **Houdini manifest generation.** For each HPM package that declares Python dependencies, HPM writes `<project>/.hpm/packages/{name}.json` with `PYTHONPATH` prepended at the shared venv's `site-packages`.

### Hash function

```rust
// simplified
pub fn content_hash(resolved: &ResolvedDependencies) -> String {
    let mut hasher = Sha256::new();
    hasher.update(format!("python:{}", resolved.python_version));
    let mut packages: Vec<_> = resolved.packages.iter().collect();
    packages.sort_by_key(|(name, _)| name.as_str());
    for (name, spec) in packages {
        hasher.update(name.as_bytes());
        hasher.update(spec.version.as_bytes());
        for extra in spec.extras.iter().flatten() {
            hasher.update(extra.as_bytes());
        }
    }
    hex::encode(hasher.finalize())[..12].to_string()
}
```

### Houdini→Python mapping

| Houdini | Python |
|---------|--------|
| 20.5+ | 3.10 |
| 21.x | 3.11 |
| 22.x | 3.13 |

The mapping is sourced from the **root** manifest's `[houdini].min_version`
— the project's Houdini build is what determines the embedded CPython ABI
that wheels in the venv must match. A dependency package's own
`[houdini].min_version` is a compatibility floor (the oldest Houdini it
runs on) and is **not** consulted for ABI selection: a `min_version =
"21.0"` package consumed by a Houdini-22 project still gets a 3.13 venv.

Unsupported versions return an error. No silent fallback — an ABI-mismatched
venv would break C-extension imports at Houdini launch instead of surfacing
the mapping gap at install time. Houdini 19.x (Python 3.7) and 20.0–20.4
(Python 3.9) are unsupported: their Python interpreters are past upstream EOL.

### Generated Houdini manifest

```json
{
  "hpath": ["/Users/me/.hpm/packages/studio/my-tool@1.0.0"],
  "env": [
    {
      "PYTHONPATH": {
        "method": "prepend",
        "value": "/Users/me/.hpm/venvs/a1b2c3d4e5f6/lib/python3.11/site-packages"
      }
    }
  ],
  "enable": "houdini_version >= '20.5'"
}
```

`method: "prepend"` delegates path-separator handling to Houdini so the same
manifest works on Windows (`;`) and Unix (`:`) without embedding an
OS-specific joiner. Generator lives in
`hpm-cli::commands::install::build_houdini_package_for_install`.

## Security and performance

### Security

- **Transport**: TLS through rustls (pure Rust, no OpenSSL). Certificate verification against the platform trust store.
- **Integrity**: SHA-256 over every installed archive, recorded in `hpm.lock`, verified before every install.
- **Signing**: `hpm pack --key` signs archives with Ed25519 over PKCS#8 PEM private keys. `keyId` = first 8 bytes of public key, hex-encoded. See [Security](security.md#package-signing) for the wire format.
- **Isolation**: bundled `uv` runs out-of-process with its own cache and config, never touching system state.

### Performance

- **Concurrent downloads**: registry-fronted installs run in parallel, bounded by `[install].parallel_downloads` (default 8).
- **Content-addressable dedup**: both HPM packages (keyed by `slug@version` in the global CAS) and Python venvs (keyed by resolved-set hash) are deduplicated globally.
- **Cache hits**: `uv`'s cache eliminates wheel re-downloads across projects; HPM's venv cache eliminates install work when a matching set is found.
- **Atomic filesystem ops**: packages are extracted to a temp directory and renamed into place to avoid partial states.

## Extension points

### Programmatic configuration

The library crates can be used without the CLI. `hpm-config::ConfigBuilder`
builds a `Config` in-memory for embedders (desktop apps, pipeline tools):

```rust
use hpm_config::{Config, RegistryType};

let config = Config::builder()
    .registry("houdinihub", "https://api.3db.dk/v1/registry", RegistryType::Api)
    .storage_dir("/studio/shared/.hpm")
    .install_path("packages/hpm")
    .build();
```

### Registry trait

Any type that implements the `Registry` async trait can be plugged into a
`RegistrySet` and used for resolution. The built-in `ApiRegistry` and
`GitRegistry` are reference implementations.

### Plugin system

Not implemented. The CLI surface is currently fixed at compile time. A
dynamic plugin system is on the roadmap but not prioritized — most studios
are better served by wrapping HPM in a higher-level pipeline tool.
