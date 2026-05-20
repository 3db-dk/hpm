# Changelog

All notable changes to HPM (Houdini Package Manager) will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.14.0] - 2026-05-20

### Added
- **Garbage collection for dev (`_dev/`) installs.** `hpm clean` (and
  `hpm clean --comprehensive`) now sweeps stranded entries in
  `~/.hpm/packages/_dev/`. The `_dev/` subtree was previously invisible to
  orphan collection because it's filtered out of `list_installed`, so
  removing a `{ path = "..." }` dep from `hpm.toml` left its snapshot or
  link entry behind forever. A new parallel cleanup pass walks every
  discovered project's path-dependencies, reads each source manifest for
  its `(slug, version)`, and treats anything in `_dev/` outside that union
  as orphan. Link installs are unlinked safely via the existing
  `remove_install_entry` primitive (no `remove_dir_all` traversal into a
  workspace). Projects with an unresolvable path-dep source log a warning
  and don't block cleanup. `ComprehensiveCleanupResult` gains a
  `removed_dev_installs: Vec<String>` field whose entries are prefixed
  `_dev/<slug>@<version>` so CLI output keeps the two cleanup scopes
  distinct.
- **Link-mode installs for path dependencies.** A new opt-in `link = true`
  on `[dependencies] my-dep = { path = "...", link = true }` installs the
  package into `~/.hpm/packages/_dev/<slug>@<version>/` as a symlink (Unix)
  or NTFS junction (Windows) instead of copying. Working-tree edits to
  `.apex` / `.hda` / `.py` / `.shelf` files are picked up by a live Houdini
  session immediately, with no re-sync needed. Junctions (vs NTFS directory
  symlinks) are used on Windows because they don't require Developer Mode
  or admin. Surfaced through `hpm add --path <dir> --link`. The
  legacy snapshot-copy behavior remains the default for path deps so
  existing manifests are unaffected.

### Fixed
- **Symlink-aware target removal in `StorageManager::install_from_path_*`
  and `remove_package`.** Both removal paths now distinguish link entries
  from real directories via `symlink_metadata` (plus `junction::exists` on
  Windows) and remove symlinks/junctions through `remove_file` /
  `junction::delete` rather than `remove_dir_all`. Prevents the catastrophic
  case where a stale Windows junction at a package path would have caused
  `remove_dir_all` to recurse into and delete the user's workspace on the
  next sync or orphan-cleanup. Defensive even without link mode, since a
  junction could have been created out-of-band.

## [0.13.0] - 2026-05-15

### Changed
- **Platform identifiers aligned with the TumbleTrove API.** `Platform` now
  carries arch-suffixed variants — `linux-x86_64`, `linux-aarch64`,
  `macos-x86_64`, `macos-aarch64`, `windows-x86_64`, `windows-aarch64` — plus
  an OS-agnostic `universal`, matching the registry's `build.platform` enum
  verbatim. `hpm pack --platform` and the `[native].platforms` list in
  `hpm.toml` accept any of these. `Platform::current()` reports the
  arch-suffixed variant for the host, so auto-detect on Apple Silicon now
  returns `macos-aarch64` instead of the old fat-binary identifier.

### Removed
- **`macos-universal` platform identifier.** The legacy hpm-only "fat binary"
  tag was dropped — the API rejects it, and the new `macos-x86_64` /
  `macos-aarch64` (or `universal` for OS-agnostic content) cover both
  meanings explicitly. Existing `hpm.toml` manifests carrying
  `macos-universal` in `[native].platforms` or any `[native.macos-universal]`
  section fail to parse and must be migrated to the arch-suffixed names.
  `Platform::os_key()` now returns `Option<&'static str>` (`None` for
  `Universal`).

## [0.12.3] - 2026-05-14

### Added
- **`prepare_script_env` and `ScriptEnvHandle` in `hpm-python`.** Promotes
  per-script venv preparation from a private helper inside `hpm run` to a
  shared, spawn-strategy-agnostic API. Given a `ScriptEntry`, the function
  lazily bootstraps bundled uv, materializes the venv if needed, and
  returns a `ScriptEnvHandle` that carries the env-var mutations
  (`VIRTUAL_ENV`, prepended `PATH`) the caller must apply before spawning.
  `apply_to(&mut HashMap<String,String>)` folds those mutations into a
  caller-staged env map, so outside embedders — `hpm run` (shells via
  `cmd /C` / `sh -c`) and the tumbletrove-desktop hook runner
  (direct-spawn via `CreateProcessW` / `execvp`) — consume the same handle
  through their own spawn primitives. Plain string entries and table-form
  entries with neither `python` nor `requirements` get a default no-op
  handle. `ensure_script_venv` + `venv_bin_dir` remain exported as
  lower-level escape hatches.

### Changed
- **`hpm-cli` `run.rs`** routes its env-prep through `prepare_script_env`
  instead of its own `ensure_script_venv_for` / `prepend_path` helpers,
  so a manifest-handling change in `hpm-python` is picked up by every
  embedder without per-caller drift.

## [0.12.2] - 2026-05-13

### Added
- **`ProjectManager::new_with_auth`.** Closes the parallel gap to the
  0.12.1 `RegistrySet::from_configs_with_auth` work: `ProjectManager`
  builds its own `RegistrySet` internally inside `sync_dependencies` and
  `resolve_and_install_from_registry`, and those previously went through
  the no-auth constructors regardless of how the embedder built its own
  registry sets. For visibility-gated registries (e.g. TumbleTrove's
  `/v1/registry`), that meant a desktop pre-flight could resolve a
  PRIVATE dep correctly via an authenticated set, then `hpm install`'s
  Simple/Registry → registry-lookup branch would fire its own anonymous
  `get_version` for the same dep and 404. The new constructor stashes an
  `Option<String>` on the manager; both internal sites now build their
  `RegistrySet` via `from_configs_with_auth(..., self.auth_token.as_deref())`.
  `ProjectManager::new` becomes a one-line delegate with `None`, so
  existing callers (`hpm-cli`, every other embedder) keep working as
  anonymous. Token semantics mirror the registry variant: callers
  tracking a refreshing token rebuild the `ProjectManager` per
  operation. Static-token API on `RegistrySet` and on `ProjectManager`
  is now a matched pair.

