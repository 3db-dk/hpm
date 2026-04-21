# HPM — Houdini Package Manager

A modern package manager for [SideFX Houdini](https://www.sidefx.com/products/houdini/), written in Rust.

HPM manages Houdini packages and their Python dependencies. It produces
reproducible installs with a lock file and checksum verification, shares
Python virtual environments across packages with identical resolved
dependencies, and generates the Houdini `package.json` files needed for
Houdini to pick the packages up on launch.

## Install

Download a pre-built binary from the
[latest release](https://github.com/3db-dk/hpm/releases/latest),
or build from source (requires Rust 1.85+):

```sh
git clone https://github.com/3db-dk/hpm.git
cd hpm
cargo build --release
# binary: target/release/hpm
```

## Quick start

```sh
# Create a new package
hpm init my-tools

# Add a registry (one-time, per user)
hpm registry add https://api.3db.dk/v1/registry --name houdinihub

# Add dependencies
hpm add some-creator/geometry-tools@1.0.0
hpm add local-tools --path ../local-tools

# Install everything (HPM packages + Python deps)
hpm install

# List what you have
hpm list --tree
```

Then point Houdini at the generated manifests by adding
`<project>/.hpm/packages` to `HOUDINI_PACKAGE_PATH`.

## Package manifest

Packages are defined in `hpm.toml`:

```toml
[package]
path = "my-studio/my-tools"       # scoped creator/slug identifier
name = "My Tools"                 # display name
version = "1.0.0"
description = "Custom Houdini tools"
authors = ["Name <name@example.com>"]
license = "MIT"

[houdini]
min_version = "20.5"              # maps to Python 3.10
max_version = "21.0"              # optional upper bound

[dependencies]
"my-studio/utility-nodes" = "1.0.0"
"my-studio/material-lib" = { version = "2.0.0", optional = true }
local-tools = { path = "../local-tools" }

[python_dependencies]
numpy = ">=1.20.0"
requests = { version = ">=2.25.0", extras = ["security"] }

[env]
MY_TOOLS_CONFIG = { method = "set", value = "$HPM_PACKAGE_ROOT/config" }
HOUDINI_TOOLBAR_PATH = { method = "prepend", value = "$HPM_PACKAGE_ROOT/toolbar" }

# Native library packaging (optional)
[native]
platforms = ["linux-x86_64", "macos-universal", "windows-x86_64"]

[native.linux-x86_64]
files = ["lib/linux-x86_64/*"]

[native.macos-universal]
files = ["lib/macos-universal/*"]

[native.windows-x86_64]
files = ["lib/windows-x86_64/*"]
```

See the [user guide](docs/user-guide.md) for the full manifest reference.

## Commands

| Command | Description |
|---------|-------------|
| `hpm init [name]` | Create a new package (`--bare` for manifest only) |
| `hpm add <pkg>...` | Add dependencies (`name@version`, `--path`, `--optional`) |
| `hpm remove <pkg>` | Remove a dependency |
| `hpm install` | Install all dependencies (`--frozen-lockfile` for CI) |
| `hpm update [pkg...]` | Update dependencies (`--dry-run` to preview) |
| `hpm list` | Show dependencies (`--tree` for tree view) |
| `hpm check` | Validate manifest and package structure |
| `hpm search <query>` | Search configured registries |
| `hpm pack` | Build a distributable archive (`--key` to sign, `--platform` for native) |
| `hpm clean` | Remove orphaned packages and venvs (`--dry-run`, `--python-only`, `--comprehensive`) |
| `hpm audit` | Security checks on the current project |
| `hpm registry <sub>` | Manage registries (`add`, `list`, `remove`, `update`) |
| `hpm completions <shell>` | Generate shell completions |

Every command accepts `-v` for verbose output, `-q` for quiet,
`-C <dir>` to change working directory, and
`--output {human,json,json-lines,json-compact}` for machine-readable output.

## Python dependencies

Python dependencies declared in `[python_dependencies]` are resolved with a
bundled copy of [uv](https://github.com/astral-sh/uv) and installed into
content-addressable virtual environments under `~/.hpm/venvs/`. Packages
whose resolved Python dependencies hash to the same set share a single venv.

HPM picks the Python version from `[houdini].min_version`:

| Houdini    | Python |
|-----------:|--------|
| 19.x       | 3.7    |
| 20.0–20.4  | 3.9    |
| 20.5+      | 3.10   |
| 21.x       | 3.11   |

Houdini versions outside this table produce an install-time error rather than
a silent fallback (this caught a real Houdini-21 ABI bug in 0.7.0). See the
[Python guide](docs/python-guide.md).

## How it works

- **Resolution** — PubGrub-style solver with conflict learning and backtracking.
- **Lock file** — `hpm.lock` pins exact versions, sources, and SHA-256 checksums.
- **Storage** — `~/.hpm/packages/` for packages, `~/.hpm/venvs/` for Python environments (same layout on Linux, macOS, Windows).
- **Houdini integration** — `hpm install` writes one `{name}.json` per dependency into `<project>/.hpm/packages/`. Point Houdini's `HOUDINI_PACKAGE_PATH` at that directory.

## Project structure

```
crates/
  hpm-cli/       CLI frontend (clap)
  hpm-core/      Storage, installation, lock files, project discovery
  hpm-config/    Configuration loading and schema
  hpm-resolver/  PubGrub dependency resolver
  hpm-package/   Manifest parsing, Houdini package.json generation
  hpm-python/    Python venv management (bundled uv)
  hpm-error/     Shared error types
```

## Development

```sh
cargo build                                    # build
cargo test                                     # test
cargo clippy --all-targets -- -D warnings      # lint
cargo fmt --check                              # format check
```

See [CONTRIBUTING.md](CONTRIBUTING.md) for setup details and the contribution
workflow.

## Documentation

The full documentation lives in [docs/](docs/) and is published at
[hpm.readthedocs.io](https://hpm.readthedocs.io/). To build it locally:

```sh
mdbook serve
```

## License

MIT. See [LICENSE](LICENSE).
