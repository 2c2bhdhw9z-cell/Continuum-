#!/usr/bin/env bash
# Builds the Step 10 Switch stub wrapper, and its harness.
#
# Three configurations, and which one you get depends on what is available:
#
#   host      no Vulkan. The stub's HostStubRenderer reports pixels, so the gate, the
#             thread affinity and the whole libretro contract are testable on a build
#             machine with no GPU. This is what CI runs.
#   vulkan    with Vulkan headers present, compiles VulkanStubRenderer as well. Still not
#             runnable without a loader and an ICD, but it type-checks the real handover.
#   ios       aarch64-apple-ios against MoltenVK. Needs a macOS host with Xcode.
#
# Usage: build.sh [host|vulkan|ios]
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
MODE="${1:-host}"
OUT="$HERE/build"
mkdir -p "$OUT"

# libretro.h comes from the same libretro-common the cores are built against, so the
# wrapper cannot drift from the ABI the frontend uses.
LIBRETRO_INC=""
for candidate in \
  "$ROOT/.work/hdr/libretro" \
  "$ROOT/.work/fceumm/src/drivers/libretro/libretro-common/include" \
  "$ROOT/.work/libretro-fceumm/src/drivers/libretro/libretro-common/include"; do
  if [ -f "$candidate/libretro.h" ]; then LIBRETRO_INC="$candidate"; break; fi
done
if [ -z "$LIBRETRO_INC" ]; then
  echo "error: no libretro.h found. Build a core first, or place headers in .work/hdr." >&2
  exit 1
fi
echo "==> libretro.h from $LIBRETRO_INC"

CXX="${CXX:-clang++}"
CXXFLAGS=(-std=c++20 -O2 -fPIC -Wall -Wextra -Wno-unused-parameter -I"$HERE" -I"$LIBRETRO_INC")
SOURCES=("$HERE/continuum_switch_libretro.cpp" "$HERE/stub_engine.cpp")

case "$MODE" in
  host)
    LIB="$OUT/libcontinuum_switch.so"
    SHARED=(-shared)
    ;;
  vulkan)
    VULKAN_INC=""
    for candidate in "$ROOT/.work/hdr" /usr/include /usr/local/include; do
      if [ -f "$candidate/vulkan/vulkan_core.h" ]; then VULKAN_INC="$candidate"; break; fi
    done
    if [ -z "$VULKAN_INC" ]; then
      echo "error: no vulkan/vulkan_core.h found." >&2
      exit 1
    fi
    echo "==> vulkan headers from $VULKAN_INC"
    CXXFLAGS+=(-DCONTINUUM_HAVE_VULKAN -I"$VULKAN_INC")
    SOURCES+=("$HERE/vulkan_stub_renderer.cpp")
    LIB="$OUT/libcontinuum_switch_vk.so"
    SHARED=(-shared)
    ;;
  ios)
    # Requires a macOS host: MoltenVK supplies both the Vulkan headers and the loader.
    : "${MOLTENVK:?set MOLTENVK to the MoltenVK.xcframework path}"
    SDK="$(xcrun --sdk iphoneos --show-sdk-path)"
    CXXFLAGS+=(-DCONTINUUM_HAVE_VULKAN -isysroot "$SDK"
               -target arm64-apple-ios16.0 -I"$MOLTENVK/include")
    SOURCES+=("$HERE/vulkan_stub_renderer.cpp")
    LIB="$OUT/libcontinuum_switch.dylib"
    # @rpath, so dlopen resolves inside the bundle. Without this it works in the
    # simulator and fails on device.
    SHARED=(-dynamiclib -install_name "@rpath/libcontinuum_switch.dylib")
    ;;
  *)
    echo "usage: build.sh [host|vulkan|ios]" >&2
    exit 1
    ;;
esac

echo "==> building $MODE"
"$CXX" "${CXXFLAGS[@]}" "${SHARED[@]}" -o "$LIB" "${SOURCES[@]}" -lpthread
echo "==> $LIB ($(du -h "$LIB" | cut -f1))"

# The harness only makes sense against a loadable library, so it is built for host builds.
if [ "$MODE" = "host" ]; then
  HARNESS="$OUT/switch_stub_test"
  "$CXX" -std=c++20 -O2 -Wall -Wextra -Wno-unused-parameter \
    -I"$HERE" -I"$LIBRETRO_INC" -o "$HARNESS" "$HERE/test_harness.cpp" -ldl -lpthread
  echo "==> $HARNESS"
  echo
  "$HARNESS" "$LIB"
fi
