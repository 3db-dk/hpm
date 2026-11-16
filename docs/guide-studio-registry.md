# Run a studio registry

A private registry for in-house packages needs no server software. A Git
repository holds the index, and any `https` file host holds the archives.
Whoever can clone the repository can install from it.

## 1. Create the index repository

```sh
mkdir hpm-index && cd hpm-index
git init
echo '{}' > config.json
git add config.json && git commit -m "Empty index"
```

`config.json` must exist; hpm uses it to recognise an index. Push the
repository to your Git host.

## 2. Pack a package and host the archive

In the package:

```sh
hpm pack --json
```

```json
{"archive":"rock-tools-0.1.0.zip","sha256":"40c67f24...436f","platform":null, ...}
```

Upload the `.zip` to a location served over `https`, for example
`https://packages.studio.example/rock-tools-0.1.0.zip`.

## 3. Add the version to the index

Each package has one file, `<creator>/<slug>.json`, with one JSON object per
line. Append a line for the new version:

```sh
mkdir -p acme
cat >> acme/rock-tools.json <<'EOF'
{"name":"acme/rock-tools","vers":"0.1.0","dl":"https://packages.studio.example/rock-tools-0.1.0.zip","cksum":"sha256:40c67f243a852eff6b08ebf52bd15757115db71e0a509fad67bade62ce3c436f","houdini_compat":"^21","description":"Rock scattering tools"}
EOF
git add acme/rock-tools.json && git commit -m "rock-tools 0.1.0" && git push
```

`cksum` is the `sha256` from `hpm pack --json`, prefixed with `sha256:`.
`houdini_compat` is the package's `[compat].houdini`. For a per-platform
archive add `"platform":"linux-x86_64"`, with one line per platform. All
fields are in [Registry formats](registry-formats.md).

## 4. Add the registry on each workstation

```sh
hpm registry add git@git.studio.example:pipeline/hpm-index.git --name studio --type git
hpm search rock
```

```
Found 1 packages matching 'rock':

  acme/rock-tools 0.1.0
    Rock scattering tools
    Houdini: ^21
```

Or declare it in a project's `hpm.toml`, so that everyone who checks the
project out resolves from it without running `hpm registry add`:

```toml
[[registries]]
name = "studio"
url = "git@git.studio.example:pipeline/hpm-index.git"
type = "git"
```

## 5. Install from it

```sh
hpm add acme/rock-tools --registry studio
```

```toml
[dependencies]
"acme/rock-tools" = { version = "0.1.0", registry = "studio" }
```

`--registry` pins the dependency to your registry. Without the pin, hpm
asks every configured registry in order and takes the first that has the
package; with it, a package of the same name appearing in a public registry
can never be installed instead.

## Publishing a new version

Pack, upload, append a line, push. Then on workstations:

```sh
hpm registry update      # fetch the index; Git registries are read from a local clone
hpm update acme/rock-tools
```

hpm does not fetch the index on its own. A version published after the last
`hpm registry update` is not visible until the next one.

## Withdrawing a version

Add `"yanked":true` to its line and push. `hpm add` and `hpm update` stop
choosing it; projects pinned to exactly that version keep installing.

## Automating it

Steps 2 and 3 are a few lines of shell in a release pipeline: pack with
`--json`, upload the archive, build the index line from the JSON, commit and
push to the index repository. See [Continuous integration](guide-ci.md).

## Private archives

hpm sends no credentials when it downloads an archive, so the host must
serve the file without authentication to the machines that install it: a
server reachable only inside the studio network is the usual answer. Access
to the *index* is controlled by Git.
