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
# Each core is compiled from source with Apple's clang for aarch64-apple-ios and produces a
# native .dylib that the iOS app dlopens out of Frameworks/ at runtime.
#
# THE SUBCOMMANDS (the header says why they keep the `ios` prefix):
#
#   scripts/build-core.sh ios fceumm             # one iOS dylib -> native/ios/build/lib/
#   scripts/build-core.sh ios-all                # every iOS core
#   scripts/build-core.sh ios-names              # print the canonical dylib filenames and stop
#   scripts/build-core.sh ios-optional-names     # the same for the optional from-source cores
#   scripts/build-core.sh ios-stage-prebuilt ... # check and stage a dylib built elsewhere
#
# A bare core name is not a subcommand. The dispatch at the end refuses it and names the
# `ios <core>` spelling instead. The one exception is `build-core.sh pcsx_rearmed`, the spelling
# the first iOS core was added with: it still works as an alias for `ios pcsx_rearmed`, because
# something may still call it, but the explicit form is the one to use and
# native/ios/build-engine.sh has been moved onto it.
#
# `ios` and `ios-all` need a macOS host and say so clearly anywhere else, the same contract as
# native/ios/build-engine.sh. `ios-names`, `ios-optional-names` and `ios-stage-prebuilt` are
# the exceptions: they only print the table or check and copy a file, so they run on any host,
# which is what makes the filenames checkable from Linux.
#
# PER-CORE GROUND TRUTH, read out of each upstream makefile rather than assumed:
#
#   fceumm           Makefile.libretro at the repo root (the root Makefile is a one-line
#                    include of it). platform=ios-arm64 sets CC='cc -arch arm64 -isysroot
#                    $(IOSSDK)', SHARED=-dynamiclib and TARGET=$(TARGET_NAME)_libretro_ios.dylib,
#                    which is already the canonical filename in the table below.
#                    Its iOS block also forces WANT_32BPP := 1, so this core renders
#                    XRGB8888 on device rather than its default RGB565. That is fine:
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
#   mgba             Ships NO makefile at all. Its libretro core is a CMake target, and
#                    CMake generates files the build needs (version.c among them). So the
#                    iOS path configures CMake for iOS, builds the static mgba_libretro
#                    archive, and links the dylib itself. See build_ios_cmake_core for why
#                    -force_load is load-bearing.
#
# All but one of the canonical filenames below are therefore exactly what upstream emits.
# Only mgba's is ours, because only mgba's link is ours.

# The iOS cores, in build order. Never empty, which matters: macOS ships bash 3.2,
# where an empty array expanded under `set -u` is an error rather than nothing.
IOS_CORES=(fceumm mgba genesis_plus_gx snes9x pcsx_rearmed mednafen_psx_hw melonds
           mednafen_pce_fast stella2023 parallel_n64 azahar ppsspp)

# Cores built from source that are ALLOWED TO FAIL. `ios-all` builds them after the required
# ones, reports a failure as a warning and still exits 0 for them, and `ios-names` does not list
# them (that list is what build-engine.sh hard-fails on). `ios-optional-names` lists them instead,
# so build-engine.sh warns, package-ipa.sh drops the missing embed from project.yml, the CI verify
# step prints a warning, and the app's cores line names the dylib as not in the bundle.
#
# flycast (Dreamcast) is built here, not fetched: the libretro iOS buildbot's flycast contains the
# ARM64 dynarec and refuses to run without JIT. Interpreter only (TARGET_NO_REC); see its
# ios_core_config entry. It has built green and shipped in every IPA since build 119.
IOS_OPTIONAL_CORES=(flycast pcsx_rearmed_jit parallel_n64_jit flycast_jit)

# THE JIT BUILDS (pcsx_rearmed_jit, parallel_n64_jit, flycast_jit). Same source and pin as the
# core they are named after, built with its recompiler on, as <core>_jit_libretro_ios.dylib. The
# app loads one only when JIT is really usable on the phone at that moment (crates/emulator-bridge/
# src/jit.rs); every other phone loads the regular build, which is not touched by any of this.
# Optional, so a JIT build that fails to compile only means that core has no JIT this build.
# PPSSPP and Azahar need no second build: they ask the host GET_JIT_CAPABLE at run time.

# mednafen_psx_hw (Beetle PSX HW) is in ios-all so Mac CI embeds the dylib in the IPA.
# Build is Mac/CI-only (`make platform=ios-arm64 HAVE_HW=1`); Linux hosts cannot cross-compile
# it. Step 4 stays Partial until a phone shows a Beetle HW frame through SET_HW_RENDER.

