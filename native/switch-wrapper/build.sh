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
    # Vulkan is *optional* here, and that is deliberate.
    #
    # The rotating colour reaches the screen through the software path —
    # HostStubRenderer writes pixels, the engine uploads them, wgpu composites. That is
    # the whole of what Phase 5 step 1 completes, and it needs no Vulkan, no MoltenVK and
    # no ICD. Requiring MoltenVK to build the .ipa would block a working app on a
    # dependency nothing in it uses yet.
    #
    # Set MOLTENVK to also compile VulkanStubRenderer, for when the hardware path lands.
    SDK="$(xcrun --sdk iphoneos --show-sdk-path)"
    IOS_MIN="${IOS_MIN:-16.0}"
    CXXFLAGS+=(-isysroot "$SDK" -target "arm64-apple-ios$IOS_MIN")
    if [ -n "${MOLTENVK:-}" ] && [ -d "${MOLTENVK}/include" ]; then
      echo "==> MoltenVK headers from $MOLTENVK/include"
      CXXFLAGS+=(-DCONTINUUM_HAVE_VULKAN -I"$MOLTENVK/include")
      SOURCES+=("$HERE/vulkan_stub_renderer.cpp")
    else
      echo "==> no MOLTENVK set; software renderer only (this is what step 1 needs)"
    fi
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

# pthread is part of libSystem on Apple platforms, and `-lpthread` is not reliably
# resolvable against the iPhoneOS SDK. Elsewhere it has to be named explicitly.
#
# Appended to CXXFLAGS rather than kept in a list of its own, because macOS ships bash 3.2,
# where expanding an empty array under `set -u` is an "unbound variable" error rather than
# nothing. A `LINK_LIBS=()` that is empty on exactly one platform is a script that works
# everywhere except the platform it was added for.
if [ "$MODE" != "ios" ]; then
  CXXFLAGS+=(-lpthread)
fi

echo "==> building $MODE"
"$CXX" "${CXXFLAGS[@]}" "${SHARED[@]}" -o "$LIB" "${SOURCES[@]}"
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
