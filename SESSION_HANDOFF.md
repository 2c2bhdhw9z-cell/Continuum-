# Continuum — Session Handoff

State of the project at tag `v0.2.0-multicore`, written to be the only document a new
session needs to read before changing anything.

Continuum is an all-in-one emulator PWA. Three real libretro cores run as standalone
WebAssembly modules; a Rust engine owns pacing, input, audio and presentation; the
front end is a strictly virtualised Netflix-style library. The same Rust crate is
intended to compile for native ARM64 later, which is why so much logic that could have
lived in JavaScript does not.

## The five rules everything is built around

1. **The Rust bridge.** The UI never talks to a core. It talks to `EmulatorBridge`,
   exposed to the web through `wasm-bindgen`. Anything the native build will also need
   — pacing, input mapping, resampling, core selection — lives in Rust.
2. **WebGPU only.** No `CanvasRenderingContext2D`, no DOM-based rendering. The browser
   test asserts this by monkey-patching `getContext` and failing on `'2d'`.
3. **One loop.** A single `requestAnimationFrame` drives input → core step → audio →
   GPU present, on the main thread.
4. **DOM virtualisation.** Node count is a function of viewport size, never of item
   count. 4,800 catalogue titles and 12 titles allocate the same DOM.
5. **Dynamic core loading.** No core binary is fetched or instantiated at boot.

Rules 2, 3, 4 and 5 each have a browser assertion, so breaking one fails CI rather than
just being noticed later.

---

## 1. Cores: WASI-libc plus a C shim, not Emscripten

### Why not Emscripten

Emscripten is the usual way to put a libretro core in a browser, and it is the wrong
tool here. Its libretro builds are whole-RetroArch bundles: they bring their own main
loop, own the canvas via SDL/GL, and own the audio graph. Those are precisely the three
things this architecture keeps in Rust. Adopting Emscripten would mean either fighting
its runtime for control of the frame, or conceding rules 1, 2 and 3.

There is a second reason. An Emscripten core expects an Emscripten host — a JS glue
file, a specific heap layout, `EM_ASM` escapes into JavaScript. That contract does not
survive the move to native ARM64, so the Phase 2 build would need a completely
different loading path.

### What we do instead

Each core is compiled with **wasi-sdk 25.0** (clang 19) as a *reactor* module against
wasi-libc, exporting the plain libretro C API:

```
scripts/build-core.sh <core>
  → clang --target=wasm32-wasi -mexec-model=reactor
  → web/cores/<core>.wasm
```

A reactor has no `main`. The host calls `_initialize` once to run the libc
initialisers, then drives `retro_*` directly. The resulting module imports **only**:

| Import module | Count | What it is |
|---|---|---|
| `host.*` | 5 | `video_refresh`, `audio_batch`, `input_poll`, `input_state`, `environment` |
| `wasi_snapshot_preview1.*` | 12–19 | clock/fd syscalls, stubbed by the JS runtime |

`build-core.sh` enforces that: after linking it rejects any import outside those two
modules, so a stray dependency is a build error rather than a blank screen.

### Why there is a C shim

`core-shim/libretro_wasm_shim.c` is compiled **into** every core. It exists because of
one hard wasm constraint: **a host cannot manufacture a function pointer inside another
module's table.** libretro's `retro_set_video_refresh(cb)` wants a C function pointer
valid in the core's own address space, and JavaScript has no way to produce one.

So the shim defines real C functions inside the core that forward to `host.*` imports,
and `shim_install` registers those with the core. It also flattens the structs whose
layout changes between libretro API revisions (`retro_game_info`,
`retro_system_av_info`) into plain arrays, keeping the offset arithmetic on the side
where the compiler checks it. `shim_abi_version` guards against a stale core binary.

Rejected alternatives: patching each core's source (unmaintainable across three cores
and their upstreams), and building a JS-side function table (does not exist as a
mechanism).

### The three cores

