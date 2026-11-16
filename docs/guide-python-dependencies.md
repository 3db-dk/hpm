# Add Python dependencies to a package

Your package's code imports numpy, or requests, or PySide helpers. Declare
them, and hpm installs them for the right Python and makes them importable
inside Houdini, without touching Houdini's own Python or the system one.

This continues from [Create your first package](guide-first-package.md).

## 1. Declare them in the package

In the package's `hpm.toml` (the package whose code does the importing):

```toml
[python_dependencies]
numpy = ">=1.26,<3"
requests = { version = ">=2.31", extras = ["socks"] }
```

Versions use pip's syntax (PEP 440), which differs from the syntax of
`[dependencies]`:

| To say | Write |
|--------|-------|
| At least 1.26 | `">=1.26"` |
| 1.26 or newer, below 3 | `">=1.26,<3"` |
| Compatible with 1.26 (1.x, at least 1.26) | `"~=1.26"` |
| Exactly 1.26.4 | `"==1.26.4"` |
| Any version | `"*"` |

Two things that look right and are not: a bare `"1.26.4"` (write
`"==1.26.4"`), and the caret `"^1.26"` (write `"~=1.26"`).

## 2. Install in the project

```sh
cd ../shot-010
hpm install
```

The first time, this downloads `uv` and a CPython build into `~/.hpm/`.
Then it resolves the constraints, creates a virtual environment under
`~/.hpm/venvs/`, and adds its `site-packages` to the package's `PYTHONPATH`
in the generated package file.

## 3. Use them in Houdini

Restart Houdini on the project and, in the Python shell:

```python
>>> import numpy
>>> numpy.__version__
'2.1.3'
```

## Which Python?

The one your Houdini ships. hpm reads it from the lower bound of the
**project's** `[compat].houdini`:

| Project's Houdini | Python |
|-------------------|--------|
| 20.5 | 3.11 |
| 21 | 3.11 |
| 22 | 3.13 |

So set the project's `[compat].houdini` to the Houdini you actually launch.
A project declaring `^21` and launched in Houdini 22 gets Python 3.11
packages in a Python 3.13 interpreter, and compiled modules such as numpy
fail to import. The package's own `[compat].houdini` says what it supports;
it does not choose the Python.

## Where to declare what

Declare a Python dependency in the package that imports it. `hpm install`
builds Houdini's environment from the installed packages' declarations.
`[python_dependencies]` in the *project's* own `hpm.toml` are not part of
that environment; they are used when the project runs its own
[`package-env` scripts](guide-scripts.md).

## Keeping versions stable

`hpm.lock` pins Houdini packages, not Python packages. `uv` resolves your
constraints again on each install, so `>=1.26` can produce a newer numpy next
month. Where that matters, pin:

```toml
[python_dependencies]
numpy = "==2.1.3"
```

The trade-off is sharing, described next.

## Several packages, one environment

All of a project's packages share one virtual environment, so every package
sees the same version of numpy. Two consequences:

- **The same Python package must be declared with the same constraint
  everywhere.** If one package says `numpy = ">=1.26"` and another
  `numpy = ">=1.26,<3"`, install fails with
  `Conflicting versions for package numpy`. Agree on one spelling across
  your packages.
- **Projects whose packages resolve to the same set share the environment
  on disk.** Loose, identical constraints across a studio's packages keep
  the number of environments small.

## Dependencies you do not want installed

`optional = true` records a dependency without installing it:

```toml
[python_dependencies]
matplotlib = { version = ">=3.8", optional = true }
```

hpm skips it. Use it to document something your code imports only if the
user has installed it themselves.

## If it goes wrong

See [Troubleshooting](troubleshooting.md#uv-fails-while-resolving-python-dependencies),
and [Python environments](python-guide.md) for how the environments are
built, shared and cleaned up.
