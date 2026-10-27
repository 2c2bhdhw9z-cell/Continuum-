#!/usr/bin/env bash
# Tests the per-core iOS build cache in scripts/build-core.sh on any host (Linux in CI).
#
# WHY THIS EXISTS. CI keeps the built core dylibs between runs and reuses one only when its line
# in core-sources.txt names the core's pin and its fingerprint as it is now (ios_core_is_cached).
# The fingerprint is what decides which cores rebuild, so if it missed an input, a changed patch
# or recipe would ship the OLD dylib with nothing saying so. That is the one failure this cache
# must never have. These checks change each input on a throwaway copy of the scripts and assert
# that exactly the right fingerprints move, and that ios-is-cached says no whenever in doubt.
#
# SINCE THE PER-CORE SPLIT the fingerprint hashes build-core.sh in TWO parts: the SHARED part
# (everything outside the `# recipe-start:` regions) and the core's OWN region. So there are two
# more things to prove, and sections 9-12 do: that the regions really hold what they claim, that
# a shared edit still moves EVERY core while a one-core edit moves only that core (and its JIT
# twin), and that a broken or missing region rebuilds instead of reusing a stale dylib.
#
# SINCE 14 OCTOBER ("WHAT COUNTS" in build-core.sh) comments, blank lines, the cache's own code and
# the lists of cores count for nothing. Section 13 proves both halves of that: that editing them
# moves no fingerprint, and that everything bash would really run or write - a here-document line,
# a line inside a string that spans lines, a line after a continuation - still moves the right
# ones; that anything the cache cannot read for certain gives no core a fingerprint; and that
# breaking any of its rules does too, with the reason named. It also keeps the cache region's
# allow-list (CACHE_FUNCTIONS), which must match build-core.sh's IOS_CACHE_FUNCTIONS.
#
# Section 14 covers the review of 393f12c: a core's patch step cannot sit outside its own region
# (patches arms, patch helpers, the patch files a region names, and functions only their own family
# may use), and the regions of one family are told apart in its hash.
#
# Section 15 is the hostile review (K3): ways a line the scan calls a comment is really run by bash
# (hidden after a line continuation, or after a ( inside [[ ]]), a build step that reads the script
# path through $0, and three more ways to apply another family's patch unseen (a continuation line
# chaining a second step, a patch helper, and a $VAR path to the patches directory). Each must fail
# closed so no core gets a fingerprint, and say why.
#
# Touches nothing in this checkout: everything happens in a temporary copy that is removed at
# the end. Bash 3.2 safe (macOS) as well as Linux.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# The checks set these themselves; one left in the caller's environment must not change them.
unset CONTINUUM_CORE_CACHE CONTINUUM_UNPINNED CONTINUUM_CORE_TOOLCHAIN

W="$(mktemp -d)"
cleanup() { rm -rf "$W"; }
trap cleanup EXIT

# The copy: the script, its patches, the headers script and Continuum Symbian's tracked files,
# committed in a git repository of its own so ios_core_fingerprint lists Symbian's files the way
# it does in a real checkout.
mkdir -p "$W/scripts" "$W/native"
cp -p "$ROOT/scripts/build-core.sh" "$ROOT/scripts/fetch-libretro-headers.sh" "$W/scripts/"
cp -Rp "$ROOT/scripts/patches" "$W/scripts/patches"
symbian_files="$(git -C "$ROOT" ls-files -- native/continuum-symbian)"
[[ -n "$symbian_files" ]] || { echo "error: no tracked files under native/continuum-symbian" >&2; exit 1; }
while IFS= read -r f; do
  mkdir -p "$W/$(dirname "$f")"
  cp -p "$ROOT/$f" "$W/$f"
done <<<"$symbian_files"
( cd "$W" && git -c init.defaultBranch=main init -q && git add -A \
    && git -c user.name=t -c user.email=t@t commit -qm base )

BC="$W/scripts/build-core.sh"
passed=0
failed=0

ok() { passed=$((passed + 1)); echo "  ok    $1"; }
bad() { failed=$((failed + 1)); echo "  FAIL  $1"; }

# check "<what>" <command...>: one counted check.
check() {
  local what="$1"
  shift
  if "$@"; then ok "$what"; else bad "$what"; fi
}

fp() { bash "$BC" ios-fingerprint "$1"; }
same() { [[ -n "$1" && "$1" == "$2" ]]; }
differ() { [[ -n "$1" && -n "$2" && "$1" != "$2" ]]; }
is_cached() { bash "$BC" ios-is-cached "$1" >/dev/null 2>&1; }
not_cached() { ! is_cached "$1"; }
pin_of() { bash "$BC" ios-pins | awk -F'\t' -v c="$1" '$1 == c { print $3 }'; }
repo_of() { bash "$BC" ios-pins | awk -F'\t' -v c="$1" '$1 == c { print $2 }'; }

# ---- helpers for the per-core region checks (sections 9-12) ----

# A core's own recipe region, and the part of the script every core shares. Empty on failure,
# which is what the fail-closed checks want to see.
slice() { bash "$BC" ios-recipe-slice "$1" 2>/dev/null; }
shared_text() { bash "$BC" ios-recipe-shared 2>/dev/null; }
contains() { [[ -n "$1" && "$1" == *"$2"* ]]; }
lacks() { [[ -n "$1" && "$1" != *"$2"* ]]; }
has_fp() { bash "$BC" ios-fingerprint "$1" >/dev/null 2>&1; }
no_fp() { ! has_fp "$1"; }
# How many lines of $BC are exactly $1. The tripwires below use it on IOS_KIND= declarations.
line_count() { grep -cxF "$1" "$BC"; }
# How many lines of the text in $1 contain $2.
count_in() { printf '%s\n' "$1" | grep -cF "$2"; }

all_cores() {
  local dylib
  for dylib in $(bash "$BC" ios-names) $(bash "$BC" ios-optional-names); do
    echo "${dylib%_libretro_ios.dylib}"
  done
}
all_cores_sorted() { all_cores | LC_ALL=C sort | tr '\n' ' ' | sed 's/ $//'; }

# One "<core> <fingerprint>" line per core, into file $1. A core whose fingerprint cannot be
# worked out is written as `none`, so it compares equal to itself and a check that expects it to
# MOVE cannot pass by accident. All cores come from one reading of the script
# (ios-cache-fingerprints); agree() below checks that gives what ios-fingerprint gives, core by core.
snapshot() {
  local core
  bash "$BC" ios-cache-fingerprints > "$1" 2>/dev/null || : > "$1"
  # A core the command did not print (it crashed, say) is `none`, never simply absent.
  for core in $(all_cores); do
    awk -v c="$core" '$1 == c { found = 1 } END { exit !found }' "$1" || echo "$core none" >> "$1"
  done
}
# Every core's ios-fingerprint, one process each (the path a build takes), equals the snapshot's.
agree() {
  local core want got
  snapshot "$W/fp-agree"
  [[ -s "$W/fp-agree" ]] || return 1
  for core in $(all_cores); do
    want="$(bash "$BC" ios-fingerprint "$core" 2>/dev/null || echo none)"
    got="$(awk -v c="$core" '$1 == c { print $2 }' "$W/fp-agree")"
    [[ -n "$got" && "$got" == "$want" ]] || { echo "        ($core: $got from one reading, $want on its own)"; return 1; }
  done
}

# The sorted, space-separated core names whose fingerprint differs between snapshots $1 and $2.
# A core that has NO fingerprint in $2 is printed as <core>(none): losing a fingerprint is not a
# move, so no check that expects a list of moved cores can pass because of it.
moved() {
  awk 'NR == FNR { was[$1] = $2; next }
       $2 == "none" { print $1 "(none)"; next }
       !($1 in was) || was[$1] != $2 { print $1 }' "$1" "$2" \
    | LC_ALL=C sort | tr '\n' ' ' | sed 's/ $//'
}

