#!/usr/bin/env bash
# Downloads the prebuilt libretro cores the .ipa ships from the libretro iOS buildbot, checks them
# exactly like a core compiled here, and stages them into native/ios/build/lib/.
#
#   scripts/fetch-buildbot-cores.sh            fetch, check and stage every core below
#   scripts/fetch-buildbot-cores.sh names      print the canonical dylib filenames and stop
#   scripts/fetch-buildbot-cores.sh systems    print "<dylib> <system>" per core and stop
#   scripts/fetch-buildbot-cores.sh pins       print "<core>\t<zip pin>\t<dylib pin>\t<system>" and stop
#
# WHY PREBUILT: sixteen more cores compiled from source would blow the CI time limit
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
# Then name, URL, sha256 of the zip, pinned|unpinned and the dylib's sha256 are written to
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
#      by the next master build, which uploads with --clobber) and its dylib: sha256 into
#      pinned_dylib_sha256. Then update crates/emulator-bridge/src/core_updates.rs: the core's
#      zip sha256 in CORE_BUILDS, and one CORE_UPDATES entry saying whether old save states load
#      (players get a heads-up from it). cargo test fails until you do.
# Each core also has a pinned sha256 of the dylib inside its zip (pinned_dylib_sha256). The
# buildbot re-zips every core nightly, so a zip can change while the dylib in it is the same build:
# a zip that misses its pin but whose dylib matches the dylib pin is the same checked core, and is
# recorded as pinned with no warning (and not uploaded, so the mirror keeps the pinned zip). A zip
# that matches its pin while its dylib does not match the dylib pin gets a ::warning:: naming both
# sha256s, and the core is kept.
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

# The pinned sha256 of each downloaded file (the ZIP for a core), as build 121 recorded them;
# dosbox_pure, mednafen_saturn, mednafen_supergrafx and puae moved on 12 October 2026 after the
# update check (.agents/tasks/updates-2026-10-12/EVALUATION.md). A case statement rather than an associative array, which macOS's bash 3.2 does not have.
# A core missing here is fetched and used anyway, as unpinned, with a warning.
pinned_sha256() {
  case "$1" in
    a5200)               echo "9c134a3b51496fbc7b43b2572e57e48ee884f92e0709c17e1c57d46ec3f605dd" ;;
    dosbox_pure)         echo "d12b0ad50a93713462b4060375b28258be940ab2692d66a33be4f72850761ad6" ;;
    handy)               echo "402ba7a02edf5ba345464de4c0e332faefe86c4d4cb73ed44f6a11962b50846b" ;;
    mednafen_ngp)        echo "1ed8617c9e45a572998d093d5b389f3ee16eeb9e019c6dbe2f9b9beaa59ff7ae" ;;
    mednafen_pce)        echo "6e078cf6fe615bf171b8c2401681457f233e26793a5eb42d736159d55483815b" ;;
    mednafen_saturn)     echo "cb67ba4be4a5d6662d67daf89e34f282274764054dacb16c24f02de2c950a582" ;;
    mednafen_supergrafx) echo "3e51d2d9131c34393e563cbfbb6b438af8f5eeb63c00b10899a64b27038c8408" ;;
    mednafen_vb)         echo "1bd318f06c0d1f794d6712a37c27f58479a37297838ea1e6d150d73283941ce4" ;;
    mednafen_wswan)      echo "9aa05d0207e283731e6d0bde7aa6830de4269b0465b53f47ceb2e99d0ff6e5a5" ;;
    pokemini)            echo "3964e0b8effcbc036e7af63adb31788b83759b5a4d78d5b3d8ce9ffc7a70ac02" ;;
    prboom)              echo "65b4f8ab9f21011a19b023bdde585946d908120c1a98c81480fec1e5b6997be7" ;;
    prosystem)           echo "c175f25ee35c79deaf5ccf8b2995dcb6c3db2890d98213a15f1de1c9b2500a9b" ;;
    puae)                echo "6f8dc0190d92af9398a29c4647147d3d300ba42620763d6ba53178f96e009889" ;;
    vice_x64sc)          echo "04c07e0f795dfe3daafbbe42a5b81a37949cb2cdf83f6b641ee456bb49473fe7" ;;
    virtualjaguar)       echo "06861e89c131c88a277954fba521d0e71afa59f1295429bacbb33abb9790ba44" ;;
    yabause)             echo "5a3e5ce7691781b0107bfc4364acab242a64e931a74b95769bde4d73b85f3c8e" ;;
    prboom.wad)          echo "b4dd3642932193cc42bca0ee98bf30004888ca4850d69e85023b8baacfba1d1d" ;;
    *)                   echo "" ;;
  esac
}

