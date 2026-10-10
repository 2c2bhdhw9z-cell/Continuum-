#!/usr/bin/env bash
# Fetches the two bundled players into native/ios/build/support/players/, which project.yml
# bundles as support/players/ inside the .app (see WebPlayers.swift):
#
#   ruffle/  Ruffle, the Flash player, its self-hosted web build from the pinned release.
#            MIT OR Apache-2.0 (LICENSE_MIT and LICENSE_APACHE are copied with it).
#   j2me/    J2meJS, the J2ME engine Manic EMU also runs (a maintained fork of Mozilla's
#            pluotsorbet), its prebuilt java/ folder at the pinned commit. GPL-2.0 (LICENSE is
#            copied with it); the emoji images are CC-BY 4.0 (style/emoji/LICENSE). The demo
#            games and their icons in java/jar and java/img are NOT copied.
#
#   scripts/fetch-players.sh           fetch, check and stage both
#
# PINNED, AND CHECKED. Ruffle's zip must match its sha256 exactly (release assets do not change).
# J2meJS comes from GitHub's tarball of one commit; GitHub does not promise that tarball's bytes
# are stable, so its sha256 is recorded but the gate is the sha256 of the engine files themselves,
# listed below. Both land in native/ios/build/lib/core-sources.txt like every core.
#
# OPTIONAL, LIKE THE BUILDBOT CORES. A failed download or check is a warning and the .ipa ships
# without that player; the CI verify step warns, and the app says on the status line that the
# player is not in this build when such a game is opened. Only a usage error exits non-zero.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT="$ROOT/native/ios/build/support/players"
LIB_DIR="$ROOT/native/ios/build/lib"
WORK="$ROOT/.work/players"

RUFFLE_TAG="v0.6.0"
RUFFLE_URL="https://github.com/ruffle-rs/ruffle/releases/download/$RUFFLE_TAG/ruffle-0.6.0-web-selfhosted.zip"
RUFFLE_SHA256="e8acfacc37443303872379d0e215999af846854d1dd3fa8fac0a765445b43dbf"

J2MEJS_COMMIT="75947c10412c7ff9927835a128b653696f402e9d"
J2MEJS_URL="https://codeload.github.com/Rosabis/J2meJS/tar.gz/$J2MEJS_COMMIT"
J2MEJS_TARBALL_SHA256="2fa3d0f331f4d94c9a51aea7162b3c7b1bb4c6713e324818598588e1c63ad944"
# "<sha256> <path inside java/>": the engine itself, which is what the pin is about.
J2MEJS_FILES=(
  "023401c9fbe45ac1dd672f924cf074c2a774e14160fb4059aaea122042836e6e main.html"
  "b9b2ac43b2945137fd5c959e71ef368a9abd5567643233a11c7301b3debbd0e9 bld/main-all.js"
  "e91382215e78d1b103a8e0a3cc9484e3b7304e25fdf18baefa3bdfb9bfbb8bc8 bld/j2me.js"
  "3f557bbf0f742c879df7f21a5b1c1ff69301f86ac945980d4f59f2ed52a512fd bld/native.js"
  "018bcccde14f67c4cf5a0e7bee56979e9427e5649c1dd0aaf48c593d42f56f7c java/classes.jar"
  "1b4cce378fc2e48f97aa9db1ad1d670c30d2f1072c9e2ccd4d4ac90c1f52631e config/default.js"
)
J2MEJS_LICENSE_SHA256="edaef632cbb643e4e7a221717a6c441a4c1a7c918e6e4d56debc3d8739b233f6"

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{ print $1 }'
  else
    shasum -a 256 "$1" | awk '{ print $1 }'
  fi
}

# Takes <name>'s line out of core-sources.txt as its fetch starts, so a player that fails today does
# not keep the line a cached manifest carried over from an earlier run (the same rule as
# forget_source in fetch-buildbot-cores.sh).
forget_source() {
  local manifest="$LIB_DIR/core-sources.txt"
  [[ -f "$manifest" ]] || return 0
  grep -v "^$1 " "$manifest" > "$manifest.tmp" 2>/dev/null || true
  mv "$manifest.tmp" "$manifest"
}

record_source() {
  local name="$1" url="$2" sha="$3"
  local manifest="$LIB_DIR/core-sources.txt"
  mkdir -p "$LIB_DIR"
  if [[ -f "$manifest" ]]; then
    grep -v "^$name " "$manifest" > "$manifest.tmp" 2>/dev/null || true
    mv "$manifest.tmp" "$manifest"
  fi
  echo "$name $url sha256:$sha" >> "$manifest"
  LC_ALL=C sort -o "$manifest" "$manifest"
}

