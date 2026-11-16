# Set a project's environment

Packages bring their own environment variables. A project often needs to
add to them or overrule them: the show's OCIO config, an asset root, an
extra Python path. The project's own `[runtime]` section does that, and it
is applied after every package.

## Set a variable for the project

In the **project's** `hpm.toml`:

```toml
[runtime]
SHOW_ROOT = { method = "set", value = "/mnt/shows/rocks" }
OCIO      = { method = "set", value = "/mnt/shows/rocks/color/config.ocio" }
```

```sh
hpm install
```

After a restart, Houdini has both. No package needs to mention the
variable. `set` on `OCIO` replaces the config Houdini ships with, rather
than being added to it.

## Add to what packages provide

```toml
[runtime]
PYTHONPATH         = { method = "append",  value = "/mnt/shows/rocks/python" }
HOUDINI_OTLSCAN_PATH = { method = "prepend", value = "/mnt/shows/rocks/otls" }
```

With `prepend` and `append`, every package keeps its own contribution and
the project's value is merged in once, after all of them. Houdini supplies
the path separator.

## Overrule a package

If a package sets a variable and the project wants a different value, use
`set` in the project:

| Project's `method` | Result |
|--------------------|--------|
| `set` | Only the project's value remains. |
| `prepend`, `append` | Package values stay; the project's is added. |

## Variables a package requires

A package can demand a value without choosing one:

```toml
# in the package
[runtime]
ROCK_TOOLS_LIBRARY = { method = "set", required = true }
```

A project that uses the package must then supply it, or `hpm install`
fails:

```toml
# in the project
[runtime]
ROCK_TOOLS_LIBRARY = { method = "set", value = "/mnt/shows/rocks/library" }
```

If the package gives `required = true` together with a `value`, that value
is the default and the project may leave it out.

## Different values per OS

```toml
[runtime.SHOW_ROOT]
method = "set"
value = [
  { when = { os = "windows" }, set = "S:/shows/rocks" },
  { when = {},                 set = "/mnt/shows/rocks" },
]
```

Houdini picks the variant at startup, so one committed `hpm.toml` serves
every workstation.

## Things to know

- `$HPM_PACKAGE_ROOT` means "this package's directory" and has no meaning in
  a project's `[runtime]`. Use real paths there.
- Other `$VARIABLES` are expanded by Houdini at startup, so
  `"$HIP/cache"` or `"$HOME/presets"` work.
- The project's entries are written to
  `.hpm/packages/~hpm-project-overrides.json`. The name sorts after every
  package's file, which is what makes Houdini apply it last.
- Global installs (`hpm global add`) have no project and therefore no
  project overrides.

Reference: [`[runtime]`](manifest.md#runtime).