# The pinned sha256 of the DYLIB inside each pinned zip. The buildbot re-zips every core nightly,
# so the zip's bytes change while the dylib inside stays the same build; a matching dylib is the
# same checked core (see FROZEN BY CHECKSUM at the top).
pinned_dylib_sha256() {
  case "$1" in
    a5200)               echo "3c1661671225181d16ec39c4a663fd4bc4cfead9d0d48addccff921f8be51002" ;;
    dosbox_pure)         echo "26d8a1c03babdff9f4ef4314a8c858bf246fffecb245d10a8f9f14294e296e25" ;;
    handy)               echo "b2d2c86e03fe3c6fd49ba9dac73a59dded85628077abd2e015659c7611604f06" ;;
    mednafen_ngp)        echo "0d9932e1ee9a198918a05386071dd6c58ff4aff78f6c85dcdc5098f8e976fa0b" ;;
    mednafen_pce)        echo "aa2f4dd3504b9bf80ae693e08013038d6d0b507b2a79c05ffc5c2ddfd251b2a8" ;;
    mednafen_saturn)     echo "404286424732f49e42253898346bf2b6a9c217930c2c963d17dc6a2df737dcab" ;;
    mednafen_supergrafx) echo "19e3804e130944e80bbe8b629d9aa3ff4b29ed35562f4dd3c6cbd0168fb21a29" ;;
    mednafen_vb)         echo "093bb6341a36b0dc60cc4a3eee59286720af4759040bcd49bfa7a60c645fa64c" ;;
    mednafen_wswan)      echo "0f3a7249303cbe3be71ac9e99f425a4cc05034abb7615baedfa95d17e8155117" ;;
    pokemini)            echo "ea44fd3e06a0fb0fa74c7c10b632934d1a5ad708068598ec5ad2a71bfe641757" ;;
    prboom)              echo "fad43c8d4bc5c9dc23306e7d4340428ea664e10c36b78266bb7030fe5089e041" ;;
    prosystem)           echo "69ff2105f2f94e28ffda83ca60a93c83b169fedcb32158faf69f17e165624601" ;;
    puae)                echo "93a8ba7e9fb337bcc6676e49ba738bf9288fe883acdf9ea077cf829fce4a3e93" ;;
    vice_x64sc)          echo "5613514bb8142d542f01684f00f4f6c66d220b1261323758c0497b230e2ba64c" ;;
    virtualjaguar)       echo "0abf4f45b628b1bcfca3a762f0959a72d56c98f9f218ed2bc8fdfb93343f74ed" ;;
    yabause)             echo "0f4c457fe1ef50cc451ffa7cb1e49b43463df4313b6165180d9fff9769188999" ;;
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
  "pokemini Pokemon-Mini"
  "mednafen_vb Virtual-Boy"
  "yabause Saturn"
  "mednafen_saturn Saturn-Beetle"
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

