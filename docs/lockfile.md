# The `hpm.lock` lock file

`hpm install` writes `hpm.lock` beside `hpm.toml`. It records what was
installed, so that the next install, on any machine, gets the same package
bytes or fails. Commit it. Do not edit it by hand.

```toml
# HPM Lock File
# This file is auto-generated. Do not edit manually.

[package]
name = "shot-010"
version = "0.1.0"

[dependencies."acme/rock-tools"]
version = "0.1.0"
checksum = "v2:ce3d0027c2544dc52a41ed3715101bfda903b837f724b7438fc812b13351db3d"

[dependencies."acme/rock-tools".source]
type = "url"
url = "https://packages.studio.example/rock-tools-0.1.0.zip"
version = "0.1.0"

[dependencies."local/my-tools"]
version = "0.3.0"

[dependencies."local/my-tools".source]
type = "path"
path = "../my-tools"

[python_dependencies.numpy]
version = ">=1.26,<3"

[metadata]
generated_at = "2026-11-16T10:59:29Z"
hpm_version = "0.40.2"
platform = "linux-x86_64"
```

## What is pinned

| Entry | Pinned to | Checked on install |
|-------|-----------|--------------------|
| Registry and URL dependencies | The version, the download URL, and a checksum of the installed files. | Yes. A package the lock pins at the manifest's version must match its checksum, whether it is freshly downloaded or reused from the store. |
| Path dependencies | The version and the source path. No checksum: the source is expected to change. | No. |
| Python dependencies | **Not pinned.** The lock repeats the constraint from `hpm.toml`; `uv` resolves it again on every install. | No. |

So `hpm.lock` makes Houdini packages reproducible, and Python packages only
as reproducible as their constraints. Where you need the same Python
package version every time, pin it with `==` in `[python_dependencies]`.

## The checksum

`checksum` is a SHA-256 over the installed tree: every file's relative path
and contents, each framed by its length, written `v2:<hex>`. It is taken
after extraction, so it does not equal the SHA-256 of the archive that
`hpm pack` prints or a registry serves as `cksum`. Lock files written by
older versions hold a bare `<hex>` digest; those are still verified.

A lock entry whose version differs from the manifest's is stale and pins
nothing; the install replaces it.

## `--frozen-lockfile`

`hpm install --frozen-lockfile` fails instead of writing the lock when:

- `hpm.lock` does not exist;
- the installed versions, or the Python constraints, differ from what the
  lock records;
- a registry or URL dependency has no checksum in the lock.

Use it in CI and on render nodes, where a lock that needs to change means
someone forgot to commit it.

## Age

`metadata.generated_at` is when the lock was written. `hpm install` and
`hpm audit` warn when it is more than 90 days old, as a prompt to run
`hpm update`. The warning never blocks an install.

## Registries are not recorded

The lock stores the URL a package was downloaded from, not the registry
that supplied it. To keep a dependency coming from one registry, pin it in
the manifest with [`registry = "<name>"`](manifest.md#dependencies).
