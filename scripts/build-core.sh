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
#   scripts/build-core.sh ios-fingerprint fceumm # the core's build-cache fingerprint
#   scripts/build-core.sh ios-is-cached fceumm   # exit 0 if the staged dylib would be reused
#   scripts/build-core.sh ios-recipe-slice fceumm # the part of this file that is fceumm's recipe
#   scripts/build-core.sh ios-recipe-shared      # the part of this file every core shares
#   scripts/build-core.sh ios-cache-check        # exit 0 if this file keeps the cache's rules
#   scripts/build-core.sh ios-apply-patches <core> <dir>  # our patches onto a source tree
#
# A bare core name is not a subcommand. The dispatch at the end refuses it and names the
# `ios <core>` spelling instead. The one exception is `build-core.sh pcsx_rearmed`, the spelling
# the first iOS core was added with: it still works as an alias for `ios pcsx_rearmed`, because
# something may still call it, but the explicit form is the one to use and
# native/ios/build-engine.sh has been moved onto it.
#
# `ios` and `ios-all` need a macOS host and say so clearly anywhere else, the same contract as
# native/ios/build-engine.sh. Every other subcommand above is the exception: they only print
# the table, hash files, patch a directory, or check and copy a file, so they run on any host,
# which is what makes the filenames, the cache and the patches checkable from Linux.
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

# THE LISTS OF CORES are not a build input (see "WHAT COUNTS", above ios_cache_scan): which
# cores are listed decides which cores get built, never how any one of them is built, so adding
# or removing a core here rebuilds no other core. Only the two array assignments may sit in this
# region, and only plain core names inside them; anything else and every core rebuilds.
# not-a-build-input-start: lists
# The iOS cores, in build order. Never empty, which matters: macOS ships bash 3.2,
# where an empty array expanded under `set -u` is an error rather than nothing.
IOS_CORES=(fceumm mesen2 mgba bsnes gearsystem blastem pcsx_rearmed mednafen_psx_hw melonds
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
IOS_OPTIONAL_CORES=(flycast pcsx_rearmed_jit parallel_n64_jit flycast_jit continuum_symbian continuum_gb continuum_arcade)
# not-a-build-input-end: lists

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
  # Where a make-based core leaves its dylib, relative to IOS_MAKE_SUBDIR. Only bsnes differs.
  IOS_OUTPUT_SUBDIR="."
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
    # recipe-start: fceumm
    fceumm)
      IOS_REPO="https://github.com/libretro/libretro-fceumm"
      IOS_PIN="ac0d00a7fc01166f84c7bac01d1084ebfd2b865e"
      IOS_DYLIB_NAME="fceumm_libretro_ios.dylib"
      IOS_KIND="make"
      IOS_MAKEFILE="Makefile.libretro"
      IOS_DISPLAY="NES"
      ;;
    # recipe-end: fceumm
    # recipe-start: mesen2
    mesen2)
      # NES, the default NES core. Makefile.libretro's ios-arm64 block emits
      # mesen2_libretro_ios.dylib at the repo root. Continuum patch
      # (scripts/patches/mesen2-nes-memory-cheats-saves.patch): the NES battery RAM and internal
      # RAM through retro_get_memory_data with FCEUmm's memory map, so the app keeps the in-game
      # save and achievements and RAM search work; typed cheats reach Mesen's cheat engine; and
      # Mesen stops writing a .sav of its own for the battery RAM.
      IOS_REPO="https://github.com/libretro/MesenCE"
      IOS_PIN="4c4e069f16a8ad6ed79e046046c56d51558c7daf"
      IOS_DYLIB_NAME="mesen2_libretro_ios.dylib"
      IOS_KIND="make"
      IOS_MAKEFILE="Makefile.libretro"
      IOS_DISPLAY="NES, Mesen 2 with a Continuum patch"
      ;;
    # recipe-end: mesen2
    # recipe-start: mgba
    mgba)
      IOS_REPO="https://github.com/libretro/mgba"
      IOS_PIN="7a12d6d4b9acb14c0ae62c9166b6a2f3d08007f6"
      IOS_DYLIB_NAME="mgba_libretro_ios.dylib"
      IOS_KIND="cmake"
      IOS_CMAKE_TARGET="mgba_libretro"
      IOS_CMAKE_ARCHIVE="mgba_libretro.a"
      IOS_DISPLAY="GBA + GB/GBC"
      ;;
    # recipe-end: mgba
    # recipe-start: bsnes
    bsnes)
      # SNES. The libretro target is bsnes/target-libretro/GNUmakefile, driven from bsnes/ with
      # target=libretro, and its ios-arm64 block links out/bsnes_libretro_ios.dylib. nall picks no
      # compiler for ios-arm64 (it would fall back to the host g++), so the arm64 clang++ and the
      # SDK are passed in, and openmp=false because nall's default adds -fopenmp, which Apple's
      # clang does not have. Continuum patch (scripts/patches/bsnes-continuum-memory-and-saves.patch):
      # work RAM and the cartridge's battery RAM through retro_get_memory_data, so the app owns
      # the .srm and achievements and RAM search can read memory; the core's own save.ram is
      # off, and save states are the exact (Strict) kind for rewind, run-ahead and online play.
      IOS_REPO="https://github.com/libretro/bsnes-libretro"
      IOS_PIN="05439f96121d2b9d7ad7a5fc1f29d7eebdcc8c43"
      IOS_DYLIB_NAME="bsnes_libretro_ios.dylib"
      IOS_KIND="make"
      IOS_MAKEFILE="GNUmakefile"
      IOS_MAKE_SUBDIR="bsnes"
      IOS_OUTPUT_SUBDIR="out"
      IOS_MAKE_VARS=(target=libretro binary=library local=false openmp=false
                     "compiler=clang++ -arch arm64 -isysroot @IOSSDK@ -miphoneos-version-min=11.0")
      IOS_DISPLAY="SNES, bsnes with a Continuum patch"
      ;;
    # recipe-end: bsnes
    # recipe-start: gearsystem
    gearsystem)
      # Master System, Game Gear and SG-1000. platforms/libretro/Makefile has an ios-arm64 block
      # that emits gearsystem_libretro_ios.dylib. Continuum patch
      # (scripts/patches/gearsystem-sg1000-memory-map.patch, which its name now undersells): the
      # SG-1000 memory map, so achievements see its RAM, AND the save-state fix that made two
      # phones' states match byte for byte (the Sports Pad's timing was being written as this
      # phone's own running cycle count, which broke the state hash online play compares).
      IOS_REPO="https://github.com/drhelius/Gearsystem"
      IOS_PIN="4a801d223ce687e68165d750a0fdda4bd8bc0a7e"
      IOS_DYLIB_NAME="gearsystem_libretro_ios.dylib"
      IOS_KIND="make"
      IOS_MAKEFILE="Makefile"
      IOS_MAKE_SUBDIR="platforms/libretro"
      IOS_DISPLAY="Master System, Game Gear and SG-1000, Gearsystem with a Continuum patch"
      ;;
    # recipe-end: gearsystem
    # recipe-start: blastem
    blastem)
      # Mega Drive, Sega CD and 32X. Makefile.libretro's ios-arm64 block builds the generated
      # (interpreter) 68K and Z80 cores, as iOS has no writable executable memory, and its `core`
      # rule copies libblastem.dylib to blastem_libretro_ios.dylib. The generator needs python3,
      # and the LTO link takes about 12 minutes. Continuum patches, applied in this order
      # (scripts/patches/): blastem-continuum-memory.patch, Sega CD and 32X memory for
      # achievements and RAM search, the Sega CD's backup RAM as its save, typed cheats, the
      # multitaps and Super Skidmarks; blastem-cartridge-saves.patch, every EEPROM and J-Cart
      # cartridge dump in MAME's software list that rom.db did not know by checksum;
      # blastem-sega-cd-discs.patch, .m3u playlists and disc changing for the Sega CD, and its
      # save cartridge as memory of its own; blastem-32x-startup.patch, Continuum's own start-up
      # code for the 32X's two SH-2s (continuum_32x_boot.s, assembled into continuum_32x_boot.h),
      # so a 32X cartridge needs no BIOS files and the player's own pair is used when both are
      # there, plus the SH-2 reset fix it needed (a reset ran an instruction fetched before it).
      IOS_REPO="https://github.com/libretro/blastem"
      IOS_PIN="ab743653ed110eeb7b1fe64d00cc5f9279c2bfec"
      IOS_DYLIB_NAME="blastem_libretro_ios.dylib"
      IOS_KIND="make"
      IOS_MAKEFILE="Makefile.libretro"
      IOS_DISPLAY="Mega Drive, Sega CD and 32X, BlastEm with a Continuum patch"
      ;;
    # recipe-end: blastem
    # recipe-start: pcsx_rearmed
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
    # recipe-end: pcsx_rearmed
    # recipe-start: pcsx_rearmed
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
    # recipe-end: pcsx_rearmed
    # recipe-start: melonds
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
    # recipe-end: melonds
    # recipe-start: mednafen_pce_fast
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
    # recipe-end: mednafen_pce_fast
    # recipe-start: stella2023
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
    # recipe-end: stella2023
    # recipe-start: mednafen_psx_hw
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
    # recipe-end: mednafen_psx_hw
    # recipe-start: parallel_n64
    parallel_n64)
      IOS_REPO="https://github.com/libretro/parallel-n64"
      IOS_PIN="a8280501a71c1bcd4c85c5b8bfe75cfa62cd6127"
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
    # recipe-end: parallel_n64
    # recipe-start: parallel_n64
    parallel_n64_jit)
      # The N64 recompiler (new_dynarec, aarch64), as the core's own macOS arm64 block builds it,
      # on the iOS platform block. Command-line variables replace the ios block's no-recompiler
      # flags: WITH_DYNAREC on, PLATCFLAGS and CPUFLAGS without NO_ASM (which compiles the
      # recompiler's setup out) and without the -D__arm__ the interpreter build carries, and the
      # assembler wrapper the macOS arm64 block uses for linkage_arm64.S. @IOSSDK@ is filled in at
      # build time. Patched for iOS: pages switched with mprotect instead of the macOS-only
      # pthread_jit_write_protect_np (scripts/patches/parallel_n64-ios-jit.patch).
      IOS_REPO="https://github.com/libretro/parallel-n64"
      IOS_PIN="a8280501a71c1bcd4c85c5b8bfe75cfa62cd6127"
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
    # recipe-end: parallel_n64
    # recipe-start: azahar
    azahar)
      # Azahar is the maintained Citra fork. Its libretro target builds for iOS
      # (their own libretro-ios job). Apple builds turn OpenGL OFF and Vulkan ON,
      # and the core presents with retro_vulkan_image set_image — the only hardware
      # picture hook this host accepts. CPU JIT is compiled out by -DIOS (no
      # dynarec, no executable memory). The fast interpreter stays on.
      IOS_REPO="https://github.com/azahar-emu/azahar"
      IOS_PIN="68202bc4ff7edf9d7280ec0dcd6572b7cd1ae26e"
      IOS_DYLIB_NAME="azahar_libretro_ios.dylib"
      IOS_KIND="cmake-shared"
      IOS_CMAKE_TARGET="citra_libretro"
      IOS_SUBMODULES=1
      # Continuum patch (scripts/patches/azahar-do-not-wait-on-pipeline-compile.patch):
      # RasterizerVulkan waits on every new pipeline because async_shader_compilation
      # defaults off (wait_built is then always true) and BindPipeline calls WaitDone.
      # That is the post-transition hitch. The patch skips the draw instead of waiting,
      # behind a core option "Shader Compile (Continuum)" (azahar_continuum_async_pipelines):
      # "Async shaders (fix, may pop in)" (default) or "Old (may stutter)", upstream's wait.
      # No JIT, no dynarec, no executable memory.
      IOS_DISPLAY="Nintendo 3DS, Vulkan, interpreter CPU (no JIT)"
      ;;
    # recipe-end: azahar
    # recipe-start: ppsspp
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
      # RETRO_ENVIRONMENT_GET_JIT_CAPABLE; the host answers yes only when JIT
      # is really usable on the phone (crates/emulator-bridge/src/jit.rs), and
      # on a no the core drops back to the IR interpreter.
      #
      # Picture is Vulkan. The toolchain builds with GLES2, the host's preferred
      # hardware context is Vulkan, and CreateGraphicsContext tries Vulkan for
      # that answer. Frames are the same set_image path Azahar uses. No BIOS.
      #
      # USE_FFMPEG=ON with the ffmpeg submodule (prebuilt static libs per
      # platform; CMake picks ffmpeg/ios/universal). PSMF video decodes. The
      # selected submodule list is what the libretro target's CMake actually
      # add_subdirectory's.
      IOS_REPO="https://github.com/hrydgard/ppsspp"
      IOS_PIN="dd6bd1a53d38773c1d07985fcb7d2f56fcaa872c"
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
        ffmpeg
      )
      IOS_DISPLAY="PSP, Vulkan, IR interpreter (no JIT, no dynarec)"
      ;;
    # recipe-end: ppsspp
    # recipe-start: continuum_gb
    continuum_gb)
      # Game Boy, Color and Advance. cBoy: this project's own core, written in Rust in
      # cores/continuum-gb (no upstream, so no repo or pin), one core for all three. A selectable
      # alternative to mGBA, which stays the default. OPTIONAL: a failure leaves the .ipa without it, nothing else.
      IOS_REPO=""
      IOS_PIN=""
      IOS_DYLIB_NAME="continuum_gb_libretro_ios.dylib"
      IOS_KIND="cargo-continuum-gb"
      IOS_DISPLAY="Game Boy / Color / Advance, cBoy (own Rust core), optional"
      ;;
    # recipe-end: continuum_gb
    # recipe-start: continuum_arcade
    continuum_arcade)
      # Arcade. cArcade: this project's own arcade emulator, written in Rust in
      # cores/continuum-arcade (no upstream, so no repo or pin), the only arcade core. OPTIONAL: a
      # failure leaves the .ipa without it, and arcade games keep their "not playable" line.
      IOS_REPO=""
      IOS_PIN=""
      IOS_DYLIB_NAME="continuum_arcade_libretro_ios.dylib"
      IOS_KIND="cargo-continuum-arcade"
      IOS_DISPLAY="Arcade, cArcade (own Rust core), optional"
      ;;
    # recipe-end: continuum_arcade
    # recipe-start: continuum_symbian
    continuum_symbian)
      # Symbian / N-Gage. Continuum Symbian: native/continuum-symbian (this project's libretro
      # plug) linked against EKA2L1's jitless iOS port, built with that port's own CMake.
      # The CPU is its dyncom interpreter; dynarmic is compiled in but never selected here.
      # OPTIONAL: a failure leaves the .ipa without Symbian, nothing else.
      IOS_REPO="https://github.com/MuhannadYT/EKA2L1_IOS"
      IOS_PIN="661031f9d8ccd37612797e32a52b2d4c6aadfcdc"
      IOS_DYLIB_NAME="continuum_symbian_libretro_ios.dylib"
      IOS_KIND="cmake-continuum-symbian"
      IOS_SUBMODULES=1
      IOS_DISPLAY="Symbian / N-Gage, Continuum Symbian on EKA2L1 (dyncom interpreter, no JIT), optional"
      ;;
    # recipe-end: continuum_symbian
    # recipe-start: flycast
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
      IOS_PIN="f03faffd2f89cb2c7c73ff08dce51a774a61ee27"
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
    # recipe-end: flycast
    # recipe-start: flycast
    flycast_jit)
      # Dreamcast with flycast's recompilers (SH4, ARM7, DSP): the same build without
      # TARGET_NO_REC. On iPhone flycast already switches its code pages between writable and
      # runnable (TARGET_IPHONE in rec_arm64.cpp and posix_vmem.cpp), which is how its own iOS app
      # runs with JIT, so this needs no patch.
      IOS_REPO="https://github.com/flyinghead/flycast"
      IOS_PIN="f03faffd2f89cb2c7c73ff08dce51a774a61ee27"
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
    # recipe-end: flycast
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

