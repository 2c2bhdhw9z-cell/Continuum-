#!/usr/bin/env bash
# Generates the Xcode project, builds the app, and packages an unsigned .ipa.
#
#   ./package-ipa.sh
#
# Output: native/ios/build/Continuum.ipa
#
# Run build-engine.sh first; this expects build/lib and build/Generated to exist.
#
# ## Why the .ipa is "unsigned"
#
# There is no Apple Developer certificate in CI and none is wanted. The app is installed by
# TrollStore, which does its own signing with a CoreTrust bypass. What it needs from us is a
# correctly structured bundle whose binary carries the right *entitlements* — so the binary
# is ad-hoc signed (`codesign -s -`) with Continuum.entitlements attached. Ad-hoc signing
# embeds the entitlement blob without needing a certificate, and TrollStore preserves it.
#
# That entitlement blob is the whole point: without it the JIT keys and the 12 GB memory
# keys are absent, and an app that cannot map executable memory is not going to run a
# recompiler.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# Needed to reach scripts/build-core.sh, which is the single source of the core dylib names.
ROOT="$(cd "$HERE/../.." && pwd)"
OUT="$HERE/build"
DERIVED="$OUT/DerivedData"
STAGE="$OUT/stage"
APP_NAME="Continuum"
ENTITLEMENTS="$HERE/$APP_NAME.entitlements"

if [ "$(uname -s)" != "Darwin" ]; then
  echo "error: needs a macOS host with Xcode." >&2
  exit 1
fi

[ -d "$OUT/lib" ] || { echo "error: run build-engine.sh first (no $OUT/lib)" >&2; exit 1; }
[ -d "$OUT/Generated" ] || { echo "error: run build-engine.sh first (no $OUT/Generated)" >&2; exit 1; }

# ------------------------------------------------------------ 1. the project

command -v xcodegen >/dev/null 2>&1 || {
  echo "error: xcodegen not found. Install with: brew install xcodegen" >&2
  exit 1
}

# EVERY BUILD NEEDS ITS OWN CFBundleVersion, and it is not a cosmetic detail.
#
# An installer decides whether an .ipa is an upgrade by comparing bundle id and version. This
# project shipped a hardcoded CFBundleVersion of "1" for its whole life, so every build was
# 0.8.0 (1) under the same id, and a freshly built .ipa was indistinguishable from the copy already
# on the phone. On-device signers skip that: signing succeeds, the install reports success, and the
# app is not replaced. The symptom is an app that stubbornly behaves like an older build, which
# looks exactly like a build that was never made.
#
# CONTINUUM_BUILD_NUMBER is the CI run number when CI sets it. Locally it falls back to a UTC
# timestamp, which is monotonic for a human working forwards in time, so a hand build also replaces
# whatever is installed.
# MoltenVK before xcodegen, because project.yml names the framework and XcodeGen resolves that path
# when it generates. Fetched here rather than in CI alone so a local build gets it too, and the
# script is idempotent so this costs nothing after the first run.
echo "==> MoltenVK"
"$ROOT/scripts/fetch-moltenvk.sh"

BUILD_NUMBER="${CONTINUUM_BUILD_NUMBER:-$(date -u +%Y%m%d%H%M)}"
echo "==> stamping CFBundleVersion $BUILD_NUMBER"
# Rewritten in place with a tab-tolerant match on the one key, then asserted, because a silent
# no-op here would put the original bug straight back with no sign of it.
python3 - "$HERE/project.yml" "$BUILD_NUMBER" <<'PY'
import re
import sys

path, build = sys.argv[1], sys.argv[2]
with open(path, "r", encoding="utf-8") as handle:
    text = handle.read()

pattern = re.compile(r'^(\s*CFBundleVersion:\s*)"[^"]*"$', re.MULTILINE)
text, count = pattern.subn(lambda m: f'{m.group(1)}"{build}"', text)
if count != 1:
    sys.exit(f"error: expected exactly one CFBundleVersion line, rewrote {count}")

with open(path, "w", encoding="utf-8") as handle:
    handle.write(text)
print(f"CFBundleVersion set to {build}")
PY

echo "==> xcodegen generate"
(cd "$HERE" && xcodegen generate --spec project.yml --project .)

# ------------------------------------------------------------ 2. the build

echo "==> xcodebuild"
xcodebuild \
  -project "$HERE/$APP_NAME.xcodeproj" \
  -scheme "$APP_NAME" \
  -configuration Release \
  -sdk iphoneos \
  -destination 'generic/platform=iOS' \
  -derivedDataPath "$DERIVED" \
  CODE_SIGNING_ALLOWED=NO \
  CODE_SIGNING_REQUIRED=NO \
  CODE_SIGN_IDENTITY="" \
  build

