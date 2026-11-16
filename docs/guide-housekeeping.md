# Keep disk usage down

hpm never deletes a package on its own. `hpm remove` and `hpm update` leave
the old version in the store, because another project may use it. `hpm
clean` removes what nothing uses, and to do that it has to know where your
projects are.

## 1. Tell hpm where your projects are

`hpm clean` treats a package as in use when a project it knows about depends
on it. It finds projects through `[projects]` in `~/.hpm/config.toml`:

```toml
[projects]
search_roots   = ["/home/me/houdini-projects", "/mnt/shows"]
explicit_paths = ["/home/me/pipeline-tools"]
max_search_depth = 3
```

- `search_roots` are scanned for directories containing an `hpm.toml`, down
  to `max_search_depth` levels.
- `explicit_paths` are individual projects that are always counted.

**Until this is set, `hpm clean` removes nothing.** With no projects, every
package would look unused, so it stops instead:

```
No HPM-managed projects found - skipping cleanup to prevent removing all packages
```

The reverse also holds: a project outside these locations is invisible, and
its packages will be removed. They are not lost; the next `hpm install` in
that project downloads them again.

Packages installed with `hpm global add` are always kept.

## 2. Look before deleting

```sh
hpm clean --dry-run
```

```
Found 1 orphaned dev installs:
  - _dev/rock-tools@0.1.0

Run 'hpm clean' to remove these packages
```

## 3. Clean

```sh
hpm clean                    # packages and development copies; asks first
hpm clean --yes              # without asking
hpm clean --python-only      # only Python environments
hpm clean --comprehensive    # packages, development copies and Python environments
```

Close Houdini sessions first. A session started from a project you have
since reinstalled may still be reading a development copy that hpm now
considers replaced.

## What uses the space

```sh
du -sh ~/.hpm/*
```

| Directory | Holds | Reclaim with |
|-----------|-------|--------------|
| `packages/` | Installed packages | `hpm clean` |
| `venvs/` | Python environments | `hpm clean --python-only` |
| `cache/` | Downloaded archives | Delete it; archives are fetched again when needed |
| `uv-cache/` | Downloaded Python wheels | Delete it |
| `uv-python/` | CPython builds | Delete it; fetched again on the next Python install |

A Python environment is removed when none of the packages that used it are
installed any more. One with no recorded users, which is what a
[script's own environment](guide-scripts.md) looks like, is removed after 30
days without use.

## A shared or larger disk

To move everything hpm stores:

```toml
[storage]
home_dir = "/fast-disk/hpm"
```

Generated package files contain absolute paths into the store, so run
`hpm install` in each project after moving it.

Do not point several users at one store on a shared volume unless they can
all be trusted with each other's Houdini sessions: whoever can write to the
store can change what the others load.
