# Continuous integration

Two pipelines come up: one for a **project** that consumes packages (a show
setup, a render-farm environment), and one for a **package** that is built
and released. The examples are plain shell; they fit any CI system.

hpm's regular output is for people. In CI, rely on exit codes, and use
`--output json` or `hpm pack --json` where a later step needs a value.
Diagnostics go to stderr, results to stdout.

## Installing hpm in a job

```sh
VERSION=0.43.0
BASE=https://github.com/3db-dk/hpm/releases/download/v$VERSION
curl -fsSLO $BASE/hpm-v$VERSION-linux-x86_64
curl -fsSLO $BASE/hpm-v$VERSION-linux-x86_64.sha256
sha256sum -c hpm-v$VERSION-linux-x86_64.sha256
install -m 0755 hpm-v$VERSION-linux-x86_64 /usr/local/bin/hpm
```

Pin the version. Cache `~/.hpm/` between jobs to avoid downloading packages,
`uv` and Python again.

## A project: reproduce the committed lock

```sh
hpm registry add git@git.studio.example:pipeline/hpm-index.git --name studio --type git --if-not-exists
hpm install --frozen-lockfile
hpm audit
```

- A Git registry is cloned with `git`, so the runner needs read access to
  the index repository: a deploy key for the `git@` form, or an `https://`
  URL the runner can already read.
- `--if-not-exists` makes the registry step safe to repeat. Registries
  declared in the project's own `hpm.toml` need no step at all.
- `--frozen-lockfile` fails if `hpm.lock` is missing, does not match
  `hpm.toml`, or lacks a checksum. Every downloaded package is checked
  against the lock.
- `hpm audit` exits 1 if an installed package no longer matches what was
  recorded.

The same three lines prepare a render node. Then start Houdini with
`HOUDINI_PACKAGE_PATH=<project>/.hpm/packages`.

A nightly job can report available updates without applying them:

```sh
hpm --output json update --dry-run
```

## A package: validate on every push

```sh
hpm check
hpm pack --json --verify-assets --output /tmp/pack-test
```

`hpm check` alone exits 0 on warnings. Packing is the stronger test: it
fails on an invalid manifest and, with `--verify-assets`, on a declared
operator whose file is not in the archive.

## A package: release on a tag

```sh
# 1. The tag must match the manifest.
test "v$(grep -m1 '^version' hpm.toml | cut -d'"' -f2)" = "$CI_TAG"

# 2. Build, if the package has compiled parts.
hpm run build-dso

# 3. Pack, signed. The key comes from a CI secret, inline.
export HPM_SIGNING_KEY="$SIGNING_KEY_PEM"
hpm pack --json --verify-assets --output release/ > pack.json

# 4. Upload the archive to your host.
ARCHIVE=$(python3 -c 'import json;print(json.load(open("pack.json"))["archive"])')
SHA=$(python3 -c 'import json;print(json.load(open("pack.json"))["sha256"])')
your-upload-tool "$ARCHIVE"

# 5. Register the version with your registry, using $SHA and the URL.
```

Notes:

- `HPM_SIGNING_KEY` may hold the PEM text itself, which is how CI secrets
  usually arrive; no key file touches the runner's disk.
- `hpm pack` refuses to pack a private key that has ended up inside the
  package directory.
- For a package with `[compat].platforms`, run steps 2 to 4 once per
  platform, each on a runner of that platform. The archives share a version
  and differ in the `platform` field of `pack.json`. Pack on the platform
  you built on; `--platform` exists for cross-packing prebuilt binaries, and
  an archive with Unix executables must be packed on Unix.
- Step 5 depends on the registry: a commit to the index repository for a
  [Git registry](guide-studio-registry.md), or whatever the operator of an
  API registry provides.

## Exit codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | The command failed |
| 2 | The command line was wrong |
| other | From `hpm run`: the script's own exit code |
