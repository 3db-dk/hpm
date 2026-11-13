#!/usr/bin/env bash
#
# Tests for ci/release-upload.sh and ci/check-version.sh against
# ci/tests/fake_github.py on 127.0.0.1. Fake credentials only.
#
#   bash ci/tests/release_upload_test.sh

set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
SCRATCH=$(mktemp -d)
PIDS=()
cleanup() {
  for pid in "${PIDS[@]}"; do kill "$pid" 2>/dev/null || true; done
  rm -rf "$SCRATCH"
}
trap cleanup EXIT

FAILS=0
pass() { echo "ok   - $1"; }
fail() { echo "FAIL - $1" >&2; FAILS=$((FAILS + 1)); }

# Start a fake GitHub with optional seed state; sets PORT.
start_fake() {
  local seed=${1:-}
  local out="$SCRATCH/port.$RANDOM"
  python3 "$ROOT/ci/tests/fake_github.py" ${seed:+"$seed"} > "$out" &
  PIDS+=($!)
  for _ in $(seq 50); do
    [ -s "$out" ] && break
    sleep 0.1
  done
  PORT=$(head -1 "$out")
}

# Run the upload script in a scratch checkout against the fake.
upload() {
  local binary=$1
  (
    cd "$SCRATCH/work"
    GITHUB_TOKEN=fake-test-token CI_COMMIT_TAG=v9.9.9 \
      GITHUB_API_URL="http://127.0.0.1:$PORT" \
      GITHUB_UPLOADS_URL="http://127.0.0.1:$PORT" \
      bash "$ROOT/ci/release-upload.sh" "$binary" linux-x86_64
  ) > "$SCRATCH/out.log" 2>&1
}

assets() {
  curl -sS -H "Authorization: token fake-test-token" \
    "http://127.0.0.1:$PORT/repos/3db-dk/hpm/releases/2/assets"
}

asset_text() {
  local name=$1
  local url
  url=$(assets | python3 -c 'import json,sys; print(next(a["browser_download_url"] for a in json.load(sys.stdin) if a["name"]==sys.argv[1]))' "$name")
  curl -sS "$url"
}

mkdir -p "$SCRATCH/work"
printf '## [9.9.9]\n\nNotes.\n' > "$SCRATCH/work/CHANGELOG.md"
printf 'binary one' > "$SCRATCH/bin1"
printf 'binary two' > "$SCRATCH/bin2"
if command -v sha256sum >/dev/null; then SUM="sha256sum"; else SUM="shasum -a 256"; fi
DIGEST1=$($SUM "$SCRATCH/bin1" | cut -d' ' -f1)

# 1. A first upload publishes the binary and its checksum file.
start_fake
if upload "$SCRATCH/bin1" \
  && [ "$(asset_text hpm-v9.9.9-linux-x86_64)" = "binary one" ] \
  && [ "$(asset_text hpm-v9.9.9-linux-x86_64.sha256)" = "$DIGEST1  hpm-v9.9.9-linux-x86_64" ]; then
  pass "first upload publishes the binary and its .sha256"
else
  fail "first upload: $(cat "$SCRATCH/out.log")"
fi

# 2. Rerunning with the same binary changes nothing and succeeds.
if upload "$SCRATCH/bin1" && [ "$(asset_text hpm-v9.9.9-linux-x86_64)" = "binary one" ]; then
  pass "rerun with the same binary is a no-op"
else
  fail "same-binary rerun: $(cat "$SCRATCH/out.log")"
fi

# 3. A different binary never replaces a published one.
if upload "$SCRATCH/bin2"; then
  fail "a different binary replaced the published one"
elif [ "$(asset_text hpm-v9.9.9-linux-x86_64)" = "binary one" ]; then
  pass "a different binary is refused and the published one kept"
else
  fail "published binary changed after a refused upload"
fi

# 4. A published binary without a checksum file is not replaced either.
cat > "$SCRATCH/seed.json" <<JSON
{"releases": [{"tag_name": "v9.9.9", "assets": {"hpm-v9.9.9-linux-x86_64": "old binary"}}]}
JSON
start_fake "$SCRATCH/seed.json"
if upload "$SCRATCH/bin1"; then
  fail "replaced a published binary that had no checksum file"
else
  pass "a published binary without a checksum file is not replaced"
fi

# 5. The API base can only be GitHub or this machine: the token goes nowhere else.
if (
  cd "$SCRATCH/work"
  GITHUB_TOKEN=fake-test-token CI_COMMIT_TAG=v9.9.9 GITHUB_API_URL="http://example.invalid" \
    bash "$ROOT/ci/release-upload.sh" "$SCRATCH/bin1" linux-x86_64
) > "$SCRATCH/out.log" 2>&1; then
  fail "accepted an API base other than GitHub or 127.0.0.1"
else
  pass "an API base other than GitHub or 127.0.0.1 is refused"
fi

# 6. The tag must name the version being built.
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/Cargo.toml" | head -1)
if (cd "$ROOT" && CI_COMMIT_TAG="v$VERSION" bash ci/check-version.sh) >/dev/null 2>&1; then
  pass "a tag matching the crate version passes"
else
  fail "a tag matching the crate version was refused"
fi
if (cd "$ROOT" && CI_COMMIT_TAG="v0.0.0-not-$VERSION" bash ci/check-version.sh) >/dev/null 2>&1; then
  fail "a tag that does not match the crate version passed"
else
  pass "a tag that does not match the crate version is refused"
fi

[ "$FAILS" -eq 0 ] || { echo "$FAILS failure(s)" >&2; exit 1; }
echo "all release script tests passed"
