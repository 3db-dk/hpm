#!/usr/bin/env bash
#
# Fail unless the tag being built (CI_COMMIT_TAG, "v<version>") names the
# workspace version in Cargo.toml, so a binary is never published under a
# version it does not report.

set -euo pipefail

: "${CI_COMMIT_TAG:?CI_COMMIT_TAG is not set}"
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
if [ "v$VERSION" != "$CI_COMMIT_TAG" ]; then
  echo "tag $CI_COMMIT_TAG does not match the workspace version $VERSION" >&2
  exit 1
fi
echo "tag $CI_COMMIT_TAG matches the workspace version"
