#!/usr/bin/env bash
#
# Build hpm's Linux release binary in an AlmaLinux 8 image, so it needs no
# newer glibc than Houdini 22 (2.28), and refuse it if it does.
#
#   bash ci/build-linux-el8.sh      (from anywhere in the repo; Linux, podman)
#
# Writes target/el8/release/hpm: a target directory of its own, so the host's
# `cargo test --release` never overwrites the binary that is uploaded. The
# image (ci/linux-el8/Containerfile) is built with the worker's rootless
# podman if this version of it is not there yet, tagged by a hash of it and
# rust-toolchain.toml; older versions are removed. $HPM_EL8_CACHE (default
# ~/.cache/hpm-el8) holds the cargo registry and git checkouts between runs.

set -euo pipefail

repo=$(git rev-parse --show-toplevel)
cd "$repo"
# Rootless podman keeps its images under $HOME, and the Woodpecker local
# backend hands each run a fresh one: use the account's own, so the image is
# built once rather than every run.
HOME=$(getent passwd "$(id -u)" | cut -d: -f6)
export HOME
cache=${HPM_EL8_CACHE:-$HOME/.cache/hpm-el8}
mkdir -p "$cache/cargo-home"

channel=$(sed -n 's/^channel = "\(.*\)"$/\1/p' rust-toolchain.toml)
[ -n "$channel" ] || { echo "no channel in rust-toolchain.toml" >&2; exit 1; }
tag=$(cat ci/linux-el8/Containerfile rust-toolchain.toml | sha256sum | cut -c1-12)
image="hpm-linux-el8:$tag"
if ! podman image exists "$image"; then
  ctx=$(mktemp -d)
  cp ci/linux-el8/Containerfile rust-toolchain.toml "$ctx/"
  podman build -t "$image" "$ctx" >&2
  rm -rf "$ctx"
fi
podman images --format '{{.Repository}}:{{.Tag}}' \
  | grep '^localhost/hpm-linux-el8:' | grep -v ":$tag\$" \
  | xargs -r podman rmi >&2 || true

podman run --rm --userns=keep-id \
  -v "$repo:$repo" -w "$repo" \
  -v "$cache:$cache" -e "CARGO_HOME=$cache/cargo-home" \
  -e "RUSTUP_TOOLCHAIN=$channel" -e "CARGO_TARGET_DIR=$repo/target/el8" \
  "$image" cargo build --locked --release -p hpm-cli --bin hpm

bash ci/linux-floors.sh target/el8/release/hpm