## [0.12.1] - 2026-05-13

### Added
- **`ApiRegistry::with_auth_token` and `RegistrySet::from_configs_with_auth`.**
  Embedded callers (e.g. TumbleTrove Desktop) can now build a registry set
  that attaches `Authorization: Bearer <token>` to every API-registry
  request. Required for visibility-gated registries: server-side, the
  TumbleTrove `/v1/registry` route shows anonymous callers only PUBLIC
  packages, so an org member trying to install their own org's PRIVATE
  package previously got a 404 from `get_versions` and the package was
  silently dropped from the generated `hpm.toml`. The token header is
  marked sensitive (reqwest won't log it). Git registries ignore the
  token — there is no auth story for the git index yet. Both
  `ApiRegistry::new` and `RegistrySet::from_configs` are unchanged
  (delegate to the new entry points with `None`), so existing callers
  including the CLI keep working as anonymous.

## [0.12.0] - 2026-05-13

### Added
- **`hpm update` actually does something now.** The previous implementation
  was a placebo — `find_available_updates` synthesised version numbers
  (`format!("{}.1", current_version)`), `query_pypi_latest` returned a
  hardcoded string. Real impl: parse each registry dep's spec as
  `semver::VersionReq`, query the registry for all versions, pick the
  highest non-yanked match, compare against `hpm.lock`. Dry-run prints the
  diff (human, JSON, JSON-lines, JSON-compact). Apply rewrites the
  manifest spec to the resolved exact version and re-runs install. Honours
  `--packages` filtering, warns when a locked version has been yanked,
  prompts before applying unless `--yes`.
- **`InstallOutcome` is now part of the public `hpm-core` API.** Returned
  per-dep from `ProjectManager::sync_dependencies`, carries the install
  path plus the lockfile-relevant `checksum` / `source` (both `Option`
  because CAS short-circuits don't refetch). Library consumers that want
  to drive their own lockfile build can do so from these outcomes.
- **`RegistrySet::from_config(&Config)`** convenience constructor. Was
  previously a free function inside the CLI's `registry` command module.

### Changed
- **`ProjectManager::new` now takes an `Arc<Config>` parameter** (breaking
  change for library consumers, e.g. the TumbleTrove desktop). The
  constructor used to call `Config::load()` internally, and so did
  `resolve_and_install_from_registry` / `sync_dependencies` — so a single
  embedded "install package" operation triggered three disk reads of
  `~/.hpm/config.toml` per click, each with a `[hpm_config] Loaded user
  configuration from …` log line drowning out unrelated warnings.
  Callers now load `Config` once and share it via `Arc<Config>`; internal
  methods read from `self.config`. `hpm-cli` was updated in lockstep:
  each command loads `Config` exactly once via `load_cli_config()` and
  passes `&Config` down, eliminating the redundant second load inside
  `install` and the third load `hpm add` → `install` used to trigger.
  `Config` is now re-exported from `hpm-core`.
- **`Config::load` success path is now `debug!`, not `info!`.** Embedded
  callers that legitimately load config once per operation no longer get
  a `[hpm_config] Loaded user configuration from …` line per call. The
  malformed-config `warn!` is unchanged.
- **`ProjectManager::sync_dependencies` returns `Vec<(String,
  InstallOutcome)>`** instead of `()`. Two reasons: (1) callers (the CLI,
  the desktop client) can now build a lockfile from sync's output; (2)
  the same call site now does parallel installs internally via a
  `JoinSet` — the parallel install path was previously duplicated in
  `hpm-cli::commands::install`.
- **`hpm install` is now a thin shell over `ProjectManager`.** The
  command goes from 1118 lines to ~315: it loads + verifies the existing
  lockfile, constructs a `ProjectManager`, calls `sync_dependencies`,
  builds a fresh lockfile from the returned outcomes (backfilling
  short-circuited entries from the prior lockfile), and writes it.
  `--frozen-lockfile` semantics tightened: previously the flag just
  skipped lockfile regeneration; now `install` aborts when the
  freshly-resolved set differs from the prior lockfile.
- **`ProjectError` variants carry typed source chains.** The stringly-typed
  `DirectoryCreation(String)`, `DirectoryRead(String)`,
  `PackageInstallation(String)`, `StorageRead(String)`,
  `InvalidDependency(String)`, `PythonResolution(String)` were replaced
  with `#[from]` / `#[source]` variants: `DirectoryCreation { path,
  source: io::Error }`, `Storage(Box<StorageError>)`,
  `Fetch(Box<FetchError>)`, `InvalidPackageSource(PackageSourceError)`,
  `NoRegistriesConfigured { name, version_req }`, `RegistryResolution {
  name, version_req, source: Box<RegistryError> }`, `NoMatchingVersion {
  name, version_req }`, `PythonResolution(Box<dyn Error + Send + Sync>)`.
  Downstream `match` arms that read the old `String` payload need
  rewriting against the typed variants.
- **`hpm-cli::commands::clean` collapsed 6× duplicated control flow.**
  The dry-run / automated / interactive × packages / python /
  comprehensive matrix is now a `(Scope, Mode)` parametrisation; 401
  lines down to 240, six functions down to three.
- **`hpm init --vcs=git` propagates `git init` failures** instead of
  warning and reporting success. If the user asked for a VCS and git is
  missing or fails, the whole init fails.
- **Top-level `Cargo.toml` is now a pure workspace manifest.** Was a
  hybrid `[package]` + `[workspace]` with a stub `src/lib.rs` "documentation-
  only crate" — removed.

### Removed
- **`hpm-resolver` crate (~2,330 lines).** Claimed to be PubGrub but
  skipped conflict-driven clause learning ("`Simplified conflict
  resolution`"); zero external callers. The install path picks the
  highest matching version per package, which doesn't need a constraint
  solver. Re-add when transitive resolution becomes a real requirement.
- **`hpm publish` subcommand.** The body was a help blurb pointing users
  at the registry publishing workflow that lives elsewhere (the
  tumbletrove creator API). A command that pretends to be documentation
  is worse than no command.
- **`hpm clean --package <name>` flag.** Declared, parsed by clap, and
  tested, but `execute_clean` never read it — pure CLI fiction.
- **Fake disk-space estimates.** `"Estimated disk space freed: ~NMB"`
  was `removed.len() * 10MB` regardless of actual content.
  `ComprehensiveCleanupResult::format_total_space_*` is gone.
- **`Registry::config()` trait method + `RegistrySet::refresh_all()`.**
  Both had zero callers anywhere. The `RegistryConfig` type backing
  `config()` is also gone.
- **`.hpmref` / symlink legacy sweep.** `install.rs` used to clean up an
  earlier install layout that doesn't exist anymore.
- **`ProjectError::ConfigLoad`** and `ProjectError::PackageNotFound`
  variants. Both unused after the typed-error refactor.

### Fixed
- **Stub `hpm update` no longer lies.** Even before the real implementation
  landed, the placebo body was replaced with a `bail!` pointing users at
  the manual workaround.

## [0.11.3] - 2026-05-11

### Fixed
- **API registry no longer installs the wrong-platform archive when the
  registry returns non-canonical platform names.** `ApiRegistry::get_version`
  previously compared `builds[].platform` against the canonical long form
  (`"windows-x86_64"` / `"linux-x86_64"` / `"macos-universal"`) with strict
  string equality, then fell through to `builds.first()` when no entry
  matched. A registry that emitted the short OS form (e.g. `"WINDOWS"`,
  `"LINUX"`) would silently get an arbitrary archive — on Windows hosts
  installing multi-platform packages, the Linux `.so` build could be
  unpacked instead of the Windows `.dll` build, with no error surfaced
  until Houdini failed to load the plugins.

  The selector now (1) accepts both the canonical long form and the short
  OS form (`"windows"`/`"linux"`/`"macos"`, case-insensitive), (2) treats
  a missing `platform` field or `"universal"` (case-insensitive) as a
  universal fallback, and (3) returns a new `RegistryError::NoCompatibleBuild`
  error when every build is platform-tagged and none match the host —
  instead of silently picking the first one. Issue #3.

## [0.11.2] - 2026-05-11

Tagged but not released — the `check` workflow failed clippy on Rust 1.95
(`needless_lifetimes` lint) before any platform binaries were built. The
fix and the platform-selection change ship together in v0.11.3.

## [0.11.1] - 2026-05-08

### Fixed
- **`hpm pack` on Windows now writes ZIP entry names with `/` separators**,
  as required by the ZIP spec (APPNOTE 4.4.17.1). Previously the entry
  name went through `Path::to_string_lossy()`, which uses the OS
  separator — so packs produced on Windows contained entries like
  `config\.gitkeep`. Most consumers tolerated this, but strict ones
  (e.g. SideFX hpackage upload) rejected the archive outright. Packs
  produced on Linux/macOS were already correct.
- **`hpm pack` on Windows now correctly excludes other-platform files
  from `[native]` packages.** The platform filter compared manifest
  globs (e.g. `lib/windows-x86_64/*`) against `Path::to_string_lossy()`,
  which on Windows produced backslash-separated strings the globs could
  not match — so platform-specific archives ended up bundling files
  from every platform. Packs produced on Linux/macOS were already
  correct.
- **Cached directory checksum is now host-OS-independent.** The
  per-package digest used by `ArchiveFetcher`'s on-disk checksum cache
  hashed `Path::to_string_lossy()` of each relative entry, producing
  different digests on Windows vs. Unix for the same tree. Now
  normalized to `/`. Existing Windows caches are recomputed once on
  first access; Unix caches are unaffected.

## [0.11.0] - 2026-05-06

### Added
- **`hpm run <script> [args...]` executes `[scripts]` entries.** Sets
  `HPM_PACKAGE_ROOT` to the manifest directory, honours
  `[scripts.platform.<os>]` overrides, and forwards trailing arguments
  to the script. Replaces the previous placeholder that printed
  "not yet implemented".
- **Per-script Python venvs.** `[scripts]` entries can opt into a
  uv-managed virtual environment by switching to the table form with
  `cmd`, optional `python`, and optional `requirements`:

  ```toml
  [scripts.tt_setup]
  cmd          = "python scripts/tt_setup.py"
  python       = "3.11"
  requirements = ["PySide6>=6.6"]
  ```

  `hpm run` resolves `requirements` through the same uv pipeline that
  backs `[python_dependencies]`, materializes a content-addressable
  venv at `~/.hpm/venvs/<hash>/`, and prepends its `bin/` (or
  `Scripts/` on Windows) to `PATH` so `python` in the command resolves
  to the pinned interpreter. Two scripts whose `python` +
  `requirements` resolve to the same closure share one venv. Plain
  string entries keep their prior behaviour. Resolves [#2].

### Changed
- **`PackageScripts.commands` and `PlatformScripts.{linux,macos,windows}`
  are now `IndexMap<String, ScriptEntry>` (was `IndexMap<String, String>`).**
  `ScriptEntry` is an untagged enum: a bare string keeps the shorthand
  form, the table form carries the new `python` / `requirements`
  hints. `PackageManifest::resolved_scripts` and `script_for` return
  `ScriptEntry` values; use `.cmd()` to get the command string. Plain
  manifests are wire-compatible.

## [0.10.2] - 2026-05-05

### Fixed
- **Project Houdini version now drives Python venv ABI.** Previously HPM
  derived the target Python from each dependency package's
  `[houdini].min_version`, so a project pinned to Houdini 22 (Python 3.13)
  consuming a `min_version = "21.0"` package would silently get a 3.11
  venv — wheels resolved against 3.11 then crashed on import inside
  Houdini 22's interpreter. The project's own root-manifest
  `[houdini].min_version` is now authoritative; per-package values
  describe compatibility floors only. Two or more dependency packages
  declaring conflicting `min_version` values (e.g. 21 + 22) used to fail
  resolution outright; with a project context they now resolve cleanly
  against the project's Houdini.
- **Python resolution no longer hard-fails on machines without any
  Python installed.** `uv pip compile` requires an interpreter, and on a
  clean Windows install (no system Python, no managed CPython yet) it
  errored with `No interpreter found in virtual environments, managed
  installations, search path, or registry`. HPM now invokes
  `uv python install <version>` ahead of resolution and venv creation,
  pins `UV_PYTHON_DOWNLOADS=automatic`, and routes managed CPython
  installs into `~/.hpm/uv-python/` to keep them inside HPM's tree.

### Changed
- **`hpm_python::collect_python_dependencies` signature now takes
  `project_houdini_version: Option<&str>` as its first argument.** Pass
  the project's `[houdini].min_version` to override per-package mapping
  (recommended); pass `None` to keep the legacy per-package behaviour.

## [0.10.1] - 2026-05-05

### Added
- **`hpm-core::packer` exposes byte-based checksum and signing helpers.**
  `compute_bytes_checksum(&[u8]) -> String` and
  `sign_bytes(&[u8], &SigningKey) -> (String, String)` join the existing
  path-based `compute_archive_checksum` / `sign_archive` (which now call
  through to the byte versions). `SigningKey` is re-exported from
  `hpm_core::packer` so downstream callers don't need a direct
  `ed25519-dalek` dependency. Lets tooling that mutates archive bytes
  after pack — e.g. `tumbletrove-desktop`'s SideFX upload flow, which
  reshapes the flat `hpm pack` archive into hpackage's expected
  `{slug}.json`-at-root + content-under-`{slug}/` layout — recompute
  the SHA-256 and re-sign without round-tripping through disk.

## [0.10.0] - 2026-05-04

### Added
- **`[env]` values can now be conditional on Houdini version, OS, or
  Python.** A `value` may be either a flat string (today's case, unchanged)
  or an ordered list of `{ when, set }` variants. hpm lowers each branch
  into the expression-object array form that Houdini's `package.json`
  documents, so a single archive can ship per-major resolver builds —
  e.g. `resolver/houdini21/`, `resolver/houdini22/` — and Houdini picks
  the matching one at startup. `when` accepts `houdini` (Cargo-style req:
  `^21`, `~21.5`, `>=21,<22`, bare-major shorthand), `os` (`linux`/
  `macos`/`windows`), and `python` (`3.11`, etc.); axes combine with
  `and` and unknown keys are rejected. `$HPM_PACKAGE_ROOT` is
  substituted in each branch; any other `$VAR` passes through verbatim
  for Houdini's own expansion. Malformed selectors fail at manifest
  validation time. New types in `hpm-package`: `EnvValueSpec`,
  `EnvValueVariant`, `WhenSelector`, plus `ManifestEnvEntry::lower` as
  the single substitution-and-emit path.

### Changed
- **`hpm install` now writes to the same canonical CAS as
  `hpm sync`/`ProjectManager::sync_dependencies`.** Previously the two
  commands maintained parallel storage layouts in `~/.hpm/packages/`
  — `<safe_name>-<version>/` (install) vs `<slug>@<version>/` (sync) —
  and several latent bugs lived in the divergence. After this change:
  - `ArchiveFetcher` extracts into a staging dir at `~/.hpm/fetch/`,
    not the CAS.
  - URL/registry deps copy into `~/.hpm/packages/<slug>@<version>/` via
    `StorageManager::install_from_path`.
  - Path deps bypass the fetcher and go straight to
    `~/.hpm/packages/_dev/<slug>@<version>/` via `install_from_path_dev`,
    matching `sync_dependencies`' isolation guarantees.
  - The per-project `<.hpm>/packages/<name>` symlinks (Unix) and
    `<name>.hpmref` reference files (Windows fallback) are gone —
    Houdini JSON manifests already carry absolute CAS paths. The sweep
    introduced earlier still cleans up these legacy entries on upgrade.
- **Breaking (library API):** new `hpm_core::cas_install_dir(packages_dir,
  name, version)` returns the canonical install path
  (`<packages_dir>/<slug>@<version>`) for a lockfile dependency name.
  `LockFile::verify_checksums` now uses it; the existing
  `fetcher_install_dir` helper documents itself as the staging path.
- **Breaking (library API):** `PackageSource` is no longer an enum
  with `Url` and `Path` variants — it's a URL-only struct
  `{ url, version }` consumed exclusively by `ArchiveFetcher`. Path
  dependencies bypass the fetcher entirely now and don't need a
  `PackageSource`. The previous `PackageSource::path`,
  `PackageSource::is_url`/`is_path`/`local_path`, the `cache_key()`
  helper, and `seahash_simple` are gone. `FetchError::PathNotFound`
  is removed (no longer constructible) and `ArchiveFetcher::fetch`
  no longer takes a path branch.
  - The lockfile retains both shapes via a new
    `hpm_core::LockedSource` enum (`Url { url, version }` /
    `Path { path }`), used by `LockedDependency.source`. Same TOML
    wire format (`type = "url"` / `type = "path"`) — existing
    lockfiles continue to deserialise.

### Added
- `hpm_package::PackagePath` — validated newtype for the canonical
  `creator/slug` package identifier. Validates kebab-case at
  deserialization, so `creator()` and `slug()` return `&str` (no
  `Option`) and downstream consumers can stop defending against
  malformed paths.

### Changed
- **Breaking (library API):** `PackageInfo.path` is now
  `PackagePath` instead of `String`. `PackageManifest::new` takes
  `PackagePath`; callers can use
  `PackagePath::new("creator/slug").unwrap()` for static identifiers
  in tests, or propagate the parse error in production code.
  `PackageInfo::creator()` and `slug()` no longer return `Option`.
  The `is_valid_package_path` and `is_valid_slug` helpers on
  `PackageManifest` are gone — validation lives on `PackagePath`.
- **Breaking:** `InstalledPackage` drops three fields:
  - `name: String` (the bare-slug shadow of `manifest.package.path`).
    Use `installed.slug()` or `installed.manifest.package.slug()`.
  - `installed_at: SystemTime`. Field was set on every construction
    and read nowhere; the `metadata.created().unwrap_or_else(now)`
    fallback that masked filesystem-level errors is also gone.
  - With `name` and `installed_at` removed, the struct is now just
    `{ version, manifest, install_path }`.
- **Breaking:** `PackageSpec` drops `registry: Option<String>` and
  `PackageSpec::with_registry`. Both were only set/read in tests.
- **Breaking:** `VersionReq::new` returns `Err` for any input
  `semver::VersionReq::parse` rejects (instead of silently storing
  `parsed: None` and falling back to literal string equality in
  `matches`). `VersionReq::parse` (the alias for `new`) is removed.
- **Breaking:** Five string-typed `ProjectError` variants
  (`ManifestRead`, `ManifestParse`, `ManifestWrite`, `ManifestRemoval`,
  `JsonSerialization`) replaced with typed siblings:
  `ManifestIo { op, path, source }`, `ManifestEdit { path, source }`,
  `ManifestStructure { path, message }`, and
  `HoudiniManifestSerialize { path, source }`. The "no hpm.toml"
  pre-edit case in `update_project_manifest` now reuses
  `ManifestLoadError::NotFound`.
- `ProjectManager.fetcher` is no longer `Option<ArchiveFetcher>`.
  The "No fetcher available" arm of `ProjectError::PackageInstallation`
  is gone (it was unreachable — `fetcher` was set to `Some(...)` at
  construction).
- CLI commands `hpm search`, `hpm clean`, and `hpm update` no longer
  silently fall back to `Config::default()` when `Config::load`
  fails. `Config::load` already handles malformed user-config
  internally; these wrappers were swallowing project-config parse
  errors.

### Fixed
- `hpm update` now uses `PackageManifest::from_path` (one of the four
  spots collapsed in this release) instead of its own
  `read_to_string` + `toml::from_str` boilerplate.
- `PackageManifest::is_valid_semver` now delegates to
  `semver::Version::parse`. The prior hand-rolled split-on-`.` +
  parse-as-`u32` check rejected valid pre-release identifiers like
  `1.0.0-alpha.1` and build metadata like `1.0.0+build.5`, so
  `manifest.validate()` would fail on perfectly legitimate package
  versions. The version field on `PackageInfo` is still a `String`
  on the wire (matching what's in `hpm.toml`); this only changes
  what `validate()` accepts.
- `ProjectManager::sync_dependencies` now sweeps stale per-package Houdini
  manifests in `<project>/.hpm/packages/`. Previously, a `<slug>.json`
  written by a prior sync was left on disk after its slug dropped out of
  the dependency set (path-dep override removed, registry yank, manual
  edit), and Houdini kept loading the orphaned package on launch. Non-
  `.json` entries in the directory are not touched.
- `StorageManager::install_from_path_dev` now isolates path-installed
  ("dev") packages under `~/.hpm/packages/_dev/<slug>@<version>/` instead
  of clobbering the shared registry CAS at the same `(slug, version)`.
  `list_installed` skips the `_dev` subtree, so an `ensure_installed`
  cache lookup from another project resolving the coordinate from a
  registry can no longer be silently served the dev content. The Path
  arm of `sync_dependencies` is the only caller; registry/URL installs
  continue to use `install_from_path`.
- `hpm install` now sweeps stale entries in `<project>/.hpm/packages/`
  after writing manifests. The CLI command duplicates much of
  `sync_dependencies` and was missing the same sweep — a `<name>.json`,
  symlink, or `.hpmref` left over from a previous install kept loading
  the orphan dep on Houdini launch. Files with unrecognised extensions
  (e.g. user-authored README.md) are left alone.
- Four call sites now use `PackageManifest::from_path` instead of
  hand-rolled `read_to_string` + `toml::from_str`:
  `discovery.rs::check_project_path`,
  `manifest_utils.rs::load_manifest`,
  `install.rs::load_package_manifest`, and `pack.rs`. They get the
  same path-attached `ManifestLoadError` already used by
  `ProjectError`/`StorageError`, and `discovery.rs` drops its
  parallel `DiscoveryError::ManifestRead/ManifestParse(PathBuf, String)`
  variants in favour of `Manifest(ManifestLoadError)`.
- `LockFile::verify_checksums` now actually verifies. The previous
  implementation looked for `<packages_dir>/<name>@<version>` while
  `ArchiveFetcher` extracts to `<packages_dir>/<safe_name>-<version>`
  (where `safe_name` replaces `/` with `-`), so the inner branch
  never ran and every install / `hpm audit` reported "Package
  checksums verified" without comparing a single byte. The path
  computation now flows through a shared `fetcher_install_dir`
  helper, and a missing package surfaces as the new
  `LockError::PackageMissing` variant rather than a silent skip.

## [0.9.5] - 2026-05-01

### Added
- `hpm_core::fetch_manifest(name, version, registry_set, storage)` — fetch a
  parsed `PackageManifest` by `(name, version)` without project context. Reads
  from CAS when the package is already installed; otherwise resolves via
  `RegistrySet`, downloads via `ArchiveFetcher`, and installs into CAS as a
  side effect. Pass `""` or `"latest"` as the version to resolve the highest
  semver across configured registries. The companion `FetchManifestError`
  wraps `StorageError`/`RegistryError`/`FetchError` and is re-exported from
  `hpm-core`. Intended for tools that need to inspect a package's `[env]`,
  `[scripts]`, or `[houdini]` sections before the user installs it into a
  project.
- `hpm_package::PackageManifest::from_path(path)` constructor and a
  `ManifestLoadError` enum with `NotFound`/`Read`/`Parse` variants. Each
  variant carries the offending `PathBuf`, so a corrupted or missing
  manifest is now reported with its source path instead of a bare TOML
  diagnostic. Replaces four duplicated `read_to_string` + `toml::from_str`
  sites in `hpm-core`.

### Changed
- **Breaking (library API):** `StorageError::ManifestRead(String)` and
  `StorageError::ManifestParse(toml::de::Error)` are replaced by a single
  `StorageError::Manifest(ManifestLoadError)` variant. Same change for
  `FetchManifestError`. Match arms on the old variants need to migrate to
  `Manifest(ManifestLoadError::{NotFound, Read, Parse} { path, .. })`.
  `ProjectError` keeps its existing `ManifestRead`/`ManifestParse` string
  variants (still used by the `toml_edit`-based edit paths in
  `update_project_manifest` and `remove_from_project_manifest`) and adds
  a `Manifest(ManifestLoadError)` variant for the typed-parse path.

### Fixed
- `test_deprecated_commands` in `hpm-cli` no longer inherits the
  developer's `$HOME` and read the real `~/.hpm/config.toml`. The test
  now overrides `HOME`/`USERPROFILE` to a `TempDir` via a new
  `hpm_binary_isolated()` helper, so it passes regardless of whether the
  developer has registries configured locally.

## [0.9.4] - 2026-04-30

### Fixed
- Project sync no longer redundantly re-fetches and re-installs every
  registry dependency. The `ensure_installed` short-circuit compared the
  scoped dependency name from `hpm.toml` (e.g. `creator/slug`) against
  `InstalledPackage.name`, which only carries the bare slug, so the lookup
  never matched and every sync fell through to remove-and-recopy the CAS
  entry. Both `ensure_installed` and `ensure_installed_from_url` now route
  through `matches_spec_name`, which bridges scoped and bare forms. On
  Windows this also prevents `os error 5` aborts when a running Houdini
  process held open handles into a package directory that another project
  was about to redundantly reinstall.
- `StorageManager::install_from_path` now maps `PermissionDenied` from the
  pre-install removal step into a dedicated `StorageError::PackageInUse`
  variant with an actionable message ("close any running Houdini that
  depends on it and try again") instead of leaking a raw `os error 5`.

## [0.9.3] - 2026-04-29

### Added
- `RegistryEntry.created_at: Option<String>` (ISO 8601). Populated by API registries that emit it; git registries deserialize to `None`. Lets clients surface per-version publish timestamps.

### Removed
- `RegistryEntry.license`. The field was unused by hpm-core and never populated by any registry implementation.

## [0.9.2] - 2026-04-29

### Fixed
- `ArchiveFetcher` now extracts both ZIP and gzipped tar archives, dispatching
  on the file's leading magic bytes (`50 4B 03 04` → ZIP, `1F 8B` → tar.gz)
  rather than assuming ZIP. Previously every fetched archive was unconditionally
  read as ZIP, so a registry entry whose URL pointed at a `.tar.gz` (e.g.
  produced by `tar -czf` in package CI) failed extraction with a misleading
  "Could not find EOCD" error and blocked the entire project install. The
  download cache key no longer carries a `.zip` extension since format is
  determined by content. Path-traversal validation runs in both extractors.

## [0.9.1] - 2026-04-28

### Fixed
- `hpm pack --platform <X>` no longer drops files that are listed under
  multiple platforms in `[native.<plat>].files`. The per-platform filter
  previously assembled an exclude set from every other platform's globs
  without consulting the target's own globs, so a glob listed identically
  under all platforms (e.g. a shared install path used by every binary
  flavour) was excluded from every archive. The filter now treats the
  target's globs as an inclusion override: a path matched by both the
  target and another platform is kept in the target's archive. Distinct
  per-platform globs continue to behave as before.

## [0.9.0] - 2026-04-27

### Added
- `[env]` entries in `hpm.toml` accept `required = true`. A package can
  declare an env var without a `value` to mark it as a placeholder that
  the consuming project's `[env]` must override; `hpm install` and
  project sync error out with `MissingRequiredEnv` when no value is
  supplied (by either the package's default or the project override),
  so packages aren't silently launched without env vars they depend on.

### Changed
- `ManifestEnvEntry::value` is now `Option<String>` (was `String`) to
  support required-without-default placeholders. TOML manifests are
  backward compatible — existing `value = "..."` entries still parse —
  but Rust API consumers that read the field directly need to update.

## [0.8.2] - 2026-04-23

### Fixed
- `VenvManager::ensure_virtual_environment` now treats a venv whose
  `metadata.json` fails to deserialize as stale and rebuilds it, instead
  of propagating the parse error as a hard launch failure. This
  self-heals venvs written by pre-0.8 hpm (ISO 8601 timestamp strings)
  when launched by 0.8+ (i64 epoch seconds), without users needing to
  manually delete `~/.hpm/venvs/<hash>/`.
- `Config::load` no longer aborts when `~/.hpm/config.toml` is
  malformed. It warns and falls back to defaults, so a corrupted user
  config can be repaired with any `hpm` command instead of requiring a
  manual file edit. Project `.hpm/config.toml` still fails hard.
- `hpm install --frozen-lockfile` now fails loudly when `hpm.lock`
  exists but can't be parsed, instead of silently skipping checksum
  verification and defeating the reproducibility guarantee the flag
  promises.
- `Config::save`, `LockFile::save`, the per-package Houdini manifest
  writer, and venv `metadata.json` updates now stage writes to
  `<path>.tmp` and atomically rename into place, so a crash mid-write
  can't leave a truncated `config.toml`, `hpm.lock`,
  `.hpm/packages/*.json`, or `~/.hpm/venvs/<hash>/metadata.json`. The
  venv self-heal still covers any legacy truncation that slipped in
  before this change.

## [0.8.1] - 2026-04-23

### Fixed
- `ProjectManager::add_dependency` now resolves first-install packages
  through the configured registries (mirroring `sync_dependencies`)
  instead of calling a stub that always returned `PackageNotFound`.
  Scoped names like `creator/slug` are also matched correctly against
  already-installed packages.

### Removed
- `StorageManager::install_package` (the unimplemented stub). Callers
  should go through `ProjectManager::add_dependency` or
  `StorageManager::install_from_path`.

## [0.8.0] - 2026-04-22

### Breaking Changes
- Dropped support for Houdini 19.x (Python 3.7) and Houdini 20.0–20.4
  (Python 3.9). Both Python interpreters are past upstream end-of-life.
  `hpm install` now errors out on these versions instead of creating a
  venv against a dead ABI. The new minimum is Houdini 20.5.
- Default `[houdini].min_version` generated by `hpm init` is now `"20.5"`
  (was `"19.5"`). Same for the fallback templates in `hpm-package`.
- Removed the `hpm-error` workspace crate. Its `HpmError` type was never
  produced by any code path. Domain errors now live in their producing
  crate (`StorageError`, `ResolverError`, `ConfigError`, ...) and surface
  through `hpm_cli::error::CliError`.
- Removed the `hpm_python::update` module and its `PythonUpdateManager`
  facade. `hpm update` now calls `VenvManager::ensure_virtual_environment`
  directly after resolving dependencies. The stub methods
  (`check_python_updates`, `query_latest_python_version`,
  `link_package_to_venv`, `is_venv_used_by_other_packages`) always
  returned `Ok(false)`/`Ok(())` and are gone.
- Removed the system-`uv` fallback in `hpm_python::bundled`. HPM now
  always uses its bundled `uv`; if the bundled binary cannot be
  downloaded, `hpm` errors out instead of silently running against a
  system `uv` that may not match the expected cache layout.
- Removed `unsafe { env::set_var }` global mutation in `hpm_python::bundled`.
  UV environment variables are now applied per-command via
  `std::process::Command::env`.
- `VenvMetadata.created_at` / `last_used` and `OrphanedVenv.created_at` /
  `last_used` now serialise as Unix-epoch seconds (i64) instead of
  chrono's RFC 3339 string. Existing venv metadata files will fail to
  parse and should be removed; HPM will rebuild them on next use.
- `StorageManager::cleanup_comprehensive` now takes a `dry_run: bool`
  argument and replaces the old `cleanup_comprehensive_dry_run` method.
- Removed the unused `PythonError`, `PythonResult`, `RegistryProvider`
  and `RegistryClient` types. Custom `PackageProvider` impls no longer
  need `#[async_trait]` — the trait now uses native async-fn-in-trait.

### Added
- Support for Houdini 22.x → Python 3.13.
- `[scripts.platform.<os>]` sub-tables (`linux`/`macos`/`windows`) for
  per-OS overrides of package scripts. Top-level `[scripts]` entries
  still apply on every platform; platform-specific entries win for the
  matching host. New helpers `PackageManifest::resolved_scripts` and
  `script_for` expose a single resolution rule for desktop / CLI / CI
  consumers.

### Changed
- `map_houdini_to_python_version` error message now lists the new
  supported range (20.5+, 21, 22) and explicitly names 19.x and 20.0–20.4
  as EOL-dropped.
- `Config::load()` errors are now propagated in `ProjectManager::new`,
  `ProjectManager::sync_dependencies`, and several CLI subcommands
  (`add`, `install`, `registry …`, `search`). Previously malformed
  `config.toml` files were silently swallowed and replaced with defaults.
- `Config` deserialisation now relies on `#[serde(default)]` on every
  field instead of the `PartialConfig` → `into_config()` machinery,
  cutting ~120 lines of boilerplate.

### Security
- Picked up `rustls-webpki` 0.103.12+ (via `cargo update`) which fixes
  RUSTSEC-2026-0098 and RUSTSEC-2026-0099 (name-constraint bypass).

### Removed (dependency cleanup)
- Dropped the following direct/indirect dependencies: `miette`,
  `owo-colors`, `anstream`, `futures`, `dirs`, `home`, `which`, `chrono`,
  `async-trait` (from `hpm-resolver`; retained in `hpm-core` where
  `Box<dyn Registry>` still needs it).
- Transitive crate count went from 377 to 345.

## [0.7.2] - 2026-04-19

### Fixed
- `VenvManager::ensure_virtual_environment` now self-heals venvs left
  half-installed by earlier hpm versions. Previously, if a pre-0.7.1 run had
  created a venv directory whose `metadata.json` claimed the packages were
  installed but whose `site-packages/` was empty (the `--target` bug),
  upgrading to 0.7.1 wasn't enough — `ensure_virtual_environment` trusted
  the existing directory and skipped the install. It now checks that each
  resolved package has a `dist-info` in `site-packages/` before reusing the
  venv, and deletes + rebuilds when the check fails.

## [0.7.1] - 2026-04-19

### Fixed
- `hpm install` now actually installs Python dependencies into the shared
  venv. The installer had been running `uv pip install --target
  {venv}/lib/python/site-packages` — a path no real venv uses — so uv planted
  files where the venv's own interpreter never looked, leaving
  `~/.hpm/venvs/<hash>/Lib/site-packages` empty while metadata.json claimed
  success. Installs now use `uv pip install --python {venv}/bin/python` (or
  `Scripts/python.exe` on Windows) and verify a `dist-info` directory lands
  for at least one resolved package before writing metadata.
- `VenvManager::get_python_site_packages_path` returns the real per-version
  Unix layout (`lib/pythonX.Y/site-packages`) instead of the fictional
  `lib/python/site-packages`, so generated `PYTHONPATH` entries point at a
  directory Python will actually import from. Callers pass the resolved
  Python version through.

### Changed
- Updated `docs/user-guide.md` and `docs/architecture.md` to match the
  current Houdini manifest shape (`hpath` + `HoudiniEnvValue` prepend) and
  the real venv layout. The old examples showed `hpm_managed`/`hpm_package`
  fields and a `generate_houdini_manifest` function that were removed in
  0.7.0.

## [0.7.0] - 2026-04-19

### Breaking Changes
- `hpm install` now errors out when `[houdini]` `min_version` is unparseable or
  outside the supported range (Houdini 19/20/21) instead of silently falling
  back to Python 3.9. This previously masked the Houdini-21 mapping bug below.

### Fixed
- Houdini 21 with `min_version = "21"` (bare major) now correctly resolves to
  Python 3.11. Before, a bare major fell through to the default arm and
  produced a Python 3.9 venv, so C-extension packages (pymongo, watchdog, etc.)
  couldn't load under Houdini's Python 3.11 ABI.
- `hpm install` now writes per-package Houdini manifests to
  `.hpm/packages/{name}.json` with the shared venv's `site-packages` prepended
  onto `PYTHONPATH`. The previous implementation built the config in memory
  and discarded it, so `import qtpy` (and any other declared Python
  dependency) failed inside Houdini despite a successful install.
- Generated `PYTHONPATH` entries use `HoudiniEnvValue::prepend` and let Houdini
  pick the path separator, fixing the hardcoded Unix-only `:` / `$PYTHONPATH`
  that emitted malformed values on Windows.

### Changed
- `VenvManager::with_venvs_dir` and `PythonCleanupAnalyzer::with_venv_manager`
  let callers (primarily tests) route at an isolated venvs directory instead
  of the developer's real `~/.hpm/venvs/`. Flaky `test_end_to_end_python_workflow`
  and `test_cleanup_system_comprehensive` now use tempdirs.

### Removed
- `hpm_python::integration` module (`generate_houdini_package_json`,
  `update_package_json_with_python`, `extract_python_env_from_package_json`).
  The module produced a non-Houdini JSON shape and was only exercised by its
  own tests; Houdini manifest generation now lives in the install command.

## [0.6.0] - 2026-04-16

### Breaking Changes
- `hpm pack --key` (and `HPM_SIGNING_KEY`, and `signing.key_path` in global config) now expects a PKCS#8 PEM file instead of a 32-byte raw seed. Regenerate keys with `openssl genpkey -algorithm ed25519 -out signing.pem`.

### Added
- `HPM_SIGNING_KEY` accepts inline PEM content (detected by a leading `-----BEGIN` marker) in addition to a file path, so CI secret stores can inject the key as a plain string without writing a temp file.
- Documented the package-signing wire format in `docs/security.md`: Ed25519 over the archive bytes, signature emitted as standard base64 (RFC 4648), `keyId` = first 8 bytes of the public key hex-encoded.

## [0.5.2] - 2026-04-10

### Fixed
- Generated per-package Houdini manifest `hpath` now points at the package root instead of `<root>/otls`, so Houdini auto-discovers convention subdirectories (`desktop/`, `toolbar/`, `radialmenu/`, `python_panels/`, `viewer_states/`, `python3.11libs/pythonrc.py`, `keymaps`, etc.) instead of only loading HDAs

## [0.5.1] - 2026-04-05

### Fixed
- Flaky CLI tests that mutated the process-wide current directory now serialize via a shared mutex, eliminating races under parallel test execution

## [0.5.0] - 2026-04-05

### Added
- `hpm pack` now auto-generates a Houdini-native `{slug}.json` in the archive if one doesn't already exist, making HPM packages directly usable by Houdini's built-in package system
- `HoudiniNativePackage` and `HpackageMetadata` types for representing Houdini-native package metadata

## [0.4.0] - 2026-03-31

### Breaking Changes
- Package identity is now a scoped path (`creator/slug`) instead of a flat name
- `PackageInfo` has a new required `path` field and `name` is now a freeform display name
- `PackageManifest::new()` requires a `path` parameter
- `PackageTemplate::new()` no longer takes a `name` parameter
- Existing `hpm.toml` files without a `path` field will fail validation

### Added
- Scoped package paths: packages are identified by `creator/slug` (e.g. `tumblehead/tumble-rig`)
- Version-qualified paths use `@`: `creator/slug@1.0.0`
- `PackageInfo::identifier()`, `creator()`, `slug()` helper methods
- `PackageManifest::is_valid_package_path()` and `is_valid_slug()` validation
- Git registry index supports scoped paths (`creator/slug.json` layout)
- API registry encodes scoped path segments individually in URLs
- Storage supports nested `creator/slug@version/` directory layout

### Changed
- Dependencies in `hpm.toml` use scoped paths as keys: `"creator/slug" = "1.0.0"`
- Archive and cache filenames replace `/` with `-` for flat naming
- `hpm init` generates both `path` and `name` fields in `hpm.toml`

## [0.3.2] - 2026-03-26

### Fixed
- Handle platform-specific builds in registry `get_version`
- Make asset uploads idempotent, improve unwrap safety

## [0.3.0] - 2026-03-25

### Added
- `[native]` section in `hpm.toml` for declaring platform-specific files
- `hpm pack --platform` flag for producing per-platform archives
- `Platform` type with support for `linux-x86_64`, `macos-universal`, `windows-x86_64`
- Auto-detection of host platform when packing native packages
- `platform` field on registry entries for future install-time platform selection

## [0.1.0] - Initial Release

### Package Management
- `hpm init` - Package initialization with standard and bare templates
- `hpm add` - Add dependencies (registry, path sources) with version specifications
- `hpm remove` - Remove dependencies from manifest
- `hpm install` - Install all dependencies with lock file support (`--frozen-lockfile` for CI)
- `hpm update` - Update dependencies to latest compatible versions
- `hpm list` - List dependencies with tree view
- `hpm check` - Validate package configuration
- `hpm pack` - Create signed package archives
- `hpm clean` - Remove orphaned packages and virtual environments
- `hpm audit` - Security audit on dependencies
- `hpm completions` - Shell completion generation (bash, zsh, fish, powershell)

### Dependency Resolution
- PubGrub-based resolver with conflict learning and backtracking
- Lock file (`hpm.lock`) with pinned versions and checksums
- Registry, path, and URL dependency sources

### Python Integration
- Virtual environment isolation with content-addressable sharing
- UV-powered dependency resolution
- Automatic Python version mapping based on Houdini version
- PYTHONPATH injection via generated Houdini `package.json`

### Storage
- Global package storage in `~/.hpm/packages/`
- Content-addressable Python venvs in `~/.hpm/venvs/`
- Project-aware cleanup with orphan detection

### Security
- SHA-256 checksums for package verification
- Ed25519 package signing and verification
- Pure Rust TLS (rustls) for all network communication
