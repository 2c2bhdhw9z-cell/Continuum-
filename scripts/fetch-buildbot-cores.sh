#!/usr/bin/env bash
# Downloads the prebuilt libretro cores the .ipa ships from the libretro iOS buildbot, checks them
# exactly like a core compiled here, and stages them into native/ios/build/lib/.
#
#   scripts/fetch-buildbot-cores.sh            fetch, check and stage every core below
#   scripts/fetch-buildbot-cores.sh names      print the canonical dylib filenames and stop
#   scripts/fetch-buildbot-cores.sh systems    print "<dylib> <system>" per core and stop
#
# WHY PREBUILT: twenty more cores compiled from source would blow the CI time limit
# (docs/MANIC_PARITY.md, "Decisions already made"). Cores the buildbot does not carry are built
# from source by scripts/build-core.sh like the original twelve.
#
# EVERY CORE HERE IS OPTIONAL. A download that fails on the day, or a dylib that fails a check,
# is dropped with a warning and this script still exits 0: the .ipa ships without that core,
# package-ipa.sh removes its embed from project.yml, the CI verify step prints a warning naming
# the system, and the app's cores line names the dylib as not in the bundle. Only a usage error
# exits non-zero.
#
# THE CHECKS, per core, all of which run on Linux as well as on the Mac runner:
#   1. it is a 64-bit arm64 Mach-O dylib (otool -hv);
#   2. its minimum OS (LC_BUILD_VERSION minos, or LC_VERSION_MIN_IPHONEOS) is iOS and is no newer
#      than the app's deployment target, 16.0; a macOS or simulator build is refused;
#   3. scripts/build-core.sh ios-stage-prebuilt, the SAME staged-dylib check every from-source
#      core goes through: all twenty libretro entry points the engine resolves, the install name
#      reset to @rpath/<name>, and the copy into native/ios/build/lib/.
# Then name, URL and sha256 of the zip are written to native/ios/build/lib/core-sources.txt.
#
# Also fetches prboom.wad, the freely licensed (GPL) support file the DOOM core needs, into
# native/ios/build/support/, which project.yml bundles and the app copies into the system folder.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BUILD_CORE="$ROOT/scripts/build-core.sh"
OUT_DIR="$ROOT/native/ios/build/lib"
SUPPORT_DIR="$ROOT/native/ios/build/support"
WORK="$ROOT/.work/buildbot"
BUILDBOT="https://buildbot.libretro.com/nightly/apple/ios-arm64/latest"
# Matches IPHONEOS_DEPLOYMENT_TARGET in native/ios/project.yml.
MAX_MIN_OS="16.0"
PRBOOM_WAD_URL="https://raw.githubusercontent.com/libretro/libretro-prboom/master/prboom.wad"

# THE LIST, defined once. "<core> <system label>". The dylib is always
# <core>_libretro_ios.dylib, which is the buildbot's own name, so it is derived rather than stated.
# Each name must also appear in native/ios/project.yml, .github/workflows/ios.yml and
# native/ios/ContinuumApp.swift; build-engine.sh checks that before anything is built.
BUILDBOT_CORES=(
  "mednafen_wswan WonderSwan"
  "mednafen_ngp Neo-Geo-Pocket"
  "mednafen_pce PC-Engine-CD"
  "mednafen_supergrafx SuperGrafx"
  "puae Amiga"
  "vice_x64sc Commodore-64"
  "dosbox_pure DOS"
  "prboom DOOM"
  "virtualjaguar Atari-Jaguar"
  "handy Atari-Lynx"
  "prosystem Atari-7800"
  "a5200 Atari-5200"
  "fbneo Arcade"
  "mame2003_plus Arcade-MAME-2003-Plus"
  "pokemini Pokemon-Mini"
  "mednafen_vb Virtual-Boy"
  "yabause Saturn"
  "mednafen_saturn Saturn-Beetle"
  "picodrive Sega-32X"
)

dylib_for() { echo "${1}_libretro_ios.dylib"; }

