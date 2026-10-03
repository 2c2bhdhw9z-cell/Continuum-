#!/usr/bin/env bash
# Compiles the bundled players' pure-Foundation logic (native/ios/WebPlayerCore.swift) with real
# swiftc and runs scripts/player-check/main.swift, then syntax-checks both bridge scripts with node.
# The key tables and save formats are Rust and are covered by `cargo test` (src/players).
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
work="$root/.work/player-check"
mkdir -p "$work"
swiftc -swift-version 5 -o "$work/check" "$root/native/ios/SkinFunctions.swift" \
  "$root/native/ios/WebPlayerCore.swift" "$root/scripts/player-check/main.swift"
"$work/check" "$work"
node --check "$work/flash-bridge.js"
node --check "$work/j2me-bridge.js"
echo "bridge scripts parse"
