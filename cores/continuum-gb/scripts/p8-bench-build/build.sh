#!/usr/bin/env bash
# Build the P8 heavy benchmark ROM (Mode 2 affine + alpha blend + 128 sprites)
# with a stock arm-none-eabi-gcc — no devkitARM, no libgba. Reuses the mgba-suite
# build's crt0, linker script and gbafix (the Nintendo logo comes from hello.gba).
set -euo pipefail
CRATE="$(cd "$(dirname "$0")/../.." && pwd)"
BUILD="$CRATE/scripts/p8-bench-build/build"
mkdir -p "$BUILD"
CC="arm-none-eabi-gcc"
CFLAGS="-O2 -mthumb -mthumb-interwork -march=armv4t -mcpu=arm7tdmi -ffunction-sections -fdata-sections -Wall -Wno-unused-variable -Wno-unused-function"
$CC $CFLAGS -c "$CRATE/scripts/p8-bench-build/main.c" -o "$BUILD/main.o"
$CC $CFLAGS -c "$CRATE/scripts/p8-bench-build/irq.c" -o "$BUILD/irq.o"
$CC -mthumb-interwork -march=armv4t -mcpu=arm7tdmi -c "$CRATE/scripts/mgba-suite-build/crt0.s" -o "$BUILD/crt0.o"
$CC -mthumb -mthumb-interwork -march=armv4t -mcpu=arm7tdmi -nostartfiles \
  -specs=nosys.specs -T "$CRATE/scripts/mgba-suite-build/gba.ld" \
  "$BUILD/crt0.o" "$BUILD/main.o" "$BUILD/irq.o" -o "$BUILD/heavy.elf" -lc -lgcc
arm-none-eabi-objcopy -O binary "$BUILD/heavy.elf" "$BUILD/heavy.gba"
python3 "$CRATE/scripts/mgba-suite-build/gbafix.py" \
  "$CRATE/test-roms/jsmolka/ppu/hello.gba" "$BUILD/heavy.gba"
arm-none-eabi-size "$BUILD/heavy.elf"
ls -la "$BUILD/heavy.gba"
echo "BUILD OK: $BUILD/heavy.gba"
