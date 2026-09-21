#!/usr/bin/env bash
# Builds everything the Xcode target needs, and leaves it in native/ios/build/.
#
#   ./build-engine.sh
#
# Produces:
#   build/lib/libemulator_bridge.a        the Rust engine, linked into the app
#   build/lib/libcontinuum_switch.dylib   the C++ libretro wrapper, dlopened at runtime
#   build/lib/*_libretro_ios.dylib        the real libretro cores, also dlopened
#   build/Generated/*.swift               the UniFFI facade
#   build/Generated/*.h                   its C header
#   build/Generated/module.modulemap      renamed so clang finds it by directory
#
# Requires a macOS host with Xcode. The Rust type-checks anywhere — `cargo check --target
# aarch64-apple-ios --features native-core,uniffi-bindings` runs on Linux and CI does exactly
# that — but linking and binding generation need the Apple SDK.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
OUT="$HERE/build"
LIBDIR="$OUT/lib"
GENDIR="$OUT/Generated"
TARGET="aarch64-apple-ios"
FEATURES="native-core,uniffi-bindings"

if [ "$(uname -s)" != "Darwin" ]; then
  cat >&2 <<'EOF'
error: this needs a macOS host.

  The Rust side type-checks anywhere:
      cargo check --target aarch64-apple-ios --features native-core,uniffi-bindings
  but linking a staticlib, generating Swift bindings and running xcodebuild need the
  Apple SDK. Use the ios-build workflow — that is what it is for.
EOF
  exit 1
fi

rm -rf "$OUT"
mkdir -p "$LIBDIR" "$GENDIR"

# ---------------------------------------------------------------- 1. the engine

echo "==> rustup target add $TARGET"
rustup target add "$TARGET"

# The `ios` profile inherits release but sets `panic = "unwind"`, so a Rust panic on device
# crosses the FFI boundary as UniFFI's `rustPanic` and reaches the HUD instead of aborting
# the process. It is a *custom* profile, so cargo writes its artefacts to
# `target/$TARGET/ios/`, not `.../release/` — hence RUST_OUT below. See Cargo.toml.
echo "==> cargo build --profile ios --target $TARGET --features $FEATURES"
# Builds every crate-type at once: the .a is what Xcode links, and the .dylib is what
# UniFFI reads metadata out of. One build, both artefacts.
(cd "$ROOT" && cargo build --profile ios --target "$TARGET" --features "$FEATURES")

RUST_OUT="$ROOT/target/$TARGET/ios"
STATIC_LIB="$RUST_OUT/libemulator_bridge.a"
DYLIB="$RUST_OUT/libemulator_bridge.dylib"

[ -f "$STATIC_LIB" ] || { echo "error: $STATIC_LIB was not produced" >&2; exit 1; }
cp "$STATIC_LIB" "$LIBDIR/"
echo "==> $LIBDIR/libemulator_bridge.a ($(du -h "$STATIC_LIB" | cut -f1))"

# ------------------------------------------------------- 2. the UniFFI bindings

echo "==> building the binding generator for the host"
(cd "$ROOT" && cargo build --release -p continuum-uniffi-bindgen)
BINDGEN="$ROOT/target/release/uniffi-bindgen"
[ -x "$BINDGEN" ] || { echo "error: $BINDGEN missing" >&2; exit 1; }

# Library mode, from the *dylib*. Not the staticlib: UniFFI's `calc_cdylib_name` only
# recognises .so/.dll/.dylib, so pointing it at the .a finds no metadata at all. And not from
# a UDL file, because the interface is declared with #[uniffi::export] proc-macros, so the
# compiled artefact is the single source of truth.
#
# The iOS cdylib is preferred because the engine build above produces it for free. If it is
# absent — linking a cdylib for a device target is the one step here that could not be
# rehearsed off a Mac — fall back to a host build. The metadata UniFFI reads is the interface
# description, which is target-independent, so the generated Swift is byte-identical either
# way. Slower, but it cannot leave the build without bindings.
BINDGEN_LIB="$DYLIB"
if [ ! -f "$BINDGEN_LIB" ]; then
  echo "==> no iOS cdylib; falling back to a host build for metadata"
  (cd "$ROOT" && cargo build --release --features "$FEATURES")
  for candidate in "$ROOT/target/release/libemulator_bridge.dylib" \
                   "$ROOT/target/release/libemulator_bridge.so"; do
    [ -f "$candidate" ] && BINDGEN_LIB="$candidate" && break
  done
fi

if [ ! -f "$BINDGEN_LIB" ]; then
  cat >&2 <<'EOF'
error: no cdylib to read UniFFI metadata from.

  Neither the iOS nor the host cdylib was produced. Check that crate-type in
  crates/emulator-bridge/Cargo.toml still includes "cdylib" — the staticlib cannot
  substitute, because UniFFI's library mode does not read static archives.
EOF
  exit 1
fi

echo "==> uniffi-bindgen generate --library $(basename "$BINDGEN_LIB") --language swift"
"$BINDGEN" generate --library "$BINDGEN_LIB" --language swift --out-dir "$GENDIR" --no-format

# Xcode finds a Clang module by looking for `module.modulemap` in a header search path.
# UniFFI names it after the module instead, so it is renamed here rather than adding a
# per-file -fmodule-map-file flag to the target.
MODULEMAP="$(find "$GENDIR" -name '*.modulemap' -maxdepth 1 | head -1)"
if [ -z "$MODULEMAP" ]; then
  echo "error: uniffi generated no modulemap in $GENDIR" >&2
  ls -la "$GENDIR" >&2
  exit 1
fi
if [ "$(basename "$MODULEMAP")" != "module.modulemap" ]; then
  mv "$MODULEMAP" "$GENDIR/module.modulemap"
