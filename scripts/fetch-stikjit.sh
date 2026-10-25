#!/usr/bin/env bash
#
# Fetch StikJIT's source into native/ios/build/stikjit/, for Continuum's built-in JIT helper
# (native/ios/JITHelper). StikJIT is the StikDebug team's library for turning JIT on from inside an
# app, with the phone's own pairing file and LocalDevVPN: what Manic and DolphiniOS call their
# built-in debugger. MPL-2.0; it bundles idevice (MIT) and the universal.js script (StikDebug,
# AGPL-3.0). Credited in Settings, ABOUT, Open-source credits.
#
# WHY THE SOURCE AND NOT THE RELEASE'S XCFRAMEWORK. The prebuilt framework is Swift-only, and its
# module interface was written by a newer Swift (6.4, Xcode 27) than the runner's Xcode. Compiling
# the eleven source files into the helper with our own Xcode removes that mismatch. The idevice
# library inside it is a plain static C library (Rust underneath), which any Xcode links.
#
# PINNED to a tag AND its commit, checked after the clone, so a moved tag cannot change what ships.
set -euo pipefail

STIKJIT_TAG="1.9.0"
STIKJIT_COMMIT="32287268fa5824f9edce4cb359f5833ce0cf7b00"
URL="https://github.com/StikDebug/StikJIT.git"

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/.." && pwd)"
OUT="$ROOT/native/ios/build/stikjit"

if [ -f "$OUT/.commit" ] && [ "$(cat "$OUT/.commit")" = "$STIKJIT_COMMIT" ] && [ -f "$OUT/idevice/libidevice_ffi.a" ]; then
  echo "==> StikJIT $STIKJIT_TAG already present"
  exit 0
fi

WORK="$ROOT/.work/stikjit"
rm -rf "$WORK" "$OUT"
mkdir -p "$(dirname "$WORK")" "$(dirname "$OUT")"
echo "==> fetching StikJIT $STIKJIT_TAG"
git -c advice.detachedHead=false clone --quiet --depth 1 --branch "$STIKJIT_TAG" "$URL" "$WORK"
GOT="$(git -C "$WORK" rev-parse HEAD)"
if [ "$GOT" != "$STIKJIT_COMMIT" ]; then
  echo "error: StikJIT $STIKJIT_TAG is commit $GOT, expected $STIKJIT_COMMIT (the tag moved upstream)" >&2
  exit 1
fi
for need in Sources/StikJIT.swift Resources/universal.js idevice/idevice.h idevice/libidevice_ffi.a \
            idevice/module.modulemap iokit/module.modulemap iokit/StikJITIOKit.h LICENSE; do
  [ -f "$WORK/$need" ] || { echo "error: StikJIT $STIKJIT_TAG has no $need" >&2; exit 1; }
done
mkdir -p "$OUT"
cp -R "$WORK/Sources" "$WORK/Resources" "$WORK/idevice" "$WORK/iokit" "$WORK/LICENSE" "$OUT/"
echo "$STIKJIT_COMMIT" > "$OUT/.commit"
echo "==> StikJIT $STIKJIT_TAG in $OUT ($(ls "$OUT/Sources" | wc -l | tr -d ' ') source files)"
