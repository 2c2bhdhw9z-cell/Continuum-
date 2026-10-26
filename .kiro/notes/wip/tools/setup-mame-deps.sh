#!/usr/bin/env bash
# Sets up what the reference MAME needs before build-mame.sh can run, in a fresh sandbox:
# MAME 0.289's source, SDL2 built with only its dummy/offscreen video and dummy/disk audio (no
# X11, no OpenGL), and a stub libEGL (bgfx loads EGL at run time; NO_X11 only needs it at link
# time). Everything lands in /projects/sandbox/.scratch/mame. Local only: never committed.
set -euo pipefail
HERE=/projects/sandbox/.scratch/mame
DEPS=$HERE/deps
mkdir -p "$HERE/src" "$DEPS/lib64" "$DEPS/include/EGL"
cd "$HERE"
[ -d mame-mame0289 ] || { curl -sSL -o mame0289.tar.gz \
  https://codeload.github.com/mamedev/mame/tar.gz/refs/tags/mame0289 && tar xzf mame0289.tar.gz; }
cd "$HERE/src"
[ -d SDL2-2.30.9 ] || { curl -sSL -o SDL2.tar.gz \
  https://github.com/libsdl-org/SDL/releases/download/release-2.30.9/SDL2-2.30.9.tar.gz && tar xzf SDL2.tar.gz; }
if [ ! -f "$DEPS/lib64/libSDL2.so" ]; then
  cmake -S SDL2-2.30.9 -B sdl-build -G Ninja -DCMAKE_BUILD_TYPE=Release \
    -DCMAKE_INSTALL_PREFIX="$DEPS" -DCMAKE_INSTALL_LIBDIR=lib64 \
    -DSDL_X11=OFF -DSDL_WAYLAND=OFF -DSDL_KMSDRM=OFF -DSDL_OPENGL=OFF -DSDL_OPENGLES=OFF \
    -DSDL_VULKAN=OFF -DSDL_ALSA=OFF -DSDL_PULSEAUDIO=OFF -DSDL_PIPEWIRE=OFF -DSDL_JACK=OFF \
    -DSDL_SNDIO=OFF -DSDL_OSS=OFF -DSDL_DUMMYVIDEO=ON -DSDL_OFFSCREEN=ON -DSDL_DISKAUDIO=ON \
    -DSDL_DUMMYAUDIO=ON -DSDL_TEST=OFF -DSDL_STATIC=OFF
  cmake --build sdl-build -j4 && cmake --install sdl-build
fi
# The stub EGL: the few symbols bgfx's EGL path names, all failing, so bgfx never uses it.
if [ ! -f "$DEPS/lib64/libEGL.so" ]; then
  cat > egl-stub.c <<'EOF'
void *eglGetDisplay(void *d) { (void)d; return 0; }
unsigned eglInitialize(void *d, int *a, int *b) { (void)d; (void)a; (void)b; return 0; }
unsigned eglTerminate(void *d) { (void)d; return 0; }
void *eglGetProcAddress(const char *n) { (void)n; return 0; }
int eglGetError(void) { return 0x3001; }
EOF
  gcc -shared -fPIC -o "$DEPS/lib64/libEGL.so" egl-stub.c
fi
echo READY
