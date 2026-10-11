#!/usr/bin/env bash
# Downloads the public Game Boy test ROMs into test-roms/ (git-ignored). None are committed:
# Blargg's ROMs carry no licence, and dmg-acid2 (MIT) is fetched too for one place to look.
set -euo pipefail
cd "$(dirname "$0")/.."
mkdir -p test-roms && cd test-roms
fetch() { [ -s "$2" ] || curl -fsSL "$1" -o "$2"; }
B=https://raw.githubusercontent.com/retrio/gb-test-roms/c240dd7d700e5c0b00a7bbba52b53e4ee67b5f15
fetch $B/cpu_instrs/cpu_instrs.gb cpu_instrs.gb
for n in 01-special 02-interrupts 03-op\ sp,hl 04-op\ r,imm 05-op\ rp 06-ld\ r,r 07-jr,jp,call,ret,rst 08-misc\ instrs 09-op\ r,r 10-bit\ ops 11-op\ a,\(hl\); do
  enc=$(printf '%s' "$n" | sed 's/ /%20/g; s/(/%28/g; s/)/%29/g')
  fetch "$B/cpu_instrs/individual/$enc.gb" "cpu_$n.gb"
done
fetch $B/instr_timing/instr_timing.gb instr_timing.gb
fetch $B/mem_timing/mem_timing.gb mem_timing.gb
fetch $B/halt_bug.gb halt_bug.gb
fetch https://github.com/mattcurrie/dmg-acid2/releases/download/v1.0/dmg-acid2.gb dmg-acid2.gb
fetch https://raw.githubusercontent.com/mattcurrie/dmg-acid2/master/img/reference-dmg.png dmg-acid2-reference.png
fetch https://github.com/mattcurrie/cgb-acid2/releases/download/v1.1/cgb-acid2.gbc cgb-acid2.gbc
fetch https://raw.githubusercontent.com/mattcurrie/cgb-acid2/master/img/reference.png cgb-acid2-reference.png
if [ ! -d mts ]; then
  curl -fsSL https://gekkio.fi/files/mooneye-test-suite/mts-20240926-1737-443f6e1/mts-20240926-1737-443f6e1.tar.xz -o mts.tar.xz \
    && python3 -c 'import tarfile,os; t=tarfile.open("mts.tar.xz"); t.extractall("mts-x"); d=os.listdir("mts-x")[0]; os.rename("mts-x/"+d,"mts"); os.rmdir("mts-x")' \
    && rm mts.tar.xz || echo "mooneye download failed (optional)"
fi
ls
