#!/usr/bin/env bash
# Fetches SameBoy (MIT) and the rgbds assembler into $1 (default /tmp/cboy-sameboy), builds
# SameBoy's core, its free boot ROMs and harness.c against them. Nothing here is committed.
# Then: python3 compare-pictures.py dmg|cgbc   and   python3 compare-landings.py dmg|cgbc
# (both read SAMEBOY_WORK, the same folder). The sandbox's git gateway only serves Continuum, so
# SameBoy is cloned with GIT_CONFIG_GLOBAL=/dev/null.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
work="${1:-${SAMEBOY_WORK:-/tmp/cboy-sameboy}}"
mkdir -p "$work"
cd "$work"
[ -d sameboy ] || GIT_CONFIG_GLOBAL=/dev/null git clone -q --depth 1 https://github.com/LIJI32/SameBoy.git sameboy
if [ ! -x rgbds/rgbasm ]; then
  mkdir -p rgbds
  curl -fsSL https://github.com/gbdev/rgbds/releases/download/v1.0.4/rgbds-linux-x86_64.tar.xz | tar xJ -C rgbds
fi
make -C sameboy -j2 bootroms lib CONF=release RGBDS="$work/rgbds/" >make.log 2>&1 || { tail -20 make.log; exit 1; }
gcc -O2 -std=gnu11 -DGB_INTERNAL -D_GNU_SOURCE -o sbh "$here/harness.c" -Isameboy sameboy/build/lib/libsameboy.a -lm 2>/dev/null
echo "built $work/sbh against SameBoy $(git -C sameboy log -1 --format=%h)"
