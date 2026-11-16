# Environment variables

## Read by hpm

| Variable | Used by | Meaning |
|----------|---------|---------|
| `HPM_SIGNING_KEY` | `hpm pack` | Ed25519 signing key. A value starting with `-----BEGIN` is the PEM itself; anything else is a path to it. `--key` overrides it. |
| `HPM_HOUDINI_MAJORS` | `hpm build` | Passed through to prepack scripts when `--houdini-majors` is not given. |
| `HOUDINI_USER_PREF_DIR` | `hpm global` | Overrides Houdini's preferences directory, with `__HVER__` expanded to the `X.Y` version, as Houdini does. |
| `RUST_LOG` | every command | Log filter, for example `RUST_LOG=debug` or `RUST_LOG=hpm_core::python=trace`. Replaces the level `-v` selects. |
| `HOME` (`USERPROFILE` on Windows) | every command | Where `~/.hpm` is. |

hpm has no variable for the location of its home or for credentials. Move
the home with [`[storage] home_dir`](configuration.md).

## Set by hpm for scripts

`hpm run`, and the `[stage].prepack` scripts `hpm build` runs, start each
script with:

| Variable | Set when | Value |
|----------|----------|-------|
| `HPM_PACKAGE_ROOT` | always | The directory holding `hpm.toml`. Also the script's working directory. |
| `VIRTUAL_ENV`, and the venv's `bin/` (or `Scripts\`) first on `PATH` | the script declares `python` or `requirements`, or `package-env = true` | The script's venv. |
| `PYTHONPATH` | `package-env = true` | The package's `python/`, each dependency's `python/`, then the venv's `site-packages`. |
| `HPM_BUILD_PROFILE` | prepack scripts only | The `--profile` name; `release` by default. |
| `HPM_PLATFORM` | prepack scripts only, when `[compat].platforms` is declared | The target platform, for example `macos-aarch64`. |
| `HPM_HOUDINI_MAJORS` | prepack scripts only, when given | The `--houdini-majors` value, or the inherited variable. Space-separated majors, for example `21 22`. |

## Usable in `hpm.toml`

`$HPM_PACKAGE_ROOT` in a [`[runtime]`](manifest.md#runtime) value is
replaced with the installed package's directory when the Houdini package
file is written. Any other `$VAR` is left for Houdini to expand at startup.

## Set for Houdini

hpm sets nothing in your shell. What Houdini sees comes from the package
files hpm generates: `HOUDINI_PATH` (through `hpath`), `PYTHONPATH`,
`HOUDINI_SCRIPT_PATH`, and whatever the packages' `[runtime]` sections
declare. The one variable you set yourself is `HOUDINI_PACKAGE_PATH`; see
[How Houdini loads hpm packages](houdini-integration.md#pointing-houdini-at-a-project).

## For hpm's own test suite

| Variable | Meaning |
|----------|---------|
| `HPM_REQUIRE_HOUDINI=1` | Fail the Houdini conformance tests instead of skipping them when no Houdini is installed. |
| `HFS` | The Houdini installation those tests use. |