| Core | Systems | Size | Real A/V |
|---|---|---|---|
| `fceumm` | nes | 1.68 MB | 256×240, 60.0988 fps, 48000 Hz, RGB565 |
| `mgba` | gba, gb, gbc | 1.52 MB | 240×160, 59.7275 fps, 65536 Hz, RGB565 |
| `genesis_plus_gx` | genesis, sms | 2.79 MB | 256×192 (SMS), 59.9227 fps, 44100 Hz, RGB565 |

`build-core.sh` has two strategies, because libretro cores do not agree on a build
system:

- **`sources`** — ask the core's own makefile for `SOURCES_C` and compile the list
  directly (fceumm: 488 objects, genesis_plus_gx: 115).
- **`cmake`** — configure with wasi-sdk's toolchain file and build the static library
  target (mgba, which also generates files the build needs).

### Per-core traps already paid for

Each of these cost real debugging time. They are in the script with comments, but worth
knowing before adding a fourth core:

- **`-DFRONTEND_SUPPORTS_RGB565`** is what makes Genesis Plus GX *negotiate* RGB565.
  `USE_16BPP_RENDERING` only makes it *render* 16bpp. Without the first flag the core
  never calls `SET_PIXEL_FORMAT`, the host keeps libretro's 0RGB1555 default, and the
  colours are quietly wrong.
- **`INLINE` must be forced to `static inline`.** libretro-common's `retro_inline.h` is
  reached before `core/macros.h` and picks bare `inline` under C17. A C99 `inline`
  definition emits no external symbol, so every call the optimiser declined to inline
  became an undefined symbol at link.
- **`-mllvm -wasm-enable-sjlj` plus `-lsetjmp`.** The Musashi 68000 uses
  `setjmp`/`longjmp` for address-error traps. The flag lowers them; `libsetjmp.a`
  provides `__wasm_setjmp`/`__wasm_longjmp`, without which they arrive as `env.*`
  imports the host cannot satisfy.
- **Flattened object names must not start with a dot.** Genesis Plus GX lists sources
  as `./core/x.c`; `tr '/' '_'` turned that into `._core_x.o`, a dotfile that `*.o`
  never matches. Objects are now collected with `find`, and the count is checked
  against the source list.
- **`-DHAVE_NO_LANGEXTRA`** drops ~28 localised core-option tables (~1.7 MB of
  initialised data in a module the browser downloads on demand). We never surface
  libretro core options.
- **`retro_game_info_ext.full_path` must be a non-empty filename.** Genesis Plus GX
  decides *which system it is* from the last three characters of that path. With NULL
  it read out of bounds and defaulted to Mega Drive — a Master System cart booted as a
  Mega Drive, running Z80 bytes on the 68000. The runtime now passes
  `"<name>.<ext>"`, deliberately with no directory component, and `dir` is `""` rather
  than NULL because cores `strncpy` it unconditionally.

### Environment protocol

`web/src/engine/core-runtime.js` implements the libretro environment calls the cores
actually use, notably `SET_PIXEL_FORMAT` (10), `SET_CONTENT_INFO_OVERRIDE` (65) and
`GET_GAME_INFO_EXT` (66). The last two are what let content be loaded **from memory**:
fceumm advertises `need_fullpath = true` and only relaxes it through the override
mechanism, and a browser has no file path to give. Everything else is explicitly
acknowledged or explicitly refused, with a comment saying which and why.

Two things to know before touching that switch:

- **Command numbers are worth machine-checking.** Several are `N | RETRO_ENVIRONMENT_EXPERIMENTAL`
  (`0x10000`), and a constant missing that bit produces a `case` that can never match.
  An audit against `libretro.h` found three wrong: `SET_MEMORY_MAPS` and
  `GET_AUDIO_VIDEO_ENABLE` had lost the experimental bit, and `SET_SERIALIZATION_QUIRKS`
  was numbered 62 — which is actually `SET_AUDIO_BUFFER_STATUS_CALLBACK`. That last one
  was the real defect: we were answering "yes" to a core asking to be *called back*
  when audio ran low, and then never calling it, which is worse than refusing. Re-run
  that comparison after adding any command.
