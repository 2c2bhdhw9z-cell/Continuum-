#!/usr/bin/env bash
# Clones the libretro cores that the cBoy inventory compares against and extracts every core
# option KEY (no descriptions) from their option-definition files. Output: one file per core in
# docs/option-sources/<core>.keys plus <core>.commit. Usage: scripts/extract-core-option-keys.sh [workdir]
set -euo pipefail
here=$(cd "$(dirname "$0")/.." && pwd)
work=${1:-/tmp/option-sources}
out="$here/docs/option-sources"
mkdir -p "$work" "$out"
# name|repo|files (globs, relative to the repo)|key prefix regex
cores=(
  "mgba|https://github.com/libretro/mgba|src/platform/libretro/libretro_core_options.h|mgba_"
  "gambatte|https://github.com/libretro/gambatte-libretro|libgambatte/libretro/libretro_core_options.h|gambatte_"
  "sameboy|https://github.com/libretro/SameBoy|libretro/libretro_core_options.*|sameboy_"
  "vbam|https://github.com/libretro/vbam-libretro|src/libretro/libretro_core_options.h|vbam_"
  "gpsp|https://github.com/libretro/gpsp|libretro/libretro_core_options.h|gpsp_"
  "tgbdual|https://github.com/libretro/tgbdual-libretro|libretro/libretro.cpp|tgbdual_"
  "beetle_gba|https://github.com/libretro/beetle-gba-libretro|libretro.cpp|gba_"
)
for entry in "${cores[@]}"; do
  IFS='|' read -r name repo files prefix <<<"$entry"
  dir="$work/$name"
  [ -d "$dir/.git" ] || git clone -q --depth 1 "$repo" "$dir" || { echo "$name: clone failed" >&2; continue; }
  git -C "$dir" rev-parse HEAD > "$out/$name.commit"
  echo "$repo" >> "$out/$name.commit"
  # shellcheck disable=SC2086
  ( cd "$dir" && cat $files 2>/dev/null ) \
    | grep -oE "\"${prefix}[A-Za-z0-9_]+\"" | tr -d '"' | sort -u > "$out/$name.keys" || true
  echo "$name: $(wc -l < "$out/$name.keys") keys at $(head -1 "$out/$name.commit")"
done
