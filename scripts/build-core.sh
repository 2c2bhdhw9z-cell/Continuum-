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
#   scripts/build-core.sh fceumm           # NES (~1.7 MB)
#   scripts/build-core.sh mgba             # GBA + GB/GBC (~1.5 MB)
#   scripts/build-core.sh genesis_plus_gx  # Mega Drive + Master System (~2.8 MB)
#   scripts/build-core.sh snes9x           # SNES, C++ (~2.5 MB)
#   scripts/build-core.sh all
#
# C and C++ cores are both supported: each source is compiled by the driver for its own
# language, and a core with any C++ is linked by clang++ so libc++ comes in. See
# CXX_BASE_FLAGS below for the one real constraint (no exceptions, no RTTI).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TOOLS="$ROOT/.tools"
WORK="$ROOT/.work"
OUT_DIR="$ROOT/web/cores"

# ---------------------------------------------------------------- iOS PS1 core
#
# PCSX ReARMed for iOS is a *completely separate* path from the wasi-sdk web strategies
# above. It does not use wasi-sdk, the RETRO_EXPORTS list, the wasm import-restriction
# check, core-shim, or web/cores/. It compiles the real libretro core from source with
# Apple's clang for aarch64-apple-ios and produces a native .dylib the iOS app dlopens.
#
# It is invoked as a distinct core name:
#
#   scripts/build-core.sh pcsx_rearmed        # PS1 core -> native/ios/build/lib/*.dylib
#
# and is dispatched before the wasi-sdk machinery ever runs, so nothing here perturbs the
# web build. This path only runs on macOS (Darwin); it errors clearly anywhere else, the
# same contract as native/ios/build-engine.sh.
#
# Ground truth (PCSX ReARMed's master Makefile.libretro, platform=ios-arm64): ARCH=arm64,
# BUILTIN_GPU=neon, DYNAREC=0, CC='cc -arch arm64 -isysroot $(IOSSDK)', GNU_LINKER=0, links
# -shared, TARGET=pcsx_rearmed_libretro_ios.dylib. The repo has git submodules (lightrec,
# libchdr) that must be initialised recursively.
#
# DYNAREC=0 is not our choice: the Makefile force-disables the lightrec/dynarec JIT for iOS
# arm64. That makes this first build INTERPRETER-only: no JIT dependency (so no dependence
# on the JIT entitlement being honoured), correct-but-slower. That tradeoff is deliberate
# for the first green build; it de-risks the loader/ROM/input/audio software frame path
# before any recompiler is involved. A faster JIT-backed build is a later step.
PS1_CORE_NAME="pcsx_rearmed"
PS1_CORE_REPO="https://github.com/libretro/pcsx_rearmed"
# THE canonical filename. This exact string must match, byte for byte:
#   - build-engine.sh copy destination in native/ios/build/lib/
#   - project.yml   framework: build/lib/pcsx_rearmed_libretro_ios.dylib
#   - package-ipa.sh fallback source/dest
#   - .github/workflows/ios.yml verify grep
# A single divergence turns macOS CI red, so it is defined once, here.
PS1_DYLIB_NAME="pcsx_rearmed_libretro_ios.dylib"
# Staged next to libcontinuum_switch.dylib so project.yml's relative `build/lib/...` path
# resolves and package-ipa.sh's fallback finds it in $OUT/lib.
PS1_OUT_DIR="$ROOT/native/ios/build/lib"

