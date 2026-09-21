#!/usr/bin/env bash
# Builds the libretro cores as native iOS dylibs, which the app dlopens out of Frameworks/.
#
# Not Emscripten and not RetroArch. A RetroArch bundle owns the canvas, the audio graph and
# the main loop, which are exactly the three things this architecture keeps in Rust. Each core
# here is compiled from its own sources against the iphoneos SDK and linked as a dylib that
# exports the plain libretro C API, and nothing else.
#
# Usage (macOS host only, except where noted):
#   scripts/build-core.sh ios fceumm       # one core  -> native/ios/build/lib/*.dylib
#   scripts/build-core.sh ios-all          # every core
#   scripts/build-core.sh ios-names        # print the canonical dylib filenames, ANY host
#
# `ios-names` runs anywhere on purpose. It is the single source of those filenames, and
# native/ios/build-engine.sh and native/ios/package-ipa.sh read them from here rather than
# repeating them, so the .ipa's contents cannot drift from what the app looks for.
#
# This script used to have a second, larger half that compiled each core to a WebAssembly
# reactor module for a browser build, where a BARE CORE NAME meant wasm and the iOS builds
# needed their own subcommand namespace to avoid colliding with it. That build is gone. The
# namespace stays because the spellings are baked into the workflow and the two packaging
# scripts, and because `ios-all` is clearer than `all` about what it produces.
#
# C and C++ cores are both supported: each source is compiled by the driver for its own
# language, and a core with any C++ is linked by clang++ so libc++ comes in.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WORK="$ROOT/.work"

# --------------------------------------------------------------- iOS native cores
#
# The iOS cores are now the only path here. They never used wasi-sdk, the RETRO_EXPORTS list,
# the wasm import-restriction check, core-shim, or web/cores/, which is why the browser half
# could be removed without touching any of this. Each core is compiled from source with Apple's clang for
# aarch64-apple-ios and produces a native .dylib that the iOS app dlopens out of
# Frameworks/ at runtime.
#
# THEY LIVE IN THEIR OWN SUBCOMMAND NAMESPACE, AND THAT IS NOT A STYLE CHOICE:
#
#   scripts/build-core.sh fceumm       # WASM, for the web. UNCHANGED.
#   scripts/build-core.sh all          # WASM, all four web cores. UNCHANGED.
#   scripts/build-core.sh ios fceumm   # one iOS dylib -> native/ios/build/lib/
#   scripts/build-core.sh ios-all      # every iOS core
#   scripts/build-core.sh ios-names    # print the canonical dylib filenames and stop
#
# A bare core name already means "build it as WASM for the web", and those spellings are
# live and documented: web/cores/README.md, README.md, SESSION_HANDOFF.md, the buildHint
# strings in the now-deleted web tooling all used them. Teaching `build-core.sh fceumm` to build an
# iOS dylib would therefore break the web build, quietly, in the one place nobody looks
# until the PWA stops loading a core. Hence the namespace.
#
# The first iOS core was added as `build-core.sh pcsx_rearmed`, which was safe only because
# pcsx_rearmed has no WASM case block. That spelling still works as an alias for
# `ios pcsx_rearmed`, because something may still call it, but the explicit form is the one
# to use and native/ios/build-engine.sh has been moved onto it.
#
# The iOS dispatch runs BEFORE ensure_toolchain, so this path never fetches wasi-sdk and
# never creates or writes web/cores/. `ios` and `ios-all` need a macOS host and say so
# clearly anywhere else, the same contract as native/ios/build-engine.sh. `ios-names` is the
# exception: it only prints the table, so it runs on any host, which is what makes the
# filenames checkable from Linux.
#
# PER-CORE GROUND TRUTH, read out of each upstream makefile rather than assumed:
#
#   fceumm           Makefile.libretro at the repo root (the root Makefile is a one-line
#                    include of it). platform=ios-arm64 sets CC='cc -arch arm64 -isysroot
#                    $(IOSSDK)', SHARED=-dynamiclib and TARGET=$(TARGET_NAME)_libretro_ios.dylib,
#                    which is already the canonical filename in the table below.
#                    Its iOS block also forces WANT_32BPP := 1, so this core renders
#                    XRGB8888 on device where the WASM build renders RGB565. That is fine:
#                    the format is renegotiated through SET_PIXEL_FORMAT inside
#                    retro_load_game and native_core.rs reports whatever the core chose.
#   genesis_plus_gx  Makefile.libretro at the repo root. platform=ios-arm64 sets CC/CXX to
#                    arm64 with -isysroot, SHARED=-dynamiclib (no --version-script on iOS,
#                    which Apple's linker would reject) and the same
#                    $(TARGET_NAME)_libretro_ios.dylib TARGET.
#   snes9x           The makefile is libretro/Makefile and it sets CORE_DIR := .., so make
#                    must run INSIDE the libretro subdirectory and the dylib lands there.
#                    C++: LD is $(CXX), so libc++ comes in by itself.
#   pcsx_rearmed     Makefile.libretro at the repo root, with recursive submodules
#                    (lightrec, libchdr). platform=ios-arm64 force-disables the dynarec,
#                    so this core is INTERPRETER-only: correct but slower, and with no
#                    dependence on the JIT entitlement being honoured. Deliberate, and
#                    unchanged from the build that already worked.
#   mgba             Ships NO makefile at all. Its libretro core is a CMake target, which
#                    is also why the WASM path uses CMake for it (CMake generates files the
#                    build needs, version.c among them). So the iOS path configures CMake
#                    for iOS, builds the static mgba_libretro archive, and links the dylib
#                    itself. See build_ios_cmake_core for why -force_load is load-bearing.
#
# All but one of the canonical filenames below are therefore exactly what upstream emits.
# Only mgba's is ours, because only mgba's link is ours.