# Every libretro entry point the engine resolves by name, which is the real contract between a
# staged dylib and the app. Kept in step with the `Symbols` struct in
# crates/emulator-bridge/src/cores/native_core.rs: that struct resolves all of these during
# `NativeLibretroCore::load`, and a core missing any one of them fails there naming the symbol.
#
# Listed here so that failure happens in CI instead of on a phone. It is not hypothetical: save
# states were broken for the whole project's life because `retro_serialize` was never resolved,
# and the symptom was a save button that failed and a rewind that recorded nothing.
IOS_REQUIRED_SYMBOLS=(
  retro_api_version
  retro_init
  retro_deinit
  retro_set_environment
  retro_set_video_refresh
  retro_set_audio_sample
  retro_set_audio_sample_batch
  retro_set_input_poll
  retro_set_input_state
  retro_get_system_info
  retro_get_system_av_info
  retro_load_game
  retro_unload_game
  retro_run
  retro_reset
  retro_serialize_size
  retro_serialize
  retro_unserialize
  retro_cheat_reset
  retro_cheat_set
)

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
  # The exact upstream commit this core is built from. See "PINNED SOURCES" above ios_clone for
  # why, for the CONTINUUM_UNPINNED=1 override, and for how to move a pin.
  IOS_PIN=""
  IOS_DYLIB_NAME=""
  IOS_KIND=""
  IOS_MAKEFILE=""
  IOS_MAKE_SUBDIR="."
  IOS_SUBMODULES=0
  IOS_CMAKE_TARGET=""
  IOS_CMAKE_ARCHIVE=""
  # Extra `VAR=value` arguments for a make-based core, as an ARRAY so a value containing a space
  # cannot split. Empty for every core that builds correctly with its own defaults.
  IOS_MAKE_VARS=()
  # Selected submodule paths. Empty means "all of them" when IOS_SUBMODULES=1.
  # A non-empty list is a shallow clone plus only those paths, so a core whose
  # full recursive tree is ffmpeg and MoltenVK does not download either.
  IOS_SUBMODULE_PATHS=()
  IOS_DISPLAY=""
  # flycast only: the C/C++ defines. TARGET_NO_REC is the interpreter; the JIT build drops it.
  IOS_FLYCAST_DEFINES="-DIOS -DTARGET_NO_REC"
  case "$1" in
    fceumm)
      IOS_REPO="https://github.com/libretro/libretro-fceumm"
      IOS_PIN="7a542dab1e87679921962a9f056186eca425c0c2"
      IOS_DYLIB_NAME="fceumm_libretro_ios.dylib"
      IOS_KIND="make"
      IOS_MAKEFILE="Makefile.libretro"
      IOS_DISPLAY="NES"
      ;;
    mgba)
      IOS_REPO="https://github.com/libretro/mgba"
      IOS_PIN="7a12d6d4b9acb14c0ae62c9166b6a2f3d08007f6"
      IOS_DYLIB_NAME="mgba_libretro_ios.dylib"
      IOS_KIND="cmake"
      IOS_CMAKE_TARGET="mgba_libretro"
      IOS_CMAKE_ARCHIVE="mgba_libretro.a"
      IOS_DISPLAY="GBA + GB/GBC"
      ;;
    genesis_plus_gx)
      IOS_REPO="https://github.com/libretro/Genesis-Plus-GX"
      IOS_PIN="58c341487e5bfcf979ea68413c7987633adb0c56"
      IOS_DYLIB_NAME="genesis_plus_gx_libretro_ios.dylib"
      IOS_KIND="make"
      IOS_MAKEFILE="Makefile.libretro"
      IOS_DISPLAY="Mega Drive + Master System + Game Gear"
      ;;
    snes9x)
      IOS_REPO="https://github.com/libretro/snes9x"
      IOS_PIN="fae2fea08f74180759ef540ee94259213f503480"
      IOS_DYLIB_NAME="snes9x_libretro_ios.dylib"
      IOS_KIND="make"
      IOS_MAKEFILE="Makefile"
      # CORE_DIR := .. inside that makefile, so it only builds from its own directory.
      IOS_MAKE_SUBDIR="libretro"
      IOS_DISPLAY="SNES, C++"
      ;;
    pcsx_rearmed)
      IOS_REPO="https://github.com/libretro/pcsx_rearmed"
      IOS_PIN="c8816799b50388e61cfe237fe2cdbb7d8175f20a"
      IOS_DYLIB_NAME="pcsx_rearmed_libretro_ios.dylib"
      IOS_KIND="make"
      IOS_MAKEFILE="Makefile.libretro"
      # lightrec and libchdr, and they are needed recursively.
      IOS_SUBMODULES=1
      IOS_DISPLAY="PS1, interpreter only"
      ;;
    pcsx_rearmed_jit)
      # The PCSX ReARMed recompiler (ari64, which has Apple arm64 support upstream: NO_WRITE_EXEC
      # pages switched between writable and runnable). DYNAREC on the command line beats the ios
      # block's `DYNAREC = 0`. NDRC_THREAD=0: no compile thread (see the host rule in options.rs).
      # The Continuum patch maps the cache at run time and uses the iPhone's 16K page size.
      IOS_REPO="https://github.com/libretro/pcsx_rearmed"
      IOS_PIN="c8816799b50388e61cfe237fe2cdbb7d8175f20a"
      IOS_DYLIB_NAME="pcsx_rearmed_jit_libretro_ios.dylib"
      IOS_KIND="make"
      IOS_MAKEFILE="Makefile.libretro"
      IOS_SUBMODULES=1
      IOS_MAKE_VARS=(DYNAREC=ari64 NDRC_THREAD=0)
      IOS_DISPLAY="PS1, recompiler (JIT build)"
      ;;
    melonds)
      IOS_REPO="https://github.com/libretro/melonDS"
      IOS_PIN="66b5d2634cd0a79030562811e6e05f5532f800ba"
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
    mednafen_pce_fast)
      IOS_REPO="https://github.com/libretro/beetle-pce-fast-libretro"
      IOS_PIN="3f946f277aef3aa99a95551618bbcd1dd2bda0d9"
      IOS_DYLIB_NAME="mednafen_pce_fast_libretro_ios.dylib"
      IOS_KIND="make"
      IOS_MAKEFILE="Makefile"
      # No submodules, and its ios block already emits
      # $(TARGET_NAME)_libretro_ios.dylib with TARGET_NAME := mednafen_pce_fast, so the
      # canonical name above is upstream's own rather than ours.
      #
      # The "fast" mednafen PC Engine core rather than the full one, deliberately. Both play
      # HuCard games; the full core adds PC Engine CD and SuperGrafx, and CD needs a syscard
      # BIOS we are not allowed to ship. So the lighter core covers exactly the part of the
      # library that works with no files from the user.
      #
      # need_fullpath is TRUE here, so this core is handed a path and opens the file itself.
      # That is the behaviour the host already had for every core; see `load_content`.
      #
      # HAVE_CHD=0 IS WHAT MAKES THIS CORE BUILD AT ALL, and it is not a workaround so much as
      # dropping something we were never going to use. The first attempt failed compiling the
      # core's BUNDLED COPY OF ZLIB 1.2.11, which is old enough to define functions in K&R style:
      #
      #     const char * ZEXPORT zError(err)
      #         int err;
      #
      # Xcode 26's clang defaults to a C standard that removed K&R definitions, so it is an error
      # rather than a warning, and the same file also collided with the SDK's own _stdio.h.
      #
      # Reading Makefile.common shows zlib, libchdr and zstd are compiled ONLY inside
      # `ifeq ($(HAVE_CHD), 1)`, so turning CHD off removes every failing file. Nothing is lost:
      # CHD is a compressed DISC format, this app routes only `.pce` HuCard files to this core, and
      # PC Engine CD needs a system card BIOS that cannot ship anyway. NEED_CD is deliberately left
      # alone, because Makefile.common defines -DNEED_CD unconditionally while guarding the sources
      # it needs, so switching it off compiles code whose files are absent.
      IOS_MAKE_VARS=(HAVE_CHD=0)
      IOS_DISPLAY="TurboGrafx-16 / PC Engine, software renderer"
      ;;
    stella2023)
      IOS_REPO="https://github.com/libretro/stella2023"
      IOS_PIN="ba52c43b9eda950eb0c0eec69cda9b17dee8c39b"
      IOS_DYLIB_NAME="stella2023_libretro_ios.dylib"
      IOS_KIND="make"
      IOS_MAKEFILE="Makefile"
      # Its libretro makefile is NOT at the repository root: the root Makefile is Stella's own
      # SDL build, with no libretro target and no ios platform at all. Building from the root
      # would fail in a way that looks like the core being unbuildable for iOS when it is not.
      IOS_MAKE_SUBDIR="src/os/libretro"
      # The maintained modern Stella rather than the 2014 fork that is also on the buildbot.
      # Both have a working ios-arm64 block; this one is the current upstream.
      #
      # need_fullpath is FALSE here, and it is the FIRST core in this project for which that
      # matters. Stella memcpys straight from retro_game_info::data with no path fallback, so a
      # frontend that hands over a path and no bytes gives it a zero-byte ROM. The host did
      # exactly that for every core until this one; see the need_fullpath handling in
      # `NativeLibretroCore::load_content`, which reads the file when a core asks for bytes.
      IOS_DISPLAY="Atari 2600, software renderer"
      ;;
    mednafen_psx_hw)
      IOS_REPO="https://github.com/libretro/beetle-psx-libretro"
      IOS_PIN="5ec9909f2654fb2041315a13fac0b704c5065c0e"
      IOS_DYLIB_NAME="mednafen_psx_hw_libretro_ios.dylib"
      IOS_KIND="make"
      IOS_MAKEFILE="Makefile"
      # HAVE_HW=1 → Vulkan + OpenGL, target mednafen_psx_hw. In IOS_CORES / ios-all.
      # Mac or GitHub Actions macos-latest only; see ios_require_darwin.
      IOS_MAKE_VARS=(HAVE_HW=1)
      IOS_DISPLAY="PlayStation, Beetle PSX HW (Vulkan) — step 4 proof core"
      ;;
    parallel_n64)
      IOS_REPO="https://github.com/libretro/parallel-n64"
      IOS_PIN="0bd516ee793bb87b57e9eef19993b37ac0098d74"
      IOS_DYLIB_NAME="parallel_n64_libretro_ios.dylib"
      IOS_KIND="make"
      IOS_MAKEFILE="Makefile"
      # Submodules, and they are needed: the core vendors mupen64plus and its plugins as
      # submodules rather than in-tree.
      IOS_SUBMODULES=1
      # THE N64 NEEDS NEITHER OF THE TWO THINGS EVERYONE ASSUMES IT NEEDS, and reading this
      # core's own ios block is what establishes that:
      #
      #     HAVE_OPENGL=0     software rasteriser, so it emits pixels through the same path
      #                       the other eight cores already use. No MoltenVK, no ANGLE, none
      #                       of steps 3 to 6 of docs/SET_HW_RENDER_DESIGN.md.
      #     WITH_DYNAREC=     interpreter, so it needs NO executable memory and therefore no
      #                       JIT, no get-task-allow and no debugger attached.
      #
      # It will be SLOW. A software rasteriser plus an interpreter is slow on any phone, and
      # that is the honest expectation rather than a defect to chase. It runs, which is the
      # thing that was in doubt.
      #
      # `parallel_n64` and not `mupen64plus_next`: both are on the libretro buildbot for
      # ios-arm64, but Mupen64Plus-Next renders through GLideN64 and has no software path, so
      # it cannot work until the graphics work is done. See docs/PLATFORM_LIMITS.md.
      #
      # Continuum patches (scripts/patches/, applied by ios_apply_core_patches after clone):
      # 1) parallel_n64-aarch64-gate-hot-state-on-new-dynarec.patch (critical):
      #    iOS builds WITH_DYNAREC= so NEW_DYNAREC is unset, but r4300.h aarch64 still
      #    aliased mupencorestop to new_dynarec_hot_state.stop. main_run only clears that
      #    under #ifdef NEW_DYNAREC, so the first VI latches stop and retro_return early-outs
      #    forever (stuck "N64 first tick", frames 0). Gate hot-state aliases on NEW_DYNAREC.
      # 2) parallel_n64-reapply-variables-after-initiate-gfx.patch (kept; device FAIL on 96
      #    ruled it out as the hang cause, but still correct for angrylion multithread-off):
      #    reapply update_variables(false) after InitiateGFX / n64video_config_init.
      IOS_DISPLAY="Nintendo 64, software rasteriser and interpreter"
      ;;
    parallel_n64_jit)
      # The N64 recompiler (new_dynarec, aarch64), as the core's own macOS arm64 block builds it,
      # on the iOS platform block. Command-line variables replace the ios block's no-recompiler
      # flags: WITH_DYNAREC on, PLATCFLAGS and CPUFLAGS without NO_ASM (which compiles the
      # recompiler's setup out) and without the -D__arm__ the interpreter build carries, and the
      # assembler wrapper the macOS arm64 block uses for linkage_arm64.S. @IOSSDK@ is filled in at
      # build time. Patched for iOS: pages switched with mprotect instead of the macOS-only
      # pthread_jit_write_protect_np (scripts/patches/parallel_n64-ios-jit.patch).
      IOS_REPO="https://github.com/libretro/parallel-n64"
      IOS_PIN="0bd516ee793bb87b57e9eef19993b37ac0098d74"
      IOS_DYLIB_NAME="parallel_n64_jit_libretro_ios.dylib"
      IOS_KIND="make"
      IOS_MAKEFILE="Makefile"
      IOS_SUBMODULES=1
      IOS_MAKE_VARS=(
        WITH_DYNAREC=aarch64
        "PLATCFLAGS=-DHAVE_POSIX_MEMALIGN -DIOS -miphoneos-version-min=8.0"
        "CPUFLAGS=-D__NEON_OPT -DARM_FIX"
        "CC_AS=perl ./tools/gas-preprocessor-new.pl -arch arm64 -- clang -arch arm64 -isysroot @IOSSDK@"
      )
      IOS_DISPLAY="Nintendo 64, software rasteriser and recompiler (JIT build)"
      ;;
    azahar)
      # Azahar is the maintained Citra fork. Its libretro target builds for iOS
      # (their own libretro-ios job). Apple builds turn OpenGL OFF and Vulkan ON,
      # and the core presents with retro_vulkan_image set_image — the only hardware
      # picture hook this host accepts. CPU JIT is compiled out by -DIOS (no
      # dynarec, no executable memory). The fast interpreter stays on.
      IOS_REPO="https://github.com/azahar-emu/azahar"
      IOS_PIN="065c9222ae7e8ce5535aa2672c8565e82c0a6d54"
      IOS_DYLIB_NAME="azahar_libretro_ios.dylib"
      IOS_KIND="cmake-shared"
      IOS_CMAKE_TARGET="citra_libretro"
      IOS_SUBMODULES=1
      # Continuum patch (scripts/patches/azahar-do-not-wait-on-pipeline-compile.patch):
      # RasterizerVulkan waits on every new pipeline because async_shader_compilation
      # defaults off (wait_built is then always true) and BindPipeline calls WaitDone.
      # That is the post-transition hitch. The patch skips the draw instead of waiting.
      # No JIT, no dynarec, no executable memory.
      IOS_DISPLAY="Nintendo 3DS, Vulkan, interpreter CPU (no JIT)"
      ;;
    ppsspp)
      # Official libretro iOS job (ppsspp .gitlab-ci.yml): cmake with
      # cmake/Toolchains/ios.cmake and -DLIBRETRO=ON. The Makefile's ios-arm64
      # block is not that job. It forces -DARMv5_ONLY -DARM and -marm even for
      # arm64, which is a 32-bit compile of a 64-bit core. Do not use it.
      #
      # CPU is the IR interpreter, not the dynarec. The host answers
      # ppsspp_cpu_core with the core's own value "IR JIT", which libretro.cpp
      # stores as CPUCore::IR_INTERPRETER and constructs as IRJit(state, false).
      # That false is compile-to-native off: no PROT_EXEC, no dynarec. The
      # dynarec value is the separate string "JIT". On iOS the core also asks
      # RETRO_ENVIRONMENT_GET_JIT_CAPABLE, which this host does not implement,
      # so a later switch to "JIT" is forced back to the IR interpreter.
      # Vertex-decoder JIT uses the same flag and stays off.
      #
      # Picture is Vulkan. The toolchain builds with GLES2, the host's preferred
      # hardware context is Vulkan, and CreateGraphicsContext tries Vulkan for
      # that answer. Frames are the same set_image path Azahar uses. No BIOS.
      #
      # USE_FFMPEG=OFF so the ffmpeg submodule (prebuilt blobs for every
      # platform) is not cloned. PMF video in a game will not decode. The
      # selected submodule list is what the libretro target's CMake actually
      # add_subdirectory's.
      IOS_REPO="https://github.com/hrydgard/ppsspp"
      IOS_PIN="53fae900997fd12ac0ef8f8d82d6f25390acbc29"
      IOS_DYLIB_NAME="ppsspp_libretro_ios.dylib"
      IOS_KIND="cmake-ppsspp"
      IOS_CMAKE_TARGET="ppsspp_libretro"
      IOS_SUBMODULES=1
      # ext/cpu_features is required. ext/cmake/cpu_features configure_file's
      # ext/cpu_features/cmake/CpuFeaturesConfig.cmake.in. It is not vendored.
      # Common/VR/OpenXRLoader.cpp is compiled into the core even when the
      # OpenXR loader is off, and it includes openxr/openxr.h. CMake always
      # adds ext/OpenXR-SDK/include. Those headers are this submodule.
      # Core/Util/PortManager.h includes ext/miniupnp headers even when
      # USE_MINIUPNPC is off. The calls are ifdef'd; the include is not.
      IOS_SUBMODULE_PATHS=(
        libretro/libretro-common
        ext/armips
        ext/glslang
        ext/SPIRV-Cross
        ext/libchdr
        ext/zstd
        ext/lua
        ext/rcheevos
        ext/aemu_postoffice
        ext/rapidjson
        ext/cpu_features
        ext/OpenXR-SDK
        ext/miniupnp
      )
      IOS_DISPLAY="PSP, Vulkan, IR interpreter (no JIT, no dynarec)"
      ;;
    flycast)
      # Dreamcast. NOT on the libretro iOS buildbot, so it is built here, and it is OPTIONAL
      # (IOS_OPTIONAL_CORES): a failure leaves the .ipa without it rather than failing the job.
      #
      # Read from flycast's own CMakeLists.txt and core/build.h:
      #   - LIBRETRO=ON makes `flycast_libretro` a SHARED library exported through
      #     shell/libretro/libretro.osx.def, so the dylib is upstream's and is only renamed.
      #   - With CMAKE_SYSTEM_NAME=iOS, CMake sets IOS, and the libretro branch then compiles
      #     GLES3 and links OpenGLES. USE_VULKAN stays ON, so the core can take the Vulkan context
      #     this host prefers, the same set_image path Azahar and PPSSPP use.
      #   - core/build.h defines FEAT_SHREC, FEAT_AREC and FEAT_DSPREC as DYNAREC_NONE when
      #     TARGET_NO_REC is defined. That is the interpreter for the SH4, the ARM7 and the DSP:
      #     no executable memory at all. It is passed in the C and C++ flags because build.h only
      #     sets it by itself for the simulator.
      #   - Upstream's own libretro iOS job passes -DUSE_OPENMP=OFF; kept. Lua, breakpad and
      #     discord are off because none of them is used by a libretro core.
      # Submodules: only the ones the libretro target add_subdirectory's or includes. SDL, oboe,
      # googletest, freetype, breakpad and the Windows-only ones are not fetched.
      IOS_REPO="https://github.com/flyinghead/flycast"
      IOS_PIN="59ed35a7ea7c1940d4c8ac221a662d0e6d6dc9ea"
      IOS_DYLIB_NAME="flycast_libretro_ios.dylib"
      IOS_KIND="cmake-flycast"
      IOS_CMAKE_TARGET="flycast_libretro"
      IOS_SUBMODULES=1
      IOS_SUBMODULE_PATHS=(
        core/deps/libchdr
        core/deps/Vulkan-Headers
        core/deps/VulkanMemoryAllocator
        core/deps/glslang
        core/deps/rcheevos
        core/deps/asio
        core/deps/libjuice
        core/deps/websocketpp
        core/deps/tinygettext
        core/deps/luabridge
      )
      IOS_DISPLAY="Dreamcast, interpreter only (TARGET_NO_REC), optional"
      ;;
    flycast_jit)
      # Dreamcast with flycast's recompilers (SH4, ARM7, DSP): the same build without
      # TARGET_NO_REC. On iPhone flycast already switches its code pages between writable and
      # runnable (TARGET_IPHONE in rec_arm64.cpp and posix_vmem.cpp), which is how its own iOS app
      # runs with JIT, so this needs no patch.
      IOS_REPO="https://github.com/flyinghead/flycast"
      IOS_PIN="59ed35a7ea7c1940d4c8ac221a662d0e6d6dc9ea"
      IOS_DYLIB_NAME="flycast_jit_libretro_ios.dylib"
      IOS_KIND="cmake-flycast"
      IOS_CMAKE_TARGET="flycast_libretro"
      IOS_SUBMODULES=1
      IOS_SUBMODULE_PATHS=(
        core/deps/libchdr
        core/deps/Vulkan-Headers
        core/deps/VulkanMemoryAllocator
        core/deps/glslang
        core/deps/rcheevos
        core/deps/asio
        core/deps/libjuice
        core/deps/websocketpp
        core/deps/tinygettext
        core/deps/luabridge
      )
      IOS_FLYCAST_DEFINES="-DIOS"
      IOS_DISPLAY="Dreamcast, recompilers (JIT build), optional"
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

