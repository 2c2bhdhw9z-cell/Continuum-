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
# Blargg's OAM corruption (OAM bug) tests: the combined ROM and the 8 singles. Pinned in
# tests/roms.rs (blargg_oam_bug*); they exercise the DMG Mode-2 OAM corruption model.
mkdir -p oam_bug
fetch $B/oam_bug/oam_bug.gb oam_bug/oam_bug.gb
for n in 1-lcd_sync 2-causes 3-non_causes 4-scanline_timing 5-timing_bug 6-timing_no_bug \
         7-timing_effect 8-instr_effect; do
  fetch "$B/oam_bug/rom_singles/$n.gb" "oam_bug/$n.gb"
done
# Blargg's sound tests, each on its own (dmg_sound on a Game Boy, cgb_sound on a Color).
mkdir -p sound
for n in 01-registers "02-len ctr" 03-trigger 04-sweep "05-sweep details" "06-overflow on trigger" \
         "07-len sweep period sync" "08-len ctr during power" "09-wave read while on" \
         "10-wave trigger while on" "11-regs after power"; do
  enc=$(printf '%s' "$n" | sed 's/ /%20/g')
  fetch "$B/dmg_sound/rom_singles/$enc.gb" "sound/dmg_sound_$n.gb"
  fetch "$B/cgb_sound/rom_singles/$enc.gb" "sound/cgb_sound_$n.gb"
done
fetch "$B/dmg_sound/rom_singles/12-wave%20write%20while%20on.gb" "sound/dmg_sound_12-wave write while on.gb"
fetch "$B/cgb_sound/rom_singles/12-wave.gb" "sound/cgb_sound_12-wave.gb"
fetch https://github.com/mattcurrie/dmg-acid2/releases/download/v1.0/dmg-acid2.gb dmg-acid2.gb
fetch https://raw.githubusercontent.com/mattcurrie/dmg-acid2/master/img/reference-dmg.png dmg-acid2-reference.png
# The same test on a Game Boy Color in Game Boy mode, in the colours its start-up code picks.
fetch https://raw.githubusercontent.com/mattcurrie/dmg-acid2/master/img/reference-cgb.png dmg-acid2-reference-cgb.png
fetch https://github.com/mattcurrie/cgb-acid2/releases/download/v1.1/cgb-acid2.gbc cgb-acid2.gbc
fetch https://raw.githubusercontent.com/mattcurrie/cgb-acid2/master/img/reference.png cgb-acid2-reference.png
if [ ! -d mts ]; then
  curl -fsSL https://gekkio.fi/files/mooneye-test-suite/mts-20240926-1737-443f6e1/mts-20240926-1737-443f6e1.tar.xz -o mts.tar.xz \
    && python3 -c 'import tarfile,os; t=tarfile.open("mts.tar.xz"); t.extractall("mts-x"); d=os.listdir("mts-x")[0]; os.rename("mts-x/"+d,"mts"); os.rmdir("mts-x")' \
    && rm mts.tar.xz || echo "mooneye download failed (optional)"
fi
# gbmicrotest (aappleby/GBMicrotest, MIT): cycle-exact register/PPU/CPU microtests, reporting
# through HRAM $FF80-$FF82. Prebuilt in c-sp/game-boy-test-roms v7.0; only that folder is kept.
# Pinned: the pass list in tests/roms.rs (GBMICROTEST_PASSING) belongs to this release.
if [ ! -d gbmicrotest ]; then
  curl -fsSL https://github.com/c-sp/game-boy-test-roms/releases/download/v7.0/game-boy-test-roms-v7.0.zip -o gb-test-roms-v7.zip \
    && unzip -oq gb-test-roms-v7.zip "gbmicrotest/*" \
    && rm gb-test-roms-v7.zip || { rm -rf gbmicrotest gb-test-roms-v7.zip; echo "gbmicrotest download failed (optional)"; }
fi
# FuzzARM (DenSinH/FuzzARM, MIT): random ARM/Thumb instruction test ROMs, prebuilt in the
# repo. ARMWrestler (destoer/armwrestler-gba-fixed, MIT) is menu-driven, so it is fetched for
# manual use but not run headless. Pinned: FUZZARM_PASS in tests/gba.rs.
mkdir -p fuzzarm armwrestler
for f in ARM_Any ARM_DataProcessing THUMB_Any THUMB_DataProcessing FuzzARM; do
  fetch "https://raw.githubusercontent.com/DenSinH/FuzzARM/master/$f.gba" "fuzzarm/$f.gba"