# not-a-build-input-start: cache
# The help text. Part of the cache region (see "WHAT COUNTS", above ios_cache_scan): it only
# prints, so editing it rebuilds nothing.
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
  ios-fingerprint <core>
                print the core's build fingerprint (what the CI core cache
                matches on). Any host.
  ios-is-cached <core>
                exit 0 when the staged dylib can be reused as it is, 1 when it
                must be built. Any host.
  ios-recipe-slice <core>
                print the part of this script that is the core's own recipe, the
                part its fingerprint hashes per core. Any host.
  ios-recipe-shared
                print the part of this script every core shares. A change in
                there rebuilds all of them. Comments, blank lines, the cache's
                own code and the lists of cores are not in it. Any host.
  ios-cache-check
                exit 0 only when this script keeps every rule of the build
                cache: which lines count, and where its regions may sit.
                Any host.
  ios-cache-fingerprints
                every core's fingerprint, from one reading of this script.
  ios-cache-scan <mode> [family]
                how the cache reads this script, line by line (mode table),
                and the parts it hashes. For scripts/check-core-cache.sh.
  ios-apply-patches <core> <source dir>
                apply the core's Continuum patches to a checkout of its source,
                failing if one no longer fits. Any host.

'ios' and 'ios-all' need a macOS host with the Xcode command line tools.

Each core is built from its pinned commit (IOS_PIN). CONTINUUM_UNPINNED=1 builds
every core at its default branch's HEAD instead, to try newer cores on purpose.
EOF
}
# not-a-build-input-end: cache

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
# were written against these trees, and a moved hunk anchor fails the core. FCEUmm, Gearsystem,
# ParaLLEl N64, Azahar, PPSSPP and Flycast moved on 12 October 2026 after the update check
# (.agents/tasks/updates-2026-10-12/EVALUATION.md); Beetle PSX HW was checked then and kept.
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
#   4. dry-run its patches against that commit (patch -p1 --dry-run --forward), if it has any;
#   5. update crates/emulator-bridge/src/core_updates.rs: the core's build in CORE_BUILDS, and one
#      CORE_UPDATES entry saying whether old save states load on the new commit (players get a
#      heads-up from it). cargo test fails until you do.
# core-sources.txt always records the commit really built, pinned or not, with a fourth field
# saying which, and a fifth with the core's build fingerprint (see ios_core_fingerprint).
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
  # In a subshell: ios_record_source_version is cache machinery, which is not a build input, so
  # nothing it sets may reach the build that follows. A subshell keeps every variable, option and
  # directory change inside it. scripts/check-core-cache.sh checks this line stays a subshell.
  ( ios_record_source_version "$core" )
}

# not-a-build-input-start: cache
# Where every core's source came from, written down:
# "<core> <repo> <commit> pinned|unpinned fp:<fingerprint>". The fingerprint names everything
# else the dylib is made from (ios_core_fingerprint); ios_core_is_cached needs it to match.
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
  # A fingerprint that cannot be worked out is written as fp:unknown, which never matches, so
  # the cache can only ever rebuild this core rather than trust it.
  local fp
  fp="$(ios_core_fingerprint "$core")" || fp="unknown"
  [[ -n "$fp" ]] || fp="unknown"
  echo "$core $IOS_REPO $sha $state fp:$fp" >> "$manifest"
  LC_ALL=C sort -o "$manifest" "$manifest"
}
# not-a-build-input-end: cache

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

# recipe-start: continuum_gb
# cBoy (cores/continuum-gb): a plain cargo cdylib for aarch64-apple-ios, on the
# toolchain pinned in rust-toolchain.toml, staged through the same symbol and install-name checks.
build_ios_continuum_gb_core() {
  local core="$1"
  local crate="$ROOT/cores/continuum-gb"
  echo "==> building $core ($IOS_DISPLAY)"
  ( cd "$crate" && rustup target add aarch64-apple-ios >/dev/null \
      && IPHONEOS_DEPLOYMENT_TARGET="${IPHONEOS_DEPLOYMENT_TARGET:-15.0}" \
         cargo build --release --target aarch64-apple-ios )
  local built="$crate/target/aarch64-apple-ios/release/libcontinuum_gb.dylib"
  [[ -f "$built" ]] || { echo "error: $core: cargo produced no $built" >&2; exit 1; }
  ios_stage_dylib "$built" "$IOS_DYLIB_NAME"
}
# recipe-end: continuum_gb

# recipe-start: continuum_arcade
# cArcade (cores/continuum-arcade): built exactly as cBoy is, a plain cargo cdylib for
# aarch64-apple-ios on the pinned toolchain, staged through the same symbol and install-name checks.
build_ios_continuum_arcade_core() {
  local core="$1"
  local crate="$ROOT/cores/continuum-arcade"
  echo "==> building $core ($IOS_DISPLAY)"
  ( cd "$crate" && rustup target add aarch64-apple-ios >/dev/null \
      && IPHONEOS_DEPLOYMENT_TARGET="${IPHONEOS_DEPLOYMENT_TARGET:-15.0}" \
         cargo build --release --target aarch64-apple-ios )
  local built="$crate/target/aarch64-apple-ios/release/libcontinuum_arcade.dylib"
  [[ -f "$built" ]] || { echo "error: $core: cargo produced no $built" >&2; exit 1; }
  ios_stage_dylib "$built" "$IOS_DYLIB_NAME"
}
# recipe-end: continuum_arcade

build_ios_make_core() {
  local core="$1"
  local make_dir="$IOS_SRC_DIR/$IOS_MAKE_SUBDIR"
  local out_dir="$make_dir/$IOS_OUTPUT_SUBDIR"

  [[ -f "$make_dir/$IOS_MAKEFILE" ]] || {
    echo "error: $core: no $IOS_MAKEFILE in $make_dir; upstream moved its libretro makefile" >&2
    exit 1
  }

  # Cleared before the build, not after. The clone in .work/ios/<core> persists between runs,
  # so without this a dylib left over from an earlier successful build would satisfy the
  # existence check below even if this build produced nothing.
  rm -f "$out_dir/$IOS_DYLIB_NAME"

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
  local built="$out_dir/$IOS_DYLIB_NAME"
  if [[ ! -f "$built" ]]; then
    built="$(find "$out_dir" -maxdepth 1 -name '*_libretro_ios.dylib' -type f | head -1 || true)"
  fi
  [[ -n "$built" && -f "$built" ]] || {
    echo "error: $core: the make finished but left no .dylib in $out_dir" >&2
    exit 1
  }
  if [[ "$(basename "$built")" != "$IOS_DYLIB_NAME" ]]; then
    echo "==> note: upstream produced $(basename "$built"); staging it as $IOS_DYLIB_NAME"
  fi

  ios_stage_dylib "$built" "$IOS_DYLIB_NAME"
}

# recipe-start: mgba
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
# recipe-end: mgba


# recipe-start: azahar
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
# recipe-end: azahar

# recipe-start: ppsspp
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
  # USE_FFMPEG=ON with the ffmpeg submodule's prebuilt ffmpeg/ios/universal static libs, the
  # same ones PPSSPP's own iOS app links. With it OFF, PSMF movies and their ATRAC audio did not
  # decode: EA titles (NFS Most Wanted 5-1-0) draw a looping movie on their loading screens, which
  # showed as a gray translucent ghost even with PPSSPP's software renderer, and the movie's
  # audio stream came out as a loud buzz. That is a missing decoder, not a GPU problem.
  # The extra linker flags are the system libraries the prebuilt ffmpeg may reference (its
  # VideoToolbox hwaccel, iconv, bzip2, zlib); the libretro target does not add them itself.
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
    -DUSE_FFMPEG=ON \
    -DCMAKE_SHARED_LINKER_FLAGS="-framework VideoToolbox -framework CoreMedia -framework CoreVideo -framework CoreFoundation -framework AudioToolbox -framework Security -liconv -lbz2 -lz" \
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
# recipe-end: ppsspp

# recipe-start: flycast
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
# recipe-end: flycast