build_ps1_ios_core() {
  if [[ "$(uname -s)" != "Darwin" ]]; then
    cat >&2 <<'EOF'
error: the pcsx_rearmed (PS1) core builds for iOS only, which needs a macOS host.

  It cross-compiles the real libretro core with Apple's clang and the iphoneos SDK:
      make -f Makefile.libretro platform=ios-arm64 IOSSDK=$(xcrun --sdk iphoneos --show-sdk-path)
  There is no wasi-sdk fallback for this path. Build it on the macOS runner via the
  ios workflow (native/ios/build-engine.sh calls this), which is what it is for.
EOF
    exit 1
  fi

  command -v xcrun >/dev/null 2>&1 || { echo "error: xcrun not found (need Xcode command line tools)" >&2; exit 1; }
  command -v make >/dev/null 2>&1 || { echo "error: make not found" >&2; exit 1; }
  command -v git >/dev/null 2>&1 || { echo "error: git not found" >&2; exit 1; }

  local iossdk
  iossdk="$(xcrun --sdk iphoneos --show-sdk-path)"
  [[ -n "$iossdk" && -d "$iossdk" ]] || { echo "error: could not resolve the iphoneos SDK path via xcrun" >&2; exit 1; }

  mkdir -p "$WORK" "$PS1_OUT_DIR"

  local src_dir="$WORK/$PS1_CORE_NAME"
  if [[ ! -d "$src_dir/.git" ]]; then
    echo "==> cloning $PS1_CORE_NAME"
    git clone "$PS1_CORE_REPO" "$src_dir"
  fi

  echo "==> initialising submodules (lightrec, libchdr)"
  ( cd "$src_dir" && git submodule update --init --recursive )

  echo "==> building $PS1_CORE_NAME for ios-arm64 (DYNAREC=0, interpreter-only)"
  # platform=ios-arm64 selects ARCH=arm64, BUILTIN_GPU=neon, DYNAREC=0, the arm64 clang
  # CC and -shared link, and emits the .dylib TARGET. IOSSDK must be passed explicitly.
  ( cd "$src_dir" && make -f Makefile.libretro platform=ios-arm64 IOSSDK="$iossdk" -j"$(sysctl -n hw.ncpu 2>/dev/null || echo 4)" )

  local built="$src_dir/$PS1_DYLIB_NAME"
  [[ -f "$built" ]] || {
    echo "error: expected $built after the make; the PS1 core did not build" >&2
    exit 1
  }

  # dlopen from Frameworks/ resolves against @rpath (LD_RUNPATH_SEARCH_PATHS in project.yml
  # points at @executable_path/Frameworks). If the Makefile did not set an @rpath install
  # name, force one so the on-device load resolves. Harmless to reassert if already correct.
  local install_name
  install_name="$(otool -D "$built" 2>/dev/null | tail -n +2 | head -1 || true)"
  if [[ "$install_name" != "@rpath/$PS1_DYLIB_NAME" ]]; then
    echo "==> setting install_name to @rpath/$PS1_DYLIB_NAME (was: ${install_name:-none})"
    install_name_tool -id "@rpath/$PS1_DYLIB_NAME" "$built"
  fi

  cp "$built" "$PS1_OUT_DIR/$PS1_DYLIB_NAME"
  echo "==> done: native/ios/build/lib/$PS1_DYLIB_NAME ($(du -h "$PS1_OUT_DIR/$PS1_DYLIB_NAME" | cut -f1))"
}

WASI_SDK_VERSION="25.0"
WASI_SDK="$TOOLS/wasi-sdk-${WASI_SDK_VERSION}-x86_64-linux"