- **Refuse loudly-called commands by name.** fceumm asks for
  `GET_CURRENT_SOFTWARE_FRAMEBUFFER` on *every frame*. Falling through to the default
  branch logged 60 lines a second, which buries the messages that matter. All three
  cores now produce zero unhandled-command logs while running.

---

## 2. The frame: one loop, wgpu, and an AudioWorklet ring

### The loop (`web/src/engine/loop.js`)

One `requestAnimationFrame`, one order, every frame:

1. engine tick — input → core step(s) → audio submit → GPU present
2. audio pump — move PCM from the Rust ring to the output device
3. UI flush — virtual scrollers reconcile their windows

The emulator goes first because a late present is a visible stutter while a late shelf
re-render is imperceptible. Scrolling is flushed from this same loop rather than its
own rAF, so a burst of scroll events cannot starve emulation.

The loop **idles**. A permanently spinning rAF would drain battery while someone reads
their library, so it stops when there is no work and is restarted by `wake(frames)`
from whatever created the work. It stays awake three extra frames to cover scroll
inertia. While a game runs it never idles. The browser test asserts rAF registrations
stay flat while the library sits still.

### Pacing (`crates/emulator-bridge/src/timing.rs`)

Displays tick at 60/120/144 Hz; cores run at 60.0988, 59.7275, 59.9227, 50. `FramePacer`
answers one question per tick — *how many core steps does this tick owe?* — from a
millisecond accumulator. Two limits keep it honest:

- `MAX_CATCH_UP_STEPS = 4`: beyond that, emulated time is abandoned rather than
  blocking the main thread.
- `STALL_THRESHOLD_MS = 500`: a longer gap (hidden tab, GC pause, breakpoint) is a
  stall, and emulated time *resynchronises* instead of fast-forwarding several seconds.

### WebGPU (`crates/emulator-bridge/src/gfx/renderer.rs`)

`wgpu` owns the surface. `Renderer::from_canvas` creates the instance, requests an
adapter against the canvas surface, requests a device with
`required_limits: adapter.limits()` (asking for more than the adapter offers fails on
low-end mobile), configures with `PresentMode::Fifo` and `CompositeAlphaMode::Auto`,
and installs an uncaptured-error handler — validation errors are otherwise silent on
the web.

Per frame the core's framebuffer is uploaded to a texture and drawn by
`gfx/frame_blit.wgsl`, which handles aspect-fit letterboxing and nearest/linear
filtering. RGB565 and XRGB8888 are both normalised on the way in (`gfx/convert.rs`).

> **The bug worth remembering:** `_instance` and `_adapter` are struct fields that are
> never read, and they are load-bearing. Dropping them after construction lets the
> browser collect the objects the device and surface came from, and the canvas goes
> black with no error. If a frame ever renders blank again, check those first.

### Audio (`crates/emulator-bridge/src/audio/`, `web/src/audio/`)

Audio is a first-class part of the engine, not an afterthought bolted onto the video
path. `AudioSink` is a trait, so the native build can swap the implementation:

```rust
trait AudioSink {
    fn spec(&self) -> AudioSpec;
    fn submit_f32(&mut self, interleaved: &[f32]);
    fn submit_i16(&mut self, interleaved: &[i16]);   // libretro's native format
    fn drain(&mut self, dst: &mut [f32]) -> usize;
}
```

`RingAudioSink` wraps `AudioRing`: a `Box<[f32]>` allocated once per session, single
writer (the core step), single reader (the host drain), same thread — so no locks and
no atomics. Nothing in the audio path touches the heap after construction, which is
what keeps a 60 Hz tick free of allocator jitter. Cores' native sample rates
(48000/65536/44100) are resampled to the device rate in `audio/resample.rs`.

