#!/usr/bin/env bash
# Builds a libretro core to a standalone WebAssembly module for this project.
#
# Not Emscripten. Emscripten's libretro builds are whole-RetroArch bundles that own the
# canvas, the audio graph and the main loop — exactly the three things this architecture
# keeps in Rust. Instead the core is compiled against wasi-libc as a *reactor* module
# that exports the libretro C API, with core-shim/ providing the callback trampolines a
# frontend cannot otherwise supply.
#
# The result imports only:
#   host.*                    five callbacks (see core-shim/libretro_wasm_shim.c)
#   wasi_snapshot_preview1.*  file/clock syscalls, stubbed by the JS runtime
#
# Usage:
#   scripts/build-core.sh fceumm     # NES  (~2 MB)
#   scripts/build-core.sh mgba       # GBA + GB/GBC (~1.9 MB)
#   scripts/build-core.sh all
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TOOLS="$ROOT/.tools"
WORK="$ROOT/.work"
OUT_DIR="$ROOT/web/cores"

WASI_SDK_VERSION="25.0"
WASI_SDK="$TOOLS/wasi-sdk-${WASI_SDK_VERSION}-x86_64-linux"

# wasi-libc ships opt-in emulation for the POSIX corners wasm lacks. Cores reach for all
# of these somewhere, usually in code that DISABLE_THREADING should have removed.
WASI_EMULATED_DEFINES="-D_WASI_EMULATED_SIGNAL -D_WASI_EMULATED_MMAN -D_WASI_EMULATED_PROCESS_CLOCKS -D_WASI_EMULATED_GETPID"
WASI_EMULATED_LIBS="-lwasi-emulated-signal -lwasi-emulated-mman -lwasi-emulated-process-clocks -lwasi-emulated-getpid"

# The libretro entry points to export. `malloc`/`free` let the host place content in the
# core's memory; the shim adds its own exports via __attribute__((export_name)).
RETRO_EXPORTS=(
  retro_api_version retro_init retro_deinit retro_get_region
  retro_get_system_info retro_get_system_av_info
  retro_set_controller_port_device retro_reset retro_run
  retro_serialize_size retro_serialize retro_unserialize
  retro_cheat_reset retro_cheat_set retro_unload_game
  retro_get_memory_data retro_get_memory_size
  malloc free
)

# ---------------------------------------------------------------- core definitions
#
# Two build strategies, because libretro cores do not agree on a build system:
#
#   sources  the core ships a libretro makefile listing SOURCES_C; ask it for the list
#            and compile the files directly (fceumm and most classic cores).
#   cmake    the core is CMake-based; configure it with wasi-sdk's toolchain file and
#            build its static library target, which also generates files the build
#            needs (mgba's version.c, for instance).
core_config() {
  case "$1" in
    fceumm)
      REPO="https://github.com/libretro/libretro-fceumm.git"
      STRATEGY="sources"
      SRC_SUBDIR="src"
      LIST_MAKEFILE="Makefile.common"
      # RGB565 halves upload bandwidth versus XRGB8888; the renderer converts either.
      DEFINES="-D__LIBRETRO__ -DPATH_MAX=1024 -DFCEU_VERSION_NUMERIC=9900 -DFRONTEND_SUPPORTS_RGB565"
      INCLUDES="-Isrc/drivers/libretro -Isrc/drivers/libretro/libretro-common/include -Isrc -Isrc/input -Isrc/boards"
      SHIM_INCLUDES="-Isrc/drivers/libretro/libretro-common/include"
      ;;
    mgba)
      REPO="https://github.com/libretro/mgba.git"
      STRATEGY="cmake"
      CMAKE_TARGET="mgba_libretro"
      CMAKE_ARCHIVE="mgba_libretro.a"
      # Mirrors the flags CMakeLists applies to the libretro target, plus the wasi
      # emulation defines. Everything optional is off: no zlib, png, sqlite, ffmpeg,
      # zip, lzma, ELF loading, scripting or debuggers.
      CMAKE_ARGS=(
        -DBUILD_LIBRETRO=ON -DLIBRETRO_STATIC=ON -DSKIP_LIBRARY=ON
        -DBUILD_QT=OFF -DBUILD_SDL=OFF -DBUILD_PYTHON=OFF
        -DBUILD_TEST=OFF -DBUILD_SUITE=OFF
        -DUSE_ZLIB=OFF -DUSE_PNG=OFF -DUSE_SQLITE3=OFF -DUSE_FFMPEG=OFF
        -DUSE_LIBZIP=OFF -DUSE_MINIZIP=OFF -DUSE_LZMA=OFF -DUSE_ELF=OFF
        -DUSE_EPOXY=OFF -DUSE_DISCORD_RPC=OFF -DUSE_EDITLINE=OFF -DUSE_LUA=OFF
        -DENABLE_SCRIPTING=OFF
      )
      SHIM_INCLUDES="-Isrc/platform/libretro -Iinclude -Isrc"
      ;;
    *)
      echo "error: unknown core '$1'. Add a case block to $(basename "$0")." >&2
      return 1
      ;;
  esac
}

ensure_toolchain() {
  mkdir -p "$TOOLS" "$WORK" "$OUT_DIR"
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
}

