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
# Then name, URL, sha256 of the zip and pinned|unpinned are written to
# native/ios/build/lib/core-sources.txt.
#
# Also fetches prboom.wad, the freely licensed (GPL) support file the DOOM core needs, into
# native/ios/build/support/, which project.yml bundles and the app copies into the system folder.
#
# FROZEN BY CHECKSUM. The buildbot only keeps "latest", which changes whenever upstream rebuilds, so
# every file has a pinned sha256 (pinned_sha256 below): the bytes build 121 shipped, the last build
# known good on a phone. Per file:
#   a. the frozen copy on this repository's `core-mirror-1` pre-release is tried first, and used
#      only if its sha256 is the pin;
#   b. otherwise the original URL. If it matches the pin it is used, and with
#      CONTINUUM_MIRROR_UPLOAD=1, GH_TOKEN and `gh` (master builds in CI) it is uploaded to the
#      mirror, so later builds no longer depend on the buildbot still serving it. A failed upload
#      is a warning;
#   c. if the original no longer matches the pin, it is STILL used, because losing a system is worse
#      than shipping a newer build of it, but CI gets a ::warning:: naming the core and
#      core-sources.txt records it as unpinned. Moving a pin: check that build on a phone, then
#      copy the sha256 from its core-sources.txt into pinned_sha256 (the mirror copy is replaced
#      by the next master build, which uploads with --clobber).
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
# The frozen copies: a public pre-release (so /releases/latest, the owner's install link, never
# resolves to it) holding <core>.zip and prboom.wad, byte for byte the pinned files.
MIRROR_TAG="core-mirror-1"
MIRROR="https://github.com/2c2bhdhw9z-cell/Continuum-/releases/download/$MIRROR_TAG"

# The pinned sha256 of each downloaded file (the ZIP for a core), as build 121 recorded them.
# A case statement rather than an associative array, which macOS's bash 3.2 does not have.
# A core missing here is fetched and used anyway, as unpinned, with a warning.
pinned_sha256() {
  case "$1" in
    a5200)               echo "9c134a3b51496fbc7b43b2572e57e48ee884f92e0709c17e1c57d46ec3f605dd" ;;
    dosbox_pure)         echo "01200d7c974308a79f29de978685f276fadd510980f49324d6dc5d503854538a" ;;
    fbneo)               echo "04d0c5a304a9a1ed038ffebf3f94854c1cab71e6e5f1d3d004d552bff51fcd3c" ;;
    handy)               echo "402ba7a02edf5ba345464de4c0e332faefe86c4d4cb73ed44f6a11962b50846b" ;;
    mame2003_plus)       echo "2cebb44c5fa26931b60de23cb4260b1db7ebaade413f42df1665cffaaad89ce1" ;;
    mednafen_ngp)        echo "1ed8617c9e45a572998d093d5b389f3ee16eeb9e019c6dbe2f9b9beaa59ff7ae" ;;
    mednafen_pce)        echo "6e078cf6fe615bf171b8c2401681457f233e26793a5eb42d736159d55483815b" ;;
    mednafen_saturn)     echo "cbb2f723ccdd99230e28c493d35b0d728112f79d48f07081ad6852dc9340e16c" ;;
    mednafen_supergrafx) echo "e4fd854cf959fc7d9d6775a875fbf0736627a0bead2a4653afeb76766bf45881" ;;
    mednafen_vb)         echo "1bd318f06c0d1f794d6712a37c27f58479a37297838ea1e6d150d73283941ce4" ;;
    mednafen_wswan)      echo "9aa05d0207e283731e6d0bde7aa6830de4269b0465b53f47ceb2e99d0ff6e5a5" ;;
    picodrive)           echo "592c0f0d05e1615994002d7e313ee91f1347421ce0ceffc7d7d4ab471e121f1f" ;;
    pokemini)            echo "3964e0b8effcbc036e7af63adb31788b83759b5a4d78d5b3d8ce9ffc7a70ac02" ;;
    prboom)              echo "65b4f8ab9f21011a19b023bdde585946d908120c1a98c81480fec1e5b6997be7" ;;
    prosystem)           echo "c175f25ee35c79deaf5ccf8b2995dcb6c3db2890d98213a15f1de1c9b2500a9b" ;;
    puae)                echo "a90d7f7382da10eb7cb5def98676bb21d21ccad8780be7a0fb72ff033e5cad66" ;;
    vice_x64sc)          echo "04c07e0f795dfe3daafbbe42a5b81a37949cb2cdf83f6b641ee456bb49473fe7" ;;
    virtualjaguar)       echo "06861e89c131c88a277954fba521d0e71afa59f1295429bacbb33abb9790ba44" ;;
    yabause)             echo "5a3e5ce7691781b0107bfc4364acab242a64e931a74b95769bde4d73b85f3c8e" ;;
    prboom.wad)          echo "b4dd3642932193cc42bca0ee98bf30004888ca4850d69e85023b8baacfba1d1d" ;;
    *)                   echo "" ;;
  esac
}

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

