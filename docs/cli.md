# CLI reference

Every `hpm` command, its flags and what it does. For task-oriented
walkthroughs see the guides; for the manifest those commands read see
[the `hpm.toml` reference](manifest.md).

## Global options

Available on every command, before or after the subcommand name, except
`--output`, which goes before it (`hpm --output json list`): `build` and
`pack` have their own `--output <dir>`.

| Option | Description |
|--------|-------------|
| `-v, --verbose` | Show progress logs on stderr (`-vv` for more). By default only warnings and errors appear there. |
| `-q, --quiet` | Suppress non-error output. |
| `--color <when>` | `auto`, `always`, or `never`. |
| `--output <format>` | `human` (default), `json`, `json-lines`, `json-compact`. The JSON error document follows the same format. |
| `-C, --directory <dir>` | Run as if invoked from `<dir>`: the project is found there, and relative paths in other arguments resolve against it. |

Commands that work on a project (`add`, `remove`, `update`, `list`, `install`,
`check`, `build`, `pack`, `run`, `audit`) also take `-m, --manifest <path>`,
naming the project's `hpm.toml` or the directory holding it.

## `hpm init`

Create a new HPM package.

```
hpm init [OPTIONS] [NAME]
```

**Options**

| Flag | Default | Description |
|------|---------|-------------|
| `--description <text>` | — | Package description. |
| `--author <name>` | `git config user.*` if set | Author (`"Name <email>"`). |
| `--version <v>` | `0.1.0` | Initial version. |
| `--license <id>` | `MIT` | License identifier. |
| `--houdini <range>` | `^21` | `[compat].houdini` Cargo-style range (e.g. `"^21"`, `">=20.5, <22"`, `">=21"`). Default is bounded to a single Houdini major — see [`[compat]`](manifest.md#compat) for why. |
| `--bare` | off | Skip standard directories and generated files; create only `hpm.toml`. |
| `--vcs <vcs>` | `git` | `git` or `none`. |

**Examples**

```sh
hpm init my-tools
hpm init --bare minimal
hpm init advanced-tools \
  --description "Advanced geometry tools" \
  --author "Artist <artist@studio.com>" \
  --license Apache-2.0 \
  --houdini ">=20.5, <22"
```

## `hpm add`

Add one or more dependencies to `hpm.toml`.

```
hpm add [OPTIONS] <PACKAGE>...
```

`<PACKAGE>` is `creator/slug` or `creator/slug@version`. `hpm add` resolves
it straight away: a bare name takes the highest non-yanked version in the
configured registries, `name@version` takes that version, and the exact
version found is written to `hpm.toml`. It then installs, like `hpm install`.

**Options**

| Flag | Description |
|------|-------------|
| `--path <dir>` | Add as a local path dependency (only valid with a single package). Path dependencies install into a `_dev/` subtree of the global packages dir so they never overwrite a registry install at the same `(slug, version)`. |
| `--link` | For path dependencies, install as a symlink (Unix) or NTFS junction (Windows) instead of copying. Working-tree edits become visible to a live Houdini session without re-running `hpm install`. Requires `--path`. Packages that declare native `[compat].platforms` (a DSO/HDK build, not `universal`) are installed as a copy of their install image even with `--link`: a linked native binary can't be rebuilt in place while a Houdini session has it loaded (Windows `LNK1104`), and a mapped DSO needs a relaunch to reload anyway. |
| `-m, --manifest <path>` | Path to the manifest to modify (`hpm.toml` or containing dir). Defaults to cwd. |
| `--optional` | Record `optional = true` on the added dependencies. The flag is metadata: `hpm install` installs optional dependencies like any other, and `hpm list` marks them. |
| `--registry <name>` | Resolve from this configured registry only, and record `registry = "<name>"` in `hpm.toml` so later installs and updates keep using it. Errors if the name is not configured. Not valid with `--path`. |

**Examples**

```sh
hpm add studio/utility-nodes@1.0.0
hpm add studio/a@1.0.0 studio/b@2.0.0
hpm add local-tools --path ../local-tools
hpm add local-tools --path ../local-tools --link  # live edits → Houdini
hpm add studio/visualize@1.0.0 --optional
hpm add studio/lib@1.0.0 -m /path/to/project
hpm add studio/internal@1.0.0 --registry studio   # pin to one registry
```

## `hpm remove`

Remove a dependency from `hpm.toml`. This does **not** delete package files
from `~/.hpm/packages/` — run `hpm clean` for that.

```
hpm remove [OPTIONS] <PACKAGE>
```

| Flag | Description |
|------|-------------|
| `-m, --manifest <path>` | Manifest to modify. |

## `hpm install`

Install the dependencies `hpm.toml` declares, at the versions it names.

```
hpm install [OPTIONS]
```

Install does the following:

1. Loads `hpm.toml` and, if present, `hpm.lock`. An `hpm.lock` that cannot be read stops the install. A lock older than 90 days prints a warning.
2. Installs every entry of `[dependencies]` into the store (`~/.hpm/packages/`), several at once. A registry dependency is looked up at exactly the version written in the manifest; a URL dependency is downloaded; a path dependency is copied or linked. A package already in the store is reused.
3. Checks each package the lock pins against its locked checksum, whether it was just downloaded or reused.
4. Collects `[python_dependencies]` from the installed packages, resolves them with the bundled `uv` for the Python that matches the project's `[compat].houdini`, and installs them into a shared venv under `~/.hpm/venvs/<hash>/`.
5. Writes one Houdini package file per dependency to `<project>/.hpm/packages/{creator}.{slug}.json`, and removes files there for dependencies that are gone.
6. Writes `hpm.lock`.

Two things install does not do:

- **It does not resolve ranges.** `"studio/tools" = "^1.2"` fails with "version not found". Write an exact version, or let `hpm add` and `hpm update` choose one and write it for you.
- **It does not install dependencies of dependencies.** Only the packages listed in the project's own `[dependencies]` are installed. If a package you use needs another package, add that one too. (TumbleTrove Desktop adds required packages for you when it writes a project's manifest; the command line does not.)

