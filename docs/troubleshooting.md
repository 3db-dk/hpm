# Troubleshooting

Start with more output. `-v` shows progress, `-vv` shows more, and
`RUST_LOG` selects modules:

```sh
hpm -v install
RUST_LOG=debug hpm install
RUST_LOG=hpm_core=debug,hpm_core::python=trace hpm install
```

## Installing

### No package registries are configured

```
error: Package error: Failed to sync project dependencies: Cannot install foo 1.2.3:
no package registries are configured. Run 'hpm registry add <url>' to add one.
```

Add one. For TumbleTrove:

```sh
hpm registry add https://api.tumbletrove.com/v1/registry --name tumbletrove
```

### Version not found in registry

```
error: Package error: Failed to sync project dependencies: Failed to resolve
acme/tools 1.2.3 from registry: Version '1.2.3' of package 'acme/tools' not found in registry
```

The package or the version is not in any configured registry. Check, in
order:

1. The name is the full `creator/slug`, not the bare slug.
2. The version exists: `hpm search <slug>`.
3. For a Git registry, the local index is current: `hpm registry update`.
4. The version in `hpm.toml` is an exact version. If the message quotes a
   range (`Version '^1.2' ... not found`), that is the cause: `hpm install`
   does not resolve ranges. Write an exact version, or run
   `hpm update <package>` to have one chosen and written for you.

### A package's own dependency is missing in Houdini

`hpm install` installs the packages your project lists and nothing else. If
a package you installed needs another package, add that one to the project
with `hpm add`.

### Checksum mismatch

```
Checksum mismatch for acme/tools: expected v2:ab12..., got v2:cd34...
```

The package's files do not match the checksum `hpm.lock` recorded. Either
the copy in the store was changed after it was installed, or the registry
now serves different bytes for that version.

For a changed store copy, move it aside and install again:

```sh
mv ~/.hpm/packages/tools@1.2.3 ~/.hpm/packages/tools@1.2.3.files.json /tmp/
hpm install
```

If a fresh download still mismatches, the published archive changed. Do not
delete `hpm.lock` to make the error go away until you know why; that is the
situation the lock exists to catch.

### `hpm.lock` exists but cannot be read

Install stops rather than overwrite a lock it could not verify against. Fix
the file (a merge conflict marker is the usual cause), or delete it to
resolve and lock again.

### `--frozen-lockfile` fails in CI

The lock is missing, does not match the manifest, or lacks a checksum for a
registry or URL dependency. Run `hpm install` locally, commit `hpm.lock`,
and retry.

### No Python version mapping for Houdini

```
No Python version mapping for Houdini 23; supported versions are 20.5+, 21, 22.
Houdini 19.x (Python 3.7) and 20.0-20.4 (Python 3.9) are past EOL.
```

The lower bound of the project's `[compat].houdini` names a Houdini hpm has
no Python for. Set the range to a supported lower bound. hpm refuses rather
than guess, because a venv built for the wrong Python installs cleanly and
then fails on import inside Houdini.

### `uv` fails while resolving Python dependencies

- **`Couldn't parse requirement` or `Expected one of ... found ^`.** A
  version in `[python_dependencies]` is not a PEP 440 specifier. Use `~=` or
  `>=..,<..` instead of `^`, and `==1.2.3` instead of a bare `1.2.3`. See
  [`[python_dependencies]`](manifest.md#python_dependencies).
- **A package name with a version glued on was "not found"** (for example
  `numpy1-26-4`). Same cause: a bare version. Write `==1.26.4`.
- **`Conflicting versions for package numpy: >=1.20 vs >=1.21`.** Two
  installed packages declare the same Python package with differently
  spelled constraints. They must be identical; change one package, or pin
  the package versions whose constraints agree.
- **`No interpreter found`.** The managed CPython download was interrupted.
  Run the install again with a network connection.
- **Disk space.** A venv is tens to hundreds of MB, and each managed CPython
  about 50 MB.

## In Houdini

### The package does not appear at all

Check that Houdini was started with `HOUDINI_PACKAGE_PATH` containing
`<project>/.hpm/packages`, and restart Houdini after any install. To see
what Houdini resolved without opening a session:

```sh
HOUDINI_PACKAGE_PATH="$PWD/.hpm/packages" HOUDINI_PACKAGE_VERBOSE=1 hconfig
```

If the package is listed but disabled, the running Houdini is outside the
package's `[compat].houdini` range.

### Python packages are not importable

1. Open `.hpm/packages/{creator}.{slug}.json` for the package that declares
   the dependency. It should have a `PYTHONPATH` entry pointing into
   `~/.hpm/venvs/`.
2. Check that directory exists. If it does not, run `hpm install` again;
   venvs are a cache and are rebuilt on demand.
3. A dependency declared only in the *project's* own
   `[python_dependencies]` is not installed for Houdini. Declare it in the
   package that imports it.

### An HDK plugin's operators do not register

The package loads, its HDAs and shelves appear, and its nodes are missing,
with no error. `HOUDINI_DSO_PATH` points at a directory that does not exist
in the installed package, typically a build directory. See
[HDK plugins: where a dev install finds its DSOs](manifest.md#hdk-plugins-where-a-dev-install-finds-its-dsos).

### A hand-installed archive registers but nothing loads

Symptom: Houdini's package log lists the package, reports `Disabled Packages
(0)`, and logs no warning, but no HDAs, DSOs, or toolbars appear.

Almost always the package file and the `{slug}/` content folder are not in
the same directory. `$HOUDINI_PACKAGE_PATH` means *the directory holding the
package file*, so `$HOUDINI_PACKAGE_PATH/{slug}/...` resolves relative to
wherever the json ended up. Moving the `{slug}/` folder in beside the json
fixes it.

A milder symptom, where HDAs and toolbars appear while native plugins do
not, means the package file Houdini is reading is not the one the archive
shipped. See
[Installing an archive without HPM](houdini-integration.md#installing-an-archive-without-hpm).

## Cleaning up

### `hpm clean` removes nothing

`hpm clean` only removes what no project uses, and it finds projects through
`[projects]` in `~/.hpm/config.toml`. With no projects found it removes
nothing at all. See [Keep disk usage down](guide-housekeeping.md).

### Starting over

Everything under `~/.hpm/` except `config.toml` and `global/` can be
rebuilt. To reset one project:

```sh
rm -rf .hpm/
hpm install
```

Keep `hpm.lock`: it is what makes the reinstall identical.

To reset everything, remove `~/.hpm/packages`, `~/.hpm/venvs`,
`~/.hpm/cache` and `~/.hpm/fetch`, then run `hpm install` in each project.
Removing `~/.hpm/global/` as well orphans the package files `hpm global`
wrote into Houdini's preferences; run `hpm global remove` first.

## Getting help

- `hpm --help` and `hpm <command> --help`.
- Report bugs at <https://github.com/3db-dk/hpm/issues>.