fetch_ruffle() {
  local dir="$WORK/ruffle" zip="$WORK/ruffle/ruffle-selfhosted.zip" dest="$OUT/ruffle"
  rm -rf "$dir" "$dest"
  mkdir -p "$dir"
  forget_source "ruffle-selfhosted"
  echo "==> Ruffle $RUFFLE_TAG (Flash player, MIT OR Apache-2.0): $RUFFLE_URL"
  if ! curl -fsSL --retry 3 --retry-delay 5 --connect-timeout 30 -o "$zip" "$RUFFLE_URL"; then
    echo "warning: Ruffle download failed; the .ipa ships without the Flash player" >&2
    return 1
  fi
  local got
  got="$(sha256_of "$zip")"
  if [[ "$got" != "$RUFFLE_SHA256" ]]; then
    echo "warning: Ruffle zip sha256 is $got, not the pinned $RUFFLE_SHA256; dropped, the .ipa ships without the Flash player" >&2
    return 1
  fi
  if ! unzip -oq "$zip" -d "$dir/unzipped"; then
    echo "warning: the Ruffle zip would not unpack; the .ipa ships without the Flash player" >&2
    return 1
  fi
  if [[ ! -f "$dir/unzipped/ruffle.js" ]] || ! ls "$dir/unzipped/"*.wasm >/dev/null 2>&1; then
    echo "warning: the Ruffle zip has no ruffle.js or .wasm; the .ipa ships without the Flash player" >&2
    return 1
  fi
  mkdir -p "$dest"
  # Everything but the source maps, which are debugging aids worth 1.5 MB and nothing at runtime.
  find "$dir/unzipped" -maxdepth 1 -type f ! -name '*.map' -exec cp {} "$dest/" \;
  record_source "ruffle-selfhosted" "$RUFFLE_URL" "$got"
  echo "    staged $(find "$dest" -type f | wc -l | tr -d ' ') files, $(du -sh "$dest" | cut -f1)"
}

fetch_j2mejs() {
  local dir="$WORK/j2mejs" tarball="$WORK/j2mejs/j2mejs.tar.gz" dest="$OUT/j2me"
  rm -rf "$dir" "$dest"
  mkdir -p "$dir"
  forget_source "j2mejs"
  echo "==> J2meJS $J2MEJS_COMMIT (J2ME engine, GPL-2.0): $J2MEJS_URL"
  if ! curl -fsSL --retry 3 --retry-delay 5 --connect-timeout 30 -o "$tarball" "$J2MEJS_URL"; then
    echo "warning: J2meJS download failed; the .ipa ships without the J2ME player" >&2
    return 1
  fi
  local got
  got="$(sha256_of "$tarball")"
  if [[ "$got" != "$J2MEJS_TARBALL_SHA256" ]]; then
    echo "warning: the J2meJS tarball sha256 is $got, not $J2MEJS_TARBALL_SHA256; GitHub regenerated it, so the engine files are checked one by one instead"
  fi
  if ! tar -xzf "$tarball" -C "$dir"; then
    echo "warning: the J2meJS tarball would not unpack; the .ipa ships without the J2ME player" >&2
    return 1
  fi
  local src="$dir/J2meJS-$J2MEJS_COMMIT"
  local entry want path
  for entry in "${J2MEJS_FILES[@]}"; do
    want="${entry%% *}"
    path="${entry#* }"
    if [[ ! -f "$src/java/$path" ]] || [[ "$(sha256_of "$src/java/$path")" != "$want" ]]; then
      echo "warning: J2meJS java/$path is missing or not the pinned file; dropped, the .ipa ships without the J2ME player" >&2
      return 1
    fi
  done
  if [[ "$(sha256_of "$src/LICENSE")" != "$J2MEJS_LICENSE_SHA256" ]]; then
    echo "warning: J2meJS LICENSE is not the pinned GPL-2.0 text; dropped, the .ipa ships without the J2ME player" >&2
    return 1
  fi
  mkdir -p "$dest/java"
  cp "$src/java/main.html" "$src/java/keymap.js" "$dest/"
  cp "$src/LICENSE" "$dest/LICENSE"
  cp "$src/java/java/classes.jar" "$dest/java/"
  local folder
  for folder in bld config js libs polyfill style; do
    cp -R "$src/java/$folder" "$dest/$folder"
  done
  find "$dest" -name '.DS_Store' -delete
  record_source "j2mejs" "$J2MEJS_URL" "$got"
  echo "    staged $(find "$dest" -type f | wc -l | tr -d ' ') files, $(du -sh "$dest" | cut -f1)"
}

case "${1:-fetch}" in
  fetch)
    command -v curl >/dev/null 2>&1 || { echo "error: curl not found" >&2; exit 1; }
    command -v unzip >/dev/null 2>&1 || { echo "error: unzip not found" >&2; exit 1; }
    mkdir -p "$OUT" "$WORK"
    fetch_ruffle || rm -rf "$OUT/ruffle"
    fetch_j2mejs || rm -rf "$OUT/j2me"
    echo
    echo "==> bundled players (native/ios/build/support/players)"
    for p in ruffle/ruffle.js j2me/main.html; do
      if [[ -f "$OUT/$p" ]]; then echo "      ok       $p"; else echo "      MISSING  $p"; fi
    done
    ;;
  -h|--help|help)
    sed -n '2,20p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
    ;;
  *)
    echo "error: unknown subcommand '$1' (use: fetch)" >&2
    exit 1
    ;;
esac
