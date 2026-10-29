# Work cut off mid-item

Never checked green, so it is kept here, outside every build path, instead of in the source. Apply
it in a fresh clone of master, run that job's checks, carry on, and delete the file in the commit
that lands the work.

| File | Job, item | What it holds |
|---|---|---|
| `cboy-gba-dma-preempt.patch` | `tasks/cboy-finish.json` B11 | A higher Advance DMA channel cutting into a lower one between units (cores/continuum-gb/src/gba/bus.rs). Builds, keeps the 65 pinned timing tests; changed no result on its own. |

Apply with `git apply .kiro/notes/wip/<file>`. Both applied cleanly to master on 14 October at
1:40 PM; if master has moved on and one doesn't, use `git apply --3way`.

`tools/` holds the scratch tools those jobs used, which a new sandbox won't have:

- `build-mame-deps.sh`, `build-mame.sh` and `mame-READY.txt`: the local reference MAME 0.289 for
  cArcade's comparisons (only the drivers it needs; never shipped, never committed). In a new
  sandbox run `bash .kiro/notes/wip/tools/build-mame-deps.sh` in the foreground (about 15
  minutes; it says what to do if a run gets cut off).
- `sbharness/`: our SameBoy reference harness for cBoy's exact drawer (`harness.c`, `mb.py`,
  `show.py`), built against a SameBoy checkout fetched with `GIT_CONFIG_GLOBAL=/dev/null`.
- `mgba-cheats/`: our harness around mGBA's own Advance cheat code, for
  `cores/continuum-gb/tests/gba_cheats_mgba.rs` (`bash .kiro/notes/wip/tools/mgba-cheats/build.sh`,
  under a minute; `.kiro/notes/docs/CBOY_CHEATS.md`).
- `pokemini/`: PokeMini (the emulator cMini replaces) built headless: `cpuharness` (its processor
  alone) and `pmharness` (the whole console, started as its libretro core starts it), for
  `cores/continuum-mini/tests/cpu_pokemini.rs` and `console_pokemini.rs`
  (`bash .kiro/notes/wip/tools/pokemini/build.sh`, under a minute;
  `.kiro/notes/docs/POKEMON_MINI_CORE.md`).
- `mgba-timing/`: mGBA's core (the Advance emulator cBoy replaces, at the commit the app ships)
  built headless, and `runner`, which runs it on test ROMs that report in r12 (Alyosha's timing
  tests): `bash .kiro/notes/wip/tools/mgba-timing/build.sh` (about 3 minutes), then
  `/projects/sandbox/.scratch/mgba-build/runner <roms...>`. On 29 October it passed 39 of the 133.
