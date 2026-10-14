# Build cache, second pass: verification (14 October 2026)

The checklist is `.kiro/notes/tasks/build-cache.json`. K1 and K2 were written by one helper; K3,
the hostile review, must be done by another. This file holds what that reviewer needs: what was
changed, the commands that were run and what they printed, and where to attack first.

## K1: comments, blank lines, the cache's own code and the lists stop counting

`scripts/build-core.sh` ("WHAT COUNTS", just above `ios_cache_scan`) now leaves four kinds of line
out of every fingerprint:

1. **Comment and blank lines** that bash really reads as comments. A `#` line or blank line still
   counts inside a here-document (any form), inside a quoted string that spans lines (`'...'`,
   `"..."`, `$'...'`), right after a line ending in a backslash, and inside `$( )`, backquotes,
   `<( )`, `(( ))`, `[[ ]]`, `${ }` or an array's `( )`. The `#!` line counts. An end-of-line
   comment counts with its line.
2. **The cache's own code**: three `# not-a-build-input-start: cache` regions holding only whole
   definitions of the functions in `IOS_CACHE_FUNCTIONS` (ios_usage, ios_record_source_version,
   ios_core_patch_family, ios_recipe_family, ios_cache_scan, ios_cache_bash_parse, ios_cache_split,
   ios_recipe_text, ios_core_fingerprint, ios_core_is_cached, ios_cache_command). The same list is
   kept in `scripts/check-core-cache.sh` (`CACHE_FUNCTIONS`); a check fails when they differ.
3. **The lists of cores**: one `# not-a-build-input-start: lists` region that wraps the whole
   `IOS_CORES=( ... )` and `IOS_OPTIONAL_CORES=( ... )` assignments, so a name appended inside
   the parentheses (continuum_arcade tonight) lands inside it by itself.
4. The `IOS_PIN="<sha>"` lines of `ios_core_config`, as before (each pin is hashed on its own).

The region markers are comments too, so they count for nothing: adding a new core's regions plus
its list entry moves no other core's fingerprint.

**How it is proved, on every fingerprint:** (a) `ios_cache_scan`, one POSIX awk program, reads the
file the way bash does, character by character, and checks every rule; (b) bash itself then parses
the whole file twice, as it is and with every left-out line removed, in a separate restricted shell
with no PATH that runs nothing (after `bash -n`), and `declare -f` must print the same program for
both; (c) only then are the kept lines (the file's own text, not bash's printout) hashed. Any
failure is no fingerprint, so `fp:unknown` and a rebuild of every core.

**Rules that fail closed** (no core gets a fingerprint, `ios-cache-check` says why): a mistyped,
nested, unbalanced or misplaced marker; a marker-looking line inside a here-document or string; a
here-document starting on a line that does not end plainly; an unquoted here-document line ending
in a backslash; a here-document delimiter with an expansion; a `case` inside `$( )`; a function
defined twice or written other than `name() {`; anything but whole allow-listed functions in a
cache region; a cache function used outside it except at the five approved lines (ios_clone and
build_all_ios_cores call it in a subshell, the dispatch); anything but `IOS_CORES=(...)` and
`IOS_OPTIONAL_CORES=(...)` of plain names in the lists region; a recipe region for a family no
listed core has; a recipe region that is not whole `ios_core_config` arms, whole
`ios_apply_core_patches` arms, or whole top-level functions; an arm for another family's core in a
region; `IOS_SELF`, `BASH_SOURCE`, `LINENO` or `BASH_LINENO` outside the cache except on the
lines that use them today; a NUL byte; and bash disagreeing with the scan.

Two shared lines changed on purpose, so nothing the cache runs can leak into a build:
`( ios_record_source_version "$core" )` in ios_clone and `if ( ios_core_is_cached "$core" ); then`
(twice) in build_all_ios_cores now run in subshells. The cache subcommands moved into
`ios_cache_command` behind one dispatch line, so editing them later is free.

### What rebuilds what, after K1

- **Every core**: any line bash would run in the shared part (helpers every core goes through,
  common flags, `IOS_MIN_VERSION`, the required-symbol list, staging, `ios_core_config`'s
  defaults), the `#!` line, `IOS_CACHE_FUNCTIONS` or `IOS_CORE_LIST_NAMES`, a new toolchain.
