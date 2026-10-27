#!/usr/bin/env bash
# Assembles cAtari's own test programs (test-programs/*.asm) into the committed .bin files beside
# them, with dasm built from its source release into test-suites/tools/ (git-ignored). The .bin
# files are committed so the tests never need the assembler; run this after changing a program.
set -euo pipefail
cd "$(dirname "$0")/.."
CRATE=$PWD

DASM_VERSION=2.20.14.1
DASM_SHA256=ec71ffd10eeaa70bf7587ee0d79a92cd3f0a017c0d6d793e37d10359ceea663a
DASM="${DASM:-$CRATE/test-suites/tools/dasm-$DASM_VERSION/bin/dasm}"
if [ ! -x "$DASM" ]; then
  mkdir -p test-suites/tools && cd test-suites/tools
  tarball=dasm-$DASM_VERSION.tar.gz
  curl -fsSL -o "$tarball" "https://codeload.github.com/dasm-assembler/dasm/tar.gz/refs/tags/$DASM_VERSION"
  if command -v sha256sum >/dev/null 2>&1; then got=$(sha256sum "$tarball" | cut -d' ' -f1)
  else got=$(shasum -a 256 "$tarball" | cut -d' ' -f1); fi
  if [ "$got" != "$DASM_SHA256" ]; then echo "$tarball: sha256 $got, expected $DASM_SHA256" >&2; exit 1; fi
  tar -xzf "$tarball"
  make -C "dasm-$DASM_VERSION" -j2 >/dev/null
  cd "$CRATE"
  DASM="$CRATE/test-suites/tools/dasm-$DASM_VERSION/bin/dasm"
fi

cd test-programs
for src in *.asm; do
  "$DASM" "$src" -f3 -o"${src%.asm}.bin" >/dev/null || { echo "$src failed to assemble" >&2; exit 1; }
done
ls -la *.bin
