# Upgrade notes

Changes that can make a working setup stop working after an hpm upgrade,
newest first. The [changelog](https://github.com/3db-dk/hpm/blob/main/CHANGELOG.md)
has every change; this page has only the ones that need action.

Use **0.40.1 or newer**. 0.40.0 is a security release that closed several
ways a cloned project, a registry or an archive could write outside hpm's
directories or run commands; 0.40.1 fixes a Windows regression in it.

## 0.40

- **Plain `http` is refused** for API registries, download URLs and URL
  dependencies, except to the local machine. Serve them over `https`.
- **Git registry URLs** must be `https://`, `ssh://`, `file://`,
  `git@host:path` or a local path. `git://` and `http://` are refused.
  Indexes are cloned again, into `~/.hpm/registry/<digest>/`; the old
  `~/.hpm/registry/<name>/` directories can be deleted.
- **A project's `.hpm/config.toml` may set only `[install]`.** `[storage]`,
  `[projects]`, `[signing]` and `[[registries]]` there are ignored with a
  warning. Put them in `~/.hpm/config.toml`, and declare a project's
  registries in its `hpm.toml`.
- **`hpm install` stops on an unreadable `hpm.lock`** instead of replacing
  it. Fix or delete the file.
- **`--frozen-lockfile` requires a checksum** in the lock for every registry
  and URL dependency. Run `hpm install` once without the flag and commit
  the result.
- **A URL dependency must match its archive.** The key and `version` must
  equal the `path` and `version` in the archive's `hpm.toml`, and `version`
  must be a semantic version.
- **`[python_dependencies]` entries are plain requirements.** Direct URLs
  (`name @ url`), environment markers and characters outside version
  specifiers fail the manifest load.
- **Zip archives must use stored or deflated entries.** Repack an archive
  made with another compression method.
- **`[stage]` place rules**: a `to` with `..`, a leading `/`, a drive, a
  backslash or a colon fails the manifest load, and a staged file whose name
  contains a colon or backslash is refused.
- **Symlinks pointing outside the package** no longer pack or dev-install.
  Replace the link with the file.
- **A project whose `.hpm` or `.hpm/packages` is a symlink or junction** no
  longer installs. Make it a real directory.
- **On Windows, `hpm run` arguments** may not contain `"`, a line break, or
  a `%NAME%` reference to a set variable.
- The first Python install after upgrading downloads a newer `uv`.

## 0.39

- **`hpm pack` names the archive after the slug**:
  `{slug}-{version}[-{platform}].zip`, not the display name. Read the path
  from `hpm pack --json`.

## 0.38

- **Unknown keys in a dependency table are errors.** `registy = "..."` or
  `optinal = true` used to be dropped silently. The rule applies to every
  `hpm.toml` hpm reads, including packages already in the store.
- **Exit codes changed.** `hpm audit` exits 1 on an integrity failure, and
  `hpm registry update` exits 1 when a registry could not be refreshed. A
  flag value hpm does not accept is a usage error, exit 2.
- **`hpm --output json update` needs `--dry-run` or `--yes`.**
- **Global flags** `-v`, `-q`, `--color` and `-C` work on either side of the
  subcommand. `--output <format>` goes before it.
- **`hpm init` no longer writes a root `package.json`.**

## 0.29

- **Generated package files are named `{creator}.{slug}.json`.** The next
  install renames them; nothing to do unless a launcher referred to the old
  `{slug}.json` names.
- **`registry = "<name>"` on a dependency is authoritative.** It no longer
  falls back to other registries.

## 0.28: old manifests no longer load

The manifest format used before 0.16 is rejected, as is any unknown section
or key. If `hpm.toml` fails with an unknown field named `houdini`, `env`,
`dev` or `native`, rewrite those sections:

| Old | New |
|-----|-----|
| `[houdini]` with `min_version = "20.5"` | `[compat]` with `houdini = ">=20.5"` |
| `[env]`, and `[dev.env]` for the same key | One [`[runtime]`](manifest.md#runtime) entry. Where the two differed, give `value` one variant per `install_source` (see [conditional values](manifest.md#conditional-values)). |
| `[native]` with `platforms = [...]` and `[native.<platform>]` `files = [...]` | `[compat]` `platforms = [...]` and [`[stage.platform.<platform>]`](manifest.md#stage) `place = [{ from, to }]` |

Other changes in 0.28:

- The manifest flag is `-m/--manifest` on every command; `-p/--package` is
  gone.
- A Git registry must use the `creator/slug` layout, and platform tags must
  be the full identifiers (`linux-x86_64`, not `LINUX`).

## Houdini versions

Houdini 19.x and 20.0 to 20.4 are not supported: their Python versions are
past end of life, and hpm refuses to build an environment for them.
