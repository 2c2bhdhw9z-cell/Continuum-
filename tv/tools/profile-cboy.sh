#!/bin/bash
# profile-cboy.sh <game.gba> [frames]: where the TV's cBoy spends its time, by function and line
# (a sampling profile on the build machine; the TV has no profiler). Set BENCH_OPTS as needed.
TVB=${TVB:-/projects/sandbox/.tvbuild}; . "$TVB/env.sh"
HERE=$(cd "$(dirname "$0")" && pwd)
(cd "$HERE/cboy-bench" && CARGO_TARGET_DIR=$TVB/target-bench cargo build --release -q 2>&1 | grep -E "^error" -A8)
B=$TVB/target-bench/release/cboy-bench
PROF=1 $B "$1" ${2:-6000} 2>&1 | tail -2
sort /tmp/samples.txt | uniq -c | sort -rn > /tmp/agg.txt
awk '{print "0x"$2}' /tmp/agg.txt > /tmp/addrs.txt
addr2line -f -C -e $B < /tmp/addrs.txt | paste - - > /tmp/res.txt
paste <(awk '{print $1}' /tmp/agg.txt) /tmp/res.txt | awk -F'\t' '{f[$2]+=$1; t+=$1} END {for (k in f) printf "%6.2f%% %s\n", f[k]*100/t, k}' | sort -rn | head -30
echo ---
paste <(awk '{print $1}' /tmp/agg.txt) /tmp/res.txt | awk -F'\t' '{split($3,a," "); f[a[1]]+=$1; t+=$1} END {for (k in f) printf "%6.2f%% %s\n", f[k]*100/t, k}' | sort -rn | sed 's#.*/tv/cores/cboy/src/##' | head -30
