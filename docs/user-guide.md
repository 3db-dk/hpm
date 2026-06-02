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
| `--houdini <range>` | `^21` | `[compat].houdini` Cargo-style range (e.g. `"^21"`, `">=20.5, <22"`, `">=21"`). Default is bounded to a single Houdini major — see [`[compat]`](#compat) for why. |
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
  --houdini ">=20.5, <22"
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
| `--path <dir>` | Add as a local path dependency (only valid with a single package). Path dependencies install into a `_dev/` subtree of the global packages dir so they never overwrite a registry install at the same `(slug, version)`. |
| `--link` | For path dependencies, install as a symlink (Unix) or NTFS junction (Windows) instead of copying. Working-tree edits become visible to a live Houdini session without re-running `hpm install`. Requires `--path`. |
| `-p, --package <path>` | Path to the manifest to modify (`hpm.toml` or containing dir). Defaults to cwd. |
| `--optional` | Mark all added dependencies as optional. |

**Examples**

```sh
hpm add studio/utility-nodes@1.0.0
hpm add studio/a@1.0.0 studio/b@2.0.0
hpm add local-tools --path ../local-tools
hpm add local-tools --path ../local-tools --link  # live edits → Houdini
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
4. Collects Python dependencies from the root manifest and every installed dependency's manifest, downloads a managed CPython matching the lower bound of the root manifest's `[compat].houdini` to `~/.hpm/uv-python/` (no-op if already present), resolves them with the bundled `uv`, and installs them into a content-addressable venv in `~/.hpm/venvs/<hash>/`.
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

- `hpm.toml` exists, parses, and passes manifest validation (scoped `creator/slug` path, semver version, `[compat].houdini` parseable, `[stage]` per-platform consistency with `[compat].platforms`).
- Generated Houdini `package.json` serializes cleanly.
- Soft warnings for: missing description, missing authors, missing keywords, missing `[compat].houdini`, missing README or license file, missing `.gitignore` when a `.git` directory is present, packages larger than 100 MB, and individual files larger than 10 MB.

Check is advisory — warnings do not fail the command.

### `hpm migrate`

Rewrite a pre-0.16 (`Manifest 1.x`) `hpm.toml` to the current schema.

```
hpm migrate [OPTIONS]
```

| Option | Description |
|--------|-------------|
| `-p`, `--package <PATH>` | Path to `hpm.toml` or its directory (defaults to the current directory). |
| `--stdout` | Print the migrated manifest to stdout instead of writing it. |
| `--check` | Only report whether migration is needed; exit non-zero if so and write nothing. Useful as a CI gate. |

The 0.16.0 "Manifest 2.0" refactor renamed and reshaped five sections
(`[houdini]` -> `[compat].houdini`, `[env]` + `[dev.env]` -> `[runtime]`,
`[native]` -> `[compat].platforms` + `[stage]`, `[scripts.platform.<os>]`
-> conditional `cmd`). Old-format manifests are still **read
automatically** — every command converts them on load and prints a
deprecation warning — but that compatibility is removed in **0.20.0**, so
migrate before then.

Behaviour of the default (no-flag) form:

- Backs the original up as `hpm.toml.bak`, then writes the converted
  manifest in place with a comment header recording the migration.
- The `[native]` -> `[stage]` conversion is **best-effort**: the old
  `files` entries were include-filters, while `[stage.platform].place`
  rules need a destination, so each derived `to` path is flagged for
  review (in the terminal and in the file header). Verify them — the
  guess is correct for the common `dso/<plat>/*` layout but not for
  relocating layouts.

### `hpm run`

Execute a script defined in the manifest's [`[scripts]`](#scripts) table.

```
hpm run <SCRIPT> [-- ARGS...]
```

| Argument | Description |
|----------|-------------|
| `<SCRIPT>` | Name of the entry under `[scripts]`. |
| `ARGS...` | Trailing arguments forwarded to the script verbatim, after shell-quoting. |

Behaviour:

- Looks up the named entry; if its `cmd` is a conditional list, the first variant whose `when.os` matches the host wins. Plain entries always match.
- Sets `HPM_PACKAGE_ROOT` to the manifest directory and runs the command from that directory through the host shell (`sh -c` on Unix, `cmd /C` on Windows).
- For [table-form entries](#per-script-python-environments) with `python` or `requirements`, materializes a uv-managed venv at `~/.hpm/venvs/<hash>/`, prepends its `bin/` (or `Scripts/` on Windows) to `PATH`, and sets `VIRTUAL_ENV`. Two scripts whose `python` + `requirements` resolve to the same closure share one venv on disk.
- The script's exit code becomes `hpm`'s exit code, so `hpm run` is safe to chain in CI or wrap in a Houdini hook.

**Example**

```toml
[scripts.tt_setup]
cmd          = "python scripts/tt_setup.py"
python       = "3.11"
requirements = ["PySide6>=6.6"]
```

```sh
hpm run tt_setup --project /path/to/project
```

### `hpm search`

Search every configured registry for packages matching a query.

```
hpm search <QUERY>
```

If no registries are configured, HPM prints instructions to add one.

### `hpm build`

Materialise the install image into a directory. Runs `[stage].prepack`
scripts (compile DSO, collapse expanded HDAs, etc.), then copies workspace
files into the output directory using the same include/exclude/place
rules `hpm pack` would apply. The result is what a registry consumer's
install would look like.

```
hpm build [OPTIONS]
```

`hpm build` is a one-shot CLI verb — it copies files and exits, with no
background watcher. The output directory is yours to manage; common
patterns:

- **Single workstation, one Houdini at a time**: leave the default
  `[stage].output_dir` (`dist/`), point Houdini at it, rerun `hpm build`
  whenever you want a refresh.
- **Multiple Houdini sessions in parallel**: pass `--output <tmpdir>` per
  session and have each session's `HOUDINI_PACKAGE_PATH` reference its
  own staging directory. Avoids cross-session DSO lock conflicts.

**Options**

| Flag | Description |
|------|-------------|
| `-m, --manifest <path>` | Path to `hpm.toml` or containing dir. Defaults to cwd. |
| `-o, --output <dir>` | Override `[stage].output_dir`. Relative paths resolve against the manifest dir; absolute paths are used verbatim. |
| `--platform <id>` | Target platform. Defaults to host when `[compat].platforms` is declared. Required when host is not in the declared list. |
| `--no-prepack` | Skip `[stage].prepack` scripts. Use in CI when build steps already ran out-of-band. |
| `--no-clean` | Keep existing output-dir contents instead of wiping first. |

**Workflow notes — live editing and DSO rebuild**

These are *user-level* concerns; HPM doesn't model them in the manifest:

- **HDA editing.** Edits made inside Houdini save back to whatever path
  Houdini loaded the HDA from. If you load from `dist/otls/foo.hda`
  (collapsed by `hpm build`), saves go into the build output and get
  clobbered on the next `hpm build`. If you want round-trip editing,
  point Houdini at an unstaged expanded HDA dir during dev, and run
  `hpm build` only when you want to produce the publishable form.
- **DSO rebuild while Houdini is loaded.** On Windows, a loaded `.dll`
  is locked. With `--output <tmpdir-A>` for session A and `--output
  <tmpdir-B>` for session B, your `cmake --build` (writing to
  `build/<plat>/`) doesn't hit either lock, and a fresh `hpm build
  --output <tmpdir-C>` writes to a third location — you only hit the
  lock when you try to rebuild *into* a directory a live Houdini still
  has loaded. The typical workflow is one temp dir per Houdini
  lifetime, thrown away on Houdini close.

### `hpm pack`

Build a distributable archive from the current package.

```
hpm pack [OPTIONS]
```

Pack runs `hpm check` first, then:

1. Auto-generates a Houdini-native `{slug}.json` inside the archive unless the user has provided one. This file follows Houdini's own package format, so the archive is usable by Houdini even without HPM.
2. Filters files by `[stage]` (per-platform `place` rules and `include`/`exclude` globs) when the manifest declares `[compat].platforms`.
3. Produces a `.zip` archive plus a SHA-256 checksum.
4. If a signing key is supplied, produces an Ed25519 signature over the archive bytes and emits a `keyId`.

**Options**

| Flag | Description |
|------|-------------|
| `--key <path>` | Ed25519 PKCS#8 PEM private key. Overrides `HPM_SIGNING_KEY`. |
| `--output <dir>` | Output directory. Defaults to the current directory. |
| `--json` | Emit the result as JSON (useful in CI). |
| `--platform <id>` | Override host-platform detection. Valid: `linux-x86_64`, `linux-aarch64`, `macos-x86_64`, `macos-aarch64`, `windows-x86_64`, `windows-aarch64`, `universal`. Only legal when `[compat].platforms` is declared. |

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

Remove orphaned packages, dev (path-dep) installs, and/or venvs that no
active project depends on.

```
hpm clean [OPTIONS]
```

| Flag | Description |
|------|-------------|
| `-n, --dry-run` | Print what would be removed, without touching anything. |
| `-y, --yes` | Skip confirmation. |
| `--python-only` | Clean only orphaned venvs. |
| `--comprehensive` | Clean packages, dev installs, **and** venvs. |

HPM identifies active projects via `[projects]` in `~/.hpm/config.toml`
(`explicit_paths` plus recursive `search_roots`). Three classes of artifact
are considered:

- **Registry/URL packages** in `~/.hpm/packages/<slug>@<version>/`:
  preserved if reachable from any active project's dependency graph.
- **Dev installs** in `~/.hpm/packages/_dev/<slug>@<version>/` (created by
  `{ path = "..." }` deps, copy or link mode): preserved if any active
  project's path-dep source manifest reports that `(slug, version)`.
  Entries are listed as `_dev/<slug>@<version>` so the source of each is
  obvious. Link installs are unlinked safely — never followed.
- **Python venvs** under `~/.hpm/venvs/`: preserved if any kept package
  declares matching `[python_dependencies]`. Removed only when
  `--python-only` or `--comprehensive` is set.

A project whose path-dep source can't be read (workspace moved or
deleted) logs a warning and doesn't block cleanup of other dev installs;
re-run `hpm install` after fixing the path to reinstate anything that was
swept.

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
| `version` | yes | Semantic version per [semver.org](https://semver.org/). `major.minor.patch` is required; pre-release identifiers (`1.0.0-alpha.1`, `1.0.0-rc.2`) and build metadata (`1.0.0+build.5`) are accepted. |
| `description` | no | Short description. |
| `authors` | no | List of `"Name <email>"` strings. |
| `license` | no | License identifier (e.g. `MIT`, `Apache-2.0`). |
| `readme` | no | Path to README, relative to the package. Defaults to `README.md` for `init`-generated packages. |
| `homepage` | no | Project homepage URL. |
| `repository` | no | Repository URL. |
| `documentation` | no | Documentation URL. |
| `keywords` | no | List of keywords for discovery. |
| `categories` | no | List of categories. |

### `[compat]`

Target-environment compatibility for the package. Two axes:

- `houdini` — a Cargo-style version requirement string.
- `platforms` — the native platforms this package supports. Omit (or
  use `["universal"]`) for pure-data / pure-Python packages; list the
  platforms the package ships binaries for otherwise.

```toml
[compat]
houdini = "^21"                                # default — Houdini 21.x only
# houdini = ">=20.5, <22"                       # explicit range
# houdini = "~21.5"                             # tilde: >=21.5, <21.6
# houdini = "21"                                # bare = caret = ^21
# houdini = ">=20.5"                            # unbounded above — only safe for pure-data
platforms = ["linux-x86_64", "macos-aarch64"]   # omit for pure-data
```

The supported operators are `=`, `>=`, `>`, `<=`, `<`, `^`, `~`, and the
bare-version shorthand (aliases caret). Multiple comparators combine with
`and` when separated by commas. The same grammar is reused inside
`[runtime]` conditional values (`when = { houdini = "^21" }`).

The lower bound of `[compat].houdini` on the **root** manifest drives the
bundled Python version. A dependency package's range is a compatibility
floor only and does not influence the venv ABI. See [Python guide](python-guide.md).

#### Why the default is bounded above

Houdini's binary compatibility doesn't survive a major-version bump.
A DSO compiled against the Houdini 21 SDK won't load in Houdini 22;
some Python module signatures shift between majors too. The init
template defaults to `houdini = "^21"` (Houdini 21.x only) for that
reason — authors who ship binaries get a safe starting point, and
authors of pure-data / pure-Python packages can widen the range
explicitly after testing on the next major.

`hpm check` warns when `[compat].platforms` is non-empty but
`[compat].houdini` is unbounded above (e.g. `">=21"`). That catches
the failure mode where a native-binary package installs cleanly on a
newer Houdini and then crashes at load.

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

# 5. Local path, installed as a symlink/junction (live edits)
#    Working-tree edits become visible to a live Houdini session without
#    re-running `hpm install`. Otherwise identical to (4): same _dev/ namespace
#    isolation, no effect on registry installs at the same coordinate.
local-tools = { path = "../local-tools", link = true }
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

### `[runtime]`

Environment variables to set when Houdini loads the package. The key is
the variable name; the value is a `{ method, value }` pair:

```toml
[runtime]
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

A package can declare an env var as required without giving it a value.
Any project that depends on the package must then supply the value in its
own `[runtime]` section in `hpm.toml`. `hpm install` (and project sync)
errors out otherwise — the package isn't launchable without it.

```toml
# In the package's hpm.toml
[runtime]
PROJECT_ASSETS = { method = "set", required = true }
```

```toml
# In the consuming project's hpm.toml
[runtime]
PROJECT_ASSETS = { method = "set", value = "/mnt/studio/assets" }
```

`required = true` may be combined with a `value`; the value then acts as
a default and the project override becomes optional. Without a value, the
entry is a hard placeholder.

A consuming project can also override any package-declared env var by
re-declaring the same key in its own `[runtime]`. How the project entry
combines with the package's depends on the *project* entry's `method`:

| Project `method` | Result |
|------------------|--------|
| `set` | Replaces the package's contribution wholesale — only the project's value is emitted. |
| `prepend` / `append` | Extends it — the package's own entry is emitted first, then the project's, so Houdini merges both in load order with the requested method. |

So a project `append`/`prepend` adds to a package-provided value (e.g.
extending a package's `PYTHONPATH`) rather than clobbering it. Use `set`
when you genuinely want to replace what the package contributes.

#### Conditional values

`value` accepts either a flat string or an ordered list of `{ when, set }`
variants. The variants are selected against four axes; the first match
wins per the rules below.

```toml
[runtime.PXR_PLUGINPATH_NAME]
method = "prepend"
value = [
  { when = { houdini = "^21" }, set = "$HPM_PACKAGE_ROOT/resolver/houdini21/r" },
  { when = { houdini = "^22" }, set = "$HPM_PACKAGE_ROOT/resolver/houdini22/r" },
]
```

| Field | Form | Evaluated by | Compiles to |
|-------|------|--------------|-------------|
| `houdini` | Cargo-style req: `"^21"`, `"~21.5"`, `">=21, <22.5"`, `"21"` (alias for `^21`) | Houdini at startup | `houdini_version >= 'X' and houdini_version < 'Y'` |
| `os` | `"linux"`, `"macos"`, `"windows"` | Houdini at startup | `houdini_os == '<os>'` |
| `python` | `"3.11"`, `"python3.10"`, etc. | Houdini at startup | `houdini_python == 'python<v>'` |
| `install_source` | `"dev"` (path dependency) or `"registry"` (registry/URL install) | hpm at install time | filtered out before emission |

The first three axes lower into Houdini's `package.json` expression form
per <https://www.sidefx.com/docs/houdini/ref/plugins.html>. `install_source`
is *install-time evaluated by hpm* — variants gated to a non-matching
install source are dropped before the Houdini package.json is written, so
a `"dev"` branch never ships to a registry consumer's manifest and a
`"registry"` branch never fires in the dev's own Houdini.

All present axes combine with `and` within a single `when`. Order matters:
Houdini picks the first matching branch. An empty `when = {}` is the
always-true fallback and should appear last. `$HPM_PACKAGE_ROOT` is
substituted in each branch, just like in flat values; any other `$VAR`
(e.g. `$HOUDINI_MAJOR_RELEASE`) passes through verbatim so Houdini's own
variable expansion handles it.

Malformed selectors fail at manifest validation time, so authors find
them before publish, not at install.

#### HDK plugin pattern (dev-only paths)

The canonical use of `install_source` is HDK plugin development: a
build-tree path that must reach the dev's own Houdini but never leak to a
registry consumer. Express this with a single `[runtime]` entry whose
dev variant points at `build/` and whose fallback points at the staged
artifact:

```toml
[runtime.HOUDINI_DSO_PATH]
method = "prepend"
value = [
  # While developing the package locally:
  { when = { install_source = "dev", os = "windows" }, set = "$HPM_PACKAGE_ROOT/build/Release" },
  { when = { install_source = "dev", os = "linux"   }, set = "$HPM_PACKAGE_ROOT/build/lib" },
  { when = { install_source = "dev", os = "macos"   }, set = "$HPM_PACKAGE_ROOT/build/lib" },
  # What ships in the published archive:
  { when = {}, set = "$HPM_PACKAGE_ROOT/dso" },
]
```

When this package is consumed via `{ path = "..." }`, the dev variant fires
and points Houdini at the live build directory. When it ships through a
registry, hpm filters the dev variants out at install time, the fallback
fires, and Houdini sees only the published `dso/` location. If you want
the variable to disappear entirely for non-dev consumers, omit the
fallback branch — an entry with no surviving variants is not emitted.

When a key appears in both the project and the package, the project
entry's `method` decides whether it replaces or combines:

- `set` — the project override replaces the package's `[runtime]` entry
  (with its surviving variants).
- `prepend` / `append` — the package's `[runtime]` entry (with surviving
  variants) is emitted first, then the project's override, and Houdini
  merges them in load order.

### `[stage]`

Defines how the install image is derived from the workspace. `[stage]`
governs both `hpm pack` (which streams an archive directly from the
workspace, applying these rules) and — when present — `hpm build` (which
materialises the same image into `output_dir` on disk so a path-dep
consumer can pick it up live).

```toml
[stage]
# Where `hpm build` materialises the install image. Default: "dist".
output_dir = "dist"

# Scripts (named entries from [scripts]) to run before staging. Fail-fast.
prepack = ["build-dso"]

# Gitignore-style globs applied on top of .gitignore and .hpmignore.
# Empty `include` means "everything not excluded". Always-excluded: .git/, .hpm/.
include = ["python/**", "otls/**", "config/**", "LICENSE", "README.md"]
exclude = ["src/**", "build/**", "tests/**"]

# Per-platform place rules. The platform key must appear in [compat].platforms.
[stage.platform.linux-x86_64]
place = [{ from = "build/linux/*.so", to = "dso/" }]

[stage.platform.macos-aarch64]
place = [{ from = "build/macos/*.dylib", to = "dso/" }]

[stage.platform.windows-x86_64]
place = [{ from = "build/win/*.dll", to = "dso/" }]
```

**Platforms.** Valid platform identifiers (TumbleTrove API build platform
enum verbatim): `linux-x86_64`, `linux-aarch64`, `macos-x86_64`,
`macos-aarch64`, `windows-x86_64`, `windows-aarch64`, `universal`. Use
`universal` for OS-agnostic content (pure-Python / data). Declare the
platforms you ship under `[compat].platforms`; each per-platform
`[stage.platform.<plat>]` table must reference a platform listed there.

**Place rules.** Each rule has a `from` glob (workspace-relative) and a
`to` path (archive-relative). If `to` ends with `/` it's a directory; the
file's basename is appended. Otherwise `to` is the literal archive path
(use when relocating a single file under a renamed name). Both `from`
and `to` use forward slashes regardless of host OS.

**Per-platform packing semantics.** When packing for `--platform <X>`:

- A path matched by `[stage.platform.X].place[*].from` is included at the
  rewritten `to` path. The target's claim wins over other platforms'.
- A path matched only by `[stage.platform.Y].place[*].from` (some `Y != X`)
  is excluded.
- A path matched by no `place` rule and not covered by `[stage].exclude`
  is included as common content at its workspace-relative path. (If
  `[stage].include` is non-empty, common content is restricted to paths
  matching one of those globs.)

This means listing the same `from` glob under every platform is a valid
way to declare "this content ships in every per-platform archive"
(e.g. a shared resolver path).

**Build vs pack.** `hpm pack` reads `[stage]` directly and streams the
archive from the workspace — useful in CI where you build immediately
before packing. `hpm build` runs `prepack` scripts and materialises the
same install image into `output_dir` on disk, so a path-dep consumer
working in another project can pick it up live (with `link = true`,
edits flow through without re-running `hpm install`).

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

Named scripts for the package. Run them with `hpm run <name> [args...]`,
which sets `HPM_PACKAGE_ROOT` to the manifest directory and forwards
trailing arguments to the script.

```toml
[scripts]
build = "python scripts/build.py"
test = "python -m pytest tests/"
```

#### Per-host variation

Scripts whose command differs per OS use a conditional `cmd` value — the
same `when`-grammar `[runtime]` uses, restricted to the `os` axis:

```toml
[scripts]
build = "cargo build"                        # runs on any host

[scripts.register]
cmd = [
  { when = { os = "windows" }, set = "\"$HPM_PACKAGE_ROOT/plugin/bin/tool.exe\" register" },
  { when = { os = "macos"   }, set = "\"$HPM_PACKAGE_ROOT/plugin/bin/tool\" register" },
  { when = { os = "linux"   }, set = "\"$HPM_PACKAGE_ROOT/plugin/bin/tool\" register" },
]
```

`hpm run` picks the first variant whose `when.os` matches the host. Add an
empty `when = {}` branch as a last entry to declare a fallback that
matches any host the explicit branches missed. A script with no matching
variant on the current host is treated as absent — `hpm run` errors with a
message that the script only matches other platforms.

Only the `os` axis is meaningful for scripts: HPM has no Houdini-version
or Python context at `hpm run` time, and `install_source` is irrelevant
because scripts run against the dev's workspace, not an install. Setting
any non-`os` axis in a script `when` is rejected at manifest validation
time.

#### Per-script Python environments

A script that needs a pinned Python interpreter or extra packages can opt
into a uv-managed virtual environment by switching from the shorthand
string to the table form:

```toml
[scripts.tt_setup]
cmd          = "python scripts/tt_setup.py"
python       = "3.11"
requirements = ["PySide6>=6.6"]
```

`hpm run tt_setup` then resolves `requirements` through the same uv
pipeline that backs `[python_dependencies]`, materialises a venv at
`~/.hpm/venvs/<hash>/`, prepends its `bin/` (or `Scripts/` on Windows) to
`PATH`, and sets `VIRTUAL_ENV` so `python` in the command resolves to the
pinned interpreter. Two scripts whose `python` + `requirements` resolve
to the same closure share one venv on disk. Plain-string entries keep
their prior behaviour and execute against whatever `python` is on `PATH`.

Conditional cmd + venv hints compose:

```toml
[scripts.regen]
cmd = [
  { when = { os = "windows" }, set = "python scripts\\regen.py" },
  { when = {},                  set = "python scripts/regen.py" },
]
python       = "3.11"
requirements = ["pyyaml"]
```

Both `python` and `requirements` are optional in the table form; omitting
both yields a regular script with no venv overhead.

The table form also accepts plain inline-table syntax:

```toml
[scripts]
tt_setup = { cmd = "python scripts/tt_setup.py", python = "3.11", requirements = ["PySide6>=6.6"] }
```

Consumers resolve scripts through `PackageManifest::script_for(name)` (or
`resolved_scripts()`) which returns the [`ScriptEntry`] verbatim;
call `ScriptEntry::resolve_cmd(host_os)` to pick the right variant.

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
├── uv-python/                       # managed CPython installs (downloaded by uv on first launch)
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

An unsupported `[compat].houdini` lower bound is a hard error rather
than a silent fallback. Update `hpm-core::python::collection` if you need to add a new major,
or set the range to a supported lower bound.

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
RUST_LOG=hpm_core=debug,hpm_core::python=trace hpm install    # per-module
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
