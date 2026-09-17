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

# ------------------------------------------------------------ 4. signing

# Nested code first: codesign refuses to seal a bundle whose contents change afterwards.
echo "==> ad-hoc signing nested code"
for dylib in "$BUNDLE"/Frameworks/*.dylib; do
  [ -e "$dylib" ] || continue
  codesign --force --sign - --timestamp=none "$dylib"
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
