# Python environments

How hpm provides Python packages to Houdini: which Python they are built
for, how the virtual environment is put together, when it is shared, and
when it is removed. For the steps to add a dependency, see
[Add Python dependencies to a package](guide-python-dependencies.md); for
the manifest syntax, see
[`[python_dependencies]`](manifest.md#python_dependencies).

## The problem

Houdini embeds its own Python interpreter. A package that imports numpy or
requests needs those installed for exactly that interpreter's version,
without installing into Houdini itself, and without disturbing the system
Python or other packages.

hpm's answer is a virtual environment per set of resolved packages, kept
under `~/.hpm/venvs/`, and put on `PYTHONPATH` by the package files Houdini
reads at startup. Houdini's own Python runs the code; the environment only
supplies `site-packages`.

## Which Python

Houdini ships one Python per release:

| Houdini | Python |
|---------|--------|
| 20.5 | 3.11 |
| 21.x | 3.11 |
| 22.x | 3.13 |

hpm takes the lower bound of the **project's** `[compat].houdini` and builds
for the matching Python. `">=20.5, <22"` uses 20.5; `"21"` and `"21.0"` mean
the same.

The project decides, not the packages. A package's own `[compat].houdini`
says which Houdini versions it supports. If it chose the Python, a project
on Houdini 22 using a package that declares `>=21` would get Python 3.11
packages, and compiled modules would fail to import in Houdini 22's Python
3.13.

A lower bound hpm has no mapping for (`>=23`, `>=19`, or something that is
not a version) is an error at install time. Houdini 19.x and 20.0 to 20.4
are not supported; their Python versions are past end of life.

## How an environment is built

During `hpm install`, after the packages are in the store:

1. Collect `[python_dependencies]` from every installed package. The same
   Python package declared twice must carry the identical constraint;
   optional entries are dropped.
2. Make sure a CPython of the right version exists under
   `~/.hpm/uv-python/`, downloading it if not.
3. Resolve the constraints to exact versions with `uv pip compile`.
4. Hash the Python version and the resolved `name: version` pairs. The
   first 16 hex characters of the SHA-256 name the environment:
   `~/.hpm/venvs/<hash>/`.
5. If that directory already holds every resolved package, use it.
   Otherwise create it and install into it.
6. Record the packages that use the environment in its `metadata.json`.
7. Add the environment's `site-packages` to `PYTHONPATH` in the package
   file of each package that declares Python dependencies.

Resolution in step 3 runs on every install. `hpm.lock` does not pin Python
packages, so a loose constraint can resolve to a newer release later, which
produces a different hash and a new environment beside the old one.

## Sharing

The environment's name depends only on what was resolved, so any two
projects whose packages resolve to the same set use the same directory:

```
Project A: numpy 2.1.3, requests 2.32.3     -> a1b2c3d4e5f60718
Project B: numpy 2.1.3, requests 2.32.3     -> a1b2c3d4e5f60718  (same directory)
Project C: numpy 2.2.0, requests 2.32.3     -> f6e5d4c3b2a19008
```

Extras affect the name only through the packages they pull in.

Within one project, all packages share one environment. That is why two
packages cannot ask for incompatible versions of the same Python package:
Houdini has one `sys.path`.

## Environments for scripts

[`[scripts]`](manifest.md#scripts) use the same machinery outside Houdini:

- A script with `python` or `requirements` gets an environment holding only
  those requirements.
- A script with `package-env = true` runs in the project's full environment:
  the Python dependencies of the project's own manifest and of its installed
  packages, with each package's `python/` folder on `PYTHONPATH`.

Note the difference from what Houdini gets: the project's own
`[python_dependencies]` are part of the `package-env` environment, and not
part of the environment built for Houdini.

## Isolation

hpm runs its own pinned `uv`, downloaded on first use to
`~/.hpm/tools/uv-<version>/` and checked against a SHA-256 compiled into
hpm. Its cache, configuration and CPython builds live under `~/.hpm/` too.
A `uv` you installed yourself is neither used nor affected, and nothing is
installed into the system Python or into Houdini.

All of these paths follow [`[storage] home_dir`](configuration.md).

## Cleanup

Environments are a cache. Deleting one is always safe; the next install
rebuilds it.

`hpm clean --python-only` and `hpm clean --comprehensive` remove
environments nothing uses:

- An environment that records its users is removed when none of those
  packages is installed any more.
- An environment with no recorded users, which is what a script's own
  environment looks like, is removed once it has gone unused for 30 days.

See [Keep disk usage down](guide-housekeeping.md).

## Resources

- [uv documentation](https://docs.astral.sh/uv/)
- [PEP 440: version specifiers](https://peps.python.org/pep-0440/)
- [Houdini package system](https://www.sidefx.com/docs/houdini/ref/plugins.html)
