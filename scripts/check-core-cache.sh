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

# 4. Any other edit of the recipe moves every core.
echo "# a recipe change" >> "$BC"
fceumm_c="$(fp fceumm)"
check "a build-core.sh edit changes fceumm" differ "$fceumm_a" "$fceumm_c"
check "a build-core.sh edit changes azahar" differ "$azahar_before" "$(fp azahar)"

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

echo "core cache checks: $passed passed, $failed failed"
[[ "$failed" -eq 0 ]] || exit 1
