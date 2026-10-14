#!/usr/bin/env bash
# Builds the local reference MAME for cArcade's board comparisons (never shipped, never committed):
# MAME 0.289 from its release tarball, a subtarget holding only the Pac-Man family, the Galaxian
# family (Konami's Scramble/Frogger/Amidar included) and Donkey Kong / Donkey Kong Jr.
# Headless: SDL2 built here with only its dummy/offscreen video and dummy/disk audio drivers, no X11,
# no OpenGL, a stub libEGL (bgfx loads EGL at run time; NO_X11 only needs it at link time).
# Usage: build-mame.sh [jobs]   (default 2; retry with 1 if a compile is killed)
set -euo pipefail
HERE=/projects/sandbox/.scratch/mame
DEPS=$HERE/deps
JOBS=${1:-2}
cd "$HERE/mame-mame0289"
export PATH="$DEPS/bin:$PATH"
export PKG_CONFIG_PATH="$DEPS/lib64/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
make -j"$JOBS" \
  SUBTARGET=arcaderef \
  SOURCES=src/mame/pacman,src/mame/galaxian,src/mame/nintendo/dkong.cpp \
  IGNORE_GIT=1 TOOLS=0 TESTS=0 NOWERROR=1 ${REGEN:+REGENIE=1} \
  NO_X11=1 NO_OPENGL=1 NO_USE_XINPUT=1 NO_USE_MIDI=1 NO_USE_PORTAUDIO=1 \
  NO_USE_PULSEAUDIO=1 NO_USE_PIPEWIRE=1 USE_QTDEBUG=0 USE_TAPTUN=0 USE_PCAP=0 \
  SDL_INSTALL_ROOT="$DEPS" \
  ARCHOPTS="-I$DEPS/include -I$DEPS/include/SDL2" \
  LDOPTS="-L$DEPS/lib64 -Wl,-rpath,$DEPS/lib64" \
  OPTIMIZE=2 SYMBOLS=0 STRIP_SYMBOLS=1 \
  PYTHON_EXECUTABLE=python3
