# User Guide

HPM is a Rust-based package manager for SideFX Houdini. It manages both HPM
packages (Houdini tools, HDAs, scripts, shelf tools, toolbars, etc.) and the
Python dependencies those packages need, and it produces the Houdini
`package.json` files that let Houdini find them at launch.

This guide covers installation, the command surface, the `hpm.toml` manifest,
global configuration, and troubleshooting.

## Table of contents

1. [Install](#install)
2. [First package](#first-package)
3. [Registries](#registries)
4. [Command reference](#command-reference)
5. [The `hpm.toml` manifest](#the-hpmtoml-manifest)
6. [Global configuration](#global-configuration)
7. [Storage layout](#storage-layout)
8. [Houdini integration](#houdini-integration)
9. [Output formats and automation](#output-formats-and-automation)
10. [Troubleshooting](#troubleshooting)

## Install

### Prerequisites

- SideFX Houdini 20.5, 21.x, or 22.x (for integration — HPM itself runs fine without it).
- Rust 1.85+ if building from source.
- Git (optional; used by `hpm init --vcs git`).

### From a pre-built binary

Download the binary for your platform from the
[latest release](https://github.com/3db-dk/hpm/releases/latest) and put it on
your `PATH`. Verify with:

```sh
hpm --version
```

### From source

```sh
git clone https://github.com/3db-dk/hpm.git
cd hpm
cargo build --release
# binary: target/release/hpm
```

### Shell completions

```sh
# Bash (add to ~/.bashrc)
eval "$(hpm completions bash)"

# Zsh (add to ~/.zshrc)
eval "$(hpm completions zsh)"

# Fish
hpm completions fish | source

# PowerShell (add to $PROFILE)
hpm completions powershell | Out-String | Invoke-Expression
```

Supported shells: `bash`, `zsh`, `fish`, `powershell`, `elvish`.

## First package

```sh
hpm init my-first-package --description "My first Houdini package"
cd my-first-package
```

The standard template creates this layout:

```
my-first-package/
├── hpm.toml           # HPM manifest
├── README.md
├── otls/              # HDAs / .otl files
├── python/            # Python modules (python/__init__.py is pre-created)
├── scripts/           # Shelf tools and script hooks
├── presets/           # Node presets
├── config/            # Configuration files
└── tests/             # Test files
```

Pass `--bare` for just `hpm.toml` and `README.md` — use this when you have a
custom layout or are wrapping an existing codebase.

### A typical workflow

```sh
hpm init my-tools                        # 1. scaffold
hpm add some-creator/utility-nodes@1.0.0 # 2. add deps
hpm add local-tools --path ../local-tools
hpm install                              # 3. resolve & install
hpm check                                # 4. sanity check
hpm list --tree                          # 5. inspect
```

Then wire Houdini up — see [Houdini integration](#houdini-integration).

## Registries

HPM resolves package names through one or more **registries**. A registry is
either an HTTP API endpoint or a Git repository serving a Cargo-style index.
Without at least one registry configured, `hpm search` and `hpm add <name>@<version>`
have nowhere to look.

```sh
# API registry (auto-detected by URL)
hpm registry add https://api.3db.dk/v1/registry --name houdinihub

# Git-index registry (auto-detected by .git suffix or host)
hpm registry add https://github.com/studio/hpm-packages.git --name studio --type git

hpm registry list
hpm registry update      # refresh caches
hpm registry remove studio
```

Global registries live in `~/.hpm/config.toml` under `[[registries]]`. A
project can also declare registries in its own `hpm.toml` under
`[[registries]]` — handy for studios that want each project to pin the
registries it resolves against. See [`[[registries]]`](#registries-array) below.

A dependency can target a specific registry:

```toml
[dependencies]
studio-tools = { version = "1.0.0", registry = "studio" }
```

Without `registry`, HPM resolves through every configured registry in order.

## Command reference

### Global options

Available on every command:

| Option | Description |
|--------|-------------|
| `-v, --verbose` | Increase verbosity (repeat for more detail). |
| `-q, --quiet` | Suppress non-error output. |
| `--color <when>` | `auto`, `always`, or `never`. |
| `--output <format>` | `human` (default), `json`, `json-lines`, `json-compact`. |
| `-C, --directory <dir>` | Run as if invoked from `<dir>`. |

### `hpm init`

Create a new HPM package.

```
hpm init [OPTIONS] [NAME]
```

**Options**

| Flag | Default | Description |
|------|---------|-------------|
| `--description <text>` | — | Package description. |
| `--author <name>` | `git config user.*` if set | Author (`"Name <email>"`). |
| `--version <v>` | `0.1.0` | Initial version. |
| `--license <id>` | `MIT` | License identifier. |
| `--houdini-min <v>` | `20.5` | Minimum Houdini version. |
| `--houdini-max <v>` | — | Maximum Houdini version. |
| `--bare` | off | Skip standard directories; create only `hpm.toml` and `README.md`. |
| `--vcs <vcs>` | `git` | `git` or `none`. |

**Examples**

```sh
hpm init my-tools
hpm init --bare minimal
hpm init advanced-tools \
  --description "Advanced geometry tools" \
  --author "Artist <artist@studio.com>" \
  --license Apache-2.0 \
  --houdini-min 20.5 --houdini-max 21.0
```

### `hpm add`

Add one or more dependencies to `hpm.toml`.

```
hpm add [OPTIONS] <PACKAGE>...
```

`<PACKAGE>` is either a bare name (resolved from registries at install time)
or `name@version`. `name` uses the `creator/slug` form.

**Options**

| Flag | Description |
|------|-------------|
| `--path <dir>` | Add as a local path dependency (only valid with a single package). |
| `-p, --package <path>` | Path to the manifest to modify (`hpm.toml` or containing dir). Defaults to cwd. |
| `--optional` | Mark all added dependencies as optional. |

**Examples**

```sh
hpm add studio/utility-nodes@1.0.0
hpm add studio/a@1.0.0 studio/b@2.0.0
hpm add local-tools --path ../local-tools
hpm add studio/visualize@1.0.0 --optional
hpm add studio/lib@1.0.0 -p /path/to/project
```

### `hpm remove`

Remove a dependency from `hpm.toml`. This does **not** delete package files
from `~/.hpm/packages/` — run `hpm clean` for that.

```
hpm remove [OPTIONS] <PACKAGE>
```

| Flag | Description |
|------|-------------|
| `-p, --package <path>` | Manifest to modify. |

### `hpm install`

Resolve and install every dependency declared in `hpm.toml`.

```
hpm install [OPTIONS]
```

Install does the following:

1. Loads `hpm.toml`.
2. If `hpm.lock` exists, verifies cached packages against stored checksums and warns if the lock is older than 90 days.
3. Resolves HPM dependencies through configured registries and downloads anything missing to `~/.hpm/packages/`.
4. Collects Python dependencies from the root manifest and every installed dependency's manifest, resolves them with the bundled `uv`, and installs them into a content-addressable venv in `~/.hpm/venvs/<hash>/`.
5. Writes one Houdini manifest per installed dependency to `<project>/.hpm/packages/{name}.json`.
6. Writes or updates `hpm.lock`.

**Options**

| Flag | Description |
|------|-------------|
| `-m, --manifest <path>` | Path to `hpm.toml` (or its containing directory). |
| `--frozen-lockfile` | Fail if `hpm.lock` is missing or would need to change. Use in CI. |

### `hpm update`

Update dependencies to their latest compatible versions.

```
hpm update [OPTIONS] [PACKAGES]...
```

With no packages, updates all. With specific packages, updates only those.

| Flag | Description |
|------|-------------|
| `-p, --package <path>` | Manifest to operate on. |
| `--dry-run` | Print the proposed plan without applying it. |
| `-y, --yes` | Skip the confirmation prompt. |

**Examples**

```sh
hpm update --dry-run
hpm update
hpm update studio/geometry-tools
hpm update --yes --output json      # CI-friendly
```

### `hpm list`

Display installed dependencies and their metadata.

```
hpm list [OPTIONS]
```

| Flag | Description |
|------|-------------|
| `-p, --package <path>` | Manifest to read. |
| `--tree` | Render dependencies as a tree. |

### `hpm check`

Validate `hpm.toml` and the surrounding project.

```
hpm check
```

Check runs:

- `hpm.toml` exists, parses, and passes manifest validation (scoped `creator/slug` path, semver version, `[native]` consistency).
- Houdini version constraints are well-formed and `min_version <= max_version`.
- Generated Houdini `package.json` serializes cleanly.
- Soft warnings for: missing description, missing authors, missing keywords, missing `[houdini]`, missing README or license file, missing `.gitignore` when a `.git` directory is present, packages larger than 100 MB, and individual files larger than 10 MB.

Check is advisory — warnings do not fail the command.

### `hpm search`

Search every configured registry for packages matching a query.

```
hpm search <QUERY>
```

If no registries are configured, HPM prints instructions to add one.

### `hpm pack`

Build a distributable archive from the current package.

```
hpm pack [OPTIONS]
```

Pack runs `hpm check` first, then:

1. Auto-generates a Houdini-native `{slug}.json` inside the archive unless the user has provided one. This file follows Houdini's own package format, so the archive is usable by Houdini even without HPM.
2. Filters files by `[native]` platform when the manifest has a `[native]` section.
3. Produces a `.zip` archive plus a SHA-256 checksum.
4. If a signing key is supplied, produces an Ed25519 signature over the archive bytes and emits a `keyId`.

**Options**

| Flag | Description |
|------|-------------|
| `--key <path>` | Ed25519 PKCS#8 PEM private key. Overrides `HPM_SIGNING_KEY`. |
| `--output <dir>` | Output directory. Defaults to the current directory. |
| `--json` | Emit the result as JSON (useful in CI). |
| `--platform <id>` | Override host-platform detection. Valid: `linux-x86_64`, `macos-universal`, `windows-x86_64`. Only legal when `[native]` is declared. |

**Signing key resolution order**

1. `--key <path>` (CLI flag).
2. `HPM_SIGNING_KEY` environment variable. If its value starts with `-----BEGIN`, it's treated as inline PEM; otherwise it's a path.
3. `[signing].key_path` in `~/.hpm/config.toml`.

**Generating a signing key**

```sh
openssl genpkey -algorithm ed25519 -out signing.pem
openssl pkey -in signing.pem -pubout -out signing.pub.pem
```

Keep `signing.pem` secret. Publish `signing.pub.pem` so consumers can verify.
See [Security](security.md#package-signing) for the wire format.

### `hpm audit`

Run a security audit on the current project.

```
hpm audit [OPTIONS]
```

| Flag | Description |
|------|-------------|
| `-m, --manifest <path>` | Manifest to audit. |

Audit checks:

- **HTTP URLs** — flags any `url = ...` dependency whose URL is `http://` rather than `https://`.
- **Lock file presence** — warns if `hpm.lock` is missing.
- **Lock file staleness** — warns if `hpm.lock` is older than 90 days.
- **Checksum verification** — verifies every cached package in `~/.hpm/packages/` matches the checksum stored in `hpm.lock`.

See [Security](security.md) for more.

### `hpm clean`

Remove orphaned packages and/or venvs that no active project depends on.

```
hpm clean [OPTIONS]
```

| Flag | Description |
|------|-------------|
| `-n, --dry-run` | Print what would be removed, without touching anything. |
| `-y, --yes` | Skip confirmation. |
| `--package <pattern>` | Target specific package patterns. |
| `--python-only` | Clean only orphaned venvs. |
| `--comprehensive` | Clean packages **and** venvs. |

HPM identifies active projects via `[projects]` in `~/.hpm/config.toml`
(`explicit_paths` plus recursive `search_roots`). Anything reachable from
those projects is preserved; the rest is a candidate for removal.

### `hpm registry`

Manage registries in `~/.hpm/config.toml`. See [Registries](#registries).

```
hpm registry add <URL> [--name <alias>] [--type api|git]
hpm registry list
hpm registry remove <NAME>
hpm registry update
```

If `--type` is omitted, HPM infers it: URLs ending in `.git` or hosted on
`github.com` / `gitea.*` are treated as `git`; everything else is `api`.

### `hpm completions`

Emit shell completion scripts — see [Install](#install).

## The `hpm.toml` manifest

A minimal manifest:

```toml
[package]
path = "my-studio/my-tools"
name = "My Tools"
version = "1.0.0"
```

All sections, in the order they appear in practice:

### `[package]`

| Field | Required | Description |
|-------|----------|-------------|
| `path` | yes | Scoped identifier, `creator/slug`. Both segments must be kebab-case (lowercase letters, digits, hyphens). Example: `tumblehead/tumble-rig`. |
| `name` | yes | Freeform display name. |
| `version` | yes | Semantic version, `major.minor.patch`. |
| `description` | no | Short description. |
| `authors` | no | List of `"Name <email>"` strings. |
| `license` | no | License identifier (e.g. `MIT`, `Apache-2.0`). |
| `readme` | no | Path to README, relative to the package. Defaults to `README.md` for `init`-generated packages. |
| `homepage` | no | Project homepage URL. |
| `repository` | no | Repository URL. |
| `documentation` | no | Documentation URL. |
| `keywords` | no | List of keywords for discovery. |
| `categories` | no | List of categories. |

### `[houdini]`

Houdini version constraints:

```toml
[houdini]
min_version = "20.5"
max_version = "21.0"   # optional
```

Both fields accept `"major"` (e.g. `"21"`) or `"major.minor"` (e.g. `"21.0"`).
`min_version` drives the bundled Python version — see [Python guide](python-guide.md).

### `[dependencies]`

HPM package dependencies. Keys are scoped `creator/slug` paths. Values take
one of four shapes:

```toml
[dependencies]

# 1. Registry, version-only (shorthand)
"studio/utility-nodes" = "1.0.0"

# 2. Registry with options
"studio/material-library" = { version = "2.0.0", optional = true }
"studio/internal-tool" = { version = "1.0.0", registry = "studio" }

# 3. Direct URL (pre-built archive — ZIP or gzipped tar both accepted)
"studio/prebuilt" = { url = "https://pkg.example.com/prebuilt-1.0.0.zip", version = "1.0.0" }

# 4. Local path (development)
local-tools = { path = "../local-tools" }
local-tools = { path = "../local-tools", optional = true }
```

The lock file (`hpm.lock`) records the resolved version and SHA-256 checksum
for each dependency, so subsequent installs are reproducible and tamper-evident.

### `[python_dependencies]`

Python packages, installed through the bundled `uv` into a shared venv:

```toml
[python_dependencies]

# Version constraint shorthand
numpy = ">=1.20.0"

# Detailed form
requests = { version = ">=2.25.0", extras = ["security", "socks"] }
matplotlib = { version = "^3.5.0", optional = true }
```

Version constraints use PEP 440 syntax (same as pip/uv). See
[Python guide](python-guide.md) for the Houdini→Python version mapping and
venv sharing behavior.

### `[env]`

Environment variables to set when Houdini loads the package. The key is the
variable name; the value is a `{ method, value }` pair:

```toml
[env]
MY_PLUGIN_ROOT = { method = "set", value = "$HPM_PACKAGE_ROOT/config" }
HOUDINI_TOOLBAR_PATH = { method = "prepend", value = "$HPM_PACKAGE_ROOT/toolbar" }
HOUDINI_AUDIT_LOG = { method = "append", value = "$HPM_PACKAGE_ROOT/logs/audit" }
```

| Method | Effect |
|--------|--------|
| `set` | Replace the variable. |
| `prepend` | Prepend to the existing variable (Houdini picks the platform separator). |
| `append` | Append to the existing variable. |

Use `$HPM_PACKAGE_ROOT` to refer to the installed package directory. HPM
merges these entries with its built-in `PYTHONPATH` and `HOUDINI_SCRIPT_PATH`
entries when generating the Houdini manifest.

#### Required env vars

A package can declare an env var as required without giving it a value. Any
project that depends on the package must then supply the value in its own
`[env]` section in `hpm.toml`. `hpm install` (and project sync) errors out
otherwise — the package isn't launchable without it.

```toml
# In the package's hpm.toml
[env]
PROJECT_ASSETS = { method = "set", required = true }
```

```toml
# In the consuming project's hpm.toml
[env]
PROJECT_ASSETS = { method = "set", value = "/mnt/studio/assets" }
```

`required = true` may be combined with a `value`; the value then acts as a
default and the project override becomes optional. Without a value, the entry
is a hard placeholder.

A consuming project can also override any package-declared env var by
re-declaring the same key in its own `[env]` — the project's entry wins.

### `[native]`

Declare that this package ships per-platform binaries (HDK plugins, shared
libraries). When `[native]` is present, `hpm pack` produces a slim, per-platform
archive that includes only the files relevant to the target platform.

```toml
[native]
platforms = ["linux-x86_64", "macos-universal", "windows-x86_64"]

[native.linux-x86_64]
files = ["lib/linux-x86_64/*"]

[native.macos-universal]
files = ["lib/macos-universal/*"]

[native.windows-x86_64]
files = ["lib/windows-x86_64/*"]
```

Valid platform identifiers: `linux-x86_64`, `macos-universal`, `windows-x86_64`.
`files` uses glob patterns relative to the package root. `hpm pack`
auto-detects the host platform; pass `--platform <id>` to target a different one.

**Per-platform filter semantics.** When packing for `--platform <X>`:

- A path matched by `[native.X].files` is included.
- A path matched only by `[native.Y].files` (some `Y != X`) is excluded.
- A path matched by both `[native.X].files` and some `[native.Y].files` is
  included — the target's claim wins.
- A path matched by no `[native.*].files` glob is included as common content.

This means listing the same glob under every platform is a valid way to
declare "this content ships in every per-platform archive" (e.g. a shared
install path that all platforms use).

### `[[registries]]` <a id="registries-array"></a>

Per-project registries. Same shape as the global version in `~/.hpm/config.toml`:

```toml
[[registries]]
name = "houdinihub"
url = "https://api.3db.dk/v1/registry"
type = "api"

[[registries]]
name = "studio"
url = "https://github.com/studio/hpm-packages.git"
type = "git"
```

Project registries are additive to global registries.

### `[scripts]`

Named scripts for the package. Reserved for a future `hpm run` command —
currently parsed and validated by `hpm check` but not executed.

```toml
[scripts]
build = "python scripts/build.py"
test = "python -m pytest tests/"
```

Entries under `[scripts]` apply on every platform. For scripts whose
command differs per OS (for example, calling a `.exe` on Windows but an
extensionless binary elsewhere), add a `[scripts.platform.<os>]` sub-table.
Valid OS keys are `linux`, `macos`, and `windows`; a platform-specific
entry wins over the top-level one on the matching host, and the top-level
entry is used as a fallback on OSes that aren't listed.

```toml
[scripts]
build = "cargo build"                        # runs on any platform

[scripts.platform.windows]
register   = "\"$HPM_PACKAGE_ROOT/plugin/bin/tool.exe\" register"
unregister = "\"$HPM_PACKAGE_ROOT/plugin/bin/tool.exe\" unregister"

[scripts.platform.macos]
register = "\"$HPM_PACKAGE_ROOT/plugin/bin/tool\" register"
```

Consumers resolve scripts through `PackageManifest::resolved_scripts(platform)`
(all entries for the given host, merged) or `script_for(name, platform)`
(single lookup). A script that is only defined under `[scripts.platform.*]`
is simply absent on OSes without an entry — UIs can use this to hide
menu items rather than fail at runtime.

## Global configuration

HPM reads `~/.hpm/config.toml` if it exists, then `<cwd>/.hpm/config.toml`
(project override) if it exists. Any missing sections fall back to defaults.

```toml
[install]
path = "packages/hpm"        # relative install path inside projects
parallel_downloads = 8

[storage]
home_dir = "/Users/me/.hpm"                # default: $HOME/.hpm
cache_dir = "/Users/me/.hpm/cache"
packages_dir = "/Users/me/.hpm/packages"
registry_cache_dir = "/Users/me/.hpm/registry"

[projects]
explicit_paths = [
    "/Users/me/studio/pipeline",
]
search_roots = [
    "/Users/me/houdini-projects",
]
max_search_depth = 3
ignore_patterns = [".git", ".hg", ".svn", "node_modules", "backup", "archive", ".cache", "temp", "tmp"]

[[registries]]
name = "houdinihub"
url = "https://api.3db.dk/v1/registry"
type = "api"

[signing]
key_path = "/Users/me/.hpm/signing.pem"    # fallback for `hpm pack`
```

### What each section controls

**`[install]`**
- `path` — the directory inside a project where `hpm install` writes the per-dependency Houdini manifests. Under the hood this is also where `.hpm/packages` is resolved; the default `packages/hpm` rarely needs changing.
- `parallel_downloads` — maximum concurrent downloads from registries (default `8`).

**`[storage]`**
- `home_dir` — HPM's root on disk. Default `$HOME/.hpm` on every platform (Linux, macOS, Windows). All other storage paths derive from this by default.
- `cache_dir`, `packages_dir`, `registry_cache_dir` — override individual subdirectories without moving the whole root.

**`[projects]`** — drives `hpm clean`'s orphan detection.
- `explicit_paths` — absolute project paths that are always considered active.
- `search_roots` — directories scanned recursively for active projects.
- `max_search_depth` — recursion limit for search_roots (default `3`).
- `ignore_patterns` — directory names/prefixes to skip during scanning. Matches full names or prefixes, not globs.

**`[[registries]]`** — list of registries. Same shape as
[`[[registries]]`](#registries-array) in `hpm.toml`.

**`[signing]`**
- `key_path` — fallback Ed25519 PKCS#8 PEM path for `hpm pack` when neither `--key` nor `HPM_SIGNING_KEY` is set.

## Storage layout

HPM stores everything under `~/.hpm/` on every supported platform. Use
`[storage]` to change the root or individual subdirectories.

```
~/.hpm/
├── config.toml                      # global configuration (optional)
├── packages/                        # extracted packages (global dedupe)
│   └── creator/
│       └── slug@1.0.0/
├── venvs/                           # content-addressable Python venvs
│   └── a1b2c3d4e5f6/
│       ├── pyvenv.cfg
│       ├── lib/python3.11/site-packages/   # Lib\site-packages on Windows
│       └── metadata.json
├── cache/                           # download cache
├── registry/                        # registry index caches (one dir per registry)
├── tools/                           # bundled uv binary
├── uv-cache/                        # isolated uv cache (never touches your system uv)
├── uv-config/                       # isolated uv config
└── logs/                            # operational logs
```

Per-project layout:

```
<project>/
├── hpm.toml
├── hpm.lock                         # pinned versions + checksums
├── .hpm/
│   ├── config.toml                  # project-level overrides (optional)
│   └── packages/                    # Houdini manifests, one per dependency
│       ├── utility-nodes.json
│       └── material-library.json
└── (your package sources)
```

## Houdini integration

`hpm install` writes one Houdini `package.json` per dependency into
`<project>/.hpm/packages/{name}.json`. Each file points `hpath` at the
absolute location of the extracted package in `~/.hpm/packages/` and, for
packages that declare `[python_dependencies]`, prepends the shared venv's
`site-packages` onto `PYTHONPATH`:

```json
{
  "hpath": ["/Users/me/.hpm/packages/studio/utility-nodes@1.0.0"],
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

`method: "prepend"` delegates path-separator handling to Houdini, so the same
manifest works on Unix (`:`) and Windows (`;`) without HPM embedding an
OS-specific joiner.

To make Houdini pick these up, add `<project>/.hpm/packages` to
`HOUDINI_PACKAGE_PATH`. For a studio-wide setup, set it in the shell or in
your DCC launcher; for a one-off project, set it when launching Houdini:

```sh
HOUDINI_PACKAGE_PATH="$PWD/.hpm/packages:$HOUDINI_PACKAGE_PATH" houdini
```

`hpath` points directly at the extracted package root, so Houdini auto-discovers
its convention subdirectories (`otls/`, `desktop/`, `toolbar/`, `python_panels/`,
`viewer_states/`, `python3.11libs/pythonrc.py`, `keymaps/`, …).

## Output formats and automation

All commands emit human-readable output by default. The `--output` global
flag selects a machine-readable format instead:

| Format | When to use |
|--------|-------------|
| `human` | Default. Colored, styled for terminal use. |
| `json` | Pretty-printed JSON. |
| `json-lines` | One JSON object per line. Good for streaming and log ingestion. |
| `json-compact` | Single-line JSON. Minimal bandwidth. |

Errors in any machine-readable format are also emitted as JSON, with fields
`success`, `error`, `error_type`, and `elapsed_ms`.

A typical CI recipe:

```sh
set -e
hpm install --frozen-lockfile                 # fail if lock is stale
hpm audit                                       # warn on security issues
hpm pack --json --output dist/                  # produce archive + manifest
```

`hpm update --dry-run --output json` is useful for nightly jobs that want to
detect available updates without applying them.

## Troubleshooting

### Package not found

```
Error: Package error: Package 'studio/foo' not found
```

Check `hpm registry list` — if it's empty, add one with `hpm registry add`.
If registries are configured, run `hpm registry update` and try again.

### Houdini version mapping failed

```
Error: No Python version mapping for Houdini 22; supported majors are 19, 20, 21.
```

As of 0.7.0, an unsupported `[houdini].min_version` is a hard error rather
than a silent fallback. Update `hpm-python` if you need to add a new major,
or set `min_version` to a supported value.

### Checksum mismatch at install time

```
Error: Package integrity check failed: ...
```

Means the cached package in `~/.hpm/packages/` no longer matches the
checksum recorded in `hpm.lock`. Either someone tampered with the cache, or
the cache predates a lock-file rewrite. Remove the offending directory
under `~/.hpm/packages/` and run `hpm install` again.

### Lock file is stale

```
Lock file is 120 days old. Consider running 'hpm update' to check for newer versions.
```

Advisory warning. HPM will still install; `hpm update` refreshes the lock.

### `--frozen-lockfile` fails in CI

The lock file either doesn't exist yet or would need to change. If this is a
fresh project, run `hpm install` locally, commit `hpm.lock`, and retry. If
the project already has a lock and CI still fails, a dependency's resolution
has drifted — review the diff from `hpm update --dry-run`.

### Python packages aren't importable inside Houdini

Check that:

1. `HOUDINI_PACKAGE_PATH` includes `<project>/.hpm/packages`.
2. The generated `.hpm/packages/{name}.json` has a `PYTHONPATH` entry for the offending package.
3. The venv directory it points to exists and contains `site-packages/`. If it doesn't, upgrade past 0.7.2 — earlier versions had a bug where the venv's `site-packages` was empty despite a successful install.

Restart Houdini after any change to `HOUDINI_PACKAGE_PATH`.

### Debug logging

```sh
RUST_LOG=debug hpm install
RUST_LOG=hpm_core=debug,hpm_python=trace hpm install    # per-module
```

### Resetting state

```sh
# per-project reset
rm -rf .hpm/ hpm.lock
hpm install

# global reset (last resort)
rm -rf ~/.hpm/
hpm install                  # will re-download everything
```

### Getting help

- `hpm --help`, `hpm <command> --help`.
- Report bugs at <https://github.com/3db-dk/hpm/issues>.
- The [Python guide](python-guide.md) covers venvs, sharing, and `uv` integration in more depth.
- The [Security guide](security.md) covers signing, checksums, and the threat model.
