# Files and directories

Every path hpm reads or writes. Other pages link here rather than repeat
the layout.

## The hpm home: `~/.hpm/`

The same location on Linux, macOS and Windows (`%USERPROFILE%\.hpm`). Move
it with [`[storage] home_dir`](configuration.md).

```
~/.hpm/
├── config.toml                   # your configuration (optional)
├── packages/                     # the store: installed packages
│   ├── utility-nodes@1.0.0/      #   one directory per slug@version
│   ├── utility-nodes@1.0.0.files.json   # install record, read by `hpm audit`
│   ├── utility-nodes@1.0.0.exec-modes   # marker for the executable-bit repair
│   └── _dev/                     #   path dependencies
│       └── my-tools@0.1.0/       #     a link, or a folder of copies named by content hash
├── venvs/                        # Python environments, shared between projects
│   └── a1b2c3d4e5f60718/         #   named by a hash of the resolved package set
├── global/                       # what `hpm global` installed
│   └── houdini-21.0.json         #   one ledger per Houdini version
├── registry/                     # clones of Git registry indexes
│   └── <digest of the URL>/
├── cache/                        # downloaded archives
├── fetch/                        # archives being extracted
├── tools/
│   └── uv-<version>/             # the uv hpm downloads on first use
├── uv-cache/                     # uv's cache, separate from any uv of your own
├── uv-config/
└── uv-python/                    # CPython builds uv downloads for the venvs
```

| Path | Safe to delete? |
|------|-----------------|
| `config.toml` | No. It holds your registries and settings. |
| `global/` | No. Without the ledger, hpm no longer knows which files it wrote into Houdini's preferences. Use `hpm global remove`. |
| `packages/` | Yes, with `hpm clean`. By hand it works too; the next `hpm install` downloads again. |
| `venvs/`, `cache/`, `fetch/`, `registry/`, `tools/`, `uv-cache/`, `uv-python/` | Yes. All are rebuilt or downloaded again when needed. |

The store is keyed by the package's **slug**, without the creator:
`acme/tools` and `other/tools` at the same version would use the same
directory. hpm refuses to store a download whose own `hpm.toml` declares a
different package than the one requested, so one cannot be served as the
other, but two such packages cannot be installed side by side.

## A project

```
<project>/
├── hpm.toml                      # the manifest. Commit.
├── hpm.lock                      # installed versions and checksums. Commit.
└── .hpm/                         # generated. Do not commit.
    ├── config.toml               # optional; may set [install] only
    └── packages/                 # the directory Houdini is pointed at
        ├── acme.utility-nodes.json        # one package file per dependency
        └── ~hpm-project-overrides.json    # the project's own [runtime] entries
```

`.hpm/packages/` belongs to hpm: an install removes any `.json` file there
that it does not recognise. hpm refuses to work in a project whose `.hpm` or
`.hpm/packages` is a symlink or junction.

## A package being developed

```
<package>/
├── hpm.toml
├── .hpmignore                    # optional; see [stage] in the manifest reference
├── dist/                         # `hpm build` output ([stage].output_dir)
└── <slug>-<version>[-<platform>].zip      # `hpm pack` output (or --output <dir>)
```

## Houdini's preferences

`hpm global` writes one file per package into the per-user, per-version
packages directory, and nothing else there:

| Platform | Directory |
|----------|-----------|
| Linux | `~/houdiniX.Y/packages/` |
| macOS | `~/Library/Preferences/houdini/X.Y/packages/` |
| Windows | `%USERPROFILE%\Documents\houdiniX.Y\packages\` |

The file is named `hpm-{creator}.{slug}.json`. `HOUDINI_USER_PREF_DIR`
overrides the location, as it does for Houdini.
