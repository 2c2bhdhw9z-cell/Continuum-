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
# Usage (WASM, for the web):
#   scripts/build-core.sh fceumm           # NES (~1.7 MB)
#   scripts/build-core.sh mgba             # GBA + GB/GBC (~1.5 MB)
#   scripts/build-core.sh genesis_plus_gx  # Mega Drive + Master System (~2.8 MB)
#   scripts/build-core.sh snes9x           # SNES, C++ (~2.5 MB)
#   scripts/build-core.sh all
#
# Usage (native iOS dylibs, macOS host only):
#   scripts/build-core.sh ios fceumm       # one core  -> native/ios/build/lib/*.dylib
#   scripts/build-core.sh ios-all          # all five iOS cores
#   scripts/build-core.sh ios-names        # print the canonical dylib filenames, any host
#
# A BARE CORE NAME ALWAYS MEANS WASM. The iOS builds live in their own subcommand namespace
# for that reason alone; see the "iOS native cores" section below, which explains why
# overloading a core name would break the web build.
#
# C and C++ cores are both supported: each source is compiled by the driver for its own
# language, and a core with any C++ is linked by clang++ so libc++ comes in. See
# CXX_BASE_FLAGS below for the one real constraint (no exceptions, no RTTI).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TOOLS="$ROOT/.tools"
WORK="$ROOT/.work"
OUT_DIR="$ROOT/web/cores"

# --------------------------------------------------------------- iOS native cores
#
# The iOS cores are a *completely separate* path from the wasi-sdk web strategies below.
# They do not use wasi-sdk, the RETRO_EXPORTS list, the wasm import-restriction check,
# core-shim, or web/cores/. Each core is compiled from source with Apple's clang for
# aarch64-apple-ios and produces a native .dylib that the iOS app dlopens out of
# Frameworks/ at runtime.
#
# THEY LIVE IN THEIR OWN SUBCOMMAND NAMESPACE, AND THAT IS NOT A STYLE CHOICE:
#
#   scripts/build-core.sh fceumm       # WASM, for the web. UNCHANGED.
#   scripts/build-core.sh all          # WASM, all four web cores. UNCHANGED.
#   scripts/build-core.sh ios fceumm   # one iOS dylib -> native/ios/build/lib/
#   scripts/build-core.sh ios-all      # all five iOS cores
#   scripts/build-core.sh ios-names    # print the canonical dylib filenames and stop
#
# A bare core name already means "build it as WASM for the web", and those spellings are
# live and documented: web/cores/README.md, README.md, SESSION_HANDOFF.md, the buildHint
# strings in scripts/core-abi-test.mjs and the `build-core.sh all` step in
# .github/workflows/deploy.yml all use them. Teaching `build-core.sh fceumm` to build an
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
# Four of the five canonical filenames below are therefore exactly what upstream emits.
# Only mgba's is ours, because only mgba's link is ours.

# The five iOS cores, in build order. Never empty, which matters: macOS ships bash 3.2,
# where an empty array expanded under `set -u` is an error rather than nothing.
IOS_CORES=(fceumm mgba genesis_plus_gx snes9x pcsx_rearmed)

# Staged next to libcontinuum_switch.dylib so project.yml's relative `build/lib/...` paths
# resolve and package-ipa.sh's fallback finds them in $OUT/lib.
IOS_OUT_DIR="$ROOT/native/ios/build/lib"

# iOS sources are cloned HERE, not into $WORK/<core>. The WASM path clones the same
# repositories into $WORK/<core>, and both builds compile in tree (`%.o: %.c` drops the
# object next to the source), so a shared directory would let stale wasm objects be linked
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
  expects. The WASM cores build here too: scripts/build-core.sh all.
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
  # CMAKE_TRY_COMPILE_TARGET_TYPE=STATIC_LIBRARY is passed rather than left as the thing to try
  # if configuring fails. CMAKE_SYSTEM_NAME=iOS puts CMake in cross-compiling mode, and mgba's
  # configure then runs its compiler check and a check_symbol_exists probe, both of which
  # normally LINK an executable. Building those probes as static libraries removes a whole class
  # of first-run configure failure, and it costs nothing here: the only thing it can skew is
  # link-based function probes, and every function mgba probes for (strdup, strndup, locale,
  # strtof_l, localtime_r) genuinely does exist on Darwin. mgba's own CMakeLists guards its
  # compensating override with `AND NOT APPLE` for exactly that reason.
  cmake -B "$build_dir" -S "$IOS_SRC_DIR" \
    -DCMAKE_SYSTEM_NAME=iOS \
    -DCMAKE_OSX_ARCHITECTURES=arm64 \
    -DCMAKE_OSX_SYSROOT="$IOSSDK" \
    -DCMAKE_OSX_DEPLOYMENT_TARGET="$IOS_MIN_VERSION" \
    -DCMAKE_TRY_COMPILE_TARGET_TYPE=STATIC_LIBRARY \
    -DCMAKE_BUILD_TYPE=Release \
    -DCMAKE_C_FLAGS="-Wno-everything" \
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

# Builds all five, and keeps going after a failure on purpose.
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

# ---------------------------------------------------------------------- dispatch
#
# The iOS subcommands are handled FIRST and then exit, so none of the wasi-sdk machinery runs
# for them: no toolchain download, no web/cores/ directory, no manifest reminder. Everything
# else falls through to the WASM path exactly as it always has, which is the whole point of
# giving the iOS builds their own subcommand names instead of overloading a core name.
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
esac

ensure_toolchain

if [[ "${1:-fceumm}" == "all" ]]; then
  for core in fceumm mgba genesis_plus_gx snes9x; do build_core "$core"; done
else
  build_core "${1:-fceumm}"
fi

echo "Set the core's manifest entry to \"kind\": \"libretro\" with the matching"
echo "module path and sizeBytes (web/cores/manifest.json)."
