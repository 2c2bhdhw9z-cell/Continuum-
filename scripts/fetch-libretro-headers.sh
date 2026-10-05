#!/usr/bin/env bash
# Fetches the libretro headers the C++ wrapper compiles against into .work/hdr/libretro/.
#
#   ./scripts/fetch-libretro-headers.sh
#   FORCE=1 ./scripts/fetch-libretro-headers.sh     re-download even if present
#
# Why this exists: `native/switch-wrapper/build.sh` compiles against these headers and only
# these, both in the Linux checks job and on the macOS runner that builds the .ipa. Neither
# has another copy to use, and hand-declaring the structs is exactly the mistake that put two
# errors in the design document before real headers were consulted.
#
# PINNED to one libretro-common commit and CHECKED by sha256, so the ABI the wrapper is
# compiled against cannot change because upstream pushed to master. Moving the pin: take the
# new commit (gh api repos/libretro/libretro-common/commits/master --jq .sha), run this with
# FORCE=1 after changing LIBRETRO_COMMON_COMMIT, and replace both sha256 values below with the
# ones it reports.
#
# .work/ is gitignored: these are upstream files, fetched on demand, never committed.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DEST="$ROOT/.work/hdr/libretro"
LIBRETRO_COMMON_COMMIT="f0acd7bf37b653d4be2a4bb3b7d04b02b2004330"
BASE="https://raw.githubusercontent.com/libretro/libretro-common/$LIBRETRO_COMMON_COMMIT/include"

# "<sha256> <header>" at that commit.
HEADERS=(
  "c928d8f176b4e4bc45ee2a41ef236d25cc19e9bc2abc2e2e4c32cf08d6423801 libretro.h"
  "502ff92f64ba22bf588b85bfcf33fb44a3802f33523e008d9019afc42d3da119 libretro_vulkan.h"
)

# macOS ships shasum and no sha256sum.
sha256_of() {
  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{ print $1 }'
  else
    sha256sum "$1" | awk '{ print $1 }'
  fi
}

mkdir -p "$DEST"

for entry in "${HEADERS[@]}"; do
  want="${entry%% *}"
  header="${entry#* }"
  if [ -f "$DEST/$header" ] && [ "${FORCE:-0}" != "1" ]; then
    if [ "$(sha256_of "$DEST/$header")" = "$want" ]; then
      echo "==> $header already present and matches the pin (FORCE=1 to refetch)"
      continue
    fi
    echo "==> $header is present but is not the pinned file; refetching"
  fi
  echo "==> fetching $header at libretro-common $LIBRETRO_COMMON_COMMIT"
  curl -fsSL --retry 3 --retry-delay 2 "$BASE/$header" -o "$DEST/$header.tmp"
  got="$(sha256_of "$DEST/$header.tmp")"
  if [ "$got" != "$want" ]; then
    rm -f "$DEST/$header.tmp"
    echo "error: $header sha256 is $got, expected $want (libretro-common $LIBRETRO_COMMON_COMMIT)" >&2
    exit 1
  fi
  mv "$DEST/$header.tmp" "$DEST/$header"
  echo "    sha256 $got"
done

# A truncated or rate-limited download is worse than a missing one: it compiles for a while
# and then fails somewhere confusing. The checksums above already rule that out; these name the
# symbol each header must define, in case a pin is ever moved to something unexpected.
grep -q 'RETRO_API_VERSION' "$DEST/libretro.h" \
  || { echo "error: libretro.h looks truncated (no RETRO_API_VERSION)" >&2; exit 1; }
grep -q 'retro_hw_render_interface_vulkan' "$DEST/libretro_vulkan.h" \
  || { echo "error: libretro_vulkan.h looks truncated" >&2; exit 1; }

echo "==> headers in $DEST"
ls -1 "$DEST" | sed 's/^/      /'
