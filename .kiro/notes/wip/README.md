# Work cut off mid-item (14 October 2026, about 9:10 AM Eastern)

Never checked green, so it is kept here, outside every build path, instead of in the source. Apply
it in a fresh clone of master, run that job's checks, carry on, and delete the file in the commit
that lands the work.

| File | Job, item | What it holds |
|---|---|---|
| `cboy-wip-base-eafddae.patch` | `cboy.json`, G2 | The first IO-write-timing edits to `bus.rs`, `cpu.rs` and `fifo.rs`, and tests in `tests/roms.rs` (54 lines). |

Apply with `git apply .kiro/notes/wip/<file>`. Both applied cleanly to master on 14 October at
1:40 PM; if master has moved on and one doesn't, use `git apply --3way`.

`tools/` holds the scratch tools those jobs used, which a new sandbox won't have:

- `build-mame-deps.sh`, `build-mame.sh` and `mame-READY.txt`: the local reference MAME 0.289 for
  cArcade's comparisons (only the drivers it needs; never shipped, never committed). In a new
  sandbox run `bash .kiro/notes/wip/tools/build-mame-deps.sh` in the foreground (about 15
  minutes; it says what to do if a run gets cut off).
- `sbharness/`: our SameBoy reference harness for cBoy's exact drawer (`harness.c`, `mb.py`,
  `show.py`), built against a SameBoy checkout fetched with `GIT_CONFIG_GLOBAL=/dev/null`.
