#!/usr/bin/env bash
# Builds Continuum Symbian for the host and runs its harness.
#
# The host build links the stub engine, so load_game fails on purpose.
# The phone build links EKA2L1 instead: scripts/build-core.sh continuum_symbian.
#
# Usage: build.sh [host]
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
MODE="${1:-host}"
if [ "$MODE" != "host" ]; then
  echo "usage: build.sh host" >&2
  echo "The phone build is scripts/build-core.sh continuum_symbian (macOS only)." >&2
  exit 1
fi

OUT="$HERE/build"
mkdir -p "$OUT"

LIBRETRO_INC="$ROOT/.work/hdr/libretro"
if [ ! -f "$LIBRETRO_INC/libretro.h" ]; then
  echo "error: no libretro.h in .work/hdr/libretro. Run scripts/fetch-libretro-headers.sh first." >&2
  exit 1
fi

CXX="${CXX:-clang++}"
if ! command -v "$CXX" >/dev/null 2>&1; then
  CXX=g++
fi

echo "==> $CXX"
"$CXX" -std=c++20 -O2 -fPIC -Wall -Wextra -Wno-unused-parameter \
  -I"$HERE" -I"$LIBRETRO_INC" -shared -o "$OUT/libcontinuum_symbian.so" \
  "$HERE/continuum_symbian_libretro.cpp" "$HERE/stub_engine.cpp" -lpthread
echo "==> $OUT/libcontinuum_symbian.so"

"$CXX" -std=c++20 -O2 -Wall -Wextra -Wno-unused-parameter \
  -I"$HERE" -I"$LIBRETRO_INC" -o "$OUT/continuum_symbian_test" \
  "$HERE/test_harness.cpp" -ldl -lpthread
"$OUT/continuum_symbian_test" "$OUT/libcontinuum_symbian.so"