The venv is built from the Python dependencies of the *installed packages*.
`[python_dependencies]` in the project's own manifest are used by
[`package-env` scripts](manifest.md#running-inside-the-package-environment),
not by the venv Houdini loads.

**Options**

| Flag | Description |
|------|-------------|
| `-m, --manifest <path>` | Path to `hpm.toml` (or its containing directory). |
| `--frozen-lockfile` | Fail if `hpm.lock` is missing or would need to change. Use in CI. |


## `hpm update`

Move registry dependencies to newer versions.

```
hpm update [OPTIONS] [PACKAGES]...
```

With no packages, checks every registry dependency. With specific packages,
only those.

For each dependency, `hpm update` asks the registry for all versions and
picks the highest non-yanked one in the dependency's *update range*:

- An exact pin such as `1.2.3` is searched as `^1.2.3`, so it moves to the newest release with the same major version (the same minor, for `0.x`).
- A range such as `>=1.2, <1.5` is searched as written.

Applying an update rewrites the entry in `hpm.toml` to the exact version
found, then installs. Path and URL dependencies are left alone, and so are
Python dependencies: `uv` resolves those again on every install.

With a non-human `--output`, pass `--dry-run` or `--yes`; there is nobody to
answer the prompt.

| Flag | Description |
|------|-------------|
| `-m, --manifest <path>` | Manifest to operate on. |
| `--dry-run` | Print the proposed plan without applying it. |
| `-y, --yes` | Skip the confirmation prompt. |

**Examples**

```sh
hpm update --dry-run
hpm update
hpm update studio/geometry-tools
hpm --output json update --yes      # CI-friendly
```

## `hpm list`

Display installed dependencies and their metadata.

```
hpm list [OPTIONS]
```

| Flag | Description |
|------|-------------|
| `-m, --manifest <path>` | Manifest to read. |
| `--tree` | Render dependencies as a tree. |

## `hpm check`

Validate `hpm.toml` and the surrounding project.

```
hpm check
```

Check runs:

- `hpm.toml` exists, parses, and passes manifest validation (scoped `creator/slug` path, semver version, `[compat].houdini` parseable, `[stage]` and `[[operators]]` per-platform consistency with `[compat].platforms`).
- Soft warnings for: missing description, missing authors, missing keywords, missing `[compat].houdini`, missing README or license file, missing `.gitignore` when a `.git` directory is present, packages larger than 100 MB, and individual files larger than 10 MB. Size is measured over what `hpm pack` would ship: files excluded by `.gitignore`, `.hpmignore` or `[stage]` (a `build/` tree, say) do not count.

Check is advisory — warnings do not fail the command.


## `hpm run`

Execute a script defined in the manifest's [`[scripts]`](manifest.md#scripts) table.

```
hpm run <SCRIPT> [-- ARGS...]
```

| Argument | Description |
|----------|-------------|
| `<SCRIPT>` | Name of the entry under `[scripts]`. |
| `ARGS...` | Trailing arguments forwarded to the script verbatim, after shell-quoting. |

Behaviour:

- Looks up the named entry; if its `cmd` is a conditional list, the first variant whose `when.os` matches the host wins. Plain entries always match.
- Sets `HPM_PACKAGE_ROOT` to the manifest directory and runs the command from that directory through the host shell (`sh -c` on Unix, `cmd /C` on Windows).
- Because it runs through a shell, `cmd` is shell syntax, and the two hosts do not share one: `sh` expands `$VAR`, while `cmd.exe` means nothing by it and spells the same thing `%VAR%`. Give each host the spelling its shell reads with a conditional `cmd` keyed on `when.os` rather than expecting one string to work on both.
- Forwarded `ARGS` are quoted per argument for that shell, so spaces and backslashes reach the script as written, and on Unix quotes do too. On Windows an argument holding a double quote or a line break is refused, because `cmd.exe` ends its quoting at either. One more Windows case has no escape and is not attempted: `cmd.exe` expands `%NAME%` even inside double quotes, and its command-line parser — unlike a batch file's — offers no way to suppress it, so an argument containing `%NAME%` where `NAME` is set in the environment arrives expanded. An unmatched `%`, or a name that is not set, passes through as written.
- For [table-form entries](manifest.md#per-script-python-environments) with `python` or `requirements`, materializes a uv-managed venv at `~/.hpm/venvs/<hash>/`, prepends its `bin/` (or `Scripts/` on Windows) to `PATH`, and sets `VIRTUAL_ENV`. Two scripts whose `python` + `requirements` resolve to the same closure share one venv on disk.
- For entries with [`package-env = true`](manifest.md#running-inside-the-package-environment), runs inside the package's full resolved environment instead: the merged venv across the project and its installed dependencies, with every package's `python/` on `PYTHONPATH`. Requires `hpm install` to have been run.
- The script's exit code becomes `hpm`'s exit code, so `hpm run` is safe to chain in CI or wrap in a Houdini hook.

**Example**

```toml
[scripts.tt_setup]
cmd          = "python scripts/tt_setup.py"
python       = "3.11"
requirements = ["PySide6>=6.6"]
```

```sh
hpm run tt_setup
```

## `hpm search`

Search every configured registry for packages matching a query.

```
hpm search <QUERY>
```

If no registries are configured, HPM prints instructions to add one.

## `hpm build`

Materialise the install image into a directory. Runs `[stage].prepack`
scripts (compile DSO, collapse expanded HDAs, etc.), then copies workspace
files into the output directory using the same include/exclude/place
rules `hpm pack` would apply. The result is what a registry consumer's
install would look like.

```
hpm build [OPTIONS]
```

`hpm build` is a one-shot CLI verb — it copies files and exits, with no
background watcher. The output directory is yours to manage; common
patterns:

- **Single workstation, one Houdini at a time**: leave the default
  `[stage].output_dir` (`dist/`), point Houdini at it, rerun `hpm build`
  whenever you want a refresh.
- **Multiple Houdini sessions in parallel**: pass `--output <tmpdir>` per
  session and have each session's `HOUDINI_PACKAGE_PATH` reference its
  own staging directory. Avoids cross-session DSO lock conflicts.

**Options**

| Flag | Description |
|------|-------------|
| `-m, --manifest <path>` | Path to `hpm.toml` or containing dir. Defaults to cwd. |
| `-o, --output <dir>` | Override `[stage].output_dir`. Relative paths resolve against the manifest dir; absolute paths are used verbatim. |
| `--platform <id>` | Target platform. Defaults to host when `[compat].platforms` is declared. Required when host is not in the declared list. |
| `--profile <name>` | Build profile. Defaults to `release`. Selects the matching `[stage.profile.<name>]` table (if any) and is exposed to prepack scripts as `HPM_BUILD_PROFILE`. |
| `--houdini-majors <list>` | Houdini major versions this build targets, space-separated (e.g. `--houdini-majors "21 22"`). Forwarded verbatim to prepack scripts as `HPM_HOUDINI_MAJORS`. Omit to leave it unset. |
| `--no-prepack` | Skip `[stage].prepack` scripts. Use in CI when build steps already ran out-of-band. |
| `--no-clean` | Keep existing output-dir contents instead of wiping first. |

**Prepack environment.** In addition to `HPM_PACKAGE_ROOT`, prepack scripts
(and any `[scripts]` they invoke) see these build-context variables:

- `HPM_BUILD_PROFILE` — the selected `--profile` (default `release`). A single
  prepack script can branch on it, e.g. `cmake --build --config $HPM_BUILD_PROFILE`,
  instead of maintaining one script per build type.
- `HPM_PLATFORM` — the resolved target platform (e.g. `macos-aarch64`), set
  whenever the package declares `[compat].platforms`.
- `HPM_HOUDINI_MAJORS` — the set of Houdini major versions this build should
  target, space-separated (e.g. `21 22`, or just `22`). A package that builds
  one native artifact per Houdini major (a compiled USD resolver, say) reads it
  to restrict the matrix — build every declared major when it is unset or empty,
  otherwise only the listed ones. hpm forwards the value verbatim and does not
  interpret it.

  `hpm build` sets it from `--houdini-majors`; CI passes the release matrix
  that way instead of exporting an ad-hoc, unprefixed variable. When the flag
  is omitted, an `HPM_HOUDINI_MAJORS` already present in the environment passes
  through unchanged, so an out-of-process producer (e.g. a desktop launcher
  that discovers the machine's installed majors) can set it and an explicit
  `--houdini-majors` still overrides. Unset in both = the package's full
  declared matrix.

`HPM_BUILD_PROFILE`, `HPM_PLATFORM`, and `HPM_HOUDINI_MAJORS` are set only for
the `hpm build` prepack path; a standalone `hpm run <script>` does not carry
build-profile, platform, or Houdini-major context.

**Workflow notes — live editing and DSO rebuild**

These are *user-level* concerns; HPM doesn't model them in the manifest:

- **HDA editing.** Edits made inside Houdini save back to whatever path
  Houdini loaded the HDA from. If you load from `dist/otls/foo.hda`
  (collapsed by `hpm build`), saves go into the build output and get
  clobbered on the next `hpm build`. If you want round-trip editing,
  point Houdini at an unstaged expanded HDA dir during dev, and run
  `hpm build` only when you want to produce the publishable form.
- **DSO rebuild while Houdini is loaded.** On Windows, a loaded `.dll`
  is locked. With `--output <tmpdir-A>` for session A and `--output
  <tmpdir-B>` for session B, your `cmake --build` (writing to
  `build/<plat>/`) doesn't hit either lock, and a fresh `hpm build
  --output <tmpdir-C>` writes to a third location — you only hit the
  lock when you try to rebuild *into* a directory a live Houdini still
  has loaded. The typical workflow is one temp dir per Houdini
  lifetime, thrown away on Houdini close.

## `hpm pack`

Build a distributable archive from the current package.

```
hpm pack [OPTIONS]
```

Pack runs `hpm check` first, then:

1. Auto-generates a Houdini-native `{slug}.json` for the archive unless the user has provided one (a hand-written file ships verbatim). This file follows Houdini's own package format, so the archive is usable by Houdini even without HPM: the archive uses the **hpackage layout** — `{slug}.json` at the root next to a `{slug}/` folder holding all package content. See [Installing an archive without HPM](houdini-integration.md#installing-an-archive-without-hpm) for how to install one, and which Houdini install flow to avoid. (`hpm install` strips the `{slug}/` wrapper again; installed trees stay flat with `hpm.toml` at the root.) The archive is named `{slug}-{version}.zip`, or `{slug}-{version}-{platform}.zip` for a per-platform build; read the path from `--json` rather than building it.
2. Filters files by `[stage]` (per-platform `place` rules and `include`/`exclude` globs) when the manifest declares `[compat].platforms`.
3. Produces a `.zip` archive plus a SHA-256 checksum. Each entry carries the
   executable bit of the file it came from, normalised to `0755` or `0644` —
   so a package that ships a program (a `[scripts]` entry pointing at
   `bin/<platform>/<tool>`, a bundled `configure.sh`) installs runnable on
   macOS and Linux. Only the executable bit survives; the group and other
   bits are normalised away so that two machines with different umasks pack
   an identical tree to identical bytes, and therefore to the same checksum.

   Windows has no file mode to read, so an archive packed there cannot carry
   an executable bit. **Pack an archive containing a Unix program from a Unix
   host** — per-platform CI that builds each archive on its own worker
   already does. (`hpm install` compensates where it can: see
   [file modes on install](design-notes.md#file-modes-on-install).)

   **Symlinks are resolved, not stored.** A symlink to a file is packed as an
   ordinary entry holding the target's bytes, so a source tree using the usual
   `libfoo.so -> libfoo.so.1.2.3` layout ships both names and installs
   correctly everywhere, including on Windows. A symlink to a *directory* is
   skipped with a warning — the pack walk does not descend through links, so
   its contents would go missing silently — and a broken link is skipped
   rather than failing the pack. The result is that hpm archives never contain
   a link entry, which is what lets extraction refuse them outright.
4. If a signing key is supplied, produces an Ed25519 signature over the archive bytes and emits a `keyId`.
5. Builds a searchable **asset index** from the manifest's [`[[operators]]`](manifest.md#operators) declarations and includes it in `--json` output (and warns if a declared `source` file is missing from the archive).


**Options**

| Flag | Description |
|------|-------------|
| `-m, --manifest <path>` | Path to `hpm.toml` or containing dir. Defaults to cwd. |
| `--key <path>` | Ed25519 PKCS#8 PEM private key. Overrides `HPM_SIGNING_KEY`. |
| `--output <dir>` | Output directory. Defaults to the current directory. |
| `--json` | Emit the result as JSON (useful in CI). |
| `--platform <id>` | Override host-platform detection. Valid: `linux-x86_64`, `linux-aarch64`, `macos-x86_64`, `macos-aarch64`, `windows-x86_64`, `windows-aarch64`, `universal`. Only legal when `[compat].platforms` is declared. |
| `--verify-assets` | Fail the pack (and delete the archive) if any `[[operators]]` `source` is missing from the produced archive, instead of just warning. |
| `--sidefx` | Shape the bundled `{slug}.json` for SideFX's hpackage repository, so the archive can be uploaded exactly as packed. Fails the pack if the manifest declares something hpackage cannot represent, if the descriptor that actually ships breaks its rules, or if `README.md` is missing. |

**Packing for SideFX's hpackage repository**

SideFX validates the root `{slug}.json` when an archive is uploaded to its
hpackage repository, and it is stricter than Houdini itself. A publisher that
rewrites the json to satisfy it has to re-zip, which invalidates the checksum
and signature `hpm pack` just reported. `--sidefx` emits the accepted form
directly, so the packed bytes are the uploaded bytes.

It changes two things, both confined to that json:

- `hpackage.version` drops trailing zero segments while more than two remain,
  so `0.1.0` becomes `0.1`. That is also the version SideFX puts in the served
  archive's URL.
- Every `>=`/`<=` operand in the `enable` expression becomes `major.minor`.
  A single segment is padded (`21` to `21.0`), and a build segment is kept only
  when it is zero, since `21.0.0` means the same range as `21.0`.

The pack fails rather than emit something whose meaning changed:

- A bound with a real build number, such as `>=20.5.445`, has no hpackage
  form. Truncating it to `>=20.5` would enable the package on builds
  `[compat].houdini` excludes, and the same expression ships to hpm-managed
  installs, so the widening would not stay on the SideFX side. Declare a
  two-segment bound if that is what you mean.
- A missing `[compat].houdini`, or a range compiling only to `<`, `>`, `==`
  or `!=`, leaves SideFX no version to read — it looks only at `>=` and `<=`
  clauses — and it refuses the upload without saying why. Express the lower
  bound with `>=`.
- A **bounded** range compiles to more than one clause, and hpackage matches
  the whole `enable` value against a single one. `>=21, <23` is refused, and
  so is the `^21` shorthand, which expands to a lower *and* an upper bound.

To publish to SideFX, declare a single lower bound such as `>=21.0`. To
keep an upper bound, host the archive somewhere else. Splitting the range
into several `enable` keys is not a workaround; see
[Design notes](design-notes.md#why-a-bounded-range-cannot-be-published-to-sidefx).

`--sidefx` also requires a `README.md` in the package directory, spelled
exactly that way. hpackage's uploader reads it and sends its text as the
published description, and refuses when it is absent or still holds the
placeholder its own scaffold writes. hpm's ordinary README check is looser —
it accepts `README.txt` and a bare `README`, and only warns — so a package can
pass `hpm check` and still be refused on upload.

**The bundled descriptor is matched by exact name**

If the package directory holds a file named exactly `{slug}.json`, `hpm pack`
ships it verbatim instead of generating one. The match is on exact spelling,
compared against the directory's entries rather than asked of the filesystem:
macOS and Windows answer `exists` case-insensitively, so a repo holding
`FLOPs.json` would otherwise ship that file as `flops.json` when packed there
and the generated descriptor when packed on Linux — the same source tree
producing different archives depending on who built it. A file differing only
by case is reported and skipped, and the descriptor is generated.

Under `--sidefx` the descriptor that reaches the archive is checked against
hpackage's rules whichever way it got there. A hand-written file is still
shipped byte for byte — the check only reads it, and never rewrites what an
author wrote — but one declaring a bounded or object `enable`, or a version
hpackage cannot store, fails the pack with the constraint named rather than
being refused at upload time by whatever tool publishes. Without this the
path hpm generates was the checked one and the path an author hand-writes was
not, so `--sidefx` silently did nothing on exactly the packages whose author
cared enough to write the file by hand.

The check reads the fields it constrains and ignores the rest, so a
hand-written descriptor may use Houdini keys hpm does not model. Generated
descriptors go through the same check, which is what keeps the generator and
the check from drifting apart. A `hpackage.version` SideFX will store
differently is reported but allowed: `0.1.0` uploads as `0.1`, and the served
archive's URL uses the trimmed form.

The flag does not affect an `hpm install`: the extractor skips this file and
generates its own package json. It matters to a manual unzip into a Houdini
packages directory, and to SideFX's uploader.

**Asset index in `--json` output**

`--json` adds an `assets` array alongside the existing checksum/signature
fields. Each entry describes one bundled operator; `None` fields are omitted.
HDA-vs-DSO is carried by `kind` (`hda_operator` / `dso_operator`).

```json
{
  "archive": "rbd-tools-1.0.0.zip",
  "sha256": "…",
  "signature": null,
  "key_id": null,
  "platform": null,
  "assets": [
    {
      "kind": "hda_operator",
      "type_name": "studio::rbd_configure::2.0",
      "label": "RBD Configure",
      "category": "Sop",
      "namespace": "studio",
      "op_version": "2.0",
      "tab_submenu": "Studio/Dynamics",
      "icon": "SOP_rbd",
      "source_file": "otls/rbd.hda"
    },
    {
      "kind": "dso_operator",
      "type_name": "studio::fast_scatter",
      "label": "Fast Scatter",
      "category": "Sop",
      "namespace": "studio",
      "source_file": "dso/scatter.so"
    }
  ]
}
```

The index is built from declarations, not by parsing the bundled files —
see [`[[operators]]`](manifest.md#operators) for why and how to declare them.

**Signing key resolution order**

1. `--key <path>` (CLI flag).
2. `HPM_SIGNING_KEY` environment variable. If its value starts with `-----BEGIN`, it's treated as inline PEM; otherwise it's a path.
3. `[signing].key_path` in `~/.hpm/config.toml`.

**Generating a signing key**

```sh
openssl genpkey -algorithm ed25519 -out signing.pem
openssl pkey -in signing.pem -pubout -out signing.pub.pem
```

Keep `signing.pem` secret, and outside the package directory: `hpm pack` and
`hpm build` refuse to package any file that is a PEM private key, and any link
that points outside the package. Publish `signing.pub.pem` so consumers can
verify.
See [Security](security.md#package-signing) for the wire format.

## `hpm audit`

Run a security audit on the current project.

```
hpm audit [OPTIONS]
```

| Flag | Description |
|------|-------------|
| `-m, --manifest <path>` | Manifest to audit. |

Audit checks:

- **HTTP URLs** — flags any `url = ...` dependency whose URL is `http://` rather than `https://`.
- **Lock file presence** — warns if `hpm.lock` is missing.
- **Lock file staleness** — warns if `hpm.lock` is older than 90 days.
- **Checksum verification** — verifies every cached package in `~/.hpm/packages/` matches the checksum stored in `hpm.lock`.
- **Installed files** — compares each locked package's installed files with the record its install wrote (missing, truncated or changed files).

The first three are advisories: they print `WARN` and the audit still
succeeds. Checksum and installed-file failures, and an `hpm.lock` that cannot
be read, are integrity failures: they print `FAIL` and `hpm audit` exits 1.

See [Security](security.md) for more.

## `hpm global`

Install a package into Houdini's user preferences, so it loads in every
session of one Houdini version without a project or launcher wiring.

```
hpm global add    <PACKAGE> --houdini <X.Y> [--registry <name>]
hpm global list             --houdini <X.Y>
hpm global remove <PACKAGE> --houdini <X.Y>
```

`<PACKAGE>` is `creator/slug` or `creator/slug@version`; a bare name resolves
to the highest non-yanked version. Resolution goes through the configured
registries exactly like `hpm add`, including the `--registry` pin.

`--houdini <X.Y>` is required and is not guessed. hpm does not discover
Houdini installations, and the preferences directory is per-version, so a
default would risk writing where your Houdini never looks. A full build
string is accepted (`21.0.729`) and truncated to its `major.minor` line.

The manifest is written as `hpm-<creator>.<slug>.json` into:

| Platform | Directory |
|----------|-----------|
| Linux | `~/houdiniX.Y/packages/` |
| macOS | `~/Library/Preferences/houdini/X.Y/packages/` |
| Windows | `%USERPROFILE%\Documents\houdiniX.Y\packages\` |

`HOUDINI_USER_PREF_DIR` takes precedence when set, with `__HVER__` expanded
to `X.Y`, matching Houdini's own behaviour.

**That directory is not hpm's.** It holds files from SideFX, other tools, and
you. Unlike a project install — which owns `<project>/.hpm/packages/` and
sweeps anything unrecognized out of it — `hpm global` never scans that
directory to decide what to delete. Every removal is driven off the ledger at
`~/.hpm/global/houdini-<X.Y>.json` and touches only files hpm recorded
writing. Files you put there by hand are left alone.

A package whose `[compat].houdini` does not include the target version is
rejected before anything is written. Installing it anyway would emit a
manifest whose `enable` expression is false, and Houdini would ignore it in
silence.

`hpm global remove` deletes the manifest but leaves the package in the store,
matching `hpm remove`. Run `hpm clean` to reclaim the space.

There is no `hpm global update` yet: with no manifest to rewrite, updating is
`hpm global add` at the new version, which replaces the entry in place.

**Examples**

```sh
hpm global add studio/utility-nodes --houdini 21.0
hpm global add studio/utility-nodes@1.2.0 --houdini 21.0 --registry studio
hpm global list --houdini 21.0
hpm global remove studio/utility-nodes --houdini 21.0
```

Only public packages resolve today: the standalone CLI has nowhere to store
credentials, so it always resolves anonymously. A private package fails as
"not found".

## `hpm clean`

Remove orphaned packages, dev (path-dep) installs, and/or venvs that no
active project depends on.

Globally installed packages are roots here too — they belong to no project,
so without the global ledger they would look unreferenced and be deleted out
from under a manifest Houdini still loads. If a ledger cannot be read,
`hpm clean` refuses to run rather than risk that.

```
hpm clean [OPTIONS]
```

| Flag | Description |
|------|-------------|
| `-n, --dry-run` | Print what would be removed, without touching anything. |
| `-y, --yes` | Skip confirmation. |
| `--python-only` | Clean only orphaned venvs. |
| `--comprehensive` | Clean packages, dev installs, **and** venvs. |

HPM identifies active projects via `[projects]` in `~/.hpm/config.toml`
(`explicit_paths` plus recursive `search_roots`; see
[Configuration](configuration.md)). **If that finds no project at all,
`hpm clean` removes nothing**: with no project to mark packages as used,
every package would look orphaned. Three classes of artifact
are considered:

- **Registry/URL packages** in `~/.hpm/packages/<slug>@<version>/`:
  preserved if reachable from any active project's dependency graph.
- **Dev installs** in `~/.hpm/packages/_dev/<slug>@<version>/` (created by
  `{ path = "..." }` deps, copy or link mode): preserved if any active
  project's path-dep source manifest reports that `(slug, version)`.
  Entries are listed as `_dev/<slug>@<version>` so the source of each is
  obvious. Link installs are unlinked safely — never followed. A copy is
  content-addressed as `_dev/<slug>@<version>/<source-hash>/`, so each
  rebuild makes a new directory. `hpm install` removes the copies it
  supersedes once no project's packages point at them; `clean` does the
  same for every package, including copies made by an earlier hpm, and
  keeps the current build. A copy a project still points at, or that a
  process recorded as using it still runs from, is kept. hpm does not know
  about a Houdini you started yourself against a project you have since
  re-installed, so close such sessions before `clean`.
- **Python venvs** under `~/.hpm/venvs/`: preserved if any kept package
  declares matching `[python_dependencies]`. Removed only when
  `--python-only` or `--comprehensive` is set.

A project whose path-dep source can't be read (workspace moved or
deleted) logs a warning and doesn't block cleanup of other dev installs;
re-run `hpm install` after fixing the path to reinstate anything that was
swept.

## `hpm registry`

Manage registries in `~/.hpm/config.toml`. See [Registries](registries.md).

```
hpm registry add <URL> [--name <alias>] [--type api|git] [--if-not-exists]
hpm registry list
hpm registry remove <NAME>
hpm registry update
```

If `--type` is omitted, HPM infers it: URLs ending in `.git` or hosted on
`github.com` / `gitea.*` are treated as `git`; everything else is `api`.

Pass `--if-not-exists` to make `add` idempotent: if a registry with the same
name already exists it is left unchanged and the command exits 0, instead of
erroring. Useful in provisioning scripts.

## `hpm completions`

Emit shell completion scripts — see [Install](install.md#shell-completions).

## Output formats and automation

All commands emit human-readable output by default. The `--output` global
flag selects a machine-readable format instead:

| Format | When to use |
|--------|-------------|
| `human` | Default. Colored, styled for terminal use. |
| `json` | Pretty-printed JSON. |
| `json-lines` | One JSON object per line. Good for streaming and log ingestion. |
| `json-compact` | Single-line JSON. Minimal bandwidth. |

Structured output is supported by `list`, `check`, `update`, `search`, and
`pack`. Commands whose output is inherently interactive (`init`, `add`,
`remove`, `install`, `build`, `audit`, `run`, `clean`, `registry`, `global`,
`completions`)
reject a non-human `--output` with a clear error rather than silently
ignoring it.

Errors in any machine-readable format are also emitted as JSON, with fields
`success`, `error`, `error_type`, and `elapsed_ms`.

See [Continuous integration](guide-ci.md) for a full pipeline.

`hpm --output json update --dry-run` is useful for nightly jobs that want to
detect available updates without applying them.

## Exit codes

| Code | Meaning |
|------|---------|
| 0 | Success. |
| 1 | An hpm failure: bad configuration, a missing or invalid manifest, a failed install, a network or I/O error, an `hpm audit` integrity failure, a registry `hpm registry update` could not refresh. |
| 2 | A usage error: an unknown flag, or a value a flag does not accept (`--vcs`, `--type`, `--platform`, `--houdini`, conflicting flags). |
| _N_ | `hpm run` passes through the exit code of the script it ran. |