# The iOS cores, in build order. Never empty, which matters: macOS ships bash 3.2,
# where an empty array expanded under `set -u` is an error rather than nothing.
IOS_CORES=(fceumm mgba genesis_plus_gx snes9x pcsx_rearmed melonds)

# Staged next to libcontinuum_switch.dylib so project.yml's relative `build/lib/...` paths
# resolve and package-ipa.sh's fallback finds them in $OUT/lib.
IOS_OUT_DIR="$ROOT/native/ios/build/lib"

# iOS sources are cloned into their own directory under $WORK rather than directly into
# $WORK/<core>. A second build path used to clone the same repositories into $WORK/<core>,
# and both compiled in tree (`%.o: %.c` drops the object next to the source), so a shared
# directory let stale objects from the other target be linked
# into an iOS dylib. Two trees, two builds, no interference.
IOS_WORK="$WORK/ios"

# Matches IPHONEOS_DEPLOYMENT_TARGET in native/ios/project.yml. Only the mgba link and its
# CMake configure read it; the four makefile cores set their own -miphoneos-version-min=8.0,
# which is lower and therefore still loadable on iOS 16, so they are left alone.
IOS_MIN_VERSION="16.0"

# The spelling the PS1 core was originally built with, kept working as an alias.
IOS_LEGACY_ALIAS="pcsx_rearmed"

# An absolute path to this script. `ios-all` re-invokes it, once per core, as a separate shell
# PROCESS rather than a subshell, and build_all_ios_cores explains why that distinction decides
# whether a failed `make` is noticed.
IOS_SELF="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/$(basename "${BASH_SOURCE[0]}")"

