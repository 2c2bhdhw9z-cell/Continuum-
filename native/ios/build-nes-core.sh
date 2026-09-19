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

# Get source files from the makefile
echo "==> Reading source files from Makefile.common"
SRC_DIR="src"

# fceumm sources (from Makefile.common)
SOURCES=(
  "cart.c" "cheat.c" "crc32.c" "driver.c" "fceu.c" "fds.c"
  "file.c" "filter.c" "ines.c" "input.c" "palette.c" "ppu.c"
  "sound.c" "state.c" "unif.c" "video.c" "vsuni.c" "x6502.c"
  "boards/01-222.c" "boards/09-034a.c" "boards/12in1.c" "boards/15.c"
  "boards/32.c" "boards/33.c" "boards/40.c" "boards/41.c" "boards/42.c"
  "boards/43.c" "boards/46.c" "boards/50.c" "boards/51.c" "boards/62.c"
  "boards/65.c" "boards/67.c" "boards/68.c" "boards/72.c" "boards/77.c"
  "boards/79.c" "boards/80.c" "boards/80013-B.c" "boards/8157.c"
  "boards/8237.c" "boards/88.c" "boards/90.c" "boards/95.c"
  "boards/99.c" "boards/103.c" "boards/106.c" "boards/108.c"
  "boards/112.c" "boards/116.c" "boards/117.c" "boards/120.c"
  "boards/121.c" "boards/133.c" "boards/137.c" "boards/151.c"
  "boards/156.c" "boards/164.c" "boards/168.c" "boards/170.c"
  "boards/175.c" "boards/176.c" "boards/177.c" "boards/178.c"
  "boards/183.c" "boards/185.c" "boards/186.c" "boards/187.c"
  "boards/189.c" "boards/193.c" "boards/199.c" "boards/208.c"
  "boards/222.c" "boards/225.c" "boards/228.c" "boards/230.c"
  "boards/232.c" "boards/234.c" "boards/235.c" "boards/244.c"
  "boards/246.c" "boards/252.c" "boards/253.c" "boards/411120-c.c"
  "boards/830118c.c" "boards/a9746.c" "boards/addrlatch.c"
  "boards/ax5705.c" "boards/bandai.c" "boards/bb.c" "boards/bmc13in1jy110.c"
  "boards/bmc42in1r.c" "boards/bmc64in1nr.c" "boards/bmc70in1.c"
  "boards/bmc70in1b.c" "boards/bmcgk-192.c" "boards/bonza.c"
  "boards/bs-5.c" "boards/cityfighter.c" "boards/coolboy.c"
  "boards/datalatch.c" "boards/deirom.c" "boards/dream.c"
  "boards/edu2000.c" "boards/famicombox.c" "boards/ffe.c"
  "boards/fk23c.c" "boards/ghostbusters63in1.c" "boards/gs-2004.c"
  "boards/gs-2013.c" "boards/h2288.c" "boards/inlnsf.c"
  "boards/karaoke.c" "boards/kof97.c" "boards/konami-qtai.c"
  "boards/ks7012.c" "boards/ks7013.c" "boards/ks7016.c"
  "boards/ks7017.c" "boards/ks7030.c" "boards/ks7031.c"
  "boards/ks7032.c" "boards/ks7037.c" "boards/ks7057.c"
  "boards/le05.c" "boards/lh32.c" "boards/lh53.c" "boards/malee.c"
  "boards/mmc1.c" "boards/mmc2and4.c" "boards/mmc3.c" "boards/mmc5.c"
  "boards/n106.c" "boards/n625092.c" "boards/novel.c"
  "boards/onebus.c" "boards/pec-586.c" "boards/sa-9602b.c"
  "boards/sachen.c" "boards/sheroes.c" "boards/subor.c"
  "boards/super24.c" "boards/supervision.c" "boards/t-262.c"
  "boards/tengen.c" "boards/tf-1201.c" "boards/vrc2and4.c"
  "boards/vrc7.c" "boards/yoko.c"
  "input/arkanoid.c" "input/cursor.c" "input/fkb.c" "input/ftrainer.c"
  "input/hypershot.c" "input/mahjong.c" "input/oekakids.c"
  "input/powerpad.c" "input/quiz.c" "input/shadow.c"
  "input/snesmouse.c" "input/suborkb.c" "input/toprider.c"
  "input/zapper.c"
  "mappers/151.c" "mappers/15.c" "mappers/17.c" "mappers/18.c"
  "mappers/201.c" "mappers/202.c" "mappers/203.c" "mappers/206.c"
  "mappers/212.c" "mappers/213.c" "mappers/214.c" "mappers/215.c"
  "mappers/217.c" "mappers/24and26.c" "mappers/32.c" "mappers/33.c"
  "mappers/40.c" "mappers/41.c" "mappers/42.c" "mappers/43.c"
  "mappers/46.c" "mappers/50.c" "mappers/51.c" "mappers/59.c"
  "mappers/6.c" "mappers/61.c" "mappers/62.c" "mappers/65.c"
  "mappers/67.c" "mappers/69.c" "mappers/71.c" "mappers/72.c"
  "mappers/73.c" "mappers/75.c" "mappers/76.c" "mappers/77.c"
  "mappers/79.c" "mappers/8.c" "mappers/80.c" "mappers/82.c"
  "mappers/83.c" "mappers/85.c" "mappers/86.c" "mappers/89.c"
  "mappers/91.c" "mappers/92.c" "mappers/97.c" "mappers/99.c"
  "mappers/emu2413.c" "mappers/mmc3_6in1.c" "mappers/mmc3_bmcfk23ca.c"
  "mappers/mmc3_malee.c" "mappers/simple.c"
  "drivers/libretro/libretro.c"
)

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
  "-I$SRC_DIR"
  "-I$SRC_DIR/drivers/libretro"
  "-I$SRC_DIR/drivers/libretro/libretro-common/include"
  "-I$SRC_DIR/input"
  "-I$SRC_DIR/boards"
  "-I$SRC_DIR/mappers"
)

# Compile all sources
OBJECTS=()
echo "==> Compiling ${#SOURCES[@]} source files"
for src in "${SOURCES[@]}"; do
  obj="build/$(basename "$src" .c).o"
  mkdir -p "$(dirname "$obj")"

  echo "    $src"
  clang -c "${CFLAGS[@]}" "$SRC_DIR/$src" -o "$obj"
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
