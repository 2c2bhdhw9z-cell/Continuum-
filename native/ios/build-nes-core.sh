#!/usr/bin/env bash
# Builds libretro fceumm (NES core) as a native iOS ARM64 dylib.
#
# Run from GitHub Actions macOS runner:
#   ./native/ios/build-nes-core.sh
#
# Produces:
#   native/ios/build/lib/libretro_fceumm.dylib
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
WORK="$ROOT/.work/fceumm-ios"
OUT="$HERE/build/lib"

REPO="https://github.com/libretro/libretro-fceumm.git"
TARGET="arm64-apple-ios"
MIN_IOS_VERSION="16.0"

echo "==> Building fceumm (NES) for iOS ARM64"

mkdir -p "$WORK" "$OUT"

# Clone if not exists
if [ ! -d "$WORK/libretro-fceumm" ]; then
  echo "==> Cloning fceumm from $REPO"
  git clone --depth=1 "$REPO" "$WORK/libretro-fceumm"
fi

cd "$WORK/libretro-fceumm"

# Extract source files from Makefile.common
echo "==> Reading source files from Makefile.common"
echo "CORE_DIR := src" > /tmp/fceumm_vars.mk
cat Makefile.common >> /tmp/fceumm_vars.mk
SOURCES_C=$(make -f /tmp/fceumm_vars.mk --no-print-directory -s -p 2>/dev/null | grep "^SOURCES_C\s*=" | sed 's/^SOURCES_C\s*=\s*//')

# Convert to array
SOURCES=()
for src in $SOURCES_C; do
  SOURCES+=("$src")
done

echo "==> Found ${#SOURCES[@]} source files"

# Compiler flags
CFLAGS=(
  "-target" "$TARGET"
  "-mios-version-min=$MIN_IOS_VERSION"
  "-O2"
  "-fPIC"
  "-DHAVE_ASPRINTF"
  "-DHAVE_STDINT_H"
  "-D__LIBRETRO__"
  "-DPATH_MAX=1024"
  "-DFCEU_VERSION_NUMERIC=9900"
  "-DFRONTEND_SUPPORTS_RGB565"
  "-Isrc/drivers/libretro"
  "-Isrc/drivers/libretro/libretro-common/include"
  "-Isrc"
  "-Isrc/input"
  "-Isrc/boards"
  "-Isrc/mappers"
)

# Compile all sources
OBJECTS=()
mkdir -p build
echo "==> Compiling ${#SOURCES[@]} source files"
for src in "${SOURCES[@]}"; do
  obj="build/$(basename "$src" .c).o"

  echo "    $src"
  clang -c "${CFLAGS[@]}" "$src" -o "$obj"
  OBJECTS+=("$obj")
done

# Link into dylib
DYLIB="$OUT/libretro_fceumm.dylib"
echo "==> Linking $DYLIB"
clang -dynamiclib \
  -target "$TARGET" \
  -mios-version-min=$MIN_IOS_VERSION \
  -install_name "@rpath/libretro_fceumm.dylib" \
  -o "$DYLIB" \
  "${OBJECTS[@]}"

# Verify
if [ ! -f "$DYLIB" ]; then
  echo "error: dylib was not produced" >&2
  exit 1
fi

SIZE=$(du -h "$DYLIB" | cut -f1)
echo "==> Success: $DYLIB ($SIZE)"
file "$DYLIB"
