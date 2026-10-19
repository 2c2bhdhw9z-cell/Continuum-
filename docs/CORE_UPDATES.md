# Have any of the emulators been updated?

Checked automatically, weekly. **Last checked: 19 October 2026, 14:05 UTC.**

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

## 10 of 32 have newer code available

Built here. "New changes" counts every commit the author has made since our locked
version. A big number is not automatically important — it can be one real fix among a
hundred tidy-ups.

| Console | New changes since ours | Newest change | Our patches still fit | Our locked version |
| --- | --- | --- | --- | --- |
| **Master System, Game Gear and SG-1000, Gearsystem with a Continuum patch** (skipped `426d4eb6` on 16 October 2026: only the desktop app and the file-based save code changed; the emulator Continuum builds is the same; newer code since) | 11 | 2026-10-19 | yes | `4a801d22` |
| **PlayStation, Beetle PSX HW (Vulkan) — step 4 proof core** (skipped `e57860d8` on 12 October 2026: 3-7% slower on the software renderer the app uses; newer code since) | 23 | 2026-10-19 | — | `5ec9909f` |
| **Nintendo 64, software rasteriser and interpreter** | 18 | 2026-10-19 | yes | `a8280501` |
| **Nintendo 3DS, Vulkan, interpreter CPU (no JIT)** | 8 | 2026-10-17 | yes | `6daa07ab` |
| **PSP, Vulkan, IR interpreter (no JIT, no dynarec)** | 189 | 2026-10-19 | yes | `a838053e` |
| **Dreamcast, interpreter only (TARGET_NO_REC), optional** | 6 | 2026-10-17 | yes | `80471d57` |

Downloaded. The buildbot's build is a different file from ours; that alone does not say
what changed in it.

| Console | What the buildbot has | Our locked build |
| --- | --- | --- |
| **Amiga** (skipped `2cf44f8e` on 16 October 2026: same code as ours (5.3.1, 7387937); only rebuilt by the buildbot; newer code since) | newer build (`99efad0c`) | `6b07a207` |
| **Commodore 64** (skipped `e842ed6f` on 16 October 2026: same code as ours (f63b566); only the build date inside differs; newer code since) | newer build (`110f2a0a`) | `caeffa2f` |
| **DOOM** (skipped `d14d2093` on 12 October 2026: turns the default sound and music volume to max, so DOOM’s sound clips; newer code since) | newer build (`b5bed4d7`) | `e7d16803` |
| **Atari Jaguar** (skipped `024c4337` on 12 October 2026: about 13% slower; newer code since) | newer build (`6f7c9348`) | `00356c5c` |

The newest changes, as their authors describe them:

- **Master System, Game Gear and SG-1000, Gearsystem with a Continuum patch**
  - Update icon
  - VGM recording indicator
  - Improve audio with vsync
  - Flush the audio queue instead of reopening device
  - Show FPS counter in a styled overlay box
- **PlayStation, Beetle PSX HW (Vulkan) — step 4 proof core**
  - retro_run: keep memory card path buffers out of the frame
  - libretro-common: pull libretro.h from RetroArch 6d6da52
  - GL: draw the analog resolve straight into the frontend's framebuffer
  - HDR: output scRGB FP16 when the frontend offers it
  - Unified color rendering (#1026)
- **Nintendo 64, software rasteriser and interpreter**
  - pi: no interrupt for bus writes or a reset transfer; domain register widths
  - parallel-rdp: pick upstream's TLUT, COPY, noise and I8 fixes
  - r4300: fix LL/SC link state, FPU compares on NaN, and ari64 SC/SCD
  - ari64: fix LDL/LDR merges and ARM helper calls
  - r4300 recompilers: DIV/DIVU by zero and INT32_MIN / -1, ARM DMULT
- **Nintendo 3DS, Vulkan, interpreter CPU (no JIT)**
  - license-header.rb: Added license header
  - fix-license-headers.py: Fixed typo in disclaimer
  - Updated translations via Transifex
  - fix-license-headers.py: Mark script as executable
  - qt: Fix missing icons in game list regression
- **PSP, Vulkan, IR interpreter (no JIT, no dynarec)**
  - Common: Share one non-Windows time implementation, fix the fallback
  - Common: Use CLOCK_UPTIME_RAW for the monotonic clock on Apple platforms
  - Merge pull request #22660 from hrydgard/ui-fixes
  - Merge pull request #22661 from hrydgard/ios-timer
  - Merge pull request #22662 from hrydgard/atrac-codec-details
- **Dreamcast, interpreter only (TARGET_NO_REC), optional**
  - input: stop the emulator while saving an in-RAM savestate (#2554)
  - savestate: reject invalid broadcast/cable/region instead of aborting (#2555)
  - cdipsr: make temp_value a local (#2549)
  - rend: clamp xBRZ texture upscale factor to 2..6
  - Fetch translations & Recreate libretro_core_options_intl.h

## Already on the newest code

| Console | Comes from | Our locked version |
| --- | --- | --- |
| NES | built here | `ac0d00a7` |
| NES and SNES, Mesen 2 with Continuum patches | built here | `4c4e069f` |
| GBA + GB/GBC | built here | `7a12d6d4` |
| SNES, bsnes with a Continuum patch | built here | `05439f96` |
| Mega Drive, Sega CD and 32X, BlastEm with a Continuum patch | built here | `ab743653` |
| PS1, interpreter only | built here | `c8816799` |
| Nintendo DS, software renderer | built here | `66b5d263` |
| TurboGrafx-16 / PC Engine, software renderer | built here | `3f946f27` |
| Atari 2600, software renderer | built here | `ba52c43b` |
| Symbian / N-Gage, Continuum Symbian on EKA2L1 (dyncom interpreter, no JIT), optional | built here | `661031f9` |
| WonderSwan | downloaded | `0f3a7249` |
| Neo Geo Pocket | downloaded | `0d9932e1` |
| PC Engine CD | downloaded | `aa2f4dd3` |
| SuperGrafx | downloaded | `19e3804e` |
| DOS | downloaded | `26d8a1c0` |
| Atari Lynx | downloaded | `b2d2c86e` |
| Atari 7800 | downloaded | `69ff2105` |
| Atari 5200 | downloaded | `3c166167` |
| Pokemon Mini | downloaded | `ea44fd3e` |
| Saturn | downloaded | `0f4c457f` |
| Saturn Beetle | downloaded | `40428642` |

## Checked and skipped

A newer version exists, and it was tried and turned down. These are not counted as new
until the author moves past the version named here.

| Console | Newer version | Why it was skipped | When |
| --- | --- | --- | --- |
| Virtual Boy | `2ee0c2c8` (buildbot build) | about 19% slower | 12 October 2026 |

