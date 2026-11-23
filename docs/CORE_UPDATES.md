# Have any of the emulators been updated?

Checked automatically, weekly. **Last checked: 23 November 2026, 18:47 UTC.**

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

## 20 of 32 have newer code available

Built here. "New changes" counts every commit the author has made since our locked
version. A big number is not automatically important — it can be one real fix among a
hundred tidy-ups.

| Console | New changes since ours | Newest change | Our patches still fit | Our locked version |
| --- | --- | --- | --- | --- |
| **NES** | 26 | 2026-11-22 | — | `ac0d00a7` |
| **NES and SNES, Mesen 2 with Continuum patches** | 8 | 2026-11-20 | no (mesen2) | `4c4e069f` |
| **SNES, bsnes with a Continuum patch** | 7 | 2026-11-23 | yes | `05439f96` |
| **Master System, Game Gear and SG-1000, Gearsystem with a Continuum patch** (skipped `426d4eb6` on 16 October 2026: only the desktop app and the file-based save code changed; the emulator Continuum builds is the same; newer code since) | 78 | 2026-11-22 | yes | `4a801d22` |
| **Mega Drive, Sega CD and 32X, BlastEm with a Continuum patch** (skipped `2230addc` on 27 October 2026: only its makers' web-browser build changed (a new build job, and two network functions renamed for it, which Continuum never uses); the emulator Continuum builds runs the same code; newer code since) | 4 | 2026-11-21 | no (blastem) | `ab743653` |
| **PS1, interpreter only** | 37 | 2026-11-22 | yes | `c8816799` |
| **PlayStation, Beetle PSX HW (Vulkan) — step 4 proof core** (skipped `e57860d8` on 12 October 2026: 3-7% slower on the software renderer the app uses; newer code since) | 116 | 2026-11-20 | — | `5ec9909f` |
| **Nintendo DS, software renderer** | 10 | 2026-11-15 | — | `66b5d263` |
| **TurboGrafx-16 / PC Engine, software renderer** (skipped `4eee99c4` on 27 October 2026: only the Spanish and Swedish translations of its settings changed, and Continuum always shows them in English; the emulator Continuum builds is the same; newer code since) | 3 | 2026-11-20 | — | `3f946f27` |
| **Nintendo 64, software rasteriser and interpreter** | 482 | 2026-11-21 | yes | `a8280501` |
| **Nintendo 3DS, Vulkan, interpreter CPU (no JIT)** | 81 | 2026-11-23 | yes | `6daa07ab` |
| **PSP, Vulkan, IR interpreter (no JIT, no dynarec)** | 2013 | 2026-11-23 | no (ppsspp) | `a838053e` |
| **Dreamcast, interpreter only (TARGET_NO_REC), optional** | 44 | 2026-11-22 | yes | `80471d57` |

Downloaded. The buildbot's build is a different file from ours; that alone does not say
what changed in it.

| Console | What the buildbot has | Our locked build |
| --- | --- | --- |
| **PC Engine CD** | newer build (`805db202`) | `aa2f4dd3` |
| **Amiga** (skipped `49ef1cd2` on 27 October 2026: same code as ours (5.3.1, 7387937); only rebuilt by the buildbot; newer code since) | newer build (`ddb509bb`) | `6b07a207` |
| **Commodore 64** | newer build (`3ea7a5a4`) | `4aa92357` |
| **DOS** | newer build (`ee17e574`) | `26d8a1c0` |
| **DOOM** (skipped `b5bed4d7` on 28 October 2026: turns the default sound and music volume to max, so DOOM’s sound clips; newer code since) | newer build (`4f6329dd`) | `e7d16803` |
| **Atari Jaguar** (skipped `024c4337` on 12 October 2026: about 13% slower; newer code since) | newer build (`9f4e0c93`) | `00356c5c` |
| **Saturn Beetle** | newer build (`74027b7f`) | `43b790bd` |

The newest changes, as their authors describe them:

- **NES**
  - sound: high-quality channels add each span between wave steps at once
  - sound: keep the high-quality filter history in savestates
  - ntsc: SSE2 and NEON blitter, bit-exact with the scalar one
  - x6502: a cycle only advances timestamp
  - ntsc: include the intrinsics header before restrict is defined
- **NES and SNES, Mesen 2 with Continuum patches**
  - fix garbled screen when not focused
  - switch to libretro save interface
  - expose game boy model when SNES is active
  - add RAW palette
  - raw palette fixes, add cropping options
- **SNES, bsnes with a Continuum patch**
  - CX4: Remove extra cycle on DMA to internal RAM (fixes excessive slowdown in Morp
  - libretro: report that states are incomplete
  - Merge pull request #63 from sunlollyking/serialization-quirks
  - libretro: update from upstream, fix merge conflicts
  - libretro: add Switch 2 (libounce) build
- **Master System, Game Gear and SG-1000, Gearsystem with a Continuum patch**
  - Reorganize menu sections
  - Bump version to 3.9.26
  - Rework About window with copyable build info
  - Fix cheat importer with multiple codes. Fix #212
  - Pace frames by the clock and drop slow dividers
- **Mega Drive, Sega CD and 32X, BlastEm with a Continuum patch**
  - ci: Add the Emscripten build to Gitlab CI (#84)
  - libretro: build the emscripten core as a static archive
  - libretro: save RAM in the Genesis Plus GX layout, converting old saves (#86)
  - libretro: load, format and save Sega CD backup RAM (#87)
- **PS1, interpreter only**
  - tests: switch to clang
  - gpu_unai: light the CLUT once for bigger lit 4/8bpp sprites
  - gpu_unai: rm unwated return
  - ci: update armhf
  - Merge branch 'master' (Nov 22 2026) into libretro
- **PlayStation, Beetle PSX HW (Vulkan) — step 4 proof core**
  - tools/gpu_thread: compare emulated GPU timing to the clock every frame
  - frontio: drop the light guns' prev_vsync
  - mdec: an IDCT that sums a row with every matrix row at once
  - spu: keep the voices the silent-voice skip passed, and walk past them
  - Fetch translations & Recreate libretro_core_options_intl.h
- **Nintendo DS, software renderer**
  - ci: bundle Windows runtime dependencies
  - ci: install the Qt 5 deployment tools
  - ci: skip optional Qt OpenGL runtime deployment
  - Merge pull request #220 from libretro/codex/fix-build-ci
  - Merge pull request #219 from sunlollyking/load-from-path
- **TurboGrafx-16 / PC Engine, software renderer**
  - Fetch translations & Recreate libretro_core_options_intl.h
  - Fetch translations & Recreate libretro_core_options_intl.h
  - Fetch translations & Recreate libretro_core_options_intl.h
- **Nintendo 64, software rasteriser and interpreter**
  - Rice, glN64, Glide64: folders named as mupen64plus names the plugins
  - Rice: texture cache in C89
  - Rice: a palette that changes is seen, and CRCs stay in RDRAM
  - Rice: render textures in C89
  - Rice: texels into the colour image in C89, nothing uploaded again
- **Nintendo 3DS, Vulkan, interpreter CPU (no JIT)**
  - core: plgldr: Add support for v3 plugins (#2833)
  - qt: Add error dialog when attempting to use video dumping without OpenGL
  - Updated translations via Transifex
  - Updated compatibility data
  - Updated translations via Transifex
- **PSP, Vulkan, IR interpreter (no JIT, no dynarec)**
  - PPSSPP sound effect theme: Remove subtle clicks at the end of the wave files
  - Config: Don't check that the default ini files exist
  - Load the achievements icon cache on an IO thread
  - Themes: Reload the list when the popup opens, sort it, don't let names replace
  - Merge pull request #23154 from hrydgard/misc-fixes
- **Dreamcast, interpreter only (TARGET_NO_REC), optional**
  - Fetch translations & Recreate libretro_core_options_intl.h
  - ci: mingw tentative fix
  - ui: missing tooltips for bottom row of game list
  - dx11: fix partial rendering for OIT
  - Force RTT Copy to VRAM for Pia Carrott 2 and Shinki Sekai Evolution

## Already on the newest code

| Console | Comes from | Our locked version |
| --- | --- | --- |
| GBA + GB/GBC | built here | `7a12d6d4` |
| Atari 2600, software renderer | built here | `ba52c43b` |
| Symbian / N-Gage, Continuum Symbian on EKA2L1 (dyncom interpreter, no JIT), optional | built here | `661031f9` |
| WonderSwan | downloaded | `0f3a7249` |
| Neo Geo Pocket | downloaded | `0d9932e1` |
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
| Virtual Boy | `2ee0c2c8` (buildbot build) | about 19% slower | 12 October 2026 |

