# Have any of the emulators been updated?

Checked automatically, weekly. **Last checked: 02 November 2026, 16:37 UTC.**

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

## 12 of 32 have newer code available

Built here. "New changes" counts every commit the author has made since our locked
version. A big number is not automatically important — it can be one real fix among a
hundred tidy-ups.

| Console | New changes since ours | Newest change | Our patches still fit | Our locked version |
| --- | --- | --- | --- | --- |
| **Master System, Game Gear and SG-1000, Gearsystem with a Continuum patch** (skipped `426d4eb6` on 16 October 2026: only the desktop app and the file-based save code changed; the emulator Continuum builds is the same; newer code since) | 41 | 2026-11-01 | yes | `4a801d22` |
| **PS1, interpreter only** | 18 | 2026-10-29 | yes | `c8816799` |
| **PlayStation, Beetle PSX HW (Vulkan) — step 4 proof core** (skipped `e57860d8` on 12 October 2026: 3-7% slower on the software renderer the app uses; newer code since) | 74 | 2026-11-01 | — | `5ec9909f` |
| **TurboGrafx-16 / PC Engine, software renderer** (skipped `4eee99c4` on 27 October 2026: only the Spanish and Swedish translations of its settings changed, and Continuum always shows them in English; the emulator Continuum builds is the same; newer code since) | 2 | 2026-10-30 | — | `3f946f27` |
| **Nintendo 64, software rasteriser and interpreter** | 121 | 2026-11-02 | yes | `a8280501` |
| **Nintendo 3DS, Vulkan, interpreter CPU (no JIT)** | 41 | 2026-11-01 | yes | `6daa07ab` |
| **PSP, Vulkan, IR interpreter (no JIT, no dynarec)** | 1124 | 2026-11-02 | yes | `a838053e` |
| **Dreamcast, interpreter only (TARGET_NO_REC), optional** | 27 | 2026-10-31 | yes | `80471d57` |

Downloaded. The buildbot's build is a different file from ours; that alone does not say
what changed in it.

| Console | What the buildbot has | Our locked build |
| --- | --- | --- |
| **Amiga** (skipped `49ef1cd2` on 27 October 2026: same code as ours (5.3.1, 7387937); only rebuilt by the buildbot; newer code since) | newer build (`81ebb930`) | `6b07a207` |
| **Commodore 64** | newer build (`28369f2e`) | `4aa92357` |
| **Atari Jaguar** (skipped `024c4337` on 12 October 2026: about 13% slower; newer code since) | newer build (`fee3785c`) | `00356c5c` |
| **Saturn Beetle** | newer build (`78df7e4c`) | `43b790bd` |

The newest changes, as their authors describe them:

- **Master System, Game Gear and SG-1000, Gearsystem with a Continuum patch**
  - Expose cartridge and extra RAM for achievements
  - Bump github/gh-aw-actions/setup-cli from 0.89.21 to 1.0.1
  - Bump vmactions/freebsd-vm from 1.5.8 to 1.5.9
  - Merge pull request #209 from drhelius/dependabot/github_actions/vmactions/freebs
  - Merge pull request #208 from drhelius/dependabot/github_actions/github/gh-aw-act
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
  - parallel-rdp: fold the VI's downscale and deinterlace into the scale pass
  - Rice: drop the frame buffer managers nobody defines, fixing the GCC 16 link
  - parallel-rdp: fit upscaling to the device's storage buffer bindings
  - parallel-rdp: keep the factor and drop the read-back where only it does not fit
  - glsm: move it out of libretro-common, into libretro/glsm
- **Nintendo 3DS, Vulkan, interpreter CPU (no JIT)**
  - core: Add remaining battery functions (#2743)
  - Add battery stats to android default.ini (#2738)
  - appimage: Provide a .png for appimage building (#2746)
  - Android Per-Application settings (for .ini files only) (#2698)
  - Updated translations via Transifex
- **PSP, Vulkan, IR interpreter (no JIT, no dynarec)**
  - CMake: Declare git-version.cpp as a byproduct of the GitVersion step
  - Docs: The Android version string no longer lags behind
  - Reinterpret: Write the stencil from the source, in the same render pass
  - Merge pull request #22887 from hrydgard/fix-stencil-rebuild-cost
  - Merge pull request #22889 from hrydgard/input-latency
- **Dreamcast, interpreter only (TARGET_NO_REC), optional**
  - add pull request template. Fix issue templates.
  - Fetch translations & Recreate libretro_core_options_intl.h
  - libretro: add a Vblank Pacing option (#2562)
  - dreamcast: Default broadcast follows the region
  - Fetch translations & Recreate libretro_core_options_intl.h

## Already on the newest code

| Console | Comes from | Our locked version |
| --- | --- | --- |
| NES | built here | `ac0d00a7` |
| NES and SNES, Mesen 2 with Continuum patches | built here | `4c4e069f` |
| GBA + GB/GBC | built here | `7a12d6d4` |
| SNES, bsnes with a Continuum patch | built here | `05439f96` |
| Nintendo DS, software renderer | built here | `66b5d263` |
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

## Checked and skipped

A newer version exists, and it was tried and turned down. These are not counted as new
until the author moves past the version named here.

| Console | Newer version | Why it was skipped | When |
| --- | --- | --- | --- |
| Mega Drive, Sega CD and 32X, BlastEm with a Continuum patch | `2230addc` (1 new changes) | only its makers' web-browser build changed (a new build job, and two network functions renamed for it, which Continuum never uses); the emulator Continuum builds runs the same code | 27 October 2026 |
| DOOM | `b5bed4d7` (buildbot build) | turns the default sound and music volume to max, so DOOM’s sound clips | 28 October 2026 |
| Virtual Boy | `2ee0c2c8` (buildbot build) | about 19% slower | 12 October 2026 |

