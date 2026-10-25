#!/usr/bin/env bash
# Fetches the test programs the tests run (never committed; the tests say SKIPPED without them).
# ZEXALL and ZEXDOC for the Master System: Frank D. Cringle's Z80 exercisers (GPL-2.0), ported by
# Eric R. Quinn and Maxim (github.com/maxim-zhao/zexall-sms), v0.21. They print to the SDSC debug
# console as well as the screen, so they run before the picture chip draws anything.
set -euo pipefail
cd "$(dirname "$0")/.."
mkdir -p test-roms
if [ ! -f test-roms/zexall.sms ]; then
  tmp="$(mktemp -d)"
  curl -fsSL -o "$tmp/z.zip" https://github.com/maxim-zhao/zexall-sms/releases/download/v0.21/ZEXALL-SMS-0.21.zip
  unzip -q -o "$tmp/z.zip" zexall.sms zexdoc.sms -d test-roms
  rm -rf "$tmp"
fi
ls -la test-roms
