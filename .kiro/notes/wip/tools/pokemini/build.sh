#!/usr/bin/env bash
# Builds PokeMini (the Pokémon Mini emulator cMini replaces, GPL v3) headless, for comparing
# cMini with it: `cpuharness` (its MINX processor on its own, for cores/continuum-mini/tests/
# cpu_pokemini.rs) and `pmharness` (the whole console, frame by frame or instruction by
# instruction). Read for behaviour only: nothing of PokeMini goes into cMini or the repository;
# its source and the programs stay in /projects/sandbox/.scratch/pokemini.
#   POKEMINI_CPU_HARNESS=/projects/sandbox/.scratch/pokemini/cpuharness \
#     cargo test --release --manifest-path cores/continuum-mini/Cargo.toml --test cpu_pokemini
set -euo pipefail
PIN=f8eb158f7082d80739075baa3653afc14fdf3d70
OUT=/projects/sandbox/.scratch/pokemini
HERE=$(cd "$(dirname "$0")" && pwd)
mkdir -p "$OUT"
cd "$OUT"
[ -d "PokeMini-$PIN" ] || { curl -sSL -o pokemini.tar.gz "https://codeload.github.com/DS-Homebrew/PokeMini/tar.gz/$PIN" && tar xzf pokemini.tar.gz; }
S="$OUT/PokeMini-$PIN/source"
F="$OUT/PokeMini-$PIN/freebios"
gcc -O2 -w -DNO_ZIP -I"$S" -I"$F" -o cpuharness "$HERE/cpuharness.c" "$S/MinxCPU.c" "$S/MinxCPU_CE.c" \
  "$S/MinxCPU_CF.c" "$S/MinxCPU_SP.c" "$S/MinxCPU_XX.c" -Wl,--unresolved-symbols=ignore-all
gcc -O2 -w -DNO_ZIP -I"$S" -I"$F" -o pmharness "$HERE/pmharness.c" "$HERE/stubs.c" "$S/PokeMini.c" \
  "$S/Hardware.c" "$S/MinxCPU.c" "$S/MinxCPU_CE.c" "$S/MinxCPU_CF.c" "$S/MinxCPU_SP.c" "$S/MinxCPU_XX.c" \
  "$S/MinxIO.c" "$S/MinxIRQ.c" "$S/MinxLCD.c" "$S/MinxPRC.c" "$S/MinxColorPRC.c" "$S/MinxTimers.c" \
  "$S/MinxAudio.c" "$S/Multicart.c" "$S/PMCommon.c" "$S/CommandLine.c" "$F/freebios.c" -lm
echo "$OUT/cpuharness $OUT/pmharness"