# C++ cores compile against wasi-sdk's libc++, with two hard constraints.
#
# No exceptions, no RTTI. wasi-sdk 25 ships libc++ and libc++abi built without the
# unwinder: `__cxa_throw`, `__cxa_allocate_exception`, `__cxa_begin_catch` and
# `_Unwind_CallPersonality` are all absent from the sysroot, so a translation unit
# containing a `throw` or a `try` block compiles and then fails to link. `-fwasm-exceptions`
# does not help — it needs the same missing runtime. This is a property of the SDK, not of
# wasm: the Exception Handling proposal is available, but nobody has built this libc++
# against it.
#
# In practice that costs nothing here. Emulator cores are written to run on consoles
# where exceptions are equally unavailable, and snes9x's own libretro makefile already
# passes -fno-rtti -fno-exceptions. A core that genuinely needed them would need a
# libc++ rebuilt from source, which is a bigger decision than adding a core.
CXX_BASE_FLAGS="-fno-exceptions -fno-rtti"

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
#   sources  the core ships a libretro makefile listing its sources; ask it for the
#            lists and compile the files directly. Both SOURCES_C and SOURCES_CXX are
#            read, so a mixed C/C++ core (snes9x) needs no special case — each file is
#            compiled by the driver for its own language.
#   cmake    the core is CMake-based; configure it with wasi-sdk's toolchain file and
#            build its static library target, which also generates files the build
#            needs (mgba's version.c, for instance).
core_config() {
  # Cleared per core: `build-core.sh all` runs these in one shell, and a stale flag from
  # the previous core is a genuinely confusing failure.
  EXTRA_CFLAGS=""
  EXTRA_CXXFLAGS=""
  EXTRA_LDFLAGS=""
  EXTRA_LIBS=""
  SOURCE_EXCLUDE=""
  FORCED_HEADER_CONTENT=""
  INCLUDES_FROM_MAKEFILE=0
  LIST_PREAMBLE=""
  # Language standard for C++ sources. Empty means the driver's default (gnu++17).
  CXX_STD=""
  case "$1" in
    fceumm)
      REPO="https://github.com/libretro/libretro-fceumm.git"
      STRATEGY="sources"
      SRC_SUBDIR="src"
      LIST_MAKEFILE="Makefile.common"
      LIST_PREAMBLE="CORE_DIR := src"
      # RGB565 halves upload bandwidth versus XRGB8888; the renderer converts either.
      DEFINES="-D__LIBRETRO__ -DPATH_MAX=1024 -DFCEU_VERSION_NUMERIC=9900 -DFRONTEND_SUPPORTS_RGB565"
      INCLUDES="-Isrc/drivers/libretro -Isrc/drivers/libretro/libretro-common/include -Isrc -Isrc/input -Isrc/boards"
      SHIM_INCLUDES="-Isrc/drivers/libretro/libretro-common/include"
      ;;
    genesis_plus_gx)
      REPO="https://github.com/libretro/Genesis-Plus-GX.git"
      STRATEGY="sources"
      # Sources and includes live in Makefile.libretro, which needs `platform` preset.
      LIST_MAKEFILE="Makefile.libretro"
      LIST_PREAMBLE="platform := unix"
      # LSB_FIRST/BYTE_ORDER: wasm is little-endian. USE_16BPP_RENDERING selects RGB565,
      # halving upload bandwidth versus the 32bpp default. The rest mirror the core's
      # own unix build.
      # HAVE_NO_LANGEXTRA drops the core-options tables translated into ~28 languages.
      # This frontend never surfaces libretro core options, so they were ~1.7 MB of
      # initialised data in a module the browser downloads on demand.
      # USE_16BPP_RENDERING makes the core *render* 16bpp; FRONTEND_SUPPORTS_RGB565 is
      # the separate flag that makes it negotiate RGB565 with the frontend. Without the
      # second one the core never calls SET_PIXEL_FORMAT and the host is left assuming
      # libretro's 0RGB1555 default, which is not what the framebuffer contains.
      DEFINES="-D__LIBRETRO__ -DLSB_FIRST -DBYTE_ORDER=LITTLE_ENDIAN -DUSE_16BPP_RENDERING -DFRONTEND_SUPPORTS_RGB565 -DHAVE_ZLIB -DUSE_LIBTREMOR -DUSE_LIBCHDR -DUSE_PER_SOUND_CHANNELS_CONFIG -D_7ZIP_ST -DZSTD_DISABLE_ASM -DHAVE_NO_LANGEXTRA"
      # GIT_VERSION must expand to a string literal, and quotes do not survive the
      # shell → make → xargs → clang fan-out. Force-include it as a header instead.
      #
      # INLINE is the same story and a more interesting bug. libretro-common's
      # retro_inline.h is reached before core/macros.h, and because clang reports
      # C17 it picks the bare `inline` branch. A C99 `inline` definition with no
      # `extern` is only an *inline definition*: no external symbol is emitted, so
      # every call the optimiser declined to inline became an undefined symbol at
      # link (gfx_render, chan_calc, word_ram_switch...). The core's own macros.h
      # documents this override — "set to your compiler's static inline keyword" —
      # and both headers guard on #ifndef INLINE, so defining it first wins.
      FORCED_HEADER_CONTENT='#define GIT_VERSION "wasi"