clone_core() {
  local core="$1"
  CORE_REPO_DIR="${CORE_SRC:-$WORK/$core}"
  if [[ ! -d "$CORE_REPO_DIR" ]]; then
    echo "==> cloning $core"
    git clone -q --depth 1 "$REPO" "$CORE_REPO_DIR"
  fi
}

# --- strategy: compile the makefile's source list ---------------------------------
build_sources_strategy() {
  local core="$1"
  cd "$CORE_REPO_DIR"

  # Ask the core's own makefile for its sources: the list runs to hundreds of mapper
  # files and changes between releases.
  cat > "$WORK/list-$core.mk" <<EOF
CORE_DIR := $SRC_SUBDIR
include $LIST_MAKEFILE
print:
	@echo \$(SOURCES_C)
EOF
  local sources
  sources=$(make -f "$WORK/list-$core.mk" print 2>/dev/null)
  [[ -n "$sources" ]] || { echo "error: could not extract SOURCES_C from $LIST_MAKEFILE" >&2; exit 1; }

  local obj_dir="$WORK/obj-$core"
  rm -rf "$obj_dir"
  mkdir -p "$obj_dir"

  # -Wno-everything: large third-party C we are not going to fix, and the noise hides
  # our own problems.
  local cflags="--target=wasm32-wasi -O2 -DNDEBUG -fno-strict-aliasing -Wno-everything $WASI_EMULATED_DEFINES $DEFINES $INCLUDES"

  echo "==> compiling $(echo "$sources" | wc -w) core sources"
  printf '%s\n' $sources | xargs -P "$(nproc)" -I{} sh -c \
    'o=$(echo "{}" | tr "/" "_" | sed "s/\.c$/.o/"); '"$CC $cflags"' -c "{}" -o '"$obj_dir"'/$o' \
    || { echo "error: core compilation failed" >&2; exit 1; }

  CORE_OBJECTS=("$obj_dir"/*.o)
}

# --- strategy: cmake + wasi toolchain ---------------------------------------------
build_cmake_strategy() {
  local core="$1"
  cd "$CORE_REPO_DIR"
  command -v cmake >/dev/null 2>&1 || { echo "error: cmake is required for $core" >&2; exit 1; }

  local build_dir="build-wasi"
  echo "==> configuring $core with cmake"
  rm -rf "$build_dir"
  cmake -B "$build_dir" -S . \
    -DCMAKE_TOOLCHAIN_FILE="$WASI_SDK/share/cmake/wasi-sdk-p1.cmake" \
    -DCMAKE_BUILD_TYPE=Release \
    -DCMAKE_C_FLAGS="$WASI_EMULATED_DEFINES -O2 -Wno-everything" \
    "${CMAKE_ARGS[@]}" >/dev/null

  echo "==> building $CMAKE_TARGET"
  cmake --build "$build_dir" --target "$CMAKE_TARGET" -j"$(nproc)" 2>&1 |
    grep -E "error|Error" && { echo "error: cmake build failed" >&2; exit 1; }

  [[ -f "$build_dir/$CMAKE_ARCHIVE" ]] || {
    echo "error: expected $build_dir/$CMAKE_ARCHIVE" >&2
    exit 1
  }
  # The archive alone is enough: the shim references the libretro entry points, which
  # pulls in the object defining them and transitively the rest of the core.
  CORE_OBJECTS=("$build_dir/$CMAKE_ARCHIVE")
}

build_core() {
  local core="$1"
  core_config "$core"
  clone_core "$core"

  case "$STRATEGY" in
    sources) build_sources_strategy "$core" ;;
    cmake) build_cmake_strategy "$core" ;;
    *) echo "error: unknown strategy '$STRATEGY'" >&2; exit 1 ;;
  esac

  echo "==> compiling shim"
  local shim_obj="$WORK/shim-$core.o"
  # shellcheck disable=SC2086
  $CC --target=wasm32-wasi -O2 -Wno-everything $WASI_EMULATED_DEFINES $SHIM_INCLUDES \
    -c "$ROOT/core-shim/libretro_wasm_shim.c" -o "$shim_obj"

  local exports=()
  for sym in "${RETRO_EXPORTS[@]}"; do exports+=("-Wl,--export=$sym"); done

  echo "==> linking"
  # Reactor model: no `main`. The host calls `_initialize` once, then drives the
  # libretro entry points.
  # shellcheck disable=SC2086
  $CC --target=wasm32-wasi -mexec-model=reactor -O2 \
    -o "$WORK/$core.wasm" "$shim_obj" "${CORE_OBJECTS[@]}" \
    $WASI_EMULATED_LIBS "${exports[@]}"

  if command -v wasm-opt >/dev/null 2>&1; then
    echo "==> wasm-opt -O3"
    wasm-opt -O3 "$WORK/$core.wasm" -o "$WORK/$core.opt.wasm" && mv "$WORK/$core.opt.wasm" "$WORK/$core.wasm"
  fi

  cp "$WORK/$core.wasm" "$OUT_DIR/$core.wasm"
  echo "==> done: web/cores/$core.wasm ($(du -h "$OUT_DIR/$core.wasm" | cut -f1))"
  echo
}

ensure_toolchain

if [[ "${1:-fceumm}" == "all" ]]; then
  for core in fceumm mgba; do build_core "$core"; done
else
  build_core "${1:-fceumm}"
fi

echo "Set the core's manifest entry to \"kind\": \"libretro\" with the matching"
echo "module path and sizeBytes (web/cores/manifest.json)."