# Every core's repository and pinned commit, for the weekly upstream update check
# (scripts/check-core-updates.sh). Read from ios_core_config like everything else, so the
# check can never drift from what is actually built.
#
# Tab separated because IOS_DISPLAY contains spaces and commas.
ios_print_pins() {
  local core
  for core in "${IOS_CORES[@]}" "${IOS_OPTIONAL_CORES[@]}"; do
    ios_core_config "$core"
    # A core with no pin cannot be compared against upstream. There are none today; this is
    # so adding one later is skipped rather than reported as a bogus "0 commits behind".
    [[ -n "$IOS_REPO" && -n "$IOS_PIN" ]] || continue
    printf '%s\t%s\t%s\t%s\n' "$core" "$IOS_REPO" "$IOS_PIN" "$IOS_DISPLAY"
  done
}

# The optional from-source cores, same shape as ios_print_names. Separate so the required list
# keeps meaning "the build fails without this".
ios_print_optional_names() {
  local core
  for core in "${IOS_OPTIONAL_CORES[@]}"; do
    ios_core_config "$core"
    echo "$IOS_DYLIB_NAME"
  done
}

# Mach-O tools. Apple's on a Mac; LLVM's spellings of the same tools anywhere else, which is what
# lets `ios-stage-prebuilt` check and fix a downloaded dylib on a Linux host too. Same output
# format for the two flags used here (`nm -gU`, `otool -D`).
ios_tool() {
  local name="$1"
  if command -v "$name" >/dev/null 2>&1 && [[ "$(uname -s)" == "Darwin" ]]; then
    echo "$name"
    return
  fi
  case "$name" in
    nm) command -v llvm-nm >/dev/null 2>&1 && { echo llvm-nm; return; } ;;
    otool) command -v llvm-otool >/dev/null 2>&1 && { echo llvm-otool; return; } ;;
    install_name_tool)
      command -v llvm-install-name-tool >/dev/null 2>&1 && { echo llvm-install-name-tool; return; } ;;
  esac
  echo "$name"
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
  ios-optional-names
                the same for the optional from-source cores (${IOS_OPTIONAL_CORES[*]}),
                which may be missing from a successful build.
  ios-stage-prebuilt <dylib> <name>
                check a dylib this script did not build (all libretro entry points),
                set its @rpath install name and stage it. Any host.

