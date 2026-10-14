#!/usr/bin/env bash
# Assembles cArcade's own test programs (test-programs/<board>/*.asm) into the committed .bin
# files beside them, with sjasmplus built from its source release into test-suites/tools/
# (git-ignored). The .bin files are committed so the tests never need the assembler; run this
# after changing a program, and the tests check that the committed bytes are what the source
# assembles to whenever the assembler is here (tests/pacman_board.rs).
set -euo pipefail
cd "$(dirname "$0")/.."
CRATE=$PWD

SJ_VERSION=1.24.0
SJ_SHA256=0b5013f07e8d8505f9e296529655668b6fdf0268625c19e883b0fff484e209f1
SJ="${SJASMPLUS:-$CRATE/test-suites/tools/sjasmplus-$SJ_VERSION/sjasmplus}"
if [ ! -x "$SJ" ]; then
  mkdir -p test-suites/tools && cd test-suites/tools
  tarball=sjasmplus-$SJ_VERSION-src.tar.xz
  curl -fsSL -o "$tarball" \
    "https://github.com/z00m128/sjasmplus/releases/download/v$SJ_VERSION/$tarball"
  if command -v sha256sum >/dev/null 2>&1; then got=$(sha256sum "$tarball" | cut -d' ' -f1)
  else got=$(shasum -a 256 "$tarball" | cut -d' ' -f1); fi
  if [ "$got" != "$SJ_SHA256" ]; then echo "$tarball: sha256 $got, expected $SJ_SHA256" >&2; exit 1; fi
  tar -xJf "$tarball"
  make -C "sjasmplus-$SJ_VERSION" -j2 USE_LUA=0 >/dev/null
  cd "$CRATE"
  SJ="$CRATE/test-suites/tools/sjasmplus-$SJ_VERSION/sjasmplus"
fi

for dir in test-programs/*/; do
  (cd "$dir" && for src in *.asm; do
    "$SJ" --nologo --msg=war "$src" >/dev/null || { echo "$dir$src failed to assemble" >&2; exit 1; }
  done)
done
ls test-programs/*/*.bin
