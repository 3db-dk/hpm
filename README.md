# HPM

A package manager for [SideFX Houdini](https://www.sidefx.com/products/houdini/), written in Rust.

HPM manages Houdini packages and their Python dependencies with reproducible installs,
a lock file, and isolated virtual environments.

## Install

Download a pre-built binary from the
[latest release](https://github.com/3db-dk/hpm/releases/latest),
or build from source (requires Rust 1.74+):

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

# Add a dependency from a registry
hpm add geometry-tools@1.0.0

# Add a local path dependency
hpm add local-tools --path ../local-tools

# Install everything (HPM packages + Python deps)
hpm install

# See what you have
hpm list --tree
```

## Package manifest

Packages are defined in `hpm.toml`:

```toml
[package]
name = "my-tools"
version = "1.0.0"
description = "Custom Houdini tools"
authors = ["Name <name@example.com>"]
license = "MIT"

[houdini]
min_version = "20.0"

[dependencies]
utility-nodes = "1.0.0"
material-lib  = { path = "../material-lib", optional = true }

[python_dependencies]
numpy = ">=1.20.0"
requests = { version = ">=2.25.0", extras = ["security"] }
```

## Commands

| Command | Description |
|---------|-------------|
| `hpm init [name]` | Create a new package (`--bare` for manifest only) |
| `hpm add <pkg>` | Add a dependency (`name@version`, `--path`, `--optional`) |
| `hpm remove <pkg>` | Remove a dependency |
| `hpm install` | Install all dependencies (`--frozen-lockfile` for CI) |
| `hpm update [pkg...]` | Update dependencies (`--dry-run` to preview) |
| `hpm list` | Show dependencies (`--tree` for tree view) |
| `hpm check` | Validate package configuration |
| `hpm pack` | Create a signed archive (`--key` for signing) |
| `hpm clean` | Remove orphaned packages and venvs (`--dry-run`, `--python-only`) |
| `hpm audit` | Security audit on dependencies |
| `hpm completions <shell>` | Generate shell completions (bash, zsh, fish, powershell) |

All commands accept `-v` for verbose output and `--output json` for machine-readable output.

## Python dependencies

Python dependencies declared in `[python_dependencies]` are resolved using a bundled
copy of [uv](https://github.com/astral-sh/uv) and installed into content-addressable
virtual environments under `~/.hpm/venvs/`. Packages with identical resolved Python
dependencies share the same venv.

HPM automatically maps Houdini versions to the correct Python version (e.g. Houdini 20.0
uses Python 3.9, 20.5 uses 3.10) and injects the venv into Houdini's `PYTHONPATH` via
the generated `package.json`.

## How it works

- **Resolution** — Uses the [PubGrub](https://github.com/dart-lang/pub/blob/master/doc/solver.md) algorithm (same as uv, Dart, Swift PM) with conflict learning and backtracking.
- **Lock file** — `hpm.lock` pins exact versions and checksums for reproducible installs.
- **Storage** — Packages live in `~/.hpm/packages/`, Python venvs in `~/.hpm/venvs/`.
- **Houdini integration** — Generates Houdini `package.json` files with search paths and environment variables.

## Project structure

```
crates/
  hpm-cli/       CLI frontend (clap)
  hpm-core/      Storage, installation, lock files, project discovery
  hpm-config/    Global configuration (~/.hpm/config.toml)
  hpm-resolver/  PubGrub dependency resolver
  hpm-package/   Package manifest parsing, Houdini package.json generation
  hpm-python/    Python venv management (uv integration)
  hpm-error/     Shared error types
```

## Development

```sh
cargo build                                    # build
cargo test                                     # test
cargo clippy --all-targets -- -D warnings      # lint
cargo fmt --check                              # format check
```

## Documentation

Full documentation is available in the [docs/](docs/) directory, or online at [hpm.readthedocs.io](https://hpm.readthedocs.io/).

To build locally with [mdBook](https://rust-lang.github.io/mdBook/):

```sh
mdbook serve
```

## License

MIT
