#!/usr/bin/env bash
# Builds the Rust bridge to WebAssembly and emits ES-module glue into
# web/vendor/bridge/.
#
# No bundler anywhere in this project: the UI is plain ES modules served straight
# off disk, so `--target web` glue can be imported directly. Keeps the dev loop to
# "run this script, reload the page".
#
# Usage: scripts/build-wasm.sh [--debug]
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_DIR="$ROOT/web/vendor/bridge"
CRATE="emulator-bridge"
TARGET="wasm32-unknown-unknown"

PROFILE="release"
PROFILE_FLAG="--release"
if [[ "${1:-}" == "--debug" ]]; then
  PROFILE="debug"
  PROFILE_FLAG=""
fi

command -v wasm-bindgen >/dev/null 2>&1 || {
  echo "error: wasm-bindgen not found. Install with:" >&2
  echo "  cargo install wasm-bindgen-cli --version 0.2.128" >&2
  exit 1
}

echo "==> cargo build ($PROFILE, $TARGET)"
cd "$ROOT"
# shellcheck disable=SC2086
cargo build -p "$CRATE" --target "$TARGET" $PROFILE_FLAG

WASM_IN="$ROOT/target/$TARGET/$PROFILE/emulator_bridge.wasm"
[[ -f "$WASM_IN" ]] || { echo "error: $WASM_IN not produced" >&2; exit 1; }

echo "==> wasm-bindgen -> $OUT_DIR"
rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR"
wasm-bindgen "$WASM_IN" \
  --out-dir "$OUT_DIR" \
  --target web

# Optional size pass. wasm-opt typically trims 15-25% off a wgpu build; skipped
# silently when binaryen is not installed so the script stays dependency-light.
if command -v wasm-opt >/dev/null 2>&1 && [[ "$PROFILE" == "release" ]]; then
  echo "==> wasm-opt -Oz"
  wasm-opt -Oz --enable-bulk-memory --enable-nontrapping-float-to-int \
    "$OUT_DIR/emulator_bridge_bg.wasm" -o "$OUT_DIR/emulator_bridge_bg.wasm.opt"
  mv "$OUT_DIR/emulator_bridge_bg.wasm.opt" "$OUT_DIR/emulator_bridge_bg.wasm"
else
  echo "==> wasm-opt not found or debug build; skipping size optimisation"
fi

SIZE=$(du -h "$OUT_DIR/emulator_bridge_bg.wasm" | cut -f1)
echo "==> done: $OUT_DIR/emulator_bridge_bg.wasm ($SIZE)"