#define INLINE static inline'
      # The Musashi 68000 core uses setjmp/longjmp for address-error traps, and its
      # <setjmp.h> include is unconditional. On wasm that needs the Exception Handling
      # proposal, which every browser this project targets already supports (the WebGPU
      # floor is far newer than EH). Enabling it keeps address-error emulation intact
      # rather than patching accuracy out of the core.
      # A browser has no CD-ROM device, and libretro-common's cdrom backend only
      # compiles when HAVE_CDROM adds fields to its file struct. Excluding it is
      # correct rather than expedient: physical-disc access cannot exist here.
      SOURCE_EXCLUDE='vfs_implementation_cdrom|libretro-common/cdrom'
      EXTRA_CFLAGS="-mllvm -wasm-enable-sjlj"
      # The lowering rewrites setjmp/longjmp into calls to __wasm_setjmp,
      # __wasm_setjmp_test and __wasm_longjmp, which wasi-sdk ships in libsetjmp.a.
      # Without it those arrive as `env.*` imports and the host would have to fake
      # a control-flow primitive in JS.
      EXTRA_LIBS="-lsetjmp"
      # Taken from the core's own INCFLAGS. Pinned rather than scraped from the
      # makefile: the scrape silently produced nothing, and every file then failed
      # on a missing header, which is a slow way to learn that.
      INCLUDES="-I./libretro/deps/libchdr/include -I./libretro/deps/lzma-19.00/include -I./libretro/deps/zlib-1.2.11 -I./libretro/deps/zstd/lib -I./core -I./core/z80 -I./core/m68k -I./core/ntsc -I./core/sound -I./core/sound/minimp3 -I./core/input_hw -I./core/cd_hw -I./core/cart_hw -I./core/cart_hw/svp -I./libretro -I./libretro/libretro-common/include"
      SHIM_INCLUDES="-Ilibretro -Ilibretro/libretro-common/include"
      ;;
    snes9x)
      # Mainline snes9x, not snes9x2010. The 2010 fork was converted to C, so it would
      # not exercise the C++ path at all — and a build strategy no core uses is a
      # strategy nobody knows is broken. Mainline is also the more accurate emulator.
      REPO="https://github.com/libretro/snes9x.git"
      STRATEGY="sources"
      # Both SOURCES_C (15 files) and SOURCES_CXX (37 files) come from here.
      LIST_MAKEFILE="libretro/Makefile.common"
      LIST_PREAMBLE="CORE_DIR := ."
      # Mirrors the core's own libretro build. STATIC_LINKING keeps it from expecting
      # to be a shared object; HAVE_STRINGS_H is needed because wasi-libc has it.
      DEFINES="-D__LIBRETRO__ -DALLOW_CPU_OVERCLOCK -DHAVE_STRINGS_H -DSTATIC_LINKING"
      INCLUDES="-I. -Ilibretro -Ilibretro/libretro-common/include -Iapu -Iapu/bapu"
      SHIM_INCLUDES="-Ilibretro -Ilibretro/libretro-common/include"
      # The blargg APU code uses C++98 dynamic exception specifications (`throw()` on
      # operator new). Those are deprecated in C++17 and removed in C++20, so pin the
      # standard the core's own makefile uses rather than inheriting the driver default.
      CXX_STD="c++14"
      # GIT_VERSION again: a string literal that does not survive the shell → make →
      # xargs → clang fan-out. Force-include it instead.
      FORCED_HEADER_CONTENT='#define GIT_VERSION ""'
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
  CXX="$WASI_SDK/bin/clang++"
  [[ -x "$CC" ]] || { echo "error: wasi-sdk clang missing at $CC" >&2; exit 1; }
  [[ -x "$CXX" ]] || { echo "error: wasi-sdk clang++ missing at $CXX" >&2; exit 1; }
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
$LIST_PREAMBLE
include $LIST_MAKEFILE
print:
	@echo \$(SOURCES_C)
print-cxx:
	@echo \$(SOURCES_CXX)
print-includes:
	@echo \$(INCFLAGS)