# Puts the real script back, so each destructive case starts from a known copy.
reset_copy() { cp -p "$ROOT/scripts/build-core.sh" "$BC"; }

# Replaces the one line of the copy that is exactly $1 with $2, and FAILS if that line is not
# there. Every edit below goes through this (or drop_lines) so a check can never quietly pass by
# editing nothing: if an anchor is renamed upstream, the check goes red instead of green. It has
# already earned that: two anchors here end in a line-continuation `\`, and the first version
# passed them through `awk -v`, which processes backslash escapes and ate it.
#
# So the text goes through the ENVIRONMENT, not -v: ENVIRON values are taken literally. And awk,
# not `sed -i 'a\'`, because GNU and BSD sed spell that differently.
replace_line() {
  CCC_FROM="$1" CCC_TO="$2" awk '
    !done && $0 == ENVIRON["CCC_FROM"] { print ENVIRON["CCC_TO"]; done = 1; next }
    { print }
    END { exit !done }
  ' "$BC" > "$BC.new" && mv "$BC.new" "$BC"
}

# Deletes every line of the copy containing $1, and fails if there was none.
drop_lines() {
  CCC_PAT="$1" awk '
    index($0, ENVIRON["CCC_PAT"]) { dropped = 1; next }
    { print }
    END { exit !dropped }
  ' "$BC" > "$BC.new" && mv "$BC.new" "$BC"
}

# ---- helpers for the WHAT COUNTS checks (section 13 on) ----

# Inserts the text $2 (one or more lines) before, or after, the first line of the copy that is
# exactly $1, and fails if there is no such line. Through ENVIRON for the same reason as above.
insert_before() {
  CCC_AT="$1" CCC_TEXT="$2" awk '
    !done && $0 == ENVIRON["CCC_AT"] { print ENVIRON["CCC_TEXT"]; done = 1 }
    { print }
    END { exit !done }
  ' "$BC" > "$BC.new" && mv "$BC.new" "$BC"
}
insert_after() {
  CCC_AT="$1" CCC_TEXT="$2" awk '
    { print }
    !done && $0 == ENVIRON["CCC_AT"] { print ENVIRON["CCC_TEXT"]; done = 1 }
    END { exit !done }
  ' "$BC" > "$BC.new" && mv "$BC.new" "$BC"
}
# Deletes the first line of the copy that is exactly $1 (the empty string means the first blank
# line), and fails if there is none.
delete_line() {
  CCC_AT="$1" awk '
    !done && $0 == ENVIRON["CCC_AT"] { done = 1; next }
    { print }
    END { exit !done }
  ' "$BC" > "$BC.new" && mv "$BC.new" "$BC"
}
# The Nth line of the copy that is exactly $1: deleted, replaced by $3, or followed by $3. Each fails
# when there is no Nth such line.
nth_edit() {
  CCC_AT="$1" CCC_N="$2" CCC_TEXT="${4:-}" CCC_OP="$3" awk '
    $0 == ENVIRON["CCC_AT"] { seen++ }
    $0 == ENVIRON["CCC_AT"] && seen == ENVIRON["CCC_N"] + 0 {
      done = 1
      if (ENVIRON["CCC_OP"] == "delete") next
      if (ENVIRON["CCC_OP"] == "replace") { print ENVIRON["CCC_TEXT"]; next }
      print; print ENVIRON["CCC_TEXT"]; next
    }
    { print }
    END { exit !done }
  ' "$BC" > "$BC.new" && mv "$BC.new" "$BC"
}
delete_nth() { nth_edit "$1" "$2" delete; }
replace_nth() { nth_edit "$1" "$2" replace "$3"; }
insert_after_nth() { nth_edit "$1" "$2" after "$3"; }
# Moves the lines from the first line that is exactly $1 to the next line that is exactly $2 (both
# included) to just before the first line that is exactly $3.
move_block() {
  CCC_A="$1" CCC_B="$2" CCC_AT="$3" awk '
    { line[NR] = $0 }
    END {
      for (n = 1; n <= NR; n++) if (!s && line[n] == ENVIRON["CCC_A"]) s = n
      for (n = s; s && n <= NR; n++) if (line[n] == ENVIRON["CCC_B"]) { e = n; break }
      for (n = 1; n <= NR; n++) if ((n < s || n > e) && line[n] == ENVIRON["CCC_AT"]) { at = n; break }
      if (!s || !e || !at) exit 1
      for (n = 1; n <= NR; n++) {
        if (n == at) for (m = s; m <= e; m++) print line[m]
        if (n < s || n > e) print line[n]
      }
    }
  ' "$BC" > "$BC.new" && mv "$BC.new" "$BC"
}
# The script's own verdict on its rules: ios-cache-check passes, or fails with a message that
# contains $1 (so a check cannot pass on some other, unrelated failure).
rules_hold() { bash "$BC" ios-cache-check >/dev/null 2>&1; }
rules_fail_saying() {
  local out
  if out="$(bash "$BC" ios-cache-check 2>&1)"; then return 1; fi
  [[ "$out" == *"$1"* ]] || { echo "        (said: $out)"; return 1; }
}
# Snapshots $W/fp-base, applies the edit "$@" (which must succeed), snapshots $W/fp-after, and
# prints the cores whose fingerprint moved. "EDIT-FAILED" when the edit did not land.
moves_after() {
  snapshot "$W/fp-base"
  "$@" || { echo "EDIT-FAILED"; return 0; }
  snapshot "$W/fp-after"
  moved "$W/fp-base" "$W/fp-after"
}
nothing_moves() { same "$(moves_after "$@")x" "x"; }
everything_moves() { same "$(moves_after "$@")" "$(all_cores_sorted)"; }
# Like moves_after, for an edit that has to be set up first: the snapshot is taken after $1.
moves_after_setup() {
  local setup="$1"
  shift
  eval "$setup" || { echo "SETUP-FAILED"; return 0; }
  moves_after "$@"
}
# No core at all gets a fingerprint: three different kinds checked, as the failure is global.
no_core_has_fp() { no_fp fceumm && no_fp flycast_jit && no_fp continuum_symbian; }

echo "==> core cache checks (copy in $W)"

# 1. Stable.
fceumm_a="$(fp fceumm)"
fceumm_b="$(fp fceumm)"
check "fceumm's fingerprint is the same on two runs" same "$fceumm_a" "$fceumm_b"
check "a fingerprint is 16 hex" eval '[[ "$fceumm_a" =~ ^[0-9a-f]{16}$ ]]'

# 2. Moving one core's pin moves only that core.
gear_before="$(fp gearsystem)"
gear_pin="$(pin_of gearsystem)"
check "gearsystem has a pin to move" eval '[[ ${#gear_pin} -eq 40 ]]'
sed -i.bak "s/IOS_PIN=\"$gear_pin\"/IOS_PIN=\"1111111111111111111111111111111111111111\"/" "$BC"
rm -f "$BC.bak"
check "gearsystem's pin really moved in the copy" eval '[[ "$(pin_of gearsystem)" == 1111111111111111111111111111111111111111 ]]'
check "moving gearsystem's pin leaves fceumm alone" same "$fceumm_a" "$(fp fceumm)"
check "moving gearsystem's pin changes gearsystem" differ "$gear_before" "$(fp gearsystem)"

# 3. A patch moves only its family.
n64_before="$(fp parallel_n64)"
n64jit_before="$(fp parallel_n64_jit)"
azahar_before="$(fp azahar)"
echo "# a changed patch" >> "$W/scripts/patches/parallel_n64-reapply-variables-after-initiate-gfx.patch"
check "a parallel_n64 patch changes parallel_n64" differ "$n64_before" "$(fp parallel_n64)"
check "a parallel_n64 patch changes parallel_n64_jit" differ "$n64jit_before" "$(fp parallel_n64_jit)"
check "a parallel_n64 patch leaves fceumm alone" same "$fceumm_a" "$(fp fceumm)"
check "a parallel_n64 patch leaves azahar alone" same "$azahar_before" "$(fp azahar)"

