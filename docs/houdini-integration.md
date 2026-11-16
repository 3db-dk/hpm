# How Houdini loads hpm packages

Houdini finds packages by reading *package files*: small JSON files in the
directories it scans at startup. hpm never patches a Houdini installation.
It writes package files, and you tell Houdini where they are. This page
describes those files: the ones `hpm install` generates, the one `hpm pack`
puts in an archive, and how each behaves inside Houdini.

## What `hpm install` writes

One file per dependency, in `<project>/.hpm/packages/`, named
`{creator}.{slug}.json`:

```json
{
  "hpath": ["/home/me/.hpm/packages/utility-nodes@1.0.0"],
  "env": [
    { "PYTHONPATH": { "method": "prepend", "value": ["/home/me/.hpm/venvs/a1b2c3d4e5f60718/lib/python3.11/site-packages"] } },
    { "PYTHONPATH": { "method": "prepend", "value": ["/home/me/.hpm/packages/utility-nodes@1.0.0/python"] } },
    { "HOUDINI_SCRIPT_PATH": { "method": "prepend", "value": ["/home/me/.hpm/packages/utility-nodes@1.0.0/scripts"] } }
  ],
  "enable": "houdini_version >= '21' and houdini_version < '22'"
}
```

- `hpath` is the absolute path of the package in [the store](files.md). It
  puts the package root on `HOUDINI_PATH`, so Houdini discovers the
  convention folders beneath it (`otls/`, `toolbar/`, `desktop/`,
  `python_panels/`, `viewer_states/`, `dso/`, `python3.11libs/`, ...).
