# Have any of the emulators been updated?

Checked automatically, weekly. **Last checked: 12 October 2026, 16:11 UTC.**

Written by `scripts/check-core-updates.sh`. Do not edit by hand; it is overwritten.

## What this page is, and is not

Each emulator in Continuum is locked to one exact version of its author's code. That is
deliberate: if the build took whatever was newest, somebody else's change overnight could
break the app with nothing on this side having changed. The cost is that nothing announces
when an emulator has genuinely improved. This page is that announcement.

**Nothing here has been taken.** Reading this page changes nothing about the app. Moving a
core to a newer version is a deliberate edit to `scripts/build-core.sh`, then a build, then
a test on a phone. A number in the table below is an invitation to look, not a problem.

## 7 of 14 have newer code available

"New changes" counts every commit the author has made since our locked version. A big
number is not automatically important — it can be one real fix among a hundred tidy-ups.

| Console | New changes since ours | Newest change | Our locked version |
| --- | --- | --- | --- |
| **NES** | 2 | 2026-10-11 | `7a542dab` |
| **Mega Drive + Master System + Game Gear** | 1 | 2026-10-09 | `58c34148` |
| **PlayStation, Beetle PSX HW (Vulkan) — step 4 proof core** | 14 | 2026-10-06 | `5ec9909f` |
| **Nintendo 64, software rasteriser and interpreter** | 34 | 2026-10-11 | `0bd516ee` |
| **Nintendo 3DS, Vulkan, interpreter CPU (no JIT)** | 18 | 2026-10-10 | `065c9222` |
| **PSP, Vulkan, IR interpreter (no JIT, no dynarec)** | 548 | 2026-10-12 | `53fae900` |
| **Dreamcast, interpreter only (TARGET_NO_REC), optional** | 11 | 2026-10-12 | `59ed35a7` |

## Already on the newest code

| Console | Our locked version |
| --- | --- |
| GBA + GB/GBC | `7a12d6d4` |
| SNES, C++ | `fae2fea0` |
| PS1, interpreter only | `c8816799` |
| Nintendo DS, software renderer | `66b5d263` |
| TurboGrafx-16 / PC Engine, software renderer | `3f946f27` |
| Atari 2600, software renderer | `ba52c43b` |
| Symbian / N-Gage, Continuum Symbian on EKA2L1 (dyncom interpreter, no JIT), optional | `661031f9` |

