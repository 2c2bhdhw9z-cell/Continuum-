#!/usr/bin/env bash
# Builds the New-look preview (the REAL native/ios/NewLookColors.swift, NewLookMotion.swift and
# NewLookDesign.swift, plus PreviewApp.swift here) for the iOS Simulator, runs each page and saves
# screenshots into $OUT, with the owner's mockup beside them. macOS with Xcode only.
# See .github/workflows/newlook-preview.yml.
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
OUT="${OUT:-$ROOT/build/newlook-preview}"
APP="$OUT/NewLookPreview.app"
BUNDLE_ID=app.continuum.newlookpreview
rm -rf "$OUT" && mkdir -p "$APP"

# Every simulator command gets a time limit: a stuck simulator once held a run for 30 minutes.
# (macOS has no `timeout` command.)
limit() {
  local secs=$1; shift
  "$@" &
  local pid=$!
  ( sleep "$secs"; kill -9 "$pid" 2>/dev/null ) &
  local watcher=$!
  local rc=0
  wait "$pid" || rc=$?
  kill "$watcher" 2>/dev/null || true
  return $rc
}

SDK="$(xcrun --sdk iphonesimulator --show-sdk-path)"
ARCH="$(uname -m)"
echo "==> compiling for $ARCH simulator against $SDK"
xcrun --sdk iphonesimulator swiftc -parse-as-library -swift-version 5 -O \
  -target "$ARCH-apple-ios16.0-simulator" -sdk "$SDK" \
  "$ROOT/native/ios/NewLookColors.swift" \
  "$ROOT/native/ios/NewLookMotion.swift" \
  "$ROOT/native/ios/NewLookDesign.swift" \
  "$HERE/PreviewApp.swift" \
  -o "$APP/NewLookPreview"
cp "$HERE/Info.plist" "$APP/Info.plist"
cp "$ROOT/.kiro/notes/docs/new-look/mockup-home.jpg" "$APP/"
for c in gb gbc gba snes n64; do
  cp "$ROOT/native/ios/Assets.xcassets/console-$c.imageset/console.png" "$APP/console-$c.png"
done
codesign --force --sign - "$APP"

echo "==> picking a simulator (the owner's phone is an iPhone 17 Pro Max)"
UDID="$(xcrun simctl list devices available -j | python3 -c '
import json, sys
data = json.load(sys.stdin)["devices"]
best = None
for runtime, devices in data.items():
    if "iOS" not in runtime:
        continue
    for d in devices:
        if d.get("isAvailable") and d["name"].startswith("iPhone"):
            name = d["name"]
            key = (runtime, "Pro Max" in name, "Pro" in name, "17" in name)
            if best is None or key > best[0]:
                best = (key, d["udid"], name, runtime)
print(best[1]); print(best[2] + " / " + best[3], file=sys.stderr)
')"
echo "    $UDID"
limit 120 xcrun simctl boot "$UDID" || true
limit 300 xcrun simctl bootstatus "$UDID" -b
# A tidy status bar, like a marketing screenshot.
limit 30 xcrun simctl status_bar "$UDID" override --time "9:41" --batteryLevel 100 \
  --cellularBars 4 --wifiBars 3 || true
limit 120 xcrun simctl install "$UDID" "$APP"

# shoot NAME PAGE [ACCENT]
shoot() {
  local name=$1 page=$2 accent=${3:-}
  limit 30 xcrun simctl terminate "$UDID" "$BUNDLE_ID" 2>/dev/null || true
  if [ -n "$accent" ]; then
    limit 60 xcrun simctl launch "$UDID" "$BUNDLE_ID" -page "$page" -accent "$accent"
  else
    limit 60 xcrun simctl launch "$UDID" "$BUNDLE_ID" -page "$page"
  fi
  sleep 5
  echo "==> $name"
  limit 60 xcrun simctl io "$UDID" screenshot --type=png "$OUT/$name.png"
  # A second one a few seconds later, to show what moves.
  sleep 4
  limit 60 xcrun simctl io "$UDID" screenshot --type=png "$OUT/$name-later.png"
}
# film NAME ACCENT SWIRL [OWN]: ten seconds of the background alone, as a video.
film() {
  local name=$1 accent=$2 swirl=$3 own=${4:-}
  limit 30 xcrun simctl terminate "$UDID" "$BUNDLE_ID" 2>/dev/null || true
  if [ -n "$own" ]; then
    limit 60 xcrun simctl launch "$UDID" "$BUNDLE_ID" -page bg -accent "$accent" -own "$own"
  else
    limit 60 xcrun simctl launch "$UDID" "$BUNDLE_ID" -page bg -accent "$accent" -swirl "$swirl"
  fi
  sleep 4
  echo "==> film $name"
  xcrun simctl io "$UDID" recordVideo --codec=h264 --force "$OUT/$name.mp4" &
  local rec=$!
  sleep 10
  kill -INT "$rec" 2>/dev/null || true
  ( sleep 30; kill -9 "$rec" 2>/dev/null ) &
  wait "$rec" 2>/dev/null || true
}
film bg-purple A66BFF-FF5FB0 accent
film bg-own3 A66BFF-FF5FB0 own A66BFF-FF5FB0-3CD278
film bg-gold FFD66B-F2B233 accent
shoot home-purple home A66BFF-FF5FB0
shoot home home

cp "$ROOT/.kiro/notes/docs/new-look/mockup-home.jpg" "$OUT/mockup-home.jpg"
rm -rf "$APP"
ls -la "$OUT"
