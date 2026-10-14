# Emulator updates, 14 October 2026

The check at the start of the session (`docs/CORE_UPDATES.md`, commit `c4ef0b4`) found four cores
with newer code: Gearsystem, Azahar, PPSSPP and Flycast. Each one gets a section below as it is
done (task file `.kiro/notes/tasks/core-updates.json`, items CU1 to CU4); the summary comes last
(CU5).

## How old save states are recorded now

`CORE_UPDATES` in `crates/emulator-bridge/src/core_updates.rs` only counts direct pairs (old pin to
new pin, no chaining). So when a core moves a second time it needs one entry from each pin a
player's install can hold:

- **build 181's pin**: an install with no `core-builds.txt` is compared against `BUILD_181`, and a
  heads-up still waiting since 181 keeps 181's pin as its "from";
- **the pin builds 182 to 185 shipped**, recorded in their `core-builds.txt`.

The old entry pointing at the 12 October pin is pointed at the new pin, and a second entry is
added from the 12 October pin. The test that a 181 install gets the right heads-up for PPSSPP needs
this too, so PPSSPP's and Flycast's moves (CU3, CU4) have to follow the same shape.

## Master System, Game Gear and SG-1000: Gearsystem `692494c8` → `4a801d22` (taken)

**What changed for players:** nothing they can see. Gearsystem's author made five changes since our
version: warning fixes (the libretro makefile and a logging macro), two ZIP-extraction safety fixes
and a Windows folder fix (all in desktop-only helpers in `src/common.h` that nothing in the libretro
core calls), and the desktop app's video recorder (`platforms/shared/desktop/`, which the core never
compiles). Nothing touches the emulation, the libretro front end, iOS or file types. The save-state
format is 111 (oldest readable 100) at all three of our pins. Our patch
(`scripts/patches/gearsystem-sg1000-memory-map.patch`) applies unchanged: dry run, and
`build-core.sh ios-apply-patches` on a clean copy of the new pin.

**Checked on a computer.** Four host builds with our patch: the new pin; `692494c8` with today's
patch (what builds 184 and 185 ship); `692494c8` and `02b9d9e7` with the patch as it was before
13 October's online-play fix (builds 182-183, and build 181). All built with no warnings. They were
loaded through the engine's own frontend (`NativeLibretroCore`, what the app uses on the phone), by
a harness kept outside the repository like 13 October's. The games were two open-licence Master
System homebrews by haroldo-ok, DataStorm (Apache-2.0) and Sub Rescue (MIT), plus DataStorm run as a
Game Gear cartridge (`.gg`). The ROMs weren't committed.

- **It plays.** 1,200 frames each with a scripted player. The two Master System games show 15 to 17
  colours and the picture moves on 620 to 960 of the frames (149 for the Game Gear run, which only
  shows the middle of a Master System screen). Sound comes out every frame. Frame 600 of both
  games shows a game in play: score, enemies, the player's ship or sub. Save, load and save again
  comes out byte for byte the same.
- **Old save states load.** A state saved at frame 600 on each earlier build (181, 182-183,
  184-185) loads on the new pin, and the next 600 frames, picture and sound, are identical to the
  old build carrying on. All nine combinations of game and build pass, and the new pin's end state
  matches 184-185's byte for byte. Before the table change the engine's own check
  (`state_version_loads`, with the version strings these builds really report) refused all three.
  Now it lets them through.