- `env` holds, in order: the shared venv's `site-packages` on `PYTHONPATH`
  (only for a package that declares `[python_dependencies]`), the package's
  `python/` folder on `PYTHONPATH` and its `scripts/` folder on
  `HOUDINI_SCRIPT_PATH` (when those folders exist), then the package's
  [`[runtime]`](manifest.md#runtime) entries.
- `enable` is the package's `[compat].houdini` range compiled to a Houdini
  expression. A Houdini outside the range skips the package silently.

`method: "prepend"` leaves the path separator to Houdini, so the same file
works on Unix (`:`) and Windows (`;`).

## Pointing Houdini at a project

Add `<project>/.hpm/packages` to `HOUDINI_PACKAGE_PATH` in the environment
Houdini starts from.

```sh
# Linux, macOS
HOUDINI_PACKAGE_PATH="$PWD/.hpm/packages" houdini
```

```powershell
# Windows (PowerShell)
$env:HOUDINI_PACKAGE_PATH = "$PWD\.hpm\packages"
houdini
```

For a studio-wide setup, set the variable in your launcher. Houdini reads
package files once, at startup: restart it after an install that changes
them.

To confirm what Houdini resolved, without opening a session:

```sh
HOUDINI_PACKAGE_PATH="$PWD/.hpm/packages" HOUDINI_PACKAGE_VERBOSE=1 hconfig
```

## Project installs and global installs

The two differ only in where the package file lands and what may be deleted
around it:

| | `hpm install` (project) | [`hpm global add`](cli.md#hpm-global) |
|---|---|---|
| Package file location | `<project>/.hpm/packages/` | Houdini's user preferences `packages/` |
| Loaded when | Houdini is pointed at the project via `HOUDINI_PACKAGE_PATH` | Every session of that Houdini version |
| Version scoping | `enable` expression only | `enable` plus a per-version directory |
| Reproducible via `hpm.lock` | Yes | No. Global installs are user state, not project state |
| Unrecognized files in the target directory | Removed; hpm owns that directory | Left alone; hpm owns only files it recorded |

Use a project install for anything a team needs to reproduce. Use a global
install for tools you want in your own everyday Houdini.

## How `[runtime]` methods are emitted

In the generated file, `prepend` and `append` are written as single-element
JSON lists. The list form matters for them — Houdini only honors `method` on a *custom*
variable when the variable's first definition uses a list value; with a
flat string every later entry silently overwrites the variable regardless
of its declared method.

`set` is emitted as a bare flat string instead (Houdini's package system
has no `set` method). This is deliberate: a path-registered variable that
Houdini seeds itself — OCIO, PYTHONPATH, `HOUDINI_*_PATH` — is always
merge-able, so a list-form `replace` *appends* onto Houdini's seed (you'd
get `builtin;project`, which breaks OCIO) instead of replacing it. A flat
string overwrites the seed cleanly and load-order-independent, which is
exactly what `set` promises. (A genuinely conditional `set` value — a
`{ when, set }` list — can't be a flat string, so it falls back to the
list `replace` form; gate the whole package on the condition, or set such
a path variable flat, if you need the overwrite there.)

## Project overrides

An install writes the project's `[runtime]` entries into
a dedicated `~hpm-project-overrides.json` in the project packages dir.
Houdini processes package files in byte-wise filename order and `~`
sorts after every slug character, so the overrides file always applies
last — after every per-package file — and each override is applied
exactly once, no matter how many packages declare the same variable.
This also means a project `[runtime]` entry takes effect even when no
package declares the variable. Since a project-level entry has no owning
package, `$HPM_PACKAGE_ROOT` is undefined there (sync warns if a project
entry references it).

## Installing an archive without HPM

The archive unzips to `{slug}.json` beside a `{slug}/` content folder. **Keep
those two together.** Every path in the json is written relative to the json's
own location — `$HOUDINI_PACKAGE_PATH` means *the directory holding this
package file* — so the pair is movable as a unit but not separable.

Given that, there are three ways to register it, none of which require
editing anything:

1. **Move both into a Houdini packages directory** —
   `$HOUDINI_USER_PREF_DIR/packages`, `$HSITE/houdini<ver>/packages`,
   `$HOUDINI_PACKAGE_DIR`, or `$HFS/packages`. You get
   `packages/{slug}.json` + `packages/{slug}/...`, exactly where the json's
   paths point.

2. **Keep both wherever you like and point Houdini at that folder.** Put a
   one-line file in a packages directory:

   ```json
   { "package_path": "/path/to/where/you/unzipped" }
   ```

   Houdini processes every package file directly in that folder, and
   `$HOUDINI_PACKAGE_PATH` inside `{slug}.json` resolves to it. Note that
   `package_path` cannot use variables set by any `env` block — only
   variables from the environment Houdini was launched with.

3. **Register it from the Package Browser.** Drag the shipped `{slug}.json`
   into the Package Browser pane — the `.json`, not the `.zip` and not the
   content folder. Houdini writes a `{slug}_autoload.json` into your packages
   directory that points back at the file, which behaves like option 2.

Separating the two — json in a packages directory, content somewhere else —
is the only arrangement that needs the file edited, because the paths are no
longer relative to anything useful. Option 2 exists to avoid exactly that.

> **The Package Browser's File > Install Package Archive does not use the
> shipped json at all.** Houdini honours a bundled package file only when it
> is named `<archive-stem>.json`; `hpm pack` names the archive
> `{slug}-{version}-{platform}.zip`, so ours is not merely ignored — it is
> never written to disk. Only the `{slug}/` folder is extracted. Houdini then
> writes its own package file into a packages directory holding `enable` and
> `hpath` only, with `hpath` pointing at the extracted folder.
>
> That still loads everything Houdini finds by convention under a
> `HOUDINI_PATH` entry — `otls/`, `toolbar/`, `scripts/`, `python_panels/`,
> `viewer_states/`, and `dso/`. What it cannot load is anything reachable only
> through a `[runtime]` variable, since no `env` block survives. A package
> whose native plugins sit in a version-keyed tree such as
> `bin/$HOUDINI_MAJOR_RELEASE.$HOUDINI_MINOR_RELEASE/dso` loses them silently;
> one that keeps them in a plain `dso/` folder is unaffected. Verified against
> Houdini 21.0.729.

## Dependencies in a hand-installed archive

An archive contains one package, not its dependency tree. The generated
`{slug}.json` lists the manifest's `[dependencies]` under Houdini's
`recommends`, never `requires`: `requires` is a hard gate, so a dependency
that isn't present would make Houdini disable the package outright and it
would load nothing at all — no HDAs, no environment, no toolbar. With
`recommends`, Houdini instead logs

```
WARNING: The recommended package <dep> for <slug> is either missing,
disabled or invalid.
```

and loads what the archive does ship. If the package genuinely needs that
dependency at runtime, install it too — either with `hpm install`, which
resolves the whole tree, or by unpacking each dependency's archive into the
same packages directory. Packages installed by HPM are unaffected: their
manifests declare neither field, because HPM has already installed every
dependency before Houdini starts.
