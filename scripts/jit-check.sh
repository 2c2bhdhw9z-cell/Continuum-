#!/bin/bash
# Runs the PSP and 3DS recompilers' own test suites on an Apple Silicon Mac against Continuum's code
# memory (crates/emulator-bridge/src/jit26.c built with CONTINUUM_JIT_TEST), in the same two-address
# shape a TXM iPhone gets: code is run from one address and written through another. A recompiler
# that writes through the address it runs from faults here the way it would on the phone.
#
# Not part of the app build. Started by hand: .github/workflows/jit-check.yml.
#
#   scripts/jit-check.sh probe      the shape itself works on this Mac
#   scripts/jit-check.sh ppsspp     PPSSPP's pspautotests, its JIT, plain memory vs Continuum's
#   scripts/jit-check.sh dynarmic   Azahar's dynarmic A32 tests, plain vs Continuum's
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
WORK="${JIT_CHECK_DIR:-$ROOT/.jit-check}"
mkdir -p "$WORK"

pin() { # core -> IOS_PIN from build-core.sh
  awk -v c="$1)" '$1 == c {f=1} f && /IOS_PIN=/ {gsub(/.*IOS_PIN="|".*/, ""); print; exit}' "$ROOT/scripts/build-core.sh"
}

shim() {
  clang -O1 -DCONTINUUM_JIT_TEST -c "$ROOT/crates/emulator-bridge/src/jit26.c" -o "$WORK/jit26.o"
}

probe() {
  shim
  cat > "$WORK/probe.c" <<'EOF'
#include <libkern/OSCacheControl.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
bool continuum_jit_region(const char *, size_t, void **, void **);
int main(void) {
    void *rx = 0, *rw = 0;
    if (!continuum_jit_region("probe", 16384, &rx, &rw)) { puts("probe: no region (CONTINUUM_DUAL=1?)"); return 1; }
    if (rx == rw) { puts("probe: one address, not two"); return 1; }
    uint32_t code[] = {0x52800540u /* mov w0, #42 */, 0xd65f03c0u /* ret */};
    memcpy(rw, code, sizeof code);
    sys_icache_invalidate(rx, sizeof code);
    int got = ((int (*)(void))rx)();
    printf("probe: rx %p rw %p, generated code returned %d\n", rx, rw, got);
    return got == 42 ? 0 : 1;
}
EOF
  clang -O1 "$WORK/probe.c" "$WORK/jit26.o" -o "$WORK/probe"
  CONTINUUM_DUAL=1 "$WORK/probe"
}

ppsspp() {
  shim
  local src="$WORK/ppsspp" sha
  sha="$(pin ppsspp)"
  if [ ! -d "$src/.git" ]; then
    git clone -q --filter=blob:none https://github.com/hrydgard/ppsspp "$src"
  fi
  git -C "$src" checkout -q "$sha"
  (cd "$src" && .github/checkout-submodules.sh /macosx/universal/)
  patch -p1 --forward -d "$src" < "$ROOT/scripts/patches/ppsspp-ios-jit-region.patch"
  # The patch's Continuum code is iPhone-only; switch it on for this Mac build too.
  perl -0pi -e 's/#if PPSSPP_PLATFORM\(IOS\)\n#include <dlfcn.h>/#if PPSSPP_PLATFORM(IOS) || defined(CONTINUUM_JIT_TEST)\n#include <dlfcn.h>/' "$src/Common/MemoryUtil.cpp"
  grep -q "defined(CONTINUUM_JIT_TEST)" "$src/Common/MemoryUtil.cpp"
  # PPSSPP_MASKED=1: the memory layout an iPhone build uses (Core/MemMap.h turns MASKED_PSP_MEMORY on
  # for iOS), where every guest address is masked to 30 bits before the base is added.
  if [ "${PPSSPP_MASKED:-0}" = 1 ]; then
    perl -pi -e 's/^#if PPSSPP_ARCH\(32BIT\) \|\| PPSSPP_PLATFORM\(UWP\)/#if 1 || PPSSPP_ARCH(32BIT) || PPSSPP_PLATFORM(UWP)/' "$src/Core/MemMap.h"
    grep -q "^#if 1 || PPSSPP_ARCH(32BIT)" "$src/Core/MemMap.h"
  fi
  (cd "$src" && CMAKE_ARGS="-DCMAKE_C_FLAGS=-DCONTINUUM_JIT_TEST -DCMAKE_CXX_FLAGS=-DCONTINUUM_JIT_TEST -DCMAKE_EXE_LINKER_FLAGS=$WORK/jit26.o" ./b.sh --headless --no-png)
  local status=0
  if [ "${PPSSPP_MASKED:-0}" = 1 ]; then
    echo "==> pspautotests, IR interpreter, masked memory (the reference)"
    (cd "$src" && CONTINUUM_DUAL=0 python3 test.py -g --graphics=software --cpu=ir 2>&1) | tee "$WORK/ppsspp-ir.txt" | tail -15 || true
  fi
  for dual in 0 1; do
    echo "==> pspautotests, CPU JIT, CONTINUUM_DUAL=$dual ($([ $dual = 1 ] && echo "Continuum's two-address memory" || echo "plain memory"))"
    (cd "$src" && CONTINUUM_DUAL=$dual python3 test.py -g --graphics=software --cpu=jit 2>&1) | tee "$WORK/ppsspp-dual$dual.txt" | tail -40 || true
  done
  echo "==> slices handed out with CONTINUUM_DUAL=1:"
  grep -c "continuum_jit_region:" "$WORK/ppsspp-dual1.txt" || true
  # The two runs must agree: anything that passes on plain memory and fails on Continuum's is ours.
  diff <(grep -E "^(Passed|Failed|Timed out|Crashed|  )" "$WORK/ppsspp-dual0.txt" | sort) \
       <(grep -E "^(Passed|Failed|Timed out|Crashed|  )" "$WORK/ppsspp-dual1.txt" | sort) || status=1
  return $status
}

