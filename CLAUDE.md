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
| `hpm-core` | Core orchestration logic, including the `python` submodule (bundled uv, venv management, Houdini→Python ABI mapping) |
| `hpm-config` | Configuration loading/saving |
| `hpm-package` | Package format and metadata |
