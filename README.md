# Continuum — multi-system emulator

A WebGPU emulator front end whose engine is a Rust crate, so that the native iOS phase
(sideloaded, JIT-capable cores) is a UI port rather than a rewrite.

**Two real cores now hot-swap cleanly.** `fceumm` (NES) and `mGBA` (GBA, plus GB/GBC)
are compiled from C to standalone WebAssembly, routed by ROM header, and swapped with
one core in memory at a time — verified by the collector, not by bookkeeping. The
Phase 1 scaffold is unchanged underneath; `v0.1.0-scaffold` marks that restore point.

![Library](docs/shot-library.png)

## What runs today

- **Two real cores.** NES via `fceumm`, GBA/GB/GBC via `mGBA` — both built by
  `scripts/build-core.sh` against wasi-libc as standalone wasm modules (no Emscripten,
  no RetroArch bundle). Each reports its own geometry, refresh and sample rate
  (256x240 / 60.0998 Hz / 48 kHz and 240x160 / 59.7275 Hz / 65536 Hz respectively), and
  those values — not the manifest's — configure the pacer, renderer and resampler.
- **Dynamic routing and clean swaps.** A `.nes` file gets fceumm, a `.gba` file gets
  mGBA, decided by header. Switching systems frees the previous core *before*
  instantiating the next: at no point are two cores alive, and the modules are actually
  reclaimed by the host.
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

## Multi-core memory discipline

The failure mode this design exists to prevent: each core is a wasm module with its own
linear memory — 5 MB for fceumm with a ROM loaded, **35 MB for mGBA** — so leaking one
per system switch exhausts a phone in a handful of swaps, and iOS kills processes on
memory pressure rather than paging.

Three mechanisms, and then three independent checks that they work.

**Mechanism.** `CoreRetention::Drop` is the default: ending a session frees its core
rather than keeping it warm. `CoreRegistry::unload_all_except` runs before a launch, so a
switch cannot leave a previous core resident. And `PlayerView.launch` tears down the
running session *before* fetching or instantiating the next core — otherwise launching
game B from inside game A would briefly hold both.

Teardown, in order: audio sink replaced (freeing the ring), input released, GPU
framebuffer texture dropped, save-state scratch freed, then the core itself. Dropping
`WasmCore` runs `retro_unload_game` → `retro_deinit` → frees the core's own allocations →
releases the last handle to its module. On the JS side `LibretroRuntime.destroy()` nulls
every cached typed-array view, because a single retained `Uint8Array` pins the whole
`ArrayBuffer` and therefore the whole core.

**Proof.** Bookkeeping alone would not be convincing, so `smoke-test.mjs` swaps NES↔GBA
three times, switches game-to-game without returning to the library, and then asserts:

| Question | How it is answered |
| --- | --- |
| Was every runtime torn down? | `instantiated === destroyed`, zero live |
| Were the modules *actually freed*? | `FinalizationRegistry` after a forced GC — the collector attests, not us |
| Were two ever alive at once? | `maxLive` high-water mark, which a post-hoc check would miss |
| Did the engine's own memory grow? | Rust wasm memory before vs after six sessions |

Current numbers: 9 runtimes instantiated, 9 destroyed, **9/9 collected**, high-water mark
1, peak 35.4 MB, engine memory 2.1 MB → 2.1 MB (+0.0). The status bar shows the same
figures live: browsing the library reads `0 resident · 0.0 MB live`.

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
- **WASI imports are few and stubbed.** fceumm needs 12 (all file syscalls); mGBA needs
  19, adding clocks, `environ` and directory calls. `clock_time_get` returns real time
  because mGBA drives the GBA's RTC from it; the rest fail cleanly, since the core is
  never given a path to open.
- **Cores need different build strategies.** fceumm exposes a libretro makefile listing
  its sources; mGBA is CMake-based, so `build-core.sh` configures it with wasi-sdk's
  toolchain file and links the resulting static archive. Adding a core is a case block in
  that script — nothing in Rust changes, because `WasmCore` is core-agnostic.

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
  roms/gba-testcart.gba         our own GBA ROM, built from roms/src/ by make-gba-rom.sh
  roms/src/                     GBA cart source: C, entry stub, linker script
  vendor/bridge/                build output of scripts/build-wasm.sh (gitignored)
scripts/
  build-wasm.sh                 cargo build + wasm-bindgen
  build-core.sh                 libretro core → standalone wasm (wasi-sdk); fceumm, mgba
  make-test-rom.py              6502 assembler + NES test ROM generator
  make-gba-rom.sh               ARM/C → GBA test ROM, header and checksum patched
  core-abi-test.mjs             headless core/ABI verification (no browser, no GPU)
  serve.mjs                     static server with correct wasm/ESM MIME types
  smoke-test.mjs                headless browser verification of the rules above
```

## Build and run

```bash
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.128   # must match the Cargo.lock version

