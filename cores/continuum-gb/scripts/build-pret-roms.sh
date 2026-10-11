#!/usr/bin/env bash
# Test-only: builds Pokemon Red/Blue/Yellow/Gold/Crystal from pret's disassemblies (needs make, gcc
# and rgbds 1.0.3 on PATH) into $1 (default /tmp/pretroms). The ROMs are never committed. Then:
#   PRET_DIR=/tmp/pretroms cargo test --release --test pret -- --nocapture
set -euo pipefail
out=${1:-/tmp/pretroms}
work=${PRET_WORK:-/tmp/pret-src}
mkdir -p "$out" "$work"
for repo in pokered pokeyellow pokegold pokecrystal; do
  [ -d "$work/$repo" ] || git clone -q --depth 1 "https://github.com/pret/$repo" "$work/$repo"
  make -C "$work/$repo" -j"$(nproc)" >/dev/null
done
cp "$work/pokered/pokered.gbc" "$work/pokered/pokeblue.gbc" "$work/pokeyellow/pokeyellow.gbc" \
   "$work/pokegold/pokegold.gbc" "$work/pokecrystal/pokecrystal.gbc" "$out/"
ls -l "$out"