# THE canonical filenames. Each of these exact strings must match, byte for byte:
#   - native/ios/build-engine.sh   (which reads them from `ios-names`, so it cannot drift)
#   - native/ios/package-ipa.sh    (same, its embed fallback reads `ios-names`)
#   - native/ios/project.yml       framework: build/lib/<name>
#   - .github/workflows/ios.yml    the "Verify the .ipa" greps
#   - native/ios/ContinuumApp.swift CoreCatalog's `library` field per core
# A single divergence turns macOS CI red, or ships an app that cannot find a core, so they
# are defined once, here, and nowhere else in this script.
ios_core_config() {
  IOS_REPO=""
  IOS_DYLIB_NAME=""
  IOS_KIND=""
  IOS_MAKEFILE=""
  IOS_MAKE_SUBDIR="."
  IOS_SUBMODULES=0
  IOS_CMAKE_TARGET=""
  IOS_CMAKE_ARCHIVE=""
  IOS_DISPLAY=""
  case "$1" in
    fceumm)
      IOS_REPO="https://github.com/libretro/libretro-fceumm"
      IOS_DYLIB_NAME="fceumm_libretro_ios.dylib"
      IOS_KIND="make"
      IOS_MAKEFILE="Makefile.libretro"
      IOS_DISPLAY="NES"
      ;;
    mgba)
      IOS_REPO="https://github.com/libretro/mgba"
      IOS_DYLIB_NAME="mgba_libretro_ios.dylib"
      IOS_KIND="cmake"
      IOS_CMAKE_TARGET="mgba_libretro"
      IOS_CMAKE_ARCHIVE="mgba_libretro.a"
      IOS_DISPLAY="GBA + GB/GBC"
      ;;
    genesis_plus_gx)
      IOS_REPO="https://github.com/libretro/Genesis-Plus-GX"
      IOS_DYLIB_NAME="genesis_plus_gx_libretro_ios.dylib"
      IOS_KIND="make"
      IOS_MAKEFILE="Makefile.libretro"
      IOS_DISPLAY="Mega Drive + Master System + Game Gear"
      ;;
    snes9x)
      IOS_REPO="https://github.com/libretro/snes9x"
      IOS_DYLIB_NAME="snes9x_libretro_ios.dylib"
      IOS_KIND="make"
      IOS_MAKEFILE="Makefile"
      # CORE_DIR := .. inside that makefile, so it only builds from its own directory.
      IOS_MAKE_SUBDIR="libretro"
      IOS_DISPLAY="SNES, C++"
      ;;
    pcsx_rearmed)
      IOS_REPO="https://github.com/libretro/pcsx_rearmed"
      IOS_DYLIB_NAME="pcsx_rearmed_libretro_ios.dylib"
      IOS_KIND="make"
      IOS_MAKEFILE="Makefile.libretro"
      # lightrec and libchdr, and they are needed recursively.
      IOS_SUBMODULES=1
      IOS_DISPLAY="PS1, interpreter only"
      ;;
    melonds)
      IOS_REPO="https://github.com/libretro/melonDS"
      IOS_DYLIB_NAME="melonds_libretro_ios.dylib"
      IOS_KIND="make"
      IOS_MAKEFILE="Makefile"
      # NO SUBMODULES, unlike pcsx_rearmed: this core vendors everything it needs.
      #
      # AND NO HARDWARE RENDERER, which is the whole reason the DS arrives before the N64
      # despite the plan in docs/SET_HW_RENDER_DESIGN.md listing it later. That plan put the DS
      # behind ANGLE because melonDS has an OpenGL renderer, and reading the makefile shows iOS
      # never gets it: `HAVE_OPENGL := 0` and `HAVE_OPENGLES3 := 0` are the file's defaults, the
      # unix block is the only one that turns GL on, and the ios block leaves both alone. So this
      # core emits pixels through the same path the other five already use, and needs none of
      # MoltenVK, ANGLE or SET_HW_RENDER.
      #
      # Its framebuffer is 256x384: BOTH SCREENS, already stacked top over bottom, in one
      # texture. So the existing single-screen composite draws them correctly with no change at
      # all; the instanced pass added for step 2 is what will later allow rearranging them, not
      # what makes them appear.
      IOS_DISPLAY="Nintendo DS, software renderer"
      ;;
    *)
      return 1
      ;;
  esac
}

# Prints the canonical dylib filenames, one per line, and touches nothing. Runs on any host
# on purpose: it is how native/ios/build-engine.sh and native/ios/package-ipa.sh learn the
# list instead of restating it, and how the list gets checked without a Mac.
ios_print_names() {
  local core
  for core in "${IOS_CORES[@]}"; do
    ios_core_config "$core"
    echo "$IOS_DYLIB_NAME"
  done
}