- **One core** (with its JIT or plain twin): a line bash runs inside its own region, its pin, its
  patches, Symbian's plug or the headers script. Unchanged from 393f12c.
- **No core**: comment and blank lines that are really comments, anywhere; the cache's own code
  and the help text; adding, removing or moving a core between the lists; a new core's own regions.

### Commands and results (K1, on 57b579b plus this change)

| Command | Result |
| --- | --- |
| `bash scripts/check-core-cache.sh` | 180 passed, 0 failed (105 before; 1 min 8 s) |
| the same under bash 3.2.57 and BWK awk 20200816, the versions macOS ships (built from source in a scratch folder, first in PATH) | 180 passed, 0 failed |
| `bash scripts/build-core.sh ios-fingerprint <core>` under both tool sets | identical for fceumm, mgba, flycast_jit, continuum_symbian, continuum_gb, ppsspp |
| `bash scripts/check-skins.sh` | 720 checks, 0 failed |
| `bash scripts/check-players.sh` | 0 failed |
| `cargo test --workspace --features emulator-bridge/native-core` | 723 passed, 0 failed (pinned 1.92.0 toolchain) |
| `cargo clippy --workspace --features emulator-bridge/native-core` | no warnings, no errors |

One existing check was reworded, not weakened: check 4 ("a shared build-core.sh edit changes
fceumm/azahar") appended a COMMENT as its shared edit. Since comments no longer count it now
appends a line of code (`: a shared recipe change`), which is what it always meant to test, and
section 13 asserts the opposite for comments. The move lists also got stricter: a core that loses
its fingerprint now shows as `<core>(none)`, so no "these cores moved" check can pass by a core
failing instead.

### Fingerprints before and after (throwaway copies; `ios-fingerprint`)

"same" means unchanged from the "this change" column.

| core | master (393f12c rules) | this change | comment edit | flycast to the required list | a core added (region + list entry) | shared flag (`IOS_MIN_VERSION` 16.0 to 16.1) |
| --- | --- | --- | --- | --- | --- | --- |
| fceumm | 4d5a9c6c1e9bd79a | d489d753d699fe59 | same | same | same | 53c9cb34d98a1c13 |
| mgba | e2e7e533f9b90a3a | c113c9bf68ec75ae | same | same | same | 5f2fbf0bf1e3493c |
| flycast | 5f4ef4dd419ba77e | ce92e68d409f7b14 | same | same | same | a2feecc00e5783ef |
| flycast_jit | 03b7eb800036a979 | fa3c3dbe044ea5a8 | same | same | same | 99b5256c4e553b6a |
| continuum_symbian | 1f9a7b3f3081142a | 692c0e536f2d4cb7 | same | same | same | 02633d6dd315b48f |
| continuum_gb | 897c8cd49b962b58 | 253550f65691b0e8 | same | same | same | 1f9b28f8a9411267 |
| ppsspp | 51d84f5a3977e555 | 0342e7a15df178f7 | same | same | same | e81f5cf3f5783472 |
| stella2023 | b65819d824d87bec | bcf7a68e09b05fc5 | same | same | same | 8a16543a3701d1ee |

Under master's rules the same comment edit, list edit and added core each changed all eight
(fceumm: 8b7eb072471abaf5, 9a5f5cb6327f0868, c7c2fb5c8bc4bbc7). The added core got a fingerprint
of its own (a07bbe59b963e37e). K1 changes every stored fingerprint once, as expected: build 185
cached 393f12c's, so the next build rebuilds every core once and the ones after are quick.

## K2: the three findings of the review of 393f12c

- **(a) A core's patch step cannot sit outside its own region.** Every rule fails closed (no core
  gets a fingerprint) and `ios-cache-check` names it: a patches arm in the shared part; a patches
  arm in another family's region (K1 already); a patches arm holding anything but
  `ios_apply_simple_patch <own core> <family>-*.patch` lines and calls to its own family's helpers;
  `ios_apply_simple_patch` used outside a region; a `scripts/patches/` file named in a region that
  is not that family's, or named in the shared part except by ios_apply_simple_patch and the
  weekly check's Symbian dry run; a function defined in a region used outside its family's regions
  (the only exception: build_ios_core's dispatch for an `IOS_KIND` that only that family sets), or
  never called by name; a config arm doing anything but setting `IOS_` variables to plain values.
  A new core's dispatch may now sit in its own region inside build_ios_core (one `if` block or
  `<kind>) <function> "$core" ;;` lines for a kind only it sets), so adding a core of our own
  needs no shared line at all. A config arm left outside every region still only costs that core
  its own fingerprint (the existing check 12 expects exactly that).