On the JS side, `pcm-worklet.js` is an `AudioWorkletProcessor` on the audio render
thread, which has a hard real-time deadline (~2.7 ms per 128-frame quantum at 48 kHz).
It allocates nothing after construction and never logs on the hot path. The main thread
**transfers** an `ArrayBuffer` of samples in and the worklet transfers the emptied
buffer straight back; that recycling is what makes the per-frame hand-off
allocation-free on both sides. On underrun it pads with silence rather than repeating
stale audio — a short gap is far less noticeable than a click loop — and counts the
underrun so the HUD shows it.

The worklet has no `import` statements on purpose: static-import support in worklets
has been uneven across Safari versions, and a failed worklet load means silence with no
obvious cause.

**iOS autoplay is handled deliberately.** The `AudioContext` is *created* lazily inside
a user gesture, never at page load, and `player-view.js` starts the audio graph before
awaiting anything else in the launch path — awaiting first is exactly what leaves iOS
silent. The unlock listeners are not `{ once: true }`, because iOS re-suspends the
context after interruptions (a call, the ringer switch), and a one-shot unlock leaves
the app permanently mute afterwards.

---

## 3. Input isolation and DOM virtualisation

### `GamepadBridge` (`crates/emulator-bridge/src/input/gamepad.rs`)

Four input sources have to become one retro pad: a physical gamepad polled through the
Gamepad API, a keyboard, an on-screen touch pad, and (Phase 2) iOS `GameController`.
All of that translation is in Rust — including the W3C "standard gamepad" layout, which
would otherwise have to be reimplemented identically in Swift and would inevitably
drift.

The front end sends deliberately dumb messages:

```
keyboard      → setButton(port, source, button, pressed)
touch overlay → setButton(port, source, button, pressed)
gamepad poll  → applyGamepad(port, buttons, axes)
```

**Why state is tracked per source.** The Gamepad API has no events, so a connected pad
is polled every frame, and each poll reports the *complete* state of that pad —
including "D-pad not pressed". With a single shared button field, that poll would clear
a D-pad press the keyboard was holding, 60 times a second: plugging in a controller
would appear to break the keyboard. So each `PadSource` keeps an independent layer and
`snapshot()` merges them — buttons OR-ed, largest-magnitude axis wins. A full poll can
then overwrite its own layer without touching anyone else's. `AXIS_DEADZONE = 0.35`
rejects stick drift and is also the threshold at which an analog stick synthesises
D-pad presses.

This was caught by a unit test, not by a person noticing their keyboard had stopped
working. Keep that test.

### `VirtualScroller` (`web/src/ui/virtual-scroller.js`)

One primitive, four call sites: the vertical stack of shelves, the horizontal card
strip inside each shelf, the all-games grid, and the save-state list.

**Modulo recycling.** A fixed pool of `poolSize` nodes is created once and *never*
detached. Item `i` always lives in slot `i % poolSize`, and because `poolSize` is
strictly greater than the number of simultaneously visible items, two visible items can
never collide in one slot. Scrolling therefore costs one `transform` write per
repositioned node plus one `bindNode` call per node whose index actually changed — no
insertions, no removals, no layout thrash. Off-window nodes are not even hidden; they
are positioned off-screen and reused moments later. `MAX_POOL = 96` caps the damage if
size arithmetic ever goes wrong, and `assertBounded()` makes the guarantee testable.

**The rule binding depends on:** binding must never change a card's size. Titles are
line-clamped in CSS and the art box has a fixed aspect ratio, so a long name cannot
push a row taller and desynchronise the scroller's arithmetic.

`scroll` events fire faster than frames, so the scroller only marks itself dirty and
lets the shared loop call `flush()`.

Measured: 4,800 titles → 105 card nodes and 7 shelf nodes, constant while scrolling. A
471-title shelf uses 15 nodes. A 129-state save list uses 10 rows.

---

## 4. Memory: `CoreRetention::Drop`

