#!/usr/bin/env bash
#
# Build libsmb2 as a static iOS arm64 library into native/ios/build/smb.
#
# SMB came out after build 117: AMSMB2 is a Swift package whose product is type: .dynamic, so
# Xcode linked AMSMB2.framework and never embedded it, and the app closed on launch looking for
# that framework. This is the other fix. The C library is linked into Continuum itself. There is
# no framework to forget. smb_min.c is the small piece of it the app calls.
#
# PINNED to a commit, not a branch. The commit is the one that added the sync share-enum call
# this app uses to list the shares on a server.
set -euo pipefail

PIN="fc710a3ebd58a3c15d0ef24322f748e38a3d3a90"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DEST="$ROOT/native/ios/build/smb"

if [[ -f "$DEST/lib/libsmb2.a" && -f "$DEST/include/smb2/libsmb2.h" && -f "$DEST/pin" ]] \
   && [[ "$(cat "$DEST/pin")" == "$PIN" ]]; then
  echo "libsmb2 already built ($PIN)"
  exit 0
fi

SDK="$(xcrun --sdk iphoneos --show-sdk-path)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

echo "==> fetching libsmb2 $PIN"
git init "$TMP/src" >/dev/null
git -C "$TMP/src" remote add origin https://github.com/sahlberg/libsmb2.git
git -C "$TMP/src" fetch --depth 1 origin "$PIN"
git -C "$TMP/src" checkout --detach FETCH_HEAD

echo "==> configuring libsmb2 for iphoneos"
cmake -S "$TMP/src" -B "$TMP/build" -G "Unix Makefiles" \
  -DCMAKE_SYSTEM_NAME=iOS \
  -DCMAKE_OSX_SYSROOT="$SDK" \
  -DCMAKE_OSX_ARCHITECTURES=arm64 \
  -DCMAKE_OSX_DEPLOYMENT_TARGET=16.0 \
  -DCMAKE_BUILD_TYPE=Release \
  -DCMAKE_INSTALL_LIBDIR=lib \
  -DBUILD_SHARED_LIBS=OFF \
  -DENABLE_LIBKRB5=OFF \
  -DENABLE_GSSAPI=OFF \
  -DENABLE_EXAMPLES=OFF \
  -DENABLE_UTILS=OFF \
  -DENABLE_LIBDCERPC=OFF \
  -DCMAKE_INSTALL_PREFIX="$DEST"

cmake --build "$TMP/build" --parallel
rm -rf "$DEST"
cmake --install "$TMP/build"
printf '%s\n' "$PIN" > "$DEST/pin"

[[ -f "$DEST/lib/libsmb2.a" ]] || { echo "error: libsmb2.a was not installed" >&2; exit 1; }
[[ -f "$DEST/include/smb2/libsmb2.h" ]] || { echo "error: libsmb2.h was not installed" >&2; exit 1; }
[[ -f "$DEST/include/smb2/libsmb2-share-enum.h" ]] || {
  echo "error: libsmb2-share-enum.h was not installed; the pin does not have share enum" >&2
  exit 1
}
echo "libsmb2 installed at $DEST"