# recipe-start: continuum_symbian
# Continuum Symbian. EKA2L1's iOS port builds with its own CMake (3.x only: CMake 4 cannot
# configure its older submodules) after the port's four submodule patches and an iphoneos
# FFmpeg. Our plug is added to that tree as one more target, so CMake carries every static
# library and framework EKA2L1 needs into the dylib. Its assets (GLES shaders, compat list,
# Symbian patch DLLs) are zipped into the dylib (a generated assets.c) and unpacked on first
# launch.
build_ios_continuum_symbian_core() {
  local core="$1"
  local src="$IOS_SRC_DIR"
  local plug="$ROOT/native/continuum-symbian"
  # This core is the only one that fetches its own build tools, and both are PINNED BY SHA256 like
  # every other download this project makes. Moving a pin: change the version in the url and the
  # path, and take the new sha256 from Kitware's published cmake-<version>-SHA-256.txt, or from the
  # ninja release asset's own digest. Both sums below were checked against the published ones.
  local cmake_dir="$IOS_WORK/cmake-3.31.6"
  local cmake3="$cmake_dir/cmake-3.31.6-macos-universal/CMake.app/Contents/bin/cmake"
  if [[ ! -x "$cmake3" ]]; then
    echo "==> fetching CMake 3.31.6 for $core"
    mkdir -p "$cmake_dir"
    ios_fetch_pinned \
      https://github.com/Kitware/CMake/releases/download/v3.31.6/cmake-3.31.6-macos-universal.tar.gz \
      "$cmake_dir/cmake.tar.gz" \
      330b9514f5112e5ed4fb08b8b05803b776fd9b539a6ae12927d14dcc0ee2ba8d \
      "CMake 3.31.6 for $core" || exit 1
    tar xzf "$cmake_dir/cmake.tar.gz" -C "$cmake_dir"
  fi
  # ninja's own macOS build (one universal x86_64 + arm64 binary at the root of the zip), not
  # brew's: `brew install ninja` took whatever version brew had that day, and was the one download
  # here that nothing could check. Passed to CMake below as CMAKE_MAKE_PROGRAM, so the pinned one
  # is the one that runs even when the runner image happens to ship a ninja of its own.
  local ninja_dir="$IOS_WORK/ninja-1.13.2"
  local ninja="$ninja_dir/ninja"
  if [[ ! -x "$ninja" ]]; then
    echo "==> fetching ninja 1.13.2 for $core"
    mkdir -p "$ninja_dir"
    ios_fetch_pinned \
      https://github.com/ninja-build/ninja/releases/download/v1.13.2/ninja-mac.zip \
      "$ninja_dir/ninja-mac.zip" \
      c99048673aa765960a99cf10c6ddb9f1fad506099ff0a0e137ad8960a88f321b \
      "ninja 1.13.2 for $core" || exit 1
    ( cd "$ninja_dir" && unzip -oq ninja-mac.zip )
    chmod +x "$ninja"
    [[ -x "$ninja" ]] || { echo "error: $core: the ninja zip left no ninja binary" >&2; exit 1; }
  fi
  local build_dir="$src/build-ios-device"
  local gen="$build_dir/continuum-symbian"

  if [[ ! -f "$src/.continuum-patched" ]]; then
    echo "==> $core: applying the iOS port's submodule patches and the Continuum frame patch"
    local P="$src/.github/ios-build/patches"
    git -C "$src/src/external/cubeb" apply "$P/cubeb.patch"
    git -C "$src/src/external/fmt" apply "$P/fmt.patch"
    git -C "$src/src/external/ffmpeg" apply "$P/ffmpeg.patch"
    git -C "$src/src/external/dynarmic/externals/oaknut" apply "$P/oaknut.patch"
    cp "$src/.github/ios-build/ffmpeg/build-ios-device.sh" "$src/src/external/ffmpeg/"
    chmod +x "$src/src/external/ffmpeg/build-ios-device.sh"
    git -C "$src" apply "$ROOT/scripts/patches/continuum-symbian-frame-readback.patch"
    grep -q continuum_symbian_frame_hook \
      "$src/src/emu/drivers/src/graphics/backend/context_eagl.mm" || {
      echo "error: $core: frame patch reported success but the hook is missing" >&2
      exit 1
    }
    cat >> "$src/src/emu/ios/CMakeLists.txt" <<CMAKE

# Continuum Symbian (added by Continuum's scripts/build-core.sh).
add_library(continuum_symbian_libretro SHARED
    "$plug/continuum_symbian_libretro.cpp"
    "$plug/eka2l1_engine.mm"
    "$gen/assets.c")
target_include_directories(continuum_symbian_libretro PRIVATE "$plug" "$ROOT/.work/hdr/libretro")
target_compile_definitions(continuum_symbian_libretro PRIVATE CONTINUUM_SYMBIAN_EKA2L1=1)
set_source_files_properties("$gen/assets.c" PROPERTIES OBJECT_DEPENDS "$gen/assets.zip")
set_target_properties(continuum_symbian_libretro PROPERTIES
    OUTPUT_NAME continuum_symbian_libretro_ios PREFIX "" SUFFIX ".dylib")
target_link_libraries(continuum_symbian_libretro PRIVATE eka2l1_ios_bridge miniz
    "-framework UIKit" "-framework Foundation" "-framework CoreGraphics"
    "-framework QuartzCore" "-framework OpenGLES" "-framework GameController"
    "-framework UniformTypeIdentifiers" "-framework AVFoundation" "-framework AudioToolbox"
    "-framework CoreAudio" "-framework CoreMotion" "-framework CoreHaptics"
    "-framework CoreMedia" "-framework CoreVideo" "-framework VideoToolbox"
    "-framework Security" z bz2 iconv)
CMAKE
    touch "$src/.continuum-patched"
  fi

  if [[ ! -f "$src/src/external/ffmpeg/macos/arm64-device/lib/libavcodec.a" ]]; then
    echo "==> $core: cross-compiling FFmpeg for iphoneos"
    ( cd "$src/src/external/ffmpeg" && ./build-ios-device.sh )
  fi

  # The asset blob's .incbin needs an absolute path. The zip itself is made after the patch
  # DLLs are built and before the plug is linked; a placeholder lets CMake configure.
  mkdir -p "$gen"
  [[ -f "$gen/assets.zip" ]] || : > "$gen/assets.zip"
  cat > "$gen/assets.c" <<ASSETS
// Generated by scripts/build-core.sh: the zipped assets, inside the dylib.
__asm__(".section __DATA,__const\\n"
        ".globl _continuum_symbian_assets\\n"
        ".globl _continuum_symbian_assets_end\\n"
        ".p2align 4\\n"
        "_continuum_symbian_assets:\\n"
        ".incbin \\"$gen/assets.zip\\"\\n"
        "_continuum_symbian_assets_end:\\n");
ASSETS

  echo "==> configuring $core ($IOS_DISPLAY)"
  "$cmake3" -S "$src" -B "$build_dir" -G Ninja \
    -DCMAKE_MAKE_PROGRAM="$ninja" \
    -DCMAKE_SYSTEM_NAME=iOS \
    -DCMAKE_OSX_SYSROOT=iphoneos \
    -DCMAKE_OSX_ARCHITECTURES=arm64 \
    -DCMAKE_OSX_DEPLOYMENT_TARGET=15.0 \
    -DCMAKE_BUILD_TYPE=Release

  echo "==> $core: building EKA2L1 and its patch DLLs"
  "$cmake3" --build "$build_dir" --target eka2l1_ios_bridge -j"$(ios_jobs)"
  local stage="$gen/stage"
  local res="$src/src/emu/drivers/resources"
  rm -rf "$stage" && mkdir -p "$stage/resources/upscale" "$stage/compat" "$stage/patch"
  cp -R "$res/gles/." "$stage/resources/"
  cp -R "$res/upscale/." "$stage/resources/upscale/"
  cp "$res/defaultbank.hsb" "$res/defaultbank.sf2" "$stage/resources/"
  cp -R "$src/miscs/compat/." "$stage/compat/"
  cp -R "$build_dir/bin/patch/." "$stage/patch/"
  rm -f "$gen/assets.zip"
  ( cd "$stage" && zip -qr ../assets.zip . )
  ls -l "$gen/assets.zip"

  echo "==> $core: linking the plug"
  "$cmake3" --build "$build_dir" --target continuum_symbian_libretro -j"$(ios_jobs)"
  local built
  built="$(find "$build_dir" -name "$IOS_DYLIB_NAME" -type f | head -1 || true)"
  [[ -n "$built" ]] || { echo "error: $core: no $IOS_DYLIB_NAME under $build_dir" >&2; exit 1; }
  ios_stage_dylib "$built" "$IOS_DYLIB_NAME"
}
# recipe-end: continuum_symbian

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
    # recipe-start: parallel_n64
    parallel_n64)
      ios_apply_parallel_n64_aarch64_hot_state_gate_patch
      ios_apply_parallel_n64_first_tick_patch
      ;;
    # recipe-end: parallel_n64
    # recipe-start: azahar
    azahar)
      ios_apply_azahar_pipeline_wait_patch
      ios_apply_simple_patch azahar azahar-use-jit-when-the-host-allows-it.patch \
        src/citra_libretro/core_settings.cpp "Continuum: the recompiler when the host says JIT"
      ios_apply_simple_patch azahar azahar-ios-jit-region.patch \
        externals/oaknut/include/oaknut/code_block.hpp "Continuum: code memory from the host app"
      ios_apply_simple_patch azahar azahar-libretro-camera-and-amiibo.patch \
        src/citra_libretro/citra_libretro.cpp "Continuum: the iPhone camera and Amiibo reach Azahar"
      ;;
    # recipe-end: azahar
    # recipe-start: ppsspp
    ppsspp)
      ios_apply_simple_patch ppsspp ppsspp-ios-jit-region.patch \
        Common/MemoryUtil.h "Continuum: code memory from the host app's prepared region"
      ;;
    # recipe-end: ppsspp
    # recipe-start: flycast
    flycast_jit)
      ios_apply_simple_patch flycast_jit flycast-ios-jit-region.patch \
        core/build.h "Continuum: an iPhone build with the recompilers on"
      ;;
    # recipe-end: flycast
    # recipe-start: parallel_n64
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
    # recipe-end: parallel_n64
    # recipe-start: pcsx_rearmed
    pcsx_rearmed_jit)
      ios_apply_simple_patch pcsx_rearmed_jit pcsx_rearmed-ios-jit.patch \
        libpcsxcore/new_dynarec/new_dynarec_config.h "Continuum: iPhone JIT build."
      ;;
    # recipe-end: pcsx_rearmed
    # recipe-start: mesen2
    mesen2)
      ios_apply_simple_patch mesen2 mesen2-nes-memory-cheats-saves.patch \
        Libretro/libretro.cpp "Continuum: NES memory, cheats and battery for the frontend"
      ;;
    # recipe-end: mesen2
    # recipe-start: bsnes
    bsnes)
      ios_apply_simple_patch bsnes bsnes-continuum-memory-and-saves.patch \
        bsnes/target-libretro/libretro.cpp "Continuum: expose work RAM and the cartridge's battery RAM"
      ;;
    # recipe-end: bsnes
    # recipe-start: gearsystem
    gearsystem)
      ios_apply_simple_patch gearsystem gearsystem-sg1000-memory-map.patch \
        platforms/libretro/libretro.cpp "Continuum: SG-1000 memory map"
      ;;
    # recipe-end: gearsystem
    # recipe-start: blastem
    blastem)
      ios_apply_simple_patch blastem blastem-continuum-memory.patch \
        libblastem.c "Continuum: Sega CD and 32X memory"
      ios_apply_simple_patch blastem blastem-cartridge-saves.patch \
        rom.db "Continuum: every EEPROM and J-Cart cartridge dump"
      ios_apply_simple_patch blastem blastem-sega-cd-discs.patch \
        libblastem.c "Continuum: Sega CD discs"
      ios_apply_simple_patch blastem blastem-32x-startup.patch \
        32x.c "Continuum: 32X start-up code"
      ;;
    # recipe-end: blastem
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

# recipe-start: parallel_n64
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
# recipe-end: parallel_n64

# recipe-start: parallel_n64
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
# recipe-end: parallel_n64

# recipe-start: azahar
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
# recipe-end: azahar

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
  # This project's own Rust cores live in the repository: nothing to clone or patch.
  if [[ "$IOS_KIND" == "cargo-continuum-gb" ]]; then
    build_ios_continuum_gb_core "$core"
    return
  fi
  # recipe-start: continuum_arcade
  if [[ "$IOS_KIND" == "cargo-continuum-arcade" ]]; then
    build_ios_continuum_arcade_core "$core"
    return
  fi
  # recipe-end: continuum_arcade
  ios_clone "$core"
  ios_apply_core_patches "$core"

  case "$IOS_KIND" in
    make) build_ios_make_core "$core" ;;
    cmake) build_ios_cmake_core "$core" ;;
    cmake-shared) build_ios_cmake_shared_core "$core" ;;
    cmake-ppsspp) build_ios_ppsspp_core "$core" ;;
    cmake-flycast) build_ios_flycast_core "$core" ;;
    cmake-continuum-symbian) build_ios_continuum_symbian_core "$core" ;;
    *) echo "error: $core: unknown iOS build kind '$IOS_KIND'" >&2; exit 1 ;;
  esac
}

# The sha256 of a file, or of stdin when no file is given, as just the hex. shasum on a Mac,
# sha256sum elsewhere; the same answer either way. Fails rather than printing something else.
ios_sha256() {
  local out hex
  if command -v shasum >/dev/null 2>&1; then
    out="$(shasum -a 256 ${1:+"$1"})" || return 1
  else
    out="$(sha256sum ${1:+"$1"})" || return 1
  fi
  hex="${out%% *}"
  [[ "$hex" =~ ^[0-9a-f]{64}$ ]] || return 1
  echo "$hex"
}

# Downloads $1 to $2, and keeps it only if its sha256 is $3. $4 names it in the messages.
#
# An unchecked tarball from a release page is the one place a changed byte upstream could walk
# into the .ipa unnoticed, so nothing here downloads without a pin. A mismatch deletes the file
# and fails rather than building from whatever arrived.
ios_fetch_pinned() {
  local url="$1" dest="$2" want="$3" what="$4" got
  curl -fsSL -o "$dest" "$url" || {
    echo "error: $what could not be downloaded from $url" >&2
    return 1
  }
  got="$(ios_sha256 "$dest")" || {
    echo "error: the sha256 of $what could not be read" >&2
    rm -f "$dest"
    return 1
  }
  [[ "$got" == "$want" ]] || {
    echo "error: $what came down as sha256 $got, not the pinned $want; refusing it" >&2
    rm -f "$dest"
    return 1
  }
  echo "==> $what: sha256 matches the pin ($want)"
}

# THE CACHE'S OWN RULES, as data. These two lines sit in the SHARED part on purpose, so they count:
# changing either one rebuilds every core once. IOS_CACHE_FUNCTIONS is the allow-list of the only
# functions a `# not-a-build-input-start: cache` region may hold (scripts/check-core-cache.sh keeps
# the same list and fails when the two disagree); IOS_CORE_LIST_NAMES names the only arrays a
# `# not-a-build-input-start: lists` region may assign. See "WHAT COUNTS" below.
IOS_CACHE_FUNCTIONS=(ios_usage ios_record_source_version ios_core_patch_family ios_recipe_family
                     ios_cache_scan ios_cache_bash_parse ios_cache_split ios_recipe_text
                     ios_core_fingerprint ios_core_is_cached ios_cache_command)
IOS_CORE_LIST_NAMES=(IOS_CORES IOS_OPTIONAL_CORES)

# not-a-build-input-start: cache
# Which scripts/patches/<family>-*.patch files belong to a core: its id without _jit (a JIT
# build applies its plain build's patches and its own), with Symbian's file prefix spelled
# with a dash.
ios_core_patch_family() {
  local family="${1%_jit}"
  case "$family" in
    continuum_symbian) family="continuum-symbian" ;;
  esac
  echo "$family"
}