print_names() {
  local entry
  for entry in "${BUILDBOT_CORES[@]}"; do
    dylib_for "${entry%% *}"
  done
}

print_systems() {
  local entry
  for entry in "${BUILDBOT_CORES[@]}"; do
    echo "$(dylib_for "${entry%% *}") ${entry#* }"
  done
}

# Apple's tool on a Mac, LLVM's spelling anywhere else. Same output for the flags used here.
otool_cmd() {
  if [[ "$(uname -s)" == "Darwin" ]] && command -v otool >/dev/null 2>&1; then
    echo otool
  elif command -v llvm-otool >/dev/null 2>&1; then
    echo llvm-otool
  else
    echo otool
  fi
}

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{ print $1 }'
  else
    shasum -a 256 "$1" | awk '{ print $1 }'
  fi
}

# "a.b" <= "c.d", numerically.
version_le() {
  local a_major a_minor b_major b_minor
  a_major="${1%%.*}"; a_minor="${1#*.}"; [[ "$a_minor" == "$1" ]] && a_minor=0
  b_major="${2%%.*}"; b_minor="${2#*.}"; [[ "$b_minor" == "$2" ]] && b_minor=0
  a_minor="${a_minor%%.*}"; b_minor="${b_minor%%.*}"
  (( a_major < b_major )) && return 0
  (( a_major > b_major )) && return 1
  (( a_minor <= b_minor ))
}

# Prints nothing and returns 0 when the dylib is arm64 iOS with a minimum OS <= MAX_MIN_OS.
# Otherwise prints the reason and returns 1.
check_macho() {
  local dylib="$1"
  local otool header loads
  otool="$(otool_cmd)"
  header="$("$otool" -hv "$dylib" 2>/dev/null || true)"
  if ! grep -q "MH_MAGIC_64 *ARM64 .*DYLIB" <<<"$header"; then
    echo "not a 64-bit arm64 Mach-O dylib"
    return 1
  fi
  loads="$("$otool" -l "$dylib" 2>/dev/null || true)"
  local min=""
  if grep -q "LC_VERSION_MIN_IPHONEOS" <<<"$loads"; then
    min="$(awk '/LC_VERSION_MIN_IPHONEOS/ { f = 1 } f && $1 == "version" { print $2; exit }' <<<"$loads")"
  elif grep -q "LC_BUILD_VERSION" <<<"$loads"; then
    # platform 2 is PLATFORM_IOS in <mach-o/loader.h>; some otools print it as "ios".
    local platform
    platform="$(awk '/LC_BUILD_VERSION/ { f = 1 } f && $1 == "platform" { print $2; exit }' <<<"$loads")"
    case "$platform" in
      2|ios|IOS|iOS) ;;
      *) echo "built for platform '$platform', not iOS"; return 1 ;;
    esac
    min="$(awk '/LC_BUILD_VERSION/ { f = 1 } f && $1 == "minos" { print $2; exit }' <<<"$loads")"
  else
    echo "no LC_BUILD_VERSION or LC_VERSION_MIN_IPHONEOS, so not provably an iOS build"
    return 1
  fi
  if [[ -z "$min" ]]; then
    echo "could not read its minimum iOS version"
    return 1
  fi
  if ! version_le "$min" "$MAX_MIN_OS"; then
    echo "needs iOS $min, newer than the app's $MAX_MIN_OS"
    return 1
  fi
  return 0
}

record_source() {
  local core="$1" url="$2" sha="$3"
  local manifest="$OUT_DIR/core-sources.txt"
  mkdir -p "$OUT_DIR"
  if [[ -f "$manifest" ]]; then
    grep -v "^$core " "$manifest" > "$manifest.tmp" 2>/dev/null || true
    mv "$manifest.tmp" "$manifest"
  fi
  echo "$core $url sha256:$sha" >> "$manifest"
  LC_ALL=C sort -o "$manifest" "$manifest"
}

