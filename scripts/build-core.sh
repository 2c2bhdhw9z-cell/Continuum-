#!/usr/bin/env bash
# Builds a libretro core to a standalone WebAssembly module for this project.
#
# Not Emscripten. Emscripten's libretro builds are whole-RetroArch bundles that own
# the canvas, the audio graph and the main loop — exactly the three things this
# architecture keeps in Rust. Instead the core is compiled against wasi-libc as a
# *reactor* module that exports the libretro C API, with core-shim/ providing the
# callback trampolines the frontend cannot otherwise supply.
#
# The result imports only:
#   host.*                    six callbacks (see core-shim/libretro_wasm_shim.c)
#   wasi_snapshot_preview1.*  a dozen file syscalls, stubbed by the JS runtime
#
# Usage:
#   scripts/build-core.sh fceumm          # NES  (~2 MB)
#   scripts/build-core.sh mgba            # GBA
#   CORE_SRC=/path/to/core scripts/build-core.sh <name>
set -euo pipefail

CORE="${1:-fceumm}"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TOOLS="$ROOT/.tools"
WORK="$ROOT/.work"
OUT_DIR="$ROOT/web/cores"

WASI_SDK_VERSION="25.0"
WASI_SDK="$TOOLS/wasi-sdk-${WASI_SDK_VERSION}-x86_64-linux"

# --- core definitions -------------------------------------------------------
# Each core needs its repository, its source list, and its compile flags. Kept in
# one place so adding a core is a data change, not a script rewrite.
case "$CORE" in
  fceumm)
    REPO="https://github.com/libretro/libretro-fceumm.git"
    SRC_SUBDIR="src"
    # RGB565 halves upload bandwidth versus XRGB8888 and the renderer converts it
    # on the CPU side anyway.
    DEFINES="-D__LIBRETRO__ -DPATH_MAX=1024 -DFCEU_VERSION_NUMERIC=9900 -DFRONTEND_SUPPORTS_RGB565"
    INCLUDES="-Isrc/drivers/libretro -Isrc/drivers/libretro/libretro-common/include -Isrc -Isrc/input -Isrc/boards"
    LIST_MAKEFILE="Makefile.common"
    ;;
  mgba)
    REPO="https://github.com/libretro/mgba.git"
    SRC_SUBDIR="src"
    DEFINES="-D__LIBRETRO__ -DDISABLE_THREADING -DMINIMAL_CORE=2 -DM_CORE_GBA=1 -DM_CORE_GB=1"
    INCLUDES="-Iinclude -Isrc -Ilibretro/libretro-common/include"
    LIST_MAKEFILE="Makefile.common"
    ;;
  *)
    echo "error: unknown core '$CORE'. Add a case block to $(basename "$0")." >&2
    exit 1
    ;;
esac

mkdir -p "$TOOLS" "$WORK" "$OUT_DIR"

# --- toolchain --------------------------------------------------------------
if [[ ! -d "$WASI_SDK" ]]; then
  echo "==> fetching wasi-sdk ${WASI_SDK_VERSION}"
  curl -fsSL \
    "https://github.com/WebAssembly/wasi-sdk/releases/download/wasi-sdk-${WASI_SDK_VERSION%%.*}/wasi-sdk-${WASI_SDK_VERSION}-x86_64-linux.tar.gz" \
    -o "$TOOLS/wasi-sdk.tar.gz"
  tar xzf "$TOOLS/wasi-sdk.tar.gz" -C "$TOOLS"
  rm -f "$TOOLS/wasi-sdk.tar.gz"
fi
CC="$WASI_SDK/bin/clang"
[[ -x "$CC" ]] || { echo "error: wasi-sdk clang missing at $CC" >&2; exit 1; }

# --- sources ----------------------------------------------------------------
CORE_REPO_DIR="$WORK/$CORE"
if [[ -n "${CORE_SRC:-}" ]]; then
  CORE_REPO_DIR="$CORE_SRC"
elif [[ ! -d "$CORE_REPO_DIR" ]]; then
  echo "==> cloning $CORE"
  git clone -q --depth 1 "$REPO" "$CORE_REPO_DIR"
fi
cd "$CORE_REPO_DIR"

# Ask the core's own makefile for its source list rather than guessing: the list
# includes hundreds of mapper files and changes between releases.
cat > "$WORK/list.mk" <<EOF
CORE_DIR := $SRC_SUBDIR
include $LIST_MAKEFILE
print:
	@echo \$(SOURCES_C)
EOF
SOURCES=$(make -f "$WORK/list.mk" print 2>/dev/null)
[[ -n "$SOURCES" ]] || { echo "error: could not extract SOURCES_C from $LIST_MAKEFILE" >&2; exit 1; }

OBJ_DIR="$WORK/obj-$CORE"
rm -rf "$OBJ_DIR"
mkdir -p "$OBJ_DIR"

# `-Wno-everything`: these are large third-party C codebases with warnings we are
# not going to fix, and the noise hides our own build problems.
CFLAGS="--target=wasm32-wasi -O2 -DNDEBUG -fno-strict-aliasing -Wno-everything $DEFINES $INCLUDES"

echo "==> compiling $(echo "$SOURCES" | wc -w) core sources"
printf '%s\n' $SOURCES | xargs -P "$(nproc)" -I{} sh -c \
  'o=$(echo "{}" | tr "/" "_" | sed "s/\.c$/.o/"); '"$CC $CFLAGS"' -c "{}" -o '"$OBJ_DIR"'/$o' \
  || { echo "error: core compilation failed" >&2; exit 1; }

echo "==> compiling shim"
SHIM_INCLUDES=$(printf '%s' "$INCLUDES")
# shellcheck disable=SC2086
$CC $CFLAGS $SHIM_INCLUDES -c "$ROOT/core-shim/libretro_wasm_shim.c" -o "$OBJ_DIR/zz_shim.o"

# --- link -------------------------------------------------------------------
# Reactor model: the module has no `main`; the host calls `_initialize` once and
# then drives the libretro entry points.
EXPORTS=""
for sym in \
  retro_api_version retro_init retro_deinit retro_get_region \
  retro_get_system_info retro_get_system_av_info \
  retro_set_controller_port_device retro_reset retro_run \
  retro_serialize_size retro_serialize retro_unserialize \
  retro_cheat_reset retro_cheat_set retro_unload_game \
  retro_get_memory_data retro_get_memory_size \
  malloc free
do
  EXPORTS="$EXPORTS -Wl,--export=$sym"
done

echo "==> linking"
# shellcheck disable=SC2086
$CC --target=wasm32-wasi -mexec-model=reactor -O2 \
  -o "$WORK/$CORE.wasm" "$OBJ_DIR"/*.o $EXPORTS

# --- optional size pass -----------------------------------------------------
if command -v wasm-opt >/dev/null 2>&1; then
  echo "==> wasm-opt -O3"
  wasm-opt -O3 "$WORK/$CORE.wasm" -o "$WORK/$CORE.opt.wasm" && mv "$WORK/$CORE.opt.wasm" "$WORK/$CORE.wasm"
fi

cp "$WORK/$CORE.wasm" "$OUT_DIR/$CORE.wasm"
SIZE=$(du -h "$OUT_DIR/$CORE.wasm" | cut -f1)
echo "==> done: web/cores/$CORE.wasm ($SIZE)"
echo
echo "Next: point the '$CORE' entry in web/cores/manifest.json at ./cores/$CORE.wasm,"
echo "set \"kind\": \"libretro\", and drop \"placeholder\"."
