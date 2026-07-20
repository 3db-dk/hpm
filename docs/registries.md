# Registries

A **registry** is where HPM looks up package metadata — names, versions,
download URLs, checksums, and dependency lists. HPM supports two flavors:

- **API registries** speak HTTP and serve JSON from a handful of endpoints.
- **Git registries** are Cargo-style indexes: a Git repository with one JSON-lines file per package.

This guide covers how to add and manage registries, how HPM resolves through
them, and how to configure them per-user vs. per-project.

## Table of contents

- [Adding a registry](#adding-a-registry)
- [Per-user vs per-project](#per-user-vs-per-project)
- [Targeting a specific registry from a dependency](#targeting-a-specific-registry-from-a-dependency)
- [Refreshing and removing](#refreshing-and-removing)
- [Auto-detection of registry type](#auto-detection-of-registry-type)
- [Searching](#searching)
- [Caching](#caching)

## Adding a registry

```sh
hpm registry add <URL> [--name <alias>] [--type api|git] [--if-not-exists]
```

Examples:

```sh
# API registry
hpm registry add https://api.tumbletrove.com/v1/registry --name tumbletrove

# Git-index registry (explicit)
hpm registry add https://github.com/studio/hpm-packages.git --name studio --type git

# No --name: HPM infers an alias from the URL
hpm registry add https://api.studio.com/registry
# → added as "registry"

# Idempotent add: succeed silently if a registry with this name already
# exists, instead of erroring. Useful in provisioning scripts.
hpm registry add https://api.tumbletrove.com/v1/registry --name tumbletrove --if-not-exists
```

By default, adding a registry whose name already exists is an error. Pass
`--if-not-exists` to treat that as a no-op (exit 0) so automated setup can
re-run safely.

This writes to `~/.hpm/config.toml`:

```toml
[[registries]]
name = "tumbletrove"
url = "https://api.tumbletrove.com/v1/registry"
type = "api"

[[registries]]
name = "studio"
url = "https://github.com/studio/hpm-packages.git"
type = "git"
```

### Listing

```sh
hpm registry list
```

## Per-user vs per-project

Registries can be declared in two places:

1. **Per-user** — `~/.hpm/config.toml`. Managed by `hpm registry add`/`remove`/`list`. Applies to every project you work on.
2. **Per-project** — `hpm.toml` under `[[registries]]`. Applies only to that project. Additive to per-user registries.

Per-project registries are useful when a studio wants each project to pin
the registries it resolves against:

```toml
# hpm.toml
[[registries]]
name = "tumbletrove"
url = "https://api.tumbletrove.com/v1/registry"
type = "api"

[[registries]]
name = "studio"
url = "https://packages.studio.com/v1/registry"
type = "api"

[dependencies]
"studio/internal-tool" = { version = "1.0.0", registry = "studio" }
```

Every team member who clones the project gets the same registry set, without
needing to run `hpm registry add` themselves.

## Targeting a specific registry from a dependency

By default, HPM resolves a dependency by querying every configured registry
in order and taking the first match. To pin a dependency to one registry,
use the detailed dependency form:

```toml
[dependencies]
"studio/internal-tool" = { version = "1.0.0", registry = "studio" }
```

Or from the command line, which records the pin in `hpm.toml` for you:

```sh
hpm add studio/internal-tool@1.0.0 --registry studio
```

The pin is **authoritative**. Resolution, install, and `hpm update` consult
only the named registry — there is no fallback to the rest of the set, since
falling back is exactly what a pin exists to prevent. Naming a registry that
is not configured is an error rather than a silent search elsewhere.

This is useful when:

- A package exists under the same name in multiple registries and you want to be unambiguous.
- A private registry should always win over a public one for specific packages.
- You want a dependency's source to survive someone else adding a registry that happens to carry the same name.

Note the lock file does not record which registry a dependency came from —
it pins the version, checksum, and resolved URL. The `registry` key in
`hpm.toml` is what makes the source reproducible.

## Refreshing and removing

```sh
hpm registry update           # refresh every configured registry's cache
hpm registry remove studio    # drop a registry from the config
```

`hpm registry update` does the right thing for each type:

- **API** registries: nothing to do — API registries are always queried live,
  so there is no cache to invalidate. HPM reports `OK (live)`.
- **Git** registries: `git pull` the index repository to pick up new packages and versions.

Run `hpm registry update` after a new version is published to a **Git**
registry. For API registries a newly published version is visible
immediately, with no refresh step.

## Auto-detection of registry type

If you don't pass `--type`, `hpm registry add` infers it from the URL:

| URL pattern | Inferred type |
|-------------|---------------|
| Ends with `.git` | `git` |
| Contains `github.com` | `git` |
| Contains `gitea` | `git` |
| Anything else | `api` |

Override with `--type api` or `--type git` when the heuristic gets it wrong.

## Searching

```sh
hpm search <query>
```

`hpm search` queries each configured registry in turn, in the order they
appear in the config. If no registries are configured, HPM prints a hint to
run `hpm registry add` and exits cleanly (emitting `[]` under
`--output json`).

With `--output json`, results are emitted as a JSON array suitable for
piping into other tooling:

```sh
hpm search geometry --output json | jq '.[].name'
```

Each entry includes the package name, version, optional description, and
optional Houdini compatibility string. A `yanked: true` entry signals that
the maintainer pulled that version; HPM still shows it in search results.

How a yank affects resolution depends on how you asked for the version:

- A **range** requirement (`^1`, `*`, `>=2, <3`) skips yanked versions when
  picking the highest match.
- An **exact** pin (`1.2.0`) still resolves, yanked or not. This is
  deliberate, so that a lockfile pinning a version that was later yanked
  keeps installing rather than breaking.

## Caching

HPM caches **Git** registry indexes under `~/.hpm/registry/<name>/`. The cache
is per-registry, not per-project, so multiple projects share the same clone.

- **Git cache**: a local clone of the index repository. Updated by `hpm registry update`.
- **API registries are not cached.** Every resolution and search hits the
  API live, so there is nothing under `~/.hpm/registry/` for them.

The cache is advisory — if it's corrupted or deleted, HPM re-fetches on the
next operation. Never edit it by hand.
