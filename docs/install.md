# Install hpm

hpm is a single executable. It needs nothing else installed: it downloads
its own copy of `uv`, and the Python interpreters for its virtual
environments, the first time a package needs them. Git is needed only for
Git registries and for `hpm init` to create a repository.

Houdini 20.5, 21 and 22 are supported.

## Download a release

Download the file for your platform, and the `.sha256` file beside it, from
the [latest release](https://github.com/3db-dk/hpm/releases/latest):

| Platform | File |
|----------|------|
| Linux x86_64 | `hpm-v<version>-linux-x86_64` |
| macOS (Apple silicon and Intel) | `hpm-v<version>-darwin-universal` |
| Windows x86_64 | `hpm-v<version>-windows-x86_64.exe` |

### Linux and macOS

```sh
sha256sum -c hpm-v<version>-linux-x86_64.sha256     # macOS: shasum -a 256 -c
chmod +x hpm-v<version>-linux-x86_64
mv hpm-v<version>-linux-x86_64 ~/.local/bin/hpm     # any directory on your PATH
hpm --version
```

### Windows

```powershell
# The two values must be equal, ignoring case.
(Get-FileHash hpm-v<version>-windows-x86_64.exe).Hash
Get-Content hpm-v<version>-windows-x86_64.exe.sha256

New-Item -ItemType Directory -Force "$env:USERPROFILE\bin"
Move-Item hpm-v<version>-windows-x86_64.exe "$env:USERPROFILE\bin\hpm.exe"
hpm --version      # after adding %USERPROFILE%\bin to your PATH
```

The checksum file comes from the same release as the binary, so it detects
a corrupted or swapped download, not a compromised release.

## Build from source

Requires Rust 1.89 or newer.

```sh
git clone https://github.com/3db-dk/hpm.git
cd hpm
cargo build --release
# binary: target/release/hpm
```

## Upgrade

Replace the executable with a newer release. The store, virtual
environments and configuration under `~/.hpm/` carry over. Read
[Upgrade notes](upgrading.md) first when skipping several versions.

## Uninstall

Delete the executable and `~/.hpm/`. Before that, run `hpm global remove`
for anything you installed with `hpm global add`, because those package
files live in Houdini's preferences directory, not under `~/.hpm/`.

## Shell completions

```sh
# Bash (add to ~/.bashrc)
eval "$(hpm completions bash)"

# Zsh (add to ~/.zshrc)
eval "$(hpm completions zsh)"

# Fish
hpm completions fish | source

# PowerShell (add to $PROFILE)
hpm completions powershell | Out-String | Invoke-Expression
```

Supported shells: `bash`, `zsh`, `fish`, `powershell`, `elvish`.

## Next

[Quickstart: use packages in a Houdini project](quickstart.md).
