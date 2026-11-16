# Run package code outside Houdini

`[scripts]` gives a package named commands: a build step, a setup wizard, a
render-farm entry point. `hpm run <name>` runs one, and can give it a Python
environment without anyone installing anything by hand.

## A plain command

```toml
[scripts]
hello = "echo package root is $HPM_PACKAGE_ROOT"
```

```sh
hpm run hello -- a b
```

```
package root is /work/rock-tools a b
```

- The command runs through the system shell (`sh -c`, or `cmd /C` on
  Windows), from the directory holding `hpm.toml`.
- `HPM_PACKAGE_ROOT` is that directory.
- Everything after the script name is passed to the command.
- hpm exits with the command's exit code.

Use `-m <dir>` or `-C <dir>` to run a script of a package elsewhere.

## Different commands per OS

Shell syntax differs between `sh` and `cmd.exe` (`$VAR` against `%VAR%`),
so give `cmd` one variant per OS where it matters:

```toml
[scripts.register]
cmd = [
  { when = { os = "windows" }, set = "\"%HPM_PACKAGE_ROOT%\\bin\\tool.exe\" register" },
  { when = {},                 set = "\"$HPM_PACKAGE_ROOT/bin/tool\" register" },
]
```

The first matching variant runs; `when = {}` matches any OS.

## A script with its own Python packages

A script that needs Python packages declares them, and hpm builds a virtual
environment for it on first run:

```toml
[scripts.setup]
cmd          = "python scripts/setup_wizard.py"
python       = "3.11"
requirements = ["PySide6>=6.6", "pyyaml"]
```

```sh
hpm run setup
```

`python` in the command is then the environment's interpreter. The
environment contains only the listed `requirements`. It does not include
the package's own modules or its `[python_dependencies]`.

Use this for tools that stand apart from the package's code: installers,
code generators, linters.

## A script that imports the package

Code that ships in the package and runs outside Houdini, such as a farm
task or a command-line tool, needs the same imports it has inside Houdini.
`package-env = true` provides them:

```toml
[scripts.submit]
cmd         = "python -m rock_tools.submit"
package-env = true
```

```sh
hpm install        # once, so the project is locked and its packages are present
hpm run submit -- --shot 010
```

The script runs with:

- the project's and its dependencies' `[python_dependencies]` installed,
  for the Python of the project's Houdini;
- the package's `python/` folder and each dependency's `python/` folder on
  `PYTHONPATH`.

It does not provide `hou`. For that, the command itself must be `hython`
from a Houdini installation.

## On a render farm

A farm job that should run with a project's packages needs two lines on the
worker:

```sh
hpm install --frozen-lockfile -C /path/to/project
hpm run submit -C /path/to/project -- --shot 010
```

The first reproduces the project's lock on that machine; the second runs
inside its environment. Nothing about the environment is set up in the job
script.

## Scripts as build steps

`hpm build` runs the scripts named in `[stage].prepack` before it assembles
the package, so `[scripts]` is also where a compile step lives. See
[Ship an HDK plugin](guide-hdk-plugin.md).

## Reference

[`[scripts]`](manifest.md#scripts) and [`hpm run`](cli.md#hpm-run), including
the Windows rules for arguments.