ios_usage() {
  cat <<EOF
scripts/build-core.sh - build the libretro cores the iOS app dlopens.

  ios <core>    build one core. Valid: ${IOS_CORES[*]}
  ios-all       build all ${#IOS_CORES[@]} cores, continuing past a failure and
                summarising at the end
  ios-names     print the canonical .dylib filenames and exit. Works on any host,
                which is why build-engine.sh and package-ipa.sh read the names from
                here rather than repeating them and drifting.

'ios' and 'ios-all' need a macOS host with the Xcode command line tools.
EOF
}

ios_require_darwin() {
  if [[ "$(uname -s)" != "Darwin" ]]; then
    cat >&2 <<'EOF'
error: the iOS cores build only on a macOS host.

  They cross-compile real libretro cores with Apple's clang and the iphoneos SDK:
      make -f <makefile> platform=ios-arm64 IOSSDK=$(xcrun --sdk iphoneos --show-sdk-path)
  and, for mgba, a CMake configure for CMAKE_SYSTEM_NAME=iOS plus a dylib link. There is no
  wasi-sdk fallback for any of it. Build them on the macOS runner through the ios workflow
  (native/ios/build-engine.sh calls this), which is what it is for.

  `scripts/build-core.sh ios-names` does work here, and prints the filenames the .ipa
  expects.
EOF
    exit 1
  fi

  command -v xcrun >/dev/null 2>&1 || { echo "error: xcrun not found (need the Xcode command line tools)" >&2; exit 1; }
  command -v make >/dev/null 2>&1 || { echo "error: make not found" >&2; exit 1; }
  command -v git >/dev/null 2>&1 || { echo "error: git not found" >&2; exit 1; }
}

ios_resolve_sdk() {
  IOSSDK="$(xcrun --sdk iphoneos --show-sdk-path)"
  [[ -n "$IOSSDK" && -d "$IOSSDK" ]] || {
    echo "error: could not resolve the iphoneos SDK path via xcrun" >&2
    exit 1
  }
}

ios_jobs() {
  sysctl -n hw.ncpu 2>/dev/null || echo 4
}

ios_clone() {
  local core="$1"
  IOS_SRC_DIR="$IOS_WORK/$core"
  mkdir -p "$IOS_WORK"
  if [[ ! -d "$IOS_SRC_DIR/.git" ]]; then
    echo "==> cloning $core for iOS"
    if [[ "$IOS_SUBMODULES" == "1" ]]; then
      # No --depth: a shallow clone and recursive submodules are a bad combination, and
      # this is the one core that has submodules.
      git clone "$IOS_REPO" "$IOS_SRC_DIR"
    else
      git clone --depth 1 "$IOS_REPO" "$IOS_SRC_DIR"
    fi
  fi
  if [[ "$IOS_SUBMODULES" == "1" ]]; then
    echo "==> initialising submodules for $core"
    ( cd "$IOS_SRC_DIR" && git submodule update --init --recursive )
  fi
}

# The one place an install name is fixed and a dylib is staged.
#
# dlopen from Frameworks/ resolves against @rpath (LD_RUNPATH_SEARCH_PATHS in project.yml
# points at @executable_path/Frameworks). If the build did not set an @rpath install name,
# force one so the on-device load resolves. Harmless to reassert when it is already correct.
ios_stage_dylib() {
  local built="$1"
  local canonical="$2"
  mkdir -p "$IOS_OUT_DIR"

  # A dylib with no libretro API in it is the one failure every check downstream would miss:
  # build-engine.sh asks whether the file exists, ios.yml greps the zip listing for its name,
  # and the Swift host checks the path in Frameworks/. All three pass, and the app then reports
  # a missing symbol on device, a whole build-and-sideload cycle later. It is not a theoretical
  # worry either: the mgba link below pulls a whole LTO archive in with -force_load precisely
  # because a link that resolves nothing still produces a valid, empty dylib. So assert the API
  # is actually in there, for every core, where the artefact is staged.
  if ! nm -gU "$built" 2>/dev/null | grep -q "retro_run"; then
    echo "error: $built exports no retro_run; it is not a usable libretro core" >&2
    echo "       (a dylib that links but resolves no core objects looks fine to every" >&2
    echo "        later check and fails only on device)" >&2
    exit 1
  fi

  local install_name
  install_name="$(otool -D "$built" 2>/dev/null | tail -n +2 | head -1 || true)"
  if [[ "$install_name" != "@rpath/$canonical" ]]; then
    echo "==> setting install_name to @rpath/$canonical (was: ${install_name:-none})"
    install_name_tool -id "@rpath/$canonical" "$built"
  fi

  cp "$built" "$IOS_OUT_DIR/$canonical"
  echo "==> done: native/ios/build/lib/$canonical ($(du -h "$IOS_OUT_DIR/$canonical" | cut -f1))"
}

build_ios_make_core() {
  local core="$1"
  local make_dir="$IOS_SRC_DIR/$IOS_MAKE_SUBDIR"

  [[ -f "$make_dir/$IOS_MAKEFILE" ]] || {
    echo "error: $core: no $IOS_MAKEFILE in $make_dir; upstream moved its libretro makefile" >&2
    exit 1
  }

  # Cleared before the build, not after. The clone in .work/ios/<core> persists between runs,
  # so without this a dylib left over from an earlier successful build would satisfy the
  # existence check below even if this build produced nothing.
  rm -f "$make_dir/$IOS_DYLIB_NAME"

  echo "==> building $core for ios-arm64 ($IOS_DISPLAY)"
  # platform=ios-arm64 is what selects the arm64 clang, -dynamiclib, the iOS defines and the
  # .dylib TARGET. IOSSDK has to be passed explicitly: the makefiles fall back to shelling
  # out to xcodebuild for it, which is slower and, on some runners, wrong.
  ( cd "$make_dir" && make -f "$IOS_MAKEFILE" platform=ios-arm64 IOSSDK="$IOSSDK" -j"$(ios_jobs)" )

  # Every one of these makefiles sets TARGET := $(TARGET_NAME)_libretro_ios.dylib for an ios
  # platform, which is already the canonical name, so that path is tried first. The search is
  # the honest fallback for a core that renames its TARGET upstream, and it is deliberately
  # narrow: only a *_libretro_ios.dylib qualifies, so an unrelated dylib in the tree can never
  # be staged under a load-bearing name.
  local built="$make_dir/$IOS_DYLIB_NAME"
  if [[ ! -f "$built" ]]; then
    built="$(find "$make_dir" -maxdepth 1 -name '*_libretro_ios.dylib' -type f | head -1 || true)"
  fi
  [[ -n "$built" && -f "$built" ]] || {
    echo "error: $core: the make finished but left no .dylib in $make_dir" >&2
    exit 1
  }
  if [[ "$(basename "$built")" != "$IOS_DYLIB_NAME" ]]; then
    echo "==> note: upstream produced $(basename "$built"); staging it as $IOS_DYLIB_NAME"
  fi

  ios_stage_dylib "$built" "$IOS_DYLIB_NAME"
}

build_ios_cmake_core() {
  local core="$1"
  command -v cmake >/dev/null 2>&1 || {
    echo "error: $core needs cmake on the host (brew install cmake)" >&2
    exit 1
  }

  local build_dir="$IOS_SRC_DIR/build-ios"
  echo "==> configuring $core with cmake for iOS ($IOS_DISPLAY)"
  rm -rf "$build_dir"
  # Everything optional is off, exactly as the WASM configure has it: no zlib, png, sqlite,
  # ffmpeg, zip, lzma, ELF loading, scripting or debuggers. LIBRETRO_STATIC=ON asks for the
  # archive rather than a shared library, because the link is done below where the iOS flags
  # are ours to set. It does NOT rename any retro_* symbol; mgba's CMakeLists uses it only to
  # choose the library type.
  #
  # Two upstream details worth knowing before reading a CI log: mgba's CMakeLists forces
  # CMAKE_OSX_DEPLOYMENT_TARGET to 10.6 inside its if(APPLE) block, which only lowers the
  # objects' minimum OS and cannot stop them loading on iOS 16, and it appends -flto to the
  # Apple Release flags, so the archive holds bitcode that the linker resolves at link time.
  #
  # Do NOT pass -DCMAKE_TRY_COMPILE_TARGET_TYPE=STATIC_LIBRARY here. It used to be set
  # pre-emptively, guessing that CMAKE_SYSTEM_NAME=iOS plus cross-compiling would make mgba's
  # configure-time probes fail while trying to LINK an executable. CI disproved the guess and
  # exposed the cost: without the flag the configure reports its compiler checks as skipped or
  # done, finishes in ~9s and generates build files, so it was never needed; meanwhile the flag
  # makes every check_function_exists probe COMPILE ONLY and never link, so they all "succeed".
  #
  # That silently broke popcount32, which is not a libc function on any platform. mgba probes
  # for it with find_function(popcount32); src/platform/cmake/FindFunction.cmake upper-cases the
  # name into check_function_exists(popcount32 HAVE_POPCOUNT32), and mgba ships its own
  # implementation in include/mgba-util/math.h behind `#ifndef HAVE_POPCOUNT32`. Compile-only
  # probing of a call to a function that does not exist merely warns, so the probe logged
  # "Looking for popcount32 - found", HAVE_POPCOUNT32 went into FUNCTION_DEFINES, math.h
  # #ifndef'd its own implementation away, and the build then died at src/gba/gba.c:476 with
  # "call to undeclared function 'popcount32'" -- an ERROR, not a warning, on AppleClang 21.
  #
  # mgba knows this trap exists and compensates, but only for everyone else: its CMakeLists
  # hardcodes a known-good FUNCTION_DEFINES list under
  # `if(CMAKE_TRY_COMPILE_TARGET_TYPE STREQUAL "STATIC_LIBRARY" AND NOT APPLE)`. The
  # `AND NOT APPLE` is why an iOS build gets no rescue and keeps the bogus probe results.
  #
  # Do NOT reach for -Wno-implicit-function-declaration to quieten gba.c either. Suppressing
  # the diagnostic does not bring popcount32 back: math.h has already compiled its only
  # implementation out, so the failure just moves to link time as an undefined symbol.
  #
  # Belt and braces on top of not passing the flag, because a retry costs a whole CI run:
  # pre-seed the probe's result. check_function_exists is a no-op when its result variable is
  # already set -- CheckFunctionExists.cmake wraps the probe in
  # `if(NOT DEFINED "${VARIABLE}" ...)` -- so -DHAVE_POPCOUNT32=OFF keeps math.h's own
  # implementation even if a probe misbehaves again. HAVE_POPCOUNT32 is the real variable name
  # rather than a guess: it is TOUPPER(popcount32) from FindFunction.cmake, and it is the same
  # macro math.h tests.
  cmake -B "$build_dir" -S "$IOS_SRC_DIR" \
    -DCMAKE_SYSTEM_NAME=iOS \
    -DCMAKE_OSX_ARCHITECTURES=arm64 \
    -DCMAKE_OSX_SYSROOT="$IOSSDK" \
    -DCMAKE_OSX_DEPLOYMENT_TARGET="$IOS_MIN_VERSION" \
    -DCMAKE_BUILD_TYPE=Release \
    -DCMAKE_C_FLAGS="-Wno-everything" \
    -DHAVE_POPCOUNT32=OFF \
    -DBUILD_LIBRETRO=ON -DLIBRETRO_STATIC=ON -DSKIP_LIBRARY=ON \
    -DBUILD_QT=OFF -DBUILD_SDL=OFF -DBUILD_PYTHON=OFF \
    -DBUILD_TEST=OFF -DBUILD_SUITE=OFF \
    -DUSE_ZLIB=OFF -DUSE_PNG=OFF -DUSE_SQLITE3=OFF -DUSE_FFMPEG=OFF \
    -DUSE_LIBZIP=OFF -DUSE_MINIZIP=OFF -DUSE_LZMA=OFF -DUSE_ELF=OFF \
    -DUSE_EPOXY=OFF -DUSE_DISCORD_RPC=OFF -DUSE_EDITLINE=OFF -DUSE_LUA=OFF \
    -DENABLE_SCRIPTING=OFF

  echo "==> building $IOS_CMAKE_TARGET"
  cmake --build "$build_dir" --target "$IOS_CMAKE_TARGET" -j"$(ios_jobs)"

  local archive="$build_dir/$IOS_CMAKE_ARCHIVE"
  [[ -f "$archive" ]] || {
    echo "error: $core: expected $archive after the cmake build" >&2
    find "$build_dir" -maxdepth 2 -name '*.a' >&2 || true
    exit 1
  }

  local built="$build_dir/$IOS_DYLIB_NAME"
  echo "==> linking $IOS_DYLIB_NAME from $IOS_CMAKE_ARCHIVE"
  # -force_load is load-bearing. Nothing in this link references retro_run or any other
  # entry point, so the linker would pull in no archive members at all and hand back a
  # valid, empty dylib that dlopens and then has no libretro API in it. -force_load takes
  # every member. -framework Foundation matches the OS_LIB mgba's CMakeLists appends on
  # Apple, and -lm the M_LIBRARY it appends everywhere else.
  cc -arch arm64 -isysroot "$IOSSDK" -miphoneos-version-min="$IOS_MIN_VERSION" \
    -dynamiclib -install_name "@rpath/$IOS_DYLIB_NAME" \
    -o "$built" -Wl,-force_load,"$archive" \
    -framework Foundation -lm
  [[ -f "$built" ]] || {
    echo "error: $core: the dylib link reported success but produced nothing" >&2
    exit 1
  }

  ios_stage_dylib "$built" "$IOS_DYLIB_NAME"
}

build_ios_core() {
  local core="$1"
  ios_core_config "$core" || {
    echo "error: '$core' is not an iOS core. Valid: ${IOS_CORES[*]}" >&2
    exit 1
  }
  ios_require_darwin
  ios_resolve_sdk
  mkdir -p "$IOS_OUT_DIR"
  # So the staging directory, and therefore the ios-all summary, can only ever report what THIS
  # run produced.
  rm -f "$IOS_OUT_DIR/$IOS_DYLIB_NAME"
  ios_clone "$core"

  case "$IOS_KIND" in
    make) build_ios_make_core "$core" ;;
    cmake) build_ios_cmake_core "$core" ;;
    *) echo "error: $core: unknown iOS build kind '$IOS_KIND'" >&2; exit 1 ;;
  esac
}

