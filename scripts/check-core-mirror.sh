#!/usr/bin/env bash
# Checks how scripts/fetch-buildbot-cores.sh picks the file for each downloaded core: the frozen
# copy on the core-mirror-1 pre-release, or the original download (FROZEN BY CHECKSUM there).
#
#   scripts/check-core-mirror.sh         the rules, on made-up files: no network, any host
#   scripts/check-core-mirror.sh live    every core and prboom.wad, from the real mirror, buildbot
#                                        and shipped install, comes out as the pinned file, and
#                                        says where from (downloads up to about 230 MB)
#
# Why it exists: on 14 October 2026 the mirror was emptied by mistake. The pinned zips could not
# be put back, only the dylibs in them, so the mirror now accepts a copy by the dylib inside it
# as well as by the zip's own bytes, and a core the buildbot no longer serves at its pin is taken
# from the last shipped install. These checks hold those rules, and the fall-backs around them.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# Functions only: the script runs nothing when it is sourced.
# shellcheck source=fetch-buildbot-cores.sh
source "$ROOT/scripts/fetch-buildbot-cores.sh"

PASS=0
FAIL=0
pass() { PASS=$((PASS + 1)); echo "  ok    $1"; }
fail() { FAIL=$((FAIL + 1)); echo "  FAIL  $1"; }
expect() {
  # expect <what> <got> <want>
  if [[ "$2" == "$3" ]]; then pass "$1"; else fail "$1 (got '$2', want '$3')"; fi
}

finish() {
  echo
  echo "$PASS passed, $FAIL failed"
  [[ "$FAIL" -eq 0 ]]
}

