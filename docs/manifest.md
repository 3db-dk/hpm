# The `hpm.toml` manifest

`hpm.toml` describes one directory to hpm. The same file plays two roles:
it describes a **package** (what `hpm pack` ships, and what other projects
depend on), and it describes a **project** (the dependencies `hpm install`
makes available to Houdini). See [Concepts](concepts.md) for the difference.

Unknown keys are errors that name the key, in every section, so a typo such
as `optinal = true` is reported instead of ignored.

A minimal manifest:

```toml
[package]
path = "my-studio/my-tools"
name = "My Tools"
version = "1.0.0"
```

All sections, in the order they appear in practice:

## `[package]`

| Field | Required | Description |
|-------|----------|-------------|
| `path` | yes | Scoped identifier, `creator/slug`. Both segments must be kebab-case (lowercase letters, digits, hyphens). Example: `tumblehead/tumble-rig`. `hpm init` defaults the creator to `local`, producing `local/<name>` — change it before publishing. |
| `name` | yes | Freeform display name. |
| `version` | yes | Semantic version per [semver.org](https://semver.org/). `major.minor.patch` is required; pre-release identifiers (`1.0.0-alpha.1`, `1.0.0-rc.2`) and build metadata (`1.0.0+build.5`) are accepted. |
| `description` | no | Short description. |
| `authors` | no | List of `"Name <email>"` strings. |
| `license` | no | License identifier (e.g. `MIT`, `Apache-2.0`). |
| `readme` | no | Path to README, relative to the package. Defaults to `README.md` for `init`-generated packages. |
| `homepage` | no | Project homepage URL. |
| `repository` | no | Repository URL. |
| `documentation` | no | Documentation URL. |
| `keywords` | no | List of keywords for discovery. |
| `categories` | no | List of categories. |

## `[compat]`

Target-environment compatibility for the package. Two axes:

- `houdini` — a Cargo-style version requirement string.
- `platforms` — the native platforms this package supports. Omit (or
  use `["universal"]`) for pure-data / pure-Python packages; list the
  platforms the package ships binaries for otherwise.

```toml
[compat]
houdini = "^21"                                # default — Houdini 21.x only
# houdini = ">=20.5, <22"                       # explicit range
# houdini = "~21.5"                             # tilde: >=21.5, <21.6
# houdini = "21"                                # bare = caret = ^21
# houdini = ">=20.5"                            # unbounded above — only safe for pure-data
platforms = ["linux-x86_64", "macos-aarch64"]   # omit for pure-data
```

The supported operators are `=`, `>=`, `>`, `<=`, `<`, `^`, `~`, and the
bare-version shorthand (aliases caret). Multiple comparators combine with
`and` when separated by commas. The same grammar is reused inside
`[runtime]` conditional values (`when = { houdini = "^21" }`).

The lower bound of `[compat].houdini` on the **root** manifest drives the
bundled Python version. A dependency package's range is a compatibility
floor only and does not influence the venv ABI. See [Python guide](python-guide.md).

### Why the default is bounded above

Houdini's binary compatibility doesn't survive a major-version bump.
A DSO compiled against the Houdini 21 SDK won't load in Houdini 22;
some Python module signatures shift between majors too. The init
template defaults to `houdini = "^21"` (Houdini 21.x only) for that
reason — authors who ship binaries get a safe starting point, and
authors of pure-data / pure-Python packages can widen the range
explicitly after testing on the next major.

`hpm check` warns when `[compat].platforms` is non-empty but
`[compat].houdini` is unbounded above (e.g. `">=21"`). That catches
the failure mode where a native-binary package installs cleanly on a
newer Houdini and then crashes at load.

## `[dependencies]`

HPM package dependencies. Keys are scoped `creator/slug` paths. Values take
one of four shapes:

```toml
[dependencies]

# 1. Registry, version-only (shorthand)
"studio/utility-nodes" = "1.0.0"

# 2. Registry with options
"studio/material-library" = { version = "2.0.0", optional = true }
"studio/internal-tool" = { version = "1.0.0", registry = "studio" }

# 3. Direct URL (pre-built archive — ZIP or gzipped tar both accepted)
"studio/prebuilt" = { url = "https://pkg.example.com/prebuilt-1.0.0.zip", version = "1.0.0" }

# 4. Local path (development). Installs a copy of the package's install
#    image: the files `hpm pack` would ship (see [stage]), not the whole
#    workspace, so .git/, build trees and virtualenvs are never copied.
"local/local-tools" = { path = "../local-tools" }
"local/optional-tools" = { path = "../optional-tools", optional = true }

# 5. Local path, installed as a symlink/junction (live edits)
#    Working-tree edits become visible to a live Houdini session without
#    re-running `hpm install`. Otherwise identical to (4): same _dev/ namespace
#    isolation, no effect on registry installs at the same coordinate.
"local/linked-tools" = { path = "../linked-tools", link = true }
```

A `registry = "<name>"` key pins the dependency to one configured registry.
The pin is authoritative: resolution, install, and `hpm update` all consult
only that registry, and pinning a registry that is not configured is an error
rather than a silent fallback to the rest of the set. Without the key, a
dependency resolves across every configured registry in order, first match
wins. Add a pin from the command line with `hpm add <package> --registry <name>`.

`optional = true` is a label. `hpm install` installs an optional dependency
like any other and `hpm list` marks it; a client that composes projects may
use it to offer the dependency as a choice.

A URL dependency's archive must contain an `hpm.toml` whose `path` and
`version` match the dependency's key and `version`, and `version` must be a
semantic version. An archive that declares another identity is refused.

[`hpm.lock`](lockfile.md) records the installed version and a SHA-256
checksum for each dependency, so later installs reproduce the same bytes.

### Versions and ranges

A full version (`"1.0.0"`) means exactly that version, here as everywhere in
the manifest. Anything else is a range in Cargo's syntax: `"^1.0"`,
`"~1.2"`, `">=1.0, <2"`.

Which of the two to write depends on who reads the manifest:

- **In a project you install with `hpm install`, write exact versions.**
  Install looks each dependency up at the version written and does not
  resolve ranges. `hpm add` and `hpm update` choose a version for you and
  write it back as an exact one.
- **In a package you publish, write the range you have tested against.**
  When a version is published, its registry dependencies (not `path` or
  `url` ones) are registered with it, and the registry serves them as that
  version's `deps`. A client that chooses versions for a whole project
  (TumbleTrove Desktop does, at every launch) gives each unpinned package
  the newest version whose `[compat].houdini` supports the project's Houdini
  and whose ranges agree with the versions chosen for the other packages,
  and adds a required package the project does not list. Give an upper
  bound when a dependency's next release may break you:

  ```toml
  [dependencies]
  "tumbletrove/tumbletrove" = ">=0.47, <0.48"
  ```

`hpm install` itself installs only the packages a project lists; it does not
follow a package's own `[dependencies]`.

## `[python_dependencies]`

Python packages the package needs inside Houdini, installed through the
bundled `uv` into a shared venv:

```toml
[python_dependencies]

# Version constraint shorthand
numpy = ">=1.26,<3"

# Detailed form
requests = { version = ">=2.25.0", extras = ["security", "socks"] }
matplotlib = { version = ">=3.5", optional = true }
```

A table gives `version`, `extras`, or both.

**Versions are PEP 440 specifiers**, the syntax pip and uv use, and they are
passed to `uv` as written. This is not the syntax of `[dependencies]`:

| Write | Not | Because |
|-------|-----|---------|
| `"==1.26.4"` | `"1.26.4"` | A bare version has no operator. The requirement becomes `numpy1.26.4`, which `uv` reads as a package name. |
| `"~=1.26"` or `">=1.26,<2"` | `"^1.26"` | `^` is not a PEP 440 operator; `uv` rejects it. |

`"*"` means any version.

Each entry must be one plain requirement: the name and extras are PEP 508
names, and the version holds only version-specifier characters, with a `-`
only after a letter or digit (`1.0-1`). Direct URLs (`name @ url`),
environment markers and pip options are refused when the manifest loads.

`optional = true` leaves the dependency out: hpm does not install it, and
there is no switch that turns it on. Use it to record a dependency users may
install themselves.

When two installed packages declare the same Python package, their version
constraints must be **spelled identically**. `>=1.20` in one and `>=1.21` in
the other is a conflict, even though both could be satisfied.

`hpm.lock` does not pin Python packages; `uv` resolves the constraints again
on each install. Pin with `==` where you need the same version every time.
See [Python environments](python-guide.md) for the Houdini-to-Python mapping
and how venvs are shared.

## `[runtime]`

Environment variables to set when Houdini loads the package. The key is
the variable name; the value is a `{ method, value }` pair:

```toml
[runtime]
MY_PLUGIN_ROOT = { method = "set", value = "$HPM_PACKAGE_ROOT/config" }
HOUDINI_TOOLBAR_PATH = { method = "prepend", value = "$HPM_PACKAGE_ROOT/toolbar" }
HOUDINI_AUDIT_LOG = { method = "append", value = "$HPM_PACKAGE_ROOT/logs/audit" }
```

| Method | Effect |
|--------|--------|
| `set` | Replace the variable. |
| `prepend` | Prepend to the existing variable (Houdini picks the platform separator). |
| `append` | Append to the existing variable. |

Use `$HPM_PACKAGE_ROOT` to refer to the installed package directory. HPM
merges these entries with its built-in `PYTHONPATH` and `HOUDINI_SCRIPT_PATH`
entries when generating the Houdini package file. The exact JSON each method
becomes, and why, is in
[How Houdini loads hpm packages](houdini-integration.md#how-runtime-methods-are-emitted).

### Required variables

A package can declare an env var as required without giving it a value.
Any project that depends on the package must then supply the value in its
own `[runtime]` section in `hpm.toml`. `hpm install` (and project sync)
errors out otherwise — the package isn't launchable without it.

```toml
# In the package's hpm.toml
[runtime]
PROJECT_ASSETS = { method = "set", required = true }
```

```toml
# In the consuming project's hpm.toml
[runtime]
PROJECT_ASSETS = { method = "set", value = "/mnt/studio/assets" }
```

`required = true` may be combined with a `value`; the value then acts as
a default and the project override becomes optional. Without a value, the
entry is a hard placeholder.

### Project overrides

A consuming project can override any package-declared variable by
re-declaring the same key in its own `[runtime]`. How the project entry
combines with the package's depends on the *project* entry's `method`:

| Project `method` | Result |
|------------------|--------|
| `set` | Replaces every package contribution wholesale — only the project's value survives. |
| `prepend` / `append` | Extends them — each declaring package's own entry stays in place, and the project's value merges in once, after all of them. |

So a project `append`/`prepend` adds to a package-provided value (e.g.
extending a package's `PYTHONPATH`) rather than clobbering it. Use `set`
when you genuinely want to replace what the packages contribute.

A project `[runtime]` entry takes effect even when no package declares the
variable. It has no owning package, so `$HPM_PACKAGE_ROOT` is undefined in
it. [How Houdini loads hpm packages](houdini-integration.md#project-overrides)
describes the file these entries are written to.


### Conditional values

`value` accepts either a flat string or an ordered list of `{ when, set }`
variants. The variants are selected against four axes; the first match
wins per the rules below.

```toml
[runtime.PXR_PLUGINPATH_NAME]
method = "prepend"
value = [
  { when = { houdini = "^21" }, set = "$HPM_PACKAGE_ROOT/resolver/houdini21/r" },
  { when = { houdini = "^22" }, set = "$HPM_PACKAGE_ROOT/resolver/houdini22/r" },
]
```

| Field | Form | Evaluated by | Compiles to |
|-------|------|--------------|-------------|
| `houdini` | Cargo-style req: `"^21"`, `"~21.5"`, `">=21, <22.5"`, `"21"` (alias for `^21`) | Houdini at startup | `houdini_version >= 'X' and houdini_version < 'Y'` |
| `os` | `"linux"`, `"macos"`, `"windows"` | Houdini at startup | `houdini_os == '<os>'` |
| `python` | `"3.11"`, `"python3.10"`, etc. | Houdini at startup | `houdini_python == 'python<v>'` |
| `install_source` | `"dev"` (path dependency) or `"registry"` (registry/URL install) | hpm at install time | filtered out before emission |

The first three axes lower into Houdini's `package.json` expression form
per <https://www.sidefx.com/docs/houdini/ref/plugins.html>. `install_source`
is *install-time evaluated by hpm* — variants gated to a non-matching
install source are dropped before the Houdini package.json is written, so
a `"dev"` branch never ships to a registry consumer's manifest and a
`"registry"` branch never fires in the dev's own Houdini.

All present axes combine with `and` within a single `when`. Order matters:
the first matching branch wins. (Houdini itself applies *every* matching
element of a conditional value array, so hpm compiles each emitted branch
to exclude all earlier branches' conditions — first-match is guaranteed by
the emitted expressions, and anything after an always-true branch is
dropped as unreachable.) An empty `when = {}` is the always-true fallback
and should appear last. `$HPM_PACKAGE_ROOT` is substituted in each branch,
just like in flat values; any other `$VAR` (e.g.
`$HOUDINI_MAJOR_RELEASE`) passes through verbatim so Houdini's own
variable expansion handles it.

Malformed selectors fail at manifest validation time, so authors find
them before publish, not at install.

### HDK plugins: where a dev install finds its DSOs

Point `HOUDINI_DSO_PATH` at the directory your `[stage]` rules place the
binaries into, **the same value for a dev install and a registry install**:

```toml
[runtime]
HOUDINI_DSO_PATH = { method = "prepend", value = "$HPM_PACKAGE_ROOT/dso" }

[stage.platform.windows-x86_64]
place = [{ from = "build/Release/*.dll", to = "dso/" }]
```

A dev install is not the workspace. A path dependency is installed as a
**copy of the staged image**: exactly the files `hpm build` would stage for
this machine (`.gitignore` + `.hpmignore`, `[stage]` include/exclude, the
release profile, and the host's `[stage.platform.*]` place rules), in a
content-addressed `_dev/<slug>@<version>/<hash>/` directory. A package that
declares native platforms in `[compat].platforms` is always copied, even with
`link = true`, so a rebuild is never blocked by the DLL a running Houdini has
mapped. A build tree such as `build/Release` is usually gitignored or moved by
a place rule, so **it does not exist in a dev install**: a dev variant pointing
`HOUDINI_DSO_PATH` at it gives Houdini a directory that isn't there, and the
dev launch registers no operators, with no error. Pointing at the placed
`dso/` works for both install kinds, and a rebuild reaches the next dev
install through the place rule.

Keep `install_source` for values that really differ between the developer's
own Houdini and a consumer's, such as a dev-only debug switch:

```toml
[runtime.MYPLUGIN_DEBUG]
method = "set"
value = [
  { when = { install_source = "dev" }, set = "1" },
  { when = {}, set = "0" },
]
```

Only a `link = true` install of a package with no native platforms points
at the workspace itself, so a build-tree path resolves only there. Don't rely
on it for DSOs: a native package should declare its platforms, which makes
its dev install a copy.


## `[stage]`

Defines how the install image is derived from the workspace. `[stage]`
governs both `hpm pack` (which streams an archive directly from the
workspace, applying these rules) and — when present — `hpm build` (which
materialises the same image into `output_dir` on disk so a path-dep
consumer can pick it up live).

```toml
[stage]
# Where `hpm build` materialises the install image. Default: "dist".
output_dir = "dist"

# Scripts (named entries from [scripts]) to run before staging. Fail-fast.
prepack = ["build-dso"]

# Gitignore-style globs applied on top of .gitignore and .hpmignore.
# Empty `include` means "everything not excluded". Always-excluded: .git/, .hpm/.
include = ["python/**", "otls/**", "config/**", "LICENSE", "README.md"]
exclude = ["src/**", "tests/**"]       # exclude wins over place: never exclude what a place rule names

# Per-platform place rules. The platform key must appear in [compat].platforms.
[stage.platform.linux-x86_64]
place = [{ from = "build/linux/*.so", to = "dso/" }]

[stage.platform.macos-aarch64]
place = [{ from = "build/macos/*.dylib", to = "dso/" }]

[stage.platform.windows-x86_64]
place = [{ from = "build/win/*.dll", to = "dso/" }]
```

**Platforms.** Valid platform identifiers (TumbleTrove API build platform
enum verbatim): `linux-x86_64`, `linux-aarch64`, `macos-x86_64`,
`macos-aarch64`, `windows-x86_64`, `windows-aarch64`, `universal`. Use
`universal` for OS-agnostic content (pure-Python / data). Declare the
platforms you ship under `[compat].platforms`; each per-platform
`[stage.platform.<plat>]` table must reference a platform listed there.

**Place rules.** Each rule has a `from` glob (workspace-relative) and a
`to` path (archive-relative). If `to` ends with `/` or `/.` it's a
directory; the file's basename is appended. `"./"`, `"."` and `""` name
the root of the install image, also as a directory. Otherwise `to` is the
literal archive path (use when relocating a single file under a renamed
name). Both `from` and `to` use forward slashes regardless of host OS.
`to` must stay inside the install image: a `to` with a `..` component, a
leading `/`, a drive, a backslash or a colon fails the manifest load.
Two files placed at the same archive path, for example two matches of a
literal `to`, or `build/Release/foo.dll` and `build/Debug/foo.dll` under
one directory, fail `hpm pack`, `hpm build` and dev installs with an error
naming both.

**Where to put native binaries.** Place them in Houdini's convention
folders at the package root — `dso/` for HDK plugins, `apexdso/` for APEX
callbacks — rather than in a version- or platform-keyed tree that a
`[runtime]` variable has to point at. Houdini finds those folders beneath
any `HOUDINI_PATH` entry, and the generated `{slug}.json` puts the package
root there via `hpath`, so the operators register with no environment at
all. This is what SideFX's own packages do: `$HFS/packages/apex.json`
contains nothing but `{"hpath": "$HFS/packages/apex"}`.

The reason is reach. A `[runtime]` variable only arrives if something
actually reads the manifest — an hpm-managed install, or the generated
`{slug}.json` sitting beside the content. Houdini's Package Browser
discards it (see [Installing an archive without HPM](houdini-integration.md#installing-an-archive-without-hpm)),
so a package that needs one to locate its own binaries loads its HDAs and
toolbars there but registers no operators, with no error. Convention
folders work on every install route; `[runtime]` does not.

`place` is what makes this cheap: let the build write wherever suits it and
relocate at pack time, leaving the source tree alone. A dev install is a copy
of that same staged image, so the placed path is the one to point at there too
(see [HDK plugins: where a dev install finds its
DSOs](#hdk-plugins-where-a-dev-install-finds-its-dsos)).

**Per-platform packing semantics.** When packing for `--platform <X>`:

- A path matched by `[stage.platform.X].place[*].from` is included at the
  rewritten `to` path. The target's claim wins over other platforms'.
- A path matched only by `[stage.platform.Y].place[*].from` (some `Y != X`)
  is excluded.
- A path matched by no `place` rule and not covered by `[stage].exclude`
  is included as common content at its workspace-relative path. (If
  `[stage].include` is non-empty, common content is restricted to paths
  matching one of those globs.)

Two rules take precedence over all of the above:

- A path matched by `[stage].exclude` is left out even when a `place` rule
  names it.
- A path ignored by `.gitignore` or `.hpmignore` is left out even when a
  `place` rule names it. Re-include build outputs with a `!` rule in
  `.hpmignore` (below).

This means listing the same `from` glob under every platform is a valid
way to declare "this content ships in every per-platform archive"
(e.g. a shared resolver path).

**Build vs pack.** `hpm pack` reads `[stage]` directly and streams the
archive from the workspace — useful in CI where you build immediately
before packing. `hpm build` runs `prepack` scripts and materialises the
same install image into `output_dir` on disk, so a path-dep consumer
working in another project can pick it up live (with `link = true`,
edits flow through without re-running `hpm install`). A copied path
dependency (`{ path = "..." }`, or a native package with `link = true`)
installs this same image for the host platform, using the base `[stage]`
plus the `release` profile.

**`.hpmignore`.** Ignore rules come from `.gitignore` and then
`.hpmignore`, both in gitignore syntax; `.git/` and `.hpm/` are always
excluded. `.hpmignore` exists mostly to re-include build outputs that
`.gitignore` keeps out of the repository:

```gitignore
# .gitignore ignores resolver/ and *.hda; ship them anyway
!resolver/
!resolver/**
!otls/*.hda
```

One difference from git: the rule nearest to a file decides. A file or
directory matched by a `!` rule is shipped even when an ancestor
directory is ignored, so `!build/Release/*.dll` ships those DLLs under an
ignored `build/`. Git would not re-include them. `[stage].include` cannot
re-include an ignored path; a `!` rule is the only way.

### Build profiles

Declared as `[stage.profile.<name>]` tables.

Build profiles are a second axis orthogonal to platform, for packages whose
native binaries can be built more than one way — typically an optimized
`release` (the default) and a symbol-bearing `debug` variant for attaching a
debugger to an HDK plugin.

`hpm build --profile <name>` always exposes the name to prepack scripts as
`HPM_BUILD_PROFILE` (see *Prepack environment* above). When a matching
`[stage.profile.<name>]` table is declared, its overrides layer onto the base
`[stage]` so the profile build is reproducible from the manifest:

```toml
[stage]
prepack = ["build-dso"]

[stage.profile.debug]
# Replaces the base prepack list when non-empty.
prepack = ["build-dso-debug"]

# Per-platform place rules here are appended to the base platform rules —
# e.g. ship Windows PDBs alongside the DSO only in the debug profile.
[stage.profile.debug.platform.windows-x86_64]
place = [{ from = "build/Debug/*.pdb", to = "dso/windows-x86_64/" }]
```

Merge semantics (base `[stage]` + selected profile):

- `prepack` — the profile's list replaces the base when non-empty; otherwise
  the base `prepack` runs.
- `include` / `exclude` — profile entries are appended to the base globs.
- `platform.<plat>.place` — profile place rules are appended to the matching
  base platform entry (a new platform key is created if absent). Each
  `[stage.profile.<name>.platform.<plat>]` key must appear in
  `[compat].platforms`, same as the base `[stage.platform.<plat>]` tables.

The default profile is `release`; you only need a `[stage.profile.release]`
table if you want to customize the default. A `--profile <name>` that names no
declared table is allowed — the env var is still set — but `hpm build` prints a
warning so a typo is visible. `hpm pack` is unaffected by `--profile`; profile
selection is a build-time concern only.

## `[[registries]]` <a id="registries-array"></a>

Per-project registries. Same shape as the global version in `~/.hpm/config.toml`:

```toml
[[registries]]
name = "tumbletrove"
url = "https://api.tumbletrove.com/v1/registry"
type = "api"

[[registries]]
name = "studio"
url = "https://github.com/studio/hpm-packages.git"
type = "git"
```

Project registries are additive to global registries.

## `[scripts]`

Named scripts for the package. Run them with `hpm run <name> [args...]`,
which sets `HPM_PACKAGE_ROOT` to the manifest directory and forwards
trailing arguments to the script.

```toml
[scripts]
build = "python scripts/build.py"
test = "python -m pytest tests/"
```

### Display metadata (`label` and `description`)

The table form accepts two optional, purely informational fields for tools
that present scripts to people (menus, buttons, tooltips):

```toml
[scripts.launch]
cmd         = "python -m mytool.ui"
label       = "Launch My Tool"
description = "Open the main UI"
```

`label` is a human-readable display name and `description` is a one-line
summary. HPM itself never acts on either — `hpm run` ignores them, and
`hpm check` only echoes them back so you can confirm they parsed. They are
consumer-agnostic metadata: any frontend that surfaces a package's scripts
may use them (falling back to the entry key when `label` is absent), but
nothing in HPM treats a labelled script as special. Both are omitted from
round-tripped manifests when unset.

### Per-host variation

Scripts whose command differs per OS use a conditional `cmd` value — the
same `when`-grammar `[runtime]` uses, restricted to the `os` axis:

```toml
[scripts]
build = "cargo build"                        # runs on any host

[scripts.register]
cmd = [
  { when = { os = "windows" }, set = "\"%HPM_PACKAGE_ROOT%\\plugin\\bin\\tool.exe\" register" },
  { when = { os = "macos"   }, set = "\"$HPM_PACKAGE_ROOT/plugin/bin/tool\" register" },
  { when = { os = "linux"   }, set = "\"$HPM_PACKAGE_ROOT/plugin/bin/tool\" register" },
]
```

The command runs through the host's shell, so each variant uses that
shell's spelling for variables: `$HPM_PACKAGE_ROOT` for `sh`,
`%HPM_PACKAGE_ROOT%` for `cmd.exe`.

`hpm run` picks the first variant whose `when.os` matches the host. Add an
empty `when = {}` branch as a last entry to declare a fallback that
matches any host the explicit branches missed. A script with no matching
variant on the current host is treated as absent — `hpm run` errors with a
message that the script only matches other platforms.

Only the `os` axis is meaningful for scripts: HPM has no Houdini-version
or Python context at `hpm run` time, and `install_source` is irrelevant
because scripts run against the dev's workspace, not an install. Setting
any non-`os` axis in a script `when` is rejected at manifest validation
time.

### Per-script Python environments

A script that needs a pinned Python interpreter or extra packages can opt
into a uv-managed virtual environment by switching from the shorthand
string to the table form:

```toml
[scripts.tt_setup]
cmd          = "python scripts/tt_setup.py"
python       = "3.11"
requirements = ["PySide6>=6.6"]
```

`hpm run tt_setup` then resolves `requirements` through the same uv
pipeline that backs `[python_dependencies]`, materialises a venv at
`~/.hpm/venvs/<hash>/`, prepends its `bin/` (or `Scripts/` on Windows) to
`PATH`, and sets `VIRTUAL_ENV` so `python` in the command resolves to the
pinned interpreter. Two scripts whose `python` + `requirements` resolve
to the same closure share one venv on disk. Plain-string entries keep
their prior behaviour and execute against whatever `python` is on `PATH`.

Conditional cmd + venv hints compose:

```toml
[scripts.regen]
cmd = [
  { when = { os = "windows" }, set = "python scripts\\regen.py" },
  { when = {},                  set = "python scripts/regen.py" },
]
python       = "3.11"
requirements = ["pyyaml"]
```

Both `python` and `requirements` are optional in the table form; omitting
both yields a regular script with no venv overhead.

### Running inside the package environment

A per-script venv is intentionally minimal — it installs only the script's
own `requirements` and does **not** expose the package's
`[python_dependencies]` or make the package's `python/` modules importable.
That's right for self-contained tooling, but the wrong shape for code that
ships *in* the package and needs to import it (a render-farm task, a CLI the
package exposes).

Set `package-env = true` to run the script inside the package's full
resolved environment instead:

```toml
[scripts.farm]
cmd         = "python -m tumblepipe.farm"
package-env = true
```

`hpm run farm` then builds the same environment `hpm install` materialises
for Houdini, but for this out-of-Houdini process:

- a merged uv venv resolved from `[python_dependencies]` across the project
  **and its installed hpm dependencies** (so a dependency's Python packages
  are present too);
- every involved package's `python/` directory on `PYTHONPATH` — the
  running package first, then each dependency, then the venv's
  `site-packages`;
- the interpreter pinned to the project's Houdini-mapped CPython (a
  per-script `python` here is ignored); any `requirements` on the entry are
  layered on top of the package environment.

This is read-only: the dependency set comes from the existing `hpm.lock`
plus the global package store, so it never fetches packages or rewrites
generated Houdini manifests. Because the venv is content-addressable, when
`hpm install` already built it the run reuses it without re-resolving.

Run `hpm install` first so the project is locked and its dependencies are
present; otherwise `hpm run` errors and points you there. Consumers like the
Deadline render-farm plugin can then delegate the whole venv/deps/PYTHONPATH
dance to `hpm run <script>` instead of reproducing it.

The table form also accepts plain inline-table syntax:

```toml
[scripts]
tt_setup = { cmd = "python scripts/tt_setup.py", python = "3.11", requirements = ["PySide6>=6.6"] }
```


## `[[operators]]`

Declares the operators (node types) the package bundles, so `hpm pack` can
emit a searchable **asset index** that lets a registry offer node-level search
(e.g. "find the package that ships an `rbd_configure` SOP") without ever
downloading or opening the archive.

```toml
[[operators]]
kind        = "hda"                          # "hda" or "dso"
type_name   = "studio::rbd_configure::2.0"
label       = "RBD Configure"
category    = "Sop"
tab_submenu = "Studio/Dynamics"
icon        = "SOP_rbd"
source      = "otls/rbd.hda"

[[operators]]
kind      = "dso"
type_name = "studio::fast_scatter"
label     = "Fast Scatter"
category  = "Sop"
source    = "dso/scatter.so"
```

| Field | Required | Description |
|-------|----------|-------------|
| `kind` | yes | `hda` (defined by an HDA/OTL digital asset) or `dso` (registered by a compiled plugin shipped as a DSO, i.e. authored against the HDK). |
| `type_name` | yes | Namespaced operator type name, e.g. `studio::rbd_configure::2.0`. `hpm` derives `namespace` and `op_version` from it where it can. |
| `category` | yes | Operator table / network category: `Sop`, `Object`, `Dop`, `Lop`, `Top`, `Vop`, `Rop`, `Driver`, … |
| `label` | no | TAB-menu display name. |
| `tab_submenu` | no | TAB submenu path, e.g. `Studio/Dynamics`. |
| `icon` | no | Icon identifier, e.g. `SOP_rbd`. |
| `source` | no | Where the operator's file lives **in the produced package** (after `[stage]` placement) — either a single archive-relative path or a per-platform table (see below). When set, `hpm pack` checks it against the produced archive. |

**Per-platform `source` (multi-platform DSOs)**

A single HDA ships at the same path in every archive, so one `source` string
works. But a compiled operator's binary differs per platform
(`.so` / `.dll` / `.dylib`, often under `dso/<platform>/`), so its `source` is a
table keyed by platform — mirroring `[stage.platform.*]`. The keys must appear
in `[compat].platforms`:

```toml
[[operators]]
kind      = "dso"
type_name = "studio::fast_scatter"
label     = "Fast Scatter"
category  = "Sop"
source    = { linux-x86_64 = "dso/linux-x86_64/scatter.so", macos-aarch64 = "dso/macos-aarch64/scatter.dylib", windows-x86_64 = "dso/windows-x86_64/scatter.dll" }
```

Each per-platform `hpm pack` resolves `source` to the entry for the platform it
targets and emits that concrete path. An operator whose table does **not** list
the platform being packed is omitted from that platform's index — its file
isn't in that package. (Omit `source` entirely to index an operator by
name/category with no path and no check.)

**Verifying sources at pack time**

By default `hpm pack` *warns* when a declared `source` is missing from the
produced archive. Pass `--verify-assets` to make it a hard error instead (and
delete the invalid archive) — recommended in CI so a package never publishes an
index advertising a file it doesn't ship. The check runs against the built
archive, so build compiled artifacts before packing.

Why declarations rather than reading the files? The HDA container format is
officially undocumented and can change between Houdini versions, and a
compiled HDK plugin (DSO) does not expose its operator names offline at all —
they are C++ constructor arguments that only materialise inside Houdini. A
declaration in `hpm.toml` is stable, version-proof, and a single source of
truth. The author already knows exactly what they ship.

Platform and Houdini-version filtering come for free from elsewhere in the
manifest: a DSO's architecture is implied by the per-platform archive it ships
in (`[compat].platforms` + `[stage]`), and the supported Houdini range is
`[compat].houdini`. See [`hpm pack`](cli.md#hpm-pack) for the emitted index shape.
