# Continuum — first playable core

A WebGPU emulator front end whose engine is a Rust crate, so that Phase 2 (a
sideloaded iOS app with JIT-capable cores) is a UI port rather than a rewrite.

**Phase 1b is in: a real NES core runs real ROMs.** `fceumm` is compiled from C to
standalone WebAssembly, driven by the Rust bridge, presented through `wgpu`, with input
from keyboard, gamepad or touch. The Phase 1 scaffold is unchanged underneath — the
tag `v0.1.0-scaffold` marks that restore point.

![Library](docs/shot-library.png)

## What runs today

- **Real emulation.** NES via `fceumm`, built by `scripts/build-core.sh` against
  wasi-libc as a standalone wasm module (no Emscripten, no RetroArch bundle). It
  reports its own geometry (256x240), refresh (60.0998 Hz) and sample rate (48 kHz),
  and those values — not the manifest's — configure the pacer and renderer.
- **Real content.** Import your own ROMs by picker or drag-and-drop; they are
  identified by header, stored in IndexedDB and restored on reload. The library ships
  one ROM of its own, the *Continuum Test Cart*, written from scratch in
  `scripts/make-test-rom.py` — because an emulator with an empty library cannot
  demonstrate anything, and commercial ROMs cannot be bundled.
- **Real input.** `GamepadBridge` in Rust owns the mapping: the W3C standard-gamepad
  layout, deadzones, stick-to-D-pad synthesis, and a per-source merge so an idle
  controller cannot cancel a keypress. The front end only forwards events.
- **Everything from Phase 1.** One `requestAnimationFrame` loop, WebGPU-only
  rendering, strictly virtualised lists, and no core fetched before it is needed.
- **4,800 synthetic catalogue entries** so the virtualisation stays provable. They are
  placeholders with no ROM behind them, marked as such, and the launch path says so
  rather than feeding a real core noise.

## The five architectural rules, and where they are enforced

| Rule | Enforced by | Verified by |
| --- | --- | --- |
| 1. UI talks to cores only through a Rust `EmulatorBridge` | `crates/emulator-bridge/src/bridge.rs`; the wasm facade in `wasm.rs` is a thin, logic-free wrapper | JS never imports anything but `engine/bridge-host.js` |
| 2. WebGPU only — no 2D canvas, no DOM rendering | `crates/emulator-bridge/src/gfx/renderer.rs` + `frame_blit.wgsl`; wasm builds compile only wgpu's `webgpu` backend, so there is no WebGL2 fallback to fall into | smoke test traps `getContext('2d')` for the whole session |
| 3. One unified `rAF` loop | `web/src/engine/loop.js` — the only `requestAnimationFrame` in the app; it also flushes the UI, after the engine tick | smoke test counts rAF registrations and asserts the loop idles |
| 4. Strict virtualisation, no O(n) DOM | `web/src/ui/virtual-scroller.js`, used four times: shelf list, cards within each shelf, all-games grid, save states | smoke test scrolls 4,800 entries and asserts node counts do not change |
| 5. No core loaded at boot | `web/src/engine/core-loader.js` declares metadata only; `CoreRegistry::attach_module` / `attach_core` are the only paths to a runnable core | smoke test asserts `residentCoreCount === 0` at boot, `1` after launch |

## How a libretro core is embedded

The hard part is not compiling C to wasm — it is that a libretro core calls the
frontend through function *pointers* it was handed, and **a host cannot manufacture a
function pointer inside another wasm module.** A JS frontend has nothing valid to pass
to `retro_set_video_refresh`.

So `core-shim/libretro_wasm_shim.c` is compiled *into* the core module. Its functions
are real, in-table function pointers as far as the core is concerned, and each one
forwards to an imported host function:

```
core (C) ──calls fn ptr──▶ shim trampoline ──wasm import──▶ JS runtime ──▶ Rust
```

One frame, in full:

```
bridge.tick(now)                                   Rust
  └─ WasmCore::run_frame(input)                     Rust: publishes staging + input
       └─ runtime.run()                             JS
            └─ retro_run()                          core, its own wasm memory
                 ├─ host.input_state  ─────────────▶ CoreHost::inputState   (Rust)
                 ├─ host.video_refresh ────────────▶ copy → Rust staging, videoReady
                 └─ host.audio_batch  ────────────▶ copy → Rust staging, audioReady
       └─ host::end_frame()                         Rust: geometry + audio counts
  └─ renderer.present(frame)                        Rust: wgpu upload + blit
```

Notes on the seams that were not obvious going in:

- **The core's memory is separate**, so exactly one copy per frame is unavoidable.
  120 KB/frame for NES RGB565 (~7 MB/s) is nothing next to the GPU upload that follows.
- **Callbacks re-enter Rust mid-tick**, while `EmulatorBridge` is already mutably
  borrowed. `CoreHost`'s methods are therefore *static* and talk to a thread-local
  frame exchange — an instance method would panic on the second `RefCell` borrow.
- **Modern cores need more than bytes.** fceumm declares `need_fullpath = true`
  globally, then overrides it per extension via `SET_CONTENT_INFO_OVERRIDE` and
  discovers the content through `GET_GAME_INFO_EXT`. Both are implemented; without them
  a valid `.nes` file is rejected.
- **Only 12 WASI imports**, all file syscalls, all stubbed. The core never touches a
  filesystem because it is never given a path.

## Layout

```
core-shim/                      C shim compiled into every core (callback trampolines)
crates/emulator-bridge/         Rust engine. Compiles to wasm32 (Phase 1) and
  src/bridge.rs                 native ARM64 (Phase 2) from the same source.
  src/wasm.rs                   wasm-bindgen facade (wasm32 only)
  src/cores/wasm_core.rs        EmulatorCore over a real libretro module
  src/cores/host.rs             CoreHost: the callbacks a running core re-enters
  src/cores/{mod,registry,diagnostic}.rs   trait, lazy registry, stand-in core
  src/gfx/                      wgpu renderer, WGSL blit, pixel-format conversion
  src/audio/                    AudioSink, ring buffer, resampler
  src/input/gamepad.rs          GamepadBridge: layouts, deadzones, source merge
  src/{timing,frame}.rs         frame pacing, geometry
web/
  index.html  styles/           dark Netflix-style shell
  src/ui/virtual-scroller.js    the virtualisation primitive
  src/ui/{library,player}-view.js, detail-sheet.js, card.js, art.js
  src/engine/core-runtime.js    instantiates a libretro core; environment protocol
  src/engine/{loop,bridge-host,core-loader,input}.js
  src/audio/{audio-output,pcm-worklet}.js
  src/ui/rom-import.js          file picker, drag-and-drop, library plumbing
  src/data/                     catalogue, systems, save states, ROM store/detect
  cores/manifest.json           core declarations (metadata only)
  roms/nes-testcart.nes         our own NES ROM, generated by make-test-rom.py
  vendor/bridge/                build output of scripts/build-wasm.sh (gitignored)
scripts/
  build-wasm.sh                 cargo build + wasm-bindgen
  build-core.sh                 libretro core → standalone wasm (wasi-sdk)
  make-test-rom.py              6502 assembler + NES test ROM generator
  core-abi-test.mjs             headless core/ABI verification (no browser, no GPU)
  serve.mjs                     static server with correct wasm/ESM MIME types
  smoke-test.mjs                headless browser verification of the rules above
```

## Build and run

```bash
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.128   # must match the Cargo.lock version

./scripts/build-wasm.sh          # engine → web/vendor/bridge/
./scripts/build-core.sh fceumm   # NES core → web/cores/fceumm.wasm (fetches wasi-sdk)
python3 scripts/make-test-rom.py # test ROM → web/roms/nes-testcart.nes
node scripts/serve.mjs 8123      # → http://localhost:8123/
```

Then open the library and press **Play** on *Continuum Test Cart*, or drop your own
ROM anywhere on the page. Without `build-core.sh`, NES launches fail with a message
telling you to run it; every other system falls back to the placeholder core.

Needs a WebGPU browser: Chrome/Edge 113+, Safari 18+, Firefox 141+. There is
deliberately no fallback renderer; without WebGPU the UI says so and refuses to
launch.