- **(b) The counts.** The comments now say what is really built: twenty cores from source
  (IOS_CORES and IOS_OPTIONAL_CORES), nineteen of them kept by the cache (cBoy is compiled every
  run), sixteen downloaded ready-made; the 13 October Symbian fix recompiled the eighteen others,
  not thirty-six. Fixed in build-core.sh and in the workflow's cache comment ("sixteen cores built
  from source", "all 35 cores").
- **(c) A separator between a family's regions.** Each region of a family is hashed after a header,
  `@@ region <n> of <family>, after shared line <count>, <lines> lines`, so merging, splitting or
  moving a family's regions, or moving lines between them, changes that family's fingerprint and
  no other.

A follow-up commit right after K2 makes the rules read the code of a line rather than a comment
at its end (so a note after an assignment in a config arm is legal; the whole line is still
hashed) and accepts a dispatch written as `elif [[ "$IOS_KIND" == "<kind>" ]]; then`, two
ordinary ways tonight's helpers might write their lines. Fingerprints are unchanged by it.

Commands and results (K2, on master after K1 plus this change): `bash scripts/check-core-cache.sh`
201 passed, 0 failed (21 new checks in section 14), and 201 passed, 0 failed again under bash
3.2.57 with BWK awk 20200816; with the follow-up, 203 and 203; `check-skins.sh` 720 checks, 0 failed;
`check-players.sh` 0 failed; `cargo test` 723 passed, 0 failed; `cargo clippy` clean. K1 is
3651403, K2 is 1de468f.

K2 changes every fingerprint once more (the recipe text gained its headers), so the next build
rebuilds every core once, as K1 already does; the three edits above again move only what they
should:

| core | master (K1) | this change | comment edit | flycast to the required list | a core added | shared flag |
| --- | --- | --- | --- | --- | --- | --- |
| fceumm | d489d753d699fe59 | df688ed0e6cd1c83 | same | same | same | c36691b1c38a5daf |
| mgba | c113c9bf68ec75ae | 1e0dce5f4510cbde | same | same | same | 16c52953860d5bd2 |
| flycast | ce92e68d409f7b14 | 192e66119a67fec3 | same | same | same | 69ebde801cdb9482 |
| flycast_jit | fa3c3dbe044ea5a8 | 95fdd5876f002edd | same | same | same | 05ad65159c095012 |
| continuum_symbian | 692c0e536f2d4cb7 | 5d9627dce740556e | same | same | same | 2f071dfdf6fab7c0 |
| continuum_gb | 253550f65691b0e8 | e6367c28819ad120 | same | same | same | 2d2876a7aad3ecfe |
| ppsspp | 0342e7a15df178f7 | e05fced2954ed33e | same | same | same | ddaf227a5c038eca |
| stella2023 | bcf7a68e09b05fc5 | d7905f4f3320ed37 | same | same | same | 8491065eca115e35 |

## For the reviewer (K3): attack these first

1. The awk lexer in `ios_cache_scan` (`lex`, `heredoc_word`, the heredoc branch): any line it calls
   a comment that bash would run or write. The bash check is the net under it: try to find a drop
   it does NOT catch (bash's `declare -f` printing two different programs the same way).
2. `ios_record_source_version` runs inside every core's build (in a subshell): anything it could do
   to the source tree or the staged files that changes a dylib is build logic in an excluded region.
3. The approved call sites and the self-reference guard: a way for build code to reach cache code,
   or to read this file's text, that the word match misses (`$0`, an alias, `eval`, a re-invocation
   of the script with a cache subcommand).
4. The lists region: anything that makes a listed name change HOW a core is built.
5. Regions: a region placed so its code runs for another family's core (K2 adds more guards here).