A core is not small. mGBA with a ROM loaded measures **35.4 MB** of linear memory;
Genesis Plus GX **14.8 MB**; fceumm **5 MB**. Leaking one per system switch exhausts a
phone in a handful of swaps, and iOS terminates on memory pressure rather than paging.

```rust
pub enum CoreRetention {
    Drop,      // default: free the core when its session ends
    KeepWarm,  // keep it instantiated for a fast relaunch
}
```

`Drop` is the default. `KeepWarm` exists for a single-system build or a desktop with
memory to spare, and is not what ships. An idle mGBA holding 35 MB to save a ~200 ms
relaunch is a bad trade on a phone.

Two mechanisms enforce one-core-at-a-time:

1. **`registry.unload_all_except(Some(core_id))` runs *before* the new core is fetched
   or instantiated.** Ordering is the whole point: instantiate-then-free would hold two
   cores at once, and an NES core plus a GBA core is the ~40 MB spike that gets a
   process killed.
2. **Dropping `Box<dyn EmulatorCore>` is what actually frees memory.** For a real core
   that runs `WasmCore::drop` → `retro_unload_game`, `retro_deinit`, free the
   allocations, then release the last handle to the wasm module so the host can collect
   its entire linear memory.

Because "we called `destroy()`" is not evidence of anything, `core-runtime.js` keeps a
`FinalizationRegistry` and a `runtimeStats` ledger (`instantiated`, `destroyed`,
`collected`, `liveBytes`, `peakLiveBytes`, `maxLive`, `live`). The browser test cycles
NES → GBA → Master System three times and then asserts:

- 12 instantiated, 12 destroyed, 0 live
- **12 of 12 collected** after a forced GC (`--js-flags=--expose-gc`)
- `maxLive === 1` — never two cores alive at any instant
- engine memory 2.1 MB → 3.1 MB across 9 sessions (wasm memory never shrinks, so the
  test is that it stops growing; the one-time +1.0 MB is the third core's staging
  buffer, not per-swap growth)

---

## 5. Subcores: one system, many cores

Added in this release. A system maps to *many* cores — `gb` runs on mGBA or Gambatte,
`sms` on Genesis Plus GX or SMS Plus GX — and neither answer is wrong, so the user gets
to choose.

`CoreDescriptor` gained `priority: i32` (higher wins, ties broken by core id so the
list never reshuffles). The manifest convention is **100 for a real core, 10 for a
placeholder**, which structurally prevents a diagnostic stand-in from becoming the
default for a system a real core already covers.

The registry owns the decision:

```rust
cores_for_system(system_id)                  -> Vec<&CoreDescriptor>  // best first
core_for_system(system_id)                   -> Option<&CoreDescriptor>
resolve_core_for_system(system_id, preferred) -> Option<&CoreDescriptor>
```

`resolve_core_for_system` treats `preferred` as a hint, not an instruction. A stored
preference naming a core that is undeclared, or that does not run this system, is
ignored in favour of the default. That matters because the preference comes from
`localStorage` and can outlive a manifest change: a stale string must never make a game
unlaunchable, and must never hand content to a core that cannot run it.

`web/src/data/core-prefs.js` is storage only — it deliberately does **not** validate
ids, because Rust does, and a second copy of that rule in JS would be a second thing to
keep correct. It falls back to in-memory state when `localStorage` throws (Safari
private browsing, storage disabled by policy).

Two UI affordances, one stored choice per system:

- a `<select>` in the detail sheet, shown only when the system has ≥2 cores;
- right-click (pointer) or 500 ms long-press (touch) on a ROM card → "Play with…",
  delegated from the scroll container because cards are recycled.

Both write the same preference and then launch, so there is only ever one answer to
"which core will this use".

---

## 6. Building and verifying

