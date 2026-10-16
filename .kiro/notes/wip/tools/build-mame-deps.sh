#!/usr/bin/env bash
# Everything build-mame.sh needs in a fresh sandbox, then the build itself (16 October 2026):
# MAME 0.289's source, a headless SDL2 (dummy/offscreen video, dummy/disk audio), SDL2_ttf (MAME's
# font code needs it) and a stub libEGL, all under /projects/sandbox/.scratch/mame.
#
# The sandbox stops background jobs when a command ends, so run this in the FOREGROUND with the
# longest timeout (30 minutes). It picks up where it stopped. If a run was killed mid-compile,
# some object files are cut short and the link fails with "undefined reference": find them with
#   for f in $(find mame-mame0289/build -name '*.o'); do nm "$f" >/dev/null 2>&1 || echo "$f"; done
# delete those and every *.a under mame-mame0289/build, and run it again.
set -euo pipefail
HERE=/projects/sandbox/.scratch/mame
D=$HERE/deps
mkdir -p "$D/lib64" && cd "$HERE"
cp "$(dirname "$0")/build-mame.sh" "$HERE/" 2>/dev/null || true
if [ ! -d mame-mame0289 ]; then
  curl -fsSL -o mame0289.tar.gz https://codeload.github.com/mamedev/mame/tar.gz/refs/tags/mame0289
  tar xzf mame0289.tar.gz
fi
if [ ! -f "$D/lib64/libSDL2.so" ]; then
  curl -fsSL -o SDL2.tar.gz https://github.com/libsdl-org/SDL/releases/download/release-2.30.9/SDL2-2.30.9.tar.gz
  tar xzf SDL2.tar.gz
  cmake -S SDL2-2.30.9 -B sdl-build -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX="$D" \
    -DCMAKE_INSTALL_LIBDIR=lib64 -DSDL_X11=OFF -DSDL_WAYLAND=OFF -DSDL_KMSDRM=OFF -DSDL_OPENGL=OFF \
    -DSDL_OPENGLES=OFF -DSDL_VULKAN=OFF -DSDL_PULSEAUDIO=OFF -DSDL_ALSA=OFF -DSDL_PIPEWIRE=OFF \
    -DSDL_JACK=OFF -DSDL_SNDIO=OFF -DSDL_DUMMYVIDEO=ON -DSDL_OFFSCREEN=ON -DSDL_DISKAUDIO=ON \
    -DSDL_DUMMYAUDIO=ON -DSDL_TEST=OFF -DSDL_STATIC=OFF >/dev/null
  cmake --build sdl-build -j8 >/dev/null && cmake --install sdl-build >/dev/null
fi
if [ ! -f "$D/include/SDL2/SDL_ttf.h" ]; then
  curl -fsSL -o ttf.tar.gz https://github.com/libsdl-org/SDL_ttf/releases/download/release-2.22.0/SDL2_ttf-2.22.0.tar.gz
  tar xzf ttf.tar.gz
  cmake -S SDL2_ttf-2.22.0 -B ttf-build -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX="$D" \
    -DCMAKE_INSTALL_LIBDIR=lib64 -DCMAKE_PREFIX_PATH="$D" -DSDL2TTF_VENDORED=OFF \
    -DSDL2TTF_HARFBUZZ=OFF -DSDL2TTF_SAMPLES=OFF -DBUILD_SHARED_LIBS=ON >/dev/null
  cmake --build ttf-build -j8 >/dev/null && cmake --install ttf-build >/dev/null
fi
[ -f "$D/lib64/libEGL.so" ] || echo 'void eglGetProcAddress(void){}' | gcc -shared -fPIC -o "$D/lib64/libEGL.so" -x c -
bash "$HERE/build-mame.sh" 8
cp "$(dirname "$0")/mame-READY.txt" "$HERE/READY" 2>/dev/null || true
ls -la "$HERE/mame-mame0289/arcaderef"