./scripts/build-wasm.sh          # engine → web/vendor/bridge/
./scripts/build-core.sh all      # fceumm + mgba → web/cores/ (fetches wasi-sdk, ~1 GB)
python3 scripts/make-test-rom.py # NES test ROM → web/roms/nes-testcart.nes
./scripts/make-gba-rom.sh        # GBA test ROM → web/roms/gba-testcart.gba
node scripts/serve.mjs 8123      # → http://localhost:8123/
```

Then open the library and press **Play** on either *Continuum Test Cart* — the NES one
idles green, the GBA one blue — or drop your own `.nes`, `.gba`, `.gb` or `.gbc` file
anywhere on the page. Systems without a real core yet (SNES, Mega Drive, N64, PS1) fall
back to the placeholder core; a launch that needs an unbuilt core says which script to
run.

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
node scripts/core-abi-test.mjs              # 32 checks across both real cores, headless
node scripts/serve.mjs 8123 &
PLAYWRIGHT_CORE=<path> node scripts/smoke-test.mjs http://localhost:8123/   # 49 checks
```

**`core-abi-test.mjs`** runs each core with its own test ROM in plain Node — no browser,
no GPU, no Rust — and asserts on what actually came out. Per core:

- geometry, refresh and sample rate as the core reports them, and the pixel format it
  negotiated through the environment protocol;
- audio frames per video frame matching `sampleRate / fps`, with a non-zero peak — the
  ROM's 440 Hz tone surviving the batch callback;
- the idle frame is the expected colour *and* contains both dark and bright pixels, so a
  flat fill cannot pass;
- holding A changes the colour and releasing restores it: input reaching the CPU and
  changing the picture;
- a held direction moves the picture, proving per-frame register writes land;
- save state diverges then restores.

The two ROMs idle in **different colours on purpose** — NES green, GBA blue — so a frame
identifies which core drew it. That is what makes the hot-swap checks meaningful instead
of a matter of trusting counters.

**`smoke-test.mjs`** covers the browser path: every Phase 1 invariant (node counts,
single loop, no 2D context, lazy cores), offscreen GPU pixel verification, the real NES
core end to end, and the multi-core swap and leak checks described above.

### Known environment limitation

In headless Chromium on SwiftShader, touching a WebGPU canvas swapchain destroys the
device (`getCurrentTexture` → `device.lost: destroyed`), after which every GPU call
silently no-ops and the canvas reads back empty. Reproduced with hand-written JS
independent of this codebase, so it is not a bug here — but it is why the browser suite
verifies pixels through `captureFrame` (an offscreen render plus buffer readback) and
reports canvas compositing as an observation rather than a failure. It is also why the
cores' *emulated* pixels are asserted in the Node harness instead. On real hardware the
same pipeline presents to the canvas.

## What's next

- **More cores.** snes9x, Genesis Plus GX, Mupen64Plus and Beetle PSX are still
  placeholders. Each is a case block in `scripts/build-core.sh` plus a manifest line;
  `WasmCore` is core-agnostic, so no Rust changes. Expect the same two questions per
  core: which build system, and does it need `need_fullpath`.
- **Core options.** `GET_VARIABLE` currently returns "unset", so cores use their
  defaults. Wiring `SET_VARIABLES` to a settings UI unlocks per-core configuration
  (region, overscan, palette).
- **Save-state persistence.** States round-trip through the core, but only in memory;
  the metadata store in `save-states.js` needs an IndexedDB payload store beside it.
- **Audio quality.** The resampler is linear; a windowed-sinc belongs there before
  anyone judges the sound.
- **Rewind and fast-forward.** The pacer already supports a speed multiplier. NES states
  are 13 KB, but GBA states are 516 KB — a rewind ring needs a memory budget and probably
  compression, not just a deeper buffer.
- **Core options.** `GET_VARIABLE` returns "unset", so every core runs on defaults. mGBA
  in particular exposes useful ones (frameskip, colour correction, GB model).

## Next: native iOS

The engine is already interface-agnostic: `bridge.rs` has no `wasm_bindgen`, no
`web_sys`, no JS types, and the renderer's `from_surface` takes any wgpu surface —
a `CAMetalLayer` works where a canvas does today. The native build adds a UniFFI facade
beside `wasm.rs` (same methods, same error type) and a SwiftUI front end that makes the
same calls this JS does.

One thing that will differ: cores are instantiated by the platform layer, because only
JS can wire up another wasm module's imports. Natively they are `dlopen`'d or statically
linked instead, so `LibretroRuntimeHandle` gains a second implementation — while
`WasmCore`'s logic, the frame exchange in `cores/host.rs` and everything above the
`EmulatorCore` trait stay as they are. What it must *not* do is reimplement pacing, input
mapping, audio buffering or session lifecycle — those live in Rust precisely so the
two platforms cannot drift.
