# HPM Documentation

HPM (Houdini Package Manager) is a Rust-based package manager for SideFX
Houdini. It manages both Houdini packages and their Python dependencies,
produces reproducible installs with a lock file and SHA-256 checksums, and
generates the `package.json` files Houdini needs to load packages at launch.

## User documentation

Published to [hpm.readthedocs.io](https://hpm.readthedocs.io/):

- **[User guide](user-guide.md)** — install, commands, the `hpm.toml` manifest, global configuration, troubleshooting.
- **[Python dependencies](python-guide.md)** — `[python_dependencies]`, Houdini-to-Python version mapping, venv sharing, cleanup.
- **[Registries](registries.md)** — configuring API and Git registries, per-user vs per-project, search and caching.
- **[Security](security.md)** — checksums, package signing, `hpm audit`, threat model.

## Contributor documentation

In-repo, not published:

- **[Architecture](architecture.md)** — system design, dependency resolution, cleanup, Python integration.
- **[API overview](api-overview.md)** — crate structure and key public types. Full rustdoc via `cargo doc`.
- **[Testing guide](testing.md)** — property-based testing strategy.
- **[CONTRIBUTING.md](../CONTRIBUTING.md)** — development setup, workflow, pull request guidelines.
