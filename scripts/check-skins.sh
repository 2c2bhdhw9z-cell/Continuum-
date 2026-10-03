#!/usr/bin/env bash
# Compiles the pure-Foundation skin code with real swiftc (Linux or macOS) and runs the checks in
# scripts/skin-check/main.swift: Manic and Delta identifiers, sharing groups, the skin library's
# rules, the function list, the dispatcher table against its source, the temporary stubs, and the
# Manic switch/function item parsing.
#
# The app's own files are compiled, not copies. DeltaSkinNormalizedRect is sliced out of
# DeltaSkinImport.swift (the rest of that file needs UIKit), and EngineHost is a one-line stub so
# SkinFunctionPending.swift can be compiled and called.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
ios="$root/native/ios"
work="$root/.work/skin-check"
mkdir -p "$work"

{
  echo "import Foundation"
  awk '/^\/\/\/ A rectangle in mappingSize space/{p=1} /^\/\/\/ Decoded skin artwork bytes/{p=0} p' \
    "$ios/DeltaSkinImport.swift"
} > "$work/rect.swift"
grep -q "struct DeltaSkinNormalizedRect" "$work/rect.swift" || { echo "rect slice failed"; exit 1; }

cat > "$work/host-stub.swift" <<'EOF'
import Foundation
final class EngineHost { init() {} }
EOF

swiftc -swift-version 5 -o "$work/check" \
  "$work/rect.swift" "$work/host-stub.swift" \
  "$ios/SkinLibrary.swift" "$ios/SkinFunctions.swift" "$ios/ManicSkinItems.swift" \
  "$root/scripts/skin-check/EngineHostStubs.swift" \
  "$root/scripts/skin-check/main.swift"
"$work/check" "$ios"