APP="$DERIVED/Build/Products/Release-iphoneos/$APP_NAME.app"
[ -d "$APP" ] || {
  echo "error: no app bundle at $APP" >&2
  find "$DERIVED/Build/Products" -maxdepth 3 -name '*.app' >&2 || true
  exit 1
}

# ------------------------------------------------------------ 3. the bundle

rm -rf "$STAGE"
mkdir -p "$STAGE/Payload"
cp -R "$APP" "$STAGE/Payload/"
BUNDLE="$STAGE/Payload/$APP_NAME.app"

# The wrapper must be in Frameworks/ for the dlopen in EngineHost.startSession to resolve
# through @executable_path/Frameworks. Xcode's embed phase should have done this; verify
# rather than assume, and place it if the phase was skipped.
mkdir -p "$BUNDLE/Frameworks"
if [ ! -f "$BUNDLE/Frameworks/libcontinuum_switch.dylib" ]; then
  echo "==> embedding libcontinuum_switch.dylib (Xcode did not)"
  cp "$OUT/lib/libcontinuum_switch.dylib" "$BUNDLE/Frameworks/"
fi

# The same fallback for every libretro core (fceumm, mgba, genesis_plus_gx, snes9x, melonds,
# pcsx_rearmed). Each is dlopened at runtime through @executable_path/Frameworks like the
# wrapper; if Xcode's embed phase was skipped for one, place it here so the signing loop
# below still seals it and the app can still load that core on device. An .ipa that is
# missing a core produces an app which launches and then cannot run that one system, which is
# about the hardest thing to diagnose from a phone.
#
# The filenames come from scripts/build-core.sh, the one place they are defined, rather than
# being restated here.
CORE_NAMES="$("$ROOT/scripts/build-core.sh" ios-names)"
[ -n "$CORE_NAMES" ] || {
  echo "error: scripts/build-core.sh ios-names returned nothing; cannot verify the cores" >&2
  exit 1
}
for core_dylib in $CORE_NAMES; do
  # build-engine.sh already hard-failed on a missing core, so this can only mean the two
  # scripts were run out of order. Say that rather than zipping an .ipa with a hole in it.
  [ -f "$OUT/lib/$core_dylib" ] || {
    echo "error: $OUT/lib/$core_dylib is missing; run build-engine.sh first" >&2
    exit 1
  }
  if [ ! -f "$BUNDLE/Frameworks/$core_dylib" ]; then
    echo "==> embedding $core_dylib (Xcode did not)"
    cp "$OUT/lib/$core_dylib" "$BUNDLE/Frameworks/"
  fi
done

# ------------------------------------------------------------ 4. signing

# Nested code first: codesign refuses to seal a bundle whose contents change afterwards.
echo "==> ad-hoc signing nested code"
for dylib in "$BUNDLE"/Frameworks/*.dylib; do
  [ -e "$dylib" ] || continue
  codesign --force --sign - --timestamp=none "$dylib"
done
# FRAMEWORKS AS WELL AS BARE DYLIBS, and this loop was missing until MoltenVK arrived. Every core
# ships as a loose .dylib, so for nine cores the glob above was the whole story; MoltenVK publishes
# its only dynamic iOS build as a .framework, which the glob does not match. An unsigned nested
# bundle makes the outer `codesign` of the .app fail, so its absence would not have been subtle,
# but a nested bundle signed by nobody is exactly the kind of thing that fails on a device and
# passes everywhere else.
for framework in "$BUNDLE"/Frameworks/*.framework; do
  [ -d "$framework" ] || continue
  echo "==> signing nested framework $(basename "$framework")"
  codesign --force --sign - --timestamp=none "$framework"
done

echo "==> ad-hoc signing $APP_NAME.app with entitlements"
[ -f "$ENTITLEMENTS" ] || { echo "error: $ENTITLEMENTS missing" >&2; exit 1; }
codesign --force --sign - --timestamp=none \
  --entitlements "$ENTITLEMENTS" \
  --generate-entitlement-der \
  "$BUNDLE"

# Printed, not assumed. This is the one thing about the .ipa that cannot be checked by
# looking at the file listing, and the one thing that decides whether a JIT will run.
echo "==> entitlements actually embedded:"
codesign -d --entitlements - --xml "$BUNDLE" 2>/dev/null | sed 's/^/      /' || {
  echo "      WARNING: could not read entitlements back" >&2
}

# ------------------------------------------------------------ 5. the .ipa

IPA="$OUT/$APP_NAME.ipa"
rm -f "$IPA"
echo "==> zipping"
(cd "$STAGE" && zip -qry "$IPA" Payload)

echo
echo "==> $IPA ($(du -h "$IPA" | cut -f1))"
echo "    contents:"
unzip -l "$IPA" | sed 's/^/      /'
