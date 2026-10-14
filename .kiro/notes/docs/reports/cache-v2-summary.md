# Build cache, second pass (cache-v2): summary for the orchestrator

Plain English. The job: a change to `scripts/build-core.sh` for almost any reason used to rebuild
every emulator (about 45-50 minutes of macOS build), because the cache judged each emulator on a
hash of the WHOLE file. This pass made the cache judge each emulator on only what can change how
its own library is built, and then hardened that against every way a change could slip past it.

## What rebuilds what now

**Rebuilds EVERY emulator** (a change to something they all share, that the build really runs):
- a shared helper every core goes through (cloning, staging, the install-name and symbol checks,
  the common compiler/linker flags, `IOS_MIN_VERSION`, the required-symbol list, `ios_core_config`'s
  defaults);
- the `#!` line at the top; the two cache-rule data lines `IOS_CACHE_FUNCTIONS` and
  `IOS_CORE_LIST_NAMES`;
- a new Xcode / toolchain (CI passes its id into every fingerprint).

**Rebuilds ONE emulator** (and the JIT or plain build that shares its source):
- anything inside that emulator's own `# recipe-start: <name>` ... `# recipe-end: <name>` region:
  its clone/checkout, its make or cmake call, its own flags, its post-processing, a build tool only
  it downloads (the Symbian core's pinned CMake and ninja);
- its pinned commit, its `scripts/patches/<family>-*.patch` files, and (Symbian only) its plug under
  `native/continuum-symbian` and the libretro-headers script.

**Rebuilds NO emulator** (counts for nothing):
- comment lines and blank lines, wherever bash really reads them as comments;
- the cache's own code and its help text (the `# not-a-build-input: cache` regions);
- adding an emulator to the lists or removing one, and a new emulator's own regions
  (the `# not-a-build-input: lists` region). So the arcade agent appending `continuum_arcade`
  inside `IOS_OPTIONAL_CORES=( ... )` plus its own region rebuilds only itself.

Everything that counts is in exactly one of those places, and the cache fails closed: anything it
cannot read for certain, any broken rule, any failure to compute a fingerprint gives NO fingerprint
(`fp:unknown`), which never matches, so that core rebuilds. A stale library can never reach the app.

## The commits (all `[skip ci]`, straight to master)

- `3651403` K1 — comments, blank lines, the cache's own code and the lists of emulators stop counting.
- `1de468f` K2 — the three findings of the morning review of 393f12c (patch steps must sit in a
  core's own region; the comment counts corrected; a separator between a family's regions).
- `d8d8654` K2 follow-up — accept a note at the end of a line and an `elif` dispatch.
- `3d411b3` K3 — the hostile review: six ways a change could have dodged the guards or shipped a
  stale emulator, each now caught and forced to rebuild (see docs/reports/cache-v2-verification.md).

## The proof

`scripts/check-core-cache.sh` is the gate, run on Linux in CI and also under the Mac's bash 3.2 and
BWK awk here. It grew from 105 checks (start of this job) to **213**, all passing on both. It also
drives `bash scripts/build-core.sh ios-fingerprint <core>`, the per-core proof. K1 and K2 each
changed the shape of what is hashed, so each moved every stored fingerprint once (one rebuild of
everything, already planned for tonight's build). K3 changed only the non-hashed cache code, so it
moved NO fingerprint and costs no extra rebuild.

## What a future change to build-core.sh must keep in mind

- Put a new emulator's own lines inside its own `# recipe-start: <name>` / `# recipe-end: <name>`
  region, and its name inside the `IOS_CORES` or `IOS_OPTIONAL_CORES` parentheses. Code left
  unmarked lands in the shared part and over-rebuilds (safe), never the other way round.
- A region may hold only: whole `ios_core_config` arms (setting `IOS_` variables to plain values),
  whole `ios_apply_core_patches` arms (only that family's `ios_apply_simple_patch` steps and its own
  patch helpers), whole top-level functions used only by that family, and the `build_ios_core`
  dispatch of a build kind only that family sets.
- Name a Continuum patch by its real file name (`<family>-something.patch`); applying another
  family's patch from a core's region is refused, because that patch is hashed under the other
  family and the fix would otherwise ship stale.
- The cache's own functions are allow-listed in BOTH `IOS_CACHE_FUNCTIONS` (build-core.sh) and
  `CACHE_FUNCTIONS` (check-core-cache.sh); they must agree. Keep every cache-region function body
  side-effect-free apart from writing `core-sources.txt` — the guards allow-list the functions but
  do not stop a body from touching the source tree, which would change a build without changing a
  fingerprint.
- No build line may read the script's own text or line numbers (`$0`, `IOS_SELF`, `BASH_SOURCE`,
  `LINENO`, `BASH_ARGV0`, `caller`), use a `(` inside `[[ ]]`, or hide code after a `#` on a
  continued line — all are refused so comments can safely be left out of the hash.

## Workflow note (parked, no workflow edits allowed)

`.github/workflows/ios.yml` already carries the correct cache comments (updated in K1/K2 before the
no-workflow-edits rule applied here). No further workflow change is needed for this job.