# ----------------------------------------------------- THE RECIPE REGIONS (the per-core split)
#
# WHY. The fingerprint below used to hash this WHOLE file, so ANY edit anywhere rebuilt every core
# the cache keeps: 45-50 minutes instead of 6-8. That is the nineteen cores built from source here
# (IOS_CORES and IOS_OPTIONAL_CORES are twenty, and cBoy, the twentieth, is compiled on every run
# anyway); the other sixteen of the thirty-six dylibs in the .ipa are downloaded ready-made and never
# compiled. On 13 October a one-line fix to the checksums of two build tools only the Symbian core
# downloads cost the owner an hour of recompiling the other eighteen, none of which had changed. So
# each core's own recipe is now hashed on its own, and a core is only rebuilt when ITS recipe, or
# something shared, moved.
#
# HOW. Each core's own recipe - its ios_core_config arm, its patches arm, the build function only
# it uses, its own patch helpers - is wrapped in a pair of marker comments:
#
#   # recipe-start: <family>
#   ...that core's lines...
#   # recipe-end: <family>
#
# (written with two `#` here on purpose: with one, this very comment would parse as a real
# marker and unbalance the file.)
#
# A core's fingerprint hashes TWO things: the SHARED remainder - every line that counts (see WHAT
# COUNTS below) outside every region - and its OWN regions. Every line that counts is in exactly
# one of those, or in a not-a-build-input region, so there is nowhere to edit that changes a build
# without changing a fingerprint:
#
#   - a shared change (ios_stage_dylib, build_ios_make_core, ios_clone, the common flags, the
#     required-symbol list, the toolchain) moves the shared hash, so EVERY core rebuilds;
#   - a change inside one family's region moves only that family's recipe hash, so only that
#     core rebuilds - with the JIT or plain build sharing its source, because regions are keyed
#     by FAMILY (the id without `_jit`), the same split scripts/patches/ already uses;
#   - the marker lines are comments, so they count for nothing themselves: adding a new core's
#     regions (with its name in the lists) rebuilds only that core. Code moved INTO a region
#     leaves the shared part, so that move rebuilds everything once; new per-core code someone
#     forgets to mark lands in the shared part and OVER-rebuilds, never the other way round.
#
# WHERE A REGION MAY SIT, checked every time (ios_cache_scan), because a region that held code
# other cores run would let an edit there reach them without moving their fingerprints:
#
#   - whole case arms of ios_core_config, every pattern a core of the region's family, setting
#     nothing but IOS_ variables to plain values (no $, no backquote, no command);
#   - whole case arms of ios_apply_core_patches, the same patterns, holding nothing but
#     `ios_apply_simple_patch <core> <family>-<name>.patch ...` lines and calls to the family's own
#     patch helpers;
#   - whole functions at the top level of the file, used by nothing outside the family's regions
#     except build_ios_core's dispatch for a build kind (IOS_KIND) that only this family sets, and
#     called by name somewhere;
#   - in build_ios_core, the dispatch of such a kind: `<kind>) <function> "$core" ;;` lines, or one
#     `if [[ "$IOS_KIND" == "<kind>" ]]; then <function> "$core"; return; fi` block, so a new core
#     of our own can be added with no shared line at all;
#   and the region's family must be a core in IOS_CORES or IOS_OPTIONAL_CORES. Every patch file a
#   region names must be one of its family's (scripts/patches/<family>-*.patch), and outside the
#   regions only ios_apply_simple_patch and the weekly check's Symbian dry run may name one, so a
#   patch is always in the fingerprint of every core that applies it.
#
# A family's regions are hashed in file order, each after a header saying which region it is,
# where it sits and how many lines it holds (recipe_out in ios_cache_scan), so moving lines from
# one region of a family to another, or moving a region, still changes that family's fingerprint.
#
# WHY MARKED REGIONS AND NOT scripts/core-recipes/<core>.sh. One file per core would hash just as
# well, but it would mean moving ~900 lines of working, Mac-only build code that nothing here can
# compile or test, purely to change which bytes get hashed. Markers move no code at all: the only
# thing that changes is the hash, and that is fully testable from Linux
# (scripts/check-core-cache.sh). The regions can be promoted to files later if this file grows.
#
# NOTHING HERE IS A NUMBER A HUMAN MAINTAINS. Every hash comes from the file's own bytes, so a
# forgotten declaration cannot silently reuse an old dylib.
#
# AND IT FAILS CLOSED, EVERY WAY. A malformed, mistyped, nested, unbalanced or misplaced marker
# makes ios_recipe_text return non-zero, which makes ios_core_fingerprint return no fingerprint at
# all, which writes fp:unknown and rebuilds. A core whose family has no region never gets a
# fingerprint, so a newly added core rebuilds for ever until it is marked - never the other way
# round. If anything at all is in doubt here, the answer is REBUILD.

# The core id with any _jit suffix taken off: the family a recipe region is named for.
ios_recipe_family() { echo "${1%_jit}"; }

# ------------------------------------------------------------- WHAT COUNTS (and what does not)
#
# A core's fingerprint covers the lines of this file that can change how its library is built.
# Four kinds of line are left out, so editing them rebuilds nothing:
#
#   1. COMMENT LINES AND BLANK LINES: a line that is empty, or whose first non-blank character is
#      `#`, when bash reads it as a comment. It does NOT when the line is inside a here-document
#      (any form: <<, <<-, a quoted delimiter), inside a quoted string that runs over several
#      lines ('...', "...", $'...'), right after a line ending in a backslash, or inside $( ),
#      backquotes, <( ) or >( ), (( )) or $(( )), [[ ]], ${ } or an array's ( ). Such lines are
#      content, and they count. A comment at the END of a line of code is part of that line, so
#      it counts too.
#   2. THE CACHE'S OWN CODE, between `# not-a-build-input-start: cache` and
#      `# not-a-build-input-end: cache`: the functions that compute, record and check
#      fingerprints and slice recipes, plus the help text. They decide WHETHER a core is rebuilt,
#      never HOW it is built, and scripts/check-core-cache.sh proves what they decide on every
#      CI run. Only whole definitions of the functions in IOS_CACHE_FUNCTIONS may sit there, and
#      code outside the region may call them only from the places it does today (ios_clone and
#      build_all_ios_cores, each in a subshell so nothing leaks into a build, and the dispatch).
#   3. THE LISTS OF CORES, between `# not-a-build-input-start: lists` and
#      `# not-a-build-input-end: lists`: IOS_CORES=( ... ) and IOS_OPTIONAL_CORES=( ... ), and
#      nothing else, holding nothing but plain core names. Which cores are listed decides which
#      get built, never how one of them is built, so adding or removing a core rebuilds no other.
#   4. The IOS_PIN="<sha>" lines of ios_core_config, as before: each core's pin is hashed on its
#      own line of its fingerprint instead.
#
# HOW THAT IS PROVED, every time a fingerprint is worked out rather than once by a person:
#
#   a. ios_cache_scan (one awk program, plain POSIX so it runs on the Mac's awk too) reads the
#      file the way bash does, character by character: quotes, here-documents, line
#      continuations, $( ), (( )), [[ ]] and the rest. It decides which lines are comments,
#      which region each line is in, and checks every rule above. A construct it cannot follow
#      for certain (a here-document starting inside a string, a case statement inside $( ), a
#      line that looks like a marker inside a here-document, a mistyped marker, a function
#      defined twice, ...) is an error.
#   b. Then BASH ITSELF checks the result (ios_cache_split): it parses the whole file
#      twice, as it is and with every left-out comment and blank line removed, in a separate
#      restricted shell that runs nothing, and the two programs it prints (`declare -f`) must be
#      identical. So a line awk took for a comment that bash would have used is caught by bash.
#   c. Only then are the kept lines hashed. What is hashed is the file's own text, not bash's
#      printout, so a fingerprint does not depend on which version of bash worked it out.
#
# Any error anywhere returns no fingerprint, so every core rebuilds and scripts/check-core-cache.sh
# goes red with the reason. Nothing here can turn a failure into a match.