# Builds them all, and keeps going after a failure on purpose.
#
# The macOS runner is the only compiler this project has, so a run that stops at the first
# broken core costs a whole cycle to learn about the second. Each core is built on its own,
# every failure is named, and the summary at the end lists what is and is not in
# native/ios/build/lib/. The command still exits non-zero if anything failed, so
# build-engine.sh and CI still go red.
#
# EACH CORE RUNS IN A SEPARATE `bash` PROCESS, NOT A SUBSHELL, AND THAT IS THE WHOLE TRICK.
# Bash disables errexit for a command used as an `if` condition, and that suppression is
# inherited by subshells inside it, so `if ( build_ios_core "$core" ); then` would let a failing
# `make`, `install_name_tool` or `cp` fall through to the artefact check instead of aborting the
# core. The only thing that decided pass or fail would then be whether a .dylib exists, which a
# stale one from an earlier run can satisfy. A separate process re-reads this script and applies
# its own `set -euo pipefail`, so the suppression cannot cross into it.
build_all_ios_cores() {
  ios_require_darwin

  # A string, not an array: on bash 3.2 an empty array expanded under `set -u` is an error,
  # and this one is empty exactly when everything worked.
  local failed=""
  local failed_count=0
  local core
  for core in "${IOS_CORES[@]}"; do
    echo
    echo "======================================================== ios: $core"
    if bash "$IOS_SELF" ios "$core"; then
      echo "==> $core ok"
    else
      echo "!!! $core FAILED (see the output above for why)" >&2
      failed="$failed $core"
      failed_count=$((failed_count + 1))
    fi
  done

  echo
  echo "==> iOS core summary (native/ios/build/lib)"
  for core in "${IOS_CORES[@]}"; do
    ios_core_config "$core"
    if [[ -f "$IOS_OUT_DIR/$IOS_DYLIB_NAME" ]]; then
      echo "      ok       $IOS_DYLIB_NAME"
    else
      echo "      MISSING  $IOS_DYLIB_NAME"
    fi
  done

  if [[ "$failed_count" -gt 0 ]]; then
    echo "error: $failed_count iOS core(s) failed to build:$failed" >&2
    exit 1
  fi
}
# ---------------------------------------------------------------------- dispatch
#
# Every subcommand is explicit and anything else is an error. This used to fall through to a
# WASM build for the browser app, and a bare core name meant "build it as wasm" - which is
# why the iOS builds were given their own namespace rather than overloading a core name. The
# browser app is gone, so the fall-through is gone with it, and an unrecognised argument now
# says so instead of quietly doing something else.
case "${1:-}" in
  ios)
    if [[ "$#" -lt 2 ]]; then
      echo "error: 'ios' needs a core name. Valid: ${IOS_CORES[*]}" >&2
      exit 1
    fi
    build_ios_core "$2"
    exit 0
    ;;
  ios-all)
    build_all_ios_cores
    exit 0
    ;;
  ios-names)
    ios_print_names
    exit 0
    ;;
  "$IOS_LEGACY_ALIAS")
    # Backwards compatibility only. `ios pcsx_rearmed` is the spelling to use; this one is
    # kept because it is what the PS1 core was originally built with.
    build_ios_core "$IOS_LEGACY_ALIAS"
    exit 0
    ;;
  ""|-h|--help|help)
    ios_usage
    exit 0
    ;;
  *)
    echo "error: unknown subcommand '$1'." >&2
    echo >&2
    # A bare core name used to build that core as WASM, so someone reaching for the old
    # spelling gets told what replaced it rather than a bare usage dump.
    if [[ " ${IOS_CORES[*]} " == *" $1 "* ]]; then
      echo "'$1' on its own used to mean a WASM build for the browser app, which no" >&2
      echo "longer exists. To build it for iOS: scripts/build-core.sh ios $1" >&2
    else
      ios_usage >&2
    fi
    exit 1
    ;;
esac
