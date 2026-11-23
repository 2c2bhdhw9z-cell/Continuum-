#!/bin/sh
# Builds Continuum for TV and packs it into an .ipk for LG webOS TVs.
#
#   tv/webos/package.sh [out-dir]
#
# Needs: the Rust toolchain from rust-toolchain.toml with the armv7-unknown-linux-gnueabi
# standard library, clang + lld, and a glibc 2.31 soft-float sysroot (Debian 11 armel's libc6,
# libc6-dev, libgcc-10-dev, libgcc-s1). The TV's programs use the soft-float ABI and
# /lib/ld-linux.so.3, so the hard-float armv7 target does not run there.
#
# TV_SYSROOT   the sysroot folder
# TV_RUSTSYS   a Rust sysroot holding lib/rustlib/armv7-unknown-linux-gnueabi
# TV_SDL       webosbrew's SDL2 for webOS (libSDL2-2.0.so.0), shipped in the app's lib/
#
# The TV has its own copy of cBoy, tv/cores/cboy (crate cboy_tv), separate from the phone's
# cores/continuum-gb (owner, 22 November 2026: "The tv core is separate. It's for a tv").
# TV speed work goes only into the TV copy; nothing here changes the iPhone app.
set -eu
HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
OUT=${1:-$HERE/dist}
T=${TV_BUILD:-/projects/sandbox/.tvbuild}
TARGET=armv7-unknown-linux-gnueabi
APP_ID=dev.continuum.tv
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' "$HERE/Cargo.toml" | head -1)

export CARGO_TARGET_ARMV7_UNKNOWN_LINUX_GNUEABI_LINKER=${TV_LINKER:-$T/arm-cc}
export CARGO_TARGET_ARMV7_UNKNOWN_LINUX_GNUEABI_RUSTFLAGS="--sysroot ${TV_RUSTSYS:-$T/rustsys} -C target-cpu=cortex-a55 -C target-feature=-soft-float,+neon,+fp-armv8"
export CC_armv7_unknown_linux_gnueabi=${TV_LINKER:-$T/arm-cc}
export AR_armv7_unknown_linux_gnueabi=llvm-ar

echo "== cBoy for the TV"
(cd "$HERE/../cores/cboy" &&
  CARGO_PROFILE_RELEASE_LTO=fat CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1 CARGO_PROFILE_RELEASE_PANIC=abort \
  CARGO_TARGET_DIR=$T/target-cboy-tv cargo build --release --target $TARGET -q)
echo "== the app"
(cd "$HERE" && CARGO_TARGET_DIR=$T/target-tv cargo build --release --target $TARGET -q)

STAGE=$(mktemp -d)
A=$STAGE/data/usr/palm/applications/$APP_ID
mkdir -p "$A/lib" "$A/cores" "$STAGE/control"
cp "$T/target-tv/$TARGET/release/continuum-tv" "$A/continuum-tv"
cp "$T/target-cboy-tv/$TARGET/release/libcboy_tv.so" "$A/cores/"
llvm-strip "$A/cores/libcboy_tv.so" 2>/dev/null || true
cp "${TV_SDL:-$T/tvlibs/libSDL2-2.0.so.0}" "$A/lib/libSDL2-2.0.so.0"
cp "$ROOT/native/ios/Assets.xcassets/AppIcon.appiconset/icon-152.png" "$A/icon.png"
cp "$ROOT/native/ios/Assets.xcassets/AppIcon.appiconset/icon-152.png" "$A/largeIcon.png"
cat > "$A/appinfo.json" <<EOF
{
    "id": "$APP_ID",
    "version": "$VERSION",
    "vendor": "Continuum",
    "type": "native",
    "main": "continuum-tv",
    "title": "Continuum",
    "icon": "icon.png",
    "largeIcon": "largeIcon.png",
    "iconColor": "#131316",
    "appDescription": "Game Boy, Color and Advance",
    "cloudgame_active": true,
    "useAllKeyboardKeys": true,
    "noSplashOnLaunch": false,
    "splashBackground": "icon.png"
}
EOF
SIZE=$(du -sk "$STAGE/data" | cut -f1)
cat > "$STAGE/control/control" <<EOF
Package: $APP_ID
Version: $VERSION
Section: misc
Priority: optional
Architecture: arm
Installed-Size: $SIZE
Maintainer: Continuum <ContinuumDev2c2b@gmail.com>
Description: Continuum for TV
webOS-Package-Format-Version: 2
webOS-Packager-Version: x.y.x
EOF
mkdir -p "$OUT"
IPK=$OUT/${APP_ID}_${VERSION}_arm.ipk
(cd "$STAGE/control" && tar --owner=0 --group=0 -czf ../control.tar.gz .)
(cd "$STAGE/data" && tar --owner=0 --group=0 -czf ../data.tar.gz .)
printf '2.0\n' > "$STAGE/debian-binary"
rm -f "$IPK"
(cd "$STAGE" && ar rc "$IPK" debian-binary control.tar.gz data.tar.gz)
rm -rf "$STAGE"
ls -la "$IPK"
sha256sum "$IPK"
