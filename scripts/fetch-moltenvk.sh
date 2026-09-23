#!/usr/bin/env bash
#
# Fetch MoltenVK's DYNAMIC iOS arm64 framework into native/ios/build/lib/.
#
# MoltenVK is Vulkan implemented on Metal, and it is the gate on every system this app cannot
# currently reach: Dreamcast, PSP and the 3DS all render through a GPU API rather than rasterising in
# software, and the N64 only runs at a playable speed that way. See docs/SET_HW_RENDER_DESIGN.md.
#
# WHY THE DYNAMIC ONE, AND WHY A FRAMEWORK. The release tarball carries two builds:
#
#   MoltenVK/MoltenVK/static/MoltenVK.xcframework/ios-arm64/libMoltenVK.a
#   MoltenVK/MoltenVK/dynamic/MoltenVK.xcframework/ios-arm64/MoltenVK.framework
#
# Only the second can be `dlopen`ed, and `dlopen` is the whole point: the app must LAUNCH whether or
# not Vulkan is usable. Linking it would mean a missing or broken MoltenVK stops the app opening at
# all, which this project has already paid for once — a startup probe that executed generated code
# made the app unlaunchable for several builds. So it is loaded by path at runtime and every failure
# is a line on the diagnostics panel.
#
# PINNED to an exact version rather than tracking latest. This is a prebuilt binary dependency whose
# internal layout this script depends on, and a silent reorganisation upstream would look like a
# broken build for no reason anyone could see. The core sources are deliberately unpinned for the
# opposite reason, which docs/PLATFORM_LIMITS.md explains.
set -euo pipefail

MOLTENVK_VERSION="v1.4.2"
# The binary inside the framework was 4,822,128 bytes at this version, so roughly 4.6 MB rather than
# the 8 MB the design document estimated.
TARBALL="MoltenVK-ios.tar"
URL="https://github.com/KhronosGroup/MoltenVK/releases/download/${MOLTENVK_VERSION}/${TARBALL}"

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/.." && pwd)"
OUT_DIR="$ROOT/native/ios/build/lib"
FRAMEWORK="$OUT_DIR/MoltenVK.framework"
# Inside the tarball.
MEMBER="MoltenVK/MoltenVK/dynamic/MoltenVK.xcframework/ios-arm64/MoltenVK.framework"

# Idempotent: a framework already in place is left alone, so a rebuild does not re-download 33 MB.
# Checked by the BINARY rather than the directory, because an interrupted extraction can leave a
# directory that exists and cannot be loaded.
if [ -f "$FRAMEWORK/MoltenVK" ]; then
  echo "==> MoltenVK already present ($(du -h "$FRAMEWORK/MoltenVK" | cut -f1))"
  exit 0
fi

WORK="$ROOT/.work/moltenvk"
mkdir -p "$WORK" "$OUT_DIR"

echo "==> fetching MoltenVK $MOLTENVK_VERSION"
curl -fsSL --retry 3 --retry-delay 2 -o "$WORK/$TARBALL" "$URL" || {
  echo "error: could not download $URL" >&2
  exit 1
}

echo "==> extracting $MEMBER"
tar xf "$WORK/$TARBALL" -C "$WORK" "$MEMBER" || {
  echo "error: $MEMBER is not in the tarball; upstream changed its layout" >&2
  echo "       contents, for whoever fixes this:" >&2
  tar tf "$WORK/$TARBALL" | grep -iE "ios-arm64|dynamic" | head -20 >&2 || true
  exit 1
}

rm -rf "$FRAMEWORK"
cp -R "$WORK/$MEMBER" "$FRAMEWORK"

# Asserted rather than assumed: a framework whose binary is missing or is the wrong architecture
# would pass every later check and fail on device, which is the failure mode this project keeps
# paying for. `file` is used rather than `lipo` so this also reports usefully when handed something
# that is not a Mach-O at all.
[ -f "$FRAMEWORK/MoltenVK" ] || {
  echo "error: extracted framework has no MoltenVK binary" >&2
  exit 1
}
DESCRIPTION="$(file -b "$FRAMEWORK/MoltenVK")"
case "$DESCRIPTION" in
  *"arm64"*"dynamically linked shared library"*) ;;
  *)
    echo "error: $FRAMEWORK/MoltenVK is not an arm64 dynamic library" >&2
    echo "       file says: $DESCRIPTION" >&2
    exit 1
    ;;
esac

echo "==> done: native/ios/build/lib/MoltenVK.framework ($(du -h "$FRAMEWORK/MoltenVK" | cut -f1))"
