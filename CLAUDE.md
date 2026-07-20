# HPM — Houdini Package Manager

A modern package manager for SideFX Houdini, written in Rust.

## Toolchain

- **Language:** Rust (MSRV 1.85)
- **Build system:** Cargo workspace
- **Repository:** https://github.com/3db-dk/hpm

## Commands

```sh
cargo build          # build all crates
cargo test           # run all tests
cargo clippy --all-targets -- -D warnings   # lint
cargo fmt --check    # check formatting
```

## Workspace structure

| Crate | Purpose |
|-------|---------|
| `hpm-cli` | CLI frontend (clap) |
| `hpm-core` | Core orchestration logic, including the `python` submodule (bundled uv, venv management, Houdini→Python ABI mapping), `global` (installs into Houdini's user prefs, with the ledger that also serves as a `hpm clean` GC root), and `houdini_prefs` (per-platform Houdini preferences-directory resolution) |
| `hpm-config` | Configuration loading/saving |
| `hpm-package` | Package format and metadata |
| `hpm-assets` | Operator asset-index model emitted by `hpm pack` |

## Documentation

`docs/` is the single source for **two** published sites:

- [hpm.readthedocs.io](https://hpm.readthedocs.io/) — mdBook, driven by
  `docs/SUMMARY.md` (`book.toml`, `.readthedocs.yaml`).
- [docs.tumbletrove.com/hpm/](https://docs.tumbletrove.com/hpm/) — VitePress,
  driven by `docs/manifest.toml`; the `tumbletrove-docs` repo clones this one
  at its latest tag and copies `docs/` in.

Adding or renaming a page means updating **both** `SUMMARY.md` and
`manifest.toml`, or the page silently vanishes from one site.

The canonical registry is `https://api.tumbletrove.com/v1/registry`, aliased
`tumbletrove` in examples. The old `houdinihub` / `api.3db.dk` naming is dead
— that domain no longer resolves, so stale examples are broken commands, not
just off-brand ones.

Docs are load-bearing prose *and* API reference: `architecture.md` and
`api-overview.md` name concrete types and functions that nothing compiles or
link-checks. When renaming a public type, grep `docs/` for the old name.