EOF
  local sources sources_cxx
  sources=$(make -f "$WORK/list-$core.mk" print 2>/dev/null)
  # A core with no C++ leaves SOURCES_CXX undefined, which is not an error.
  sources_cxx=$(make -f "$WORK/list-$core.mk" print-cxx 2>/dev/null)
  [[ -n "$sources$sources_cxx" ]] || {
    echo "error: could not extract SOURCES_C or SOURCES_CXX from $LIST_MAKEFILE" >&2
    exit 1
  }

  if [[ -n "${SOURCE_EXCLUDE:-}" ]]; then
    local before after
    before=$(( $(echo "$sources" | wc -w) + $(echo "$sources_cxx" | wc -w) ))
    sources=$(printf '%s\n' $sources | grep -Ev "$SOURCE_EXCLUDE" | tr '\n' ' ')
    sources_cxx=$(printf '%s\n' $sources_cxx | grep -Ev "$SOURCE_EXCLUDE" | tr '\n' ' ')
    after=$(( $(echo "$sources" | wc -w) + $(echo "$sources_cxx" | wc -w) ))
    echo "==> excluded $((before - after)) source(s) matching /$SOURCE_EXCLUDE/"
  fi

  local obj_dir="$WORK/obj-$core"
  rm -rf "$obj_dir"
  mkdir -p "$obj_dir"

  # -Wno-everything: large third-party C we are not going to fix, and the noise hides
  # our own problems.
  # Some cores keep a long, changing include list in their makefile; ask for it rather
  # than duplicating a dozen -I paths here and watching them rot.
  if [[ "${INCLUDES_FROM_MAKEFILE:-0}" == "1" ]]; then
    INCLUDES=$(make -f "$WORK/list-$core.mk" print-includes 2>/dev/null)
  fi
  local forced=""
  if [[ -n "${FORCED_HEADER_CONTENT:-}" ]]; then
    printf '%s\n' "$FORCED_HEADER_CONTENT" > "$WORK/forced-$core.h"
    forced="-include $WORK/forced-$core.h"
  fi
  local common="--target=wasm32-wasi -O2 -DNDEBUG -fno-strict-aliasing -Wno-everything $WASI_EMULATED_DEFINES $DEFINES $INCLUDES $forced"
  local cflags="$common ${EXTRA_CFLAGS:-}"
  local cxxflags="$common $CXX_BASE_FLAGS ${CXX_STD:+-std=$CXX_STD} ${EXTRA_CXXFLAGS:-}"

  local n_c n_cxx
  n_c=$(echo "$sources" | wc -w)
  n_cxx=$(echo "$sources_cxx" | wc -w)
  if [[ "$n_cxx" -gt 0 ]]; then
    echo "==> compiling $n_c C + $n_cxx C++ core sources"
  else
    echo "==> compiling $n_c core sources"
  fi

  # Compiled through an exported function rather than by interpolating the flags into an
  # `sh -c` string. That construct silently ran zero commands once the flags grew to
  # include `-mllvm -wasm-enable-sjlj` and a pinned include list — no errors, no objects,
  # and the failure only surfaced at link time. A function takes its arguments as
  # arguments, so nothing depends on quoting surviving three levels of shell.
  export CORE_CC="$CC"
  export CORE_CXX="$CXX"
  export CORE_CFLAGS="$cflags"
  export CORE_CXXFLAGS="$cxxflags"
  export CORE_OBJ_DIR="$obj_dir"
  compile_one() {
    local src="$1"
    local obj driver flags
    # Strip any leading `./` before flattening the path. Genesis-Plus-GX lists its
    # sources as `./core/vdp_ctrl.c`; without this the flattened name is
    # `._core_vdp_ctrl.o`, a dotfile, and every object silently disappears from the
    # link (bash globs do not match leading dots).
    #
    # The source extension is kept in the object name (`cpu.cpp` → `cpu.cpp.o`) so a
    # core carrying both `dsp.c` and `dsp.cpp` cannot have one overwrite the other.
    obj="$(printf '%s' "$src" | sed 's#^\./##' | tr '/' '_').o"
    case "$src" in
      *.cpp | *.cc | *.cxx) driver="$CORE_CXX"; flags="$CORE_CXXFLAGS" ;;
      *) driver="$CORE_CC"; flags="$CORE_CFLAGS" ;;
    esac
    # shellcheck disable=SC2086
    "$driver" $flags -c "$src" -o "$CORE_OBJ_DIR/$obj"
  }
  export -f compile_one

  printf '%s\n' $sources $sources_cxx \
    | xargs -P "$(nproc)" -I{} bash -c 'compile_one "$@"' _ {} \
    || { echo "error: core compilation failed" >&2; exit 1; }

  # Recorded for the link step: a core with C++ objects must be linked by the C++
  # driver so that libc++ and libc++abi come in.
  CORE_HAS_CXX=$([[ "$n_cxx" -gt 0 ]] && echo 1 || echo 0)

  # Collected with find rather than a glob so the link list can never be quietly
  # truncated by shell expansion rules. Sorted for a deterministic link order.
  mapfile -t CORE_OBJECTS < <(find "$obj_dir" -name '*.o' -type f | sort)
  local built=${#CORE_OBJECTS[@]}
  [[ "$built" -gt 0 ]] || { echo "error: no objects were produced" >&2; exit 1; }
  echo "==> compiled $built objects"

  # A source list that produced fewer objects than it had entries means some files
  # failed in a way xargs did not propagate. Fail loudly instead of linking a
  # half-built core and debugging it at runtime.
  local expected=$((n_c + n_cxx))
  if [[ "$built" -ne "$expected" ]]; then
    echo "error: expected $expected objects from the source lists, got $built" >&2
    exit 1
  fi
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
  CORE_HAS_CXX=0
}

