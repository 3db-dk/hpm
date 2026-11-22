#!/usr/bin/env bash
#
# Refuse a Linux binary that needs a newer glibc or libstdc++ than Houdini 22
# (glibc 2.28, GLIBCXX_3.4.22). The loader refuses such a binary outright on
# an older system; built on the Ubuntu 24.04 worker, hpm needed GLIBC_2.39
# and ran on nothing older than that distro.
#
#   ci/linux-floors.sh <binary>...     check each binary (readelf -V)
#   ci/linux-floors.sh --text          check readelf -V text on stdin
#
# Prints what each needs; exits 1 if anything is above a floor.

set -euo pipefail

GLIBC_FLOOR=2.28
GLIBCXX_FLOOR=3.4.22

# highest <name>: the highest <name>_<version> in the text on stdin, or
# nothing. GLIBC_ does not match inside GLIBCXX_ (\b), nor GLIBC_PRIVATE.
highest() {
  { grep -oE "\\b$1_[0-9]+(\\.[0-9]+)*\\b" || true; } | sed "s/^$1_//" | sort -V | tail -1
}

# above <floor> <version>: true when version > floor, part by part as
# numbers, a missing part counting as 0 (so 2.28.0 is not above 2.28).
above() {
  [ -n "$2" ] && awk -v f="$1" -v v="$2" 'BEGIN {
    nf = split(f, a, "."); nv = split(v, b, "."); n = nf > nv ? nf : nv
    for (i = 1; i <= n; i++) {
      x = (i <= nv) ? b[i] + 0 : 0; y = (i <= nf) ? a[i] + 0 : 0
      if (x != y) exit !(x > y)
    }
    exit 1
  }'
}

# check <label>: the readelf text on stdin against the floors.
check() {
  local text glibc glibcxx
  text=$(cat)
  glibc=$(highest GLIBC <<<"$text")
  glibcxx=$(highest GLIBCXX <<<"$text")
  echo "$1 needs GLIBC_${glibc:-none}, GLIBCXX_${glibcxx:-none}"
  if above "$GLIBC_FLOOR" "$glibc" || above "$GLIBCXX_FLOOR" "$glibcxx"; then
    echo "$1 needs more than glibc $GLIBC_FLOOR / GLIBCXX_$GLIBCXX_FLOOR: older distros cannot load it" >&2
    return 1
  fi
}

[ $# -gt 0 ] || { echo "usage: linux-floors.sh <binary>... | --text" >&2; exit 2; }
if [ "$1" = "--text" ]; then
  check stdin
  exit
fi
status=0
for bin in "$@"; do
  readelf -V --wide "$bin" | check "$bin" || status=1
done
exit "$status"
