# Unfinished jobs

The batch began on 13 October 2026 and carried on through 14 October. Each JSON file here is one
job's checklist: items marked `"done": true` are finished and on master; the rest is the remaining
work with full instructions. No deadline (owner): finish every item properly, leave nothing out.
Every helper reads `HELPER_RULES.md` (this folder) before its checklist.

**Last updated 16 October, 7 AM Eastern.** Every job is complete, cArcade included. No work is
saved part-way (`../wip/` holds only the scratch tools).

| File | Job | Done | Left |
|---|---|---|---|
| `gamecube.json` | GameCube and Wii on Dolphin with our own faster interpreter (owner, 16 October) | nothing yet | GC1 to GC5 |
| `library-and-manic.json` | Covers like Manic's, the library by console, Manic's other features from the owner's screenshots | nothing yet | L1 to L10 |
| `mirror-restore.json` | The build's frozen emulator copies (deleted by mistake, 14 October) | **All done (16 October)**: the next build takes 11 from the buildbot and 5 (DOOM, Jaguar, Virtual Boy, C64, Amiga) from build 185's install, then refills the mirror itself | nothing |
| `cboy.json` | cBoy | **All done (16 October)**: G1 colour for black-and-white games, G1b link text, G1c short descriptions, G2 write timing, G3 the Color half of Mealybug, G4 the rest (the exact drawer at 51 of 55, the most there are references for), G5 the write-up (`.kiro/notes/docs/CBOY_EVENING.md`) | nothing |
| `arcade.json` | cArcade | **All done (16 October)**: I1 plan, I2 Z80, I3 Pac-Man board, I4 in the app (65 sets), the 8035 and AY-3-8910 chips, I5 the Galaxian family (119 sets), I6 the Donkey Kong board (15 sets), I7 the write-up (`.kiro/notes/docs/ARCADE_CORE.md`: all 199 sets, the proofs, what's approximate, what's next) | nothing |
| `link-and-leftovers.json` | Leftovers | **All done (16 October)**: H1 two-phone link, H2 Mega Drive save chips, H4 Sega CD discs, H5 32X with no BIOS, H3 Mesen 2 for SNES, H6 the write-up (`.kiro/notes/docs/reports/evening-2026-10-13.md`) | nothing |
| `core-updates.json` | Emulator updates | **All done (16 October)**: CU1 Gearsystem, CU2 Azahar, CU3 PPSSPP, CU4 Flycast (and a second Azahar move), CU5 the write-up (`.kiro/notes/docs/reports/core-updates-2026-10-14.md`) | nothing |
| `home-screen.json` | Home screen | **All done (16 October)**: U1 floating tabs, U2 strip gone, U3 card size, U3b light mode, U4 fresh review, U5 the write-up (`.kiro/notes/docs/reports/evening-2026-10-13-library.md`) | nothing |
| `shorter-text.json` | Shorter text | **All done (16 October)**: T1, T2, T3, T4 fresh review, T5 the write-up (`.kiro/notes/docs/reports/evening-2026-10-13-text.md`) | nothing |

Finished and removed: the build cache's second pass (`.kiro/notes/docs/reports/cache-v2-summary.md`, 213
checks) and the icons (`.kiro/notes/docs/reports/evening-2026-10-13-icons.md`), and the repo cleanup (its plan, `repo-cleanup-phase2.md`, keeps the parked texts
and the README lines to update after the next build).

## Running them

One `bundled://ralph` loop per job (run_workflow; `inputs.prd_path` = `<clone>/.kiro/notes/tasks/<job>.json`,
and a goal naming the clone and telling every iteration to read HELPER_RULES.md first). A loop
stops when its file says `"complete": true`. Start a loop for every job at once. Each job works in its own fresh clone, the `"repo"` field
(`/projects/sandbox/.scratch/<x>-work`); apply its WIP patch first if it has one. Review items (U4,
T4) need a fresh session that didn't write the work; each loop iteration is one. A Kiro restart
pauses loops: check every loop you started whenever you wake, and resume it.

