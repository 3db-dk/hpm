# Registry formats

What a registry has to serve for hpm to resolve packages from it. Read this
to host a registry of your own; to *use* one, see [Registries](registries.md),
and for a walkthrough see [Run a studio registry](guide-studio-registry.md).

A registry holds metadata only. Archives live wherever their `dl` URL
points: a release page, an object store, a file server.

## The entry

Both registry types describe a published archive with the same JSON object:

```json
{
  "name": "acme/rock-tools",
  "vers": "1.2.0",
  "dl": "https://packages.example.com/rock-tools-1.2.0.zip",
  "cksum": "sha256:40c67f243a852eff6b08ebf52bd15757115db71e0a509fad67bade62ce3c436f",
  "houdini_compat": ">=21, <23",
  "platform": "linux-x86_64",
  "description": "Rock scattering tools",
  "deps": [{ "name": "acme/core", "req": ">=1.0, <2" }]
}
```

| Field | Required | Meaning |
|-------|----------|---------|
| `name` | yes | The package path, `creator/slug`. |
| `vers` | yes | A semantic version. |
| `dl` | yes | Download URL of the archive (`.zip` or `.tar.gz`). `https`, or `http` to the local machine only. |
| `cksum` | no | SHA-256 of the archive bytes, as `sha256:<hex>` or bare hex. This is the `sha256` that `hpm pack --json` prints. Without it the package installs with a warning and no download check. |
| `houdini_compat` | no | The package's `[compat].houdini` range. Shown by `hpm search`, and used by clients that choose versions. |
| `platform` | no | The platform this archive is built for: one of the [platform identifiers](manifest.md#stage), or `universal`. Absent means universal. |
| `yanked` | no | `true` withdraws the version: ranges skip it, an exact request still gets it. |
| `deps` | no | The version's registry dependencies, as `{ name, req }`. Read by clients that solve a whole project; `hpm install` ignores it. |
| `description`, `author`, `created_at` | no | Shown in search results. |
| `sig`, `kid` | no | Ed25519 signature and key id from `hpm pack --key`. Stored but not checked by hpm; see [Security](security.md#package-signing). |

One entry is one archive. A version built for three platforms is three
entries that differ in `dl`, `cksum` and `platform`. On install hpm takes
the entry for the host platform, else a universal one, else fails.

The archive itself must contain an `hpm.toml` declaring the same `name` and
`vers`; hpm refuses one that declares another package. `hpm pack` output
always qualifies.

## Git registry

A Git repository laid out like this:

```
index/
├── config.json           # must exist; `{}` is enough
└── acme/                 # one directory per creator
    ├── rock-tools.json   # one file per package
    └── core.json
```

Each package file holds one entry per line (JSON Lines), one line per
published archive:

```
{"name":"acme/rock-tools","vers":"1.1.0","dl":"https://.../rock-tools-1.1.0.zip","cksum":"sha256:..."}
{"name":"acme/rock-tools","vers":"1.2.0","dl":"https://.../rock-tools-1.2.0.zip","cksum":"sha256:..."}
```

Publishing a version is appending a line and pushing. Withdrawing one is
adding `"yanked":true` to its line.

hpm clones the repository to `~/.hpm/registry/` on first use and reads the
clone afterwards. It fetches again only on `hpm registry update`.

The registry URL may be `https://`, `ssh://`, `file://`, `git@host:path`, or
a local or network path. `git://` and `http://` are refused. Access control
is Git's: whoever can clone the repository can resolve from it.

## API registry

An HTTPS service with three read-only endpoints under a base URL, which is
what `hpm registry add` takes:

| Request | Response |
|---------|----------|
| `GET {base}/packages?q={query}` | `{ "packages": [entry, ...], "total": n }` with the latest version of each match |
| `GET {base}/packages/{creator}/{slug}` | `{ "versions": [entry, ...] }` with every archive of every version |
| `GET {base}/packages/{creator}/{slug}/{version}` | `{ "builds": [entry, ...] }` with every archive of that version |

An unknown package or version is a `404`. A response may be at most 64 MiB.

API registries are queried live on every resolution; nothing is cached, and
`hpm registry update` has nothing to do for them.

The `hpm` command line sends no credentials, so it can resolve public
packages only. Applications that embed hpm, such as TumbleTrove Desktop, can
attach a bearer token; it is sent only to `https` registries configured for
the user, never to one a project declares.

## Limits

These apply to anything hpm downloads, from either registry type or from a
URL dependency:

- Zip entries must be stored or deflated. Other compression methods (bzip2,
  LZMA, zstd, ...) and encrypted entries fail to extract.
- An archive may expand to at most 1,000,000 entries and 256 GiB.
- Entry names with an absolute path, a `..` component, a backslash or a
  colon are refused. Symlink entries are skipped.
- A connection must open within 30 seconds, and a download that sends
  nothing for 2 minutes fails.
