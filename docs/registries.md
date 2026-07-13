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
hpm registry add https://api.3db.dk/v1/registry --name houdinihub

# Git-index registry (explicit)
hpm registry add https://github.com/studio/hpm-packages.git --name studio --type git

# No --name: HPM infers an alias from the URL
hpm registry add https://api.studio.com/registry
# → added as "registry"

# Idempotent add: succeed silently if a registry with this name already
# exists, instead of erroring. Useful in provisioning scripts.
hpm registry add https://api.3db.dk/v1/registry --name houdinihub --if-not-exists
```

By default, adding a registry whose name already exists is an error. Pass
`--if-not-exists` to treat that as a no-op (exit 0) so automated setup can
re-run safely.

This writes to `~/.hpm/config.toml`:

```toml
[[registries]]
name = "houdinihub"
url = "https://api.3db.dk/v1/registry"
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
name = "houdinihub"
url = "https://api.3db.dk/v1/registry"
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

This is useful when:

- A package exists under the same name in multiple registries and you want to be unambiguous.
- A private registry should always win over a public one for specific packages.
- You want the lockfile to record which registry resolved the dependency, so audits can answer "where did this come from".

## Refreshing and removing

```sh
hpm registry update           # refresh every configured registry's cache
hpm registry remove studio    # drop a registry from the config
```

`hpm registry update` does the right thing for each type:

- **API** registries: invalidate the metadata cache under `~/.hpm/registry/<name>/`.
- **Git** registries: `git pull` the index repository to pick up new packages and versions.

Run `hpm registry update` when a new version has been published and you want
to pick it up without waiting for cache expiry.

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

`hpm search` queries every configured registry in parallel. If no registries
are configured, HPM prints a hint to run `hpm registry add` and exits cleanly.

With `--output json`, results are emitted as a JSON array suitable for
piping into other tooling:

```sh
hpm search geometry --output json | jq '.[].name'
```

Each entry includes the package name, version, optional description, and
optional Houdini compatibility string. A `yanked: true` entry signals that
the maintainer pulled that version; HPM still shows it in search results
but `hpm install` will refuse to use it.

## Caching

HPM caches registry metadata under `~/.hpm/registry/<name>/`. The cache is
per-registry, not per-project, so multiple projects share the same cache.

- **API cache**: response bodies for the endpoints HPM hits during resolution. Cleared by `hpm registry update` or by deleting the directory.
- **Git cache**: a local clone of the index repository. Updated by `hpm registry update`.

The cache is advisory — if it's corrupted or deleted, HPM re-fetches on the
next operation. Never edit it by hand.
