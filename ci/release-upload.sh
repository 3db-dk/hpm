#!/usr/bin/env bash
#
# Publish a built hpm binary as an asset on the GitHub release for the current tag.
#
#   ci/release-upload.sh <source-binary> <asset-suffix>
#
# The asset is named "hpm-v$VERSION-$SUFFIX", with $VERSION taken from
# CI_COMMIT_TAG, so callers never have to derive the version themselves.
#
# Beside the binary it publishes "<asset>.sha256" ("<hex>  <asset>", the
# format `sha256sum -c` reads), uploaded first.
#
# A published binary is never replaced. A rerun whose binary hashes to the
# published checksum is a no-op (so a pipeline cut short can be rerun); a
# different binary, or a published binary without a checksum file, is an
# error: shipping new bytes under an existing version takes a new tag.
# Every failure here is fatal on purpose.

set -euo pipefail

SOURCE=${1:?usage: release-upload.sh <source-binary> <asset-suffix>}
SUFFIX=${2:?usage: release-upload.sh <source-binary> <asset-suffix>}

: "${GITHUB_TOKEN:?GITHUB_TOKEN is not set}"
: "${CI_COMMIT_TAG:?CI_COMMIT_TAG is not set}"

REPO=${RELEASE_REPO:-3db-dk/hpm}
# The token is sent to these, so they can only be GitHub, or this machine
# (ci/tests runs the script against a fake API on 127.0.0.1).
API=${GITHUB_API_URL:-https://api.github.com}
UPLOADS=${GITHUB_UPLOADS_URL:-https://uploads.github.com}
for base in "$API" "$UPLOADS"; do
  # Whole-string match: a base that merely starts like an allowed one
  # (`http://127.0.0.1:1@elsewhere`) is refused.
  if ! [[ "$base" =~ ^(https://api\.github\.com|https://uploads\.github\.com|http://127\.0\.0\.1:[0-9]+)$ ]]; then
    echo "refusing API base $base: only GitHub or 127.0.0.1" >&2
    exit 1
  fi
done
VERSION=${CI_COMMIT_TAG#v}
ARTIFACT="hpm-v$VERSION-$SUFFIX"

if [ ! -f "$SOURCE" ]; then
  echo "source binary not found: $SOURCE" >&2
  exit 1
fi

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
BODY="$WORK/body.json"
# The token goes to curl through a header file in this private directory
# (mktemp -d is mode 700), never on a command line where other users on the
# worker could read it from the process list. printf is a shell builtin.
AUTH="$WORK/auth-header"
printf 'Authorization: token %s\n' "$GITHUB_TOKEN" > "$AUTH"

# Issues a GitHub API request, prints the HTTP status, leaves the body in $BODY.
# Deliberately omits `-f` so a 4xx reaches the status checks below instead of
# collapsing into a generic curl exit code.
api() {
  local method=$1 url=$2
  shift 2
  curl -sS -X "$method" "$url" \
    -H "@$AUTH" \
    -H "Accept: application/vnd.github+json" \
    -o "$BODY" -w '%{http_code}' "$@"
}

die() {
  echo "$1" >&2
  echo "--- response body ---" >&2
  cat "$BODY" >&2
  exit 1
}

mkdir -p artifacts
cp "$SOURCE" "artifacts/$ARTIFACT"

# --- release notes from CHANGELOG.md -----------------------------------------

NOTES=$(sed -n "/^## \[${VERSION}\]/,/^## \[/{/^## \[/d;p;}" CHANGELOG.md || true)
RELEASE_JSON=$(printf '%s' "$NOTES" | python3 -c '
import json, sys
body = sys.stdin.read().strip()
tag = sys.argv[1]
data = {"tag_name": tag, "name": tag}
if body:
    data["body"] = body
else:
    data["generate_release_notes"] = True
print(json.dumps(data))
' "$CI_COMMIT_TAG")

printf '%s' "$RELEASE_JSON" > "$WORK/release.json"

# --- create the release ------------------------------------------------------
#
# The three platform workflows run concurrently and all try to create it, so
# exactly one gets 201 and the losers get 422. Any other status is a real fault.

code=$(api POST "$API/repos/$REPO/releases" \
  -H "Content-Type: application/json" \
  --data-binary "@$WORK/release.json")
case "$code" in
  201) echo "created release $CI_COMMIT_TAG" ;;
  422) echo "release $CI_COMMIT_TAG already exists" ;;
  *)   die "failed to create release (HTTP $code)" ;;
esac

# --- resolve the release id --------------------------------------------------

code=$(api GET "$API/repos/$REPO/releases/tags/$CI_COMMIT_TAG")
[ "$code" = "200" ] || die "failed to look up release $CI_COMMIT_TAG (HTTP $code)"
RELEASE_ID=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["id"])' "$BODY")

# --- publish the checksum file, then the binary --------------------------

if command -v sha256sum >/dev/null 2>&1; then
  DIGEST=$(sha256sum "artifacts/$ARTIFACT" | cut -d' ' -f1)
else
  DIGEST=$(shasum -a 256 "artifacts/$ARTIFACT" | cut -d' ' -f1)
fi
printf '%s  %s\n' "$DIGEST" "$ARTIFACT" > "artifacts/$ARTIFACT.sha256"

# Prints "<id> <browser_download_url>" for the named asset, or nothing.
find_asset() {
  local name=$1
  code=$(api GET "$API/repos/$REPO/releases/$RELEASE_ID/assets?per_page=100")
  [ "$code" = "200" ] || die "failed to list release assets (HTTP $code)"
  python3 -c '
import json, sys
assets = json.load(open(sys.argv[1]))
a = next((a for a in assets if a["name"] == sys.argv[2]), None)
if a:
    print(a["id"], a["browser_download_url"])
' "$BODY" "$name"
}

# Upload one file under its own name and confirm the size that landed.
upload_asset() {
  local name=$1 file="artifacts/$1"
  code=$(api POST "$UPLOADS/repos/$REPO/releases/$RELEASE_ID/assets?name=$name" \
    -H "Content-Type: application/octet-stream" \
    --data-binary "@$file")
  [ "$code" = "201" ] || die "failed to upload $name (HTTP $code)"
  local expected
  expected=$(wc -c < "$file" | tr -d '[:space:]')
  code=$(api GET "$API/repos/$REPO/releases/$RELEASE_ID/assets?per_page=100")
  [ "$code" = "200" ] || die "failed to verify release assets (HTTP $code)"
  python3 -c '
import json, sys
assets = json.load(open(sys.argv[1]))
name, expected = sys.argv[2], int(sys.argv[3])
asset = next((a for a in assets if a["name"] == name), None)
if asset is None:
    sys.exit("%s is missing from the release after a successful upload" % name)
if asset["state"] != "uploaded":
    sys.exit("%s is in state %r, expected uploaded" % (name, asset["state"]))
if asset["size"] != expected:
    sys.exit("%s is %d bytes on the release, expected %d" % (name, asset["size"], expected))
print("verified %s (%d bytes)" % (name, asset["size"]))
' "$BODY" "$name" "$expected"
}

SUMS_ASSET=$(find_asset "$ARTIFACT.sha256")
BIN_ASSET=$(find_asset "$ARTIFACT")

if [ -n "$SUMS_ASSET" ]; then
  # Assets of a published release are public; read the checksum without
  # sending the token anywhere.
  PUBLISHED=$(curl -fsSL "${SUMS_ASSET#* }" | cut -d' ' -f1)
  [ "$PUBLISHED" = "$DIGEST" ] || die "release $CI_COMMIT_TAG already publishes a different $ARTIFACT (sha256 $PUBLISHED); a published binary is never replaced, tag a new version"
  echo "$ARTIFACT.sha256 already published and matches"
elif [ -n "$BIN_ASSET" ]; then
  die "release $CI_COMMIT_TAG already has $ARTIFACT without a checksum file; a published binary is never replaced, tag a new version"
else
  upload_asset "$ARTIFACT.sha256"
fi

if [ -n "$BIN_ASSET" ]; then
  echo "$ARTIFACT already published and matches its checksum"
else
  upload_asset "$ARTIFACT"
fi
