# HPM — Houdini Package Manager

A modern package manager for SideFX Houdini, written in Rust.

## Toolchain

- **Language:** Rust (MSRV 1.89)
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
| `hpm-core` | Core orchestration logic, including the `python` submodule (bundled uv, venv management, Houdini→Python ABI mapping), `global` (installs into Houdini's user prefs, with the ledger that also serves as a `hpm clean` GC root), `houdini_prefs` (per-platform Houdini preferences-directory resolution), `storage::dev_users` (who uses a `_dev` copy, and reclaiming the copies nobody does) and `process_users` (the PID-plus-start-time rule for whether a recorded process still runs, shared with TumbleTrove Desktop) |
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
`crates/hpm-cli/tests/docs.rs` fails when the two disagree, and also checks
links, `toml` examples, the flags in `cli.md` and the identifiers the
contributor pages name.

Pages are flat files grouped by purpose: `guide-*.md` are task walkthroughs,
`cli.md` / `manifest.md` / `configuration.md` and the other reference pages
state every option, and the "How it works" pages explain. `changelog.md`
only includes the top-level `CHANGELOG.md`.

HPM is a general tool, and its public docs name no registry or product:
`docs/`, `README.md` and `CONTRIBUTING.md` contain zero mentions of
TumbleTrove (the registry, the website, or Desktop). Examples use one invented registry throughout, the
one `docs/guide-studio-registry.md` builds: a Git index at
`git@git.studio.example:pipeline/hpm-index.git`, aliased `studio`, serving
`acme/rock-tools` from `https://packages.studio.example/`. Where an API
registry or a second registry is needed it is
`https://registry.vendor.example/v1/registry`, aliased `vendor`; where the docs need to refer to Desktop they
say "an application that embeds hpm". TumbleTrove does not accept
third-party publishing, so the docs never tell readers to publish there.
The old `houdinihub` / `api.3db.dk` naming is dead as well. `CHANGELOG.md` is published as a docs page, so the same rule applies to it
(past entries were reworded on 2026-11-16). Source comments and tests may
still name TumbleTrove where they record a real incident or an embedder
contract.

Docs are load-bearing prose *and* API reference: `architecture.md` and
`api-overview.md` name concrete types and functions that nothing compiles or
link-checks. When renaming a public type, grep `docs/` for the old name.