- **13 October's online-play fix still holds.** Phone A plays straight to frame 210. Phone B joins
  at frame 50, loads A's frame-200 state and plays on to 210. 0 bytes differ on the new pin and on
  184-185. On 182-183, before the fix, 6 bytes differ: the low three bytes of the Sports Pad's clock
  (`last_clock`, at bytes 40 to 47 of each controller port's 48-byte block), the same two 8-byte
  fields 13 October found (both counts fit in three bytes here). The second half of that fix also
  holds. States were saved from processes whose memory was first filled with two different byte
  patterns: 0 bytes differ on the new pin and 184-185. On 182-183, 2 bytes come out as the pattern
  itself: Video's light-phaser struct's two spare bytes.
- **The cache.** The pin move rebuilds Gearsystem only. `ios-fingerprint gearsystem` went from
  `68307747f04aca5a` to `d3291d0dbfaf630a`, while blastem, mgba and fceumm stayed the same.
  `scripts/check-core-cache.sh`: 213 passed.

**In the table:** `02b9d9e7` → `4a801d22` (build 181) and `692494c8` → `4a801d22` (builds 182 to 185),
both "old states load".

**The heads-up**, once, on the first Master System, Game Gear or SG-1000 game after installing over
an earlier build: "Heads up: the Master System and Game Gear emulator (Gearsystem) just got an update
from the people who make it. If anything seems off, send us feedback." The owner deletes the app
before every install, so the owner won't see it. For them the phone check is just that a Master
System or Game Gear game plays.

**Out of date once this ships:** STATUS.md's "After 181" line still says Gearsystem
`02b9d9e7`→`692494c8`; the next install has `4a801d22`. TESTING.md's "load a state you saved on
181. ✅ It loads." still holds.

## 3DS: Azahar `bfa76872` → `68202bc4` (taken)

**What changed for players:** nothing they can see. Azahar's makers made two changes since our
version. The morning check found the first; the second landed at 8:27 AM Eastern, after it.

- **"core: Fix bundle file oversights" (#2673, `73ed0e91`).** It's about 3DS "bundle" files (one
  file holding a game with its update and DLC: `.bcia`, `.bcci`, `.bcxi`). Azahar now checks that
  every file inside a bundle really fits in it, and refuses a broken bundle instead of reading past
  its end. Its tar reader (the `externals/microtar` submodule, `79f84057` → `5c8004c3`) reads each
  header field strictly. It also finds a game stored as `.cxi` or `.app` inside a bundle, and
  stops installing a bundle when one of its parts is bad. Smaller fixes come with it: ticket and
  title-metadata loading reports an error where it used to return an uninitialised value,
  `BundleCIAHandler::OpenContentFile` no longer runs off the end without returning, a game's icon
  is read from the right part of it, and `SubIOFile` checks its bounds without overflowing.
  Players won't notice any of it: the app doesn't treat bundle files as 3DS games
  (`import/detect.rs` takes `.3ds`, `.3dsx`, `.cci` and `.cxi`).
- **"Updated translations via Transifex" (`68202bc4`).** It only touches the desktop app's language
  files (`dist/languages/*.ts`, used only by `citra_qt`) and Android's `strings.xml`. The libretro
  core compiles neither.

Nothing touches the emulation (CPU, GPU, sound), the Vulkan renderer, the libretro front end
(`src/citra_libretro/` is identical at both pins), iOS, core options, the save-state code
(`src/core/savestate.cpp` is identical) or which file types open. Upstream's own `libretro-ios`
job passed on `73ed0e91`. On `68202bc4` it was still running when I checked, and that commit
changes no file the core compiles. All four of our patches (`scripts/patches/azahar-*.patch`)
apply: a dry run, then `build-core.sh ios-apply-patches azahar` on a clean copy of the new pin with
its `dynarmic` and `oaknut` submodules. Every file they touch is identical at both pins. No host
build: Azahar is a GPU core, so the iOS build and the phone confirm it.

**Old save states don't load, and that's Azahar's own rule, same as on 12 October.**
`retro_unserialize` calls `System::LoadStateBuffer`, which compares the git commit stored in the
state's header with the core's own (`Common::g_scm_rev`), and refuses the state when they differ.
The iOS build takes that commit from its own clone of the pin, so a state saved on build 181
(`065c9222`) or on builds 182 to 185 (`bfa76872`) is refused on `68202bc4`, whatever changed.
In-game saves are fine. They are files on the emulated SD card and system memory, and nothing in
this update touches how those are stored: its file-system changes only read tickets, title
metadata and game files.

- **The cache.** The pin move rebuilds Azahar only. `ios-fingerprint azahar` went from
  `0c0c314f28853b38` to `4a2dc54790918f88`, while mgba, ppsspp, flycast and flycast_jit stayed the
  same. `scripts/check-core-cache.sh`: 217 passed.
- **The checks.** `cargo test --workspace --features emulator-bridge/native-core`: 735 passed.
  `cargo clippy`: clean.

**In the table:** `065c9222` → `68202bc4` (build 181) and `bfa76872` → `68202bc4` (builds 182 to 185),
both refused.

**The heads-up**, once, on the first 3DS game after installing over an earlier build: "Heads up:
the 3DS emulator (Azahar) just got an update. Older save states can’t load on it, but your in-game
saves are fine. If anything seems off, send us feedback." The owner deletes the app before every
install, so they won't see it. Their phone check is just that a 3DS game plays like before, with
its in-game save still there.

**Out of date once this ships:** STATUS.md's "After 181" line still says Azahar
`065c9222`→`bfa76872`; the next install has `68202bc4`. TESTING.md's "Open a 3DS game. ✅ It plays
like before, and the heads-up says older save states can’t load on it but your in-game saves are
fine." still holds.
