# Python Guide

HPM bundles [uv](https://github.com/astral-sh/uv) and uses it to manage the
Python dependencies declared by Houdini packages. This guide covers how to
declare them, how HPM maps Houdini versions to Python versions, how venv
sharing works, and how cleanup and troubleshooting fit together.

## Table of contents

- [Overview](#overview)
- [Declaring dependencies](#declaring-dependencies)
- [Houdini to Python version mapping](#houdini-to-python-version-mapping)
- [Virtual environment sharing](#virtual-environment-sharing)
- [Houdini integration](#houdini-integration)
- [Cleanup](#cleanup)
- [Troubleshooting](#troubleshooting)
- [Technical reference](#technical-reference)

## Overview

HPM's Python layer solves one specific problem: Houdini packages frequently
want Python dependencies (numpy, pymongo, qtpy, watchdog, …) and those
dependencies must be available to Houdini's embedded Python interpreter at
the right ABI version, without interfering with the system Python or with
other Houdini packages.

HPM addresses this by:

- Resolving every package's `[python_dependencies]` with the bundled `uv`.
- Installing the resolved packages into a content-addressable virtual environment in `~/.hpm/venvs/<hash>/`.
- Sharing that venv across every package whose resolved dependency set hashes to the same value.
- Emitting a Houdini manifest per package that prepends the venv's `site-packages` onto `PYTHONPATH`.
- Automatically mapping the lower bound of `[compat].houdini` to a Python version compatible with Houdini's embedded interpreter.

The bundled `uv` and its caches (`~/.hpm/uv-cache/`, `~/.hpm/uv-config/`) are
fully isolated from any system `uv` you might have, so HPM never interferes
with other Python workflows.

## Declaring dependencies

Python dependencies live in the `[python_dependencies]` section of
`hpm.toml`. Two forms are supported:

```toml
[python_dependencies]

# Shorthand: version constraint only
numpy = ">=1.20.0"
requests = "^2.28.0"

# Detailed: version, extras, optional
scipy = { version = ">=1.7.0", extras = ["sparse"] }
matplotlib = { version = "^3.5.0", optional = true }
plotly = { version = ">=5.0.0", optional = true }
```

Version constraints follow PEP 440 (the same grammar pip and uv use):

| Specifier | Meaning |
|-----------|---------|
| `>=1.0.0` | Minimum version. |
| `^1.0.0` | Compatible release (`>=1.0.0, <2.0.0`). |
| `~=1.0.0` | Approximately equal (`>=1.0.0, <1.1.0`). |
| `==1.0.0` | Exact version. |
| `!=1.0.0` | Exclude a version. |
| `>1.0.0`, `<2.0.0` | Strict bounds. |

### Best practice: allow compatible updates

```toml
[python_dependencies]
numpy = "^1.20.0"     # allows 1.20.x, 1.21.x, …, but not 2.x
requests = ">=2.25.0" # minimum, with headroom for sharing
```

Avoid `==1.0.5`-style pins — they prevent venv sharing across packages that
would otherwise converge on the same resolved set, and they block legitimate
security patches. Avoid `*` — it lets `uv` pick a version your peers don't
have pinned, defeating the lock file's reproducibility guarantee.

## Houdini to Python version mapping

HPM reads `[compat].houdini` from the **project's** root manifest (the
`hpm.toml` of the project being installed/launched), extracts its lower
bound, and maps that to the Python version Houdini ships that interpreter
with:

| Houdini version    | Python version |
|--------------------|----------------|
| 20.5, 20.x (x ≥ 5) | 3.10           |
| 21.x               | 3.11           |
| 22.x               | 3.13           |

A range like `">=20.5, <22"` uses `20.5` for the mapping. Both `"21"` and
`"21.0"` are accepted as lower bounds — bare majors are treated as `major.0`.

The project's Houdini version is **authoritative** for venv ABI selection.
A dependency package's own `[compat].houdini` describes its compatibility
floor (the oldest Houdini it supports) — it does not influence which CPython
the venv targets. If it did, a project on Houdini 22 (Python 3.13) consuming
a `[compat].houdini = ">=21.0"` package would silently get a 3.11 venv whose
C-extension wheels would crash on import inside Houdini 22.

### Unsupported: Houdini 19.x and 20.0 – 20.4

These ship Python 3.7 and 3.9 respectively, both past upstream end-of-life.
HPM refuses to create venvs against them rather than installing a dead ABI.
If you need to run one of those Houdini versions, stay on HPM 0.7.x.

### No silent fallback

If `[compat].houdini`'s lower bound is unparseable (`"latest"`) or points at
a Houdini major outside this table (`">=23"`, `">=18"`), `hpm install`
**errors out** rather than silently picking a wrong Python — an
ABI-mismatched venv would let the install succeed and then break
C-extension imports (`pymongo`, `watchdog`, …) at Houdini launch instead.

```
Error: No Python version mapping for Houdini 23; supported versions are 20.5+, 21, 22.
Houdini 19.x (Python 3.7) and 20.0–20.4 (Python 3.9) are past EOL.
```

If you need a new Houdini major before HPM ships support for it, update the
mapping in `crates/hpm-python/src/dependency.rs::map_houdini_to_python_version`
and open a PR.

## Virtual environment sharing

When multiple packages resolve to the same set of `(python_version, packages,
versions, extras)`, HPM installs them once and shares the venv:

```
Package A: numpy==1.24.0, requests==2.28.0     → hash a1b2c3d4
Package B: numpy==1.24.0, requests==2.28.0     → hash a1b2c3d4   (shared with A)
Package C: numpy==1.25.0, requests==2.28.0     → hash f6e5d4c3   (different)
```

The hash is a SHA-256 over the sorted resolved set plus the Python version,
truncated to 12 hex characters. Any change to the resolved set — a newer
lockfile, a different extras list, a different Python version — produces a
new hash and therefore a new venv.

### Why this matters

- **Disk usage** drops dramatically for studios with many packages that all want, say, `numpy` and `qtpy`.
- **Install speed** — a matching hash means no resolution and no install, just a pointer from `.hpm/packages/{name}.json` to the existing venv.
- **Consistency** — every package sharing a venv sees the same transitive dependency versions.

The per-venv `metadata.json` records which HPM packages are using the venv,
which `hpm clean --python-only` uses to detect orphans.

### Per-script venvs

`[scripts]` entries can opt into the same venv machinery for out-of-process
hooks (Houdini setup wizards, lifecycle scripts, anything that runs *before*
or *outside* Houdini's embedded Python). The table form takes a `python`
version and inline `requirements`; `hpm run` resolves them through uv,
materializes a venv under `~/.hpm/venvs/<hash>/`, and prepends its `bin/`
(or `Scripts/`) to `PATH` for the script process. See
[`[scripts]`](user-guide.md#scripts) in the user guide for syntax. Two
scripts whose resolved closures match share storage with each other and,
where the resolved set happens to coincide, with `[python_dependencies]`
venvs.

## Houdini integration

Once `hpm install` has produced the venv, it writes a Houdini manifest per
dependency into `<project>/.hpm/packages/{name}.json`:

```json
{
  "hpath": ["/Users/me/.hpm/packages/studio/geometry-tools@1.0.0"],
  "env": [
    {
      "PYTHONPATH": {
        "method": "prepend",
        "value": "/Users/me/.hpm/venvs/a1b2c3d4e5f6/lib/python3.11/site-packages"
      }
    }
  ],
  "enable": "houdini_version >= '20.5'"
}
```

`method: "prepend"` delegates path-separator handling to Houdini, so the
same manifest works on Unix (`:`) and Windows (`;`) without embedding an
OS-specific joiner.

Point Houdini's `HOUDINI_PACKAGE_PATH` at `<project>/.hpm/packages` so these
manifests are picked up at launch:

```sh
export HOUDINI_PACKAGE_PATH="$PROJECT/.hpm/packages:$HOUDINI_PACKAGE_PATH"
houdini
```

Restart Houdini after any change.

Once loaded, your Python dependencies are importable from Houdini's Python
context:

```python
import hou
import numpy as np
import scipy.spatial

points = np.array([[0, 0, 0], [1, 1, 1], [2, 2, 2]])
tree = scipy.spatial.KDTree(points)
```

## Cleanup

`hpm clean` has Python-aware modes that use the venv metadata to detect
unused environments:

```sh
hpm clean --python-only --dry-run      # preview orphan venvs
hpm clean --python-only                # remove them
hpm clean --comprehensive              # packages + venvs in one pass
hpm clean --comprehensive --yes        # non-interactive, for scripts
```

HPM never removes a venv that a package in an active project still uses.
Active projects come from `[projects]` in `~/.hpm/config.toml` — see the
[user guide](user-guide.md#global-configuration).

Example output:

```
$ hpm clean --python-only --dry-run

Analyzing Python virtual environments for cleanup (dry run)...
Found 3 orphaned virtual environments that would be removed:
  - ~/.hpm/venvs/abc123def (145 MB, created 30 days ago)
  - ~/.hpm/venvs/def456ghi ( 89 MB, created 15 days ago)
  - ~/.hpm/venvs/ghi789jkl (234 MB, created  7 days ago)
Would free approximately: 468 MB
```

## Troubleshooting

### Conflicting versions

```
Error: Conflicting versions for package numpy:
  - studio/geometry-tools requires numpy>=1.20,<1.21
  - studio/mesh-tools requires numpy>=1.25
```

Options:

- Relax one of the constraints so a shared resolution exists.
- Mark one package's `numpy` as `optional = true` — it won't participate in resolution unless you opt in.
- Split the conflicting packages into separate projects, each with its own lock file and venv.

### Python packages aren't importable in Houdini

Check, in order:

1. `HOUDINI_PACKAGE_PATH` includes `<project>/.hpm/packages`. Print it in a shelf tool to confirm.
2. `.hpm/packages/{name}.json` exists for the offending package and has a `PYTHONPATH` entry.
3. The venv the `PYTHONPATH` points at exists and its `site-packages/` contains a `dist-info/` for the offending package. If it doesn't, upgrade past **0.7.2** — earlier versions had a `uv pip install --target` bug that left `site-packages` empty despite a successful install. 0.7.2 self-heals these legacy venvs on the next `hpm install`.

### uv fails to create a venv

Symptom: `hpm install` errors out before any packages install.

Likely causes and fixes:

- **`No interpreter found in virtual environments, managed installations, search path, or registry`.** HPM auto-installs a managed CPython matching the project's Houdini ABI on first launch. If the auto-install was interrupted (e.g. offline at the time), retry with a connection — `uv python install <ver>` will resume into `~/.hpm/uv-python/`. If you've upgraded from HPM ≤0.10.1 and previously hit this error, just retrying on the new version fixes it.
- **Python interpreter unavailable for the target version.** `uv` downloads interpreters on demand; a network failure at that step surfaces here. Retry, or clear the cache with `rm -rf ~/.hpm/uv-cache/ ~/.hpm/uv-python/` and retry.
- **Disk space.** Each venv is tens to hundreds of MB, plus ~50 MB for each managed CPython under `~/.hpm/uv-python/`; check `df`.
- **Permissions.** Ensure `~/.hpm/` is writable by your user.

```sh
RUST_LOG=debug hpm install               # full HPM debug logs
RUST_LOG=hpm_python=trace hpm install    # Python-specific
```

### Venv sizes are growing

HPM deliberately keeps venvs around so incremental installs stay fast. Run
`hpm clean --python-only --dry-run` periodically to see what could be
reclaimed, then `hpm clean --python-only` (or `--comprehensive`).

```sh
du -sh ~/.hpm/venvs/           # check total size
du -sh ~/.hpm/venvs/* | sort -h  # find the largest
```

## Technical reference

### Storage layout

```
~/.hpm/
├── packages/
│   └── creator/
│       └── slug@1.0.0/
├── venvs/
│   └── <hash>/                       # 12-char SHA-256 truncation
│       ├── pyvenv.cfg
│       ├── bin/python                # Unix; Scripts\python.exe on Windows
│       ├── lib/python3.11/site-packages/   # Lib\site-packages on Windows
│       └── metadata.json             # resolved deps + using packages
├── tools/
│   └── uv                            # bundled uv binary
├── uv-cache/                         # isolated uv cache
├── uv-config/                        # isolated uv config
├── uv-python/                        # managed CPython installs (UV_PYTHON_INSTALL_DIR)
└── cache/
```

### Content hash

```rust
// crates/hpm-python/src/types.rs (simplified)
pub fn calculate_content_hash(resolved: &ResolvedDependencySet) -> String {
    let mut hasher = Sha256::new();
    hasher.update(format!("python:{}", resolved.python_version));
    let mut packages: Vec<_> = resolved.packages.iter().collect();
    packages.sort_by_key(|(name, _)| name.as_str());
    for (name, spec) in packages {
        hasher.update(name.as_bytes());
        hasher.update(spec.version.as_bytes());
        for extra in spec.extras.iter().flatten() {
            hasher.update(extra.as_bytes());
        }
    }
    hex::encode(hasher.finalize())[..12].to_string()
}
```

The hash is stable across machines: give `uv` the same constraints and the
same index, and HPM's manifest generator and venv deduplication will agree
on the same 12-character prefix.

### Install flow

1. Collect `[python_dependencies]` from the root manifest and every installed HPM dependency's manifest.
2. Read `[compat].houdini` from the **root** manifest, extract its lower bound, and map it to a Python version. Per-package `[compat].houdini` is ignored for ABI selection.
3. Resolve the merged dependency set with `uv` (lockfile-aware).
4. Hash the resolved set + Python version → venv directory name.
5. If that directory exists and its `site-packages/` has a `dist-info/` for each resolved package, reuse it. Otherwise, delete and rebuild.
6. Run `uv pip install --python <venv>/bin/python` to populate `site-packages/`.
7. Write `metadata.json` with the resolved set and the list of HPM packages using the venv.
8. For each installed HPM package, write a `<project>/.hpm/packages/{name}.json` Houdini manifest that prepends the venv's `site-packages` onto `PYTHONPATH`.

### Resources

- [uv documentation](https://docs.astral.sh/uv/)
- [PEP 440 — version specifiers](https://peps.python.org/pep-0440/)
- [Houdini package system](https://www.sidefx.com/docs/houdini/ref/plugins.html)
