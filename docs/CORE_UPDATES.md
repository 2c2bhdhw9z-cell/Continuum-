# Have any of the emulators been updated?

Checked automatically, weekly. **Last checked: 16 November 2026, 16:31 UTC.**

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

## 18 of 32 have newer code available

Built here. "New changes" counts every commit the author has made since our locked
version. A big number is not automatically important — it can be one real fix among a
hundred tidy-ups.

| Console | New changes since ours | Newest change | Our patches still fit | Our locked version |
| --- | --- | --- | --- | --- |
| **NES** | 25 | 2026-11-16 | — | `ac0d00a7` |
| **NES and SNES, Mesen 2 with Continuum patches** | 6 | 2026-11-14 | no (mesen2) | `4c4e069f` |
| **SNES, bsnes with a Continuum patch** | 2 | 2026-11-06 | yes | `05439f96` |
| **Master System, Game Gear and SG-1000, Gearsystem with a Continuum patch** (skipped `426d4eb6` on 16 October 2026: only the desktop app and the file-based save code changed; the emulator Continuum builds is the same; newer code since) | 71 | 2026-11-16 | yes | `4a801d22` |
| **Mega Drive, Sega CD and 32X, BlastEm with a Continuum patch** (skipped `2230addc` on 27 October 2026: only its makers' web-browser build changed (a new build job, and two network functions renamed for it, which Continuum never uses); the emulator Continuum builds runs the same code; newer code since) | 2 | 2026-11-08 | yes | `ab743653` |
| **PS1, interpreter only** | 18 | 2026-10-29 | yes | `c8816799` |
| **PlayStation, Beetle PSX HW (Vulkan) — step 4 proof core** (skipped `e57860d8` on 12 October 2026: 3-7% slower on the software renderer the app uses; newer code since) | 83 | 2026-11-13 | — | `5ec9909f` |
| **Nintendo DS, software renderer** | 10 | 2026-11-15 | — | `66b5d263` |
| **TurboGrafx-16 / PC Engine, software renderer** (skipped `4eee99c4` on 27 October 2026: only the Spanish and Swedish translations of its settings changed, and Continuum always shows them in English; the emulator Continuum builds is the same; newer code since) | 2 | 2026-10-30 | — | `3f946f27` |
| **Nintendo 64, software rasteriser and interpreter** | 418 | 2026-11-16 | yes | `a8280501` |
| **Nintendo 3DS, Vulkan, interpreter CPU (no JIT)** | 72 | 2026-11-12 | yes | `6daa07ab` |
| **PSP, Vulkan, IR interpreter (no JIT, no dynarec)** | 1712 | 2026-11-16 | yes | `a838053e` |
| **Dreamcast, interpreter only (TARGET_NO_REC), optional** | 36 | 2026-11-13 | yes | `80471d57` |

Downloaded. The buildbot's build is a different file from ours; that alone does not say
what changed in it.

| Console | What the buildbot has | Our locked build |
| --- | --- | --- |
| **Amiga** (skipped `49ef1cd2` on 27 October 2026: same code as ours (5.3.1, 7387937); only rebuilt by the buildbot; newer code since) | newer build (`26e33f43`) | `6b07a207` |
| **Commodore 64** | newer build (`2b3f946b`) | `4aa92357` |
| **DOS** | newer build (`ee17e574`) | `26d8a1c0` |
| **Atari Jaguar** (skipped `024c4337` on 12 October 2026: about 13% slower; newer code since) | newer build (`535d8d7e`) | `00356c5c` |
| **Saturn Beetle** | newer build (`852f006a`) | `43b790bd` |

The newest changes, as their authors describe them:

- **NES**
  - filter: run the resampling taps forward
  - sound: high-quality channels add each span between wave steps at once
  - sound: keep the high-quality filter history in savestates
  - ntsc: SSE2 and NEON blitter, bit-exact with the scalar one
  - x6502: a cycle only advances timestamp
- **NES and SNES, Mesen 2 with Continuum patches**
  - fix serialization undershoot that broke savestates on some games
  - suppress RecentGames cruft
  - fix garbled screen when not focused
  - switch to libretro save interface
  - expose game boy model when SNES is active
- **SNES, bsnes with a Continuum patch**
  - libretro: report that states are incomplete
  - Merge pull request #63 from sunlollyking/serialization-quirks
- **Master System, Game Gear and SG-1000, Gearsystem with a Continuum patch**
  - Use video timing uniforms in bundled shaders
  - Document shader uniforms and fix shader README errors
  - Ignore overscan size for Game Gear in SMS mode
  - Use Cmd hotkeys on macOS and standardize labels
  - Change default video recording hotkey to Ctrl+Shift+V
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
  - vulkan: frame duping sees display VRAM writes
  - vulkan: order the barrier after a pass that sampled what it grows over
  - C89: declarations at the head of their blocks in the core sources
  - C89: the test hosts and checkers
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
- **Nintendo 64, software rasteriser and interpreter**
  - GLideNHQ: its image files in C89, a 24-bit BMP's width its own
  - GLideNHQ: its checksums, pool and buffers in C89
  - GLideNHQ: an odd-sized texture's last texel converted
  - GLideNHQ: the texture cache keeps the buffer, not a copy of it
  - GLideNHQ: its texture caches in C89
- **Nintendo 3DS, Vulkan, interpreter CPU (no JIT)**
  - android: Request local network access permission on Android 17+
  - qt: Correctly mark certain newly-added strings as untranslatable
  - swkbd.cpp: Fixed formatting
  - android: Correct one untranslatable string being marked as translatable
  - Updated translations via Transifex
- **PSP, Vulkan, IR interpreter (no JIT, no dynarec)**
  - libretro: Remove the frontend's log callback in retro_deinit
  - Merge pull request #23074 from kel-mo/libretro-remove-log-callback
  - Merge pull request #23070 from hrydgard/arm32-jit-thumb-calls
  - Merge pull request #23065 from hrydgard/crush-memstick-changed
  - Merge pull request #23071 from mario910/fix/buzz-hle-video-framebuffer
- **Dreamcast, interpreter only (TARGET_NO_REC), optional**
  - oboe: add 200 ms timeout to push() to avoid ANR when pausing game
  - make sure settings.display.width/height is never null
  - gl, dx9: recreate textures when an update changes their size or format (#2604)
  - gl, dx11: make video routing output opaque
  - vk, dx11: keep video routing output opaque under overlays

## Already on the newest code

| Console | Comes from | Our locked version |
| --- | --- | --- |
| GBA + GB/GBC | built here | `7a12d6d4` |
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

