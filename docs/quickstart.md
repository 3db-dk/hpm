# Quickstart: use packages in a Houdini project

Ten minutes, from an empty directory to a Houdini session that loads a
package hpm installed. You need [hpm installed](install.md) and Houdini 20.5
or newer.

## 1. Add a registry

A registry is where hpm looks packages up. Add the one you install from,
once per machine. This documentation uses one example registry throughout:
a studio's own, named `studio`, which
[Run a studio registry](guide-studio-registry.md) builds from nothing.
Substitute your registry's URL:

```sh
hpm registry add git@git.studio.example:pipeline/hpm-index.git --name studio --type git
```

## 2. Create a project

A project is a directory with an `hpm.toml` that lists the packages you
want. `--bare` creates only that file:

```sh
hpm init my-project --bare --houdini "^21"
cd my-project
```

`--houdini` is the Houdini version the project runs in. `^21` means any
Houdini 21; use `^22` for Houdini 22, or `^20.5` for 20.5. It matters most
when packages bring Python dependencies, because it selects the Python
version they are installed for.

## 3. Find and add a package

```sh
hpm search rock
hpm add acme/rock-tools
```

`hpm add` takes the full `creator/slug` name that `hpm search` prints. It
picks the newest version, writes it to `hpm.toml`, and installs it:

```toml
[dependencies]
"acme/rock-tools" = "0.1.0"
```

To get a specific version, ask for it: `hpm add acme/rock-tools@0.1.0`.

## 4. See what was installed

```sh
hpm list
```

Two things now exist beside `hpm.toml`:

- `hpm.lock` records the exact version and a checksum. Commit it.
- `.hpm/packages/` holds one small JSON file per package, which tells
  Houdini where the package is. Do not commit `.hpm/`.

The package itself is in the shared store under `~/.hpm/packages/`, not in
the project.

## 5. Start Houdini with the project's packages

Houdini loads the packages listed in the directories named by
`HOUDINI_PACKAGE_PATH`. Point it at the project:

```sh
# Linux, macOS
HOUDINI_PACKAGE_PATH="$PWD/.hpm/packages" houdini
```

```powershell
# Windows (PowerShell)
$env:HOUDINI_PACKAGE_PATH = "$PWD\.hpm\packages"
houdini
```

`houdini` here is whatever starts Houdini on your machine, from a shell where
Houdini's environment is set up.

## 6. Check it loaded

In Houdini, the package's nodes are in the TAB menu and its shelves in the
shelf list. To check from Houdini's Python shell:

```python
import hou
print(hou.getenv("HOUDINI_PATH"))
```

The output includes a path under `.hpm/packages/` in your home directory.

If the package is missing, see
[Troubleshooting](troubleshooting.md#the-package-does-not-appear-at-all).

## Day to day

```sh
hpm add acme/fast-scatter         # add one more
hpm remove acme/rock-tools
hpm update --dry-run             # what newer versions exist?
hpm update                       # move to them
hpm install                      # after pulling a teammate's hpm.toml and hpm.lock
```

Restart Houdini after any of these; it reads package files only at startup.

## A tool in every session, without a project

For a personal tool you always want, skip the project and install it into
Houdini's own preferences for one Houdini version:

```sh
hpm global add acme/rock-tools --houdini 21.0
hpm global list --houdini 21.0
hpm global remove acme/rock-tools --houdini 21.0
```

It then loads in every Houdini 21.0 session with no environment variable.
See [`hpm global`](cli.md#hpm-global).

## Next

- [Concepts](concepts.md): what a project, a package and the store are.
- [Create your first package](guide-first-package.md).
- [Add Python dependencies](guide-python-dependencies.md).