```bash
# Rust engine → web/vendor/bridge/  (~428 KB)
./scripts/build-wasm.sh

# Cores → web/cores/*.wasm   (fetches wasi-sdk 25 into .tools/ on first run)
./scripts/build-core.sh fceumm | mgba | genesis_plus_gx | all

# Test ROMs → web/roms/
python3 scripts/make-test-rom.py     # nes-testcart.nes  (24592 B)
./scripts/make-gba-rom.sh            # gba-testcart.gba  (1144 B)
python3 scripts/make-sms-rom.py      # sms-testcart.sms  (32768 B)

# Verification, fastest first
cargo test                           # 70 unit tests
cargo fmt --all --check
cargo clippy --all-targets           # zero warnings
node scripts/core-abi-test.mjs       # 48 checks, 3 cores, no browser
node scripts/capture-frames.mjs      # docs/frame-{nes,gba,sms}[-a].png

# Browser suite (58 checks). One shell invocation: /tmp and background jobs
# do not survive between tool calls.
node scripts/serve.mjs 8123 &
PLAYWRIGHT_CORE=/tmp/pw/node_modules/playwright-core \
  node scripts/smoke-test.mjs http://localhost:8123/
```

`core-abi-test.mjs` is the loop to live in: ~1 second, no browser, no GPU, no Rust, and
it only fails when a core contract is genuinely broken. The browser suite takes ~40 s
and can fail for a dozen unrelated environmental reasons.

### The test ROMs are the measuring instrument

All three carts are written from scratch in this repository (no commercial ROMs), and
each idles in a **different colour** so a captured frame identifies which core drew it
from pixels alone:

| Cart | Idle | Button held | Also proves |
|---|---|---|---|
| `nes-testcart.nes` | green | red | scroll on Left/Right, 440 Hz APU tone |
| `gba-testcart.gba` | blue | yellow | marker slides, 440 Hz tone |
| `sms-testcart.sms` | magenta | cyan | scroll on Right, 440 Hz PSG tone |

That is what makes the hot-swap checks meaningful rather than a matter of trusting
bookkeeping. The SMS cart is hand-assembled by a small Z80 assembler inside
`make-sms-rom.py`; note `PROGRAM_ORIGIN = 0x0070`, because the first version laid code
across `0x0000` and then wrote the NMI handler at `0x0066` on top of its own
tile-upload loop — the PSG still played, so the cart looked alive while rendering a
black screen.

### Known environment limitation

Headless Chromium with SwiftShader destroys the WebGPU device on `getCurrentTexture`,
so the canvas reads back empty and any later `captureFrame` cannot map its buffer. This
is reproducible in plain JavaScript with no Rust involved — it is not our bug. Pixels
are therefore verified two ways that do not depend on canvas compositing: offscreen GPU
readback early in the run, and the Node ABI harness. The browser suite reports the
affected checks as `INFO` rather than pretending to assert them.

---

## 7. Project structure

```
crates/emulator-bridge/src/
  lib.rs          bridge.rs        EmulatorBridge, CoreRetention, session lifecycle
  wasm.rs         the wasm-bindgen facade — the entire JS-visible API surface
  timing.rs       FramePacer
  frame.rs        FrameGeometry, PixelFormat, FrameView
  error.rs        BridgeError
  cores/
    mod.rs        EmulatorCore trait, CoreDescriptor, ContentHint
    registry.rs   CoreRegistry: declare / attach / take / unload, subcore resolution
    wasm_core.rs  real libretro core behind the trait
    host.rs       CoreHost — the five host.* callbacks, thread-local frame exchange
    diagnostic.rs pattern-generating stand-in for placeholder entries
  gfx/            renderer.rs (wgpu), convert.rs, frame_blit.wgsl
  audio/          mod.rs (AudioSink), ring.rs, resample.rs
  input/          mod.rs (Button, InputSnapshot), gamepad.rs (GamepadBridge)

core-shim/libretro_wasm_shim.c   callback trampolines, compiled into every core

web/
  index.html  sw.js  manifest.webmanifest
  cores/      manifest.json + the built .wasm cores
  roms/       the three test carts (+ src/ for the GBA cart)
  styles/     tokens.css base.css shell.css library.css player.css
  vendor/     bridge/  ← wasm-bindgen output, generated
  src/
    main.js
    engine/   bridge-host.js core-loader.js core-runtime.js input.js loop.js
    ui/       library-view.js virtual-scroller.js card.js detail-sheet.js
              player-view.js core-menu.js rom-import.js art.js toast.js
    data/     catalog.js builtins.js systems.js rom-store.js rom-detect.js
              content-store.js save-states.js core-prefs.js
    audio/    audio-output.js pcm-worklet.js

scripts/  build-wasm.sh build-core.sh make-*-rom.* serve.mjs
          core-abi-test.mjs smoke-test.mjs capture-frames.mjs
```

