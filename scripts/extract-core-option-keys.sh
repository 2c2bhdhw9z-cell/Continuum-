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

# Standalone emulators (settings, not libretro options). Same output layout, prefixed std_.
clone() { [ -d "$work/$1/.git" ] || git clone -q --depth 1 "$2" "$work/$1"; git -C "$work/$1" rev-parse HEAD > "$out/$1.commit"; echo "$2" >> "$out/$1.commit"; }
clone std_sameboy https://github.com/LIJI32/SameBoy
{ sed -n '/typedef struct/,/} configuration_t/p' "$work/std_sameboy/SDL/configuration.h" \
    | grep -oE '[a-z_0-9]+(\[[A-Za-z0-9_]+\])?;' | sed 's/\[.*//; s/;//' | grep -v '^_deprecated\|^padding$\|^configuration_t$' | sed 's/^/SDL\/configuration.h:/'
  grep -ohE '@"GB[A-Z][A-Za-z0-9]+"' "$work/std_sameboy/iOS/GBSettingsViewController.m" | tr -d '@"' | sed 's/^/iOS\/GBSettingsViewController.m:/'
} | sort -u > "$out/std_sameboy.keys"
clone std_mgba https://github.com/mgba-emu/mgba
( cd "$work/std_mgba" && grep -roE 'mCoreConfigGet[A-Za-z]*Value\([^,]+, "[A-Za-z0-9_.]+"' src ) | sed -E 's/:.*"([^"]+)"$/:\1/' | sort -t: -k2 -u | sort > "$out/std_mgba.keys"
clone std_nanoboyadvance https://github.com/nba-emu/NanoBoyAdvance
( cd "$work/std_nanoboyadvance" && grep -oE 'toml::find_or<[^>]+>\([a-z_]+, "[a-z_]+"' src/platform/*/src/config.cc ) | sed -E 's/:.*"([a-z_]+)"$/:\1/' | sort -t: -k2 -u | sort > "$out/std_nanoboyadvance.keys"
# BGB is closed source: its settings come from the options list in its manual.
curl -sL https://bgb.bircd.org/manual.html -o "$work/bgb.html"
python3 - "$work/bgb.html" > "$out/std_bgb.keys" <<'PY'
import html, re, sys
t = html.unescape(re.sub('<[^>]+>', ' ', open(sys.argv[1], errors='ignore').read())); t = re.sub(r'\s+', ' ', t)
for a, _ in re.findall(r'([A-Za-z0-9][A-Za-z0-9 /\-"\'.]{1,40}?): \((on|off|[0-9a-z %.,]{1,14})\)', t):
    print(a.split('. ')[-1].strip())
PY
printf 'https://bgb.bircd.org/manual.html\nhttps://bgb.bircd.org/manual.html\n' > "$out/std_bgb.commit"
for f in std_sameboy std_mgba std_nanoboyadvance std_bgb; do echo "$f: $(wc -l < "$out/$f.keys") settings"; done
