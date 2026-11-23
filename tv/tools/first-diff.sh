#!/bin/bash
# first-diff.sh <game.gba> [frames]: the first frame where the TV's cBoy and the phone's part
# ways, picture (v) or sound (a). Set BENCH_OPTS as needed. Builds both benches the way
# check-cboy.sh does.
set -u
HERE=$(cd "$(dirname "$0")" && pwd); ROOT=$(cd "$HERE/../.." && pwd)
TVB=${TVB:-/projects/sandbox/.tvbuild}; . "$TVB/env.sh"
rm -rf "$TVB/bench-ref" && cp -r "$HERE/cboy-bench" "$TVB/bench-ref"
sed -i "s#path = \"../../cores/cboy\"#path = \"$ROOT/cores/continuum-gb\"#; s#package = \"cboy-tv\"#package = \"continuum_gb\"#" "$TVB/bench-ref/Cargo.toml"
(cd "$HERE/cboy-bench" && CARGO_TARGET_DIR=$TVB/target-bench cargo build --release -q 2>&1 | grep -E "^error" -A8)
(cd "$TVB/bench-ref" && CARGO_TARGET_DIR=$TVB/target-bench-ref cargo build --release -q 2>&1 | grep -E "^error" -A8)
F=${2:-3000}
HASH=1 FRAMES=1 "$TVB/target-bench-ref/release/cboy-bench" "$1" "$F" 2>/dev/null > "$TVB/frames-ref.txt"
HASH=1 FRAMES=1 "$TVB/target-bench/release/cboy-bench" "$1" "$F" 2>/dev/null > "$TVB/frames-new.txt"
paste -d' ' "$TVB/frames-ref.txt" "$TVB/frames-new.txt" | awk '
  $3 != $8 && !v { print "picture first differs at frame " $1; v = 1 }
  $5 != $10 && !a { print "sound first differs at frame " $1; a = 1 }
  END { if (!v) print "picture SAME"; if (!a) print "sound SAME" }'
