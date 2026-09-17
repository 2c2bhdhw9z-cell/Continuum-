# Continuum — Phase 1 shell

A WebGPU emulator front end whose engine is a Rust crate, so that Phase 2 (a
sideloaded iOS app with JIT-capable cores) is a UI port rather than a rewrite.

This commit is the **Phase 1 shell**: the library UI, the virtualisation, and the
Rust bridge with its WebGPU renderer. There is no emulation yet, and no C/C++
bindings — every seam where they will attach is in place and exercised.

![Library](docs/shot-library.png)

## What actually runs today

- **The whole pipeline, end to end.** Launching a game fetches a core module,
  validates and instantiates it, starts a session, and drives
  input → core step → audio → GPU present from one `requestAnimationFrame` loop.
- **A stand-in core.** `cores/manifest.json` points every entry at
  `web/cores/diagnostic-core.wasm` (a valid, empty 8-byte module). Rust then
  instantiates a `DiagnosticCore` carrying that entry's *real* geometry, refresh
  rate and sample rate, which renders a test pattern, reacts to input, produces
  audio and supports save states. It contains no emulation — its job is to make the
  surrounding machinery real and measurable before any emulator exists.
- **4,800 synthetic catalogue entries** so the virtualisation is provable rather
  than plausible. See `web/src/data/catalog.js`.

## The five architectural rules, and where they are enforced

| Rule | Enforced by | Verified by |
| --- | --- | --- |
| 1. UI talks to cores only through a Rust `EmulatorBridge` | `crates/emulator-bridge/src/bridge.rs`; the wasm facade in `wasm.rs` is a thin, logic-free wrapper | JS never imports anything but `engine/bridge-host.js` |
| 2. WebGPU only — no 2D canvas, no DOM rendering | `crates/emulator-bridge/src/gfx/renderer.rs` + `frame_blit.wgsl`; wasm builds compile only wgpu's `webgpu` backend, so there is no WebGL2 fallback to fall into | smoke test traps `getContext('2d')` for the whole session |
| 3. One unified `rAF` loop | `web/src/engine/loop.js` — the only `requestAnimationFrame` in the app; it also flushes the UI, after the engine tick | smoke test counts rAF registrations and asserts the loop idles |
| 4. Strict virtualisation, no O(n) DOM | `web/src/ui/virtual-scroller.js`, used four times: shelf list, cards within each shelf, all-games grid, save states | smoke test scrolls 4,800 entries and asserts node counts do not change |
| 5. No core loaded at boot | `web/src/engine/core-loader.js` declares metadata only; `CoreRegistry::attach_module` is the sole path to a runnable core | smoke test asserts `residentCoreCount === 0` at boot, `1` after launch |

## Layout

```
crates/emulator-bridge/         Rust engine. Compiles to wasm32 (Phase 1) and
  src/bridge.rs                 native ARM64 (Phase 2) from the same source.
  src/wasm.rs                   wasm-bindgen facade (wasm32 only)
  src/cores/                    EmulatorCore trait, lazy registry, stand-in core
  src/gfx/                      wgpu renderer, WGSL blit, pixel-format conversion
  src/audio/                    AudioSink, ring buffer, resampler
  src/{input,timing,frame}.rs   input state, frame pacing, geometry
web/
  index.html  styles/           dark Netflix-style shell
  src/ui/virtual-scroller.js    the virtualisation primitive
  src/ui/{library,player}-view.js, detail-sheet.js, card.js, art.js
  src/engine/{loop,bridge-host,core-loader,input}.js
  src/audio/{audio-output,pcm-worklet}.js
  src/data/                     catalogue, systems, save states, content store
  cores/manifest.json           core declarations (metadata only)
  vendor/bridge/                build output of scripts/build-wasm.sh (gitignored)
scripts/
  build-wasm.sh                 cargo build + wasm-bindgen
  serve.mjs                     static server with correct wasm/ESM MIME types
  smoke-test.mjs                headless browser verification of the rules above
```

## Build and run