fetch_one() {
  local core="$1" system="$2"
  local dylib zip url dir
  dylib="$(dylib_for "$core")"
  zip="$dylib.zip"
  url="$BUILDBOT/$zip"
  dir="$WORK/$core"
  rm -rf "$dir"
  mkdir -p "$dir"
  rm -f "$OUT_DIR/$dylib"

  echo "==> $core ($system): $url"
  if ! curl -fsSL --retry 3 --retry-delay 5 --connect-timeout 30 -o "$dir/$zip" "$url"; then
    echo "warning: $core: download failed; the .ipa ships without $system" >&2
    return 1
  fi
  if ! unzip -oq "$dir/$zip" -d "$dir/unzipped"; then
    echo "warning: $core: the zip would not unpack; the .ipa ships without $system" >&2
    return 1
  fi
  local built="$dir/unzipped/$dylib"
  if [[ ! -f "$built" ]]; then
    built="$(find "$dir/unzipped" -name '*_libretro_ios.dylib' -type f | head -1 || true)"
  fi
  if [[ -z "$built" || ! -f "$built" ]]; then
    echo "warning: $core: no $dylib inside the zip; the .ipa ships without $system" >&2
    return 1
  fi

  local reason
  if ! reason="$(check_macho "$built")"; then
    echo "warning: $core: dropped, $reason; the .ipa ships without $system" >&2
    return 1
  fi
  # The same staged-dylib check, install-name reset and copy as every from-source core.
  if ! bash "$BUILD_CORE" ios-stage-prebuilt "$built" "$dylib"; then
    echo "warning: $core: dropped, it failed the staged-dylib check; the .ipa ships without $system" >&2
    rm -f "$OUT_DIR/$dylib"
    return 1
  fi
  record_source "$core" "$url" "$(sha256_of "$dir/$zip")"
  return 0
}

fetch_support() {
  mkdir -p "$SUPPORT_DIR"
  echo "==> prboom.wad (DOOM support file, GPL): $PRBOOM_WAD_URL"
  if curl -fsSL --retry 3 --retry-delay 5 --connect-timeout 30 \
      -o "$SUPPORT_DIR/prboom.wad.tmp" "$PRBOOM_WAD_URL"; then
    mv "$SUPPORT_DIR/prboom.wad.tmp" "$SUPPORT_DIR/prboom.wad"
    record_source "prboom.wad" "$PRBOOM_WAD_URL" "$(sha256_of "$SUPPORT_DIR/prboom.wad")"
  else
    rm -f "$SUPPORT_DIR/prboom.wad.tmp"
    echo "warning: prboom.wad download failed; DOOM will ask for it at launch" >&2
  fi
}

fetch_all() {
  mkdir -p "$OUT_DIR" "$WORK"
  command -v curl >/dev/null 2>&1 || { echo "error: curl not found" >&2; exit 1; }
  command -v unzip >/dev/null 2>&1 || { echo "error: unzip not found" >&2; exit 1; }

  local entry ok="" dropped=""
  for entry in "${BUILDBOT_CORES[@]}"; do
    local core="${entry%% *}" system="${entry#* }"
    if fetch_one "$core" "$system"; then
      ok="$ok $core"
    else
      dropped="$dropped $core"
    fi
  done
  fetch_support

  echo
  echo "==> buildbot core summary (native/ios/build/lib)"
  for entry in "${BUILDBOT_CORES[@]}"; do
    local dylib
    dylib="$(dylib_for "${entry%% *}")"
    if [[ -f "$OUT_DIR/$dylib" ]]; then
      echo "      ok       $dylib"
    else
      echo "      MISSING  $dylib (${entry#* })"
    fi
  done
  if [[ -n "$dropped" ]]; then
    echo "warning: prebuilt core(s) not staged:$dropped" >&2
  fi
}

case "${1:-fetch}" in
  names) print_names ;;
  systems) print_systems ;;
  fetch) fetch_all ;;
  -h|--help|help)
    sed -n '2,8p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
    ;;
  *)
    echo "error: unknown subcommand '$1' (use: names, systems, fetch)" >&2
    exit 1
    ;;
esac