Roughly 6,300 lines of Rust and 6,000 of JavaScript. `.tools/` (wasi-sdk) and `.work/`
(core checkouts and objects) are gitignored and rebuilt on demand.

### Where the two sides meet

Worth internalising, because it is the seam every future change crosses:

- **JS instantiates a real core** (`core-runtime.js`), because only JS can wire up
  another wasm module's imports, then hands Rust an opaque handle via
  `attachCoreRuntime`. `CoreRegistry` cannot tell the difference between a core that
  arrived this way and the diagnostic stand-in — which is why adding real cores changed
  almost nothing above that line.
- **The hot path is zero-copy.** `tick` and `drainAudioInto` write into `Box<[T]>`
  buffers owned by the bridge, which JS reads through typed-array views over wasm
  memory. Returning a `#[wasm_bindgen]` struct or taking `&mut [f32]` would each cost
  an allocation per frame. The views' only failure mode is wasm memory growth detaching
  the `ArrayBuffer`, which the host detects by checking `buffer.byteLength === 0` and
  re-creating the view.
- **Telemetry is 12 `f64` slots** at a stable pointer, published with
  `telemetryLayout()` so the two sides cannot drift.

---

## 8. Where to go next

Nothing is blocked. In rough order of value:

1. **A fourth real core.** `snes9x` is the obvious gap. It is C++, so it needs
   `libc++`/`libc++abi` from wasi-sdk and exception handling — a new axis for
   `build-core.sh`, which currently only builds C. `gambatte` is C++ too, and promoting
   it from placeholder to real would make the `gb` subcore choice a genuine one.
2. **A Mega Drive test cart.** Genesis Plus GX is verified in Master System mode only.
   A 68000 cart would cover the other half of the core, and the extension-driven system
   switch in both directions.
3. **Save-state persistence to IndexedDB.** States round-trip through the core today
   but do not survive a reload; `data/save-states.js` and `data/rom-store.js` already
   have the shape for it.
4. **Native ARM64.** `EmulatorCore` is the seam: implement it over `dlopen`ed or
   statically linked cores, swap `AudioSink` for CoreAudio, point `wgpu` at a
   `CAMetalLayer`. `timing.rs`, `input/`, `audio/` and `cores/registry.rs` should need no
   changes. The tokens in `styles/tokens.css` are meant to become a Swift `Theme`
   struct.
5. **Core options.** `HAVE_NO_LANGEXTRA` is set and `GET_VARIABLE` returns nothing, so
   every core runs on defaults. Exposing options means a UI, persistence, and deciding
   which of the dozens per core are worth showing.

### Things not to undo

- `_instance` / `_adapter` in `Renderer` (§2) — deleting them blanks the canvas.
- Per-source input layers in `GamepadBridge` (§3) — merging them breaks the keyboard
  whenever a pad is connected.
- Freeing the old core *before* fetching the new one (§4) — reordering doubles peak
  memory.
- `full_path` in `retro_game_info_ext` (§1) — emptying it makes Genesis Plus GX
  misidentify the system.
- Starting the audio graph before the first `await` in the launch path (§2) — moving it
  later leaves iOS silent.
