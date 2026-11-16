# Configuration

hpm's own settings live in `config.toml`. This is separate from
[`hpm.toml`](manifest.md), which describes a package or project. Most users
need only two parts of it: `[[registries]]`, which `hpm registry add` writes
for you, and `[projects]`, which tells `hpm clean` what is still in use.

HPM reads `~/.hpm/config.toml` if it exists, then `<cwd>/.hpm/config.toml`
(project override) if it exists. Layering is presence-aware: each file
overrides exactly the values it sets, so a project config that omits a
section (or a single key) leaves the user-level value in place, and
anything no file sets falls back to the built-in default. Setting only
`storage.home_dir` re-derives the cache/packages/registry directories
under it unless they are set explicitly. A malformed config file is a
hard error, not a silent fallback to defaults.

A project config may set only `[install]`. A project directory is often a
repository someone else wrote, so `[storage]`, `[projects]`, `[signing]` and
`[[registries]]` (where hpm keeps its store and tools, what `hpm clean`
treats as live, which key `hpm pack` signs with, and which servers an
embedder's sign-in token may be sent to) are read only from
`~/.hpm/config.toml`; in a project config they are ignored with a warning
naming the section. A project declares the registries it resolves from in
`hpm.toml`'s `[[registries]]`.

```toml
[install]
parallel_downloads = 8       # packages a sync fetches and installs at once

[storage]
home_dir = "/Users/me/.hpm"                # default: $HOME/.hpm
cache_dir = "/Users/me/.hpm/cache"
packages_dir = "/Users/me/.hpm/packages"
registry_cache_dir = "/Users/me/.hpm/registry"

[projects]
explicit_paths = [
    "/Users/me/studio/pipeline",
]
search_roots = [
    "/Users/me/houdini-projects",
]
max_search_depth = 3
ignore_patterns = [".git", ".hg", ".svn", "node_modules", "backup", "archive", ".cache", "temp", "tmp"]

[[registries]]
name = "tumbletrove"
url = "https://api.tumbletrove.com/v1/registry"
type = "api"

[signing]
key_path = "/Users/me/.hpm/signing.pem"    # fallback for `hpm pack`
```

## What each section controls

**`[install]`**
- `parallel_downloads` — the most registry or URL packages a sync fetches
  and installs at once (default 8; must be at least 1).
- `path` — written by older hpm versions but never used (projects always
  install into `<project>/.hpm/packages`). It is still accepted, with a
  warning, so an existing config keeps loading; remove it.

Any other key in a config file is an error that names it.

**`[storage]`**
- `home_dir` — HPM's root on disk. Default `$HOME/.hpm` on every platform (Linux, macOS, Windows). All other storage paths derive from this by default.
- `cache_dir`, `packages_dir`, `registry_cache_dir` — override individual subdirectories without moving the whole root.

**`[projects]`** — drives `hpm clean`'s orphan detection.
- `explicit_paths` — absolute project paths that are always considered active.
- `search_roots` — directories scanned recursively for active projects.
- `max_search_depth` — recursion limit for search_roots (default `3`).
- `ignore_patterns` — directory names to skip during scanning. Each matches a whole directory name exactly (not a prefix or a glob): `temp` skips `temp/` but not `temporal-fx/`.

**`[[registries]]`** — list of registries. Same shape as
[`[[registries]]`](manifest.md#registries-array) in `hpm.toml`.

**`[signing]`**
- `key_path` — fallback Ed25519 PKCS#8 PEM path for `hpm pack` when neither `--key` nor `HPM_SIGNING_KEY` is set.