```bash
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.128   # must match the Cargo.lock version

./scripts/build-wasm.sh          # → web/vendor/bridge/
node scripts/serve.mjs 8123      # → http://localhost:8123/
```

Needs a WebGPU browser: Chrome/Edge 113+, Safari 18+, Firefox 141+. There is
deliberately no fallback renderer; without WebGPU the UI says so and refuses to
launch.

**Controls.** Arrows = D-pad, `Z`/`X` = B/A, `A`/`S` = Y/X, `Q`/`W` = L/R,
`Enter` = Start, `Shift` = Select, `P` = pause, `Esc` = back. Gamepads are polled
inside the engine tick; touch devices get an on-screen pad.

## Verification

```bash
cargo test                                  # 53 unit tests: pacing, ring buffer,
                                            # resampler, registry, conversion, scaling
node scripts/serve.mjs 8123 &
PLAYWRIGHT_CORE=<path> node scripts/smoke-test.mjs http://localhost:8123/
```

The smoke test drives a real browser and checks 32 properties, including:

- 4,800 titles render in ~105 card nodes, and scrolling 16,000 px adds none;
- a shelf of 471 titles uses 15 card nodes; a 129-state list uses 10 row nodes;
- the launch path reaches `running`, with the core loaded only at launch;
- frames advance under the loop (~90 in 1.5 s), pause halts them, resume continues;
- ~66,000 audio frames/1.5 s flow core → sink → host, matching 44.1 kHz;
- save state then load rewinds the core's frame counter;
- **pixel-level**: a GPU readback of the rendered frame is checked for the core's
  top-left marker (`242,63,89` — proving blit orientation and channel order) and for
  correct letterbox bars (`5,5,8`) on a mismatched target aspect.

### Known environment limitation

In headless Chromium on SwiftShader, touching a WebGPU canvas swapchain destroys
the device (`getCurrentTexture` → `device.lost: destroyed`), after which every GPU
call silently no-ops and the canvas reads back empty. Reproduced with hand-written
JS independent of this codebase, so it is not a bug here — but it is why the smoke
test verifies pixels through `captureFrame` (an offscreen render + buffer readback)
instead of screenshotting the canvas, and reports canvas compositing as an
observation rather than a failure. On real hardware the same pipeline presents to
the canvas.

`captureFrame` is not test scaffolding: save-state thumbnails and screenshots need
exactly this, and it renders through the same pipeline, shader and scaling maths as
`present`.

## Phase 1b: mounting real cores

1. Build libretro cores to `wasm32-unknown-unknown`, drop them in `web/cores/`.
2. Per entry in `cores/manifest.json`: point `module` at the real file, set
   `sizeBytes`, remove `"placeholder": true`. The geometry, `targetFps`,
   `audioSampleRate` and `pixelFormat` values there are already correct per system.
3. Implement `instantiate()` in `crates/emulator-bridge/src/cores/registry.rs` —
   `retro_set_environment` → `retro_init` → `retro_get_system_av_info`, wrapped in
   an `EmulatorCore` impl. **This is the only Rust change required.** Libretro's
   `audio_sample_batch` maps straight onto `AudioSink::submit_i16`, and its
   framebuffer callback onto `FrameView`.
4. Replace the linear resampler in `src/audio/resample.rs` with a windowed-sinc.
5. Back `web/src/data/content-store.js` with IndexedDB/OPFS plus a file-import flow;
   the async `Uint8Array` interface it already exposes is the final one.

Every one of these is marked `TODO(phase1b)` in the source.

## Phase 2: native iOS

The engine is already interface-agnostic: `bridge.rs` has no `wasm_bindgen`, no
`web_sys`, no JS types, and the renderer's `from_surface` takes any wgpu surface —
a `CAMetalLayer` works where a canvas does today. Phase 2 adds a UniFFI facade
beside `wasm.rs` (same methods, same error type) and a SwiftUI front end that makes
the same calls this JS does. What it must *not* do is reimplement pacing, input
mapping, audio buffering or session lifecycle — those live in Rust precisely so the
two platforms cannot drift.