# 4. An edit to the SHARED part of the recipe moves every core. (Appending at the end of the file
# lands after the last recipe region, so this is a shared edit; section 10 does the per-core ones.)
# A line of CODE, not a comment: since 14 October a comment counts for nothing (section 13 proves
# that), so this appends a command bash would run, which is what this check has always meant.
echo ": a shared recipe change" >> "$BC"
fceumm_c="$(fp fceumm)"
check "a shared build-core.sh edit changes fceumm" differ "$fceumm_a" "$fceumm_c"
check "a shared build-core.sh edit changes azahar" differ "$azahar_before" "$(fp azahar)"

# 5. The toolchain.
check "CONTINUUM_CORE_TOOLCHAIN changes fceumm" \
  differ "$fceumm_c" "$(CONTINUUM_CORE_TOOLCHAIN=x bash "$BC" ios-fingerprint fceumm)"

# 6. Continuum Symbian's plug moves only Symbian.
symbian_before="$(fp continuum_symbian)"
first_symbian="$(printf '%s\n' "$symbian_files" | head -1)"
echo "// a plug change" >> "$W/$first_symbian"
check "a native/continuum-symbian edit changes continuum_symbian" \
  differ "$symbian_before" "$(fp continuum_symbian)"
check "a native/continuum-symbian edit leaves fceumm alone" same "$fceumm_c" "$(fp fceumm)"

# 7. ios-is-cached: yes only for the exact line, the dylib, CONTINUUM_CORE_CACHE=1 and a pin.
LIB="$W/native/ios/build/lib"
MANIFEST="$LIB/core-sources.txt"
mkdir -p "$LIB"
fceumm_fp="$(fp fceumm)"
fceumm_line="fceumm $(repo_of fceumm) $(pin_of fceumm) pinned"
echo "not a real dylib" > "$LIB/fceumm_libretro_ios.dylib"
echo "$fceumm_line fp:$fceumm_fp" > "$MANIFEST"
export CONTINUUM_CORE_CACHE=1
check "fceumm is reused with its dylib and its exact line" is_cached fceumm
echo "$fceumm_line" > "$MANIFEST"
check "an old 4-field line is never reused" not_cached fceumm
echo "$fceumm_line fp:0000000000000000" > "$MANIFEST"
check "a different fingerprint is never reused" not_cached fceumm
echo "$fceumm_line fp:$fceumm_fp" > "$MANIFEST"
rm -f "$LIB/fceumm_libretro_ios.dylib"
check "a missing dylib is never reused" not_cached fceumm
echo "not a real dylib" > "$LIB/fceumm_libretro_ios.dylib"
check "fceumm is reused again once the dylib is back" is_cached fceumm
check "nothing is reused under CONTINUUM_UNPINNED=1" eval '! CONTINUUM_UNPINNED=1 bash "$BC" ios-is-cached fceumm >/dev/null 2>&1'
unset CONTINUUM_CORE_CACHE
check "nothing is reused without CONTINUUM_CORE_CACHE=1" not_cached fceumm
export CONTINUUM_CORE_CACHE=1
echo "not a real dylib" > "$LIB/continuum_gb_libretro_ios.dylib"
echo "continuum_gb   pinned fp:$(fp continuum_gb)" >> "$MANIFEST"
check "cBoy (continuum_gb, no pin) is never reused" not_cached continuum_gb
echo "not a real dylib" > "$LIB/continuum_arcade_libretro_ios.dylib"
echo "continuum_arcade   pinned fp:$(fp continuum_arcade)" >> "$MANIFEST"
check "cArcade (continuum_arcade, no pin) is never reused" not_cached continuum_arcade
echo "not a real dylib" > "$LIB/continuum_gc_libretro_ios.dylib"
echo "continuum_gc   pinned fp:$(fp continuum_gc)" >> "$MANIFEST"
check "the GameCube core (continuum_gc, no pin) is never reused" not_cached continuum_gc
echo "not a real dylib" > "$LIB/continuum_sms_libretro_ios.dylib"
echo "continuum_sms   pinned fp:$(fp continuum_sms)" >> "$MANIFEST"
check "cGear (continuum_sms, no pin) is never reused" not_cached continuum_sms
echo "not a real dylib" > "$LIB/continuum_atari_libretro_ios.dylib"
echo "continuum_atari   pinned fp:$(fp continuum_atari)" >> "$MANIFEST"
check "cAtari (continuum_atari, no pin) is never reused" not_cached continuum_atari
unset CONTINUUM_CORE_CACHE

# 8. Every patch belongs to a core, so every patch is in some core's fingerprint.
families=""
for dylib in $(bash "$BC" ios-names) $(bash "$BC" ios-optional-names); do
  core="${dylib%_libretro_ios.dylib}"
  family="${core%_jit}"
  case "$family" in continuum_symbian) family="continuum-symbian" ;; esac
  families="$families $family"