live_checks() {
  local entry core dylib from
  # Global, not local: the EXIT trap runs after this function has returned.
  tmp="$(mktemp -d "${TMPDIR:-/tmp}/continuum-mirror.XXXXXX")"
  trap 'rm -rf "$tmp"' EXIT
  WORK="$tmp/work"
  echo "==> what a build gets for each downloaded core"
  for entry in "${BUILDBOT_CORES[@]}"; do
    core="${entry%% *}"
    dylib="$(dylib_for "$core")"
    fetch_pinned "$core" "$core.zip" "$BUILDBOT/$dylib.zip" "$tmp/$core.zip" "$dylib" >/dev/null || true
    case "$FETCHED_URL" in
      "$MIRROR"/*) from="the mirror" ;;
      "$SHIPPED_IPA_URL") from="$SHIPPED_TAG's install" ;;
      "$BUILDBOT"/*) from="the buildbot" ;;
      *) from="nowhere" ;;
    esac
    # A re-zip at the buildbot is the pinned build too: fetch_one accepts it by its dylib.
    if [[ "$FETCHED_STATE" == "unpinned" && -f "$tmp/$core.zip" \
        && "$(zip_dylib_sha256 "$tmp/$core.zip" "$dylib")" == "$(pinned_dylib_sha256 "$core")" ]]; then
      FETCHED_STATE="pinned"
    fi
    if [[ "$FETCHED_STATE" == "pinned" ]]; then
      pass "$core: the pinned build, from $from"
    else
      fail "$core: no pinned build anywhere (got ${FETCHED_STATE:-nothing} from $from)"
    fi
    rm -f "$tmp/$core.zip"
  done
  fetch_pinned "prboom.wad" "prboom.wad" "$PRBOOM_WAD_URL" "$tmp/prboom.wad" >/dev/null || true
  if [[ "$FETCHED_STATE" == "pinned" ]]; then
    pass "prboom.wad: the pinned file, from ${FETCHED_URL%%/prboom.wad}"
  else
    fail "prboom.wad: not the pinned file anywhere"
  fi
  finish
}

if [[ "${1:-}" == "live" ]]; then
  live_checks
  exit $?
fi

# ---- The rules, on made-up files ----------------------------------------------------------------

T="$(mktemp -d "${TMPDIR:-/tmp}/continuum-mirror-check.XXXXXX")"
trap 'rm -rf "$T"' EXIT
mkdir -p "$T/mirror" "$T/upstream" "$T/src/a" "$T/src/b" "$T/src/c"

# The mirror and the original are folders here: curl reads file:// URLs like any other.
MIRROR="file://$T/mirror"
DYLIB="testcore_libretro_ios.dylib"

# Two different "builds" of one core, and three zips: the pinned zip, a re-zip of the same build
# (another date, another order, so different bytes), and a zip of the other build.
printf 'pinned build of testcore\n' > "$T/src/a/$DYLIB"
printf 'some other build of testcore\n' > "$T/src/b/$DYLIB"
(cd "$T/src/a" && zip -qX "$T/pinned.zip" "$DYLIB")
printf 'readme\n' > "$T/src/c/README.txt"
cp "$T/src/a/$DYLIB" "$T/src/c/$DYLIB"
(cd "$T/src/c" && zip -q "$T/rezipped.zip" README.txt "$DYLIB")
(cd "$T/src/b" && zip -qX "$T/other.zip" "$DYLIB")
# A zip whose dylib sits in a folder, as some buildbot zips are laid out.
mkdir -p "$T/src/d/inner"
cp "$T/src/a/$DYLIB" "$T/src/d/inner/$DYLIB"
(cd "$T/src/d" && zip -qr "$T/nested.zip" inner)
printf 'not a zip\n' > "$T/broken.zip"

ZIP_PIN="$(sha256_of "$T/pinned.zip")"
DYLIB_PIN="$(sha256_of "$T/src/a/$DYLIB")"
printf 'the pinned wad\n' > "$T/pinned.wad"
WAD_PIN="$(sha256_of "$T/pinned.wad")"
printf 'a changed wad\n' > "$T/other.wad"

pinned_sha256() {
  case "$1" in
    testcore) echo "$ZIP_PIN" ;;
    test.wad) echo "$WAD_PIN" ;;
    *) echo "" ;;
  esac
}
pinned_dylib_sha256() {
  case "$1" in
    testcore) echo "$DYLIB_PIN" ;;
    *) echo "" ;;
  esac
}

# mirror_has <zip or nothing>; upstream_has <zip or nothing>
mirror_has() { rm -f "$T/mirror/testcore.zip"; [[ -z "${1:-}" ]] || cp "$1" "$T/mirror/testcore.zip"; }
upstream_has() { rm -f "$T/upstream/testcore.zip"; [[ -z "${1:-}" ]] || cp "$1" "$T/upstream/testcore.zip"; }
UP="file://$T/upstream/testcore.zip"
DEST="$T/got/testcore.zip"
mkdir -p "$T/got"

# A made-up shipped install, with no copy of testcore in it until shipped_has puts one there.
SHIPPED_TAG="test-install"
SHIPPED_IPA_URL="file://$T/install.ipa"
WORK="$T/work"
shipped_has() {
  rm -rf "$T/ipa" "$T/install.ipa" "$WORK/shipped"
  mkdir -p "$T/ipa/Payload/Continuum.app/Frameworks"
  [[ -z "${1:-}" ]] || cp "$1" "$T/ipa/Payload/Continuum.app/Frameworks/$DYLIB"
  printf 'x\n' > "$T/ipa/Payload/Continuum.app/Info.plist"
  (cd "$T/ipa" && zip -qr "$T/install.ipa" Payload)
}
shipped_has ""

run() { fetch_pinned testcore testcore.zip "$UP" "$DEST" "$DYLIB" >/dev/null; }

echo "==> zip_dylib_sha256"
expect "finds the dylib by name" "$(zip_dylib_sha256 "$T/pinned.zip" "$DYLIB")" "$DYLIB_PIN"
expect "finds it in a folder inside the zip" "$(zip_dylib_sha256 "$T/nested.zip" "$DYLIB")" "$DYLIB_PIN"
expect "a broken zip gives nothing" "$(zip_dylib_sha256 "$T/broken.zip" "$DYLIB")" ""

echo "==> the mirror's copy"
mirror_has "$T/pinned.zip"; upstream_has "$T/other.zip"
run
expect "the exact pinned zip is taken from the mirror" "$FETCHED_URL $FETCHED_STATE" "$MIRROR/testcore.zip pinned"
expect "  and is not uploaded again" "$FETCHED_UPLOAD" "0"

mirror_has "$T/rezipped.zip"
run
expect "a re-zip of the pinned dylib is taken from the mirror" "$FETCHED_URL $FETCHED_STATE" "$MIRROR/testcore.zip pinned"
expect "  and is recorded with its own zip sha256" "$FETCHED_SHA" "$(sha256_of "$T/rezipped.zip")"
expect "  and is not uploaded again" "$FETCHED_UPLOAD" "0"

mirror_has "$T/nested.zip"
run
expect "a re-zip with the dylib in a folder is taken from the mirror" "$FETCHED_URL $FETCHED_STATE" "$MIRROR/testcore.zip pinned"

mirror_has "$T/other.zip"; upstream_has "$T/pinned.zip"
run
expect "a different dylib on the mirror is refused, and the original is used" "$FETCHED_URL $FETCHED_STATE" "$UP pinned"
expect "  and the original, being the pinned zip, is uploaded" "$FETCHED_UPLOAD" "1"

mirror_has "$T/broken.zip"
run
expect "a broken copy on the mirror is refused, and the original is used" "$FETCHED_URL" "$UP"

mirror_has ""; upstream_has "$T/pinned.zip"
run
expect "no copy on the mirror: the original is used" "$FETCHED_URL $FETCHED_STATE" "$UP pinned"

mirror_has ""; upstream_has "$T/other.zip"
run
expect "no copy and a changed original: used anyway, as unpinned" "$FETCHED_URL $FETCHED_STATE" "$UP unpinned"
if [[ "$FETCHED_WARNING" == "::warning::testcore changed upstream"* ]]; then pass "  with a warning"; else fail "  with a warning (got '$FETCHED_WARNING')"; fi

mirror_has ""; upstream_has ""
if run; then fail "nothing anywhere: fetch_pinned fails"; else pass "nothing anywhere: fetch_pinned fails"; fi

mirror_has "$T/rezipped.zip"; upstream_has "$T/other.zip"
fetch_pinned testcore testcore.zip "$UP" "$DEST" >/dev/null || true
expect "without the dylib's name, only the zip's own bytes count" "$FETCHED_URL" "$UP"

echo "==> the last shipped install"
shipped_has "$T/src/a/$DYLIB"
mirror_has ""; upstream_has "$T/other.zip"
run
expect "no copy on the mirror, a newer build upstream: the shipped install's copy is used" "$FETCHED_URL $FETCHED_STATE" "$SHIPPED_IPA_URL pinned"
expect "  and it is the pinned dylib" "$(zip_dylib_sha256 "$DEST" "$DYLIB")" "$DYLIB_PIN"
expect "  and is uploaded, so the mirror refills itself" "$FETCHED_UPLOAD" "1"

mirror_has ""; upstream_has ""
run
expect "no copy on the mirror, nothing upstream: the shipped install's copy is used" "$FETCHED_URL $FETCHED_STATE" "$SHIPPED_IPA_URL pinned"

mirror_has ""; upstream_has "$T/rezipped.zip"; rm -rf "$WORK/shipped"
run
expect "the buildbot only re-zipped the pinned build: the install is not needed" "$FETCHED_URL" "$UP"
if [[ -e "$WORK/shipped/$SHIPPED_TAG.ipa" ]]; then fail "  and is not downloaded"; else pass "  and is not downloaded"; fi

mirror_has ""; upstream_has "$T/pinned.zip"; rm -rf "$WORK/shipped"
run
expect "the original is the pinned zip: used, and the install is not needed" "$FETCHED_URL" "$UP"
if [[ -e "$WORK/shipped/$SHIPPED_TAG.ipa" ]]; then fail "  and is not downloaded"; else pass "  and is not downloaded"; fi

mirror_has "$T/rezipped.zip"; upstream_has "$T/other.zip"; rm -rf "$WORK/shipped"
run
expect "an accepted mirror copy wins over the install" "$FETCHED_URL" "$MIRROR/testcore.zip"

shipped_has "$T/src/b/$DYLIB"
mirror_has ""; upstream_has "$T/other.zip"
run
expect "the install holds a different build: refused, the original is used as unpinned" "$FETCHED_URL $FETCHED_STATE" "$UP unpinned"

shipped_has ""
mirror_has ""; upstream_has "$T/other.zip"
run
expect "the install has no copy at all: the original is used as unpinned" "$FETCHED_URL $FETCHED_STATE" "$UP unpinned"

rm -f "$T/install.ipa"; rm -rf "$WORK/shipped"; SHIPPED_FAILED=""
mirror_has ""; upstream_has "$T/other.zip"
run
expect "the install can't be downloaded: the original is used as unpinned" "$FETCHED_URL $FETCHED_STATE" "$UP unpinned"
upstream_has ""
if run; then fail "  and with no original either, fetch_pinned fails"; else pass "  and with no original either, fetch_pinned fails"; fi
SHIPPED_FAILED=""
shipped_has ""

echo "==> prboom.wad-style files (no dylib inside)"
WUP="file://$T/upstream/test.wad"
cp "$T/pinned.wad" "$T/mirror/test.wad"; cp "$T/other.wad" "$T/upstream/test.wad"
fetch_pinned test.wad test.wad "$WUP" "$T/got/test.wad" >/dev/null
expect "the exact file is taken from the mirror" "$FETCHED_URL $FETCHED_STATE" "$MIRROR/test.wad pinned"
cp "$T/other.wad" "$T/mirror/test.wad"; cp "$T/pinned.wad" "$T/upstream/test.wad"
fetch_pinned test.wad test.wad "$WUP" "$T/got/test.wad" >/dev/null
expect "a changed file on the mirror is refused" "$FETCHED_URL $FETCHED_STATE $FETCHED_UPLOAD" "$WUP pinned 1"

echo "==> fetch_one, end to end (the Mach-O and staging checks stubbed out)"
# The real checks need an iOS dylib; they are the same for every source and are not what this
# script is about. Everything else in fetch_one runs as it does in CI, where it is called as an
# `if` condition (so `|| true` here).
OUT_DIR="$T/lib"
WORK="$T/work"
BUILDBOT="file://$T/upstream"
mkdir -p "$OUT_DIR" "$WORK"
check_macho() { return 0; }
cat > "$T/stage.sh" <<'EOF'
#!/usr/bin/env bash
# ios-stage-prebuilt <dylib> <name>
cp "$2" "$OUT_DIR/$3"
EOF
chmod +x "$T/stage.sh"
export OUT_DIR
BUILD_CORE="$T/stage.sh"
UPLOADED=""
mirror_upload() { UPLOADED="$UPLOADED $(basename "$1")"; }
# fetch_one asks the buildbot for <core>_libretro_ios.dylib.zip.
bb_has() { rm -f "$T/upstream/$DYLIB.zip"; [[ -z "${1:-}" ]] || cp "$1" "$T/upstream/$DYLIB.zip"; }

mirror_has ""; bb_has "$T/rezipped.zip"; UPLOADED=""
fetch_one testcore Test >"$T/out.txt" 2>&1 || true
line="$(grep '^testcore ' "$OUT_DIR/core-sources.txt" || true)"
expect "empty mirror, buildbot re-zipped the pinned dylib: recorded as pinned" "$(awk '{ print $4 }' <<<"$line")" "pinned"
expect "  and uploaded, so the mirror refills itself" "$UPLOADED" " testcore.zip"
expect "  and staged" "$(sha256_of "$OUT_DIR/$DYLIB")" "$DYLIB_PIN"

mirror_has "$T/rezipped.zip"; bb_has "$T/other.zip"; UPLOADED=""
fetch_one testcore Test >"$T/out.txt" 2>&1 || true
line="$(grep '^testcore ' "$OUT_DIR/core-sources.txt" || true)"
expect "re-zipped copy on the mirror, newer build at the buildbot: the mirror's is staged" "$(sha256_of "$OUT_DIR/$DYLIB")" "$DYLIB_PIN"
expect "  recorded as pinned, from the mirror" "$(awk '{ print $2, $4 }' <<<"$line")" "$MIRROR/testcore.zip pinned"
expect "  and nothing uploaded" "$UPLOADED" ""

mirror_has ""; bb_has "$T/other.zip"; UPLOADED=""
fetch_one testcore Test >"$T/out.txt" 2>&1 || true
line="$(grep '^testcore ' "$OUT_DIR/core-sources.txt" || true)"
expect "empty mirror, newer build at the buildbot: kept, as unpinned" "$(awk '{ print $4 }' <<<"$line")" "unpinned"
if grep -q "::warning::testcore changed upstream" "$T/out.txt"; then pass "  with a warning"; else fail "  with a warning"; fi
expect "  and nothing uploaded" "$UPLOADED" ""

finish