done
fetch https://raw.githubusercontent.com/destoer/armwrestler-gba-fixed/master/armwrestler-gba-fixed.gba armwrestler/armwrestler-gba-fixed.gba
fetch https://raw.githubusercontent.com/destoer/armwrestler-gba-fixed/master/armwrestler.gba armwrestler/armwrestler.gba
# AGBEEG Aging Cartridge (zaydlang/AGBEEG-Aging-Cartridge, MIT): the clean-room reimplementation
# of Nintendo's AGS Aging Cartridge, prebuilt in its releases. It reports through a CI MMIO
# register (see tests/gba.rs, ags_aging_tests). The original Nintendo AGS Aging Cartridge is
# internal Nintendo IP and is not fetched.
mkdir -p ags-aging
fetch https://github.com/zaydlang/AGBEEG-Aging-Cartridge/releases/download/v0.0.2/AGBEEG_AGING_CARTRIDGE.gba ags-aging/AGBEEG_AGING_CARTRIDGE.gba
# mGBA test suite (mgba-emu/suite, MPL-2.0): BIOS math, DMA, IO, memory, SIO, timers, video.
# The repo is cloned here, but there is NO prebuilt suite.gba (no releases) and building it
# needs devkitARM (libgba, crt0, linker script, gbafix, grit). apt.devkitpro.org is
# unreachable from this VM (403) and there is no Docker, so the suite cannot be built here
# (P3, 2026-11-24; re-checked P5 Item 3, 2026-11-25: no libgba.a exists anywhere in the
# repo, scripts/mgba-suite-build/ holds only a stub — 12 libgba headers, crt0.s, gba.ld
# and 3 of libgba's sources — and grit/gbafix are absent; fetching grit/libgba sources is
# outside the approved fetch list). Do not claim it passed until it is built and run.
if [ ! -d mgba-suite ]; then
  fetch_repo mgba-emu/suite e6942030d25ffe3ba76c72b73a86da073ec857cc mgba-suite || echo "mgba suite download failed (optional)"
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
# nba-emu/hw-test (MIT): hardware tests for the Advance, prebuilt ROMs. Pinned to
# master 3932a0e5 — verified 2026-11-25: a fresh tarball of that commit diffs clean
# against the local copy. Pinned: HW_TEST_PASSING in tests/gba.rs (4 of 25 pass).
# Note the project has since moved to Codeberg; the GitHub copy is the pinned one.
if [ ! -d hw-test ]; then
  fetch_repo nba-emu/hw-test 3932a0e54dbb5d94b84798d2d5e462e080b95599 hw-test || echo "hw-test download failed (optional)"
fi
# SingleStepTests/ARM7TDMI (MIT): single-step CPU test vectors, 45 .json.bin files
# (20,000 vectors each) plus transcode_json.py. Pinned to master e3097d88 — verified
# 2026-11-25: all 49 files in a fresh tarball of that commit hash-match the local copy.
# NOT wired into tests/gba.rs yet: the vectors need a data-driven harness (binary parser
# + single-instruction retire + bus-transaction compare) that the core does not have.
if [ ! -d sst ]; then
  fetch_repo SingleStepTests/ARM7TDMI e3097d88d428752736b949d94c094d5da50f0db6 sst || echo "SingleStepTests download failed (optional)"
fi
# Game Boy Advance: jsmolka/gba-tests (MIT, ships built ROMs), pinned.
if [ ! -d jsmolka ]; then
  fetch_repo jsmolka/gba-tests a7113b67e63f83a9b321696ddd7042ccfad6c881 jsmolka || echo "gba-tests download failed"
fi
ls
# Alyosha's Game Boy Advance timing tests (MIT; jsmolka's collection with cycle-exact tests of the
# prefetch buffer, wait states, DMA, timers, interrupts and serial added, measured on a console).
# Pinned: the pass list in tests/gba.rs belongs to this commit.
if [ ! -d alyosha ]; then
  fetch_repo alyosha-tas/gba-tests 66f5f1d60cd7030b600606886c76f5c821ecaf9d alyosha || echo "alyosha gba-tests download failed (optional)"
fi
# SameSuite's sound tests (MIT): built from source with rgbds (its release for Linux), both pinned.
# They read the channels back a sample at a time on a Color. Optional, like the rest.
if [ ! -d samesuite-apu ]; then
  (set -e
   fetch_repo LIJI32/SameSuite f15645fb049a47ea235f6d2c9a033e72d8087901 samesuite-src
   mkdir -p rgbds && curl -fsSL https://github.com/gbdev/rgbds/releases/download/v1.0.4/rgbds-linux-x86_64.tar.xz | tar xJ -C rgbds
   PATH="$PWD/rgbds:$PATH" make -s -C samesuite-src >/dev/null
   mkdir -p samesuite-apu && find samesuite-src/apu -name '*.gb' -exec cp {} samesuite-apu/ \;
   rm -rf samesuite-src) || { rm -rf samesuite-apu samesuite-src; echo "SameSuite build failed (optional)"; }
fi
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

# afska's gba-link-connection v8.0.3 (MIT): the LinkWireless demo, which tests/wireless.rs runs on
# two Advances to check the Wireless Adapter. Optional.
mkdir -p test-roms/gba-link
if [ ! -f test-roms/gba-link/LinkWireless_demo.gba ]; then
  curl -fsSL -o test-roms/gba-link/LinkWireless_demo.gba \
    https://github.com/afska/gba-link-connection/releases/download/v8.0.3/LinkWireless_demo.gba \
    || echo "LinkWireless demo not fetched (tests/wireless.rs will say SKIPPED)"
fi
