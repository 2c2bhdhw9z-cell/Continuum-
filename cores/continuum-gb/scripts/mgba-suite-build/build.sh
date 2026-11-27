#!/usr/bin/env bash
# Build mgba-emu/suite (test-roms/mgba-suite) into a .gba with a stock
# arm-none-eabi-gcc — no devkitARM, no libgba, no grit, no gbafix.
# grit.py replaces grit, gbafix.py replaces gbafix, and the scaffold headers
# in scripts/mgba-suite-build/include/ replace libgba.
set -euo pipefail
CRATE="$(cd "$(dirname "$0")/../.." && pwd)"
SUITE="$CRATE/test-roms/mgba-suite"
BUILD="$CRATE/scripts/mgba-suite-build/build"
GEN="$BUILD/gen"
mkdir -p "$BUILD" "$GEN"

python3 "$CRATE/scripts/mgba-suite-build/grit.py" "$GEN"

CC="arm-none-eabi-gcc"
CFLAGS="-O2 -mthumb -mthumb-interwork -march=armv4t -mcpu=arm7tdmi \
  -ffunction-sections -fdata-sections \
  -I$CRATE/scripts/mgba-suite-build/include -I$SUITE/include -I$GEN \
  -Wall -Wno-unused-variable -Wno-unused-function"
ASFLAGS="-mthumb-interwork -march=armv4t -mcpu=arm7tdmi \
  -I$SUITE/include"

OBJS=""
for f in "$SUITE"/src/*.c "$CRATE/scripts/mgba-suite-build/src/"*.c; do
  o="$BUILD/$(basename "$f" .c).o"
  $CC $CFLAGS -c "$f" -o "$o"
  OBJS="$OBJS $o"
done
for f in "$GEN"/*.s "$CRATE/scripts/mgba-suite-build/crt0.s"; do
  o="$BUILD/$(basename "$f" .s).o"
  $CC $ASFLAGS -c "$f" -o "$o"
  OBJS="$OBJS $o"
done
# The asm timing tests include macros.s through the C preprocessor.
for f in "$SUITE"/src/*.s "$SUITE"/src/tests/*.s; do
  o="$BUILD/$(basename "$f" .s).o"
  $CC $ASFLAGS -x assembler-with-cpp -c "$f" -o "$o"
  OBJS="$OBJS $o"
done

$CC -mthumb -mthumb-interwork -march=armv4t -mcpu=arm7tdmi -nostartfiles \
  -specs=nosys.specs -T "$CRATE/scripts/mgba-suite-build/gba.ld" \
  -Wl,-Map="$BUILD/suite.map" \
  $OBJS -o "$BUILD/suite.elf" -lc -lgcc

arm-none-eabi-objcopy -O binary "$BUILD/suite.elf" "$BUILD/suite.gba"
python3 "$CRATE/scripts/mgba-suite-build/gbafix.py" \
  "$CRATE/test-roms/jsmolka/ppu/hello.gba" "$BUILD/suite.gba"
arm-none-eabi-size "$BUILD/suite.elf"
ls -la "$BUILD/suite.gba"
echo "BUILD OK: $BUILD/suite.gba"