print_pins() {
  local entry core
  for entry in "${BUILDBOT_CORES[@]}"; do
    core="${entry%% *}"
    printf '%s\t%s\t%s\t%s\n' "$core" "$(pinned_sha256 "$core")" "$(pinned_dylib_sha256 "$core")" "${entry#* }"
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
# were not the pinned ones (see FROZEN BY CHECKSUM at the top). For a core, an optional sixth
# argument, the dylib's sha256, is appended as " dylib:<sha>".
record_source() {
  local core="$1" url="$2" sha="$3" state="$4" pin="$5" dylib_sha="${6:-}"
  local manifest="$OUT_DIR/core-sources.txt"
  local line="$core $url sha256:$sha $state"
  [[ "$state" == "pinned" ]] || line="$line pin:${pin:-none}"
  [[ -z "$dylib_sha" ]] || line="$line dylib:$dylib_sha"
  mkdir -p "$OUT_DIR"
  if [[ -f "$manifest" ]]; then
    grep -v "^$core " "$manifest" > "$manifest.tmp" 2>/dev/null || true
    mv "$manifest.tmp" "$manifest"
  fi
  echo "$line" >> "$manifest"
  LC_ALL=C sort -o "$manifest" "$manifest"
}

# Takes <name>'s line out of core-sources.txt. Called as each fetch STARTS, next to the rm -f of
# its file, so the line comes back only through record_source once the file has passed every
# check. Otherwise a fetch that failed today kept the line from the run that stored the CI core
# cache, and the manifest described a core this .ipa does not have.
forget_source() {
  local name="$1"
  local manifest="$OUT_DIR/core-sources.txt"
  [[ -f "$manifest" ]] || return 0
  grep -v "^$name " "$manifest" > "$manifest.tmp" 2>/dev/null || true
  mv "$manifest.tmp" "$manifest"
}

# Downloads one pinned file to <dest>: the mirror's copy if its sha256 is the pin, else the original.
#   fetch_pinned <name> <mirror asset> <original url> <dest>
# Sets FETCHED_URL, FETCHED_SHA, FETCHED_STATE (pinned|unpinned), FETCHED_UPLOAD (1 when the
# file is the pinned one but did not come from the mirror, so it should be uploaded once it has
# passed its checks) and FETCHED_WARNING (the ::warning:: line for an unpinned file, which the
# caller echoes). Returns 1 only when nothing could be downloaded at all.
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
  FETCHED_WARNING=""
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
    # Not echoed here: fetch_one first checks whether the dylib inside is still the pinned one.
    if [[ -z "$pin" ]]; then
      FETCHED_WARNING="::warning::$name has no pinned sha256 in scripts/fetch-buildbot-cores.sh; using the download from $original (sha256 $got), recorded as unpinned in core-sources.txt"
    else
      FETCHED_WARNING="::warning::$name changed upstream: $original is sha256 $got, not the pinned $pin. Using it anyway so the system is not lost; recorded as unpinned in core-sources.txt"
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
  forget_source "$core"

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

  local dylib_sha dylib_pin
  dylib_sha="$(sha256_of "$built")"
  dylib_pin="$(pinned_dylib_sha256 "$core")"
  if [[ "$FETCHED_STATE" == "unpinned" && -n "$dylib_pin" && "$dylib_sha" == "$dylib_pin" ]]; then
    FETCHED_STATE="pinned"
    FETCHED_WARNING=""
    echo "    same dylib as the pin (the buildbot re-zipped it)"
  elif [[ "$FETCHED_STATE" == "pinned" && -n "$dylib_pin" && "$dylib_sha" != "$dylib_pin" ]]; then
    # stdout, where every runner version reads workflow commands from.
    echo "::warning::$core: the zip matches its pin but the dylib inside is sha256 $dylib_sha, not the pinned dylib $dylib_pin. Keeping it; check pinned_dylib_sha256 in scripts/fetch-buildbot-cores.sh"
  fi
  if [[ -n "$FETCHED_WARNING" ]]; then
    echo "$FETCHED_WARNING"
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
  record_source "$core" "$FETCHED_URL" "$FETCHED_SHA" "$FETCHED_STATE" "$(pinned_sha256 "$core")" "$dylib_sha"
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
  forget_source "prboom.wad"
  echo "==> prboom.wad (DOOM support file, GPL): $PRBOOM_WAD_URL"
  if fetch_pinned "prboom.wad" "prboom.wad" "$PRBOOM_WAD_URL" "$wad" && [[ -s "$wad" ]]; then
    if [[ -n "$FETCHED_WARNING" ]]; then
      echo "$FETCHED_WARNING"
    fi
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
  pins) print_pins ;;
  fetch) fetch_all ;;
  -h|--help|help)
    sed -n '2,9p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
    ;;
  *)
    echo "error: unknown subcommand '$1' (use: names, systems, pins, fetch)" >&2
    exit 1
    ;;
esac