**Controls.** Arrows = D-pad, `Z`/`X` = B/A, `A`/`S` = Y/X, `Q`/`W` = L/R,
`Enter` = Start, `Shift` = Select, `P` = pause, `Esc` = back.

Controllers are picked up automatically and assigned to the first free port; the HUD
shows how many are connected. On touch devices the on-screen pad appears, and its
D-pad is tracked as one surface so diagonals actually work. All three sources can be
used at once — they are merged in Rust, not fought over.

## Verification

Three layers, fastest first.

```bash
cargo test                                  # 65 unit tests: pacing, ring buffer,
                                            # resampler, registry, gamepad mapping,
                                            # pixel conversion, scaling
node scripts/core-abi-test.mjs              # 17 checks: the real core, headless
node scripts/serve.mjs 8123 &
PLAYWRIGHT_CORE=<path> node scripts/smoke-test.mjs http://localhost:8123/   # 40 checks
```

**`core-abi-test.mjs`** runs fceumm with the test ROM in plain Node — no browser, no
GPU, no Rust — and asserts on what actually came out:

- 256x240 at 60.0998 fps, 48 kHz, RGB565, negotiated through the environment protocol;
- 799 audio frames per video frame (48000/60.0998) with a non-zero peak, which is the
  ROM's 440 Hz square wave surviving the batch callback;
- the frame is green *and* has both dark and bright pixels — a flat fill would mean the
  pattern tables were ignored;
- holding A turns the average pixel red, releasing it turns it back: input reaching the
  CPU and changing the picture;
- holding Right shifts 112 pixels along scanline 124: per-frame register writes landing
  (sampled off the tile border, which is invariant under horizontal scroll);
- save state diverges then restores.

It takes under a second, which is what makes core work tractable.

**`smoke-test.mjs`** then checks the browser path, including that the real core reaches
`running`, reports its own A/V info to the bridge, advances ~90 frames per 1.5 s, pushes
~66,000 resampled audio frames through the Rust sink, and survives input and teardown —
plus every Phase 1 invariant (node counts, single loop, no 2D context, lazy cores) and
offscreen pixel verification of the GPU path.

### Known environment limitation

In headless Chromium on SwiftShader, touching a WebGPU canvas swapchain destroys the
device (`getCurrentTexture` → `device.lost: destroyed`), after which every GPU call
silently no-ops and the canvas reads back empty. Reproduced with hand-written JS
independent of this codebase, so it is not a bug here — but it is why the browser suite
verifies pixels through `captureFrame` (an offscreen render plus buffer readback) and
reports canvas compositing as an observation rather than a failure. It is also why the
core's *emulated* pixels are asserted in the Node harness instead. On real hardware the
same pipeline presents to the canvas.

## What's next

- **More cores.** `scripts/build-core.sh` already has an `mgba` stub; each additional
  core is a case block plus a manifest line. `WasmCore` is core-agnostic, so no Rust
  changes.
- **Core options.** `GET_VARIABLE` currently returns "unset", so cores use their
  defaults. Wiring `SET_VARIABLES` to a settings UI unlocks per-core configuration
  (region, overscan, palette).
- **Save-state persistence.** States round-trip through the core, but only in memory;
  the metadata store in `save-states.js` needs an IndexedDB payload store beside it.
- **Audio quality.** The resampler is linear; a windowed-sinc belongs there before
  anyone judges the sound.
- **Rewind and fast-forward.** The pacer already supports a speed multiplier, and
  save states are cheap (13 KB for NES) — a ring of them is most of a rewind feature.

## Phase 2: native iOS

The engine is already interface-agnostic: `bridge.rs` has no `wasm_bindgen`, no
`web_sys`, no JS types, and the renderer's `from_surface` takes any wgpu surface —
a `CAMetalLayer` works where a canvas does today. Phase 2 adds a UniFFI facade
beside `wasm.rs` (same methods, same error type) and a SwiftUI front end that makes
the same calls this JS does. What it must *not* do is reimplement pacing, input
mapping, audio buffering or session lifecycle — those live in Rust precisely so the
two platforms cannot drift.
