# Publish a package

Publishing has two halves. hpm does the first: it builds the archive and
tells you its checksum. The second is yours: put the archive somewhere it
can be downloaded, and tell a registry where it is. hpm has no `publish`
command and no upload step.

## 1. Prepare the release

- Set the real `path` (`creator/slug`) and bump `version` in `hpm.toml`. A
  published version is permanent: registries and lock files identify the
  bytes by it. Fix mistakes with a new version.
- Declare the Houdini range you tested in `[compat].houdini`.
- Declare dependencies on other packages as ranges you have tested, for
  example `"acme/core" = ">=1.2, <2"`. See
  [versions and ranges](manifest.md#versions-and-ranges).
- Add a `README.md` and a license file.

```sh
hpm check
```

## 2. Declare what the package provides (optional)

`[[operators]]` lists the node types in the package, so a registry can
search by node name:

```toml
[[operators]]
kind      = "hda"
type_name = "acme::rock_scatter::1.0"
label     = "Rock Scatter"
category  = "Sop"
source    = "otls/rock_scatter.hda"
```

See [`[[operators]]`](manifest.md#operators).

## 3. Pack

```sh
hpm pack --json --verify-assets --output release/
```

```json
{
  "archive": "release/rock-tools-1.0.0.zip",
  "sha256": "40c67f243a852eff6b08ebf52bd15757115db71e0a509fad67bade62ce3c436f",
  "signature": null,
  "key_id": null,
  "platform": null,
  "assets": []
}
```

Keep this output. The registry needs `sha256`, and `platform` when it is
set. Read the archive's name from it rather than constructing it.

A package that declares `[compat].platforms` is packed once per platform,
on that platform; see [Ship an HDK plugin](guide-hdk-plugin.md).

## 4. Sign (optional)

```sh
openssl genpkey -algorithm ed25519 -out signing.pem      # once; keep it secret, outside the package
openssl pkey -in signing.pem -pubout -out signing.pub.pem

hpm pack --json --key signing.pem
```

`signature` and `key_id` are then filled in. Be clear about what this buys:
hpm produces signatures and registries can store them, but **hpm does not
verify signatures on install**. They are for a registry or a pipeline that
checks them itself. See [Security](security.md#package-signing).

## 5. Host the archive

Upload the `.zip` anywhere that serves it over `https`: a release on your
code host, an object storage bucket, a studio file server. Do not change the
file after packing; the checksum and signature cover its exact bytes.

## 6. Register the version

Tell the registry the package path, version, download URL, checksum, and
platform. How depends on the registry, and on whether it accepts packages
from you at all: a registry decides who may publish to it.

- **A Git registry you control.** Append one line to the package's index
  file and push. See [Run a studio registry](guide-studio-registry.md).
- **An API registry.** hpm only reads from API registries; registering a
  version is done through whatever its operator provides. The fields it
  needs are the ones in the `hpm pack --json` output. See
  [Registry formats](registry-formats.md) for what such a registry serves.

Then check it from a clean project:

```sh
hpm registry update          # Git registries only
hpm search rock-tools
hpm add acme/rock-tools@1.0.0
```

## Without a registry

Two ways to hand a package to someone directly:

- **A URL dependency.** They add the archive's URL to their project:

  ```toml
  [dependencies]
  "acme/rock-tools" = { url = "https://packages.studio.example/rock-tools-1.0.0.zip", version = "1.0.0" }
  ```

  The key and `version` must match the archive's own `hpm.toml`.

- **No hpm at all.** The archive unzips to `rock-tools.json` beside a
  `rock-tools/` folder. Both go into a Houdini packages directory. See
  [Installing an archive without HPM](houdini-integration.md#installing-an-archive-without-hpm)
  for the options and the one Houdini install flow to avoid.

## SideFX's hpackage repository

SideFX validates the archive's package file more strictly than Houdini
does. `hpm pack --sidefx` produces a file it accepts, or fails and says
why:

```sh
hpm pack --sidefx
```

The main restriction: `[compat].houdini` must be a single lower bound such
as `">=21.0"`. A range with an upper bound, including `^21`, cannot be
published there. A `README.md` is required. Details are under
[`hpm pack`](cli.md#hpm-pack).

## Withdrawing a version

Mark it yanked in the registry. Ranges then skip it, while projects that
already pin that exact version keep installing. Do not delete or replace
the archive: every lock file that names it would start failing.