fi

echo "==> generated:"
ls -1 "$GENDIR" | sed 's/^/      /'

# ----------------------------------------------------- 3. the C++ core wrapper

echo "==> building the Switch wrapper for iOS"
"$ROOT/native/switch-wrapper/build.sh" ios
WRAPPER="$ROOT/native/switch-wrapper/build/libcontinuum_switch.dylib"
[ -f "$WRAPPER" ] || { echo "error: $WRAPPER was not produced" >&2; exit 1; }
cp "$WRAPPER" "$LIBDIR/"
echo "==> $LIBDIR/libcontinuum_switch.dylib ($(du -h "$WRAPPER" | cut -f1))"

# ------------------------------------------------ 4. the libretro cores

# The real cores the .ipa ships, all compiled from source for iOS by scripts/build-core.sh
# and dlopened at runtime like the wrapper above:
#
#   fceumm           NES
#   mgba             GBA + GB/GBC
#   genesis_plus_gx  Mega Drive + Master System + Game Gear
#   snes9x           SNES
#   pcsx_rearmed     PS1, INTERPRETER-only (its makefile force-disables the JIT for iOS
#                    arm64), which is the same build that already worked
#
# build-core.sh clones each core into .work/ios/, runs its own build for ios-arm64, fixes the
# @rpath install_name and stages the .dylib straight into build/lib/ (this same $LIBDIR).
# `ios-all` builds them all and deliberately keeps going after a failure, so one broken core
# cannot hide the state of the other four; it still exits non-zero, so this script still
# stops before an .ipa can be built with a hole in it.
#
# The expected filenames are READ from build-core.sh rather than restated here. They have to
# match byte for byte in project.yml, package-ipa.sh, ios.yml and ContinuumApp.swift already,
# and a list copied into a fifth place is a list that eventually disagrees with the build.
echo "==> asking build-core.sh which core dylibs to expect"
CORE_NAMES="$("$ROOT/scripts/build-core.sh" ios-names)"
CORE_COUNT="$(printf '%s\n' "$CORE_NAMES" | grep -c '\.dylib$' || true)"
# CHECKS THE SHAPE, NOT A MAGIC NUMBER. This used to assert exactly 5, which is the one thing a
# guard here must not do: adding a sixth core turned a correct list into a build failure, and the
# error told the reader the list was wrong when the assertion was. What this is actually defending
# against is `ios-names` returning nothing, or returning something that is not a list of core
# filenames, either of which would let the rest of this script proceed on an empty set and produce
# an .ipa with no cores in it. Both of those are caught by requiring at least one entry and
# requiring every entry to look like a core dylib, and neither needs editing when a core is added.
[ "$CORE_COUNT" -ge 1 ] || {
  echo "error: build-core.sh ios-names listed no core dylibs at all" >&2
  printf '%s\n' "$CORE_NAMES" >&2
  exit 1
}
MALFORMED="$(printf '%s\n' "$CORE_NAMES" | grep -v '_libretro_ios\.dylib$' || true)"
[ -z "$MALFORMED" ] || {
  echo "error: build-core.sh ios-names returned entries that are not core dylibs:" >&2
  printf '%s\n' "$MALFORMED" >&2
  exit 1
}
printf '%s\n' "$CORE_NAMES" | sed 's/^/      /'

# Three files still have to spell these names out by hand, and none of them can be checked by a
# compiler: project.yml decides what Xcode embeds, ios.yml decides what the .ipa is asserted to
# contain, and ContinuumApp.swift decides what the app looks for in Frameworks/. A typo in any of
# them produces a build that goes green and an app that cannot find a core. Checked HERE, before
# twenty minutes of core compiles, because a wrong string should cost seconds.
echo "==> checking the core filenames agree across project.yml, ios.yml and ContinuumApp.swift"
DIVERGED=""
for core_dylib in $CORE_NAMES; do
  for consumer in "$HERE/project.yml" \
                  "$ROOT/.github/workflows/ios.yml" \
                  "$HERE/ContinuumApp.swift"; do
    grep -q "$core_dylib" "$consumer" || {
      echo "error: $(basename "$consumer") does not mention $core_dylib" >&2
      DIVERGED="$DIVERGED $(basename "$consumer"):$core_dylib"
    }
  done
done
[ -z "$DIVERGED" ] || {
  echo "error: the core dylib names have diverged:$DIVERGED" >&2
  echo "       scripts/build-core.sh ios_core_config is the definition; fix the consumer." >&2
  exit 1
}
# Counted rather than spelled, so adding a core cannot leave this line claiming a number it no
# longer checks. The list comes from `ios-names`, which is the one definition.
echo "      all $(echo "$CORE_NAMES" | wc -w | tr -d ' ') names present in all three"

echo "==> building $(echo "$CORE_NAMES" | wc -w | tr -d ' ') libretro cores for iOS"
"$ROOT/scripts/build-core.sh" ios-all

# Hard-fail, naming every file that is absent. Shipping an .ipa without a core would launch
# and then fail to load that core on device, which is far harder to diagnose than a red build
# here. The loop reports all of them rather than stopping at the first.
MISSING=""
for core_dylib in $CORE_NAMES; do
  if [ -f "$LIBDIR/$core_dylib" ]; then
    echo "==> $LIBDIR/$core_dylib ($(du -h "$LIBDIR/$core_dylib" | cut -f1))"
  else
    echo "error: $LIBDIR/$core_dylib was not produced" >&2
    MISSING="$MISSING $core_dylib"
  fi
done
[ -z "$MISSING" ] || {
  echo "error: missing iOS core dylib(s):$MISSING" >&2
  exit 1
}

cat <<EOF

==> engine ready in $OUT

    Next: native/ios/package-ipa.sh
EOF