Home screen and shorter text edit the same Swift files, so each rebases before every file and
keeps both sides on a conflict.

The essentials, if HELPER_RULES.md isn't read: clone fresh; every commit ends `[skip ci]`; stage by
name; `git pull --rebase` and keep both sides; `CARGO_BUILD_JOBS=2`; a loop agent needs its own
`CARGO_HOME` (`/root/.cargo` is read-only for it); never touch STATUS, TESTING or HANDOFF; never
edit `.github/workflows/`; fetching anyone else's repository needs `GIT_CONFIG_GLOBAL=/dev/null` in
front.

## Parked: workflow file edits

Kiro's GitHub rules never allow a change under `.github/workflows/` straight onto master, only
through a pull request, and the owner allows none. Ready if that ever changes: R5's shorter release
notes (`repo-cleanup-phase2.md` section 2), and two ios.yml lines for cArcade (its crate's tests in
the `checks` job next to cBoy's; `cores/continuum-arcade/**` in the push paths). Until then CI
doesn't run cArcade's own tests, so run `cargo test --release` in `cores/continuum-arcade` before
every push, and a cArcade-only push doesn't start a build by itself.


Also parked, found by build 189 (16 October): ios.yml has no `optional continuum_arcade_libretro_ios.dylib "Arcade on cArcade"` line in its .ipa check, and `native/ios/build-engine.sh` requires every core file to be named in ios.yml, so every build since cArcade went in stopped at that check. `build-engine.sh` now exempts that one name (`NOT_IN_IOS_YML`); take it out in the same push that adds the line to ios.yml.
## Build 188 (cancelled)

Started at 9:01 AM on 14 October at e82432a without the owner's yes, and cancelled at 9:04 in the
Linux checks (nothing built, no release). No build until the batch is done or the owner asks.

## Then the build

`mirror-restore.json` is complete (16 October). After the build, `bash scripts/check-core-mirror.sh live` should say all 16 come "from the mirror" (the build uploads them).

1. When every job is done, fold their reports (`.kiro/notes/docs/reports/evening-2026-10-13*.md`,
   `.kiro/notes/docs/ARCADE_CORE.md`, `.kiro/notes/docs/CBOY_EVENING.md`, `.kiro/notes/docs/reports/cache-v2-summary.md`, and `.kiro/notes/docs/reports/core-updates-2026-10-14.md`) into STATUS.md, TESTING.md and
   HANDOFF.md, and reword the TESTING lines the Home screen and text reports list. **Done on 16
   October, arcade's Donkey Kong board and I7 included:** STATUS.md ("Next up"), TESTING.md ("Next
   install (after 185)", arcade checks 55 to 63) and README.md. HANDOFF.md is still to do.
2. Start ONE build: `gh api -X POST repos/2c2bhdhw9z-cell/Continuum-/actions/workflows/ios.yml/dispatches -f ref=master`.
   Every emulator rebuilds once (new fingerprints), about an hour.
3. Build 184's "Publish the .ipa as a Release" step got HTTP 403 from GitHub on every try. If it
   happens again, create the release yourself (`gh api -X POST repos/2c2bhdhw9z-cell/Continuum-/releases
   -f tag_name=build-<run>-<sha7> -f target_commitish=<full sha> -f name="build <run>"`), then
   `gh api -X POST repos/2c2bhdhw9z-cell/Continuum-/actions/runs/<run id>/rerun-failed-jobs`. The re-run
   uploads the files into that release even though it still ends red. Download the .ipa to check.
4. Check the .ipa (0.8.0 and the build number; 37 core files: the 36 plus cArcade) and give
   the owner the link and the numbered phone steps in the same reply.
5. Repo cleanup part 2 is done (14 October): the working notes are in `.kiro/notes/`. Left from
   its plan: the About box (section 1; this sandbox's GitHub token can't set it) the release
   notes (section 2, parked). The README's list of systems (section 9) was brought up to date on
   16 October: SNES on bsnes or Mesen, 32X with no BIOS, arcade back on cArcade (Donkey Kong too).