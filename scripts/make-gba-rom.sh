#!/usr/bin/env bash
# Builds web/roms/gba-testcart.gba from web/roms/src/.
#
# Why an original ROM: verifying "ROM → core → bridge → WebGPU/audio → screen" needs
# content, and commercial ROMs cannot be shipped. This one is ours — see
# web/roms/src/gba-testcart.c for what it draws and why.
#
# Uses whatever clang is on PATH (LLVM ships the ARM backend, so no separate
# arm-none-eabi toolchain is needed) plus ld.lld and llvm-objcopy.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SRC="$ROOT/web/roms/src"
OUT="$ROOT/web/roms/gba-testcart.gba"
WORK="$ROOT/.work/gba-rom"

for tool in clang ld.lld llvm-objcopy; do
  command -v "$tool" >/dev/null 2>&1 || { echo "error: $tool not found" >&2; exit 1; }
done

mkdir -p "$WORK"

# armv4t/arm7tdmi is the GBA's CPU. `-marm` because the entry stub is ARM code and
# thumb interworking would complicate the branch at offset 0.
CFLAGS=(
  --target=armv4t-none-eabi
  -mcpu=arm7tdmi
  -marm
  -O2
  -ffreestanding
  -fno-builtin
  -fomit-frame-pointer
  -Wall
  -Wextra
)

echo "==> compiling"
clang "${CFLAGS[@]}" -c "$SRC/gba-entry.S" -o "$WORK/entry.o"
clang "${CFLAGS[@]}" -c "$SRC/gba-testcart.c" -o "$WORK/main.o"

echo "==> linking"
ld.lld -T "$SRC/gba.ld" -o "$WORK/testcart.elf" "$WORK/entry.o" "$WORK/main.o"

echo "==> extracting cartridge image"
llvm-objcopy -O binary "$WORK/testcart.elf" "$WORK/testcart.gba"

echo "==> patching header"
python3 - "$WORK/testcart.gba" "$OUT" <<'PY'
import sys, pathlib

src, dst = pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2])
rom = bytearray(src.read_bytes())

# A cartridge image must cover the full 0xC0-byte header.
if len(rom) < 0xC0:
    rom.extend(b"\x00" * (0xC0 - len(rom)))

def put(offset, data):
    rom[offset:offset + len(data)] = data

put(0xA0, b"CONTINUUMTST")      # game title, 12 bytes
put(0xAC, b"CTST")              # game code
put(0xB0, b"CN")                # maker code
rom[0xB2] = 0x96                # fixed value; mgba checks this one
rom[0xB3] = 0x00                # main unit code
rom[0xB4] = 0x00                # device type
put(0xB5, b"\x00" * 7)          # reserved
rom[0xBC] = 0x00                # software version

# Header checksum: real hardware refuses to boot without it, even though mgba does not
# check it. Getting it right costs three lines and makes the ROM valid on a real GBA.
checksum = 0
for byte in rom[0xA0:0xBD]:
    checksum = (checksum - byte) & 0xFF
rom[0xBD] = (checksum - 0x19) & 0xFF

# Entry point must be an ARM branch: mgba's GBAIsROM tests exactly this byte.
if rom[3] != 0xEA:
    raise SystemExit(f"entry word is not an ARM branch (byte 3 = 0x{rom[3]:02X})")

# Pad to a 4-byte boundary; flash carts and some tools expect it.
while len(rom) % 4:
    rom.append(0x00)

dst.parent.mkdir(parents=True, exist_ok=True)
dst.write_bytes(bytes(rom))
print(f"wrote {dst.relative_to(dst.parents[2])}: {len(rom)} bytes")
print(f"  entry branch: 0x{rom[3]:02X}  fixed byte 0xB2: 0x{rom[0xB2]:02X}  checksum: 0x{rom[0xBD]:02X}")
print("  behaviour:    blue grid; A -> yellow; Left/Right move the marker; 440 Hz tone")
PY
