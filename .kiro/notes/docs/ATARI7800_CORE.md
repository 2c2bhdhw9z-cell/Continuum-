# cAtari: Continuum's own Atari 7800 emulator

**Where it stands (27 October 2026): finished, in the app as a second choice next to ProSystem**
(Settings, Cores, Core for Atari 7800 games), not yet seen on a phone. ProSystem stays the default
until the owner has played it. The job list is `.kiro/notes/tasks/atari7800.json` (A1-A9).

Why it exists: the owner's order of 25 October, a core of our own for each system, as good as the
outside one it replaces and with more settings, written from scratch (licences: no outside
emulator's code). Asked for the next easiest core on 27 October, the owner chose the 7800 ("Then do
the Atari 7800."). Our own Rust, GPL-3.0-only, `cores/continuum-atari` (crate `continuum_atari`,
core id `continuum_atari`, library `continuum_atari_libretro_ios.dylib`, settings `catari_`).

## What it plays

Every 7800 cartridge in .a78 form (the header names the board) or as a plain ROM (found by its
contents in MAME's CC0 list of 189 cartridges, `src/cart_list.rs`, or by its size), and ProSystem's
.cdf descriptions. The boards: plain ROMs up to 48 KB, SuperGame (with RAM, with nine banks, with
bank 6 at $4000), Absolute (F-18 Hornet), Activision (Double Dragon, Rampage), the mirror-RAM
prototype, VersaBoard and MegaCart+, BankSet in all its forms (flat, with RAM, SuperGame, 52 KB),
the Souper board (Rikki & Vikki) with its music player, a POKEY at $4000, $0440, $0450 or $0800
(two at once on the dual boards), a Covox, a YM2151 on the cartridge, and the XM expansion module
and XBoarD. With them: the High Score Cartridge (the
player's own highscre.bin), the SaveKey/AtariVox, the two-button and one-button joysticks, the
light gun games, paddles, Pause, Select, Reset and both difficulty switches, NTSC and PAL. Games
start straight away; the player's own BIOS (ProSystem's file names) can play the Atari logo first.

## How it's built (`cores/continuum-atari/src/`)

- `m6502.rs`: the 6502 (Atari's SALLY), every opcode including the undocumented ones, each bus
  cycle in the chip's order, interrupts polled where the chip polls them.
- `machine.rs`: the memory map, time in MARIA clocks (a processor cycle is 4, a line 454, a frame
  263 lines on NTSC and 313 on PAL), MARIA's DMA holding the processor, WSYNC, the slow 1.19 MHz
  cycles for the TIA and RIOT (a setting), INPTCTRL and its lock, the controllers, the sound mix.
  The console starts in the line after the picture (251 NTSC, 295 PAL), as MAME's does, and each
  frame runs from there.
- `maria.rs`: display lists, every read mode (160A/B, 320A-D, kangaroo), holey DMA, character mode,
  display list interrupts and the DMA cost of each.
- `tia.rs`, `riot.rs`, `pokey.rs`: the TIA's sound and buttons, the RIOT (timers as Stella measures
  the 6532), POKEY (four channels, 4/5/9/17-bit polynomials, three clocks, joined channels,
  high-pass filters, two-tone and asynchronous modes). A POKEY's or a YM2151's interrupt reaches
  the processor only when the cartridge's header says it is wired.
- `cart.rs`: headers (version 3 and version 4), the known list, every board, what MARIA sees on the
  BankSet and Souper boards.
- `xm.rs`, `ym2151.rs`: the XM expansion module and the XBoarD (128 KB of RAM in 8 KB pages, the
  processor and MARIA each seeing their own, its POKEY at $0450, its High Score part, and the five
  control registers the community documents), and its YM2151 FM music chip: eight channels of four
  operators, the eight ways to connect them, the LFO, the noise generator and the two timers.
- `coretone.rs`, `bupchip.rs`: the Souper board's music player, the "BupChip" that plays Rikki &
  Vikki's music. The game writes one-byte commands to $8007 (play song n, stop, pause, carry on,
  volume, reset). The board plays the songs from the game's own sample, instrument and song files
  (the .cdf names them) with CoreTone, Osman Celimli's wavetable synthesizer: sixteen channels of
  8-bit samples into 48 kHz stereo, its scripts stepping 240 times a second. `coretone.rs` is
  CoreTone 1.2.2cz translated to Rust, under its zlib licence. It keeps the notice and is marked
  as altered, with the changes listed at its top (only the music part, and safe answers where the
  C would crash or hang). The app's open-source credits name it.