# macOS ships shasum and no sha256sum; most Linux hosts have both. fetch_all checks one exists.
sha256_of() {
  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{ print $1 }'
  else
    sha256sum "$1" | awk '{ print $1 }'
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

# "<name> <url it came from> sha256:<sha> pinned", or "... unpinned pin:<sha>" when the bytes
# were not the pinned ones (see FROZEN BY CHECKSUM at the top).
record_source() {
  local core="$1" url="$2" sha="$3" state="$4" pin="$5"
  local manifest="$OUT_DIR/core-sources.txt"
  local line="$core $url sha256:$sha $state"
  [[ "$state" == "pinned" ]] || line="$line pin:${pin:-none}"
  mkdir -p "$OUT_DIR"
  if [[ -f "$manifest" ]]; then
    grep -v "^$core " "$manifest" > "$manifest.tmp" 2>/dev/null || true
    mv "$manifest.tmp" "$manifest"
  fi
  echo "$line" >> "$manifest"
  LC_ALL=C sort -o "$manifest" "$manifest"
}

# Downloads one pinned file to <dest>: the mirror's copy if its sha256 is the pin, else the original.
#   fetch_pinned <name> <mirror asset> <original url> <dest>
# Sets FETCHED_URL, FETCHED_SHA, FETCHED_STATE (pinned|unpinned) and FETCHED_UPLOAD (1 when the
# file is the pinned one but did not come from the mirror, so it should be uploaded once it has
# passed its checks). Returns 1 only when nothing could be downloaded at all.
#
# Callers run this as an `if` condition, where `set -e` is off, so every step checks for itself.
fetch_pinned() {
  local name="$1" asset="$2" original="$3" dest="$4"
  local pin got
  pin="$(pinned_sha256 "$name")"
  FETCHED_URL=""
  FETCHED_SHA=""
  FETCHED_STATE=""
  FETCHED_UPLOAD=0
  rm -f "$dest"

  if [[ -n "$pin" ]]; then
    # -s without -S: a 404 here is the normal state of an asset not mirrored yet, not an error.
    if curl -fsL --retry 2 --retry-delay 3 --connect-timeout 30 -o "$dest" "$MIRROR/$asset"; then
      got="$(sha256_of "$dest")"
      if [[ "$got" == "$pin" ]]; then
        echo "    frozen copy from $MIRROR_TAG, sha256 matches the pin"
        FETCHED_URL="$MIRROR/$asset"
        FETCHED_SHA="$got"
        FETCHED_STATE="pinned"
        return 0
      fi
      echo "    the $MIRROR_TAG copy of $asset is sha256 $got, not the pin; ignoring it"
    else
      echo "    no frozen copy of $asset on $MIRROR_TAG; using the original"
    fi
    rm -f "$dest"
  fi

  if ! curl -fsSL --retry 3 --retry-delay 5 --connect-timeout 30 -o "$dest" "$original"; then
    rm -f "$dest"
    return 1
  fi
  got="$(sha256_of "$dest")"
  FETCHED_URL="$original"
  FETCHED_SHA="$got"
  if [[ -n "$pin" && "$got" == "$pin" ]]; then
    echo "    sha256 matches the pin"
    FETCHED_STATE="pinned"
    FETCHED_UPLOAD=1
  else
    FETCHED_STATE="unpinned"
    # stdout, where every runner version reads workflow commands from.
    if [[ -z "$pin" ]]; then
      echo "::warning::$name has no pinned sha256 in scripts/fetch-buildbot-cores.sh; using the download from $original (sha256 $got), recorded as unpinned in core-sources.txt"
    else
      echo "::warning::$name changed upstream: $original is sha256 $got, not the pinned $pin. Using it anyway so the system is not lost; recorded as unpinned in core-sources.txt"
    fi
  fi
  return 0
}

# Uploads a verified pinned file to the mirror, when this run is allowed to (master builds in CI).
# Never fails: the build already has the file, and the next build will simply try again.
mirror_upload() {
  local file="$1"
  [[ "${CONTINUUM_MIRROR_UPLOAD:-0}" == "1" ]] || return 0
  if [[ -z "${GH_TOKEN:-}" || -z "${GITHUB_REPOSITORY:-}" ]] || ! command -v gh >/dev/null 2>&1; then
    echo "    not uploading $(basename "$file") to $MIRROR_TAG: needs GH_TOKEN, GITHUB_REPOSITORY and gh"
    return 0
  fi
  echo "    uploading $(basename "$file") to the $MIRROR_TAG release of $GITHUB_REPOSITORY"
  if ! gh release upload "$MIRROR_TAG" "$file" --repo "$GITHUB_REPOSITORY" --clobber; then
    echo "::warning::could not upload $(basename "$file") to the $MIRROR_TAG release; this build is unaffected, and the next one will fetch it from upstream and try again"
  fi
  return 0
}

fetch_one() {
  local core="$1" system="$2"
  local dylib zip url dir
  dylib="$(dylib_for "$core")"
  url="$BUILDBOT/$dylib.zip"
  dir="$WORK/$core"
  # Named <core>.zip because that is the mirror's asset name, and `gh release upload` names the
  # asset after the file.
  zip="$dir/$core.zip"
  rm -rf "$dir"
  mkdir -p "$dir"
  rm -f "$OUT_DIR/$dylib"

  echo "==> $core ($system): $url"
  if ! fetch_pinned "$core" "$core.zip" "$url" "$zip"; then
    echo "warning: $core: download failed; the .ipa ships without $system" >&2
    return 1
  fi
  if ! unzip -oq "$zip" -d "$dir/unzipped"; then
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
  record_source "$core" "$FETCHED_URL" "$FETCHED_SHA" "$FETCHED_STATE" "$(pinned_sha256 "$core")"
  # Only after every check passed, so the mirror only ever holds pinned files that staged cleanly.
  if [[ "$FETCHED_UPLOAD" == "1" ]]; then
    mirror_upload "$zip"
  fi
  return 0
}

fetch_support() {
  local dir="$WORK/support"
  local wad="$dir/prboom.wad"
  rm -rf "$dir"
  mkdir -p "$dir" "$SUPPORT_DIR"
  rm -f "$SUPPORT_DIR/prboom.wad"
  echo "==> prboom.wad (DOOM support file, GPL): $PRBOOM_WAD_URL"
  if fetch_pinned "prboom.wad" "prboom.wad" "$PRBOOM_WAD_URL" "$wad" && [[ -s "$wad" ]]; then
    cp "$wad" "$SUPPORT_DIR/prboom.wad" || {
      echo "warning: could not stage prboom.wad; DOOM will ask for it at launch" >&2
      return 0
    }
    record_source "prboom.wad" "$FETCHED_URL" "$FETCHED_SHA" "$FETCHED_STATE" "$(pinned_sha256 prboom.wad)"
    if [[ "$FETCHED_UPLOAD" == "1" ]]; then
      mirror_upload "$wad"
    fi
  else
    echo "warning: prboom.wad download failed; DOOM will ask for it at launch" >&2
  fi
}

fetch_all() {
  mkdir -p "$OUT_DIR" "$WORK"
  command -v curl >/dev/null 2>&1 || { echo "error: curl not found" >&2; exit 1; }
  command -v unzip >/dev/null 2>&1 || { echo "error: unzip not found" >&2; exit 1; }
  command -v shasum >/dev/null 2>&1 || command -v sha256sum >/dev/null 2>&1 \
    || { echo "error: neither shasum nor sha256sum found; the pinned checksums cannot be checked" >&2; exit 1; }

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