'ios' and 'ios-all' need a macOS host with the Xcode command line tools.

Each core is built from its pinned commit (IOS_PIN). CONTINUUM_UNPINNED=1 builds
every core at its default branch's HEAD instead, to try newer cores on purpose.
EOF
}

ios_require_darwin() {
  if [[ "$(uname -s)" != "Darwin" ]]; then
    cat >&2 <<'EOF'
error: the iOS cores build only on a macOS host.

  They cross-compile real libretro cores with Apple's clang and the iphoneos SDK:
      make -f <makefile> platform=ios-arm64 IOSSDK=$(xcrun --sdk iphoneos --show-sdk-path)
  and, for mgba, a CMake configure for CMAKE_SYSTEM_NAME=iOS plus a dylib link. There is no
  non-Mac fallback for any of it. Build them on the macOS runner through the ios workflow
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

# PINNED SOURCES. Every from-source core is built from one exact upstream commit, IOS_PIN in
# ios_core_config, and not from whatever its default branch says on the day CI runs.
#
# The pins are the commits build 121 was built from, the last build known good on a phone. This
# project has one compiler, a macOS runner that takes ~30 minutes, and an owner who gets no app at
# all when it goes red, so an upstream commit landing overnight must not be able to change or break
# a build nobody touched. It also keeps the Continuum patches in scripts/patches/ applying: they
# were written against these trees, and a moved hunk anchor fails the core.
#
# CONTINUUM_UNPINNED=1 in the environment builds every core at its default branch's HEAD instead,
# exactly the old behaviour. It is for deliberately trying newer cores, never for a normal build.
#
# MOVING A PIN, one core at a time:
#   1. run a build with CONTINUUM_UNPINNED=1 (locally, or for one run in the build-engine step's
#      env in .github/workflows/ios.yml);
#   2. check that build on a phone;
#   3. take the core's commit from that build's core-sources.txt (third field of its line, in the
#      ios-build-metadata artefact) and paste it into the core's IOS_PIN above;
#   4. dry-run its patches against that commit (patch -p1 --dry-run --forward), if it has any.
# core-sources.txt always records the commit really built, pinned or not, with a fourth field
# saying which.
ios_unpinned() {
  [[ "${CONTINUUM_UNPINNED:-0}" == "1" ]]
}

# Fetches one commit (a sha, or HEAD) from origin into FETCH_HEAD, retrying a flaky network.
# Shallow cores fetch it at depth 1; GitHub serves any reachable commit by sha, so this is the
# whole download for them. Full-depth cores already hold their history and fetch without --depth,
# because deepening a full clone with --depth would make it shallow under the submodule update.
ios_fetch_commit() {
  local full_depth="$1" ref="$2"
  local attempt
  for attempt in 1 2 3; do
    if [[ "$full_depth" == "1" ]]; then
      git -C "$IOS_SRC_DIR" fetch origin "$ref" && return 0
    else
      git -C "$IOS_SRC_DIR" fetch --depth 1 origin "$ref" && return 0
    fi
    echo "==> fetching $ref failed (attempt $attempt of 3)" >&2
    [[ "$attempt" == "3" ]] || sleep 5
  done
  return 1
}

# Puts the checkout at the commit this build uses, BEFORE any submodule update, so the submodules
# initialised afterwards (all of them, or ppsspp's and flycast's selected lists) are the ones that
# commit names rather than the ones the default branch names today.
ios_checkout_source() {
  local core="$1" full_depth="$2"
  local want
  if ios_unpinned; then
    echo "==> $core: CONTINUUM_UNPINNED=1, building the default branch's HEAD, not the pin $IOS_PIN"
    ios_fetch_commit "$full_depth" HEAD || {
      echo "error: $core: could not fetch the default branch's HEAD from $IOS_REPO" >&2
      exit 1
    }
    want="$(git -C "$IOS_SRC_DIR" rev-parse FETCH_HEAD)"
  else
    want="$IOS_PIN"
    [[ -n "$want" ]] || {
      echo "error: $core has no IOS_PIN in ios_core_config (CONTINUUM_UNPINNED=1 builds HEAD)" >&2
      exit 1
    }
    if ! git -C "$IOS_SRC_DIR" cat-file -e "$want^{commit}" 2>/dev/null; then
      echo "==> $core: fetching pinned commit $want"
      ios_fetch_commit "$full_depth" "$want" || {
        echo "error: $core: could not fetch the pinned commit $want from $IOS_REPO." >&2
        echo "       If upstream rewrote its history, move the pin (see PINNED SOURCES in" >&2
        echo "       scripts/build-core.sh); CONTINUUM_UNPINNED=1 builds HEAD meanwhile." >&2
        exit 1
      }
    fi
  fi
  local head
  head="$(git -C "$IOS_SRC_DIR" rev-parse -q --verify 'HEAD^{commit}' 2>/dev/null || true)"
  if [[ "$head" != "$want" ]]; then
    # --force only matters for a reused local .work/ios/<core> already patched at another commit:
    # it resets the tracked files, and ios_apply_core_patches puts the patches back. A fresh CI
    # checkout has nothing to discard.
    git -C "$IOS_SRC_DIR" -c advice.detachedHead=false checkout --quiet --force --detach "$want"
  fi
  head="$(git -C "$IOS_SRC_DIR" rev-parse HEAD)"
  [[ "$head" == "$want" ]] || {
    echo "error: $core: checkout is at $head, expected $want" >&2
    exit 1
  }
}

ios_clone() {
  local core="$1"
  IOS_SRC_DIR="$IOS_WORK/$core"
  mkdir -p "$IOS_WORK"
  # No --depth when every submodule is initialised. A shallow parent and a recursive submodule
  # update fight each other, and several cores need the whole tree (pcsx_rearmed, parallel_n64,
  # azahar). Depth 1 is fine when there are no submodules, and when IOS_SUBMODULE_PATHS names
  # exactly which ones to fetch: those gitlinks are in the checked-out commit.
  local full_depth=0
  if [[ "$IOS_SUBMODULES" == "1" && ${#IOS_SUBMODULE_PATHS[@]} -eq 0 ]]; then
    full_depth=1
  fi
  if [[ ! -d "$IOS_SRC_DIR/.git" ]]; then
    echo "==> cloning $core for iOS"
    if [[ "$full_depth" == "1" ]]; then
      git clone "$IOS_REPO" "$IOS_SRC_DIR"
    elif ios_unpinned; then
      git clone --depth 1 "$IOS_REPO" "$IOS_SRC_DIR"
    else
      # Pinned and shallow: an empty repository, then ios_checkout_source fetches exactly the
      # pinned commit at depth 1, rather than cloning HEAD only to replace it.
      git init --quiet "$IOS_SRC_DIR"
      git -C "$IOS_SRC_DIR" remote add origin "$IOS_REPO"
    fi
  fi
  ios_checkout_source "$core" "$full_depth"
  if [[ "$IOS_SUBMODULES" == "1" ]]; then
    if (( ${#IOS_SUBMODULE_PATHS[@]} > 0 )); then
      echo "==> initialising selected submodules for $core: ${IOS_SUBMODULE_PATHS[*]}"
      ( cd "$IOS_SRC_DIR" && git submodule update --init --recursive "${IOS_SUBMODULE_PATHS[@]}" )
    else
      echo "==> initialising submodules for $core"
      ( cd "$IOS_SRC_DIR" && git submodule update --init --recursive )
    fi
  fi
  ios_record_source_version "$core"
}

# Where every core's source came from, written down: "<core> <repo> <commit> pinned|unpinned".
#
# The commit is read back from the checkout (the one really built), not copied from IOS_PIN, so
# the record stays true under CONTINUUM_UNPINNED=1, and it is where a new pin is taken from (see
# PINNED SOURCES above ios_clone). Before the pins, cores were built at whatever HEAD was on the
# day, and this file was the only way to say what moved when a core that worked last week broke.
#
# It is not hypothetical for the DS in particular. Two melonDS option VALUES are hardcoded in
# `option_overrides` and matched by `strcmp` inside the core, so an upstream rename turns the DS
# touch screen off again with no error on either side. The pin prevents that from happening by
# itself; this makes a deliberate move attributable to a commit rather than to a mystery.
#
# Written to a file as well as the log because the log ages out of the Actions UI while this ships
# in the build metadata artefact next to the entitlements and the generated Swift.
ios_record_source_version() {
  local core="$1"
  local sha state="pinned"
  sha="$(git -C "$IOS_SRC_DIR" rev-parse HEAD 2>/dev/null || echo unknown)"
  [[ "$sha" == "$IOS_PIN" ]] || state="unpinned"
  echo "==> $core source: $IOS_REPO @ $sha ($state)"
  mkdir -p "$IOS_OUT_DIR"
  local manifest="$IOS_OUT_DIR/core-sources.txt"
  # Rewritten per core rather than appended blindly, so a rebuild of one core updates its line
  # instead of adding a second one that disagrees with the first.
  if [[ -f "$manifest" ]]; then
    grep -v "^$core " "$manifest" > "$manifest.tmp" 2>/dev/null || true
    mv "$manifest.tmp" "$manifest"
  fi
  echo "$core $IOS_REPO $sha $state" >> "$manifest"
  LC_ALL=C sort -o "$manifest" "$manifest"
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
  #
  # ALL of it, not just retro_run. Checking one symbol only catches a core that resolved
  # nothing at all; it says nothing about a core that lost one entry point, which is the more
  # likely and much quieter failure. mgba is built with LTO (see build_ios_cmake_core), and LTO
  # is free to internalise any symbol the link does not reference, so "some of the API survived"
  # is a state this build can actually produce.
  local exported
  exported="$("$(ios_tool nm)" -gU "$built" 2>/dev/null | awk '{ print $NF }')"
  local missing=()
  local symbol
  for symbol in "${IOS_REQUIRED_SYMBOLS[@]}"; do
    # Mach-O prefixes C symbols with an underscore, and the match is anchored so that
    # retro_serialize cannot be satisfied by retro_serialize_size.
    grep -qx "_$symbol" <<<"$exported" || missing+=("$symbol")
  done
  if (( ${#missing[@]} > 0 )); then
    echo "error: $built is missing ${#missing[@]} libretro entry point(s):" >&2
    printf '         %s\n' "${missing[@]}" >&2
    echo "       (a dylib that links but exports an incomplete API looks fine to every" >&2
    echo "        later check and fails only on device, when the engine resolves them)" >&2
    exit 1
  fi

  local install_name
  install_name="$("$(ios_tool otool)" -D "$built" 2>/dev/null | tail -n +2 | head -1 || true)"
  if [[ "$install_name" != "@rpath/$canonical" ]]; then
    echo "==> setting install_name to @rpath/$canonical (was: ${install_name:-none})"
    "$(ios_tool install_name_tool)" -id "@rpath/$canonical" "$built"
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
  # `${arr[@]+"${arr[@]}"}` rather than plain `"${arr[@]}"`, because macOS ships bash 3.2 and an
  # EMPTY array expanded under `set -u` is an error there rather than nothing. Seven of the eight
  # cores pass no extra variables, so the empty case is the common one. See the note at the top of
  # this file about IOS_CORES for the same hazard.
  if (( ${#IOS_MAKE_VARS[@]} > 0 )); then
    # @IOSSDK@ is the SDK path, which is only known now (ios_core_config runs before the SDK is
    # resolved). Only the N64 JIT build's assembler wrapper uses it.
    local i
    for i in "${!IOS_MAKE_VARS[@]}"; do
      IOS_MAKE_VARS[$i]="${IOS_MAKE_VARS[$i]//@IOSSDK@/$IOSSDK}"
    done
    echo "==> extra make variables: ${IOS_MAKE_VARS[*]}"
  fi
  ( cd "$make_dir" && make -f "$IOS_MAKEFILE" platform=ios-arm64 IOSSDK="$IOSSDK" \
      ${IOS_MAKE_VARS[@]+"${IOS_MAKE_VARS[@]}"} -j"$(ios_jobs)" )

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
  # Everything optional is off: no zlib, png, sqlite, ffmpeg, zip, lzma, ELF loading,
  # scripting or debuggers. LIBRETRO_STATIC=ON asks for the archive rather than a shared
  # library, because the link is done below where the iOS flags are ours to set. It does NOT rename any retro_* symbol; mgba's CMakeLists uses it only to
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
  #
  # -u for every entry point, and THIS IS THE FIX FOR THE INTERMITTENT mgba FAILURE that
  # produced a core with no libretro API in it on one run and a good one on the next, from the
  # same commit and the same Xcode.
  #
  # -force_load was necessary but not sufficient, because this archive holds LLVM bitcode
  # rather than native objects. mgba's CMakeLists adds -flto unconditionally on Apple:
  #
  #   if(APPLE OR CMAKE_C_COMPILER_ID STREQUAL "GNU" AND BUILD_LTO)
  #
  # which CMake parses as `APPLE OR (GNU AND BUILD_LTO)`, so -DBUILD_LTO=OFF cannot turn it
  # off. -force_load therefore guarantees the archive MEMBERS are loaded, and then LTO decides
  # what to emit from them. Nothing in the link referenced the libretro API, so LTO was entitled
  # to internalise and dead-strip it, and whether it did came down to how its parallel codegen
  # happened to partition the module. That is the nondeterminism.
  #
  # -u names each symbol as an undefined root the link must satisfy, which marks it live before
  # LTO runs and leaves nothing to chance. Preferred over -exported_symbols_list, which would
  # also work: that hides every other symbol, and hiding one the core needs is a failure this
  # script could not see, whereas an over-long -u list fails loudly at link time.
  local keep_alive=()
  local symbol
  for symbol in "${IOS_REQUIRED_SYMBOLS[@]}"; do
    keep_alive+=(-Wl,-u,"_$symbol")
  done
  cc -arch arm64 -isysroot "$IOSSDK" -miphoneos-version-min="$IOS_MIN_VERSION" \
    -dynamiclib -install_name "@rpath/$IOS_DYLIB_NAME" \
    -o "$built" -Wl,-force_load,"$archive" \
    "${keep_alive[@]}" \
    -framework Foundation -lm
  [[ -f "$built" ]] || {
    echo "error: $core: the dylib link reported success but produced nothing" >&2
    exit 1
  }

  ios_stage_dylib "$built" "$IOS_DYLIB_NAME"
}


# A libretro core whose own CMake emits a shared library, not an archive we relink.
#
# mgba is the other cmake core and it is NOT this path: it asks for a static archive
# and this script links the dylib, because that is how mgba's iOS build is shaped.
# Azahar's libretro target already emits azahar_libretro.dylib. Relinking that the
# mgba way would drop the Vulkan driver and every other library the core linked.
build_ios_cmake_shared_core() {
  local core="$1"
  command -v cmake >/dev/null 2>&1 || {
    echo "error: $core needs cmake on the host (brew install cmake)" >&2
    exit 1
  }

  local build_dir="$IOS_SRC_DIR/build-ios"
  echo "==> configuring $core with cmake for iOS ($IOS_DISPLAY)"
  rm -rf "$build_dir"
  # Flags follow Azahar's own libretro-ios job, plus this app's deployment target
  # and SDK. -DIOS is what their sources test to compile the CPU JIT out. OpenGL
  # stays off because Azahar's CMake forces ENABLE_OPENGL off on Apple; Vulkan
  # stays on. Warnings-as-errors is off so a newer AppleClang diagnostic cannot
  # fail a core whose own CI is green on a different Xcode. LTO is off so the
  # libretro entry points cannot be internalised the way mgba's were.
  cmake -G "Unix Makefiles" -S "$IOS_SRC_DIR" -B "$build_dir" \
    -DENABLE_LIBRETRO=ON \
    -DIOS=ON \
    -DCMAKE_SYSTEM_NAME=iOS \
    -DCMAKE_OSX_ARCHITECTURES=arm64 \
    -DCMAKE_OSX_SYSROOT="$IOSSDK" \
    -DCMAKE_OSX_DEPLOYMENT_TARGET="$IOS_MIN_VERSION" \
    -DCMAKE_BUILD_TYPE=Release \
    -DCMAKE_POSITION_INDEPENDENT_CODE=ON \
    -DCMAKE_C_FLAGS="-DIOS" \
    -DCMAKE_CXX_FLAGS="-DIOS" \
    -DCITRA_USE_PRECOMPILED_HEADERS=OFF \
    -DCITRA_WARNINGS_AS_ERRORS=OFF \
    -DENABLE_LTO=OFF \
    -DENABLE_OPT=OFF

  echo "==> building $IOS_CMAKE_TARGET"
  cmake --build "$build_dir" --target "$IOS_CMAKE_TARGET" -j"$(ios_jobs)"

  local built
  built="$(find "$build_dir" -name 'azahar_libretro*.dylib' -type f | head -1 || true)"
  [[ -n "$built" && -f "$built" ]] || {
    echo "error: $core: cmake finished but left no azahar_libretro dylib under $build_dir" >&2
    find "$build_dir" -name '*.dylib' -type f >&2 || true
    exit 1
  }
  echo "==> upstream produced $(basename "$built"); staging as $IOS_DYLIB_NAME"
  # Stage from a copy so install_name_tool does not rewrite the build tree's
  # output, and so a name that is not the canonical one still lands under it.
  local staged="$build_dir/$IOS_DYLIB_NAME"
  cp "$built" "$staged"
  ios_stage_dylib "$staged" "$IOS_DYLIB_NAME"
}

# PPSSPP's own libretro iOS CI: the ios toolchain plus -DLIBRETRO=ON.
# Not the azahar cmake function. That one passes -DIOS and Citra flags, and
# finding the dylib is hardcoded to azahar_libretro.
build_ios_ppsspp_core() {
  local core="$1"
  command -v cmake >/dev/null 2>&1 || {
    echo "error: $core needs cmake on the host (brew install cmake)" >&2
    exit 1
  }

  local build_dir="$IOS_SRC_DIR/build-ios"
  echo "==> configuring $core with cmake for iOS ($IOS_DISPLAY)"
  rm -rf "$build_dir"
  # The toolchain sets IOS, arm64, GLES2, and the iphoneos SDK. Passing
  # CMAKE_SYSTEM_NAME=iOS ourselves would fight that file: PPSSPP's iOS
  # detection is the toolchain's IOS variable, and its libretro job does not
  # pass CMAKE_SYSTEM_NAME.
  #
  # USE_FFMPEG=OFF: no ffmpeg submodule, so in-game PMF video does not decode.
  # USE_DISCORD off: libretro does not link discord-rpc. USE_MINIUPNPC
  # off skips building miniupnpc, but PortManager.h still includes it, so
  # ext/miniupnp is cloned above.
  # HEADLESS, the unit tests, and the atlas tool are extra binaries this dylib
  # does not need. LTO is not requested; the libretro entry points are kept by
  # libretro/libretro.osx.def, which the core's own CMake passes as
  # -exported_symbols_list.
  cmake -G "Unix Makefiles" -S "$IOS_SRC_DIR" -B "$build_dir" \
    -DCMAKE_TOOLCHAIN_FILE="$IOS_SRC_DIR/cmake/Toolchains/ios.cmake" \
    -DLIBRETRO=ON \
    -DUSE_FFMPEG=OFF \
    -DUSE_DISCORD=OFF \
    -DUSE_MINIUPNPC=OFF \
    -DHEADLESS=OFF \
    -DUNITTEST=OFF \
    -DATLAS_TOOL=OFF \
    -DCMAKE_BUILD_TYPE=Release \
    -DCMAKE_OSX_DEPLOYMENT_TARGET="$IOS_MIN_VERSION"

  echo "==> building $IOS_CMAKE_TARGET"
  cmake --build "$build_dir" --target "$IOS_CMAKE_TARGET" -j"$(ios_jobs)"

  local built
  built="$(find "$build_dir" -name 'ppsspp_libretro*.dylib' -type f | head -1 || true)"
  [[ -n "$built" && -f "$built" ]] || {
    echo "error: $core: cmake finished but left no ppsspp_libretro dylib under $build_dir" >&2
    find "$build_dir" -name '*.dylib' -type f >&2 || true
    exit 1
  }
  echo "==> upstream produced $(basename "$built"); staging as $IOS_DYLIB_NAME"
  local staged="$build_dir/$IOS_DYLIB_NAME"
  cp "$built" "$staged"
  ios_stage_dylib "$staged" "$IOS_DYLIB_NAME"
}

# flycast's libretro target, which its CMake emits as a shared library.
#
# -DIOS in the C/C++ flags: CMAKE_SYSTEM_NAME=iOS sets the CMake variable IOS, but flycast only turns
# that into TARGET_IPHONE. libretro-common's glsym/rglgen_headers.h tests the C macro IOS, and
# without it falls to the __APPLE__ branch and includes macOS-only <OpenGL/gl3.h> (build 118:
# "fatal error: 'OpenGL/gl3.h' file not found"). libretro's own ios-cmake CI template passes -DIOS.
#
# Its own path because the flags are its own: TARGET_NO_REC for the interpreter (core/build.h),
# USE_OPENMP off as upstream's libretro iOS job has it, and the bundled libzip because there is no
# host libzip in the iphoneos SDK. LTO is not requested, and the exports come from
# shell/libretro/libretro.osx.def, which flycast's CMake passes as -exported_symbols_list.
build_ios_flycast_core() {
  local core="$1"
  command -v cmake >/dev/null 2>&1 || {
    echo "error: $core needs cmake on the host (brew install cmake)" >&2
    exit 1
  }

  local build_dir="$IOS_SRC_DIR/build-ios"
  echo "==> configuring $core with cmake for iOS ($IOS_DISPLAY)"
  rm -rf "$build_dir"
  cmake -G "Unix Makefiles" -S "$IOS_SRC_DIR" -B "$build_dir" \
    -DLIBRETRO=ON \
    -DCMAKE_SYSTEM_NAME=iOS \
    -DCMAKE_OSX_ARCHITECTURES=arm64 \
    -DCMAKE_OSX_SYSROOT="$IOSSDK" \
    -DCMAKE_OSX_DEPLOYMENT_TARGET="$IOS_MIN_VERSION" \
    -DCMAKE_BUILD_TYPE=Release \
    -DCMAKE_C_FLAGS="$IOS_FLYCAST_DEFINES" \
    -DCMAKE_CXX_FLAGS="$IOS_FLYCAST_DEFINES" \
    -DUSE_OPENMP=OFF \
    -DUSE_LUA=OFF \
    -DUSE_BREAKPAD=OFF \
    -DUSE_DISCORD=OFF \
    -DUSE_HOST_LIBZIP=OFF \
    -DUSE_HOST_LIBCHDR=OFF \
    -DENABLE_CTEST=OFF

  echo "==> building $IOS_CMAKE_TARGET"
  cmake --build "$build_dir" --target "$IOS_CMAKE_TARGET" -j"$(ios_jobs)"

  local built
  built="$(find "$build_dir" -name 'flycast_libretro*.dylib' -type f | head -1 || true)"
  [[ -n "$built" && -f "$built" ]] || {
    echo "error: $core: cmake finished but left no flycast_libretro dylib under $build_dir" >&2
    find "$build_dir" -name '*.dylib' -type f >&2 || true
    exit 1
  }
  echo "==> upstream produced $(basename "$built"); staging as $IOS_DYLIB_NAME"
  local staged="$build_dir/$IOS_DYLIB_NAME"
  cp "$built" "$staged"
  ios_stage_dylib "$staged" "$IOS_DYLIB_NAME"
}

# Continuum-owned edits to the upstream core checkouts.
#
# Cores are built at their IOS_PIN (see PINNED SOURCES above ios_clone), and these patches were
# checked against those commits. When upstream behaviour and Continuum's host disagree in a way
# that cannot be fixed from the frontend alone, the fix lives here as a patch applied after clone
# and before make, so the next ios-all / IPA rebuild picks it up without waiting on an upstream
# merge. Moving a patched core's pin means dry-running its patch against the new commit first.
#
# Idempotent: a re-run against a leftover .work/ios/<core> that already has the hunk skips
# rather than failing. If the anchor moved (a moved pin, or CONTINUUM_UNPINNED=1), patch fails
# loudly so CI goes red instead of shipping an unpatched core.
ios_apply_core_patches() {
  local core="$1"
  case "$core" in
    parallel_n64)
      ios_apply_parallel_n64_aarch64_hot_state_gate_patch
      ios_apply_parallel_n64_first_tick_patch
      ;;
    azahar)
      ios_apply_azahar_pipeline_wait_patch
      ios_apply_simple_patch azahar azahar-use-jit-when-the-host-allows-it.patch \
        src/citra_libretro/core_settings.cpp "Continuum: the recompiler when the host says JIT"
      ios_apply_simple_patch azahar azahar-ios-jit-region.patch \
        externals/oaknut/include/oaknut/code_block.hpp "Continuum: code memory from the host app"
      ;;
    ppsspp)
      ios_apply_simple_patch ppsspp ppsspp-ios-jit-region.patch \
        Common/MemoryUtil.h "Continuum: code memory from the host app's prepared region"
      ;;
    flycast_jit)
      ios_apply_simple_patch flycast_jit flycast-ios-jit-region.patch \
        core/build.h "Continuum: an iPhone build with the recompilers on"
      ;;
    parallel_n64_jit)
      ios_apply_parallel_n64_aarch64_hot_state_gate_patch
      ios_apply_parallel_n64_first_tick_patch
      ios_apply_simple_patch parallel_n64_jit parallel_n64-ios-jit.patch \
        mupen64plus-core/src/device/r4300/new_dynarec/new_dynarec.c "Continuum: iPhone JIT build."
      # ORDER MATTERS: this one's context is the file as the patch above leaves it. Both were
      # replayed from a pristine checkout of the pin, in this order, to confirm it.
      ios_apply_simple_patch parallel_n64_jit parallel_n64-ios-jit-region.patch \
        mupen64plus-core/src/device/r4300/new_dynarec/new_dynarec.c "Continuum, part 2:"
      ;;
    pcsx_rearmed_jit)
      ios_apply_simple_patch pcsx_rearmed_jit pcsx_rearmed-ios-jit.patch \
        libpcsxcore/new_dynarec/new_dynarec_config.h "Continuum: iPhone JIT build."
      ;;
  esac
}

# One patch from scripts/patches/, applied once, failing loudly if it no longer fits.
# Arguments: core name (for messages), patch file name, a file the patch changes, and a marker
# string the patched file contains.
ios_apply_simple_patch() {
  local core="$1" name="$2" file="$3" marker="$4"
  local src="$IOS_SRC_DIR/$file"
  local patch="$ROOT/scripts/patches/$name"
  [[ -f "$src" ]] || { echo "error: $core: expected $src after clone; upstream layout changed" >&2; exit 1; }
  [[ -f "$patch" ]] || { echo "error: $core: missing Continuum patch at $patch" >&2; exit 1; }
  if grep -qF "$marker" "$src"; then
    echo "==> $core: $name already applied"
    return
  fi
  echo "==> $core: applying $name"
  patch -p1 --forward -d "$IOS_SRC_DIR" < "$patch" || {
    echo "error: $core: $name failed to apply; refresh it against the pinned source" >&2
    exit 1
  }
  grep -qF "$marker" "$src" || {
    echo "error: $core: $name reported success but its marker is missing" >&2
    exit 1
  }
}

ios_apply_parallel_n64_aarch64_hot_state_gate_patch() {
  local src="$IOS_SRC_DIR/mupen64plus-core/src/device/r4300/r4300.h"
  local patch="$ROOT/scripts/patches/parallel_n64-aarch64-gate-hot-state-on-new-dynarec.patch"
  local marker="Continuum: iOS builds WITH_DYNAREC="

  [[ -f "$src" ]] || {
    echo "error: parallel_n64: expected $src after clone; upstream layout changed" >&2
    exit 1
  }
  [[ -f "$patch" ]] || {
    echo "error: parallel_n64: missing Continuum patch at $patch" >&2
    exit 1
  }

  if grep -qF "$marker" "$src"; then
    echo "==> parallel_n64: Continuum aarch64 NEW_DYNAREC gate already present"
    return
  fi

  echo "==> parallel_n64: applying Continuum aarch64 NEW_DYNAREC gate (r4300.h hot-state aliases)"
  # -p1 strips a/ b/ from the unified diff. Fail loud if the hunk no longer matches HEAD:
  # an unpatched dylib would leave iOS interpreter builds stuck after the first VI.
  if ! patch -p1 --forward -d "$IOS_SRC_DIR" < "$patch"; then
    echo "error: parallel_n64: Continuum aarch64 NEW_DYNAREC gate patch failed to apply." >&2
    echo "       Upstream likely moved the aarch64 branch in r4300.h; refresh the patch" >&2
    echo "       against mupen64plus-core/src/device/r4300/r4300.h (mupencorestop aliases)." >&2
    exit 1
  fi

  grep -qF "$marker" "$src" || {
    echo "error: parallel_n64: patch reported success but the Continuum marker is missing" >&2
    exit 1
  }
}

ios_apply_parallel_n64_first_tick_patch() {
  local src="$IOS_SRC_DIR/libretro/libretro.c"
  local patch="$ROOT/scripts/patches/parallel_n64-reapply-variables-after-initiate-gfx.patch"
  local marker="Continuum: re-apply core options after InitiateGFX"

  [[ -f "$src" ]] || {
    echo "error: parallel_n64: expected $src after clone; upstream layout changed" >&2
    exit 1
  }
  [[ -f "$patch" ]] || {
    echo "error: parallel_n64: missing Continuum patch at $patch" >&2
    exit 1
  }

  if grep -qF "$marker" "$src"; then
    echo "==> parallel_n64: Continuum first-tick patch already present"
    return
  fi

  echo "==> parallel_n64: applying Continuum first-tick patch (reapply options after InitiateGFX)"
  # -p1 strips a/ b/ from the unified diff. Fail loud if the hunk no longer matches HEAD:
  # an unpatched dylib would look like a Continuum host bug on device.
  if ! patch -p1 --forward -d "$IOS_SRC_DIR" < "$patch"; then
    echo "error: parallel_n64: Continuum first-tick patch failed to apply." >&2
    echo "       Upstream likely moved emu_step_initialize; refresh the patch against" >&2
    echo "       libretro/libretro.c around plugin_connect_all / CoreDoCommand(EXECUTE)." >&2
    exit 1
  fi

  grep -qF "$marker" "$src" || {
    echo "error: parallel_n64: patch reported success but the Continuum marker is missing" >&2
    exit 1
  }
}

ios_apply_azahar_pipeline_wait_patch() {
  local src="$IOS_SRC_DIR/src/video_core/renderer_vulkan/vk_rasterizer.cpp"
  local patch="$ROOT/scripts/patches/azahar-do-not-wait-on-pipeline-compile.patch"
  local marker="Continuum: do not block the emulation thread on Vulkan pipeline compile."

  [[ -f "$src" ]] || {
    echo "error: azahar: expected $src after clone; upstream layout changed" >&2
    exit 1
  }
  [[ -f "$patch" ]] || {
    echo "error: azahar: missing Continuum patch at $patch" >&2
    exit 1
  }

  if grep -qF "$marker" "$src"; then
    echo "==> azahar: Continuum pipeline-wait patch already present"
    return
  fi

  echo "==> azahar: applying Continuum pipeline-wait patch (do not WaitDone on new pipelines)"
  if ! patch -p1 --forward -d "$IOS_SRC_DIR" < "$patch"; then
    echo "error: azahar: Continuum pipeline-wait patch failed to apply." >&2
    echo "       Upstream likely moved wait_built in vk_rasterizer.cpp; refresh the patch." >&2
    exit 1
  fi

  grep -qF "$marker" "$src" || {
    echo "error: azahar: patch reported success but the Continuum marker is missing" >&2
    exit 1
  }
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
  ios_apply_core_patches "$core"

  case "$IOS_KIND" in
    make) build_ios_make_core "$core" ;;
    cmake) build_ios_cmake_core "$core" ;;
    cmake-shared) build_ios_cmake_shared_core "$core" ;;
    cmake-ppsspp) build_ios_ppsspp_core "$core" ;;
    cmake-flycast) build_ios_flycast_core "$core" ;;
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
# Is this core's staged dylib already here AND recorded as built from the commit it is
# pinned to? Then it does not need building again.
#
# WHY THIS EXISTS. Building the cores is 35 of the ~40 minutes of a CI build, every build,
# including one that only changed a line of documentation. Worse, the owner feels that cost
# twice: anything that goes wrong late in the job means waiting the whole 35 minutes again
# for the retry. Every core is pinned to an exact upstream commit, so the output is a pure
# function of (that commit + this script + the patches) and is safe to keep.
#
# The proof of provenance is ios_record_source_version's manifest, not the file's presence:
# the line has to name this core's repository, its pinned sha, and the word `pinned`, which
# that function only writes when the checkout's HEAD really was the pin. A dylib left over
# from a different commit cannot satisfy that.
#
# PATCHES ARE NOT COVERED BY THE PIN, so they are covered by the CI cache key instead
# (hashFiles over scripts/patches/** in .github/workflows/ios.yml): editing a patch throws
# the whole cache away and everything rebuilds.
#
# OFF BY DEFAULT. Only CI sets CONTINUUM_CORE_CACHE=1. A build run by hand stays exactly as
# predictable as it was, and `rm -rf native/ios/build/lib` is still the way to force
# everything.
ios_core_is_cached() {
  [[ "${CONTINUUM_CORE_CACHE:-0}" == "1" ]] || return 1
  # NEVER reuse anything when the run deliberately asked for newer upstream code.
  #
  # CONTINUUM_UNPINNED=1 exists to build every core at its default branch's HEAD instead of
  # its pin, which is how a newer core gets tried on purpose. Without this line that request
  # would be silently ignored: the restored manifest still says `pinned`, the check below
  # would pass, the build would be skipped, and the run would quietly ship the OLD dylib
  # while reporting success. Reusing a cached build is only ever correct when the run wanted
  # the pinned commit in the first place.
  ios_unpinned && return 1
  local core="$1"
  ios_core_config "$core"
  [[ -n "${IOS_PIN:-}" ]] || return 1
  [[ -f "$IOS_OUT_DIR/$IOS_DYLIB_NAME" ]] || return 1
  local manifest="$IOS_OUT_DIR/core-sources.txt"
  [[ -f "$manifest" ]] || return 1
  grep -qx "$core $IOS_REPO $IOS_PIN pinned" "$manifest" || return 1
  return 0
}

build_all_ios_cores() {
  ios_require_darwin

  # A string, not an array: on bash 3.2 an empty array expanded under `set -u` is an error,
  # and this one is empty exactly when everything worked.
  local failed=""
  local failed_count=0
  local reused=0
  local core
  for core in "${IOS_CORES[@]}"; do
    echo
    echo "======================================================== ios: $core"
    if ios_core_is_cached "$core"; then
      echo "==> $core: reusing the cached dylib, still at its pinned commit"
      reused=$((reused + 1))
      continue
    fi
    if bash "$IOS_SELF" ios "$core"; then
      echo "==> $core ok"
    else
      echo "!!! $core FAILED (see the output above for why)" >&2
      failed="$failed $core"
      failed_count=$((failed_count + 1))
    fi
  done

  # Optional cores: same separate-process build, but a failure is a warning. The .ipa ships
  # without the core and every downstream consumer reports it as missing rather than failing.
  local optional_failed=""
  for core in "${IOS_OPTIONAL_CORES[@]}"; do
    echo
    echo "======================================================== ios (optional): $core"
    if ios_core_is_cached "$core"; then
      echo "==> $core: reusing the cached dylib, still at its pinned commit"
      reused=$((reused + 1))
      continue
    fi
    if bash "$IOS_SELF" ios "$core"; then
      echo "==> $core ok"
    else
      echo "!!! optional core $core FAILED; the .ipa will ship without it" >&2
      optional_failed="$optional_failed $core"
      ios_core_config "$core"
      rm -f "$IOS_OUT_DIR/$IOS_DYLIB_NAME"
    fi
  done

  echo
  if [[ "$reused" -gt 0 ]]; then
    echo "==> $reused core(s) reused from the cache, not rebuilt"
  fi
  echo "==> iOS core summary (native/ios/build/lib)"
  for core in "${IOS_CORES[@]}" "${IOS_OPTIONAL_CORES[@]}"; do
    ios_core_config "$core"
    if [[ -f "$IOS_OUT_DIR/$IOS_DYLIB_NAME" ]]; then
      echo "      ok       $IOS_DYLIB_NAME"
    else
      echo "      MISSING  $IOS_DYLIB_NAME"
    fi
  done
  if [[ -n "$optional_failed" ]]; then
    echo "warning: optional core(s) not built:$optional_failed" >&2
  fi

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
  ios-optional-names)
    ios_print_optional_names
    exit 0
    ;;
  ios-pins)
    ios_print_pins
    exit 0
    ;;
  ios-stage-prebuilt)
    # Runs the SAME staged-dylib check every from-source core goes through (all twenty entry
    # points, the @rpath install name, the copy into native/ios/build/lib) on a dylib this script
    # did not build. scripts/fetch-buildbot-cores.sh calls it for every downloaded core, so a
    # prebuilt dylib cannot reach the .ipa on a weaker check than a compiled one. Any host: the
    # Mach-O tools fall back to LLVM's off a Mac (see ios_tool).
    if [[ "$#" -lt 3 ]]; then
      echo "error: 'ios-stage-prebuilt' needs <dylib path> <canonical name>" >&2
      exit 1
    fi
    ios_stage_dylib "$2" "$3"
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