- `savekey.rs`: the SaveKey's 24LC256 EEPROM, by the data sheet.
- `palette.rs`: our own YIQ colour model, fitted to MAME's measured palettes (NTSC "Atari", PAL,
  and the warm-up stages Cool, Warm, Hot).
- `libretro.rs`, `options.rs`: the core: 29 settings (`.kiro/notes/docs/A7800_OPTIONS_INVENTORY.md`),
  save states, battery saves (the High Score Cartridge's RAM, then the SaveKey's 32 KB), memory for
  achievements and cheats, the picture's shape.

## What the tests prove (`cargo test --release` in the crate)

- The 6502: all 2,560,000 SingleStepTests cases match cycle for cycle; Klaus Dormann's functional
  test passes (`tests/m6502_*.rs`; `scripts/fetch-test-suites.sh` pins both).
- Against MAME 0.289's a7800, with our own test programs (`test-programs/*.asm`, assembled with dasm
  2.20.14.1 by `scripts/build-test-programs.sh`) and our own stand-in BIOS; references made by
  `tests/mame/make_refs.py`, only hashes committed:
  - dot for dot on every frame (`tests/mame_frames.rs`): zones 120 frames, DMA timing 60 (the
    processor counting its own cycles under every kind of DMA load), every read mode 70, every
    control 110, five bank-switching boards 20 each, the High Score Cartridge 12, and on the PAL
    console (a7800p, 313 lines) zones 60 and every read mode 70 (MAME's PAL palette shows hue 15
    as hue 1, so those two count as one there);
  - the TIA's sound, all 204,800 samples of every mode at many pitches and volumes
    (`tests/tia_mame.rs`);
  - POKEY's sound, every one of 6.2 million clocks, through MAME's own recording (3.1 million
    samples, none more than 8 out of 32,768; `tests/pokey_mame.rs`);
  - the XM's YM2151, all 223,878 samples of a test that plays every way the operators connect,
    feedback, pitches across the range, the envelopes, the LFO on all four of its waveforms and the
    noise generator (`tests/ym2151_mame.rs`).
- The music player against the original CoreTone, driven by ProSystem's BupChip layer through
  `tests/coretone/bupchip_harness.c`. All 520,000 samples are the same (`tests/coretone.rs`). The
  test runs made-up sample, instrument and song files through every script command and every
  command the game can write:
  - looped, one-shot and backwards samples;
  - volume and pitch slides, one rising past the top;
  - nested and endless loops, and a loop stack overflowing;
  - calls, and a break unwinding sixteen calls deep;
  - sixteen channels at once, loud enough to wrap round;
  - every note, a sample the package lacks, and a song that isn't one.
  The console plays it from a .cdf through $8007, and the app's engine finds the files when they
  sit side by side with mismatched capitals (`catari_frontend.rs`).
- The RIOT's timer against Stella, which has the same chip, on a 2600 port of the same test
  (`tests/riot_stella.rs`): MAME reads INTIM 2-3 lower, from rounding in its clock arithmetic.
- The libretro core end to end (`tests/libretro.rs`) and through the app's engine
  (`crates/emulator-bridge/tests/catari_frontend.rs`, with `CATARI_SO`).
- Unit tests for every board MAME doesn't have (BankSet, Souper, POKEY at $0440/$0800, .cdf), as the
  emulators that play them describe them, and the SaveKey by its data sheet.

