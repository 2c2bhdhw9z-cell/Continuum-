#!/bin/bash
# check-cboy.sh [frames]: the TV's cBoy (tv/cores/cboy) against the phone's (cores/continuum-gb).
# Picture and sound fingerprints must be SAME on every game; then the speed of both.
# Games: cBoy's free homebrew set (cores/continuum-gb/scripts/fetch-test-roms.sh fetches it into
# cores/continuum-gb/test-roms/homebrew) and, if present, the owner's own Pokémon games in
# $TVB/private (copied from the TV for this, never into the repo).
set -u
HERE=$(cd "$(dirname "$0")" && pwd); ROOT=$(cd "$HERE/../.." && pwd)
TVB=${TVB:-/projects/sandbox/.tvbuild}; . "$TVB/env.sh"
F=${1:-3000}
# the reference: the same bench, built against the phone's cBoy
rm -rf "$TVB/bench-ref" && cp -r "$HERE/cboy-bench" "$TVB/bench-ref"
sed -i "s#path = \"../../cores/cboy\"#path = \"$ROOT/cores/continuum-gb\"#; s#package = \"cboy-tv\"#package = \"continuum_gb\"#" "$TVB/bench-ref/Cargo.toml"
(cd "$HERE/cboy-bench" && CARGO_TARGET_DIR=$TVB/target-bench cargo build --release -q 2>&1 | grep -E "^error" -A8)
(cd "$TVB/bench-ref" && CARGO_TARGET_DIR=$TVB/target-bench-ref cargo build --release -q 2>&1 | grep -E "^error" -A8)
NEW=$TVB/target-bench/release/cboy-bench; REF=$TVB/target-bench-ref/release/cboy-bench
games=("$ROOT"/cores/continuum-gb/test-roms/homebrew/*.gba "$TVB"/private/*.gba)
for g in "${games[@]}"; do
  [ -f "$g" ] || continue
  for o in "" cboy_idle_skip=on; do
    a=$(BENCH_OPTS=$o HASH=1 "$REF" "$g" "$F" 2>&1 | tail -1 | awk '{print $NF}')
    b=$(BENCH_OPTS=$o HASH=1 "$NEW" "$g" "$F" 2>&1 | tail -1 | awk '{print $NF}')
    printf "%-45s %-20s %s\n" "$(basename "$g" | cut -c1-45)" "${o:-plain}" "$([ "$a" = "$b" ] && echo SAME || echo "DIFFERENT ($a vs $b)")"
  done
done
for g in "$TVB"/private/*Emerald*.gba "$ROOT"/cores/continuum-gb/test-roms/homebrew/skyland.gba; do
  [ -f "$g" ] || continue
  printf "speed %-30s phone cBoy %s   TV cBoy %s\n" "$(basename "$g" | cut -c1-30)" \
    "$(BENCH_OPTS=cboy_idle_skip=on "$REF" "$g" 4000 2>&1 | tail -1 | grep -o '[0-9.]* fps')" \
    "$(BENCH_OPTS=cboy_idle_skip=on "$NEW" "$g" 4000 2>&1 | tail -1 | grep -o '[0-9.]* fps')"
done
