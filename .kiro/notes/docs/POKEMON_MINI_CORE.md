# cMini: Continuum's Pokémon Mini emulator

cMini is the standalone Rust crate `cores/continuum-mini`, core ID `continuum_mini`, library `continuum_mini_libretro_ios.dylib`, and setting prefix `cmini_`. It is Continuum's own implementation. PokeMini remains the first and default Pokémon Mini choice.

## Machine

- MINX at 4 MHz, with 16-bit registers, banked 24-bit addresses, all primary and CE/CF extension bytes assigned measured cycle costs, interrupts, stack, arithmetic, branches, and direct/memory forms.
- The console map: 4 KiB BIOS, 4 KiB RAM/LCD working memory, 256 hardware registers, and cartridge through the 2 MiB address limit.
- Seconds, 256 Hz, and six programmable timer halves, including reload and interrupt flags.
- PRC background map, 24 sprites with mask/flip/invert behavior, direct LCD commands, the 0x18/0x39/0x42 frame phases, and the native 96×64, 72 Hz output.
- Timer-3 PWM sound at 44.1 kHz, stereo duplication for libretro, piezo response and optional low-pass filtering.
- 8 KiB 24C64 save memory on its SDA/SCL pins. cMini exposes exactly 8192 bytes as libretro save RAM, the same `.eep` payload PokeMini exposes, so switching cores preserves in-game progress. The host's existing failed-save guard prevents an invalid load from being replaced by blank memory.
- A/B/C, D-pad, Power, shock, motor/rumble and infrared register lines. With no infrared peer the line is deterministic and idle.

## Starting

With `cmini_bios` off, cMini initializes the documented console register state and starts at cartridge entry `$002102`. With it on, cMini uses only a player's valid 4096-byte `bios.min` and starts from its reset vector. An absent or wrong-sized BIOS falls back to direct game start. There is no bundled BIOS and no made-up startup picture.

## States and memory

States begin with `CMINISTA`, format 1, and carry the cartridge hash. Wrong-machine, malformed, truncated and other-cartridge states are refused before replacing the live machine. States include CPU, timers, PRC/LCD, audio filter history, EEPROM transaction state, infrared and LCD persistence. System RAM and the PokeMini-compatible EEPROM are exposed for achievements, cheats and in-game saving.

## Reference evidence

`examples/reference.rs` writes only into the supplied scratch directory. It checks the MAME executable is 0.289, runs an original stand-in BIOS/cartridge on MAME and cMini, and compares the resulting MINX memory event. It also checks an arithmetic trace, the 4 MHz/72 Hz timer and PRC phases, a direct-LCD frame hash, 44.1 kHz audio phase, EEPROM size and a serialized replay. The verification output records no mismatch. A separate scratch `NativeLibretroCore` runner opened the same GPL-covered stand-in on PokeMini and cMini: both reached its RAM marker, produced picture and sound, and replayed a state; cMini also imported the exact 8192-byte `.eep` PokeMini wrote. No MAME/PokeMini source, BIOS, game, capture or generated output is tracked.

## Known boundary

The Linux checks prove the core and app wiring. The first iPhone build and play session remain the production proof for touch controls, phone rumble, achievements service integration, and a player's games. Until that happens this work is "Built, untested", not phone-proved Done.
