# cAtari (Atari 7800): settings inventory

Every setting the 7800 emulators that matter offer, and what cAtari does with each: "done" (a cAtari
setting, key named), "app" (the Continuum app does it for every core), "n/a" (not about the 7800, or
the emulator's own computer screen). Read from each emulator's own code at the commit given
(27 October 2026); keys and wording are theirs, cAtari's are its own.

- ProSystem (libretro; the app's current 7800 core, GPL v2): 4 settings. [8a88014](https://github.com/libretro/prosystem-libretro/blob/8a88014287c7a01cd568067e5a557d0a2b2a051f/core/libretro_core_options.h)
- MAME 0.289's a7800: the two difficulty switches, the BIOS choice, the High Score Cartridge as a
  cartridge slot. [src/mame/atari/a7800.cpp](https://github.com/mamedev/mame/blob/mame0289/src/mame/atari/a7800.cpp)
- a7800 (the 7800 community's standalone, MAME-based): MAME's, plus a machine per palette (NTSC Hot,
  Warm, Cool; PAL Cool, Warm, Hot), a developer mode, POKEY at $0800, BankSet boards, the XM.
  [7a2afdc](https://github.com/7800-devtools/a7800/tree/7a2afdc1ea08fc331b16b750d8c1f02d4ef62fc8)
- BizHawk's A7800Hawk: the controller in each port (unplugged, 2600 joystick, ProLine, light gun).
  [d4562a6](https://github.com/TASEmulators/BizHawk/tree/d4562a64b70d1948ac9c9433c5178ce3cea053b1/src/BizHawk.Emulation.Cores/Consoles/Atari/A7800Hawk)
- JS7800 (in a browser): screen size, aspect ratio, full screen, palette (ProSystem's, Cool/Warm/Hot
  each dark or light), screen filter, saving high scores (on the device or its server), keyboard and
  gamepad mapping, the XM, frame skipping, vertical sync. [fe918ce](https://github.com/raz0red/js7800/blob/fe918ce422bc130972c45510809f2caec4450efe/site/src/js/settings-dialog.js)
- EMU7800: touch controls shown and their spacing; the rest is per game (controllers, console type).
  [bc63652](https://github.com/emu7800/emu7800.github.io/blob/bc636528d5d19b1af1551ebb27f1d450b2e6fa8a/src/shell/Services/Dto/ApplicationSettings.cs)

## In cAtari (27 October)

26 settings (`cores/continuum-atari/src/options.rs`), in four groups: System (TV system, console
start-up screen, High Score Cartridge, slow chip access), Picture (colours, colour strength,
brightness, whole picture, smooth flicker), Sound (volume, console sound, cartridge sound, soften
the sound and its strength, sample rate), Controls (controller 1 and 2, dual-stick games, the two
difficulty switches and how their buttons work, opposite directions, light gun crosshair, its colour,
its aim both ways).

## Key by key

| Emulator | Setting | cAtari | Note |
|---|---|---|---|
| ProSystem | `prosystem_color_depth` (16-bit, 24-bit) | n/a | cAtari always sends full colour |
| ProSystem | `prosystem_low_pass_filter` | done | `catari_lowpass` |
| ProSystem | `prosystem_low_pass_range` (5-95%) | done | `catari_lowpass_level`, the same steps |
| ProSystem | `prosystem_gamepad_dual_stick_hack` | done | `catari_dual_stick` |
| MAME, a7800 | Left / Right Difficulty Switch | done | `catari_left_difficulty`, `catari_right_difficulty`, and the L and R buttons (`catari_difficulty_buttons`: flip or hold) |
| MAME, a7800 | BIOS (Atari 7800, or the prototype with Asteroids) | done | `catari_bios` plays the player's own BIOS (ProSystem's file names); the prototype is MAME's own dump: n/a |
| MAME, a7800 | High Score Cartridge (a cartridge slot) | done | `catari_hsc` (Auto: games whose header asks; Every game; Off), the player's own highscre.bin |
| a7800 | Palette machines: NTSC Hot, Warm, Cool; PAL Cool, Warm, Hot | done | `catari_palette` (Atari, Cool, Warm, Hot, PAL console, black and white) with `catari_saturation` and `catari_brightness`; our own colour model, fitted to MAME's measured palettes |
| a7800 | Developer mode | n/a | for writing 7800 programs on a computer |
| a7800 | Controllers per port (joystick, light gun, paddles, others) | done | `catari_port1` (Auto, joystick, light gun, paddles), `catari_port2` (Auto, joystick, paddles, SaveKey/AtariVox); trak-ball, mice and keypads are 2600 or rare: gap |
| a7800 | XM expansion module | gap | XM games play their SuperGame part; the XM's own RAM, POKEY and YM2151 sound chip are later |
| BizHawk | Port 1 / Port 2 (unplugged, standard 2600 stick, ProLine, light gun) | done | the 2600 stick reads as a one-button ProLine on the 7800; "unplugged" is the app's |
| JS7800 | Screen size, aspect ratio, full screen, filter | app | the app's picture size, shape and screen filters (cAtari reports the picture's shape) |
| JS7800 | Palette: ProSystem's, Cool/Warm/Hot dark and light | done | `catari_palette` plus brightness for dark or light; ProSystem's own table isn't copied (colour strength gives the brighter look) |
| JS7800 | Save scores, where, local fallback | done / app | high scores keep in the High Score Cartridge or SaveKey, saved by the app with the game; an online score server is n/a |
| JS7800 | Keyboard and gamepad mapping | app | the app's controls |
| JS7800 | Expansion module (XM): Auto, on, off | gap | as a7800's XM above |
| JS7800 | Frame skipping, vertical sync | app | the app's speed and display |
| EMU7800 | Touch controls shown, their spacing | app | the app's touch controls |

## cAtari's own, on top

- Slow chip access (`catari_slow_io`): the real board's 1.19 MHz cycles for the TIA and RIOT (Auto:
  the light gun games, as a7800 does; MAME and ProSystem leave it out).
- Separate volumes for the console's sound and the cartridge's POKEY, and the sample rate.
- Whole picture (`catari_overscan`), smooth flicker (`catari_blend`, light or half and half), opposite
  directions, the light gun's crosshair (on/off, eight colours) and aim (both ways).
- The SaveKey/AtariVox, and the boards beyond MAME's (BankSet, Souper, POKEY at $0440 and $0800), with
  no setting needed: the header names them.

## Gaps, and why

- The XM expansion module (its RAM, POKEY and YM2151): later, as a whole piece; its games still play
  their SuperGame part.
- The Souper board's music player (Rikki & Vikki's music, which ProSystem plays from the game's .cdf
  files): later; the game plays, without its music.
- Trak-ball, ST and Amiga mice, 2600 keypads and driving controllers in the header's controller list:
  later; no 7800 game needs them, and the app's touch controls have no stand-in for them yet.
