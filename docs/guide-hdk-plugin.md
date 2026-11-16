# Ship an HDK plugin

A package with compiled code differs from a data package in three ways: it
is built per platform, the build output is not where Houdini should load it
from, and it is tied to a Houdini major version. `hpm.toml` has a section
for each.

The example is a SOP plugin built with CMake into `build/`.

## 1. Declare platforms and Houdini version

```toml
[package]
path = "acme/fast-scatter"
name = "Fast Scatter"
version = "1.0.0"

[compat]
houdini = "^21"
platforms = ["linux-x86_64", "macos-aarch64", "windows-x86_64"]
```

Keep the Houdini range bounded to one major. A plugin compiled against the
Houdini 21 SDK does not load in Houdini 22, and `hpm check` warns when a
package with platforms has no upper bound. For more than one major, see
[Support several Houdini versions](guide-multiple-houdini-versions.md).

## 2. Describe the build as a script

```toml
[scripts]
build-dso = "cmake -S . -B build && cmake --build build --config Release"
```

Run it with `hpm run build-dso`. Where the command differs per OS, give
`cmd` [one variant per OS](manifest.md#per-host-variation). The script
runs in your shell, so it needs the Houdini build environment set up the way
your CMake project expects.

## 3. Place the binaries where Houdini looks

Houdini loads plugins from a `dso/` folder under any `HOUDINI_PATH` entry.
Your build writes somewhere else. `[stage]` moves files as the package is
assembled, without changing your source tree:

```toml
[stage]
prepack = ["build-dso"]                 # scripts to run before staging
exclude = ["src/**", "CMakeLists.txt"]    # sources stay out of the package

[stage.platform.linux-x86_64]
place = [{ from = "build/*.so", to = "dso/" }]

[stage.platform.macos-aarch64]
place = [{ from = "build/*.dylib", to = "dso/" }]

[stage.platform.windows-x86_64]
place = [{ from = "build/Release/*.dll", to = "dso/" }]
```

With the plugin in `dso/` at the package root, it needs no environment
variable at all. That matters: a package that locates its plugin through a
`[runtime]` variable loses it on install routes that ignore the variable,
such as Houdini's own Package Browser.

Two rules decide whether a placed file actually ships, and both are easy to
trip over:

- **`[stage].exclude` beats `place`.** Do not exclude `build/**`; that
  removes the binaries the place rules name.
- **Ignored files are not staged.** Build output is normally in
  `.gitignore`, so re-include just the binaries in an `.hpmignore` beside
  `hpm.toml`:

  ```gitignore
  # .hpmignore
  !build/*.so
  !build/*.dylib
  !build/Release/*.dll
  ```

  Everything else in `build/` stays ignored, so object files and caches do
  not end up in the package.

Keep `dist/` (the output of the next step) in `.gitignore` as well, which
`hpm init` does. Otherwise `hpm pack` ships the previous build's `dist/`
inside the archive.

## 4. Build the install image

```sh
hpm build
```

This runs the `prepack` scripts, then writes the package exactly as it would
be installed into `dist/`: your data folders, plus `dso/` holding this
machine's plugin. Look at it. If `dist/dso/` is empty, the place rule's
`from` did not match.

Point a Houdini at `dist/` to test it directly, or depend on the package by
path from a project (`hpm add acme/fast-scatter --path ../fast-scatter`). A
path dependency on a package with platforms installs this same image as a
copy, so after a rebuild run `hpm install` in the project and restart
Houdini.

## 5. A debug build

Profiles let one manifest describe more than one build:

```toml
[scripts]
build-dso-debug = "cmake -S . -B build && cmake --build build --config Debug"

[stage.profile.debug]
prepack = ["build-dso-debug"]

[stage.profile.debug.platform.windows-x86_64]
place = [{ from = "build/Debug/*.pdb", to = "dso/" }]    # added to the base rules
```

```sh
hpm build --profile debug
```

A profile's `prepack` replaces the base list; its place rules are added to
the base ones. Two files placed at the same path are an error, so have the
debug build write its binaries where the base rules already look, and use
the profile for what is extra, such as the PDB files above (re-included in
`.hpmignore` like the binaries).

A single script can also branch on `$HPM_BUILD_PROFILE`, which `hpm build`
sets for prepack scripts. See
[build profiles](manifest.md#build-profiles).

## 6. Declare the operators

So a registry can answer "which package provides the `fast_scatter` SOP":

```toml
[[operators]]
kind      = "dso"
type_name = "acme::fast_scatter"
label     = "Fast Scatter"
category  = "Sop"
source    = { linux-x86_64 = "dso/fast_scatter.so", macos-aarch64 = "dso/fast_scatter.dylib", windows-x86_64 = "dso/fast_scatter.dll" }
```

hpm cannot read operator names out of a compiled plugin, so they are
declared. `source` lets `hpm pack` check the file really is in the archive.

## 7. Pack, once per platform

Build and pack on each platform you declared:

```sh
hpm run build-dso
hpm pack --verify-assets --json
```

This produces `fast-scatter-1.0.0-linux-x86_64.zip` on Linux, and so on.
Each archive contains the shared files and only that platform's binaries.
`--verify-assets` fails the pack if a declared operator's file is missing.

`hpm pack` does not run `prepack` scripts; build first. Pack archives that
contain Unix executables on a Unix machine, because Windows cannot record
the executable bit.

When all archives are published under the same version, a user's
`hpm install` picks the one for their platform.

## Next

- [Publish a package](guide-publishing.md).
- [Continuous integration](guide-ci.md) for building every platform on a tag.
- The [`[stage]` reference](manifest.md#stage) for every rule.
