# G1R: Apple stats box off (side project, not part of Continuum)

This is a personal patch for the owner's copy of G1R, the gen1recomp++ iPhone app from
github.com/bryanthaboi/gen1recomp. It hides Apple's Metal Performance HUD (the grey stats box) in
G1R only, so the phone's **Settings → Developer → Show Graphics HUD** switch can stay on for the
owner's own apps. **Confirmed working on the owner's iPhone on 12 October 2026 (G1R 0.3.82).**

It uses the same two settings as Continuum's `native/ios/AppleOverlay.swift`:
- every Metal layer gets an empty `developerHUDProperties` dictionary;
- the `MetalForceHudEnabled` user default is set to NO, which only counts from the next launch.

G1R has no native code to change, so `AppleHudOff.lua` does this from G1R's own Lua code through
LuaJIT's FFI. It re-checks the layers from a wrapped `love.graphics.present`. On the first launch it
shows a one-time popup ("Apple stats box: OFF, screens found: N"), which also proves it ran.

## Redo it for a new G1R release
1. Download the official `gen1recomp++-<version>-ios.ipa` from that repo's Releases.
2. Run `python3 patch_game.py <that .ipa> AppleHudOff.lua <out .ipa>`. Inside `game.love` it adds
   `src/core/AppleHudOff.lua` and one `pcall(require, "src.core.AppleHudOff")` line near the top
   of `main.lua`. Nothing else changes: not the program, not Info.plist, not any launcher file.
3. Optional check on Linux: build LuaJIT, then run
   `gcc -shared -fPIC -o libfakeobjc.so test/fakeobjc.c` and
   `luajit test/run.lua ./libfakeobjc.so .`. The test uses a pretend Objective-C runtime.
4. Send the owner a private link. G1R's license does not allow sharing modified copies that
   include its launcher, so never post them publicly. In the sandbox, creating repos and gists is
   blocked, so a 72-hour litterbox.catbox.moe link was used.

The owner installs with eSign and deletes the old app first, so each install starts with no data.
Two earlier tries injected a native add-on into the program, and both failed. Don't go back to
that approach.
