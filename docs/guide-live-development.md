# Develop a package against a live Houdini

While you work on a package you want Houdini to load your working copy, not
a published version. A **path dependency** does that. It comes in two modes,
and which one you use decides how edits reach Houdini.

## Copy mode (the default)

```sh
hpm add acme/rock-tools --path ../rock-tools
```

```toml
[dependencies]
"acme/rock-tools" = { path = "../rock-tools" }
```

`hpm install` copies the package's *install image* into the store: the files
`hpm pack` would ship, not the whole working directory. `.git/`, build
directories and anything ignored are left behind.

- **To pick up a change:** run `hpm install` in the project, then restart
  Houdini.
- **Use it when** you want to test exactly what users will get, and for any
  package with compiled plugins.

Each install of changed content goes to a new directory, so a Houdini that
is still running keeps the copy it started with.

## Link mode

```sh
hpm add acme/rock-tools --path ../rock-tools --link
```

```toml
[dependencies]
"acme/rock-tools" = { path = "../rock-tools", link = true }
```

The store entry is a symlink (a junction on Windows, which needs no special
permission) to your working directory. Houdini reads your files directly.

- **To pick up a change:** nothing to run. What Houdini does with the edit
  depends on the file; see the table below.
- **Use it when** the package is Python, HDAs, shelves and other data.

A package that declares native platforms in `[compat].platforms` is always
installed as a copy, even with `link = true`. A linked plugin binary could
not be rebuilt while Houdini has it loaded, and Houdini cannot reload one
without a restart anyway.

## What needs what

| You changed | Copy mode | Link mode |
|-------------|-----------|-----------|
| A Python module | `hpm install`, restart | Reload the module (`importlib.reload`) or restart |
| An HDA | `hpm install`, restart | Houdini sees the file; refresh asset libraries or restart |
| A shelf, a preset, other data | `hpm install`, restart | Restart, or reload where Houdini offers it |
| `hpm.toml`: `[runtime]`, `[python_dependencies]`, `[compat]` | `hpm install`, restart | `hpm install`, restart |
| A compiled plugin | Rebuild, `hpm install`, restart | Same; it is installed as a copy |

Changes to `hpm.toml` always need `hpm install`, because they change the
package file hpm generates for Houdini, and Houdini reads those only at
startup.

## Editing HDAs from inside Houdini

Houdini saves an HDA back to the file it loaded. In link mode that is your
working copy, which is what you want. In copy mode it is the copy in the
store: the edit is not in your repository, and the next install will not
include it. Use link mode when you edit assets in Houdini.

## Values that differ in development

A `[runtime]` value can depend on whether the package was installed from a
path or from a registry:

```toml
[runtime.ROCK_TOOLS_DEBUG]
method = "set"
value = [
  { when = { install_source = "dev" }, set = "1" },
  { when = {}, set = "0" },
]
```

Do not use this to point at a build directory. A copy-mode install does not
contain it; see
[HDK plugins: where a dev install finds its DSOs](manifest.md#hdk-plugins-where-a-dev-install-finds-its-dsos).

## Going back to the published version

Replace the path dependency with a registry one:

```sh
hpm add acme/rock-tools@1.2.0
```

Path installs live apart from registry installs in the store, so a
development copy of `rock-tools 1.2.0` is never mistaken for the published
`1.2.0`, in this project or any other.

`hpm clean` removes development copies no project refers to. Close Houdini
sessions that use them first.
