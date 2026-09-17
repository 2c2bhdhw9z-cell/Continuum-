# Core binaries

This directory holds emulator cores, fetched on demand by `src/engine/core-loader.js`.
**Nothing here is loaded at boot** — only `manifest.json` is, and that is metadata.

## What is here now

`diagnostic-core.wasm` is a valid but empty WebAssembly module (the 8-byte header
and no sections). Every entry in `manifest.json` points at it, which is what lets the
entire pipeline run today:

```
manifest → declare → fetch (with progress) → Cache API → magic-byte validation
→ attachCoreModule → registry → session → paced ticks → WebGPU present
```

The Rust registry accepts it, then instantiates a `DiagnosticCore`
(`crates/emulator-bridge/src/cores/diagnostic.rs`) with the descriptor from the
manifest. That stand-in renders a test pattern at the system's real geometry and
refresh rate, responds to input, and produces audio — so the launch path, frame
pacing, renderer, audio graph and HUD are all exercised against real values before
any emulator exists.

## What replaces it

Phase 1b builds actual libretro cores to `wasm32-unknown-unknown` and drops them
here, one file per core id:

```
nestopia.wasm  snes9x.wasm  gambatte.wasm  mgba.wasm
genesis_plus.wasm  mupen64plus.wasm  yabause.wasm  mednafen_psx.wasm
```

Then, per entry in `manifest.json`:

1. point `module` at the real file;
2. set `sizeBytes` to the real size (drives the download progress bar);
3. remove `"placeholder": true`.

The geometry, `targetFps`, `audioSampleRate` and `pixelFormat` values in the manifest
are already the correct ones for each system, so they need no revisiting.

The Rust side changes in exactly one place: `instantiate()` in
`crates/emulator-bridge/src/cores/registry.rs`.

## Licensing

Cores are third-party GPL/LGPL software and are not vendored into this repository.
The build script that fetches and compiles them is Phase 1b work; keep the licence
text alongside each binary when it lands.
