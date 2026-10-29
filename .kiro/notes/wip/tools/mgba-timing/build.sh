#!/usr/bin/env bash
# Builds mGBA's core (the Advance emulator cBoy replaces, at the commit the app ships, the same
# source as ../mgba-cheats) headless, and `runner`, which runs it on test ROMs that report in r12
# (Alyosha's timing tests, jsmolka's), so cBoy's results can be put next to mGBA's. Scratch only.
#   bash .kiro/notes/wip/tools/mgba-timing/build.sh   (about 3 minutes)
set -euo pipefail
PIN=7a12d6d4b9acb14c0ae62c9166b6a2f3d08007f6
SRC=/projects/sandbox/.scratch/mgba-cheats
OUT=/projects/sandbox/.scratch/mgba-build
HERE=$(cd "$(dirname "$0")" && pwd)
mkdir -p "$SRC" "$OUT"
cd "$SRC"
[ -d "mgba-$PIN" ] || { curl -sSL -o mgba.tar.gz "https://codeload.github.com/libretro/mgba/tar.gz/$PIN" && tar xzf mgba.tar.gz; }
cd "$OUT"
[ -f libmgba.a ] || {
  cmake -G Ninja -DCMAKE_BUILD_TYPE=Release -DLIBMGBA_ONLY=ON -DM_CORE_GB=OFF -DUSE_ZLIB=OFF -DUSE_PNG=OFF \
    -DUSE_LIBZIP=OFF -DUSE_MINIZIP=OFF -DUSE_SQLITE3=OFF -DUSE_ELF=OFF -DUSE_LZMA=OFF -DUSE_FFMPEG=OFF \
    -DUSE_EPOXY=OFF -DUSE_DISCORD_RPC=OFF -DUSE_LUA=OFF -DUSE_JSON_C=OFF -DUSE_EDITLINE=OFF -DUSE_GDB_STUB=OFF \
    -DUSE_DEBUGGERS=OFF -DBUILD_SHARED=OFF -DBUILD_STATIC=ON -DENABLE_SCRIPTING=OFF "$SRC/mgba-$PIN" > cmake.log
  ninja > ninja.log
}
# The library's own defines (its headers' structures depend on them).
DEFS=$(grep -m1 "DEFINES = " build.ninja | sed 's/ *DEFINES = //')
gcc -O2 -std=gnu11 $DEFS -I"$OUT/include" -I"$SRC/mgba-$PIN/include" -o runner "$HERE/runner.c" libmgba.a -lm -lpthread
echo "$OUT/runner"