dynarmic() {
  shim
  local az="$WORK/azahar" sha
  sha="$(pin azahar)"
  if [ ! -d "$az/.git" ]; then
    git clone -q --filter=blob:none https://github.com/azahar-emu/azahar "$az"
  fi
  git -C "$az" checkout -q "$sha"
  git -C "$az" submodule update -q --init --depth 1 externals/oaknut externals/dynarmic
  git -C "$az/externals/dynarmic" submodule update -q --init --recursive --depth 1
  patch -p1 --forward -d "$az" < "$ROOT/scripts/patches/azahar-ios-jit-region.patch"
  # dynarmic's own tests, with the oaknut Azahar ships (patched) in place of dynarmic's copy.
  rm -rf "$az/externals/dynarmic/externals/oaknut"
  cp -R "$az/externals/oaknut" "$az/externals/dynarmic/externals/oaknut"
  local plain="$WORK/dynarmic-plain"
  rm -rf "$plain" && cp -R "$az/externals/dynarmic" "$plain"
  # The test copy takes the iPhone path of the patched oaknut on this Mac.
  perl -pi -e 's/TARGET_OS_IPHONE/(TARGET_OS_IPHONE || defined(CONTINUUM_JIT_TEST))/g' \
    "$az/externals/dynarmic/externals/oaknut/include/oaknut/code_block.hpp"
  local common=(-G Ninja -DCMAKE_BUILD_TYPE=Release -DDYNARMIC_TESTS=ON -DDYNARMIC_FRONTENDS=A32
                -DDYNARMIC_WARNINGS_AS_ERRORS=OFF -DDYNARMIC_USE_PRECOMPILED_HEADERS=OFF)
  cmake -S "$plain" -B "$plain/build" "${common[@]}" >/dev/null
  cmake --build "$plain/build" --target dynarmic_tests
  cmake -S "$az/externals/dynarmic" -B "$WORK/dynarmic-dual" "${common[@]}" \
    -DCMAKE_CXX_FLAGS=-DCONTINUUM_JIT_TEST -DCMAKE_EXE_LINKER_FLAGS="$WORK/jit26.o" >/dev/null
  cmake --build "$WORK/dynarmic-dual" --target dynarmic_tests
  local status=0
  echo "==> dynarmic A32 tests, plain memory"
  "$plain/build/tests/dynarmic_tests" 2>&1 | tail -5 | tee "$WORK/dynarmic-plain.txt" || true
  echo "==> dynarmic A32 tests, Continuum's two-address memory"
  CONTINUUM_DUAL=1 "$WORK/dynarmic-dual/tests/dynarmic_tests" 2>&1 | tee "$WORK/dynarmic-dual.txt" | grep -v "continuum_jit_region:" | tail -5 || status=1
  echo "==> slices handed out: $(grep -c "continuum_jit_region:" "$WORK/dynarmic-dual.txt" || true)"
  diff <(grep -E "test cases|assertions" "$WORK/dynarmic-plain.txt") \
       <(grep -E "test cases|assertions" "$WORK/dynarmic-dual.txt") || status=1
  return $status
}

telemetry() { # the crash catcher and the frame watchdog (crates/emulator-bridge/src/telemetry.c)
  cat > "$WORK/tel.c" <<'EOF'
#include <stdio.h>
#include <string.h>
#include <unistd.h>
void continuum_tel_open(const char *dir);
void continuum_tel_core_log(int level, const char *text);
void continuum_tel_note(const char *text);
void continuum_tel_frame_begin(void);
void continuum_tel_frame_end(void);
__attribute__((noinline)) static void busy(unsigned ms) { usleep(ms * 1000); }
__attribute__((noinline)) static void crash_here(void) { *(volatile int *)16 = 1; }
int main(int argc, char **argv) {
    continuum_tel_open(argv[2]);
    continuum_tel_core_log(1, "a core line");
    continuum_tel_core_log(3, "an error line");
    continuum_tel_frame_begin(); busy(120); continuum_tel_frame_end();
    if (strcmp(argv[1], "stuck") == 0) { continuum_tel_frame_begin(); busy(5500); continuum_tel_frame_end(); }
    if (strcmp(argv[1], "crash") == 0) { continuum_tel_frame_begin(); crash_here(); }
    return 0;
}
EOF
  clang -O1 -g -fno-omit-frame-pointer "$ROOT/crates/emulator-bridge/src/telemetry.c" "$WORK/tel.c" -o "$WORK/tel"
  rm -rf "$WORK/tel-out" && mkdir -p "$WORK/tel-out"
  "$WORK/tel" stuck "$WORK/tel-out"
  "$WORK/tel" crash "$WORK/tel-out" || echo "(crashed with status $?, as it should)"
  for f in "$WORK"/tel-out/*.txt; do echo "==> $(basename "$f")"; cat "$f"; done
  grep -q "STUCK frame" "$WORK/tel-out/core-log-previous.txt"
  grep -q "slow frame" "$WORK/tel-out/core-log-previous.txt"
  grep -q "crash_here" "$WORK/tel-out/crash.txt"
}

case "${1:-all}" in
  probe) probe ;;
  telemetry) telemetry ;;
  ppsspp) ppsspp ;;
  dynarmic) dynarmic ;;
  all) probe && dynarmic && ppsspp ;;
  *) echo "usage: $0 probe|ppsspp|dynarmic|all" >&2; exit 2 ;;
esac