## Where MAME isn't the real console, and what the tests allow for

- MAME rounds the console's clocks to whole hertz (MARIA 7,159,090, the processor and POKEY
  1,789,772, the TIA 31,399), so its processor runs 2 parts in 7 million slower than a quarter of
  MARIA, as it is on the board and in cAtari. The POKEY test moves each of our writes to the clock
  MAME's slower processor makes it on; the TIA test runs our TIA at MAME's rate and starts its sound
  clock where MAME's writes land (190 to 205 MARIA clocks all match).
- MAME brings its sound up to date 50 times a second, and a change of level that falls on the
  clock where an update lands comes a clock late; the POKEY test allows for that (once in the test).
- MAME reads POKEY registers as they were when POKEY last caught up with the processor (it runs
  POKEY in slices of time), and loses a processor cycle at each POKEY access. cAtari reads POKEY at
  the exact clock and loses nothing. Writes in the test come just after a WSYNC, where both agree.

## What is approximate, or not there yet

- The music player keeps its own time, as the board's chip does. Its 48,000 samples a second are
  real time on both TV systems. ProSystem steps it four times a frame, which on a PAL console
  plays the music five-sixths as fast.
- A save state carries the music player's whole state, so the music carries on from the same
  sample. ProSystem keeps only the song and restarts it.
- The music's level is our choice: at full swing as loud as the console's own sound, like the XM's
  FM chip. ProSystem adds it at about four times that against the TIA, which clips. It has its own
  volume setting.
- Without its music files the game still plays, silently. ProSystem refuses to load it.
- The XM's RAM paging follows the registers the a7800 emulator and the community document, not
  MAME 0.289, whose XM is a rougher stand-in (one 16 KB window, no separate pages for MARIA): there
  is nothing to compare it against.
- The slow TIA/RIOT cycles slow the processor but not POKEY's clock (on the board POKEY runs on the
  processor's clock); the setting is on by default only for light gun games, which have no POKEY.
- The AtariVox's speech isn't played (no emulator does); its memory is the SaveKey's.
- Not compared with another emulator: BankSet, Souper and POKEY at $0440/$0800 (MAME 0.289 has
  none of them). The light gun has its core unit tests plus the FEAT-002 app-input probe: taps
  follow the exact drawn picture, bars do not fire, and controls keep precedence. A real light-gun
  game is still a phone check. Paddles have their core unit tests; their app movement source is in
  the next FEAT-002 checkpoint.

## Where it's wired in

- `native/ios/ContinuumApp.swift`: `CoreSpec continuumAtari` (BIOS names: the two 7800 BIOS files
  and highscre.bin, so they install as BIOS and never show as games), in `all`, and
  `coreChoices[.atari7800] = [prosystem, continuumAtari]`; the picture's shape comes from the core.
- `scripts/build-core.sh`: three `recipe-start: continuum_atari` regions, `IOS_KIND`
  "cargo-continuum-atari", and `IOS_OPTIONAL_CORES` (a build without it still works).
- The app takes Rikki & Vikki's music files (`.smp`, `.ins`, `.mus`: `CoreCatalog.companionExtensions`,
  importable and backed up, never Library rows). The import doesn't ask what system the .bin a .cdf
  names is (`ImportCenter.finishImport`, the engine's `referenced_files` reading the .cdf). The core
  looks for each file the .cdf names beside it, by its path, then by its name alone, capitals
  ignored, because the app keeps every import side by side.
- `native/ios/project.yml` embeds the dylib; `native/ios/not-in-ios-yml.txt` lists it;
  `scripts/check-core-cache.sh` checks it (229 checks). `TouchControls.swift` gives Atari 7800 the
  renderer's whole-picture pointer for the light gun.
- `native/ios/build-engine.sh` and `.github/workflows/` are untouched, so the core cache stays.

## What's next

1. The owner plays it on the phone (TESTING.md); then cAtari can become the default for the 7800.
2. The same 6502 opens the way to the 2600, 5200 and Lynx.
