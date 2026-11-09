# Have any of the emulators been updated?

Checked automatically, weekly. **Last checked: 09 November 2026, 16:55 UTC.**

Written by `scripts/check-core-updates.sh`. Do not edit by hand; it is overwritten.

## What this page is, and is not

Each emulator in Continuum is locked to one exact version. That is deliberate: if the build
took whatever was newest, somebody else's change overnight could break the app with nothing
on this side having changed. The cost is that nothing announces when an emulator has
genuinely improved. This page is that announcement.

It covers every emulator: the ones built here from their author's code, and the ones
downloaded ready-made from the libretro buildbot. For the ones built here it also checks
whether our own patches still fit the author's newest code, so taking an update would not
start with a broken build.

**Nothing here has been taken.** Reading this page changes nothing about the app. Moving a
core to a newer version is a deliberate edit to `scripts/build-core.sh` (or
`scripts/fetch-buildbot-cores.sh` for a downloaded one), then a build, then a test on a
phone. A number in the tables below is an invitation to look, not a problem.

## 17 of 32 have newer code available

Built here. "New changes" counts every commit the author has made since our locked
version. A big number is not automatically important — it can be one real fix among a
hundred tidy-ups.

| Console | New changes since ours | Newest change | Our patches still fit | Our locked version |
| --- | --- | --- | --- | --- |
| **NES** | 5 | 2026-11-02 | — | `ac0d00a7` |
| **NES and SNES, Mesen 2 with Continuum patches** | 3 | 2026-11-09 | yes | `4c4e069f` |
| **SNES, bsnes with a Continuum patch** | 2 | 2026-11-06 | yes | `05439f96` |
| **Master System, Game Gear and SG-1000, Gearsystem with a Continuum patch** (skipped `426d4eb6` on 16 October 2026: only the desktop app and the file-based save code changed; the emulator Continuum builds is the same; newer code since) | 47 | 2026-11-06 | yes | `4a801d22` |
| **Mega Drive, Sega CD and 32X, BlastEm with a Continuum patch** (skipped `2230addc` on 27 October 2026: only its makers' web-browser build changed (a new build job, and two network functions renamed for it, which Continuum never uses); the emulator Continuum builds runs the same code; newer code since) | 2 | 2026-11-08 | yes | `ab743653` |
| **PS1, interpreter only** | 18 | 2026-10-29 | yes | `c8816799` |
| **PlayStation, Beetle PSX HW (Vulkan) — step 4 proof core** (skipped `e57860d8` on 12 October 2026: 3-7% slower on the software renderer the app uses; newer code since) | 74 | 2026-11-01 | — | `5ec9909f` |
| **TurboGrafx-16 / PC Engine, software renderer** (skipped `4eee99c4` on 27 October 2026: only the Spanish and Swedish translations of its settings changed, and Continuum always shows them in English; the emulator Continuum builds is the same; newer code since) | 2 | 2026-10-30 | — | `3f946f27` |
| **Nintendo 64, software rasteriser and interpreter** | 164 | 2026-11-09 | yes | `a8280501` |
| **Nintendo 3DS, Vulkan, interpreter CPU (no JIT)** | 63 | 2026-11-06 | yes | `6daa07ab` |
| **PSP, Vulkan, IR interpreter (no JIT, no dynarec)** | 1511 | 2026-11-09 | yes | `a838053e` |
| **Dreamcast, interpreter only (TARGET_NO_REC), optional** | 30 | 2026-11-08 | yes | `80471d57` |

Downloaded. The buildbot's build is a different file from ours; that alone does not say
what changed in it.

| Console | What the buildbot has | Our locked build |
| --- | --- | --- |
| **Amiga** (skipped `49ef1cd2` on 27 October 2026: same code as ours (5.3.1, 7387937); only rebuilt by the buildbot; newer code since) | newer build (`3eaf6f12`) | `6b07a207` |
| **Commodore 64** | newer build (`d011c639`) | `4aa92357` |
| **DOS** | newer build (`ee17e574`) | `26d8a1c0` |
| **Atari Jaguar** (skipped `024c4337` on 12 October 2026: about 13% slower; newer code since) | newer build (`fee3785c`) | `00356c5c` |
| **Saturn Beetle** | newer build (`852f006a`) | `43b790bd` |

The newest changes, as their authors describe them:

- **NES**
  - Merge pull request #662 from NRS-NewRisingSun/additions
  - libretro: register the core options before content is loaded (#661)
  - libretro: set core option display state after every registration
  - libretro: remove unused stereo delay string and stray macro semicolon
  - libretro: send path checks through the frontend VFS (#660)
- **NES and SNES, Mesen 2 with Continuum patches**
  - fix core options, sample rate, and NES volume balance
  - fix serialization undershoot that broke savestates on some games
  - suppress RecentGames cruft
- **SNES, bsnes with a Continuum patch**
  - libretro: report that states are incomplete
  - Merge pull request #63 from sunlollyking/serialization-quirks
- **Master System, Game Gear and SG-1000, Gearsystem with a Continuum patch**
  - Add CRT Guest Sonkun shader presets with variants
  - Update libretro info file with latest features
  - Apply fullscreen mode changes without toggling fullscreen
  - Keep captured mouse input away from the GUI
  - Add option to share gamepads between ports
- **Mega Drive, Sega CD and 32X, BlastEm with a Continuum patch**
  - ci: Add the Emscripten build to Gitlab CI (#84)
  - libretro: build the emscripten core as a static archive
- **PS1, interpreter only**
  - remove debug.c/socket.c
  - cdriso: harden format parsers
  - update lightrec
  - Merge branch 'master' (Oct 29 2026) into libretro
  - git subrepo pull --branch=4885d601ff4c0b78c63fe80a83df85da657fb21c deps/lightrec
- **PlayStation, Beetle PSX HW (Vulkan) — step 4 proof core**
  - rhi_tt: release the stack copy of a new fused page
  - gpu: measure a savestate without reading VRAM back
  - gl: pack VRAM words on the GPU for float and byte readbacks
  - vulkan: flush and invalidate only the bytes the host touched
  - gl: resolve only the tiles a frame wrote
- **TurboGrafx-16 / PC Engine, software renderer**
  - Fetch translations & Recreate libretro_core_options_intl.h
  - Fetch translations & Recreate libretro_core_options_intl.h
- **Nintendo 64, software rasteriser and interpreter**
  - GLideN64: move S2DEX to C89
  - GLideN64: fix the C89 microcode build errors and the check that missed them
  - GLideN64: convert the F5Rogue microcode to C89
  - GLideN64: convert the F5Indi_Naboo microcode to C89
  - GLideN64: convert RDP to C89 and run LLE lists from guest memory
- **Nintendo 3DS, Vulkan, interpreter CPU (no JIT)**
  - android: Fixed crash due to unhandled setting key in default ini
  - android: add KEEP_SCREEN_ON flag to secondary window (#2773)
  - qt: Add ROM bundle extraction (#2774)
  - Updated translations via Transifex
  - renderer_vulkan: Gracefully handle lack of MSAA support (#2779)
- **PSP, Vulkan, IR interpreter (no JIT, no dynarec)**
  - Translate the 30 fps limit setting
  - Merge pull request #23011 from hrydgard/xmb-settings-persist
  - Merge pull request #23012 from hrydgard/ge-pace-30fps
  - Modules: Find the caller's module by address, fix self stop/unload
  - Merge pull request #23014 from hrydgard/module-self-unload
- **Dreamcast, interpreter only (TARGET_NO_REC), optional**
  - dreamcast: Default broadcast follows the region
  - Fetch translations & Recreate libretro_core_options_intl.h
  - ci: enable macOS build
  - bba: don't [de]serialize bba if not available on the current platform
  - reios: pass correct data size to GetMemPtr when reading sectors

## Already on the newest code

| Console | Comes from | Our locked version |
| --- | --- | --- |
| GBA + GB/GBC | built here | `7a12d6d4` |
| Nintendo DS, software renderer | built here | `66b5d263` |
| Atari 2600, software renderer | built here | `ba52c43b` |
| Symbian / N-Gage, Continuum Symbian on EKA2L1 (dyncom interpreter, no JIT), optional | built here | `661031f9` |
| WonderSwan | downloaded | `0f3a7249` |
| Neo Geo Pocket | downloaded | `0d9932e1` |
| PC Engine CD | downloaded | `aa2f4dd3` |
| SuperGrafx | downloaded | `19e3804e` |
| Atari Lynx | downloaded | `b2d2c86e` |
| Atari 7800 | downloaded | `69ff2105` |
| Atari 5200 | downloaded | `3c166167` |
| Pokemon Mini | downloaded | `ea44fd3e` |
| Saturn | downloaded | `0f4c457f` |

## Checked and skipped

A newer version exists, and it was tried and turned down. These are not counted as new
until the author moves past the version named here.

| Console | Newer version | Why it was skipped | When |
| --- | --- | --- | --- |
| DOOM | `b5bed4d7` (buildbot build) | turns the default sound and music volume to max, so DOOM’s sound clips | 28 October 2026 |
| Virtual Boy | `2ee0c2c8` (buildbot build) | about 19% slower | 12 October 2026 |

