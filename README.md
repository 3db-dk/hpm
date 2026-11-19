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
verify it against the `.sha256` beside it (see [Install hpm](docs/install.md)),
or build from source (requires Rust 1.89+):

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
hpm registry add git@git.studio.example:pipeline/hpm-index.git --name studio --type git

# Add dependencies
hpm add acme/rock-tools@0.1.0
hpm add local-tools --path ../local-tools

# Install everything (HPM packages + Python deps)
hpm install

# List what you have
hpm list --tree
```

Then point Houdini at the generated manifests by adding
`<project>/.hpm/packages` to `HOUDINI_PACKAGE_PATH`.

For a tool you want in every Houdini session rather than in one project,
skip the project entirely:

```sh
hpm global add acme/rock-tools --houdini 21.0
```

## Package manifest

Packages are defined in `hpm.toml`:

```toml
[package]
path = "acme/my-tools"       # scoped creator/slug identifier
name = "My Tools"                 # display name
version = "1.0.0"
description = "Custom Houdini tools"
authors = ["Name <name@example.com>"]
license = "MIT"

[compat]
houdini = ">=20.5, <22"           # Cargo-style range; see the manifest reference
platforms = ["linux-x86_64", "macos-aarch64", "windows-x86_64"]   # omit for pure-data

[dependencies]
"acme/utility-nodes" = "1.0.0"
"acme/material-lib" = { version = "2.0.0", optional = true }
local-tools = { path = "../local-tools" }

[python_dependencies]
numpy = ">=1.26"
requests = { version = ">=2.25.0", extras = ["security"] }

[runtime]
MY_TOOLS_CONFIG = { method = "set", value = "$HPM_PACKAGE_ROOT/config" }
HOUDINI_TOOLBAR_PATH = { method = "prepend", value = "$HPM_PACKAGE_ROOT/toolbar" }

# Staging: how the install image is derived from the workspace.
# Required when [compat].platforms is set.
[stage]
output_dir = "dist"
prepack = ["build-dso"]            # runs entries from [scripts]
include = ["python/**", "otls/**", "config/**"]
exclude = ["src/**", "tests/**"]   # exclude wins over place: do not exclude build outputs

[stage.platform.linux-x86_64]
place = [{ from = "build/linux/*.so", to = "dso/" }]

[stage.platform.macos-aarch64]
place = [{ from = "build/macos/*.dylib", to = "dso/" }]

[stage.platform.windows-x86_64]
place = [{ from = "build/win/*.dll", to = "dso/" }]
```

See the [manifest reference](docs/manifest.md) for every section and key.

## Commands

| Command | Description |
|---------|-------------|
| `hpm init [name]` | Create a new package (`--bare` for manifest only) |
| `hpm add <pkg>...` | Add dependencies (`name@version`, `--path`, `--path --link`, `--optional`, `--registry <name>` to pin) |
| `hpm remove <pkg>` | Remove a dependency |
| `hpm install` | Install all dependencies (`--frozen-lockfile` for CI) |
| `hpm update [pkg...]` | Update dependencies (`--dry-run` to preview) |
| `hpm list` | Show dependencies (`--tree` for tree view) |
| `hpm check` | Validate manifest and package structure |
| `hpm build` | Materialise the install image into `[stage].output_dir` (`-o <dir>` for a custom path, e.g. per-Houdini-session staging; `--profile <name>` to select a `[stage.profile.*]` build variant) |
| `hpm run <script> [args...]` | Run a script from `[scripts]` (args after the name are forwarded to it) |
| `hpm search <query>` | Search configured registries |
| `hpm pack` | Build a distributable archive (`--key` to sign, `--platform` for native) |
| `hpm clean` | Remove orphaned packages and venvs (`--dry-run`, `--python-only`, `--comprehensive`) |
| `hpm audit` | Security checks on the current project |
| `hpm registry <sub>` | Manage registries (`add`, `list`, `remove`, `update`) |
| `hpm global <sub>` | Install into Houdini's user preferences, outside any project (`add`, `list`, `remove`; `--houdini <X.Y>` required) |
| `hpm completions <shell>` | Generate shell completions |

Every command accepts `-v` for verbose output, `-q` for quiet and
`-C <dir>` to run in another directory, on either side of the subcommand,
and `--output {human,json,json-lines,json-compact}` before it for
machine-readable output.

## Python dependencies

Python dependencies declared in `[python_dependencies]` are resolved with a
bundled copy of [uv](https://github.com/astral-sh/uv) and installed into
content-addressable virtual environments under `~/.hpm/venvs/`. Packages
whose resolved Python dependencies hash to the same set share a single venv.

Version constraints use pip's syntax (PEP 440): `>=1.26`, `~=1.26`, `==1.26.4`.
The caret (`^1.26`) and a bare version are not accepted.

HPM picks the Python version from the lower bound of the project's
`[compat].houdini`:

| Houdini    | Python |
|-----------:|--------|
| 20.5+      | 3.11   |
| 21.x       | 3.11   |
| 22.x       | 3.13   |

Houdini 19.x (Python 3.7) and 20.0–20.4 (Python 3.9) are unsupported — their
Python interpreters are past upstream EOL. Unsupported versions produce an
install-time error rather than a silent fallback. See
[Add Python dependencies](docs/guide-python-dependencies.md) and
[Python environments](docs/python-guide.md).

## How it works

- **Resolution** — `hpm install` selects each package on its own: the highest non-yanked version matching its version requirement. hpm-core's `registry::solve` chooses a whole set at once (each version's Houdini range, whether it has a build for the host, and its requirements of the other packages), for applications that embed hpm; `hpm install` does not use it yet.
- **Lock file** — `hpm.lock` pins exact versions, sources, and SHA-256 checksums of Houdini packages. Python packages are resolved by uv on each install and are not pinned by it.
- **Storage** — `~/.hpm/packages/` for packages, `~/.hpm/venvs/` for Python environments (same layout on Linux, macOS, Windows).
- **Houdini integration** — `hpm install` writes one `{creator}.{slug}.json` per dependency into `<project>/.hpm/packages/`. Point Houdini's `HOUDINI_PACKAGE_PATH` at that directory, or use `hpm global` to install into Houdini's user preferences so a package loads in every session without a project.

## Project structure

```
crates/
  hpm-cli/       CLI frontend (clap)
  hpm-core/      Storage, installation, lock files, project discovery,
                 Python venv management (`python` submodule, bundled uv)
  hpm-config/    Configuration loading and schema
  hpm-package/   Manifest parsing, Houdini package.json generation
  hpm-assets/    Operator asset-index model emitted by `hpm pack`
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
[hpm.readthedocs.io](https://hpm.readthedocs.io/). Start with the
[quickstart](docs/quickstart.md), the [guides](docs/README.md#guides) or the
[CLI reference](docs/cli.md). To build it locally:

```sh
mdbook serve
```

## License

MIT. See [LICENSE](LICENSE).
