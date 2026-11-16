# Concepts

The handful of ideas the rest of the documentation assumes.

## Package

A **package** is a directory of things Houdini can load: digital assets in
`otls/`, shelves in `toolbar/`, Python modules in `python/`, compiled
plugins in `dso/`, and so on, with an `hpm.toml` that names it.

A package is identified by its **path**, `creator/slug`, for example
`acme/rock-tools`, and a semantic **version**. `hpm pack` turns the
directory into an archive that can be published and installed elsewhere.

## Project

A **project** is a directory whose `hpm.toml` lists the packages a piece of
work needs: a show, a shot, a personal setup. `hpm install` makes those
packages available and writes the files that let Houdini load them.

A project and a package use the same file, and a directory can be both. The
difference is which commands you run and which sections they read:

| | As a package | As a project |
|---|---|---|
| You run | `hpm check`, `hpm build`, `hpm pack` | `hpm add`, `hpm install`, `hpm update` |
| `[package]` | Its published identity | A name for the project |
| `[compat].houdini` | The Houdini versions it supports | The project's Houdini, which selects the Python version |
| `[dependencies]` | What it needs from other packages, as ranges | What to install, as exact versions |
| `[runtime]` | Variables it sets in Houdini | Overrides applied on top of every package |
| `[stage]`, `[[operators]]` | What ships in the archive | Not used |

`hpm init` creates a package layout. For a project that only consumes
packages, `hpm init --bare` creates just the `hpm.toml`.

## Store

Installed packages live once per machine, in the **store** at
`~/.hpm/packages/`, as `<slug>@<version>/`. Projects do not contain copies.
Ten projects using the same version of a package share one directory.

## Package files

Houdini discovers packages by reading small JSON **package files**. For each
dependency, `hpm install` writes one into `<project>/.hpm/packages/`,
pointing at the package in the store. You start Houdini with
`HOUDINI_PACKAGE_PATH` set to that directory, and Houdini loads exactly the
project's packages.

This is the whole integration: hpm does not modify Houdini, and Houdini does
not know hpm exists. [How Houdini loads hpm packages](houdini-integration.md)
shows the files.

## Global install

`hpm global add` skips the project. It writes the package file into
Houdini's own preferences directory, so the package loads in every session
of one Houdini version. Use it for personal tools; use projects for
anything a team must reproduce.

## Registry

A **registry** is an index that maps `creator/slug` and a version to a
download URL and checksum. hpm consults the registries you have added when
you run `hpm add`, `hpm install`, `hpm update` or `hpm search`. The public
TumbleTrove registry is one; a studio can [run its own](guide-studio-registry.md).

A dependency does not have to come from a registry. It can also be a
**URL** to an archive, or a **path** to a package directory on disk, which
is how you use a package while developing it.

## Versions, and what install does not do

A project's `[dependencies]` hold exact versions. `hpm add` picks the newest
version and writes it; `hpm update` moves it forward; `hpm install` installs
what is written.

`hpm install` installs the packages the project lists. It does not read
those packages' own dependencies and install them too. If a package needs
another package, the project lists both. TumbleTrove Desktop, which builds
on hpm, works those requirements out and writes the complete list for you.

## Lock file

`hpm install` records the installed versions and a checksum of each in
`hpm.lock`. Committed alongside `hpm.toml`, it makes the next install, on
any machine, produce the same packages or fail. See
[the lock file reference](lockfile.md) for what it does and does not pin.

## Python environments

A package can declare Python packages it needs inside Houdini. hpm installs
them with a bundled `uv` into a virtual environment built for the Python
that the project's Houdini ships, and adds it to that package's
`PYTHONPATH`. Environments are shared: projects whose packages resolve to
the same set use the same one. See [Python environments](python-guide.md).

## Where to go next

- [Quickstart](quickstart.md): install a package into a project and load it.
- [Create your first package](guide-first-package.md).
- [The `hpm.toml` reference](manifest.md).
