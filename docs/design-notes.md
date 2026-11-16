# Design notes

Why hpm behaves the way it does in a few places where the behaviour is
surprising until you know the reason. Nothing here is needed to use hpm;
the reference pages state the rules, and this page explains them.

## File modes on install

An archive's declared Unix modes are applied as-is, with one repair. Packages
are fetched from wherever their version record points — GitHub Releases, SideFX
hpack, a studio's own bucket — and many zip producers drop Unix modes entirely,
stamping every entry `0644`. Extracting such an archive faithfully would yield a
package whose own declared programs cannot be run, failing at spawn with
`Permission denied (os error 13)` on macOS and Linux while working on Windows,
where executability is not a file mode.

So when an archive grants a file no execute permission but the file's leading
bytes identify it as a program — an ELF or Mach-O object, or a `#!` script —
install restores the executable bit, mirroring the read bits (`0644` becomes
`0755`, `0640` becomes `0750`). The repair is additive: a mode the archive does
declare is applied untouched, and a file that is not a program is never made
executable. hpm already runs `[scripts]` programs out of the installed tree, so
this grants nothing a well-formed archive could not claim for itself. (A
registry archive is checked against the entry's SHA-256 when the registry
supplies one; signatures are not verified yet, and URL dependencies are not
checked. See [Security](security.md).)

The same repair reaches trees that are already installed. A package at a version
you already have is never re-extracted, so a tree installed by an older hpm
would otherwise keep its broken modes forever — upgrading hpm would appear to
change nothing. Instead, whenever hpm reuses an installed tree (`install` or `add` finding the version already present) it sweeps it once with the
rule above and records that in a `<name>@<version>.exec-modes` file *beside* the
package directory. The file lives outside the tree deliberately: a package's
checksum is computed over its contents, so a marker inside would change it.

Two limits worth knowing. The sweep is content-driven, exactly like the
extraction repair, so it fixes any program in the tree — including a helper
binary a script shells out to — but it only ever adds execute where read is
already granted, and it will not guess at a file whose bytes don't identify it.
(A shell script with no `#!` line is not rescued by any of this and cannot be:
the kernel refuses to execute it whatever its mode. Ship a shebang, or declare
an interpreter in the script's `cmd`.) And a tree is swept once: if you
deliberately strip a bit afterwards, hpm will not put it back.

## Symlinks in archives

**Symlink entries are skipped on install.** Neither the zip nor the tar.gz extractor
creates a link, and a skipped entry is logged. A link is the one entry type
that can redirect a *later* entry's write outside the target directory —
validating the link's own target would not prevent that — so refusing them is
what keeps path-traversal checking sufficient. Archives `hpm pack` produces
never contain one (links are resolved at pack time; see [`hpm pack`](cli.md#hpm-pack)); a third-party
archive that does will install without them, so a package must not depend on a
link existing after install.
## Why a bounded range cannot be published to SideFX

[`hpm pack --sidefx`](cli.md#hpm-pack) refuses a package whose
`[compat].houdini` has both a lower and an upper bound. SideFX accepts one `enable` clause, so the natural repair is to split the
range into one object key per clause — which is also what hpackage's own
documentation suggests. Do not. Houdini reads an object `enable` as a
*conditional map*, not a conjunction: every key whose expression matches
contributes its boolean, and when no key matches Houdini logs `Unsupported
value for enable` and leaves the package **enabled**.

Measured with `hconfig` on 21.0.729 and 22.0.368. Against a range the running
Houdini sits above, the object form loads the package and appends its `hpath`
to `HOUDINI_PATH`; against one it sits below, it loads with the warning. A
single-key object behaves the same way, so there is no safe subset. The
equivalent string form was correctly disabled in every case.

So the multi-clause string Houdini honours is exactly the one hpackage cannot
read, and the form hpackage can read is not a gate. To publish to SideFX,
declare a single lower bound such as `>=21.0`. To keep an upper bound, host
the archive somewhere else — the range is unaffected there.
