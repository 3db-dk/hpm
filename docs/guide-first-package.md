# Create your first package

You will make a package with a Python module and a shelf, load it in Houdini
from a project, and build the archive you would publish. No registry is
involved.

## 1. Scaffold

```sh
hpm init rock-tools --description "Rock scattering tools"
```

```
Package structure:
├── .gitignore
├── README.md
├── config
├── hpm.toml
├── otls
├── presets
├── python
│   └── __init__.py
├── scripts
└── tests
```

The folders are Houdini conventions, not hpm ones. Anything Houdini finds
under a `HOUDINI_PATH` entry works: `otls/` for digital assets, `toolbar/`
for shelves, `python_panels/`, `viewer_states/`, `dso/`, and so on. Delete
the ones you do not use and add the ones you need.

## 2. Name it

`hpm init` writes a placeholder identity. Open `rock-tools/hpm.toml` and set
the real one:

```toml
[package]
path = "acme/rock-tools"       # creator/slug: lowercase letters, digits, hyphens
name = "Rock Tools"            # display name
version = "0.1.0"
description = "Rock scattering tools"
authors = ["You <you@example.com>"]
license = "MIT"
readme = "README.md"
keywords = ["houdini"]

[compat]
houdini = "^21"                # Houdini 21.x
```

`path` is how others will depend on the package. The creator part is your
account or studio name on the registry you publish to.

## 3. Add content

A Python module:

```python
# rock-tools/python/rock_tools.py
def hello():
    return "rocks"
```

A shelf. Create `rock-tools/toolbar/` and save a shelf file into it from
Houdini (right-click a shelf tab, **Edit Shelf Tab**, and choose a file under
that folder), or copy an existing `.shelf` file there.

A digital asset: save an HDA into `rock-tools/otls/`.

## 4. Check it

```sh
cd rock-tools
hpm check
```

`hpm check` validates the manifest and warns about what a published package
should have: a description, a README, a license file. Warnings do not fail
the command.

## 5. Use it from a project

Create a project next to the package and depend on it by path:

```sh
cd ..
hpm init shot-010 --bare
cd shot-010
hpm add acme/rock-tools --path ../rock-tools
```

```toml
[dependencies]
"acme/rock-tools" = { path = "../rock-tools" }
```

Start Houdini on the project:

```sh
HOUDINI_PACKAGE_PATH="$PWD/.hpm/packages" houdini
```

In Houdini's Python shell:

```python
>>> import rock_tools
>>> rock_tools.hello()
'rocks'
```

hpm put the package's `python/` folder on `PYTHONPATH` and the package root
on `HOUDINI_PATH`, which is why the module imports and the shelf and HDAs
appear.

A path dependency installs a *copy* of the package. After you change the
package, run `hpm install` in the project again and restart Houdini.
[Develop a package against a live Houdini](guide-live-development.md) shows
how to skip the copy.

## 6. Set an environment variable

Packages often need Houdini to know where something is. Declare it in the
package's `hpm.toml`:

```toml
[runtime]
ROCK_TOOLS_PRESETS = { method = "set", value = "$HPM_PACKAGE_ROOT/presets" }
```

`$HPM_PACKAGE_ROOT` becomes the installed package's directory. After
`hpm install` and a restart, `hou.getenv("ROCK_TOOLS_PRESETS")` returns it.
See [`[runtime]`](manifest.md#runtime).

## 7. Build the archive

```sh
cd ../rock-tools
hpm pack
```

This writes `rock-tools-0.1.0.zip` and prints its SHA-256. The archive
contains the package under a `rock-tools/` folder and a `rock-tools.json`
beside it, so it also works unzipped into a Houdini packages directory
without hpm.

Files ignored by `.gitignore` are left out. To control exactly what ships,
see [`[stage]`](manifest.md#stage).

## Next

- [Add Python dependencies](guide-python-dependencies.md) such as numpy.
- [Publish a package](guide-publishing.md).
- [The `hpm.toml` reference](manifest.md).
