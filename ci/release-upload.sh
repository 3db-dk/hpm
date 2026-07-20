#!/usr/bin/env bash
#
# Publish a built hpm binary as an asset on the GitHub release for the current tag.
#
#   ci/release-upload.sh <source-binary> <asset-suffix>
#
# The asset is named "hpm-v$VERSION-$SUFFIX", with $VERSION taken from
# CI_COMMIT_TAG, so callers never have to derive the version themselves.
#
# Reruns of a tag pipeline *replace* an existing asset of the same name. The
# previous implementation appended `|| true` to the upload so a rerun would
# survive the 422 GitHub returns for a duplicate name, but that is not
# idempotence: GitHub rejects the upload and keeps the old binary, so a rerun
# after a code change republished a stale artifact while reporting success.
# Every failure here is fatal on purpose.

set -euo pipefail

SOURCE=${1:?usage: release-upload.sh <source-binary> <asset-suffix>}
SUFFIX=${2:?usage: release-upload.sh <source-binary> <asset-suffix>}

: "${GITHUB_TOKEN:?GITHUB_TOKEN is not set}"
: "${CI_COMMIT_TAG:?CI_COMMIT_TAG is not set}"

REPO=${RELEASE_REPO:-3db-dk/hpm}
VERSION=${CI_COMMIT_TAG#v}
ARTIFACT="hpm-v$VERSION-$SUFFIX"

if [ ! -f "$SOURCE" ]; then
  echo "source binary not found: $SOURCE" >&2
  exit 1
fi

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
BODY="$WORK/body.json"

# Issues a GitHub API request, prints the HTTP status, leaves the body in $BODY.
# Deliberately omits `-f` so a 4xx reaches the status checks below instead of
# collapsing into a generic curl exit code.
api() {
  local method=$1 url=$2
  shift 2
  curl -sS -X "$method" "$url" \
    -H "Authorization: token $GITHUB_TOKEN" \
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

code=$(api POST "https://api.github.com/repos/$REPO/releases" \
  -H "Content-Type: application/json" \
  --data-binary "@$WORK/release.json")
case "$code" in
  201) echo "created release $CI_COMMIT_TAG" ;;
  422) echo "release $CI_COMMIT_TAG already exists" ;;
  *)   die "failed to create release (HTTP $code)" ;;
esac

# --- resolve the release id --------------------------------------------------

code=$(api GET "https://api.github.com/repos/$REPO/releases/tags/$CI_COMMIT_TAG")
[ "$code" = "200" ] || die "failed to look up release $CI_COMMIT_TAG (HTTP $code)"
RELEASE_ID=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["id"])' "$BODY")

# --- drop any existing asset of the same name --------------------------------

code=$(api GET "https://api.github.com/repos/$REPO/releases/$RELEASE_ID/assets?per_page=100")
[ "$code" = "200" ] || die "failed to list release assets (HTTP $code)"
ASSET_ID=$(python3 -c '
import json, sys
assets = json.load(open(sys.argv[1]))
print(next((str(a["id"]) for a in assets if a["name"] == sys.argv[2]), ""))
' "$BODY" "$ARTIFACT")

if [ -n "$ASSET_ID" ]; then
  code=$(api DELETE "https://api.github.com/repos/$REPO/releases/assets/$ASSET_ID")
  case "$code" in
    204|404) echo "removed previous $ARTIFACT" ;;
    *)       die "failed to delete previous $ARTIFACT (HTTP $code)" ;;
  esac
fi

# --- upload ------------------------------------------------------------------

code=$(api POST "https://uploads.github.com/repos/$REPO/releases/$RELEASE_ID/assets?name=$ARTIFACT" \
  -H "Content-Type: application/octet-stream" \
  --data-binary "@artifacts/$ARTIFACT")
[ "$code" = "201" ] || die "failed to upload $ARTIFACT (HTTP $code)"

# --- confirm what actually landed --------------------------------------------

EXPECTED=$(wc -c < "artifacts/$ARTIFACT" | tr -d '[:space:]')
code=$(api GET "https://api.github.com/repos/$REPO/releases/$RELEASE_ID/assets?per_page=100")
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
' "$BODY" "$ARTIFACT" "$EXPECTED"
