# Support several Houdini versions

How much work this is depends on what the package contains.

| The package contains | What to do |
|----------------------|------------|
| HDAs, shelves, pure Python | Widen `[compat].houdini`. One archive serves every version. |
| Paths or settings that differ per version | Conditional `[runtime]` values. |
| Python dependencies with compiled parts (numpy, ...) | Nothing in the package; the project's Houdini selects the build. |
| Compiled HDK plugins | One binary per Houdini major, in version-keyed folders. |

## Declare the range

```toml
[compat]
houdini = ">=21, <23"      # Houdini 21 and 22
```

hpm compiles the range into the package file's `enable` expression, so a
Houdini outside it skips the package. Only declare versions you have
tested. Useful forms:

| Range | Means |
|-------|-------|
| `"^21"` | 21.x |
| `"~21.5"` | 21.5.x |
| `">=20.5, <23"` | 20.5 through 22.x |
| `">=21"` | 21 and everything after. For data-only packages you are willing to vouch for. |

## Values that differ per version

A `[runtime]` value can be a list of variants, each with a condition. The
first variant that matches wins:

```toml
[runtime.ROCK_TOOLS_UI]
method = "set"
value = [
  { when = { houdini = "^21" }, set = "$HPM_PACKAGE_ROOT/ui/h21" },
  { when = { houdini = "^22" }, set = "$HPM_PACKAGE_ROOT/ui/h22" },
]
```

Houdini evaluates the conditions at startup, so one installed package
adapts to whichever Houdini loads it. Conditions can also test `os` and
`python`; see [conditional values](manifest.md#conditional-values).

Houdini's own variables pass through untouched, which is often shorter:

```toml
[runtime]
ROCK_TOOLS_UI = { method = "set", value = "$HPM_PACKAGE_ROOT/ui/h$HOUDINI_MAJOR_RELEASE" }
```

## Python dependencies

Houdini 21 ships Python 3.11 and Houdini 22 ships 3.13. A package does not
choose between them. The **project** does: hpm builds the virtual
environment for the Python that matches the lower bound of the project's
`[compat].houdini`.

For the package author this means: declare Python dependencies with
constraints that have releases for every Python you support, and widen the
package's Houdini range. For the project owner: set the project's
`[compat].houdini` to the Houdini that is actually launched, and use one
project per Houdini major rather than one project launched in both.

## Compiled plugins

A plugin built against one Houdini major does not load in another, so a
package covering two majors carries two binaries. Both cannot be named
`dso/fast_scatter.so`, so place them in version-keyed folders and point
Houdini at the right one:

```toml
[compat]
houdini = ">=21, <23"
platforms = ["linux-x86_64"]

[stage.platform.linux-x86_64]
place = [
  { from = "build/h21/*.so", to = "dso/21/" },
  { from = "build/h22/*.so", to = "dso/22/" },
]

[runtime]
HOUDINI_DSO_PATH = { method = "prepend", value = "$HPM_PACKAGE_ROOT/dso/$HOUDINI_MAJOR_RELEASE" }
```

The cost of this layout: the plugin is found only where the `[runtime]`
variable is applied. That is every hpm install and every manual install
that keeps the shipped package file, but not Houdini's Package Browser
"Install Package Archive", which discards it. If that route matters to your
users, publish one package version per Houdini major instead, each with a
plain `dso/` folder and a `^21` or `^22` range.

### Building only some majors

A build script that loops over Houdini versions can take the list from
hpm:

```sh
hpm build --houdini-majors "21 22"
```

Prepack scripts receive it as `HPM_HOUDINI_MAJORS`. hpm passes the value
along and does not interpret it. Treat an empty or unset value as "build
every major the package declares", so that a plain `hpm build` still does
the full job, and CI can narrow it to the majors installed on a runner.

## Declaring operators per platform

`[[operators]]` `source` is checked against one path per platform. With
version-keyed folders, name one of the binaries, or omit `source` and index
the operator by name only.
