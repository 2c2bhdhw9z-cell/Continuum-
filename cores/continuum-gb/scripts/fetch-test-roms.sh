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
# A GitHub repository at one exact commit, into directory $3. git first; if git cannot reach
# GitHub (some sandboxes only let plain HTTPS downloads through), the same commit as a tarball.
fetch_repo() {
  local repo="$1" sha="$2" dir="$3"
  rm -rf "$dir"
  if git clone -q "https://github.com/$repo" "$dir" 2>/dev/null && git -C "$dir" checkout -q "$sha"; then
    return 0
  fi
  rm -rf "$dir"
  mkdir -p "$dir"
  if curl -fsSL "https://codeload.github.com/$repo/tar.gz/$sha" | tar -xz -C "$dir" --strip-components=1; then
    return 0
  fi
  # Nothing half-downloaded is left behind for the `[ ! -d ... ]` checks to mistake for a copy.
  rm -rf "$dir"
  return 1
}
# Game Boy Advance: jsmolka/gba-tests (MIT, ships built ROMs), pinned.
if [ ! -d jsmolka ]; then
  fetch_repo jsmolka/gba-tests a7113b67e63f83a9b321696ddd7042ccfad6c881 jsmolka || echo "gba-tests download failed"
fi
ls
# Mealybug Tearoom tests (MIT): mid-scanline register changes, for the exact (pixel-by-pixel)
# drawer. Pinned: the expected pictures and the pass list in tests/roms.rs belong to this commit.
if [ ! -d mealybug ]; then
  fetch_repo mattcurrie/mealybug-tearoom-tests 70e88fb90b59d19dfbb9c3ac36c64105202bb1f4 mealybug-src \
    && mkdir -p mealybug && (cd mealybug && unzip -oq ../mealybug-src/mealybug-tearoom-tests.zip) \
    && cp -r mealybug-src/expected mealybug/expected && cp mealybug-src/LICENSE mealybug/ \
    && rm -rf mealybug-src || echo "mealybug download failed (optional)"
fi
# Real Game Boy Advance homebrew games, for tests/homebrew.rs: whole games, not test ROMs, so the
# Advance side is run the way a player runs it. Every one is published free by its author under an
# open licence and pinned to the release the test was written against; never a commercial
# cartridge, and none of them is committed (this whole folder is git-ignored). Each is optional:
# a host that is down must not stop the rest of the suite, so a failed download is just reported.
mkdir -p homebrew
try() { [ -s "homebrew/$2" ] || curl -fsSL "$1" -o "homebrew/$2" || { rm -f "homebrew/$2"; echo "homebrew/$2 not downloaded (optional)"; }; }
try https://raw.githubusercontent.com/ZeroDayArcade/Pong-Homebrew-GBA/c784b6036a4f188c50932b411e98126bfcbd07d6/Pong-Homebrew-GBA.gba pong.gba          # Pong, MIT
try https://github.com/rawbits2010/Wigly-Hunt/releases/download/v0.1/wigly_hunt_-_LD48.gba wigly-hunt.gba                                            # Wigly Hunt, GPL-3.0
try https://github.com/Godless-1/everything-counts/releases/download/v0.1.0-alpha/everything-counts-v0.1.0-alpha.gba everything-counts.gba           # Everything Counts, AGPL-3.0
try https://github.com/copyrat90/sym_merged/releases/download/Day-44/sym_merged.gba sym-merged.gba                                                   # Symbol Merged, MIT
try https://github.com/evanbowman/skyland-gba/releases/download/2022.1.7.0/Skyland.gba skyland.gba                                                   # Skyland, MPL-2.0
# Attack on Voxelburg (GPL-3.0) ships as an archive; only the game itself is kept.
if [ ! -s homebrew/voxelburg.gba ]; then
  (curl -fsSL https://github.com/MutantStargoat/voxelburg/releases/download/jam/voxelburg_jam.tar.gz -o homebrew/voxelburg.tar.gz \
    && tar -xzf homebrew/voxelburg.tar.gz -C homebrew --strip-components=1 voxelburg_jam/voxelburg.gba \
    && rm -f homebrew/voxelburg.tar.gz) || { rm -f homebrew/voxelburg.tar.gz; echo "homebrew/voxelburg.gba not downloaded (optional)"; }
fi
