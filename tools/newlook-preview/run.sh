#!/usr/bin/env bash
# Builds the New-look motion preview (the real native/ios/NewLookMotion.swift plus the files
# here) for the iOS Simulator, runs it, and saves screenshots a fraction of a second apart and a
# short video into $OUT. macOS with Xcode only. See .github/workflows/newlook-preview.yml.
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
OUT="${OUT:-$ROOT/build/newlook-preview}"
APP="$OUT/NewLookPreview.app"
rm -rf "$OUT" && mkdir -p "$APP"

SDK="$(xcrun --sdk iphonesimulator --show-sdk-path)"
ARCH="$(uname -m)"
echo "==> compiling for $ARCH simulator against $SDK"
xcrun --sdk iphonesimulator swiftc -parse-as-library -swift-version 5 -O \
  -target "$ARCH-apple-ios16.0-simulator" -sdk "$SDK" \
  "$ROOT/native/ios/NewLookMotion.swift" "$HERE/PreviewStubs.swift" "$HERE/PreviewApp.swift" \
  -o "$APP/NewLookPreview"
cp "$HERE/Info.plist" "$APP/Info.plist"
codesign --force --sign - "$APP"

echo "==> picking a simulator"
UDID="$(xcrun simctl list devices available -j | python3 -c '
import json, sys
data = json.load(sys.stdin)["devices"]
best = None
for runtime, devices in data.items():
    if "iOS" not in runtime:
        continue
    for d in devices:
        if d.get("isAvailable") and d["name"].startswith("iPhone"):
            key = (runtime, "Pro" in d["name"])
            if best is None or key > best[0]:
                best = (key, d["udid"], d["name"], runtime)
print(best[1]); print(best[2] + " / " + best[3], file=sys.stderr)
')"
echo "    $UDID"
xcrun simctl boot "$UDID" || true
xcrun simctl bootstatus "$UDID" -b
xcrun simctl install "$UDID" "$APP"
xcrun simctl launch "$UDID" app.continuum.newlookpreview
sleep 6

echo "==> screenshots"
for i in 0 1 2 3 4 5 6 7; do
  xcrun simctl io "$UDID" screenshot --type=png "$OUT/frame-$i.png"
  sleep 0.6
done
echo "==> video"
xcrun simctl io "$UDID" recordVideo --codec=h264 --force "$OUT/motion.mp4" &
REC=$!
sleep 8
kill -INT "$REC" || true
wait "$REC" || true
ls -la "$OUT"
