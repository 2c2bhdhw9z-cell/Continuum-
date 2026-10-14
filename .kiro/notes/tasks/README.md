# Unfinished jobs (stopped 13 October 2026, 6:25 PM Eastern)

The Kiro account ran out of credits, so eight jobs were stopped part-way. Every helper reads `HELPER_RULES.md` (this folder) before its checklist. Everything they finished
is on master. Each file here is one job's checklist: items marked `"done": true` are finished and
pushed, the rest is the remaining work with full instructions. No deadline (owner): finish every
item properly, leave nothing out.

| File | Job | Done | Left |
|---|---|---|---|
| `link-and-leftovers.json` | Two-phone Game Boy link, then the leftovers | H1 (link, app half) | H2 Sampras Tennis 96 EEPROM, H3 Mesen 2 for SNES, H4 Sega CD disc swap and RAM cart, H5 our own 32X start-up code, H6 write-up |
| `arcade.json` | cArcade, our own arcade emulator | I1 plan, I2 Z80 (all 1,604,000 SingleStepTests, ZEXDOC, ZEXALL) | I3 Pac-Man board, I4 in the app, I5 Galaxian, I6 Donkey Kong, I7 write-up |
| `cboy.json` | cBoy | G1 colour for black-and-white games (Mealybug 24 to 27 of 55) | G1b link option text, G1c shorter option descriptions, G2 IO write timing, G3 Color half of Mealybug, G4 the rest, G5 write-up |
| `build-cache.json` | Build cache, second pass | nothing yet | K1 to K4. Must land BEFORE the next build |
| `home-screen.json` | Home screen | U1 floating tabs | U2 remove the "80 games" strip, U3 Game card size, U3b light mode (Dark, Light, System), U4 review, U5 write-up |
| `repo-cleanup.json` | Repo cleanup, part 1 | R1 inventory (`repo-cleanup-inventory.md`) | R2 README, R3 About box, R4 clutter, R5 release notes, R6 public docs, R7 part-2 plan |
| `icons.json` | Icons | IC1 (Default tile fixed) | IC0 the 12 icons the owner circled, IC2 icon names on iOS 16, IC3 write-up |
| `shorter-text.json` | Shorter text everywhere | T1 first half of Settings | T2 second half (the PSP section and the BIOS name list go), T3 other screens, T4 review, T5 write-up |
| `core-updates.json` | Emulator updates (added 14 October) | nothing yet | CU1 Gearsystem, CU2 Azahar, CU3 PPSSPP, CU4 Flycast, CU5 write-up |

Added 14 October (new account): `core-updates.json`, the four emulators with newer code that
morning's check found (Gearsystem, Azahar, PPSSPP, Flycast), CU1 to CU5.

## Restarting them

Each job has its own clone, named in its file's `"repo"` field (since 14 October link-and-leftovers
uses `/projects/sandbox/.scratch/link-work` and its Mesen item H3 `.../mesen-work`, so the main copy
stays the orchestrator's). On 14 October they were restarted as parallel helper agents
(`invoke_sub_agent`, `general-task-execution`), each told to read a shared rules sheet first and to
mark items done in THESE files on master as they land, so this folder is always the true state.
The rules sheet's essentials, for a restart: clone fresh; every commit ends `[skip ci]`; stage by
name; `git pull --rebase` and keep both sides; `CARGO_BUILD_JOBS=2`; never touch STATUS, TESTING or
HANDOFF; and fetching anyone else's repository needs `GIT_CONFIG_GLOBAL=/dev/null` in front (the
sandbox's git routes github.com through a gateway that only serves this repository: without it
`check-core-updates.sh`, `build-core.sh` and every upstream clone fail with "repository not found").
The review items (K3, U4, T4) go to a fresh helper that did not write the work. When a job
finishes, delete its file here in a docs-only push.

**cBoy G2, G3 and G4 are the exact drawer.** Before any of it starts, stop and tell the owner so
they can turn the thinking up to max, and begin nothing until they reply that it's set (owner
rule, asked again on 14 October).

cBoy's exact drawer follows SameBoy: clone github.com/LIJI32/SameBoy and (with `GIT_CONFIG_GLOBAL=/dev/null`)
github.com/mattcurrie/mealybug-tearoom-tests into `/projects/sandbox/.scratch/sameboy` and
`/projects/sandbox/.scratch/mealybug-src` again, never into the repo.

## Then the build

1. When all eight are done, fold their reports (`docs/reports/evening-2026-10-13*.md`,
   `docs/ARCADE_CORE.md`, `docs/CBOY_EVENING.md`, the cache summary) into STATUS.md, TESTING.md and
   HANDOFF.md, and reword the TESTING lines the Home screen and text reports list.
2. Start ONE build: `gh api -X POST repos/2c2bhdhw9z-cell/Continuum-/actions/workflows/ios.yml/dispatches -f ref=master`.
   Every emulator rebuilds once (new fingerprints), about an hour.
3. Build 184's "Publish the .ipa as a Release" step got HTTP 403 from GitHub on every try. If it
   happens again, create the release yourself (`gh api -X POST repos/2c2bhdhw9z-cell/Continuum-/releases
   -f tag_name=build-<run>-<sha7> -f target_commitish=<full sha> -f name="build <run>"`), then
   `gh api -X POST repos/2c2bhdhw9z-cell/Continuum-/actions/runs/<run id>/rerun-failed-jobs`. The re-run
   uploads the files into that release even though it still ends red. Download the .ipa to check.
4. Check the .ipa (0.8.0 and the build number; 36 core files, plus cArcade once it is in) and give
   the owner the link and the numbered phone steps in the same reply.
5. After the build starts: repo cleanup part 2, from the plan R7 writes (move HANDOFF.md,
   STATUS.md, TESTING.md and the internal docs into `.kiro/notes/` and fix every reference).
