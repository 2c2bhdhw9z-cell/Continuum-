#!/usr/bin/env bash
# Builds the mGBA cheat harness that cores/continuum-gb/tests/gba_cheats_mgba.rs compares cBoy
# against (.kiro/notes/docs/CBOY_CHEATS.md). It compiles mGBA's own cheat code, at the commit the
# app ships (scripts/build-core.sh, mgba IOS_PIN), around a fake memory. Scratch only: mGBA's
# source and the binary stay in /projects/sandbox/.scratch/mgba-cheats, never in the repository.
# Then: MGBA_CHEAT_HARNESS=/projects/sandbox/.scratch/mgba-cheats/harness \
#   cargo test --release --manifest-path cores/continuum-gb/Cargo.toml --test gba_cheats_mgba
set -euo pipefail
PIN=7a12d6d4b9acb14c0ae62c9166b6a2f3d08007f6
OUT=/projects/sandbox/.scratch/mgba-cheats
HERE=$(cd "$(dirname "$0")" && pwd)
mkdir -p "$OUT/gen/mgba"
cd "$OUT"
[ -d "mgba-$PIN" ] || { curl -sSL -o mgba.tar.gz "https://codeload.github.com/libretro/mgba/tar.gz/$PIN" && tar xzf mgba.tar.gz; }
M="$OUT/mgba-$PIN"
printf '#ifndef MGBA_FLAGS_H\n#define MGBA_FLAGS_H\n#define M_CORE_GBA\n#define M_CORE_GB\n#endif\n' > gen/mgba/flags.h
gcc -O2 -w -std=gnu11 -DM_CORE_GBA -DM_CORE_GB -DMINIMAL_CORE=2 -Igen -I"$M/include" -I"$M/src" -o harness \
  "$HERE/harness.c" "$M/src/core/cheats.c" "$M/src/gba/cheats.c" "$M/src/gba/cheats/gameshark.c" \
  "$M/src/gba/cheats/parv3.c" "$M/src/gba/cheats/codebreaker.c" "$M/src/util/table.c" \
  "$M/src/util/string.c" "$M/src/util/hash.c" "$M/src/util/vector.c" -Wl,--unresolved-symbols=ignore-all
echo "$OUT/harness"
