#!/usr/bin/env bash
# Builds the Rust engine as a static library for iOS, plus the UniFFI Swift bindings.
#
# Run from a macOS host with Xcode. On any other platform this stops early and says why,
# rather than producing something that looks like an iOS build and is not.
#
#   ./build-engine.sh [device|simulator|xcframework]
#
# `staticlib`, not `cdylib`: a static archive lets the linker dead-strip, and it is what a
# Swift target links against. That crate-type was deliberately not added during the Phase 4
# audit — `cargo check` does not link, so the edit would have been unverifiable then.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
MODE="${1:-device}"
OUT="$HERE/build"
FEATURES="native-core,uniffi-bindings"

if [ "$(uname -s)" != "Darwin" ]; then
  cat >&2 <<'EOF'
error: this needs a macOS host.

  The Rust side type-checks anywhere — `cargo check --target aarch64-apple-ios
  --features native-core,uniffi-bindings` runs on Linux and is what CI should verify —
  but linking a staticlib and generating Swift bindings needs the Apple SDK.
EOF
  exit 1
fi

mkdir -p "$OUT"

# `staticlib` has to be in crate-type before this will link. See the note above.
if ! grep -q 'staticlib' "$ROOT/crates/emulator-bridge/Cargo.toml"; then
  cat >&2 <<'EOF'
error: crate-type does not include "staticlib".

  Add it in the same commit that first links a real binary:
      crate-type = ["staticlib", "cdylib", "rlib"]
EOF
  exit 1
fi

build_target() {
  local target="$1"
  echo "==> cargo build --release --target $target"
  (cd "$ROOT" && cargo build --release --target "$target" --features "$FEATURES")
}

case "$MODE" in
  device)     TARGETS=(aarch64-apple-ios) ;;
  simulator)  TARGETS=(aarch64-apple-ios-sim) ;;
  xcframework) TARGETS=(aarch64-apple-ios aarch64-apple-ios-sim) ;;
  *) echo "usage: build-engine.sh [device|simulator|xcframework]" >&2; exit 1 ;;
esac

for target in "${TARGETS[@]}"; do build_target "$target"; done

# UniFFI bindings, generated from the built library rather than from a UDL file: the
# proc-macro mode means the Rust signatures are the single source of truth.
#
# Generated, never committed. A stale checked-in binding that silently disagrees with the
# Rust is the worst failure mode in this stack — it compiles, links, and then corrupts
# arguments at runtime.
echo "==> uniffi-bindgen"
LIB="$ROOT/target/${TARGETS[0]}/release/libemulator_bridge.a"
mkdir -p "$OUT/Generated"
(cd "$ROOT" && cargo run --release --features "$FEATURES" \
  --bin uniffi-bindgen -- generate --library "$LIB" --language swift --out-dir "$OUT/Generated") \
  || cat >&2 <<'EOF'
note: no uniffi-bindgen binary in this workspace yet.

  Add one — it is six lines — so the generator version cannot drift from the runtime:

      // crates/emulator-bridge/src/bin/uniffi-bindgen.rs
      fn main() { uniffi::uniffi_bindgen_main() }
EOF

if [ "$MODE" = "xcframework" ]; then
  echo "==> xcframework"
  xcodebuild -create-xcframework \
    -library "$ROOT/target/aarch64-apple-ios/release/libemulator_bridge.a" \
    -headers "$OUT/Generated" \
    -library "$ROOT/target/aarch64-apple-ios-sim/release/libemulator_bridge.a" \
    -headers "$OUT/Generated" \
    -output "$OUT/EmulatorBridge.xcframework"
fi

cat <<EOF

==> done. Xcode wiring, in order:

  1. Add $OUT/Generated/*.swift to Compile Sources.
  2. Add the modulemap in $OUT/Generated to Import Paths.
  3. Link libemulator_bridge.a (or the xcframework).
  4. Build native/switch-wrapper with 'build.sh ios' and embed the dylib in Frameworks/.
  5. Set Runpath Search Paths to @executable_path/Frameworks — without it dlopen resolves
     in the simulator and fails on device.
  6. Set Code Signing Entitlements to native/ios/Continuum.entitlements.
EOF
