# Proposal: solid `.tar.zst` release archives

Status: prototype on branch `feat/solid-archive-tzst` (not released, not
in `SUMMARY.md`). Nothing about registries or published packages changes
until this is accepted.

## Why

`hpm pack` writes a zip, and zip compresses every file on its own. A package
that ships many near-identical files gets no benefit from the overlap: every
version of a compiled graph, scenes that differ by a few nodes, the same
helper copied into several folders. A solid archive compresses the whole tree
as one stream, so the second copy of a file costs a few bytes.

Measured on a real 3,594-file release (762 MB unpacked), repacked from its
own files by `solid_bench` (see below):

| archive | size | pack | install (extract) | tree |
|---|---|---|---|---|
| zip (deflate, today's `hpm pack`) | 242.1 MB | 6.1 s | 3.64 s | reference |
| tar.zst level 19, window 2^27 (**default**) | 116.4 MB (-52 %) | 23.5 s | 3.28 s | identical |
| tar.zst level 19, window 2^30 | 115.4 MB | 23.0 s | 3.15 s | identical |
| tar.zst level 15, window 2^27 | 120.2 MB | 4.7 s | 3.16 s | identical |
| tar.zst level 9, window 2^27 | 122.4 MB | 1.4 s | 3.41 s | identical |

Release build, Windows, NVMe, multi-threaded compression; install time is
hpm's own extractor (two zstd decode passes included). The installed tree
from every archive has the same checksum as the zip's. A bigger window buys
1 MB, so the default stays at 2^27 (readable by any decoder). Even level 9
halves the download; level 19 costs ~20 s of CI time per pack. Windows'
built-in `tar.exe` (bsdtar 3.8) lists and unpacks the archive.

## Format

- A tar (GNU headers, mtime 0, uid/gid 0, mode 0644/0755 as the zip records
  it) compressed as **one zstd frame**: level 19, long-distance matching,
  window 2^27 (128 MiB), frame checksum on.
- The tar holds exactly the zip's entries, in the zip's order (sorted staged
  files under `{slug}/`, then the root `{slug}.json`). Installing either
  archive yields the same tree, so the tree checksum a lockfile records is
  the same for both.
- 2^27 is the largest window every zstd decoder accepts without being told
  to, so `tar --zstd -xf` / 7-Zip / `zstd -d` unpack it as well. hpm's
  decoder accepts up to 2^30 to leave room, and no more, so one archive
  cannot make it allocate more than 1 GiB.
- Reproducible: two packs of one tree are byte-identical, including across
  compression thread counts (tested).
- Each archive is checksummed and signed on its own (`sha256`, `sig`, `kid`):
  it is a different byte stream from the zip.

## Producing it

`hpm pack --solid` (hidden flag while experimental) writes the zip exactly as
before and a `{slug}-{version}[-{platform}].tar.zst` next to it. With
`--json` the payload gains one key, only when `--solid` is given:

```json
{"archive": "...zip", "sha256": "...", "signature": "...", "key_id": "...",
 "platform": "windows-x86_64", "assets": [...],
 "solid": {"format": "tar.zst", "archive": "...tar.zst", "sha256": "...",
           "signature": "...", "key_id": "..."}}
```

## Installing it

The extractor never trusted extensions: it sniffs magic bytes. It now
recognises zstd (`28 B5 2F FD`) next to zip and gzip, and runs the tar path
it already had for `.tar.gz` (same layout resolution, path confinement,
size/entry budget, symlinks skipped, Unix modes applied). Verification is
unchanged: SHA-256 of the archive bytes against the registry `cksum` before
anything is extracted.

## Declaring it in a registry (backward compatible)

An hpm that predates this refuses a zstd archive ("Unrecognized archive
format"), so a solid archive must never be the only archive an old client can
reach. It must also not be a second entry for the same platform: build
selection takes the first entry that matches the host, and an old client
could pick the solid one.

So the solid archive rides **inside** the existing entry, as an optional
alternate:

```json
{"name": "acme/rock-tools", "vers": "1.2.0", "platform": "windows-x86_64",
 "dl": "https://packages.studio.example/rock-tools-1.2.0-windows-x86_64.zip",
 "cksum": "sha256:...", "sig": "...", "kid": "...",
 "alt": [{"fmt": "tar.zst",
          "dl": "https://packages.studio.example/rock-tools-1.2.0-windows-x86_64.tar.zst",
          "cksum": "sha256:...", "sig": "...", "kid": "...", "size": 115000000}]}
```

- Old clients: `RegistryEntry` has no `deny_unknown_fields`, so `alt` is
  ignored and they install the zip, as today. Nothing to negotiate.
- New clients: take the first `alt` whose `fmt` they can extract and verify
  it against *its* `cksum`. A download failure (404, network) falls back to
  the primary `dl`; a checksum mismatch does not (that is an integrity error,
  not a missing file).
- Lockfiles keep recording the primary `dl` (and the tree checksum, which is
  the same for both), so a lock written by a new hpm still installs with an
  old one.
- `size` lets a client or UI show the real download size.
- A Git index carries the same field on its JSON lines.

Once every supported client reads `alt`, a registry may later serve the
solid archive as the primary `dl` (with a zip as an `alt` for third-party
tools), but that is a separate decision.

## Not in the prototype

- Client-side `alt` selection (parsing the field, choosing, fallback) —
  designed above, not written: it is only useful once a registry serves the
  field.
- Single-pass tar extraction. The tar path reads the archive twice (pass 1
  collects names for the layout decision); for zstd that is two decodes.
  Measured cost is in the table; a one-pass extract into a scratch directory
  followed by renames would remove it if it ever matters.

## Benchmark

```sh
HPM_SOLID_BENCH_ZIP=<release.zip> HPM_SOLID_BENCH_DIR=<scratch> \
  cargo test --release -p hpm-core --lib solid_bench -- --ignored --nocapture
```

`HPM_SOLID_BENCH_PARAMS=19:27,19:30` picks the level:window-log variants.
