#!/usr/bin/env bash
# Fetches the libretro headers the C++ wrapper compiles against into .work/hdr/libretro/.
#
#   ./scripts/fetch-libretro-headers.sh
#
# Why this exists: `native/switch-wrapper/build.sh` prefers the libretro-common that ships
# inside the vendored core sources, so the wrapper cannot drift from the ABI the cores use.
# That directory only exists after a core has been built, which happens on Linux with
# wasi-sdk. The macOS runner that builds the .ipa never does that, so it needs the headers
# from somewhere — and hand-declaring the structs is exactly the mistake that put two
# errors in the design document before real headers were consulted.
#
# .work/ is gitignored: these are upstream files, fetched on demand, never committed.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DEST="$ROOT/.work/hdr/libretro"
BASE="https://raw.githubusercontent.com/libretro/libretro-common/master/include"

mkdir -p "$DEST"

for header in libretro.h libretro_vulkan.h; do
  if [ -f "$DEST/$header" ] && [ "${FORCE:-0}" != "1" ]; then
    echo "==> $header already present (FORCE=1 to refetch)"
    continue
  fi
  echo "==> fetching $header"
  curl -fsSL --retry 3 --retry-delay 2 "$BASE/$header" -o "$DEST/$header.tmp"
  mv "$DEST/$header.tmp" "$DEST/$header"
done

# A truncated or rate-limited download is worse than a missing one: it compiles for a while
# and then fails somewhere confusing. Check for a symbol each header must define.
grep -q 'RETRO_API_VERSION' "$DEST/libretro.h" \
  || { echo "error: libretro.h looks truncated (no RETRO_API_VERSION)" >&2; exit 1; }
grep -q 'retro_hw_render_interface_vulkan' "$DEST/libretro_vulkan.h" \
  || { echo "error: libretro_vulkan.h looks truncated" >&2; exit 1; }

echo "==> headers in $DEST"
ls -1 "$DEST" | sed 's/^/      /'
