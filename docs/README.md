# HPM documentation

HPM is a package manager for SideFX Houdini. It installs Houdini packages
and the Python packages they need, records exactly what it installed in a
lock file, and writes the files Houdini reads to load them. It is a single
command-line program, `hpm`.

## Start here

- **[Install hpm](install.md)**
- **[Quickstart](quickstart.md)**: install a package into a project and load it in Houdini.
- **[Concepts](concepts.md)**: packages, projects, the store, registries, the lock file.

## Guides

Step-by-step, one task each.

| I want to | Guide |
|-----------|-------|
| Make a package | [Create your first package](guide-first-package.md) |
| See my edits in Houdini while I work | [Develop against a live Houdini](guide-live-development.md) |
| Use numpy or other Python packages | [Add Python dependencies](guide-python-dependencies.md) |
| Ship compiled plugins | [Ship an HDK plugin](guide-hdk-plugin.md) |
| Cover Houdini 21 and 22 | [Support several Houdini versions](guide-multiple-houdini-versions.md) |
| Release a package | [Publish a package](guide-publishing.md) |
| Host in-house packages | [Run a studio registry](guide-studio-registry.md) |
| Automate installs and releases | [Continuous integration](guide-ci.md) |
| Run tools and farm tasks | [Run package code outside Houdini](guide-scripts.md) |
| Set OCIO, asset roots, show paths | [Set a project's environment](guide-studio-environment.md) |
| Free disk space | [Keep disk usage down](guide-housekeeping.md) |

## Reference

- [CLI](cli.md): every command and flag.
- [The `hpm.toml` manifest](manifest.md): every section and key.
- [Configuration](configuration.md): `~/.hpm/config.toml`.
- [The `hpm.lock` lock file](lockfile.md)
- [Files and directories](files.md)
- [Environment variables](environment-variables.md)
- [Registry formats](registry-formats.md)
- [Troubleshooting](troubleshooting.md)
- [Upgrade notes](upgrading.md)

## How it works

- [How Houdini loads hpm packages](houdini-integration.md)
- [Python environments](python-guide.md)
- [Registries](registries.md)
- [Security](security.md)
- [Design notes](design-notes.md)

## Contributing

- [Architecture](architecture.md)
- [API overview](api-overview.md)
- [Testing guide](testing.md)
- [CONTRIBUTING.md](https://github.com/3db-dk/hpm/blob/main/CONTRIBUTING.md): development setup, workflow, and how these docs are published.