build_core() {
  local core="$1"
  CORE_HAS_CXX=0
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
  $CC --target=wasm32-wasi -O2 -Wno-everything $WASI_EMULATED_DEFINES $SHIM_INCLUDES ${EXTRA_CFLAGS:-} \
    -c "$ROOT/core-shim/libretro_wasm_shim.c" -o "$shim_obj"

  local exports=()
  for sym in "${RETRO_EXPORTS[@]}"; do exports+=("-Wl,--export=$sym"); done

  # A C++ core is linked by the C++ driver, which is what pulls in libc++ and
  # libc++abi. Linking C++ objects with plain `clang` produces a wall of undefined
  # std:: symbols, so this is not a stylistic choice.
  local link_driver="$CC"
  local link_lang="C"
  if [[ "${CORE_HAS_CXX:-0}" == "1" ]]; then
    link_driver="$CXX"
    link_lang="C++ (libc++)"
  fi

  echo "==> linking [$link_lang]"
  # Reactor model: no `main`. The host calls `_initialize` once, then drives the
  # libretro entry points.
  # --strip-debug, not --strip-all: DWARF is a few hundred KB the browser would
  # download and never read, but the `name` section stays so a core that traps
  # produces a readable stack in devtools.
  # shellcheck disable=SC2086
  $link_driver --target=wasm32-wasi -mexec-model=reactor -O2 -Wl,--strip-debug \
    ${EXTRA_LDFLAGS:-} \
    -o "$WORK/$core.wasm" "$shim_obj" "${CORE_OBJECTS[@]}" \
    $WASI_EMULATED_LIBS ${EXTRA_LIBS:-} "${exports[@]}"

  # A core that imports anything beyond the five host callbacks and WASI cannot be
  # instantiated by the runtime in web/src/engine/core-runtime.js. Catching it here
  # is the difference between a build error and a blank screen.
  local stray
  stray=$("$WASI_SDK/bin/llvm-objdump" --headers "$WORK/$core.wasm" >/dev/null 2>&1; node -e '
    const fs = require("fs");
    const m = new WebAssembly.Module(fs.readFileSync(process.argv[1]));
    const bad = [...new Set(WebAssembly.Module.imports(m)
      .filter(i => i.module !== "host" && i.module !== "wasi_snapshot_preview1")
      .map(i => i.module + "." + i.name))];
    if (bad.length) console.log(bad.join(" "));
  ' "$WORK/$core.wasm" 2>/dev/null || true)
  if [[ -n "$stray" ]]; then
    echo "error: $core imports symbols the host does not provide: $stray" >&2
    exit 1
  fi

  if command -v wasm-opt >/dev/null 2>&1; then
    echo "==> wasm-opt -O3"
    wasm-opt -O3 "$WORK/$core.wasm" -o "$WORK/$core.opt.wasm" && mv "$WORK/$core.opt.wasm" "$WORK/$core.wasm"
  fi

  cp "$WORK/$core.wasm" "$OUT_DIR/$core.wasm"
  echo "==> done: web/cores/$core.wasm ($(du -h "$OUT_DIR/$core.wasm" | cut -f1))"
  echo
}

# The PS1 core is dispatched before the wasi-sdk web machinery so it never fetches wasi-sdk
# and never touches web/cores/. It is a native iOS .dylib, not a wasm module.
if [[ "${1:-}" == "$PS1_CORE_NAME" ]]; then
  build_ps1_ios_core
  exit 0
fi

ensure_toolchain

if [[ "${1:-fceumm}" == "all" ]]; then
  for core in fceumm mgba genesis_plus_gx snes9x; do build_core "$core"; done
else
  build_core "${1:-fceumm}"
fi

echo "Set the core's manifest entry to \"kind\": \"libretro\" with the matching"
echo "module path and sizeBytes (web/cores/manifest.json)."
