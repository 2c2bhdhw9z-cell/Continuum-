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