# Classifies every line of this file and checks every rule in WHAT COUNTS. mode is one of:
#   kept     every line that counts, wherever it is (what bash must parse the same way)
#   shared   the lines that count outside every region: the part every core shares
#   recipe   the lines that count inside the regions of family $2
#   cores    the core names in the lists region, one per line
#   allow    the cache region's allow-list, as IOS_CACHE_FUNCTIONS declares it
#   table    one line per line of the file: number, class, region, function, text (for checks)
#   split    kept, shared and recipe ($2, or * for every family) at once, into the files kept,
#            shared and recipe.<family> in the directory $3: one reading for all of them, so they
#            cannot disagree
# Exits 1, with the reason on stderr, when any rule is broken; CALLERS MUST CHECK THE STATUS.
# No single quote may appear inside the program below: it is one single-quoted argument.
ios_cache_scan() {
  local mode="$1" want="${2:-}" dir="${3:-}"
  awk -v mode="$mode" -v want="$want" -v dir="$dir" '
function fail(msg) { if (bad == "") bad = "line " NR ": " msg }
function failat(n, msg) { if (bad == "") bad = "line " n ": " msg }
function push(c) { sp++; stk[sp] = c }
function pop() { if (sp > 0) sp--; else fail("a closing quote or bracket with nothing open") }
function nested(   k) {
  for (k = 1; k <= sp; k++) if (stk[k] != "Q" && stk[k] != "S" && stk[k] != "D") return 1
  return 0
}
function endword(   t) {
  t = stk[sp]
  if (w == "[[" && (t == "N" || t == "C" || t == "X" || t == "P")) push("T")
  else if (w == "]]" && t == "T") pop()
  else if (w == "case" && (t == "C" || t == "X" || t == "P")) fail("a case statement inside $( ), which this scan does not follow")
  w = ""
}
function heredoc_word(s, i, strip,   n, c, d, q, quoted) {
  n = length(s)
  while (i <= n && (substr(s, i, 1) == " " || substr(s, i, 1) == "\t")) i++
  d = ""; quoted = 0
  while (i <= n) {
    c = substr(s, i, 1)
    if (c == " " || c == "\t" || index(";&|()<>", c)) break
    if (c == SQ) {
      quoted = 1; i++
      while (i <= n && substr(s, i, 1) != SQ) { d = d substr(s, i, 1); i++ }
      if (i > n) { fail("a here-document delimiter with an open quote"); return n }
      i++; continue
    }
    if (c == DQ) {
      quoted = 1; i++
      while (i <= n && substr(s, i, 1) != DQ) {
        q = substr(s, i, 1)
        if (q == BS || q == "$" || q == BT) { fail("a here-document delimiter with an escape or expansion in quotes"); return n }
        d = d q; i++
      }
      if (i > n) { fail("a here-document delimiter with an open quote"); return n }
      i++; continue
    }
    if (c == BS) {
      quoted = 1; i++
      if (i > n) { fail("a here-document delimiter ending in a backslash"); return n }
      d = d substr(s, i, 1); i++; continue
    }
    if (c == "$" || c == BT) { fail("a here-document delimiter with an expansion in it"); return n }
    d = d c; i++
  }
  if (d == "") { fail("a here-document operator with no delimiter"); return n }
  hd_n++; hd_delim[hd_n] = d; hd_strip[hd_n] = strip; hd_quoted[hd_n] = quoted
  return i - 1
}
# One line: updates the context stack, cont (the line ends in a continuation), hd_n (the
# here-documents it opens) and cpos (where a comment starts on it, 0 for none). The contexts:
# N plain, S single quotes, D $-single quotes, Q double quotes, B backquotes, V ${ }, A (( )) or
# $(( )), a a bracket inside A, C $( ), X <( ) or >( ), P a bracket inside C X or R, R an array
# assignment, T [[ ]].
function lex(s,   n, i, c, c2, c3, t) {
  n = length(s); cpos = 0
  # A line continued from the previous one (a backslash at its end) resumes the SAME word bash was
  # building, so a leading # is only a comment when the previous line broke at a word boundary.
  # Carry the pending word and previous character across; otherwise bash would join `x\`+`#y` into
  # the single word `x#y` and run it, while a reset here would read `#y` as a comment and blind
  # every guard that reads the code of the line. Only word contexts need it (strings and
  # here-documents never treat # as a comment).
  if (L_prevcont && stk[sp] != "Q" && stk[sp] != "S" && stk[sp] != "D" && stk[sp] != "B") { w = wcarry; prev = prevcarry }
  else { w = ""; prev = "" }
  wcarry = ""; prevcarry = ""
  for (i = 1; i <= n; i++) {
    c = substr(s, i, 1); c2 = substr(s, i + 1, 1); c3 = substr(s, i + 2, 1); t = stk[sp]
    if (t == "S") { if (c == SQ) pop(); continue }
    if (t == "D") { if (c == BS) { if (i == n) cont = 1; i++; continue }; if (c == SQ) pop(); continue }
    if (t == "B") { if (c == BS) { i++; continue }; if (c == BT) pop(); continue }
    if (t == "Q" || t == "V" || t == "A" || t == "a") {
      if (c == BS) { if (i == n) cont = 1; i++; continue }
      if (t == "Q" && c == DQ) { pop(); continue }
      if (t == "V" && c == "}") { pop(); continue }
      if (t == "A" && c == ")") {
        if (c2 == ")") { pop(); i++; continue }
        fail("an arithmetic expression closed by a single bracket"); return
      }
      if (t == "A" || t == "a") { if (c == "(") { push("a"); continue }; if (t == "a" && c == ")") { pop(); continue } }
      if (t != "Q" && c == SQ) { push("S"); continue }
      if (t != "Q" && c == DQ) { push("Q"); continue }
      if (c == BT) { push("B"); continue }
      if (c == "$") {
        if (t != "Q" && c2 == SQ) { push("D"); i++; continue }
        if (c2 == "(" && c3 == "(") { push("A"); i += 2; continue }
        if (c2 == "(") { push("C"); i++; w = ""; continue }
        if (c2 == "{") { push("V"); i++; continue }
      }
      continue
    }
    # The word contexts (N C X P R T): a # that starts a word starts a comment in all of them.
    if (c == " " || c == "\t") { endword(); prev = c; continue }
    if (c == "#" && w == "") { cpos = i; break }
    if (c == BS) { if (i == n) { cont = 1; wcarry = w; prevcarry = prev; break }; w = w "e"; i++; prev = "e"; continue }
    if (c == SQ) { w = w "q"; push("S"); prev = "q"; continue }
    if (c == DQ) { w = w "q"; push("Q"); prev = "q"; continue }
    if (c == BT) { w = w "q"; push("B"); prev = "q"; continue }
    if (c == "$") {
      if (c2 == SQ) { w = w "q"; push("D"); i++; prev = "q"; continue }
      if (c2 == DQ) { w = w "q"; push("Q"); i++; prev = "q"; continue }
      if (c2 == "(" && c3 == "(") { w = w "q"; push("A"); i += 2; prev = "q"; continue }
      if (c2 == "(") { push("C"); i++; prev = ""; w = ""; continue }
      if (c2 == "{") { w = w "q"; push("V"); i++; prev = "q"; continue }
      w = w c; prev = c; continue
    }
    if (c == "(") {
      # A ( inside [[ ]] is extglob (@( ?( *( +( !( ) or grouping; bash does NOT treat a # inside
      # it as a comment, but endword would clear the word here and the next # would look like one.
      # Nothing in this file needs it, so refuse rather than guess ($( ) is handled above as $).
      if (t == "T") { fail("a ( inside [[ ]] (extglob or grouping), which this scan does not follow for certain"); return }
      if (t != "T" && w ~ /^[A-Za-z_][A-Za-z0-9_]*(\[[^]]*\])?\+?=$/) { w = ""; push("R"); prev = c; continue }
      if (t != "T" && w == "" && (prev == "<" || prev == ">")) { push("X"); prev = ""; continue }
      if (t != "T" && t != "R" && w == "" && c2 == "(") { push("A"); i++; prev = c; continue }
      endword()
      if (t == "C" || t == "X" || t == "P" || t == "R") push("P")
      prev = c; continue
    }
    if (c == ")") {
      endword()
      if (t == "C" || t == "X") { pop(); w = "q" } else if (t == "P" || t == "R") pop()
      prev = c; continue
    }
    if (c == "<" && c2 == "<" && t != "T") {
      endword()
      if (c3 == "<") { i += 2; prev = "<"; continue }
      if (t == "R") { fail("a here-document inside an array assignment"); return }
      i += 2
      if (substr(s, i, 1) == "-") i = heredoc_word(s, i + 1, 1); else i = heredoc_word(s, i, 0)
      prev = "w"; continue
    }
    if (index(";&|<>", c)) { endword(); prev = c; continue }
    w = w c; prev = c
  }
  if (!cont) endword()
}
function trim(s) { sub(/^[ \t]+/, "", s); sub(/[ \t]+$/, "", s); return s }
function family(core) { sub(/_jit$/, "", core); return core }
# 1 when name appears in s as a whole word, not inside a longer identifier.
function hasword(s, name,   p, b, a) {
  while ((p = index(s, name)) > 0) {
    b = (p == 1) ? "" : substr(s, p - 1, 1)
    a = substr(s, p + length(name), 1)
    if (b !~ /[A-Za-z0-9_]/ && a !~ /[A-Za-z0-9_]/) return 1
    s = substr(s, p + length(name))
  }
  return 0
}
function kept(n) { return L_c[n] != "blank" && L_c[n] != "comment" }
function pinline(n) { return L_f[n] == "ios_core_config" && L_c[n] == "code" && L_t[n] ~ /^[ \t]*IOS_PIN="[0-9a-f]*"[ \t]*$/ }
function isshared(n) { return kept(n) && L_b[n] == "shared" && !pinline(n) }
# The regions of a family, in file order, each after a header saying which region it is, where it
# sits (how many shared lines that count come before it) and how many lines it holds: so moving a
# line from one region of a family to another, or a region to another place, changes the hash.
function recipe_out(fam, dest,   k, n, ord, cnt) {
  ord = 0
  for (k = 1; k <= ninst; k++) {
    if (ikind[k] != "r" || iname[k] != fam) continue
    ord++; cnt = 0
    for (n = istart[k] + 1; n < iend[k]; n++) if (kept(n) && !pinline(n)) cnt++
    out("@@ region " ord " of " fam ", after shared line " shbefore[istart[k]] ", " cnt " lines", dest)
    for (n = istart[k] + 1; n < iend[k]; n++) if (kept(n) && !pinline(n)) out(L_t[n], dest)
  }
}
function out(s, dest) { if (dest == "") print s; else print s > dest }
function patchprefix(fam) { return (fam == "continuum_symbian") ? "continuum-symbian" : fam }
function ownkind(kd, fam) { return (kd in kfam) && kfam[kd] == fam && !(kd in kmulti) }
# Line n of build_ios_core calls f the way the dispatch does, for a kind only family fam sets:
# "<kind>) f "$core" ;;", or f "$core" right after if (or elif) [[ "$IOS_KIND" == "<kind>" ]]; then.
function dispatch_ok(n, f, fam,   s, kd, p) {
  s = trim(L_k[n])
  if (s ~ /^[a-z0-9-]+\) [A-Za-z_][A-Za-z0-9_]* "\$core" ;;$/) {
    kd = s; sub(/\).*/, "", kd)
    return s == kd ") " f " \"$core\" ;;" && ownkind(kd, fam)
  }
  if (s != f " \"$core\"") return 0
  for (p = n - 1; p > 0 && !kept(p); p--) continue
  if (p < 1 || trim(L_k[p]) !~ /^(el)?if \[\[ "\$IOS_KIND" == "[a-z0-9-]+" \]\]; then$/) return 0
  kd = trim(L_k[p]); sub(/^(el)?if \[\[ "\$IOS_KIND" == "/, "", kd); sub(/".*/, "", kd)
  return ownkind(kd, fam)
}
# The words of the top-level declaration NAME=( ... ) in the shared part, into arr; returns how
# many, or -1 when it is missing, declared twice, or not plain identifiers.
function decl(name, arr,   n, txt, cnt, m, k, e, el) {
  cnt = 0; txt = ""
  for (n = 1; n <= last; n++) {
    if (!kept(n) || L_d[n] != name) continue
    if (L_b[n] != "shared" || L_f[n] != "") { failat(n, name " must be declared in the shared part, outside any function"); return -1 }
    if (L_c[n] == "code") cnt++
    txt = txt " " L_t[n]
  }
  if (cnt != 1) { fail(name "=( ... ) must be declared exactly once, at the top level"); return -1 }
  sub(/^[ \t]*[A-Z_]+=\(/, "", txt); sub(/\)[ \t]*$/, "", txt)
  if (txt !~ /^[a-zA-Z0-9_ \t]*$/) { fail(name " must hold plain names only"); return -1 }
  m = split(txt, el, /[ \t]+/); k = 0
  for (e = 1; e <= m; e++) if (el[e] != "") { k++; arr[k] = el[e] }
  return k
}
BEGIN {
  SQ = sprintf("%c", 39); DQ = "\""; BS = "\\"; BT = "`"
  sp = 0; stk[0] = "N"; hd_cur = 0; hd_total = 0
  rname = ""; xname = ""; ninst = 0; inst = 0; fname = ""; indecl = ""
  # The only places outside the cache region allowed to call a function inside it: the function
  # the call sits in, a separator, and the whole line.
  approved["ios_clone" SUBSEP "( ios_record_source_version \"$core\" )"] = 1
  approved["build_all_ios_cores" SUBSEP "if ( ios_core_is_cached \"$core\" ); then"] = 1
  approved["" SUBSEP "ios_cache_command \"$@\""] = 1
  approved["" SUBSEP "ios_usage"] = 1
  approved["" SUBSEP "ios_usage >&2"] = 1
  # Comments and blank lines only count for nothing if nothing outside the cache reads this
  # file, its text or its line numbers. These are the only lines that may use those words.
  nself = split("IOS_SELF BASH_SOURCE LINENO BASH_LINENO BASH_ARGV0 caller", selfword, " ")
  selfok["" SUBSEP "ROOT=\"$(cd \"$(dirname \"${BASH_SOURCE[0]}\")/..\" && pwd)\""] = 1
  selfok["" SUBSEP "IOS_SELF=\"$(cd \"$(dirname \"${BASH_SOURCE[0]}\")\" && pwd)/$(basename \"${BASH_SOURCE[0]}\")\""] = 1
  selfok["build_all_ios_cores" SUBSEP "if bash \"$IOS_SELF\" ios \"$core\"; then"] = 1
  # The only shared lines that may name a file in scripts/patches/: the one helper every patch
  # step goes through, and the dry run of the Symbian patch for the weekly update check.
  patchok["ios_apply_simple_patch" SUBSEP "local patch=\"$ROOT/scripts/patches/$name\""] = 1
  patchok["" SUBSEP "git -C \"$IOS_SRC_DIR\" apply --check \"$ROOT/scripts/patches/continuum-symbian-frame-readback.patch\""] = 1
}
{
  line = $0; cpos = 0
  if (hd_cur > 0) {
    cls = "heredoc"
    t = line
    if (hd_strip[hd_cur]) sub(/^\t+/, "", t)
    if (t == hd_delim[hd_cur]) {
      hd_cur++
      if (hd_cur > hd_total) { hd_cur = 0; hd_total = 0 }
    } else if (!hd_quoted[hd_cur] && line ~ /\\$/) {
      t = line; sub(/\\+$/, "", t)
      if ((length(line) - length(t)) % 2 == 1) fail("a here-document line ending in a backslash, which joins it to the next line")
    }
  } else {
    top = stk[sp]
    if (cont) start = "cont"
    else if (top == "Q" || top == "S" || top == "D") start = "string"
    else if (nested()) start = "nested"
    else start = "plain"
    L_prevcont = cont; cont = 0; hd_n = 0
    lex(line)
    if (start == "plain") {
      if (line ~ /^[ \t]*$/) cls = "blank"
      else if (NR == 1 && line ~ /^#!/) cls = "code"
      else if (line ~ /^[ \t]*#/) cls = "comment"
      else cls = "code"
    } else cls = start
    if (hd_n > 0) {
      top = stk[sp]
      if (cont || !(top == "N" || top == "C" || top == "X" || top == "P")) fail("a here-document starts on a line that does not end plainly")
      hd_total = hd_n; hd_cur = 1
    }
  }
  # Region markers: comment lines read in the plain state only, spelled exactly.
  ismarker = 0
  looks = (tolower(line) ~ /^[ \t]*#+[ \t]*(recipe|not-a-build-input)[-_ ]*(start|end)/)
  if (cls == "comment" && looks) {
    if ($1 == "#" && NF == 3 && ($2 == "recipe-start:" || $2 == "recipe-end:") && $3 ~ /^[a-z][a-z0-9_]*$/) ismarker = 1
    else if ($1 == "#" && NF == 3 && ($2 == "not-a-build-input-start:" || $2 == "not-a-build-input-end:") && ($3 == "cache" || $3 == "lists")) ismarker = 2
    else fail("a mistyped region marker; it must be exactly # recipe-start: <family>, # recipe-end: <family>, # not-a-build-input-start: cache|lists or # not-a-build-input-end: cache|lists")
  } else if (looks) fail("a line that looks like a region marker sits inside a here-document, a string or a continued line")
  if (ismarker) {
    if ($2 ~ /-start:$/) {
      if (rname != "" || xname != "") fail("a region opens inside another region")
      ninst++; ikind[ninst] = (ismarker == 1) ? "r" : "x"; iname[ninst] = $3; istart[ninst] = NR; inst = ninst
      if (ismarker == 1) rname = $3; else xname = $3
    } else {
      if (inst == 0 || iname[inst] != $3 || ikind[inst] != ((ismarker == 1) ? "r" : "x")) fail($2 " " $3 " does not close the region that is open")
      iend[inst] = NR; rname = ""; xname = ""; inst = 0
    }
    L_b[NR] = "marker"
  } else if (inst > 0) L_b[NR] = ikind[inst] ":" iname[inst]
  else L_b[NR] = "shared"
  L_i[NR] = ismarker ? 0 : inst
  # Top-level functions: name() { at the start of a line, closed by a } alone at the start of a line.
  if (cls == "code") {
    code = (cpos > 0) ? substr(line, 1, cpos - 1) : line
    isdef = (code ~ /^[A-Za-z_][A-Za-z0-9_]*\(\)[ \t]*\{/)
    if (code ~ /^[ \t]*function[ \t]/ || code ~ /^[A-Za-z_][A-Za-z0-9_]*[ \t]+\(\)/ || (code ~ /^[A-Za-z_][A-Za-z0-9_]*\(\)/ && !isdef))
      fail("a function written another way; this file writes them as name() { ... }")
    if (fname == "") {
      if (isdef) {
        f = code; sub(/\(.*/, "", f)
        if (f in fstart) fail("function " f " is defined twice, so the second would silently replace the first")
        fstart[f] = NR; L_f[NR] = f
        if (trim(code) ~ /\{$/) fname = f
        else if (trim(code) ~ /\}$/) fend[f] = NR
        else fail("a function definition line that ends with neither { nor }")
      } else {
        if (code == "}") fail("a } at the start of a line outside any function")
        L_f[NR] = ""
      }
    } else {
      L_f[NR] = fname
      if (isdef) fail("function " fname " has no } at the start of a line before the next function")
      if (code == "}") { fend[fname] = NR; fname = "" }
    }
  } else L_f[NR] = fname
  # The two declarations of the rules themselves, which may span lines.
  if (cls == "code" && line ~ /^(IOS_CACHE_FUNCTIONS|IOS_CORE_LIST_NAMES)=\(/) { indecl = line; sub(/=.*/, "", indecl) }
  L_d[NR] = indecl
  if (indecl != "" && sp == 0) indecl = ""
  L_c[NR] = cls; L_t[NR] = line
  # The code part of the line, without a comment at its end: what the rules below read. (What is
  # hashed is still the whole line, so editing such a comment still counts.)
  L_k[NR] = (cpos > 0) ? substr(line, 1, cpos - 1) : line
}
END {
  last = NR
  if (hd_cur > 0) fail("a here-document with no closing line")
  if (sp > 0) fail("an open quote or bracket at the end of the file")
  if (cont) fail("a line continuation at the end of the file")
  if (fname != "") fail("function " fname " never closes")
  if (inst > 0) fail("region " iname[inst] " never closes")
  nallow = decl("IOS_CACHE_FUNCTIONS", allowed)
  for (k = 1; k <= nallow; k++) isallowed[allowed[k]] = 1
  nlistn = decl("IOS_CORE_LIST_NAMES", lnames)
  for (k = 1; k <= nlistn; k++) islistname[lnames[k]] = 1
  # The lists region: only NAME=( core names ) for the names in IOS_CORE_LIST_NAMES.
  ncore = 0
  for (k = 1; k <= ninst; k++) {
    if (ikind[k] != "x" || iname[k] != "lists") continue
    txt = ""
    for (n = istart[k] + 1; n < iend[k]; n++) if (kept(n)) txt = txt " " L_t[n]
    while (txt ~ /[^ \t]/) {
      if (!match(txt, /^[ \t]*[A-Z_][A-Z0-9_]*=\([a-z0-9_ \t]*\)/)) { failat(istart[k], "the lists region holds something other than array assignments of plain core names"); break }
      one = substr(txt, RSTART, RLENGTH); txt = substr(txt, RSTART + RLENGTH)
      an = one; sub(/^[ \t]*/, "", an); sub(/=.*/, "", an)
      if (!(an in islistname)) { failat(istart[k], "the lists region assigns " an ", which is not in IOS_CORE_LIST_NAMES"); break }
      if (an in listseen) { failat(istart[k], an " is assigned twice in the lists region"); break }
      listseen[an] = 1
      body = one; sub(/^[^(]*\(/, "", body); sub(/\)$/, "", body)
      m = split(body, el, /[ \t]+/)
      for (e = 1; e <= m; e++) if (el[e] != "") {
        if (el[e] !~ /^[a-z][a-z0-9_]*$/) { failat(istart[k], "not a plain core name: " el[e]); break }
        ncore++; cores[ncore] = el[e]; listfam[family(el[e])] = 1
      }
    }
  }
  for (k = 1; k <= nlistn; k++) if (!(lnames[k] in listseen)) fail("no " lnames[k] "=( ... ) inside a not-a-build-input: lists region")
  # The cache region: nothing but whole definitions of allow-listed functions.
  for (k = 1; k <= ninst; k++) {
    if (ikind[k] != "x" || iname[k] != "cache") continue
    for (n = istart[k] + 1; n < iend[k]; n++) {
      if (!kept(n)) continue
      f = L_f[n]
      if (f == "") { failat(n, "code outside any function inside the cache region"); break }
      if (!(f in isallowed)) { failat(n, "function " f " is inside the cache region but not in IOS_CACHE_FUNCTIONS"); break }
      if (fstart[f] < istart[k] || fend[f] > iend[k]) { failat(n, "function " f " is only partly inside the cache region"); break }
    }
  }
  # And nothing outside it calls into it except from the approved places, or reads this file.
  for (n = 1; n <= last; n++) {
    if (!kept(n) || L_b[n] == "x:cache" || L_d[n] != "") continue
    for (k = 1; k <= nallow; k++) if (hasword(L_k[n], allowed[k]) && !((L_f[n] SUBSEP trim(L_k[n])) in approved))
      failat(n, allowed[k] " is cache machinery, so it may not be used here (only ios_clone, build_all_ios_cores and the dispatch may call it, as they do today)")
    for (k = 1; k <= nself; k++) if (hasword(L_k[n], selfword[k]) && !((L_f[n] SUBSEP trim(L_k[n])) in selfok))
      failat(n, selfword[k] " outside the cache: a build that read this file or its line numbers would see comments, so they could not be left out")
    if ((L_k[n] ~ /\$0([^0-9]|$)/ || L_k[n] ~ /\$\{0[^0-9A-Za-z_]/) && !((L_f[n] SUBSEP trim(L_k[n])) in selfok))
      failat(n, "$0 outside the cache: a build that read the script path could see comments, so they could not be left out")
  }
  # Who owns what, for the rules below: the family each IOS_KIND belongs to (kmulti: more than
  # one), the family each function defined inside a recipe region belongs to, and how many shared
  # lines that count come before each line (where a region sits).
  for (n = 1; n <= last; n++) {
    if (L_b[n] ~ /^r:/ && L_f[n] == "ios_core_config" && L_c[n] == "code" && L_k[n] ~ /^[ \t]*IOS_KIND="[^"]+"[ \t]*$/) {
      kd = trim(L_k[n]); sub(/^IOS_KIND="/, "", kd); sub(/"$/, "", kd); fam = substr(L_b[n], 3)
      if ((kd in kfam) && kfam[kd] != fam) kmulti[kd] = 1
      kfam[kd] = fam
    }
  }
  for (f in fstart) if (L_b[fstart[f]] ~ /^r:/) rfun[f] = substr(L_b[fstart[f]], 3)
  cnt = 0
  for (n = 1; n <= last; n++) { shbefore[n] = cnt; if (kept(n) && L_b[n] == "shared") cnt++ }
  # Recipe regions: whole case arms of ios_core_config or ios_apply_core_patches, whole functions,
  # or the dispatch of a build kind only this family sets, inside build_ios_core.
  for (k = 1; k <= ninst; k++) {
    if (ikind[k] != "r") continue
    F = iname[k]
    if (!(F in listfam)) { failat(istart[k], "recipe region " F " belongs to no core in IOS_CORES or IOS_OPTIONAL_CORES, so no fingerprint would cover it"); continue }
    kind = ""; first = 0; lastk = 0
    for (n = istart[k] + 1; n < iend[k]; n++) {
      if (!kept(n)) continue
      if (!first) first = n
      lastk = n; f = L_f[n]
      if (f == "ios_core_config" || f == "ios_apply_core_patches" || f == "build_ios_core") lk = f
      else if (f != "" && fstart[f] > istart[k] && fend[f] < iend[k]) lk = "functions"
      else { failat(n, "recipe region " F " holds code that is neither a whole case arm nor a whole function"); break }
      if (kind == "") kind = lk
      else if (kind != lk) { failat(n, "recipe region " F " mixes case arms and functions"); break }
    }
    if (kind == "ios_core_config" || kind == "ios_apply_core_patches") {
      if (L_k[first] !~ /^[ \t]*[a-z][a-z0-9_]*([ \t]*\|[ \t]*[a-z][a-z0-9_]*)*\)[ \t]*$/) failat(first, "recipe region " F " must start with its case arm pattern")
      if (trim(L_k[lastk]) != ";;") failat(lastk, "recipe region " F " must end with the ;; of its case arm")
      for (n = first; n <= lastk; n++) {
        if (!kept(n)) continue
        s = trim(L_k[n])
        if (s ~ /^[a-z][a-z0-9_]*([ \t]*\|[ \t]*[a-z][a-z0-9_]*)*\)$/) {
          sub(/\)$/, "", s); m = split(s, el, /[ \t]*\|[ \t]*/)
          for (e = 1; e <= m; e++) if (family(el[e]) != F) failat(n, "the case arm for " el[e] " sits in recipe region " F)
          continue
        }
        if (s == ";;") continue
        # A config arm only sets IOS_ variables to plain values: no expansion, no command, nothing
        # it could leave behind for another core (ios_core_config runs for every core in turn).
        if (kind == "ios_core_config") {
          if (L_c[n] == "code" && L_k[n] ~ /^[ \t]*IOS_[A-Z0-9_]+=("[^"$`\\]*"|[A-Za-z0-9_.\/:@+,=%-]*)[ \t]*$/) continue
          if (L_c[n] == "code" && L_k[n] ~ /^[ \t]*IOS_[A-Z0-9_]+=\(([ \t]*("[^"$`\\]*"|[A-Za-z0-9_.\/:@+,=%-]+))*[ \t]*\)?[ \t]*$/) continue
          if (L_c[n] == "nested" && L_k[n] ~ /^[ \t]*(("[^"$`\\]*"|[A-Za-z0-9_.\/:@+,=%-]+)[ \t]*)*\)?[ \t]*$/) continue
          failat(n, "a config arm in recipe region " F " may only set IOS_ variables to plain values: " s)
          continue
        }
        # A patches arm holds its own patches and nothing else (the K2 guard): calls to
        # ios_apply_simple_patch for a patch of this family, with their continuation lines, and
        # calls to the own patch helpers of this family. A continuation line is the tail of such a
        # call (its file and marker); it may not chain a second command, or a `; ios_apply...` on
        # the second line of a two-line call would apply another patch unseen.
        if (L_c[n] == "cont") {
          if (index(trim(L_k[n]), ";") > 0 || index(trim(L_k[n]), "&") > 0 || index(trim(L_k[n]), "|") > 0 || hasword(L_k[n], "ios_apply_simple_patch"))
            failat(n, "a continuation line in the patches arm of recipe region " F " runs more than one patch step: " trim(L_k[n]))
          continue
        }
        if (s ~ /^ios_apply_simple_patch [a-z][a-z0-9_]* [A-Za-z0-9_.+-]+\.patch( |$)/) {
          split(s, el, " ")
          if (family(el[2]) != F) failat(n, "ios_apply_simple_patch in recipe region " F " names the core " el[2])
          if (index(el[3], patchprefix(F) "-") != 1) failat(n, el[3] " is not a patch of " F " (scripts/patches/" patchprefix(F) "-*.patch), so editing it would not rebuild " F)
          continue
        }
        if (s ~ /^[A-Za-z_][A-Za-z0-9_]*$/ && (s in rfun) && rfun[s] == F) continue
        failat(n, "the patches arm in recipe region " F " holds something other than the patch steps of " F ": " s)
      }
    }
    if (kind == "build_ios_core") {
      nl = 0
      for (n = first; n <= lastk; n++) if (kept(n)) { nl++; dl[nl] = trim(L_k[n]) }
      if (dl[1] ~ /^if \[\[ "\$IOS_KIND" == "[a-z0-9-]+" \]\]; then$/) {
        kd = dl[1]; sub(/^if \[\[ "\$IOS_KIND" == "/, "", kd); sub(/".*/, "", kd)
        f = dl[2]; sub(/ .*/, "", f)
        if (nl != 4 || dl[2] != f " \"$core\"" || dl[3] != "return" || dl[4] != "fi" || !ownkind(kd, F) || rfun[f] != F)
          failat(first, "recipe region " F " in build_ios_core may only hold: if [[ \"$IOS_KIND\" == \"<a kind only " F " sets>\" ]]; then <a function of " F "> \"$core\"; return; fi")
      } else {
        for (e = 1; e <= nl; e++) {
          kd = dl[e]; sub(/\).*/, "", kd); f = dl[e]; sub(/^[^ ]* /, "", f); sub(/ .*/, "", f)
          if (dl[e] != kd ") " f " \"$core\" ;;" || kd !~ /^[a-z0-9-]+$/ || !ownkind(kd, F) || rfun[f] != F) {
            failat(first, "recipe region " F " in build_ios_core may only hold <a kind only " F " sets>) <a function of " F "> \"$core\" ;; lines"); break
          }
        }
      }
    }
  }
  # No patches arm outside a recipe region, and no patch step anywhere but in one: a patch step in
  # the shared part cannot be told apart from one for every core, and one in the region of another
  # family would not rebuild its core when it changed. (A config arm left outside every region only
  # costs that core its own fingerprint, so it rebuilds every time: ios_core_fingerprint refuses a
  # recipe that does not set its dylib name.)
  for (n = 1; n <= last; n++) {
    if (!kept(n) || L_b[n] != "shared") continue
    s = trim(L_k[n])
    if (L_f[n] == "ios_apply_core_patches" && s ~ /^[a-z][a-z0-9_]*([ \t]*\|[ \t]*[a-z][a-z0-9_]*)*\)$/)
      failat(n, "the ios_apply_core_patches arm " s " is outside its recipe region")
    if (hasword(s, "ios_apply_simple_patch") && n != fstart["ios_apply_simple_patch"])
      failat(n, "ios_apply_simple_patch is used outside a recipe region: a patch step belongs in its core region")
  }
  # The family each Continuum patch prefix belongs to (azahar- -> azahar, continuum-symbian- ->
  # continuum_symbian). A patch file this project ships is named <family>-<something>.patch, and
  # that name is how ios_core_fingerprint finds it, so the name is the one spelling that cannot be
  # faked: to apply the real patch of a core you must name its file.
  for (fff in listfam) patchfam[patchprefix(fff)] = fff
  # Every Continuum patch (scripts/patches/<family>-*.patch) named outside the cache must belong to
  # the right family, HOWEVER it is spelled: a bare filename, a $VAR path, a continuation line or a
  # patch helper all carry the file name, so this keys on the name, not on the scripts/patches
  # path. That shuts the ways a patches arm, a helper or a continuation line could apply the patch
  # of another family (covered by the fingerprint of that family, not this one) and ship it stale.
  # An upstream-tree patch that the source of a core ships (cubeb.patch, fmt.patch ...) carries no
  # core family prefix and is covered by the pin, so it is left alone.
  for (n = 1; n <= last; n++) {
    if (!kept(n) || L_b[n] == "x:cache") continue
    rest = L_k[n]
    while (match(rest, /[A-Za-z0-9_.+-]+\.patch/)) {
      tok = substr(rest, RSTART, RLENGTH); rest = substr(rest, RSTART + RLENGTH)
      pf = ""
      for (pp in patchfam) if (index(tok, pp "-") == 1 && length(pp) > length(pf)) pf = pp
      if (pf == "") continue
      if (L_b[n] == "shared") {
        if (!((L_f[n] SUBSEP trim(L_k[n])) in patchok)) { failat(n, "the patch " tok " is named in the shared part, so editing it would not rebuild every core that uses it"); break }
      } else if (L_b[n] ~ /^r:/) {
        F = substr(L_b[n], 3)
        if (patchfam[pf] != F) { failat(n, "recipe region " F " names " tok ", a patch of " patchfam[pf] ", so a fix to it would not rebuild " F); break }
      }
    }
  }
  # A line that points at the scripts/patches directory but names no family-prefixed patch file on
  # the same line (so the rule above cannot attribute it) is refused in a region or the shared
  # part alike: it is the shape of a build step reaching patches through a variable, exactly how a
  # cross-family patch would be smuggled past the name rule.
  for (n = 1; n <= last; n++) {
    if (!kept(n) || L_b[n] == "x:cache" || index(L_k[n], "scripts/patches") == 0) continue
    if ((L_f[n] SUBSEP trim(L_k[n])) in patchok) continue
    if (L_k[n] ~ /scripts\/patches\/[A-Za-z0-9_.+-]+\.patch/) continue
    failat(n, "a line names the scripts/patches directory with no patch file this scan can attribute to a family; name the full scripts/patches/<family>-*.patch path")
  }
  # A function of a recipe region is for that family alone: only its regions may use it, or the
  # dispatch in build_ios_core for a kind only that family sets. Every one must be called by name,
  # since a function reached any other way could not be checked.
  for (f in rfun) {
    F = rfun[f]; uses = 0
    for (n = 1; n <= last; n++) {
      if (!kept(n) || L_b[n] == "x:cache" || n == fstart[f] || !hasword(L_k[n], f)) continue
      uses++
      if (L_b[n] == "r:" F || (L_f[n] == "build_ios_core" && dispatch_ok(n, f, F))) continue
      failat(n, f " belongs to recipe region " F ", so only the regions of " F " may use it (or the dispatch in build_ios_core, for a kind only " F " sets)")
    }
    if (uses == 0) failat(fstart[f], f " in recipe region " F " is never called by name, and a function reached any other way cannot be checked")
  }
  if ((mode == "recipe" || (mode == "split" && want != "" && want != "*")) && bad == "") {
    found = 0
    for (k = 1; k <= ninst; k++) if (ikind[k] == "r" && iname[k] == want) found = 1
    if (!found) fail("there is no recipe region named " want)
  }
  if (bad != "") { print "error: " bad > "/dev/stderr"; exit 1 }
  if (mode == "table") { for (n = 1; n <= last; n++) printf "%d\t%s\t%s\t%s\t%s\n", n, L_c[n], L_b[n], L_f[n], L_t[n]; exit 0 }
  if (mode == "kept") { for (n = 1; n <= last; n++) if (kept(n)) print L_t[n]; exit 0 }
  if (mode == "shared") { for (n = 1; n <= last; n++) if (isshared(n)) print L_t[n]; exit 0 }
  if (mode == "recipe") { recipe_out(want, ""); exit 0 }
  if (mode == "split") {
    printf "" > (dir "/kept"); printf "" > (dir "/shared")
    for (n = 1; n <= last; n++) {
      if (kept(n)) print L_t[n] > (dir "/kept")
      if (isshared(n)) print L_t[n] > (dir "/shared")
    }
    close(dir "/kept"); close(dir "/shared")
    # One file per family asked for (* for every family), each closed before the next is opened.
    for (k = 1; k <= ninst; k++) {
      if (ikind[k] != "r" || (want != "*" && iname[k] != want) || (iname[k] in written)) continue
      fam = iname[k]; written[fam] = 1
      printf "" > (dir "/recipe." fam)
      recipe_out(fam, dir "/recipe." fam)
      close(dir "/recipe." fam)
    }
    exit 0
  }
  if (mode == "cores") { for (k = 1; k <= ncore; k++) print cores[k]; exit 0 }
  if (mode == "allow") { for (k = 1; k <= nallow; k++) print allowed[k]; exit 0 }
  print "error: ios_cache_scan has no mode " mode > "/dev/stderr"
  exit 1
}
' "$IOS_SELF"
}

# What bash itself makes of file $1: its parse, printed back by `declare -f`. Prints nothing and
# returns 1 unless bash parses it as a complete script (`bash -n`, which runs nothing) AND as the
# body of one function. The second parse runs in a separate, restricted shell with an empty
# environment and no PATH, so even a file that went wrong could not run a command; and the -n
# check first means the file cannot close that function early and run the rest of itself.
ios_cache_bash_parse() {
  local out
  # A NUL byte would end the text bash is handed early, so a file holding one is never trusted.
  LC_ALL=C tr -d '\000' < "$1" | cmp -s - "$1" || return 1
  "$BASH" -n "$1" 2>/dev/null || return 1
  out="$(env -i PATH=/nonexistent-continuum "$BASH" --norc --noprofile -r -c '
IFS= read -r -d "" text
eval "__continuum_parse_check__() {
$text
__continuum_parse_check_end__
}" || exit 1
declare -f __continuum_parse_check__' < "$1" 2>/dev/null)" || return 1
  [[ "$out" == "__continuum_parse_check__ () "* ]] || return 1
  [[ "$out" == *"__continuum_parse_check_end__"$'\n'"}" ]] || return 1
  printf '%s\n' "$out"
}

# One reading of this file, checked and split (steps a and b of WHAT COUNTS): writes the files
# kept, shared and recipe.<family> (for family $1; every family for *, none for "") into
# directory $2.
# Returns 0 only when every rule holds AND bash parses this script, and the script with its
# left-out lines removed, into the same program. Anything else, a missing tool included, is a 1.
ios_cache_split() {
  local family="$1" dir="$2" whole cut
  ios_cache_scan split "$family" "$dir" || return 1
  whole="$(ios_cache_bash_parse "$IOS_SELF")" || return 1
  cut="$(ios_cache_bash_parse "$dir/kept")" || return 1
  [[ -n "$whole" && "$whole" == "$cut" ]] || {
    echo "error: bash reads $IOS_SELF differently once its comment and blank lines are taken out, so something taken for a comment is not one" >&2
    return 1
  }
}

# mode=shared prints the lines that count outside every region; mode=recipe prints the lines that
# count inside every region named $1, in file order (a family can own several regions: its config
# arm, its patches arm, its build function). See WHAT COUNTS for which lines count. IOS_PIN= lines
# of ios_core_config are left out of both, because each pin is hashed on its own.
#
# Returns 1 when any rule of WHAT COUNTS is broken, when bash does not confirm what was left out,
# or when mode=recipe and no region of that name exists. CALLERS MUST CHECK THE STATUS.
ios_recipe_text() {
  local want="$1" mode="$2" dir rc=1
  [[ "$mode" == "shared" ]] && want=""
  dir="$(mktemp -d "${TMPDIR:-/tmp}/continuum-cache.XXXXXX")" || return 1
  if ios_cache_split "$want" "$dir"; then
    if [[ "$mode" == "shared" ]]; then cat "$dir/shared" && rc=0; else cat "$dir/recipe.$want" && rc=0; fi
  fi
  rm -rf "$dir"
  return "$rc"
}

# The first 16 hex of a sha256 over everything a core's dylib is made from besides the upstream
# tree itself (the note above ios_core_is_cached says what and why). Any host. Every step can
# fail the function, so a missing tool - or a recipe region that cannot be located, or a broken
# rule of WHAT COUNTS - means NO fingerprint, never a partial one, and therefore a rebuild.
#
# $2, only from ios-cache-fingerprints: a directory ios_cache_split already wrote for every family,
# so the checks can fingerprint every core from one reading. Every build path passes nothing and
# gets a fresh, checked reading of its own.
ios_core_fingerprint() {
  local core="$1" split="${2:-}"
  ios_core_config "$core" || return 1
  local family rfamily dir shared recipe lines name sum path files
  family="$(ios_core_patch_family "$core")"
  rfamily="$(ios_recipe_family "$core")"
  # TWO hashes of this script, not one: the part every core shares, and this core's own recipe
  # regions, both from ONE checked reading of the file (ios_cache_split). Every line that counts
  # is in exactly one of them (or in a not-a-build-input region), so nothing that builds a core is
  # left uncovered, and a change to another core's recipe cannot move this fingerprint. Any
  # failure returns NO fingerprint, which rebuilds.
  shared="" recipe=""
  if [[ -n "$split" ]]; then
    dir="$split"
  else
    dir="$(mktemp -d "${TMPDIR:-/tmp}/continuum-cache.XXXXXX")" || return 1
    ios_cache_split "$rfamily" "$dir" || { rm -rf "$dir"; return 1; }
  fi
  # The slice has to be THIS core's recipe: only its ios_core_config arm sets its dylib name.
  # A slice without that line means a marker is in the wrong place, so refuse to fingerprint
  # rather than hash some other core's recipe and then reuse a stale dylib.
  if [[ -s "$dir/shared" && -s "$dir/recipe.$rfamily" ]] \
      && grep -qF "IOS_DYLIB_NAME=\"$IOS_DYLIB_NAME\"" "$dir/recipe.$rfamily"; then
    shared="$(ios_sha256 "$dir/shared")" || shared=""
    recipe="$(ios_sha256 "$dir/recipe.$rfamily")" || recipe=""
  fi
  [[ -n "$split" ]] || rm -rf "$dir"
  [[ -n "$shared" && -n "$recipe" ]] || return 1
  lines="core $core"$'\n'"pin $IOS_PIN"$'\n'"toolchain ${CONTINUUM_CORE_TOOLCHAIN:-local}"$'\n'
  lines="${lines}shared $shared"$'\n'"recipe $rfamily $recipe"$'\n'
  files="$(cd "$ROOT/scripts/patches" 2>/dev/null && for name in "$family"-*.patch; do
             if [[ -f "$name" ]]; then echo "$name"; fi
           done | LC_ALL=C sort)" || return 1
  if [[ -n "$files" ]]; then
    while IFS= read -r name; do
      sum="$(ios_sha256 "$ROOT/scripts/patches/$name")" || return 1
      lines="${lines}patch $name $sum"$'\n'
    done <<<"$files"
  fi
  if [[ "$core" == "continuum_symbian" ]]; then
    files="$(git -C "$ROOT" ls-files -- native/continuum-symbian 2>/dev/null || true)"
    if [[ -z "$files" ]]; then
      files="$(cd "$ROOT" && find native/continuum-symbian -path native/continuum-symbian/build -prune \
                 -o -type f -print | LC_ALL=C sort)" || return 1
    fi
    [[ -n "$files" ]] || return 1
    while IFS= read -r path; do
      sum="$(ios_sha256 "$ROOT/$path")" || return 1
      lines="${lines}plug $path $sum"$'\n'
    done <<<"$files"
    sum="$(ios_sha256 "$ROOT/scripts/fetch-libretro-headers.sh")" || return 1
    lines="${lines}headers $sum"$'\n'
  fi
  sum="$(printf '%s' "$lines" | ios_sha256)" || return 1
  echo "${sum:0:16}"
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
# (That note belongs to build_all_ios_cores, further down.)
#
# Is this core's staged dylib already here AND recorded as built from the commit it is
# pinned to? Then it does not need building again.
#
# WHY THIS EXISTS. Building the cores is 35 of the ~40 minutes of a CI build, every build,
# including one that only changed a line of documentation. Worse, the owner feels that cost
# twice: anything that goes wrong late in the job means waiting the whole 35 minutes again
# for the retry. Every core is pinned to an exact upstream commit, so the output is a pure
# function of (that commit + this script + its patches + the toolchain) and is safe to keep.
#
# The proof of provenance is ios_record_source_version's manifest, not the file's presence:
# the line has to name this core's repository, its pinned sha, the word `pinned`, which
# that function only writes when the checkout's HEAD really was the pin, and this core's
# fingerprint as it is NOW. A dylib left over from a different commit cannot satisfy that.
#
# THE FINGERPRINT (ios_core_fingerprint) covers everything else the dylib is made from, PER
# CORE: its pin, every scripts/patches/<family>-*.patch, TWO parts of this script - the SHARED
# part (every line that counts outside the regions) and this core's OWN regions, as WHAT COUNTS
# above ios_cache_scan sets out - the toolchain id CI passes in CONTINUUM_CORE_TOOLCHAIN, and for
# Continuum Symbian its plug (native/continuum-symbian) and the libretro headers script. Editing
# one core's patch rebuilds that core and nothing else. Matching patches by family puts a JIT
# build's patch in the plain build's fingerprint too, which only costs an extra rebuild, never a
# stale dylib. scripts/check-core-cache.sh tests it.
#
# SO WHICH CHANGE REBUILDS WHAT.
#   - EVERY core: a change to the SHARED part that bash would run - a helper every core goes
#     through (ios_clone, build_ios_make_core, ios_stage_dylib), the staging and install_name
#     logic, the common compiler and linker flags, the required-symbol list, ios_core_config's
#     defaults - or a new toolchain, because it can change every dylib.
#   - ONE core (with the JIT or plain build sharing its source): a change inside its own region -
#     its clone and checkout, its cmake or make invocation, its own flags, its post-processing, or
#     a tool only it downloads, like the Symbian core's pinned CMake and ninja - or to its pin, its
#     patches, or (Symbian) its plug. That is the whole point: the 13 October Symbian build-tool
#     fix cost an hour of rebuilding the eighteen other cores built from source, and now it costs
#     one core.
#   - NO core: a comment or a blank line anywhere they are really comments, the cache's own code
#     and the help text (the cache regions), adding a core to the lists or taking one out (the
#     lists region; the new core builds, the others are reused), and a new core's own regions.
#
# AN OLD 4-FIELD LINE ("<core> <repo> <pin> pinned", from before the fingerprint) NEVER
# MATCHES, so the first build with this (build 182) rebuilds every core once, and later builds
# rebuild only the cores whose inputs changed. If in doubt it rebuilds: a fingerprint that
# cannot be worked out is never a match.
#
# THE SAME ONE-OFF COST APPLIES WHENEVER THE SHAPE OF WHAT IS HASHED CHANGES: the per-core split
# (13 October, `shared` + `recipe <family>` instead of one `recipe`) and WHAT COUNTS (14 October,
# comments, blank lines, the cache's code and the lists left out) each changed every stored
# fingerprint, so the first build after each rebuilds every core once. The builds after it are
# the quick ones.
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
  local fp
  fp="$(ios_core_fingerprint "$core")" || return 1
  [[ -n "$fp" ]] || return 1
  grep -qxF "$core $IOS_REPO $IOS_PIN pinned fp:$fp" "$manifest" || return 1
  return 0
}

# The cache's own subcommands, every one of them, so the dispatch hands them over in one line and
# changing them rebuilds nothing. All of them run on any host.
ios_cache_command() {
  case "$1" in
    ios-fingerprint)
      # scripts/check-core-cache.sh tests the per-core cache with it on Linux.
      if [[ "$#" -lt 2 ]] || ! ios_core_config "$2"; then
        echo "error: 'ios-fingerprint' needs a core name. Valid: ${IOS_CORES[*]} ${IOS_OPTIONAL_CORES[*]}" >&2
        exit 1
      fi
      ios_core_fingerprint "$2" || { echo "error: could not work out $2's fingerprint" >&2; exit 1; }
      exit 0
      ;;
    ios-is-cached)
      # Exit 0 when ios-all would reuse the staged dylib, 1 when it would build it.
      if [[ "$#" -lt 2 ]] || ! ios_core_config "$2"; then
        echo "error: 'ios-is-cached' needs a core name. Valid: ${IOS_CORES[*]} ${IOS_OPTIONAL_CORES[*]}" >&2
        exit 1
      fi
      if ios_core_is_cached "$2"; then
        exit 0
      fi
      exit 1
      ;;
    ios-recipe-slice)
      # Prints the part of this script that is THIS core's own recipe, which is what the
      # fingerprint hashes per core. Takes a core name and resolves it to its family, so
      # `ios-recipe-slice flycast_jit` prints the `flycast` region. Exits 1, printing nothing
      # usable, when the region cannot be located - the fail-closed path, which is what
      # scripts/check-core-cache.sh asserts on.
      if [[ "$#" -lt 2 ]] || ! ios_core_config "$2"; then
        echo "error: 'ios-recipe-slice' needs a core name. Valid: ${IOS_CORES[*]} ${IOS_OPTIONAL_CORES[*]}" >&2
        exit 1
      fi
      ios_recipe_text "$(ios_recipe_family "$2")" recipe || {
        echo "error: no recipe region for $2 in $IOS_SELF (so it would be rebuilt)" >&2
        exit 1
      }
      exit 0
      ;;
    ios-recipe-shared)
      # Prints the shared remainder: every line that counts outside every region. A change in
      # here rebuilds every core.
      ios_recipe_text "" shared || {
        echo "error: the recipe regions in $IOS_SELF do not parse (so every core would be rebuilt)" >&2
        exit 1
      }
      exit 0
      ;;
    ios-cache-check)
      # Exit 0 only when every rule of WHAT COUNTS holds and bash confirms what is left out.
      ios_recipe_text "" shared > /dev/null || {
        echo "error: $IOS_SELF breaks a build cache rule (see above), so every core would be rebuilt" >&2
        exit 1
      }
      exit 0
      ;;
    ios-cache-fingerprints)
      # "<core> <fingerprint>" for every listed core ("none" when it has none), all from ONE
      # checked reading of this file, so scripts/check-core-cache.sh can compare every core before
      # and after an edit quickly. It checks this matches ios-fingerprint, core by core.
      local dir core fp read_ok=0
      dir="$(mktemp -d "${TMPDIR:-/tmp}/continuum-cache.XXXXXX")" || exit 1
      ios_cache_split "*" "$dir" && read_ok=1
      for core in "${IOS_CORES[@]}" "${IOS_OPTIONAL_CORES[@]}"; do
        fp="none"
        if [[ "$read_ok" == "1" ]]; then fp="$(ios_core_fingerprint "$core" "$dir")" || fp="none"; fi
        echo "$core ${fp:-none}"
      done
      rm -rf "$dir"
      exit 0
      ;;
    ios-cache-scan)
      # The classification itself, for scripts/check-core-cache.sh: ios-cache-scan <mode> [family],
      # with the modes ios_cache_scan lists. It runs the rules but not the bash check.
      [[ "$#" -ge 2 ]] || { echo "error: 'ios-cache-scan' needs a mode" >&2; exit 1; }
      ios_cache_scan "$2" "${3:-}" || exit 1
      exit 0
      ;;
    *)
      echo "error: unknown subcommand '$1'." >&2
      ios_usage >&2
      exit 1
      ;;
  esac
}
# not-a-build-input-end: cache

# THE "SOMETHING WAS BUILT" MARKER: native/ios/build/lib/cores-rebuilt.txt, one core per line.
# The CI core cache saves a new entry only when this file exists (the "Save the built cores" step
# in .github/workflows/ios.yml), so a run that reused every core stores nothing, and a run that
# finally built a core the cached set was missing stores the completed set.
#
# Not under CONTINUUM_UNPINNED=1. Those dylibs are built from HEAD, ios_core_is_cached never
# reuses them, and saving them would make them the newest cache entry, so the next normal build
# would restore them and rebuild every core. With no marker that run saves nothing and the pinned
# set stays the one restored.
IOS_REBUILT_MARKER="$IOS_OUT_DIR/cores-rebuilt.txt"

ios_note_rebuilt() {
  ios_unpinned && return 0
  # Nor for a core with no pin (cBoy, built from cores/continuum-gb in this checkout). It is built
  # on every run and never reused from the cache, so building it is no reason to store a new cache
  # entry; noting it made every build save another full copy of the core set.
  ios_core_config "$1"
  [[ -n "${IOS_PIN:-}" ]] || return 0
  echo "$1" >> "$IOS_REBUILT_MARKER"
}

# A core that did not build leaves NOTHING behind: not its dylib, and not its line in
# core-sources.txt either, because that line is written when the source is checked out, before the
# compile, and the manifest has to describe only what this build really contains. It also matters
# to the cache: a manifest line with no dylib, or a dylib from a run that died, must not be saved
# and reused as if it were a finished build.
ios_forget_core() {
  local core="$1"
  local manifest="$IOS_OUT_DIR/core-sources.txt"
  ios_core_config "$core"
  rm -f "$IOS_OUT_DIR/$IOS_DYLIB_NAME"
  if [[ -f "$manifest" ]]; then
    grep -v "^$core " "$manifest" > "$manifest.tmp" 2>/dev/null || true
    mv "$manifest.tmp" "$manifest"
  fi
}

build_all_ios_cores() {
  ios_require_darwin

  # Started empty here, as well as by build-engine.sh's clean, so it can only ever name what THIS
  # run built. Removed rather than truncated: CI asks whether the file exists, and an empty file
  # still exists.
  mkdir -p "$IOS_OUT_DIR"
  rm -f "$IOS_REBUILT_MARKER"

  # A string, not an array: on bash 3.2 an empty array expanded under `set -u` is an error,
  # and this one is empty exactly when everything worked.
  local failed=""
  local failed_count=0
  local reused=0
  local rebuilt=0
  local core
  for core in "${IOS_CORES[@]}"; do
    echo
    echo "======================================================== ios: $core"
    # Both cache questions below run in a subshell: the cache's own code is not a build input
    # (WHAT COUNTS, above ios_cache_scan), so nothing it sets may reach the builds that follow.
    if ( ios_core_is_cached "$core" ); then
      echo "==> $core: reusing the cached dylib, still at its pinned commit"
      reused=$((reused + 1))
      continue
    fi
    if bash "$IOS_SELF" ios "$core"; then
      echo "==> $core ok"
      ios_note_rebuilt "$core"
      rebuilt=$((rebuilt + 1))
    else
      echo "!!! $core FAILED (see the output above for why)" >&2
      failed="$failed $core"
      failed_count=$((failed_count + 1))
      ios_forget_core "$core"
    fi
  done

  # Optional cores: same separate-process build, but a failure is a warning. The .ipa ships
  # without the core and every downstream consumer reports it as missing rather than failing.
  local optional_failed=""
  for core in "${IOS_OPTIONAL_CORES[@]}"; do
    echo
    echo "======================================================== ios (optional): $core"
    if ( ios_core_is_cached "$core" ); then
      echo "==> $core: reusing the cached dylib, still at its pinned commit"
      reused=$((reused + 1))
      continue
    fi
    if bash "$IOS_SELF" ios "$core"; then
      echo "==> $core ok"
      ios_note_rebuilt "$core"
      rebuilt=$((rebuilt + 1))
    else
      echo "!!! optional core $core FAILED; the .ipa will ship without it" >&2
      optional_failed="$optional_failed $core"
      ios_forget_core "$core"
    fi
  done

  echo
  if [[ "$reused" -gt 0 ]]; then
    echo "==> $reused core(s) reused from the cache, not rebuilt"
  fi
  echo "==> $rebuilt core(s) built by this run"
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
  ios-fingerprint|ios-is-cached|ios-recipe-slice|ios-recipe-shared|ios-cache-*)
    # The build cache's own subcommands. Their code is in the cache region (WHAT COUNTS above
    # ios_cache_scan), so changing it rebuilds no core.
    ios_cache_command "$@"
    ;;
  ios-apply-patches)
    # Any host. The weekly check (scripts/check-core-updates.sh) runs it on a fresh copy of
    # upstream's newest code to say whether our patches still fit. It changes that copy.
    if [[ "$#" -lt 3 ]] || ! ios_core_config "$2"; then
      echo "error: 'ios-apply-patches' needs <core> <source dir>. Valid: ${IOS_CORES[*]} ${IOS_OPTIONAL_CORES[*]}" >&2
      exit 1
    fi
    [[ -d "$3" ]] || { echo "error: no source directory at $3" >&2; exit 1; }
    IOS_SRC_DIR="$(cd "$3" && pwd)"
    if [[ "$2" == "continuum_symbian" ]]; then
      # Its patch is applied inside its build function, after the iOS port's own patches, so
      # this only checks that it would apply.
      git -C "$IOS_SRC_DIR" apply --check "$ROOT/scripts/patches/continuum-symbian-frame-readback.patch"
    else
      ios_apply_core_patches "$2"
    fi
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