done
for patch in "$ROOT"/scripts/patches/*.patch; do
  name="$(basename "$patch")"
  matched=0
  for family in $families; do
    case "$name" in "$family"-*) matched=1 ;; esac
  done
  check "$name belongs to a core" eval "[[ $matched -eq 1 ]]"
done

# 9. The marked regions really are each core's own recipe.
#
# The fingerprint trusts the markers, so this section proves the markers are where they claim to
# be. From here on the copy is reset to the real script first, so sections 9-12 do not depend on
# what 1-8 changed.
reset_copy

for core in $(all_cores); do
  core_slice="$(slice "$core")"
  check "$core's recipe region is the one that names its dylib" \
    contains "$core_slice" "IOS_DYLIB_NAME=\"${core}_libretro_ios.dylib\""
done

# Exactly one IOS_DYLIB_NAME= is left outside the regions: the empty default in ios_core_config's
# preamble. Any more would mean a core's config arm was never marked, and that core would be
# judged on somebody else's recipe.
shared_now="$(shared_text)"
check "every core's config arm is inside a region" same "$(count_in "$shared_now" 'IOS_DYLIB_NAME="')" "1"

# A build function only one family uses is part of that family's recipe.
check "build_ios_cmake_core is in mgba's recipe" contains "$(slice mgba)" "build_ios_cmake_core() {"
check "build_ios_cmake_shared_core is in azahar's recipe" \
  contains "$(slice azahar)" "build_ios_cmake_shared_core() {"
check "build_ios_ppsspp_core is in ppsspp's recipe" contains "$(slice ppsspp)" "build_ios_ppsspp_core() {"
check "build_ios_flycast_core is in flycast's recipe" \
  contains "$(slice flycast)" "build_ios_flycast_core() {"
check "build_ios_flycast_core is in flycast_jit's recipe too" \
  contains "$(slice flycast_jit)" "build_ios_flycast_core() {"
check "build_ios_continuum_symbian_core is in continuum_symbian's recipe" \
  contains "$(slice continuum_symbian)" "build_ios_continuum_symbian_core() {"
check "build_ios_continuum_gb_core is in continuum_gb's recipe" \
  contains "$(slice continuum_gb)" "build_ios_continuum_gb_core() {"
check "build_ios_continuum_arcade_core is in continuum_arcade's recipe" \
  contains "$(slice continuum_arcade)" "build_ios_continuum_arcade_core() {"
check "build_ios_continuum_sms_core is in continuum_sms's recipe" \
  contains "$(slice continuum_sms)" "build_ios_continuum_sms_core() {"
check "build_ios_continuum_atari_core is in continuum_atari's recipe" \
  contains "$(slice continuum_atari)" "build_ios_continuum_atari_core() {"
check "build_ios_continuum_gc_core is in continuum_gc's recipe" \
  contains "$(slice continuum_gc)" "build_ios_continuum_gc_core() {"

# A helper that many cores go through is SHARED, so a change to it rebuilds all of them.
check "build_ios_make_core is in the shared part" contains "$shared_now" "build_ios_make_core() {"
check "build_ios_make_core is not in fceumm's recipe" lacks "$(slice fceumm)" "build_ios_make_core() {"
check "ios_apply_simple_patch is in the shared part" contains "$shared_now" "ios_apply_simple_patch() {"
check "ios_apply_simple_patch is not in fceumm's recipe" \
  lacks "$(slice fceumm)" "ios_apply_simple_patch() {"

# The 13 October incident in one check: the build tools only the Symbian core downloads are part
# of ITS recipe, so pinning them again costs one core, not all of them.
symbian_slice="$(slice continuum_symbian)"
check "the Symbian core's own pinned ninja is in its recipe" contains "$symbian_slice" "ninja-1.13.2"
check "the Symbian core's own pinned CMake is in its recipe" contains "$symbian_slice" "cmake-3.31.6"

# TRIPWIRES. Each of these build functions is inside one family's region, which is only correct
# while one family uses it. If a second core ever takes the same IOS_KIND, that core's recipe
# would live in another core's region and an edit to it would rebuild the wrong core. If one of
# these goes red: move the function out of the region into the shared part, or split it per core.
check "only one core uses the plain cmake build" same "$(line_count '      IOS_KIND="cmake"')" "1"
check "only one core uses the cmake-shared build" same "$(line_count '      IOS_KIND="cmake-shared"')" "1"
check "only one core uses the cmake-ppsspp build" same "$(line_count '      IOS_KIND="cmake-ppsspp"')" "1"
check "only one core uses the cmake-continuum-symbian build" \
  same "$(line_count '      IOS_KIND="cmake-continuum-symbian"')" "1"
check "only one core uses the cargo-continuum-gb build" \
  same "$(line_count '      IOS_KIND="cargo-continuum-gb"')" "1"
check "only one core uses the cargo-continuum-arcade build" \
  same "$(line_count '      IOS_KIND="cargo-continuum-arcade"')" "1"
check "only one core uses the cargo-continuum-gc build" \
  same "$(line_count '      IOS_KIND="cargo-continuum-gc"')" "1"
# Two, and ONE family: flycast and flycast_jit, which share a source tree and a region name.
check "only the flycast family uses the cmake-flycast build" \
  same "$(line_count '      IOS_KIND="cmake-flycast"')" "2"

# 10. What moves when: the whole point of the split.
snapshot "$W/fp-base"

# A shared change: one line of the staging every single core's dylib goes through.
check "the shared staging edit landed" \
  replace_line '  local canonical="$2"' '  local canonical="$2" # a shared change'
snapshot "$W/fp-shared"
check "editing the shared staging logic rebuilds every core" \
  same "$(moved "$W/fp-base" "$W/fp-shared")" "$(all_cores_sorted)"

# The 13 October incident, the way it would go now: the Symbian core's own pinned build tool.
reset_copy
snapshot "$W/fp-base"
check "the Symbian build-tool edit landed" \
  replace_line '      c99048673aa765960a99cf10c6ddb9f1fad506099ff0a0e137ad8960a88f321b \' \
               '      0000000000000000000000000000000000000000000000000000000000000000 \'
snapshot "$W/fp-symbian"
check "changing the Symbian core's pinned ninja rebuilds Symbian alone" \
  same "$(moved "$W/fp-base" "$W/fp-symbian")" "continuum_symbian"

# One core's own compiler flag, on a core that has a JIT twin: both move, nothing else does.
reset_copy
snapshot "$W/fp-base"
check "the flycast flag edit landed" replace_line '    -DUSE_OPENMP=OFF \' '    -DUSE_OPENMP=ON \'
snapshot "$W/fp-flycast"
check "changing one of flycast's own flags rebuilds flycast and its JIT build only" \
  same "$(moved "$W/fp-base" "$W/fp-flycast")" "flycast flycast_jit"
check "after that edit, alone and all at once still agree" agree

# And one core with no twin.
reset_copy
snapshot "$W/fp-base"
fceumm_d="$(fp fceumm)"
check "the mgba edit landed" \
  replace_line '      IOS_CMAKE_ARCHIVE="mgba_libretro.a"' '      IOS_CMAKE_ARCHIVE="mgba_other.a"'
snapshot "$W/fp-mgba"
check "changing mgba's own recipe rebuilds mgba alone" \
  same "$(moved "$W/fp-base" "$W/fp-mgba")" "mgba"
check "changing mgba's own recipe leaves fceumm's fingerprint alone" same "$fceumm_d" "$(fp fceumm)"

# 11. The cache DECISION, not just the hash: an unrelated core's recipe change must leave this
# core's staged dylib reusable, which is the minute-saving this whole change is for.
reset_copy
rm -f "$MANIFEST"
for core in fceumm mgba; do
  echo "not a real dylib" > "$LIB/${core}_libretro_ios.dylib"
  echo "$core $(repo_of "$core") $(pin_of "$core") pinned fp:$(fp "$core")" >> "$MANIFEST"
done
export CONTINUUM_CORE_CACHE=1
check "fceumm is reused before any recipe edit" is_cached fceumm
check "mgba is reused before any recipe edit" is_cached mgba
check "the mgba edit landed (cache decision)" \
  replace_line '      IOS_CMAKE_ARCHIVE="mgba_libretro.a"' '      IOS_CMAKE_ARCHIVE="mgba_other.a"'
check "mgba is rebuilt after its own recipe changed" not_cached mgba
check "fceumm is STILL reused after mgba's recipe changed" is_cached fceumm
unset CONTINUUM_CORE_CACHE

# 12. Fail closed. A broken region must rebuild, never reuse. Destructive, so last, with the real
# script put back between cases.
reset_copy
check "the recipe-end deletion landed" drop_lines "# recipe-end: mgba"
check "an unbalanced region gives mgba no fingerprint" no_fp mgba
check "an unbalanced region gives fceumm no fingerprint either" no_fp fceumm

reset_copy
check "the malformed marker landed" \
  replace_line '    # recipe-start: fceumm' '    # recipe-start: fceumm extra'
check "a malformed marker gives fceumm no fingerprint" no_fp fceumm
check "a malformed marker gives mgba no fingerprint" no_fp mgba
check "a malformed marker gives azahar no fingerprint" no_fp azahar

# An unmarked core: the structure still parses, but that core has no region. It must lose its
# fingerprint for ever (and so rebuild every time) rather than match a stale dylib.
reset_copy
check "the stella2023 markers were removed" drop_lines "recipe-start: stella2023"
check "the stella2023 end markers were removed too" drop_lines "recipe-end: stella2023"
check "an unmarked core gets no fingerprint" no_fp stella2023
export CONTINUUM_CORE_CACHE=1
echo "not a real dylib" > "$LIB/stella2023_libretro_ios.dylib"
echo "stella2023 $(repo_of stella2023) $(pin_of stella2023) pinned fp:unknown" >> "$MANIFEST"
check "an unmarked core is never reused" not_cached stella2023
unset CONTINUUM_CORE_CACHE
check "the other cores still work when one core is unmarked" has_fp fceumm

# 13. WHAT COUNTS (14 October): comments, blank lines, the cache's own code and the lists of
# cores count for nothing; everything bash would run or write still counts. See "WHAT COUNTS" in
# scripts/build-core.sh. Every case starts from the real script.
reset_copy
check "the real build-core.sh keeps every rule of WHAT COUNTS (bash agrees too)" rules_hold
check "every core's fingerprint is the same worked out alone or all at once" agree

# The allow-list of the cache region is kept HERE as well as in build-core.sh, so adding a function
# to the cache region takes a deliberate edit in both files, and this check fails until they agree.
CACHE_FUNCTIONS="ios_usage ios_record_source_version ios_core_patch_family ios_recipe_family
ios_cache_scan ios_cache_bash_parse ios_cache_split ios_recipe_text ios_core_fingerprint
ios_core_is_cached ios_cache_command"
want_allow="$(printf '%s\n' $CACHE_FUNCTIONS | LC_ALL=C sort | tr '\n' ' ')"
got_allow="$(bash "$BC" ios-cache-scan allow 2>/dev/null | LC_ALL=C sort | tr '\n' ' ')"
check "build-core.sh's IOS_CACHE_FUNCTIONS is exactly the allow-list kept here" same "$got_allow" "$want_allow"
in_cache="$(bash "$BC" ios-cache-scan table 2>/dev/null \
  | awk -F'\t' '$3 == "x:cache" && $4 != "" { print $4 }' | LC_ALL=C sort -u | tr '\n' ' ')"
check "the cache regions hold exactly the allow-listed functions, all of them" same "$in_cache" "$want_allow"
listed_cores="$(bash "$BC" ios-cache-scan cores 2>/dev/null | LC_ALL=C sort | tr '\n' ' ' | sed 's/ $//')"
check "the lists region holds every core ios-names and ios-optional-names report" \
  same "$listed_cores" "$(all_cores_sorted)"
shared_now="$(shared_text)"
check "ios_clone still records the source in a subshell" \
  contains "$shared_now" '  ( ios_record_source_version "$core" )'
check "build_all_ios_cores still asks the cache in a subshell, for both lists" \
  same "$(count_in "$shared_now" 'if ( ios_core_is_cached "$core" ); then')" "2"
check "the cache machinery is not in the shared text" lacks "$shared_now" "ios_core_is_cached() {"
check "the help text is not in the shared text" lacks "$shared_now" "ios_usage() {"
check "the lists of cores are not in the shared text" lacks "$shared_now" "IOS_OPTIONAL_CORES=("

# Comment lines.
reset_copy
check "adding a comment line to the shared part moves no fingerprint" nothing_moves \
  insert_before 'ios_stage_dylib() {' '# a brand-new comment, which bash never runs'
reset_copy
check "editing a comment line in the shared part moves no fingerprint" nothing_moves \
  replace_line '# The one place an install name is fixed and a dylib is staged.' \
               '# The one place an install name is set and a dylib is staged (reworded).'
reset_copy
check "removing a comment line from the shared part moves no fingerprint" nothing_moves \
  delete_line '# The one place an install name is fixed and a dylib is staged.'
reset_copy
check "editing a comment inside a core's own region moves no fingerprint" nothing_moves \
  replace_line '      # NES, the default NES core. Makefile.libretro'"'"'s ios-arm64 block emits' \
               '      # NES, the default NES core (reworded). Its ios-arm64 block emits'

# Blank lines.
reset_copy
check "adding a blank line to the shared part moves no fingerprint" nothing_moves \
  insert_before 'ios_stage_dylib() {' ''
reset_copy
check "removing a blank line from the shared part moves no fingerprint" nothing_moves delete_line ''

# The cache's own code, and the help text.
reset_copy
check "editing the cache's own code (ios_core_is_cached) moves no fingerprint" nothing_moves \
  replace_line '  grep -qxF "$core $IOS_REPO $IOS_PIN pinned fp:$fp" "$manifest" || return 1' \
               '  grep -qxF "$core $IOS_REPO $IOS_PIN pinned fp:$fp" "$manifest" || { return 1; }'
reset_copy
check "editing ios_record_source_version's message moves no fingerprint" nothing_moves \
  replace_line '  echo "==> $core source: $IOS_REPO @ $sha ($state)"' \
               '  echo "==> $core source: $IOS_REPO at $sha ($state)"'
reset_copy
check "editing the help text moves no fingerprint" nothing_moves \
  replace_line '  ios-all       build all ${#IOS_CORES[@]} cores, continuing past a failure and' \
               '  ios-all       build every one of the ${#IOS_CORES[@]} cores, going on past a failure and'

# The lists, and adding or removing a core. The fake core is shaped like a new core of our own:
# its config arm and its build function each in its own region, and its name appended INSIDE the
# IOS_OPTIONAL_CORES=( ... ) parentheses, where it lands in the lists region by itself.
add_fake_core() {
  insert_before '    *)' '    # recipe-start: continuum_fake
    continuum_fake)
      # A core that only exists in these checks.
      IOS_REPO=""
      IOS_PIN=""
      IOS_DYLIB_NAME="continuum_fake_libretro_ios.dylib"
      IOS_KIND="cargo-continuum-fake"
      IOS_DISPLAY="A fake core for the cache checks"
      ;;
    # recipe-end: continuum_fake' \
  && insert_before 'build_ios_make_core() {' '# recipe-start: continuum_fake
# The fake core'"'"'s own build function.
build_ios_continuum_fake_core() {
  echo "==> building $1 (a fake)"
}
# recipe-end: continuum_fake
' \
  && insert_before '  ios_clone "$core"' '  # recipe-start: continuum_fake
  if [[ "$IOS_KIND" == "cargo-continuum-fake" ]]; then
    build_ios_continuum_fake_core "$core"
    return
  fi
  # recipe-end: continuum_fake' \
  && replace_line \
    'IOS_OPTIONAL_CORES=(flycast pcsx_rearmed_jit parallel_n64_jit flycast_jit continuum_symbian continuum_gb continuum_arcade continuum_gc continuum_sms continuum_atari)' \
    'IOS_OPTIONAL_CORES=(flycast pcsx_rearmed_jit parallel_n64_jit flycast_jit continuum_symbian continuum_gb continuum_arcade continuum_gc continuum_sms continuum_atari continuum_fake)'
}
reset_copy
check "adding a core (its regions and its name in a list) moves no other core's fingerprint" \
  same "$(moves_after add_fake_core)" "continuum_fake"
check "the added core lands in the lists region by itself" \
  contains "$(bash "$BC" ios-cache-scan cores 2>/dev/null | tr '\n' ' ')" "continuum_fake"
check "the added core has a fingerprint of its own" has_fp continuum_fake
check "with the added core, alone and all at once still agree" agree
reset_copy
check "a list wrapped onto a second line still lands in the lists region" nothing_moves \
  replace_line \
    'IOS_OPTIONAL_CORES=(flycast pcsx_rearmed_jit parallel_n64_jit flycast_jit continuum_symbian continuum_gb continuum_arcade continuum_gc continuum_sms continuum_atari)' \
    'IOS_OPTIONAL_CORES=(flycast pcsx_rearmed_jit parallel_n64_jit flycast_jit
                    continuum_symbian continuum_gb continuum_arcade continuum_gc continuum_sms continuum_atari)'
remove_stella() {
  drop_lines "recipe-start: stella2023" && drop_lines "recipe-end: stella2023" \
    && CCC_FROM='    stella2023)' awk '
         $0 == ENVIRON["CCC_FROM"] { skip = 1; done = 1 }
         skip && $0 == "      ;;" { skip = 0; next }
         !skip { print }
         END { exit !done }' "$BC" > "$BC.new" && mv "$BC.new" "$BC" \
    && replace_line '           mednafen_pce_fast stella2023 parallel_n64 azahar ppsspp)' \
                    '           mednafen_pce_fast parallel_n64 azahar ppsspp)'
}
reset_copy
check "removing a core (its region and its name) moves no other core's fingerprint" \
  nothing_moves remove_stella
check "the removed core is really gone from the lists" \
  lacks "$(bash "$BC" ios-cache-scan cores 2>/dev/null | tr '\n' ' ')x" "stella2023"
check "the removed core is really gone from the script" eval '! grep -q "stella2023)" "$BC"'
reset_copy
check "moving a core from the optional list to the required one moves no fingerprint" nothing_moves \
  eval 'replace_line "           mednafen_pce_fast stella2023 parallel_n64 azahar ppsspp)" \
          "           mednafen_pce_fast stella2023 parallel_n64 azahar ppsspp flycast)" \
        && replace_line "IOS_OPTIONAL_CORES=(flycast pcsx_rearmed_jit parallel_n64_jit flycast_jit continuum_symbian continuum_gb continuum_arcade continuum_gc continuum_sms continuum_atari)" \
          "IOS_OPTIONAL_CORES=(pcsx_rearmed_jit parallel_n64_jit flycast_jit continuum_symbian continuum_gb continuum_arcade continuum_gc continuum_sms continuum_atari)"'

# What still counts: a line bash runs, anywhere in the shared part, moves every core.
reset_copy
check "editing an executable shared line (the make call every make core uses) moves every core" \
  everything_moves replace_line '      ${IOS_MAKE_VARS[@]+"${IOS_MAKE_VARS[@]}"} -j"$(ios_jobs)" )' \
                                '      ${IOS_MAKE_VARS[@]+"${IOS_MAKE_VARS[@]}"} -j1 )'
reset_copy
check "adding a line of code to the shared part moves every core" \
  everything_moves insert_before 'ios_stage_dylib() {' 'CONTINUUM_NEW_SHARED_SETTING=1'
reset_copy
check "a #-line inside a shared here-document counts: editing it moves every core" \
  everything_moves insert_after 'error: the iOS cores build only on a macOS host.' \
                                '# a line starting with # inside a here-document is text, not a comment'
reset_copy
check "a #-line inside the Symbian core's here-document counts, for Symbian alone" \
  same "$(moves_after replace_line '# Continuum Symbian (added by Continuum'"'"'s scripts/build-core.sh).' \
                                   '# Continuum Symbian (added by build-core.sh, reworded).')" "continuum_symbian"
reset_copy
check "a blank line inside a here-document counts" \
  everything_moves insert_after 'error: the iOS cores build only on a macOS host.' ''
reset_copy
check "the #! line at the top counts (it picks the shell that runs the build)" \
  everything_moves replace_line '#!/usr/bin/env bash' '#!/bin/bash'

# A #-line inside a string that runs over several lines is text, so editing it moves every core.
# Each string is first put into the shared part (in build_ios_make_core), then its #-line edited.
string_case() {
  reset_copy
  insert_after '  local make_dir="$IOS_SRC_DIR/$IOS_MAKE_SUBDIR"' "$1" || return 1
  snapshot "$W/fp-base"
  replace_line "$2" "$3" || return 1
  snapshot "$W/fp-after"
  same "$(moved "$W/fp-base" "$W/fp-after")" "$(all_cores_sorted)"
}
check "a #-line inside a double-quoted string counts" string_case \
  '  local continuum_note="one
# inside a double-quoted string
two"' '# inside a double-quoted string' '# inside a double-quoted string, edited'
check "a #-line inside a single-quoted string counts" string_case \
  "  local continuum_note='one
# inside a single-quoted string
two'" '# inside a single-quoted string' '# inside a single-quoted string, edited'
check "a #-line inside a \$'...' string counts" string_case \
  "  local continuum_note=\$'one
# inside an ANSI-C string
two'" '# inside an ANSI-C string' '# inside an ANSI-C string, edited'
check "a #-line right after a line continuation counts" string_case \
  '  : one \
# right after a continuation' '# right after a continuation' '# right after a continuation, edited'
check "a #-line inside \$( ) that runs over several lines counts" string_case \
  '  local continuum_note="$(echo one
# inside a command substitution
echo two)"' '# inside a command substitution' '# inside a command substitution, edited'
check "a #-line inside an array's ( ) counts" string_case \
  '  local continuum_list=(one
# inside an array
two)' '# inside an array' '# inside an array, edited'
check "a #-line inside a <<- here-document with a quoted delimiter counts" string_case \
  "  cat <<-'CONTINUUM_DOC' >/dev/null
	# inside a tab-stripped here-document
	CONTINUUM_DOC" '	# inside a tab-stripped here-document' '	# inside a tab-stripped here-document, edited'
check "the end-of-line comment on a line of code counts with that line" string_case \
  '  : a command # with a comment after it' '  : a command # with a comment after it' \
  '  : a command # with a comment after it, edited'

# Anything the scan cannot follow for certain gives NO core a fingerprint, so every core rebuilds.
unclassifiable() {
  reset_copy
  insert_after '  local make_dir="$IOS_SRC_DIR/$IOS_MAKE_SUBDIR"' "$1" || return 1
  no_core_has_fp && rules_fail_saying "$2"
}
check "a case statement inside \$( ) fails closed" unclassifiable \
  '  local continuum_x="$(case a in a) echo;; esac)"' "case statement inside"
check "a here-document delimiter with an expansion in it fails closed" unclassifiable \
  '  cat <<$continuum_delim >/dev/null
$continuum_delim' "here-document delimiter with an expansion"
check "a here-document that starts inside an open string fails closed" unclassifiable \
  '  cat <<CONTINUUM_DOC "one
two"
CONTINUUM_DOC' "does not end plainly"
check "an unquoted here-document line ending in a backslash fails closed" unclassifiable \
  '  cat <<CONTINUUM_DOC >/dev/null
a line ending in a backslash \
joins this one
CONTINUUM_DOC' "ending in a backslash"
check "a line that looks like a marker inside a here-document fails closed" unclassifiable \
  '  cat <<CONTINUUM_DOC >/dev/null
# recipe-end: mgba
CONTINUUM_DOC' "looks like a region marker"
check "a function written another way fails closed" unclassifiable \
  'function continuum_other_style { :; }' "written another way"
check "a script bash cannot parse fails closed" unclassifiable '  if then fi' "syntax error"
check "a build step that reads this script's own text fails closed" unclassifiable \
  '  grep -c recipe "$IOS_SELF" >/dev/null' "a build that read this file"
check "a build step that uses line numbers fails closed" unclassifiable \
  '  echo "line $LINENO" >/dev/null' "a build that read this file"
reset_copy
check "the mistyped marker landed" replace_line '    # recipe-start: fceumm' '    # recipe-start fceumm'
check "a mistyped marker fails closed" no_core_has_fp
check "a mistyped marker is named" rules_fail_saying "mistyped region marker"

# Bash is the second line of defence: a comment the scan wrongly drops must still be caught.
# These break the SCAN in the copy on purpose, the way a bug in it would.
reset_copy
check "the scan was broken on purpose (it drops a line of code)" \
  replace_line '      else if (line ~ /^[ \t]*#/) cls = "comment"' \
               '      else if (line ~ /^[ \t]*#/ || line ~ /local canonical=/) cls = "comment"'
check "bash catches a dropped line of code, and no core gets a fingerprint" no_core_has_fp
check "bash's verdict is named" rules_fail_saying "bash reads"
reset_copy
check "the scan was broken on purpose (it drops a here-document line)" \
  replace_line '    cls = "heredoc"' '    cls = (line ~ /^#/) ? "comment" : "heredoc"'
check "bash catches a dropped here-document line, and no core gets a fingerprint" no_core_has_fp

# Every guard: breaking it gives no core a fingerprint, and says why.
guard() {
  local why="$1"
  shift
  reset_copy
  "$@" || return 1
  no_core_has_fp && rules_fail_saying "$why"
}
check "a function that is not on the allow-list inside the cache region fails closed" guard \
  "not in IOS_CACHE_FUNCTIONS" insert_before 'ios_core_is_cached() {' 'ios_cache_helper() { :; }'
check "code outside any function inside the cache region fails closed" guard \
  "code outside any function inside the cache region" \
  insert_before 'ios_core_is_cached() {' 'CONTINUUM_SETTING=1'
check "a cache region that ends in the middle of a function fails closed" guard \
  "only partly inside the cache region" \
  insert_after 'ios_core_is_cached() {' '# not-a-build-input-end: cache
# not-a-build-input-start: cache'
check "build code calling the cache machinery fails closed" guard "is cache machinery" \
  insert_after '  local make_dir="$IOS_SRC_DIR/$IOS_MAKE_SUBDIR"' '  ios_core_fingerprint "$1" >/dev/null'
check "recording the source outside a subshell fails closed" guard "is cache machinery" \
  replace_line '  ( ios_record_source_version "$core" )' '  ios_record_source_version "$core"'
move_ios_jobs_into_the_cache() {
  CCC_AT='ios_jobs() {' awk '
      !done && $0 == ENVIRON["CCC_AT"] { skip = 3; done = 1 }
      skip > 0 { skip--; next }
      { print }
      END { exit !done }' "$BC" > "$BC.new" && mv "$BC.new" "$BC" \
    && insert_before 'ios_core_is_cached() {' 'ios_jobs() {
  sysctl -n hw.ncpu 2>/dev/null || echo 4
}' \
    && replace_line '                     ios_core_fingerprint ios_core_is_cached ios_cache_command)' \
                    '                     ios_core_fingerprint ios_core_is_cached ios_cache_command ios_jobs)'
}
check "build code moved into the cache region (and allow-listed) fails closed" guard \
  "is cache machinery" move_ios_jobs_into_the_cache
check "a function defined a second time (in a region, overriding shared code) fails closed" guard \
  "defined twice" insert_before 'build_ios_make_core() {' '# recipe-start: fceumm
ios_stage_dylib() { :; }
# recipe-end: fceumm'
check "the allow-list declared a second time fails closed" guard "declared exactly once" \
  insert_before 'ios_stage_dylib() {' 'IOS_CACHE_FUNCTIONS=(ios_stage_dylib)'
check "something other than a list of cores in the lists region fails closed" guard \
  "something other than array assignments" \
  insert_before '# not-a-build-input-end: lists' 'echo "not a list"'
check "an array that is not a list of cores in the lists region fails closed" guard \
  "not in IOS_CORE_LIST_NAMES" insert_before '# not-a-build-input-end: lists' 'IOS_REQUIRED_EXTRA=(retro_run)'
check "an expansion inside a list of cores fails closed" guard "something other than array assignments" \
  replace_line \
    'IOS_OPTIONAL_CORES=(flycast pcsx_rearmed_jit parallel_n64_jit flycast_jit continuum_symbian continuum_gb continuum_arcade continuum_gc continuum_sms continuum_atari)' \
    'IOS_OPTIONAL_CORES=(flycast pcsx_rearmed_jit parallel_n64_jit flycast_jit continuum_symbian continuum_gb continuum_arcade $(echo continuum_gc))'
check "lists of cores outside the lists region fail closed" guard "inside a not-a-build-input: lists region" \
  eval 'delete_line "# not-a-build-input-start: lists" && delete_line "# not-a-build-input-end: lists"'
check "a recipe region for a core that is not listed fails closed" guard "belongs to no core" \
  insert_before 'build_ios_make_core() {' '# recipe-start: nosuchcore
nosuchcore_helper() { :; }
# recipe-end: nosuchcore'
check "a recipe region inside a shared function fails closed" guard \
  "neither a whole case arm nor a whole function" \
  replace_line '  local canonical="$2"' '    # recipe-start: fceumm
  local canonical="$2"
    # recipe-end: fceumm'
merge_mesen2_into_fceumm() {
  delete_line '    # recipe-end: fceumm' && delete_line '    # recipe-start: mesen2' \
    && replace_line '    # recipe-end: mesen2' '    # recipe-end: fceumm'
}
check "one core's config arm inside another core's region fails closed" guard \
  "the case arm for mesen2 sits in recipe region fceumm" merge_mesen2_into_fceumm

# 14. The three findings of the review of 393f12c (K2).
#
# (a) A core's patch step cannot sit outside its own region: every patches arm is in its own
# family's region and holds only that family's patches and helpers; a function of a region is used
# only by that family (or by build_ios_core's dispatch for a kind only that family sets).
reset_copy
patch_arms_in_shared="$(bash "$BC" ios-cache-scan table 2>/dev/null | awk -F'\t' '
  $3 == "shared" && $4 == "ios_apply_core_patches" && $5 ~ /^[ \t]*[a-z][a-z0-9_|]*\)[ \t]*$/' | wc -l | tr -d ' ')"
check "no patches arm sits in the shared part" same "$patch_arms_in_shared" "0"
check "every patches arm sits in its own family's region" \
  same "$(bash "$BC" ios-cache-scan table 2>/dev/null | awk -F'\t' '
    $4 == "ios_apply_core_patches" && $5 ~ /^[ \t]*[a-z][a-z0-9_]*\)[ \t]*$/ {
      core = $5; gsub(/[ \t)]/, "", core); fam = core; sub(/_jit$/, "", fam)
      if ($3 != "r:" fam) bad++ ; n++ }
    END { print (n > 0 && bad == 0) ? "yes" : "no: " n + 0 " arms, " bad + 0 " misplaced" }')" "yes"
check "a patches arm outside any region fails closed" guard "outside its recipe region" \
  eval 'delete_nth "    # recipe-start: blastem" 2 && delete_nth "    # recipe-end: blastem" 2'
check "a patches arm inside another core's region fails closed" guard \
  "the case arm for gearsystem sits in recipe region bsnes" \
  eval 'delete_nth "    # recipe-end: bsnes" 2 && delete_nth "    # recipe-start: gearsystem" 2 \
          && replace_nth "    # recipe-end: gearsystem" 2 "    # recipe-end: bsnes"'
check "a patches arm applying another family's patch fails closed" guard "is not a patch of mesen2" \
  replace_line '      ios_apply_simple_patch mesen2 mesen2-nes-memory-cheats-saves.patch \' \
               '      ios_apply_simple_patch mesen2 bsnes-continuum-memory-and-saves.patch \'
check "a patches arm naming another core fails closed" guard "names the core bsnes" \
  replace_line '      ios_apply_simple_patch mesen2 mesen2-nes-memory-cheats-saves.patch \' \
               '      ios_apply_simple_patch bsnes mesen2-nes-memory-cheats-saves.patch \'
check "a core's own patch helper moved out of its region fails closed" guard \
  "holds something other than the patch steps of azahar" \
  eval 'delete_nth "# recipe-start: azahar" 2 && delete_nth "# recipe-end: azahar" 2'
check "a core's patch helper called from another core's patches arm fails closed" guard \
  "holds something other than the patch steps of ppsspp" \
  insert_after_nth '    ppsspp)' 2 '      ios_apply_azahar_pipeline_wait_patch'
check "a patch step in the shared part fails closed" guard "ios_apply_simple_patch is used outside a recipe region" \
  insert_after '  local make_dir="$IOS_SRC_DIR/$IOS_MAKE_SUBDIR"' \
               '  ios_apply_simple_patch "$core" fceumm-extra.patch Makefile "a marker"'
check "a shared line naming a patch file fails closed" guard "is named in the shared part" \
  insert_after '  local make_dir="$IOS_SRC_DIR/$IOS_MAKE_SUBDIR"' \
               '  patch -p1 -d "$IOS_SRC_DIR" < "$ROOT/scripts/patches/fceumm-extra.patch"'
check "a region naming another family's patch file fails closed" guard \
  "recipe region azahar names ppsspp-ios-jit-region.patch, a patch of ppsspp" \
  replace_line '  local patch="$ROOT/scripts/patches/azahar-do-not-wait-on-pipeline-compile.patch"' \
               '  local patch="$ROOT/scripts/patches/ppsspp-ios-jit-region.patch"'
check "one family's build function used by a second family fails closed" guard "belongs to recipe region" \
  replace_line '      IOS_KIND="cmake-ppsspp"' '      IOS_KIND="cmake-flycast"'
check "one region's function called from another family's region fails closed" guard \
  "build_ios_cmake_core belongs to recipe region mgba" \
  insert_after_nth '  local built' 2 '  build_ios_cmake_core "$core"'
check "a function in a region that nothing calls by name fails closed" guard "is never called by name" \
  insert_before 'build_ios_make_core() {' '# recipe-start: fceumm
fceumm_unused_helper() { :; }
# recipe-end: fceumm'
check "a config arm that runs a command fails closed" guard "may only set IOS_ variables to plain values" \
  insert_after '      IOS_DISPLAY="NES"' '      export CFLAGS=-O0'
check "a config arm value with an expansion in it fails closed" guard \
  "may only set IOS_ variables to plain values" \
  insert_after '      IOS_DISPLAY="NES"' '      IOS_MAKE_VARS=("CFLAGS=$HOME")'
check "a dispatch in a region for a kind another family sets too fails closed" guard \
  "in build_ios_core may only hold" \
  eval 'add_fake_core && replace_line "      IOS_KIND=\"cargo-continuum-fake\"" "      IOS_KIND=\"cargo-continuum-gb\"" \
          && replace_line "  if [[ \"\$IOS_KIND\" == \"cargo-continuum-fake\" ]]; then" \
                          "  if [[ \"\$IOS_KIND\" == \"cargo-continuum-gb\" ]]; then"'
check "a dispatch region holding anything else fails closed" guard "in build_ios_core may only hold" \
  eval 'add_fake_core && insert_after "    build_ios_continuum_fake_core \"\$core\"" "    echo extra"'
# The rules read the code of a line, not a comment at its end, so ordinary edits by the people
# adding cores stay legal: a note after an assignment, a dispatch written as an elif.
reset_copy
check "a comment at the end of a config arm line is fine" eval \
  'replace_line "      IOS_DISPLAY=\"NES\"" "      IOS_DISPLAY=\"NES\"  # the NES, a note" && rules_hold && has_fp fceumm'
fake_core_by_elif() {
  add_fake_core || return 1
  CCC_PAT="# recipe-start: continuum_fake" awk '
    $0 == "  # recipe-start: continuum_fake" { skip = 1; done = 1 }
    skip { if ($0 == "  # recipe-end: continuum_fake") skip = 0; next }
    { print }
    END { exit !done }' "$BC" > "$BC.new" && mv "$BC.new" "$BC" || return 1
  replace_line '    build_ios_continuum_gb_core "$core"' '    build_ios_continuum_gb_core "$core"
    return
  elif [[ "$IOS_KIND" == "cargo-continuum-fake" ]]; then
    build_ios_continuum_fake_core "$core"'
}
reset_copy
check "a new core dispatched by an elif after cBoy's if is fine" \
  eval 'fake_core_by_elif && rules_hold && has_fp continuum_fake && has_fp continuum_gb'

# (c) A separator between a family's regions: each region is hashed after a header naming which
# one it is, where it sits and how many lines it holds, so lines moving between a family's regions,
# or a region moving, change that family's fingerprint (and nobody else's).
reset_copy
check "the recipe text names each region, where it sits and how many lines it holds" \
  contains "$(slice flycast_jit)" "@@ region 4 of flycast, after shared line "
reset_copy
check "merging two of flycast's regions into one rebuilds flycast and its JIT build only" \
  same "$(moves_after eval 'delete_nth "    # recipe-end: flycast" 1 && delete_nth "    # recipe-start: flycast" 2')" \
       "flycast flycast_jit"
reset_copy
check "moving flycast's build function to another place rebuilds flycast and its JIT build only" \
  same "$(moves_after move_block '# recipe-start: flycast' '# recipe-end: flycast' 'build_ios_make_core() {')" \
       "flycast flycast_jit"

# 15. The hostile review (K3). Each hazard is tried on a throwaway copy; every one must fail closed
# (no core gets a fingerprint) and name the reason. These would all have passed before this pass.
reset_copy

# A line that bash runs but the scan took for a comment, hidden after a line continuation. Without
# the carry fix the scan read the second line as a comment from column one and every guard that
# reads the code of a line went blind; bash joins `: x\`+`#;cmd` into one command and runs it.
check "a cache call hidden after a line continuation fails closed" guard "is cache machinery" \
  insert_after '  local make_dir="$IOS_SRC_DIR/$IOS_MAKE_SUBDIR"' '  : x\
#; ios_core_fingerprint "$core" >/dev/null'
check "the script path read after a line continuation fails closed" guard 'IOS_SELF outside the cache' \
  insert_after '  local make_dir="$IOS_SRC_DIR/$IOS_MAKE_SUBDIR"' '  : x\
#; cat "$IOS_SELF" >/dev/null'

# A ( inside [[ ]] (grouping, or an extglob like @( ): bash does not treat a # after it as a
# comment, but the scan would, so it refuses rather than guess. A grouping ( ) is used here
# because it parses on both bash 5.2 and 3.2, so the scan (not bash's own parser) is what rejects
# it on either host; an extglob @( ) would be a parse error on the Mac's bash 3.2 instead.
check "a ( inside [[ ]] fails closed" guard "a ( inside [[ ]]" \
  insert_after '  local make_dir="$IOS_SRC_DIR/$IOS_MAKE_SUBDIR"' \
    '  [[ ( "$core" == x ) ]] && ios_core_fingerprint "$core" >/dev/null'

# A build step that reads the script through $0 (like $IOS_SELF): it would see comments, so they
# could not be dropped without changing the build.
check "a build step that reads \$0 fails closed" guard '$0 outside the cache' \
  insert_after '  local make_dir="$IOS_SRC_DIR/$IOS_MAKE_SUBDIR"' '  grep -c recipe "$0" >/dev/null'

# A patches arm whose continuation line chains a second command: the second line of a two-line
# ios_apply_simple_patch call is where another patch step could hide, so it may carry only the
# call's own tail.
check "a patches-arm continuation line that chains a second command fails closed" guard \
  "runs more than one patch step" \
  insert_after '        src/citra_libretro/citra_libretro.cpp "Continuum: the iPhone camera and Amiibo reach Azahar"' \
    '      ios_apply_simple_patch azahar azahar-x.patch \
        src/x.cpp "m"; rm -rf "$IOS_SRC_DIR"'

# A patch helper inside a region that applies another family'"'"'s patch: caught by the patch NAME,
# whatever path spells it, because the fingerprint of that other family (not this one) covers it.
check "a region patch helper applying another family's patch fails closed" guard \
  "a patch of ppsspp" \
  insert_after '  local marker="Continuum: do not block the emulation thread on Vulkan pipeline compile."' \
    '  ios_apply_simple_patch azahar ppsspp-ios-jit-region.patch Common/MemoryUtil.h "Continuum: code memory"'
check "a region reaching another family's patch through a \$VAR path fails closed" guard \
  "a patch of ppsspp" \
  insert_after '  local marker="Continuum: do not block the emulation thread on Vulkan pipeline compile."' \
    '  local pp="$ROOT/scripts/patches"
  patch -p1 --forward -d "$IOS_SRC_DIR" < "$pp/ppsspp-ios-jit-region.patch" || true'

# A line that names the patches directory but no attributable patch file (the shape of a build step
# reaching patches through a variable) is refused on its own.
check "a line naming the patches directory with no attributable file fails closed" guard \
  "names the scripts/patches directory" \
  insert_after '  local marker="Continuum: do not block the emulation thread on Vulkan pipeline compile."' \
    '  local pdir="$ROOT/scripts/patches"; ls "$pdir" >/dev/null'

reset_copy

echo "core cache checks: $passed passed, $failed failed"
[[ "$failed" -eq 0 ]] || exit 1
