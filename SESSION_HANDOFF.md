# Continuum — Session Handoff

> **Scope, read first.** The product is the sideloadable iOS `.ipa`, and it is the only
> deliverable. It ships five libretro cores covering nine systems (§17). Everything under `web/`
> is legacy scaffolding: its job was to prove the Rust engine before there was any way to compile
> for the device, that job is finished, and it is pending removal. It is not a second supported
> target. The web material in this document is retained for its engineering history, because the
> reasoning, the traps paid for and the invariants it records are the same ones the native build
> depends on. Anything below that calls the project a PWA is describing that history, not the
> plan. Authoritative scope: `.kiro/steering/product-scope.md`. Plain-language overview for the
> repo owner: `README.md`. On-device test checklist: `TESTING.md`.

State of the project at tag `v0.6.0-library` for the web material, plus §16 and §17 for the iOS
build, written to be the only document a new session needs to read before changing anything.

Continuum is an all-in-one emulator for iPhone. A Rust engine owns pacing, input, audio and
presentation, and the platform layer loads the cores. On iOS those are five `dlopen`ed libretro
dylibs staged into `Frameworks/` (§17). In the legacy browser build they were four standalone
WebAssembly modules, three C and one C++, behind a strictly virtualised Netflix-style library;
that front end survives only as the design reference for the SwiftUI UI. The reason so much
logic lives in Rust rather than in JavaScript is exactly that the same crate now compiles for
native ARM64.

## The five rules everything is built around

1. **The Rust bridge.** The UI never talks to a core. It talks to `EmulatorBridge`,
   exposed to the web through `wasm-bindgen`. Anything the native build will also need
   — pacing, input mapping, resampling, core selection — lives in Rust.
2. **WebGPU only.** No `CanvasRenderingContext2D`, no DOM-based rendering. The browser
   test asserts this by monkey-patching `getContext` and failing on `'2d'`.
3. **One loop.** A single `requestAnimationFrame` drives input → core step → audio →
   GPU present, on the main thread.
4. **DOM virtualisation.** Node count is a function of viewport size, never of item
   count. A library of 12 titles and one of 4,000 allocate the same DOM.
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

Rejected alternatives: patching each core's source (unmaintainable across four cores and
their upstreams), and building a JS-side function table (does not exist as a mechanism).

### The four cores

| Core | Systems | Lang | Size | Real A/V |
|---|---|---|---|---|
| `fceumm` | nes | C | 1.68 MB | 256×240, 60.0988 fps, 48000 Hz, RGB565 |
| `mgba` | gba, gb, gbc | C | 1.52 MB | 240×160, 59.7275 fps, 65536 Hz, RGB565 |
| `genesis_plus_gx` | genesis, sms | C | 2.79 MB | 256×192 (SMS), 59.9227 fps, 44100 Hz, RGB565 |
| `snes9x` | snes | C++ | 3.14 MB | 256×224, 60.0988 fps, 32040 Hz, RGB565 |

`build-core.sh` has two strategies, because libretro cores do not agree on a build
system:

- **`sources`** — ask the core's own makefile for `SOURCES_C` *and* `SOURCES_CXX` and
  compile the lists directly (fceumm: 488 objects, genesis_plus_gx: 115, snes9x: 31 C +
  24 C++).
- **`cmake`** — configure with wasi-sdk's toolchain file and build the static library
  target (mgba, which also generates files the build needs).

### C++ cores

Each source is compiled by the driver for its own language, and a core with any C++ is
*linked* by `clang++` — that is what pulls in `libc++` and `libc++abi`. Object files keep
their source extension (`cpu.cpp` → `cpu.cpp.o`) so a core carrying both `dsp.c` and
`dsp.cpp` cannot have one silently overwrite the other.

**Exceptions and RTTI are off and cannot be turned on.** wasi-sdk 25 ships `libc++` and
`libc++abi` built without the unwinder: `__cxa_throw`, `__cxa_allocate_exception`,
`__cxa_begin_catch`, `__cxa_end_catch`, `_Unwind_CallPersonality` and
`__wasm_lpad_context` are all absent from the sysroot. A translation unit containing a
`throw` or a `try` compiles cleanly and then fails to link, and `-fwasm-exceptions` fails
identically because it needs the same runtime. This is a property of the SDK rather than
of wasm — the Exception Handling proposal exists, nobody has built this libc++ against
it. Enabling them would mean rebuilding libc++ from source.

It has cost nothing so far. Emulator cores are written for consoles where exceptions are
equally unavailable, and snes9x's own libretro makefile already passes `-fno-rtti
-fno-exceptions`. The STL itself is fine: `<string>`, `<vector>` and friends link and
work, they just abort instead of throwing.

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
- **`CXX_STD="c++14"` for snes9x.** Its blargg APU code uses C++98 dynamic exception
  specifications (`throw()` on `operator new`), deprecated in C++17 and removed in
  C++20. Pin whatever standard the core's own makefile uses rather than inheriting the
  driver default.
- **snes9x refuses to load if the host rejects RGB565.** It calls `SET_PIXEL_FORMAT`
  inside `retro_load_game` and returns `false` on failure, so a host that only accepted
  XRGB8888 would see "content rejected" with no further explanation.

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

Measured: 324 stored titles → 105 card nodes and 7 shelf nodes, constant while scrolling. A
471-title shelf uses 15 nodes. A 129-state save list uses 10 rows.

---

## 4. Memory: `CoreRetention::Drop`

A core is not small. mGBA with a ROM loaded measures **35.4 MB** of linear memory;
Snes9x **22.2 MB**; Genesis Plus GX **14.8 MB**; fceumm **5 MB**. Leaking one per system switch exhausts a
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
NES → GBA → Master System → SNES three times and then asserts:

- 16 instantiated, 16 destroyed, 0 live
- **16 of 16 collected** after a forced GC (`--js-flags=--expose-gc`)
- `maxLive === 1` — never two cores alive at any instant
- engine memory 4.6 MB → 4.6 MB across 12 sessions. Wasm memory never shrinks, so the
  test is that growth *stops*; per-session staging buffers are released rather than
  stacked.

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

## 5b. Save states that survive

Persistence is pure JavaScript — no Rust change was needed, because the bridge already
exposed `saveState()`/`loadState()` and the core's own version string was already read
by `core-runtime.js` at instantiation.

### Where things live

`idb.js` owns the single IndexedDB connection and the whole schema. That matters: two
modules opening the same database at different versions is not a subtle bug, it is a
deadlock, and whichever module loads second stops working. `rom-store.js` and
`save-states.js` both import from it.

```text
rom-meta    { id, name, systemId, extension, size, addedAt }
rom-data    { id, bytes }
state-meta  { id, gameId, slot, auto, createdAt, frame, sizeKb,
              coreId, coreName, coreVersion, stateSize, contentId }
state-data  { id, bytes }
```

Metadata and payloads are separate stores throughout, and the *metadata* is hydrated
into memory once at boot while payloads stay on disk. That is what lets `listFor()` be
synchronous, which it has to be: the state list is a `VirtualScroller` and `bindNode`
runs on the scroll path, where nothing can be awaited. Metadata is a few dozen bytes per
state; payloads are 13 KB for the NES and a megabyte for the Mega Drive.

Sharing one database also makes `deleteRom()` able to remove a ROM and its states in a
single transaction, rather than orphaning megabytes nothing in the UI can reach.

### Why every state is tagged

A libretro save state is an opaque dump of a core's internal structs, meaningful only to
the same build of the same core. `retro_unserialize` is not versioned and does **not**
reliably reject a foreign blob — it can succeed into a corrupted machine that breaks
minutes later somewhere unrelated. So each record carries `coreId`, the core's
self-reported `coreVersion` and the exact `stateSize`, and `compatibility()` refuses on
any mismatch instead of hoping the core notices. `describeIncompatibility()` turns each
refusal into a sentence that says what happened rather than "unknown error".

Placeholder cores are tagged `version: 'placeholder'` so a state written against the
diagnostic core can never be mistaken for one from the real core that replaces it.

### Never having to press Save

One auto-save per game, keyed `<gameId>:auto`, overwritten in place — a separate key
namespace from numbered manual slots, so it can never consume a slot the user was using
and the resume path never has to guess which record is newest.

It is written on three triggers, and the reason there are three is that none of them is
reliable alone:

| Trigger | Why |
|---|---|
| `exit()` | Captures *before* `bridge.stop()`, which frees the core. The capture is synchronous for exactly this reason; only the write is deferred. |
| `visibilitychange → hidden` | The load-bearing one on iOS, where `pagehide` and `beforeunload` are unreliable. Backgrounding fires while the page is still alive, so the transaction has time to commit. |
| every 30 s while running | The honest answer to "never lose progress": shutdown hooks cannot be trusted, so the worst case is bounded regardless of how the app goes away. Piggy-backed on the HUD's slow tick, so it cannot fire while the loop is idle or paused. |

Launching restores the checkpoint before the loop starts, so the first frame presented is
the restored one — restoring a few frames later shows a flash of the game's boot screen,
which reads as a bug. The detail sheet's resume row is the only way to discard a
checkpoint, and it exists because otherwise there would be no way to start a game over.

Quota is handled by evicting the oldest *manual* states and retrying once. Auto-saves are
never evicted: they are the thing standing between the user and lost progress.
`requestPersistentStorage()` is asked for once at boot, because iOS clears
non-persistent storage for sites left unvisited — granted silently for installed PWAs,
usually refused for a plain tab.

---

## 5c. Deployment

`.github/workflows/deploy.yml` builds and publishes to GitHub Pages on every push to
`master`, plus `workflow_dispatch` so a deploy can be re-run from the Actions tab without
a commit — which matters when the only device to hand is a phone.

A checkout is **not** deployable on its own: the bridge is generated from the crate and
the four cores are build outputs this repository does not vendor. The workflow is the
only thing that produces a complete `web/`.

Three things it gets right that are easy to get wrong:

- **wasm-bindgen-cli is installed at the version read out of `Cargo.lock`**, not
  hard-coded. A mismatch there produces glue that silently will not load.
- **The GBA cart must not use the wasi-sdk clang.** That LLVM is built with the
  WebAssembly backend only and rejects ARM codegen flags outright
  (`Unknown command line argument '-arm-add-build-attributes'`) — this failed the first
  run. Ubuntu's clang has every target; only `ld.lld` and `llvm-objcopy` need locating,
  under the versioned names Ubuntu actually ships.
- **The service worker cache is keyed to the commit SHA.** Assets are served
  cache-first, so a shell cache outliving a deploy keeps serving the previous build's
  JavaScript. CI stamps `VERSION`, and `main.js` reloads once when the new worker takes
  over — guarded against firing on a first visit, during a running game, or twice.

**Enabling Pages was a one-time manual step** that no token in CI can do — Settings →
Pages → Source: *GitHub Actions*. It is done, and the site is live at
<https://2c2bhdhw9z-cell.github.io/Continuum-/>. If a future deploy job 404s with
"Ensure GitHub Pages has been enabled", that setting has been reverted; the build job is
unaffected, so the caches stay warm and a re-run is quick.

The browser suite takes a URL, so it runs against the deployment as-is and not only
against localhost — `node scripts/smoke-test.mjs https://2c2bhdhw9z-cell.github.io/Continuum-/`
was 73/73 on `v0.5.0-mobile`. Worth doing after any change to the service worker or to
core loading, because both behave differently on a real origin.

---

## 6. Building and verifying

```bash
# Rust engine → web/vendor/bridge/  (~428 KB)
./scripts/build-wasm.sh

# Cores → web/cores/*.wasm   (fetches wasi-sdk 25 into .tools/ on first run)
./scripts/build-core.sh fceumm | mgba | genesis_plus_gx | snes9x | all

# Test ROMs → web/roms/
python3 scripts/make-test-rom.py     # nes-testcart.nes  (24592 B)
./scripts/make-gba-rom.sh            # gba-testcart.gba  (1144 B)
python3 scripts/make-sms-rom.py      # sms-testcart.sms  (32768 B)
python3 scripts/make-snes-rom.py     # snes-testcart.sfc (32768 B)

# Verification, fastest first
cargo test                           # 85 unit tests
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings          # what CI runs; zero warnings
node scripts/core-abi-test.mjs       # 64 checks, 4 cores, no browser
node scripts/capture-frames.mjs      # docs/frame-{nes,gba,sms,snes}[-a].png

# Phase 5 native (§15). Neither is part of the web deploy; both run in CI.
./native/switch-wrapper/build.sh host               # 15 checks
cargo check --target aarch64-apple-ios --features native-core,uniffi-bindings

# Browser suite (108 checks). One shell invocation: /tmp and background jobs
# do not survive between tool calls.
node scripts/serve.mjs 8123 &
PLAYWRIGHT_CORE=/tmp/pw/node_modules/playwright-core \
  node scripts/smoke-test.mjs http://localhost:8123/
```

Pass `-- -D warnings` to clippy, because that is what the workflow does. The toolchain is
pinned in `rust-toolchain.toml` specifically so that these commands produce the same output
here as on the runner; the reasoning is in that file. Two traps if a clippy result looks
wrong: it caches, so a second run prints `Finished` without re-emitting the warnings it
found the first time (`touch` a source file to force it), and without `-D warnings` a
lint that fails CI is only a warning locally.

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
| `snes-testcart.sfc` | white | red | scroll on Right, 440 Hz DSP tone via SPC700 |

That is what makes the hot-swap checks meaningful rather than a matter of trusting
bookkeeping. Each cart is hand-assembled by a small assembler inside its generator
script — 6502, ARM, Z80 and 65816 respectively. Two traps already paid for:

- **SMS:** `PROGRAM_ORIGIN = 0x0070`, because the first version laid code across
  `0x0000` and then wrote the NMI handler at `0x0066` on top of its own tile-upload
  loop. The PSG still played, so the cart looked alive while rendering a black screen.
- **SNES:** `$2122` (CGDATA) is *one* register written twice, unlike `$2116`/`$2117` and
  `$2118`/`$2119` which are genuine low/high pairs. A 16-bit store puts the high byte in
  `$2123`, the window-mask register — half a colour written and the window settings
  corrupted. Separately, VRAM uploads must be 16-bit stores: with `VMAIN = 0x80` the
  address increments after the *high* byte write, so a run of 8-bit stores to `$2118`
  rewrites word zero forever.

### The SNES cart needs two programs

Worth knowing because it is unlike the others: the SNES gives the main CPU no way to
reach the DSP's registers — they live behind the SPC700's own address space. So the
cartridge uploads a second program through the APU boot ROM and jumps to it.

`spc_payload()` builds a 269-byte ARAM image: seventeen DSP register writes (each one
`mov $F2,#reg` then `mov $F3,#val`), a sample directory at a page boundary because the
`DIR` register is a page *number*, and one looping BRR block holding a 16-sample square
wave. `emit_apu_upload()` drives the handshake, which is a lockstep mailbox: every byte
written to `$2141` is acknowledged by the boot ROM echoing a counter back through
`$2140`, and each stage spins until it sees that echo.

A single wrong constant therefore *hangs* rather than misbehaving, which is why each
stage records how far it got in direct-page `$10`. A test reads that byte out of work RAM
via `retro_get_memory_data(RETRO_MEMORY_SYSTEM_RAM)`; `5` means the whole upload
completed. If SNES audio ever goes silent, read that byte first.

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
    diagnostic.rs pattern-generating stand-in for systems with no real core yet
  gfx/            renderer.rs (wgpu), convert.rs, frame_blit.wgsl
  audio/          mod.rs (AudioSink), ring.rs, resample.rs
  input/          mod.rs (Button, InputSnapshot), gamepad.rs (GamepadBridge)

core-shim/libretro_wasm_shim.c   callback trampolines, compiled into every core

web/
  index.html  sw.js  manifest.webmanifest
  cores/      manifest.json + the built .wasm cores
  roms/       the four test carts (+ src/ for the GBA cart)
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

1. **Promote Gambatte to a real core.** It is C++, and the C++ path exists now, so this
   is mostly a matter of its flags — and it would turn the `gb`/`gbc` subcore choice from
   a demonstration into a genuine one. `smsplus` is the same story for `sms`.
2. **A Mega Drive test cart.** Genesis Plus GX is verified in Master System mode only.
   A 68000 cart would cover the other half of the core, and the extension-driven system
   switch in both directions.
3. **Native ARM64.** Audited — see §9 below. The engine type-checks for
   `aarch64-apple-ios` today; what remains is a `LibretroRuntimeHandle` over
   `dlopen`ed or statically linked cores, a CoreAudio `AudioSink`, a `CAMetalLayer`
   surface, and a UniFFI facade beside `wasm.rs`. The tokens in `styles/tokens.css`
   are meant to become a Swift `Theme` struct.
4. **Core options.** `HAVE_NO_LANGEXTRA` is set and `GET_VARIABLE` returns nothing, so
   every core runs on defaults. Exposing options means a UI, persistence, and deciding
   which of the dozens per core are worth showing.
5. **A save-state UI beyond the per-game list.** No way to browse states across games or
   export one. Settings now reports what they cost in storage (§10), so the numbers are
   there; the browsing is not.
6. **Offline cover art.** Tiers 1–3 store a URL because the thumbnail server sends no CORS
   header (§11), so scraped covers depend on the HTTP cache. A tiny same-origin proxy, or
   accepting a capture as the durable copy, would close that gap.
7. **Bulk import.** Files go in one at a time through the picker. A directory picker
   (`showDirectoryPicker`) plus a progress sheet would make a real collection practical;
   the store and the artwork queue already handle batches.

### Things not to undo

**Do not reintroduce generated library data.** The suite asserts a fresh install holds
exactly four entries, that no entry carries a rating/genre/player-count/progress field, and
that an unplayed game has no save states. Those checks exist because all three used to be
false. If the virtualisation needs a large library to test against, seed real ROM records
into IndexedDB from the test — `smoke-test.mjs` §1b does exactly that, and its fixture
counter has to land inside the first 8 KB or every ROM hashes identically.

- `_instance` / `_adapter` in `Renderer` (§2) — deleting them blanks the canvas.
- Per-source input layers in `GamepadBridge` (§3) — merging them breaks the keyboard
  whenever a pad is connected.
- Freeing the old core *before* fetching the new one (§4) — reordering doubles peak
  memory.
- `full_path` in `retro_game_info_ext` (§1) — emptying it makes Genesis Plus GX
  misidentify the system.
- Starting the audio graph before the first `await` in the launch path (§2) — moving it
  later leaves iOS silent.
- The placeholder-core predicate in `smoke-test.mjs`'s offscreen GPU section (§6) — it
  asks the registry which core would run an entry rather than naming systems to skip. An
  earlier version excluded `nes` by hand and broke the day SNES got a real core.
- The synchronous `listFor()` in `save-states.js` (§5b) — making it async would break
  the virtualised state list, whose `bindNode` cannot await.
- Capturing the state *before* `bridge.stop()` in `exit()` (§5b) — after `stop()` the
  core is gone and there is nothing left to serialise.
- The single IndexedDB opener in `idb.js` (§5b) — a second module opening the same
  database at its own version deadlocks both.


---

## 9. Native ARM64 audit

Run at `v0.5.0-mobile`. The claim being tested: that the engine is genuinely
interface-agnostic, and that the case for keeping so much logic in Rust was not
self-deception.

```bash
rustup target add aarch64-apple-darwin aarch64-apple-ios
cargo check   --target aarch64-apple-darwin        # clean
cargo check   --target aarch64-apple-ios           # clean — the .ipa target
cargo clippy  --target aarch64-apple-darwin --all-targets   # clean, incl. the 70 tests
```

All clean, first run, no warnings. wgpu's Metal backend compiles for both
(`objc2-metal`, `objc2-quartz-core`, `raw-window-metal`, `wgpu-core-deps-apple`).

### No web types have leaked

`bridge.rs` mentions `wasm_bindgen` exactly once — in a doc comment claiming it does
not use it. That claim is now compiler-checked rather than aspirational: `bridge.rs`
is compiled for both Apple targets, so any web type in it would be a hard error.

Web-only code is confined to three files and five `cfg` gates:

| File | Lines | What a native build needs instead |
|---|---:|---|
| `wasm.rs` | 769 | A UniFFI facade with the same methods |
| `cores/wasm_core.rs` | 380 | `EmulatorCore` over `dlopen`ed or static cores |
| `cores/host.rs` | 263 | The same five callbacks, called from C rather than JS |
| `gfx/renderer.rs` (one fn) | ~24 | `from_surface` with a `CAMetalLayer` — already shared |

**4,896 of 6,308 lines — 77% — compile unchanged for iOS.** The gated 23% is exactly
the platform boundary and nothing more: a facade, a core loader, a callback bridge and
one surface constructor.

### Two properties that make it portable, worth not breaking

- **The engine never reads a clock.** `FramePacer::plan()` takes a timestamp as an
  argument. There is no `Instant::now()` or `performance.now()` anywhere in the shared
  code, which is why `timing.rs` needs no platform shim at all.
- **The 70 unit tests are not just tests.** They compile and run in the
  `not(target_arch = "wasm32")` configuration — the *same* configuration iOS uses. So
  every `cargo test` on a dev machine is already a regression test for the native
  build's pacing, ring buffer, resampler, registry, gamepad mapping and pixel
  conversion. `cargo clippy --target aarch64-apple-ios --all-targets` confirms they
  type-check for the device too.

### One concrete thing the `.ipa` will need

`Cargo.toml` declares `crate-type = ["cdylib", "rlib"]`. Linking into a Swift app
wants **`staticlib`**, and the comment above that line already says "iOS staticlib"
while the list does not contain it.

It is deliberately not changed here. `cargo check` does not link, so adding it would
be an unverifiable edit — and this sandbox can no longer build for `wasm32` (the target
and `wasm-bindgen` are both absent), so the change could break the web build with no
way to notice before it reached CI and the deploy the phone testing depends on. Add it
when someone is in a position to link an actual binary.

### What the audit does *not* prove

That it links, or runs. No Apple linker or SDK is available here, so this establishes
that the code is *type-correct* for the target and that the module boundaries are in
the right places — not that a binary works. The first real test is a `staticlib` linked
into a SwiftUI shell calling `EmulatorBridge` directly.


---

## 10. The library is real data now

Until `v0.5.0` the front end generated a 4,800-entry catalogue at boot. It existed for a
good reason — the virtualisation had to be provable before there was anything to browse —
and it had earned its retirement: every launch path carried a "does this entry actually
have bytes" branch, `save-states.js` fabricated histories so the state list had something
long to render, and the shelves were mostly rows of things nobody could play.

All of it is gone. `web/src/data/catalog.js` is now an index over real content only.

### What an entry is

```text
  id          content hash (FNV-1a over the first 8 KB + length), or builtin-*
  title       from the filename, with [dump tags] removed and (Region) kept
  systemId    from rom-detect.js, which reads headers rather than trusting extensions
  filename    the original name, unmodified — cover art lookup depends on it
  sizeBytes   real
  region      parsed from the filename's tags, or null. Never guessed.
  source      'imported' | 'builtin'
  favorite / lastPlayed / playCount      the user's, persisted in `entry-flags`
  art         {kind: 'url'|'blob', url, tier} or null
```

**There is no rating, genre, year, player count or progress percentage.** A front end
cannot know any of them about a file it was handed, and the browser suite asserts their
absence (`entries carry no invented metadata`) so they cannot creep back.

`progress` is worth a specific note: cards used to show a percentage bar, which is
unknowable — no emulator can tell how far through a game a save state is. It was replaced
with a "RESUME" pip driven by `autoStateFor()`, which is both knowable and useful.

### Shelves are dynamic, and that is a rule

`buildShelves()` emits a shelf only when it has content. A fresh install produces exactly
one row, *Continuum Test Carts*. A system with no imports produces nothing — not an empty
row, not a "0 titles" heading. `featuredIndex()` returns `-1` for an empty library and the
hero is removed from the layout rather than featuring nothing.

`featuredIndex` had a real bug when written: the four bundled carts register in the same
tick, so their `addedAt` differed by zero or one millisecond depending on where the clock
ticked, and the hero changed between reloads while the docstring claimed determinism. Now
built-ins register with `addedAt: 0` and ties break on `sortKey`.

### Import once

`rom-import.js` writes bytes to `rom-data` and metadata to `rom-meta`;
`restoreLibrary()` reads them back at every boot along with `entry-flags` and `rom-art`.
Three `getAll()` calls, no payloads — restoring a hundred games costs kilobytes, because a
ROM's bytes are only fetched when it is launched.

Database is at **v3**; v3 added `rom-art` and `entry-flags`. Upgrades are additive and
guarded, so an existing collection survives.

`entry-flags` is deliberately separate from `rom-meta`. Favourites apply to built-in carts
too, and those have no `rom-meta` row because their bytes ship with the app — writing a
fake ROM record to hold a boolean would make "what is in my library" and "what is in my
storage" two different questions with one answer.

## 11. Cover art, and the CORS wall

Five tiers, in `web/src/data/artwork.js`:

| Tier | Source | Stored as |
| --- | --- | --- |
| 1 | libretro `Named_Boxarts` | URL |
| 2 | `Named_Titles`, then `Named_Snaps` | URL |
| 3 | the same three with tags dropped | URL |
| 4 | captured from the game past frame 60 | blob |
| 5 | an image the user picks | blob |
| — | generated console plate (`ui/art.js`) | nothing |

**`thumbnails.libretro.com` sends no `Access-Control-Allow-Origin` header.** Verified, not
assumed: a `GET` with an `Origin` returns 200 with no CORS header, and `OPTIONS` answers
without one either. So script cannot read those bytes — `fetch` in `cors` mode is refused,
`no-cors` yields an opaque response whose `status` is always 0, and drawing the image to a
canvas taints it. What *does* work is an `<img>`, so `boxart.js` probes with `Image`
load/error events and persists the resolved **URL**. A 404 there returns `text/html`, so
`onerror` fires reliably.

The consequence to remember: **scraped art is not guaranteed offline.** It lives in the
HTTP cache. Tiers 4 and 5 are real blobs and work with no network at all.

### Naming

Libretro requires `& * / : ` < > ? \ | "` to be replaced with `_` — a substitution, not a
strip, so "Ratchet & Clank" becomes "Ratchet _ Clank" with both spaces intact.

The fallback ladder is **graduated**, and the ordering was measured rather than guessed:

```text
  Super Mario World (USA) [!]   404
  Super Mario World (USA)       200   ← stripDumpTags: square brackets only
  Super Mario World             404   ← stripTags: everything
```

Stripping everything on the first retry throws away the region and misses the only name
that exists. So `stripDumpTags` (tier `-relaxed`) comes before `stripTags` (tier
`-untagged`). Nine candidates worst case, deduplicated, then remembered as a miss for a
week in `localStorage`.

Built-in carts are **never** probed: they are original ROMs written for this project, so
no thumbnail server has heard of them, and asking would be six guaranteed 404s per cart on
every fresh install.

### The PNG encoder

`web/src/data/png.js` encodes captured frames with **no canvas of any kind** —
`CompressionStream('deflate')` supplies the zlib-wrapped DEFLATE that a PNG `IDAT`
requires, with stored (uncompressed) blocks as a fallback. This is not pedantry: rule 2
says a 2D context is never created, the suite's hook only watches `HTMLCanvasElement`, and
an `OffscreenCanvas` would have slipped past it. An invariant that holds only where it is
measured is not an invariant.

`captureFrame(w, h)` renders into an offscreen texture at *any* requested size, so
thumbnails are asked for at 512×384 and nothing is resampled in JavaScript.

**Headless GPUs cannot map a buffer back**, so tier 4 end-to-end is reported as `INFO` in
the suite, not asserted. The encoder itself is asserted: signature, chunk order, both CRCs
and a real decode through `createImageBitmap`.

### Object URLs

A blob needs an object URL, and an object URL is a leak until revoked. Cards recycle
constantly, so `artwork.js` mints **at most one URL per entry**, created when its blob is
attached and revoked when replaced or deleted. Binding a card is then a property read.
Never call `createObjectURL` in `bindCard`.

## 12. Virtual scroller: `capPoolToCount`

New option, and it has a rule attached. It caps the pool at the item count as well as at
the viewport, which is what stops a four-cart library from building seven shelf nodes
holding 98 cards.

**It may only be used where `count` is a property of the data, not of scroll position.**
The shelf list, the grid and the save-state list qualify. A shelf's *own* horizontal
scroller does not: one pooled shelf node is rebound from a 4-item shelf to a 54-item one as
the user scrolls, so capping there grows the pool mid-scroll and breaks rule 4. Uncapped
scrollers are also sized *eagerly*, before any count is known, for the same reason.

This was caught by the suite — `card node count constant while scrolling` went 60 → 105 —
which is exactly what that check is for.


## 13. The state-bleed bug, and why it was in the scroller

Reported as: play an SNES game, then open the sheet for a GBA game you have never
launched, and the SNES auto-save is listed under it.

Nothing was wrong with the data. `listFor` is keyed by game id and always was. The fault
was in `virtual-scroller.js`, in the pass that parks nodes leaving the window:

```js
const index = this.slotIndex[slot];
if (index === -1) continue;        // ← the bug
```

`setCount` and `refresh` both invalidate every binding with `slotIndex.fill(-1)`. This
loop read that `-1` and skipped the slot, on the assumption that a slot with no binding
must already be parked — true of a node fresh out of `_growPool`, false of one whose
binding had just been thrown away. Opening a sheet for a game with no history calls
`setCount(0)`, the bind loop has nothing to bind, and the previous game's row was simply
left where it was, content and position intact.

The test is now "is this slot inside the window", and `slotOffset` carries a
`PARKED_OFFSET` sentinel so an already-parked node is not re-written every frame. (It was
`NaN` before, which never compares equal to itself, so idle nodes were being re-parked on
every flush.)

**The same fault had a second symptom**: a search matching fewer results than the card
pool left the previous results on screen underneath. One fix, both gone, and there is a
check for each.

### The sheet also blanks itself now

`DetailSheet.close()` calls `_blank()`, which zeroes every field the panel renders —
title, metadata, badge, glyph, artwork and its provenance line, the core picker's options,
the resume block, the states count — and empties the list through the scroller with a
*synchronous* flush. `open()` flushes synchronously too, before returning, so no frame is
ever painted with the pool's previous contents.

All three exits — close button, backdrop scrim, Escape — share one handler, so this is
the only place the reset lives.

The state rows also get an `unbindNode`, which wipes a row's slot/when/detail text as it
leaves the window. That is not what stops it being *seen* — parking does — it is so the
DOM holds no copy of a game's save history once its sheet is closed.

## 14. A test that was racing the frame loop

The byte-exactness check in §5f compared a re-serialised state against `savedPayload`,
which was built like this:

```js
const manual = await player.saveState();
const savedPayload = Array.from(host.bridge.saveState());   // ← wrong
```

That second `saveState()` runs *after* the await on the IndexedDB transaction, by which
point emulation has moved on several frames. So the comparison was against a snapshot a
few frames newer than the record on disk — and one that had never been through IndexedDB,
which is the only thing the check claims to prove. It passed most of the time because the
NES test cart's state barely changes frame to frame, and failed with a few dozen differing
bytes whenever a counter happened to tick in between.

It now reads the stored bytes back with `payloadFor(GAME, manual.slot)`. Deterministic,
and actually testing the round trip.

If a check in this suite fails intermittently, look for this shape before re-running it.


---

## 15. Step 10: the stub engine is built

`docs/SET_HW_RENDER_DESIGN.md` describes the hardware-render path; this section is what
happened when it was actually built. A dummy frame — a colour that rotates once every 120
frames — now travels from a C++ libretro core, through the Rust bridge, across a UniFFI
boundary, to a Swift Metal view. Everything in this section compiles; the C++ half also
*runs*, here, with 15 passing checks.

### The layout

```
native/switch-wrapper/
  switch_engine.h              ISwitchEngine + the injected-Vulkan-context types
  frame_gate.h                 the retro_run ↔ engine-thread handshake
  stub_engine.h/.cpp           StubEngine, HostStubRenderer, StubExpectedColour
  vulkan_stub_renderer.h/.cpp  the real vkCmdClearColorImage path
  continuum_switch_libretro.cpp  the libretro surface
  test_harness.cpp             dlopens the .so and drives it like a frontend
  build.sh                     host | vulkan | ios
native/ios/
  MetalCanvas.swift            CAMetalLayer + CADisplayLink
  ContinuumApp.swift           SwiftUI harness, EngineHost, lifecycle
  Continuum.entitlements       JIT + the 12 GB memory keys
  build-engine.sh              macOS-only
crates/emulator-bridge/src/
  gfx/hw.rs                    the hardware-frame seam
  cores/native_core.rs         dlopen-based libretro host
  uniffi_api.rs                the Swift-facing facade
```

### Two renderers, one interface — and why that is not gold-plating

`StubEngine` renders through an `IStubRenderer`, and there are two implementations:
`HostStubRenderer`, which computes the colour and reports it as plain pixels, and
`VulkanStubRenderer`, which does a real `vkCmdClearColorImage` into double-buffered
`VkImage` targets and is compiled only under `CONTINUUM_HAVE_VULKAN`.

This was not the obvious design — Vulkan-only is fewer moving parts. But the things most
likely to be *wrong* in this wrapper are the frame gate, the threading, and the libretro
contract, and none of those are Vulkan-specific. Splitting the renderer out means all three
are exercisable on a build host with no GPU and no Vulkan loader, which is exactly what this
sandbox is. That is where the 15 checks come from. A Vulkan-only wrapper would have been
committed entirely untested.

`build.sh vulkan` still compiles the Vulkan path against real headers, so the injected-context
code is at least type-checked against the true ABI.

### The frame gate bug worth knowing about

`retro_run` runs on the frontend's thread; the engine runs its own loop. The gate between
them uses **two monotonic counters**, not a semaphore:

```cpp
uint64_t requested_;   // frames the frontend has asked for
uint64_t completed_;   // frames the engine has finished
```

`PumpFrame` sets `target = completed_ + 1` and waits for `completed_ >= target`.

The natural way to write this is `++requested_` and wait for the engine to catch up. That is
wrong, and the failure is subtle enough to survive casual testing: when a frame misses its
deadline, `PumpFrame` returns a duped frame but the request it banked is still outstanding.
The engine works through the backlog, and afterwards every `retro_run` finds an
already-completed frame waiting and immediately requests another — the game runs at double
speed while the frontend displays half its frames. Games would feel fast and look fine in a
screenshot.

`completed_ + 1` is idempotent: a timed-out request leaves nothing behind. The harness check
named *"no frames are banked after a timeout"* stalls the engine deliberately, then verifies
the engine advanced 11 frames across 10 subsequent runs while the frontend served 10. Do not
"simplify" this into a semaphore.

### The headers are fetched, and they corrected the design doc

`libretro.h`, `libretro_vulkan.h`, and the Vulkan headers now land in `.work/hdr/`
(gitignored) rather than being hand-declared. This immediately caught two mistakes in my own
design document: `retro_hw_render_interface_vulkan` has `void *handle` as its **third** field
(and every function pointer on the interface takes it as the first argument), and the order
is `queue` then `queue_index`, with `get_device_proc_addr` before `get_instance_proc_addr`.
§12.5 of the design doc now records this.

Useful constants, since they are easy to get wrong: `RETRO_HW_FRAME_BUFFER_VALID` is
`((void*)-1)`, the Vulkan render interface version is `5`, the negotiation interface version
is `2`, and the environment callbacks are `41` (GET_HW_RENDER_INTERFACE), `43`
(SET_HW_RENDER_CONTEXT_NEGOTIATION_INTERFACE) and `56` (GET_PREFERRED_HW_RENDER).

### `MaybeSend`, because `JsValue` is not `Send`

`NativeLibretroCore` is driven from a Swift thread and wants `Send`. `WasmCore` holds a
`JsValue`, which is deliberately not `Send` and never will be. So `EmulatorCore` and
`AudioSink` now sit on a conditional supertrait in `lib.rs`:

```rust
#[cfg(target_arch = "wasm32")]        pub trait MaybeSend {}
#[cfg(not(target_arch = "wasm32"))]   pub trait MaybeSend: Send {}
```

The alternatives were adding plain `Send` (breaks the web build outright) or duplicating both
trait definitions per target (two definitions to keep in sync forever).

### `DIRECTORIES` is global, `EXCHANGE` is thread-local

Deliberate asymmetry in `native_core.rs`. `EXCHANGE` — the input/video/audio hand-off — is
populated and consumed inside a single `run_frame` call, so thread-locality is exactly right
and costs no synchronisation. `DIRECTORIES` is written when the core is loaded and read later
during `retro_load_game`, potentially on a different thread, so it is a process-global
`Mutex`. Making EXCHANGE global would add a lock to the hot path; making DIRECTORIES
thread-local would silently lose the save/system paths.

### Feature-gated so the web build cannot notice

`libloading` and `uniffi` are optional and native-only; `native-core` and `uniffi-bindings`
are off by default. The wasm dependency graph is unchanged and the wasm binary is still
440 KB. `uniffi::setup_scaffolding!()` has to live in `lib.rs`, not in `uniffi_api.rs` — it
defines `UniFfiTag`, which the macros resolve from the crate root.

One thing left undone on purpose: `crate-type` does **not** include `staticlib`. `cargo check`
does not link, so adding it here would be an unverifiable change to the build; instead
`native/ios/build-engine.sh` fails with the exact line to add when someone runs it on a Mac.

### What compiling found that review had not

Seven API mismatches, all mine, all caught by the compiler rather than by reading:
`InputSnapshot` has no `Default` (so `Exchange` holds `Option<InputSnapshot>`) and no
`is_pressed` (use `libretro_state(port, device, index, id) -> i16`); `target_fps` is `f64`;
`FrameView::stride_bytes` is `usize`; the variant is `PixelFormat::Rgba8888`;
`BridgeError::CoreNotLoaded` is a *struct* variant and `CoreBusy` carries a core id;
`pause()` returns `()` while `resume` takes `now_ms`; `AudioStats::queued_frames` is already
`u32` but `underruns` is `u64`.

### Verifying it

```bash
./native/switch-wrapper/build.sh host      # 15/15 checks — the real test
./native/switch-wrapper/build.sh vulkan    # compiles against real Vulkan headers
cargo test                                 # 85 (was 74 before rewind and volume)
cargo clippy --features native-core,uniffi-bindings --all-targets
cargo check --target aarch64-apple-ios --features native-core,uniffi-bindings
swiftc -frontend -parse native/ios/*.swift # syntax only; no UIKit/Metal on Linux
```

Swift 6.3 is present on this box, but without the iOS SDK, so the Swift is parse-checked
only — it has never been type-checked against UIKit or Metal. Treat it as a first draft.

### The one real gap

~~`attach_metal` returns `EngineError::Graphics`.~~ Closed by step 1 — see §16.


---

## 16. Step 1, and the .ipa is built in the cloud

Two things landed together, and the second explains the first: **this project is developed
entirely from a phone.** There is no Mac, no Xcode, no simulator. "The Swift compiles later"
was never going to happen, so CI is the compiler, and the iOS workflow produces a
downloadable `.ipa` on every push and on demand.

### Who creates the `MTLDevice` — the design doc had it backwards

§2 of `docs/SET_HW_RENDER_DESIGN.md` and the old header of `MetalCanvas.swift` both said
Swift creates the `MTLDevice` and injects it into the engine. Two facts, read out of
`wgpu-hal-30.0.1`, make that impossible:

1. **No public constructor takes an existing device.** The only path from an `MTLDevice` to a
   `metal::Adapter` is `AdapterShared::expose`, which is private (`src/metal/mod.rs:428`), so
   `Instance::create_adapter_from_hal` is unreachable. The queue half *is* public
   (`Queue::queue_from_raw`); the device half is not.
2. **`Surface::configure` overwrites the layer's device.** It calls
   `CAMetalLayer::setDevice` with wgpu's own device (`src/metal/surface.rs:276`).

Fact 2 is the one that matters. Injection was not merely unsupported — it was *undone one
call later*. A build doing it would have looked fine until the first attempt to share a
texture between the core and the compositor, which would have failed with no obvious cause.

So the arrow is reversed: **wgpu creates the device; Swift reads it back** with
`metalDeviceHandle()` / `metalQueueHandle()`. The invariant the design actually cares about —
one `MTLDevice` shared by wgpu, Swift and later MoltenVK, so every texture is shareable by
construction — is unchanged. Only ownership moves.

On iOS this costs nothing in device selection: `wgpu-hal` enumerates via
`objc2_metal::MTLCopyAllDevices`, which on iOS is a shim returning whatever
`MTLCreateSystemDefaultDevice()` would have (`objc2-metal-0.3.2/src/device.rs`). One GPU, and
that is it.

The consequence for Swift is that `configureLayer()` is nearly empty now. wgpu's `configure`
sets `device`, `pixelFormat`, `framebufferOnly`, `colorspace`, `maximumDrawableCount`,
`opaque` and `drawableSize`. Setting any of them in Swift is theatre. One intent was lost in
the move and is flagged in the file rather than quietly dropped: Swift used to pin
`maximumDrawableCount = 2` to save a frame of latency, and wgpu derives it as
`desired_maximum_frame_latency + 1` = 3. Changing that means changing the shared renderer for
the browser too, so it is a measurement to make on a device.

### Two bugs that only surfaced once the path could run

- **`launch()` requires a renderer**, and the harness kicked off `launchStub()` from `.task`
  while the attach happens in `layoutSubviews` — a race it loses whenever layout is slow. The
  session now starts from the attach callback, so the ordering is structural.
- **Nothing declared the stub core.** `load_native_core` refuses undeclared ids rather than
  inventing geometry, so it could never have loaded. `declare_core` and `core_state` are now
  on the UniFFI surface; `wasm.rs` has had both all along.

### The iOS pipeline

```
.github/workflows/ios.yml          macos-latest; workflow_dispatch + push
native/ios/project.yml             XcodeGen spec — the .xcodeproj is generated, not committed
native/ios/Info.plist              bundle keys; Metal is a required capability
native/ios/build-engine.sh         Rust → .a + .dylib, UniFFI bindings, C++ wrapper
native/ios/package-ipa.sh          xcodegen → xcodebuild → Payload/ → codesign → .ipa
scripts/fetch-libretro-headers.sh  libretro.h for hosts that never build a core
```

Things in there that are load-bearing and easy to undo by accident:

- **Bindings come from the `.dylib`, never the `.a`.** UniFFI's `calc_cdylib_name` only
  recognises `.so`/`.dll`/`.dylib`, so pointing it at the static archive finds no metadata at
  all. The staticlib is what Xcode links; the cdylib exists to be read.
- **The generator is its own crate** (`crates/uniffi-bindgen`). A `[[bin]]` inside
  `emulator-bridge` would compile the whole engine for the host to produce a tool that parses
  a file, and `uniffi`'s `cli` feature would drag clap into the iOS staticlib.
- **`#[uniffi::export]` ignores `#[cfg]` on individual methods** and generates scaffolding for
  them regardless — a `cfg`-gated method fails to compile off-Apple with "method not found".
  The platform split lives inside function bodies, which has the side benefit that the
  generated Swift is identical whichever target's library was read.
- **The wrapper dylib is embedded but not linked** (`link: false`). It is reached by `dlopen`;
  linking it too would mean a wrong rpath stops the app launching at all instead of showing
  "libcontinuum_switch.dylib is missing" in the HUD.
- **The wrapper's iOS build does not require MoltenVK.** The rotating colour arrives through
  the software path, which is all step 1 completes.
- **Entitlements are applied by `codesign`, not by Xcode.** Signing is disabled for the build;
  `package-ipa.sh` ad-hoc signs with `--entitlements`, and TrollStore preserves that blob.
  Without it the JIT and increased-memory keys are absent. The workflow prints the
  entitlements it actually embedded, because that is the one property of the file a listing
  cannot show.

### macOS ships bash 3.2

`LINK_LIBS=()` expanded as `"${LINK_LIBS[@]}"` under `set -u` is an *error* on bash 3.2 —
"unbound variable" — not an empty expansion. It was empty on exactly one platform: the one it
had been added for. Fixed by removing the array rather than reaching for
`${arr[@]+"${arr[@]}"}`, which is obscure enough that someone would later simplify it and
break macOS again. If you add an array to any script here, make sure it cannot be empty.

### What Linux can and cannot verify

More than expected. The UniFFI bindings can be generated *here*, from a host cdylib, because
the metadata is an interface description and target-independent — and the generated Swift
`swiftc -typecheck`s against Foundation on Linux. Every Swift call site was checked against
those generated signatures: names, argument labels, argument *order*, `Data` vs `Vec<u8>`,
`Float` vs `Double`.

What that still cannot catch is a type error in code that needs UIKit. It missed exactly one,
and the failure is instructive:

```swift
Text("... \(fps, specifier: "%.0f") fps" + " · \(dropped) dropped")
// error: '+' on 'RangeReplaceableCollection' requires 'LocalizedStringKey' to conform
```

`swiftc -parse` accepts that happily. It is a *type* error, so nothing short of the iOS SDK
would ever have found it — which is the whole argument for the cloud build. Treat
`swiftc -frontend -parse` as a spell-checker, not a compiler.

### Verifying it

```bash
# Everything in §6, plus:
cargo clippy --target aarch64-apple-ios --features native-core,uniffi-bindings -- -D warnings

# Generate and type-check the Swift facade without a Mac:
cargo build --release --features native-core,uniffi-bindings
cargo build --release -p continuum-uniffi-bindgen
./target/release/uniffi-bindgen generate \
  --library target/release/libemulator_bridge.so --language swift \
  --out-dir native/ios/build/Generated --no-format
cp native/ios/build/Generated/*FFI.modulemap native/ios/build/Generated/module.modulemap
swiftc -typecheck -I native/ios/build/Generated \
  -Xcc -fmodule-map-file=native/ios/build/Generated/module.modulemap \
  native/ios/build/Generated/emulator_bridge.swift
```

The `.ipa` itself: run the **iOS** workflow and download the `Continuum-ipa-<sha>` artifact.
It is ~1.8 MB — a 4.4 MB arm64 executable with the engine statically linked, plus the 95 KB
wrapper in `Frameworks/`.

### Still not proven, as of this step (superseded by §18)

**Superseded 2026-09-20.** The app has since been run on an iPhone 17 Pro Max and all five cores
drove real games; see §18. The paragraph below records the state of knowledge when the cloud
build landed, and its argument for the HUD is what made the device run legible.

At that point the app had never been run. Everything up to and including "xcodebuild produced a
signed bundle with the right entitlements" was verified by CI; whether the rotating colour
actually appears was not, and could not be, from here. The HUD exists for exactly that reason:
each line distinguishes a different failure, because on a sideloaded build with no debugger it is
the only diagnostic there is.

A Rust panic on device now surfaces rather than aborting silently. It used to be that
`panic = "abort"` in the release profile took the whole process down on a panic instead of
letting it cross the FFI boundary, which threw away the `rustPanic` case UniFFI generates and
the HUD relies on. The fix is now in place: `Cargo.toml` carries a `[profile.ios]` that
`inherits = "release"` and sets `panic = "unwind"`, and `native/ios/build-engine.sh` builds
the engine with `cargo build --profile ios` and reads its artefacts from
`target/aarch64-apple-ios/ios/` rather than `.../release/`.

It is a *separate* profile, not a change to `[profile.release]`, on purpose: the wasm/web
build (`scripts/build-wasm.sh`) and `cargo test` both use release and must keep
`panic = "abort"` — `wasm32-unknown-unknown` has no real unwinder, so flipping release there
would regress a build this change must not touch. Only `build-engine.sh` uses `--profile ios`,
so only the iOS engine unwinds. The artefact-path cost the previous note warned about is paid
inside `build-engine.sh` alone: `package-ipa.sh` reads from `native/ios/build/` (which
`build-engine.sh` populates) and `native/switch-wrapper/build.sh` has its own `build/` dir, so
neither needed touching. The host-side UniFFI bindgen and its metadata fallback stay on
release, because the interface metadata UniFFI reads is profile- and target-independent.

Still unexercised: whether the panic-to-HUD path actually fires on device. The app has now been
run on device (§18), but nothing panicked during that run, so the unwind-across-FFI-to-HUD path
has not been observed firing. The build no longer aborts on the way there.

## 17. Phase 5 Step 2: the first real core (PCSX ReARMed, software)

Step 10 booted a stub. Step 2 boots a real libretro core, PCSX ReARMed, through the same
software frame path. Nothing about the frame loop, the compositor or the audio ring changed;
what changed is that the thing on the other side of `dlopen` is now a real emulator with real
expectations, and the loader had to grow up to meet them.

### (a) Why PS1, and why PCSX ReARMed first

The point of the first real core is to de-risk the parts that the stub could not exercise:
the loader against a core that actually rejects a bad load, ROM ingestion against content the
core insists on opening itself, input mapping against a real controller layout, and the audio
pipeline against a core that produces real sample-rate audio. All four ride the software
frame path, which already works. Doing them before MoltenVK means the hardware path lands on
top of a loader that is already proven rather than being debugged at the same time as Vulkan.
PS1 is the right system for that: PCSX ReARMed is small, boots without a BIOS via HLE, and its
software renderer needs no hardware context at all. paraLLEl-N64 follows immediately after, and
it is what forces the full MoltenVK/Vulkan hardware path, so PS1 is deliberately the last core
that can get away with software only.

### (b) Thin passthrough, and the FrameGate stays reserved

The loader is Option 1, a thin passthrough: `native_core.rs` `dlopen`s the core and forwards
the `retro_*` calls directly. It does not route through the switch-wrapper's IoC FrameGate.
That gate is the inversion-of-control seam built for the future standalone Switch engine, and
it stays reserved for it: the stub wrapper and its 15/15 harness are still a live, independent
gate, and none of it is on the PS1 path. A PS1 frame goes core, staging, Metal, entirely
inside Rust, with no gate in the middle.

### (c) The environment commands the loader had to learn

The stub answered about six environment calls. PCSX ReARMed issues far more inside
`retro_load_game`, and would refuse to load against the old surface, so `on_environment` was
extended (every command number machine-checked against `.work/hdr/libretro/libretro.h`, with
an inline comment citing each value, because §1 already recorded that an experimental-bit
mistake produces a case that can never match):

- `SET_PIXEL_FORMAT` (10) is mandatory. The core points at a `c_uint`; the loader maps it
  (libretro 1 = XRGB8888, 2 = RGB565), stores the choice, and returns true. `0RGB1555` (0) is
  refused with false, because the compositor does not normalise it.
- `GET_VARIABLE` (15) returns false with a null value, which libretro defines as "use the core
  default". PCSX ReARMed reads 73 variables; none are fabricated, so the core runs on its own
  defaults. That is deliberate: inventing option strings is how a core ends up configured
  wrong in ways nobody chose.
- The option and descriptor families are accepted as no-ops returning true:
  `SET_VARIABLES`, the `SET_CORE_OPTIONS*` variants and their display/update callbacks,
  `SET_INPUT_DESCRIPTORS`, `SET_CONTROLLER_INFO`, `SET_PERFORMANCE_LEVEL`, `SET_SYSTEM_AV_INFO`,
  `SET_GEOMETRY`, `SET_MESSAGE` and `SET_MESSAGE_EXT`. Accepting them keeps the core happy
  without claiming a capability that is never delivered.
- Two calls are deliberately refused (they fall through to the silent default-false arm):
  `SET_AUDIO_BUFFER_STATUS_CALLBACK` and `GET_INPUT_BITMASKS`. §1's lesson is not to claim a
  callback you will never make, so the audio-buffer callback is refused cleanly; refusing
  bitmasks routes the core to per-id `input_state`, which `InputSnapshot::libretro_state`
  already serves. The default arm stays silent so a per-frame refusal cannot spam the log.

### (d) Pixel-format negotiation

`video()` used to hardcode `PixelFormat::Rgba8888`. It now reports the format the core
negotiated. PCSX ReARMed chooses RGB565 by default and XRGB8888 only if
`pcsx_rearmed_rgb32_output` is on; both are normalised on the CPU by `gfx/convert.rs`, so
reporting the true format is exactly what makes PS1 colours come out right. The negotiated
value is carried through a process-global `Mutex<Option<PixelFormat>>` alongside `DIRECTORIES`,
for the same lifetime reason: `SET_PIXEL_FORMAT` fires inside `retro_load_game` under a
Mutex-held bridge, possibly off the callback thread. It is reset before each load so a stale
value from a prior core cannot leak, and it defaults to the declared descriptor format when no
`SET_PIXEL_FORMAT` arrived.

### (e) BIOS handling and the HUD

The system directory is `applicationSupportDirectory`, created if absent and passed as both
`systemDir` and `saveDir`. A real BIOS placed there (for example `scph1001.bin`) raises
compatibility, but PCSX ReARMed does not require one: with no BIOS it falls back to HLE (its
`pcsx_rearmed_bios` option, `Config.HLE`) and still boots, at reduced accuracy. Because that is
a compatibility note rather than a hard failure, the app checks the system dir for the known
BIOS filenames and puts the result on the HUD ("BIOS: scph1001.bin" or "BIOS: none, HLE
fallback"), so a missing BIOS is a legible on-screen condition rather than a silent drop in
accuracy. No BIOS is ever bundled: shipping a PS1 BIOS is a copyright violation.

### (f) need_fullpath and the fullpath plumbing

PCSX ReARMed declares `need_fullpath = true` unconditionally and hard-rejects a load if
`info->path` is NULL (its `frontend/libretro.c` around line 2010, "info->path required"): it
opens and reads the disc image itself and ignores the data pointer. So the launch filename has
to be a real, openable path, not the file stem the stub got away with. `ContentHint` gained an
optional `full_path`, populated from the launch path when it contains a directory separator,
and `load_content` hands the core that real path when the rom bytes are empty. On the Swift
side the app writes a placeholder file to a real path in the writable area and passes that path
as the launch filename with empty rom bytes. No commercial ROM is bundled, and CI cannot boot a
game anyway; the point of this step is that the loader and the path plumbing run end to end. On
device, dropping a real `.cue`/`.bin`/`.pbp`/`.chd` at that path would boot it.

### (g) The build-core.sh iOS strategy and the artefact chain

`scripts/build-core.sh` gained an isolated iOS path, Darwin-guarded and completely separate
from the wasi-sdk web strategies. It clones `libretro/pcsx_rearmed`, inits its submodules
(lightrec, libchdr), and runs `make -f Makefile.libretro platform=ios-arm64 IOSSDK=<sdk>`,
which sets ARCH=arm64, BUILTIN_GPU=neon and, importantly, DYNAREC=0. The dynarec/lightrec JIT
is force-disabled for iOS arm64, so the first green build is interpreter only: correct but
slower, no JIT entitlement dependency to fight, which is the right tradeoff for proving the
loader. The Makefile emits `pcsx_rearmed_libretro_ios.dylib`, and the build reasserts its
install_name to `@rpath/pcsx_rearmed_libretro_ios.dylib` so it can be `dlopen`ed from
`Frameworks/`.

That one filename is load-bearing across five files, and a single divergence is what turns CI
red:

```
scripts/build-core.sh   produces  native/ios/build/lib/pcsx_rearmed_libretro_ios.dylib
native/ios/build-engine.sh stages  into build/lib/ and hard-fails if it is missing
native/ios/project.yml  embeds     build/lib/pcsx_rearmed_libretro_ios.dylib (embed:true, link:false, codeSign:false)
native/ios/package-ipa.sh signs    every Frameworks/*.dylib, with a fallback copy of the same name
.github/workflows/ios.yml verifies unzip -l "$IPA" | grep -q pcsx_rearmed_libretro_ios.dylib
```

It is embedded but not linked, reached by `dlopen`, for the same reason as the stub wrapper: a
wrong rpath then fails to load with a legible HUD line instead of stopping the app from
launching at all. The stub wrapper stays embedded alongside it; its harness is still a gate.

### (h) What remained unproven at this step (resolved on device, §18)

**Resolved 2026-09-20.** On-device PS1 boot is now established: Crash Bandicoot (USA) on
`pcsx_rearmed`, 2390 frames at 60 fps with 0 dropped, `BIOS (pcsx_rearmed): none, HLE fallback`.
See §18. The boundary stated below was accurate for CI and is still accurate for CI; it was a
device run, not CI, that closed it, and the HUD lines it names are what carried the result back.

On-device PS1 boot is unproven, and CI cannot prove it. CI proves exactly one thing: that the
macOS build produces a signed bundle that embeds the core dylib. It does not, and cannot, boot
a game, because no ROM or BIOS is bundled and there is no device in the loop. So "the loader
loads PCSX ReARMed, negotiates a pixel format, and renders a PS1 frame on a real phone" is not
established by anything here. The HUD is the diagnostic for when it is finally run on device:
the status line, the BIOS/HLE line, the GPU line, and the frames/fps/dropped counter each
distinguish a different failure, and on a sideloaded build with no debugger they are the only
diagnostics there are. The Swift itself is checked here only as far as `swiftc -frontend -parse`
reaches, which per §16 is a spell-checker, not a compiler: anything needing UIKit is proven
only by the cloud build.

### (i) Multi-file cue/bin and the iOS folder scope

A CD-based PS1 game is not one file. A `.cue` sheet is a short text descriptor that names one
or more `.bin` track files sitting next to it, and PCSX ReARMed (need_fullpath, §(f)) opens the
`.cue` and then opens each `.bin` it references itself. That is exactly what the original
single-file `.fileImporter` broke: iOS grants a security scope to the ONE file the user picks,
so when the user picked the `.cue`, the core's `fopen` of the adjacent `.bin` was outside any
granted scope and iOS denied it. Cue/bin games therefore failed on device even though the same
core reads them fine on a desktop. The fix is purely about the scope iOS grants; the core and
the Rust launch path are unchanged (they already accept a real absolute file path).

The fix, Option 1 (primary), is to pick a FOLDER instead of a file. The picker now offers
`allowedContentTypes: [.folder]` (`UTType.folder` is a valid system type, no exported type
declaration needed), so the user selects the folder that holds the `.cue` and its `.bin`
tracks. `EngineHost.launch(url:)` treats the URL as a folder, calls
`startAccessingSecurityScopedResource()` on the FOLDER, and holds that scope in
`activeScopedURL` for the whole session. A folder scope covers the entire subtree, so the core
can open the `.cue` AND every adjacent `.bin` under it. The scope is released only in
`stopSession()` (when the session ends or a new folder is picked) and in the no-entry and
launch-failure error paths, never in a defer right after launch, because the core keeps the
files open for the session. There is exactly one start per successful pick and exactly one
matching stop, so the scope is balanced and never double-started or leaked.

Inside the folder the host chooses the launch entry, the single file it hands the core, by a
fixed extension priority: `.cue` > `.pbp` > `.iso` > `.chd` > `.bin`. The `.cue` (or a
self-contained `.pbp`/`.iso`/`.chd` image) is what the core is given; `.bin` is a last resort
for a raw single-track image with no descriptor. Enumeration is
`FileManager.default.contentsOfDirectory(at:includingPropertiesForKeys:options:)` with hidden
files skipped, extensions compared case-insensitively. When several files share the winning
extension the choice is deterministic: candidates are sorted by `lastPathComponent` and the
first is taken, and the chosen file plus its folder are named on the HUD. If the folder holds
no loadable entry, the HUD shows a legible "no .cue/.pbp/.iso/.chd found in <folder>" line, the
folder scope is released, and no launch occurs.

Option 2 (secondary, complementary) adds two Info.plist keys: `UIFileSharingEnabled` exposes
the app's Documents directory in the Files app and Finder, and
`LSSupportsOpeningDocumentsInPlace` lets the app open documents in place. Together they let a
user drag a folder of a `.cue` plus its `.bin` tracks straight into the app's Documents
directory. Files inside Documents are inside the app sandbox, so they need no security scope at
all; this is the drag-a-folder-in path that bypasses the picker.

Those two keys have to be injected through `native/ios/project.yml`'s `info.properties`, not
just written into `native/ios/Info.plist`. XcodeGen GENERATES the Info.plist at `info.path` on
every `xcodegen generate`: the emitted plist is a fixed set of auto-default keys
(CFBundleIdentifier, CFBundleName, CFBundleDevelopmentRegion, CFBundleExecutable,
CFBundleInfoDictionaryVersion, CFBundlePackageType, CFBundleShortVersionString=1.0,
CFBundleVersion=1) MERGED with the target's `info.properties` map, and it OVERWRITES the on-disk
Info.plist if it differs (XcodeGen `Sources/XcodeGenKit/FileWriter.swift` `writePlists` plus
`InfoPlistGenerator.swift`). It does NOT read the committed Info.plist content. So the two new
keys, plus every existing custom key (CFBundleDisplayName, the 0.8.0/1 version overrides,
LSRequiresIPhoneOS, MinimumOSVersion, UIRequiredDeviceCapabilities, UILaunchScreen,
UIApplicationSupportsIndirectInputEvents, UIStatusBarHidden,
UIViewControllerBasedStatusBarAppearance and the two orientation arrays), now live in
`info.properties` so the built app's plist is complete. The committed `native/ios/Info.plist`
is kept as a faithful human-readable seed with the same keys, but it is `info.properties` that
determines what ships.

As with the rest of §17, on-device multi-file cue/bin boot cannot be proven in this sandbox and
is not proven by CI either. There is no macOS, Xcode, iOS SDK or xcodegen here, and
`swiftc -frontend -parse` is a spell-checker (§16), so the `.fileImporter([.folder])` call, the
`FileManager` enumeration and the security-scope balance were reviewed by eye. CI proves only
that the macOS build produces a signed bundle embedding the core dylib. That a real phone picks
a folder, holds the folder scope, and lets PCSX ReARMed read a `.cue` and its `.bin` tracks is
verified only by the orchestrator's CI build and a subsequent on-device run, not here.

**Resolved 2026-09-20, but by the design that superseded this one.** Multi-file cue/bin works on
device: a `.cue` and its `.bin` were selected together in one import, both landed in Documents,
and the `.cue` booted (§18). That is §(j)'s import-and-copy Library, where content lives inside
the app sandbox and no security scope is involved at all. The folder-scope picker described in
this subsection is therefore still unexercised on device; what the device run proves is the path
that replaced it. Keep this subsection for why the single-file `.fileImporter` failed, which is
the reasoning the import path inherited.

### (j) Every system in the .ipa: five cores, one loaded at a time

The .ipa shipped one core. It now ships five, which is every core the web build has plus PS1:

| core | systems | dylib in Frameworks/ |
| --- | --- | --- |
| `fceumm` | NES | `fceumm_libretro_ios.dylib` |
| `snes9x` | SNES | `snes9x_libretro_ios.dylib` |
| `mgba` | GBA, GB, GBC | `mgba_libretro_ios.dylib` |
| `genesis_plus_gx` | Mega Drive, Master System, Game Gear | `genesis_plus_gx_libretro_ios.dylib` |
| `pcsx_rearmed` | PS1 | `pcsx_rearmed_libretro_ios.dylib` |

Those five filenames are the load-bearing strings of this whole change. They are defined once,
in `ios_core_config()` in `scripts/build-core.sh`, and they have to agree byte for byte with
`native/ios/project.yml` (five `embed: true, link: false, codeSign: false` framework entries),
`native/ios/package-ipa.sh` (the embed fallback), `.github/workflows/ios.yml` (one `unzip -l`
grep per core, each with its own `::error::` naming the system that would not run) and
`CoreCatalog` in `native/ios/ContinuumApp.swift`. `build-engine.sh` and `package-ipa.sh` do not
restate them at all: they read them from `scripts/build-core.sh ios-names`, and build-engine.sh
asserts the list has exactly five entries before it trusts it. Nothing in Xcode will ever tell
you one of these is wrong. The app builds, installs, launches, and then cannot find a core.

#### `build-core.sh ios <core>`, and why it is a subcommand

`scripts/build-core.sh <name>` means "build `<name>` as WASM for the web", and those spellings
are live: `web/cores/README.md`, `README.md`, this document, the `buildHint` strings in
`scripts/core-abi-test.mjs` and the `build-core.sh all` step in `.github/workflows/deploy.yml`
all use them. The first iOS core was added as `build-core.sh pcsx_rearmed`, which was safe only
because `pcsx_rearmed` has no WASM case block. Teaching `build-core.sh fceumm` to build an iOS
dylib would have broken the web build, quietly, in the one place nobody looks until the PWA
stops loading a core. So the iOS builds got their own namespace instead:

```
scripts/build-core.sh fceumm       # WASM. UNCHANGED, and every bare core name still means WASM.
scripts/build-core.sh all          # WASM, all four web cores. UNCHANGED.
scripts/build-core.sh ios fceumm   # one iOS dylib -> native/ios/build/lib/
scripts/build-core.sh ios-all      # all five, and it keeps going after a failure
scripts/build-core.sh ios-names    # print the canonical filenames, on any host
```

The iOS dispatch runs before `ensure_toolchain`, so it never fetches wasi-sdk and never creates
or writes `web/cores/`. `pcsx_rearmed` on its own still works as an alias for
`ios pcsx_rearmed`. `ios-names` is the one iOS subcommand that runs off a Mac, which is what
makes the filename table checkable from Linux, and it is also how the two shell scripts
downstream learn the list.

`ios-all` deliberately does not stop at the first broken core. The macOS runner is the only
compiler this project has, so a run that dies on core one costs a whole cycle to learn about
core two. Each core builds in a subshell, every failure is named, the summary lists what is and
is not in `native/ios/build/lib/`, and the command still exits non-zero so `build-engine.sh`
and CI still go red.

#### How each core actually builds, from reading its makefile

Four of the five have a libretro makefile with a real `platform=ios-arm64` target, and all four
emit `$(TARGET_NAME)_libretro_ios.dylib`, which is already the canonical name:

* `fceumm` and `genesis_plus_gx` build at the repo root with `-f Makefile.libretro`. fceumm's
  root `Makefile` is a one-line include of it.
* `snes9x` is the exception worth knowing: its makefile is `libretro/Makefile` and it sets
  `CORE_DIR := ..`, so make has to run **inside** the `libretro` subdirectory and the dylib
  lands there. It is C++, and its `LD` is `$(CXX)`, so libc++ comes in by itself.
* `pcsx_rearmed` is unchanged from the build that already worked: recursive submodules
  (lightrec, libchdr), and its iOS block force-disables the dynarec, so it stays
  interpreter-only.

`mgba` has no makefile at all. Its libretro core is a CMake target, which is also why the WASM
path uses CMake for it: CMake generates files the build needs, `version.c` among them. So the
iOS path configures CMake with `CMAKE_SYSTEM_NAME=iOS`, `CMAKE_OSX_ARCHITECTURES=arm64` and
`CMAKE_OSX_SYSROOT=<iphoneos sdk>`, builds the static `mgba_libretro.a`, and then links the
dylib itself:

```
cc -arch arm64 -isysroot "$IOSSDK" -miphoneos-version-min=16.0 \
   -dynamiclib -install_name "@rpath/mgba_libretro_ios.dylib" \
   -o mgba_libretro_ios.dylib -Wl,-force_load,mgba_libretro.a \
   -framework Foundation -lm
```

`-force_load` is the load-bearing flag. Nothing in that link references `retro_run` or any other
entry point, so without it the linker pulls in no archive members and hands back a valid, empty
dylib that dlopens and then has no libretro API in it. `-framework Foundation` matches the
`OS_LIB` mgba's CMakeLists appends on Apple. Two upstream details to expect in a CI log: mgba's
CMakeLists forces `CMAKE_OSX_DEPLOYMENT_TARGET` to 10.6 inside its `if(APPLE)` block, which only
lowers the objects' minimum OS and cannot stop them loading on iOS 16, and it appends `-flto` to
the Apple Release flags, so the archive holds bitcode the linker resolves at link time. The
configure also passes `-DCMAKE_TRY_COMPILE_TARGET_TYPE=STATIC_LIBRARY`, because
`CMAKE_SYSTEM_NAME=iOS` puts CMake in cross-compiling mode and mgba's configure then runs probes
that would otherwise link an executable. Skewed link-based function probes are the only cost, and
every function mgba probes for exists on Darwin anyway, which is why its own CMakeLists guards
the compensating override with `AND NOT APPLE`.

#### Three checks that exist because of how this can fail quietly

Everything downstream of a staged dylib only ever asks whether a file exists: build-engine.sh
checks the path and prints a size, ios.yml greps the zip listing for the name, and the Swift host
checks `Frameworks/`. All three pass for an empty dylib, and the failure then lands on device a
whole build-and-sideload cycle later. So `ios_stage_dylib` asserts the API is actually present,
for all five cores, with `nm -gU | grep -q retro_run`, at the moment the artefact is staged. That
is the guard the mgba link in particular needs.

`ios-all` runs each core as a separate `bash` process, not a subshell. Bash disables errexit for
a command used as an `if` condition and that suppression is inherited by subshells inside it, so
`if ( build_ios_core "$core" ); then` would let a failing `make`, `install_name_tool` or `cp`
fall through to the artefact check, leaving "did a .dylib appear" as the only pass criterion. A
stale dylib from an earlier run satisfies that. A separate process re-reads the script and
applies its own `set -euo pipefail`. For the same reason the expected dylib is deleted before
each build and before each staging: `.work/ios/<core>` persists between runs, so nothing left
over from a previous build can be reported as this build's output.

`build-engine.sh` checks, before it compiles anything, that all five names appear in
`project.yml`, `ios.yml` and `ContinuumApp.swift`. Those three restate the names by hand and no
compiler can check them, so a typo would otherwise cost twenty minutes of core builds and then
produce a green build with an app that cannot find a core. The check costs seconds and fails
naming the file and the consumer.

The macOS job's `timeout-minutes` went from 60 to 120 in the same change. The build step now
compiles five native cores rather than one, including mgba's LTO build and link and
pcsx_rearmed's full-depth clone with recursive submodules, and nothing but cargo is cached
between runs. A timeout is this job's worst failure because it produces no artefact and no
`::error::`, so it reads like an infrastructure hang. If runs get slow enough to matter, the next
lever is an `actions/cache` step for `.work/ios` keyed on `hashFiles('scripts/build-core.sh')`,
which is safe now that a stale artefact can no longer be mistaken for a fresh one.

iOS sources are cloned into `.work/ios/<core>`, not `.work/<core>`. The WASM path clones the same
repositories into `.work/<core>` and both builds compile in tree, so a shared directory would
let stale wasm objects be linked into an iOS dylib.

#### Extension to core, in exactly one place

The Library (import-and-copy into Documents, which superseded the folder-scope design in §(i))
now accepts every ROM extension the five cores cover, and routes a tap by extension:

| extension | core |
| --- | --- |
| `.nes` | `fceumm` |
| `.sfc`, `.smc` | `snes9x` |
| `.gba`, `.gb`, `.gbc` | `mgba` |
| `.sms`, `.gg`, `.md`, `.gen` | `genesis_plus_gx` |
| `.cue`, `.chd`, `.pbp`, `.iso` | `pcsx_rearmed` |

`importableExtensions` is that key set plus `.bin`; `launchableExtensions` is derived as the
importable set minus `.bin`, so the two cannot drift apart. A `.bin` is a CD track that a cue
sheet names literally, so it must be importable for those references to resolve and must never
be tappable. The mapping lives once, in `CoreCatalog.routes`, and both the Library row and
`launch(entry:)` read it: each row's detail line ends with the core it will launch on, so a
routing mistake is visible in the list before a tap rather than as a game booting on the wrong
emulator. An extension with no mapped core writes its own HUD line and launches nothing.

Genesis Plus GX is the reason the launch path still passes the real filename through: the core
picks Mega Drive, Master System or Game Gear from the content extension, which is what
`ContentHint::from_filename` on the Rust side is for.

#### Declare five, load one

All five cores are declared when the surface attaches, and none are loaded. `declare_core`
stores a descriptor in the registry and touches no filesystem: no dlopen, no read of the dylib,
no core allocated. Only the core a tapped game needs is loaded, in `ensureCoreLoaded(coreId:)`,
which keeps the dynamic-loading rule intact. Five resident cores would be five emulators' worth
of memory for four systems nobody asked to play. The up-front declaration still earns its keep
twice over: a Library tap goes straight to the load, and a dylib that never reached
`Frameworks/` is named on the new `cores:` HUD line before the user taps anything, because from
the Library a missing dylib and a broken core look identical.

`ensureCoreLoaded(coreId:)` keeps the fix from §(h)'s successor turn verbatim, generalised to
five cores: `engine.coreState(coreId:)` is read fresh on every call, no Swift `Bool` caches it
anywhere, `loaded` and `bound` are the usable states, a nil state collapses to `unknown` and
takes the reload path, and after declare plus load the state is RE-READ rather than success
being inferred from nothing having thrown. It is still called AFTER `stopSession()`, never
before, because `engine.stop()` unloads the core under the `Drop` retention policy.

No Rust changed to ship four more cores. `NativeLibretroCore` is core-agnostic and
`CoreRegistry` already held many declared cores, which is the payoff for §(c) and §(d) having
been done properly: geometry, fps and sample rate are overwritten from
`retro_get_system_av_info` on load, and the pixel format is renegotiated through
`SET_PIXEL_FORMAT` inside `retro_load_game`. That last point has a concrete consequence worth
recording so nobody "corrects" it: fceumm's iOS makefile forces `WANT_32BPP`, so on device that
core negotiates XRGB8888 even though `CoreCatalog` declares RGB565 and the web build renders
RGB565. The declaration is a pre-load hint, the negotiation settles it, and the renderer
converts either.

#### Still not proven from here (largely closed by the device run, §18)

**Updated 2026-09-20.** The device run in §18 closed most of what follows: all five cores do
compile for `aarch64-apple-ios`, the `.ipa` does carry five dylibs (`cores: 5 of 5 declared` on
the HUD), and `.nes`, `.smc`, `.gba`, `.gbc`, `.md`, `.gg` and a PS1 `.cue` each imported and
launched on the core `CoreCatalog.routes` names. `.sms` and `.gb` remain unlaunched on device,
though their cores are proven by sibling extensions. The sandbox boundary below is unchanged and
still states exactly what can and cannot be checked from here, which is what the next core will
run into.

The same boundary as the rest of §17, and it has not moved. This sandbox has no macOS, no Xcode
and no iOS SDK, so nothing here compiled a single core dylib. What was verified: `cargo test`
74 passing and 84 with `--features native-core,uniffi-bindings` (the Rust is untouched),
`cargo check --profile ios --target aarch64-apple-ios` clean, `swiftc -frontend -parse` on both
Swift files, `bash -n` on all three shell scripts, `project.yml` and `ios.yml` loading as YAML
with all five dylibs asserted present and `info.properties` intact, `Info.plist` loading as a
plist, `build-core.sh ios-names` printing the five names, a scripted trace showing the names
agree across all six files, a scripted check that the routing table's keys are exactly the
launchable extension set, and a stubbed-copy harness proving that `fceumm`, `mgba`,
`genesis_plus_gx`, `snes9x` and `all` still route to the WASM path. What
was not: that any of the five cores compiles for `aarch64-apple-ios`, that the .ipa contains
five dylibs, and that a `.nes`, `.sfc`, `.gba`, `.sms`, `.md` or PS1 disc imports and launches
on the wrong or the right core. Those are established only by the orchestrator's CI build plus
a device run, and the HUD is the readout: the `cores:` line says which dylibs are in the bundle,
and every opening, running and failure line names the core id, so a wrong route or a missing
core is one glance rather than a deduction.

## 18. Device verification, 2026-09-20: five cores, seven systems, one phone

Recorded so that no later session repeats this work, or reads §16 and §17's "unproven" notes as
current. Hardware: iPhone 17 Pro Max (A19 Pro), running a sideloaded `.ipa` from the iOS
workflow. Every observation here comes from on-device screenshots of the HUD, which remains the
only instrument, exactly as §16 argued it would have to be.

Every screenshot showed `cores: 5 of 5 declared`, and every game held 60 fps with 0 dropped
frames. The frame count is whatever the counter read when the screenshot was taken, so treat it
as a lower bound on how long the core sustained the frame loop, not as a benchmark:

| system | core | content | frames at screenshot |
| --- | --- | --- | --- |
| NES | `fceumm` | Kart Fighter | 285 |
| SNES | `snes9x` | Super Mario World (U) | 466 |
| GBA | `mgba` | Pokemon Emerald (USA, Europe) | 321 |
| GBC | `mgba` | Pokemon Yellow (UE) | 1162 |
| Mega Drive | `genesis_plus_gx` | Mortal Kombat 3 (USA) | 2354 |
| Game Gear | `genesis_plus_gx` | Simpsons: Krusty's Fun House (U) | 2188 |
| PS1 | `pcsx_rearmed` | Crash Bandicoot (USA) | 2390 |

What that settles, subsystem by subsystem:

* **The five-dylib bundle, and declare-five-load-one (§17(j)).** All five dylibs reach
  `Frameworks/` under their canonical filenames, `dlopen` finds each one, and `declare_core` plus
  `ensureCoreLoaded(coreId:)` make exactly the needed core resident on demand. Seven launches
  across five cores in one session, no stale-`Bool` reload failure and no wrong-core route.
* **`CoreCatalog.routes` extension routing (§17(j)).** `.nes`, `.smc`, `.gba`, `.gbc`, `.md`,
  `.gg` and `.cue` each launched on the core the table names, with the row's detail line agreeing
  with the core that then ran. The pre-tap affordance works as designed.
* **`ContentHint::from_filename` for the shared core (§17(j)).** One `genesis_plus_gx` instance
  drove Mega Drive and Game Gear content correctly in the same session, which is precisely the
  discrimination nothing off-device could check.
* **Pixel-format renegotiation inside `retro_load_game` (§17(c), §17(d)).** Cores with different
  native formats all drew correct pictures through the one software path, fceumm included, whose
  iOS makefile forces `WANT_32BPP` and so negotiates XRGB8888 against its RGB565 declaration.
  The declaration is a pre-load hint and the negotiation settles it: now confirmed on hardware,
  and still not a defect to "correct".
* **`need_fullpath` and multi-file cue/bin (§17(f), §17(i)).** A `.cue` and its `.bin` imported
  together in one selection, copied into Documents, and booted, with the core opening the track
  itself. This proves §17(j)'s import-and-copy Library, not §17(i)'s folder scope.
* **BIOS-less PS1 through HLE (§17(e)).** No BIOS file was present. The HUD read
  `BIOS (pcsx_rearmed): none, HLE fallback` and the game booted regardless.
* **The Metal software path and the memory entitlement (§16).** 60 fps sustained with 0 dropped,
  and roughly 6.8 GB available to the app, so the increased-memory entitlement survived the
  installer that was used.
* **Library bookkeeping (§17(j)).** `imported 5 of 5` for a single five-file batch.
  `library: 6 game(s) of 7 file(s) in Documents`, the seventh file being Crash Bandicoot's `.bin`
  track: `launchableExtensions` is `importableExtensions` minus `.bin`, so the track is
  importable and never tappable, and that count is correct rather than a discrepancy. The
  cue-summing `sizeText` read `CUE · 602.8 MB · pcsx_rearmed`, the sheet plus its unique tracks,
  not the 87-byte sheet.

### What this run did NOT establish

* `.sms` and `.gb` were never launched, for want of content. Both route to a core proven by a
  sibling extension (`genesis_plus_gx` by `.md` and `.gg`, `mgba` by `.gba` and `.gbc`), so the
  residual risk is in the routing row and `ContentHint::from_filename`, not in the core itself.
  Five of five cores are verified; seven of nine systems are.
* The panic-to-HUD unwind path (§16) still has not fired, because nothing panicked.
* §17(i)'s folder-scope picker remains unexercised, having been superseded by the import path.
* `.sfc`, `.gen`, `.chd`, `.pbp` and `.iso` were not exercised; each shares a core and a routing
  row with an extension that was.
* Nothing here measured audio output, input latency, thermal behaviour or a long session. "60 fps,
  0 dropped" is what the HUD showed at the moments captured.

---

## 19. Sound, at last: the iOS audio output path

Until this step the device had no audio output whatsoever. The engine produced PCM, `RingAudioSink`
resampled it, `AudioRing` buffered it, and nothing ever read it: `drain_audio` existed on the
bridge (`bridge.rs:617`) but was not on the UniFFI surface, and no Swift file mentioned
AVAudioEngine. The HUD's "69 ms audio" was a ring filling and never emptying, and its own comment
said so. The browser build had working audio the whole time, which is why this step is mostly
transcription rather than invention: `wasm.rs` already exported `drainAudioInto`,
`setOutputSampleRate` and `audioQueuedFrames`, and the native facade now exports the same
capabilities in UniFFI's shapes.

### Push from the tick, pull from the render callback

```text
CADisplayLink (main thread)                audio IO thread (real-time)
engine.applyGamepad(...)
engine.tick(nowMillis:)      <- holds the engine Mutex for the whole tick
engine.drainAudio(maxFrames:) <- takes it again, briefly
ring.append(...)  ring.publish()           ring.pull(...) -> AudioBufferList
```

The obvious implementation has the render block call `drainAudio` itself. That is a real-time
thread taking the same `Mutex<EmulatorBridge>` the display link holds for the entire duration of
every tick, sixty times a second: priority inversion, heard as clicks and dropouts rather than seen
as a stall, and worst exactly when the emulator is busiest. So `MetalCanvas.tick` drains into a
Swift-side ring immediately after the step, and `AVAudioSourceNode`'s render block reads that ring.
The render block never calls into Rust, never locks, never allocates and never blocks; everything
it touches is allocated when `AudioOutput` is built.

### The ring has no atomics, and is still correct

Swift below iOS 18 has no fence it may legally use here: `Synchronization.Atomic` is iOS 18 and
this app targets 16, C11 atomics are not importable, `OSMemoryBarrier` is deprecated and Apple's
own position is that imported atomics are not to be trusted from Swift, and every lock is banned on
a real-time thread by construction. So `AudioSampleRing` buys its ordering with arithmetic:

**Each side publishes its cursor one call late.** The producer stores the frontier it reached at
the end of the *previous* tick before appending anything new, so the consumer is only ever shown
samples written at least one display frame ago. The consumer does the same with its read cursor. In
between, each thread performs a dispatch, a Rust mutex acquire and release, and a good deal of ARC
traffic, every one of which is an atomic read-modify-write and therefore a full barrier on arm64.

The residual error is benign by construction, which is what makes it a design rather than a hope.
Cursors only increase, so a stale read is always a *smaller* number: a consumer reading a stale
publish sees less audio and underruns a hair early, and a producer reading a stale consume sees
less free space and asks Rust for fewer samples, where Rust's ring already has documented overrun
behaviour. Neither side can ever see a cursor ahead of the truth, so neither can read an unwritten
slot or overwrite an unread one. Two consequences worth knowing:

- **`ring.publish()` must be called on every tick, including ticks that append nothing.** It is the
  store that hands the previous tick's samples over. A path that returns early without it leaves
  the consumer silent while the ring fills behind it, which is the original bug wearing a disguise.
- When the deployment target reaches iOS 18 this becomes two `Atomic<Int>` with explicit
  `.releasing`/`.acquiring` orderings and the lag can go. Until then **the lag is the ordering
  guarantee.** Do not simplify it away.

### The rate is negotiated, not assumed

`EmulatorBridge` defaults `output_sample_rate` to 48000 because something has to be assumed before
an audio graph exists. On iOS that assumption is wrong often enough to matter: a modern iPhone
speaker is 48000, several Bluetooth routes are 44100, and a wired interface can be neither. So
`AudioOutput` activates the session, reads `AVAudioSession.sampleRate`, and reports it through
`setOutputSampleRate`. Resampling then happens in `audio/resample.rs`, which already does it and is
already tested; a second implementation in Swift would be a second thing to keep correct, and
getting it subtly wrong sounds like a slightly out-of-tune game rather than like a bug.

A rate change drops both rings, because the queued tail was resampled for a device that has gone.
That is what `flush_audio` is for.

### Latency, and why it is what it is

Steady state is roughly 50 ms: a 33 ms prime buffer (two video frames, expressed in seconds so it
means the same thing at 44100 and 48000) plus one tick of publish lag, with the Rust ring near zero
because the tick empties it. The Swift ring is 8192 frames, which is headroom rather than latency:
it is what absorbs a tick that ran late without anything being dropped. On an underrun the render
block emits silence, counts it, and **re-primes** rather than limping along at whatever depth
starved it. That re-prime is also why pausing a game costs exactly one counted underrun instead of
one per callback.

### Traps paid for in this step

- **`#[uniffi::export]` and `#[cfg]`, again.** Nothing new here is platform-split, which is the
  point: §16 records that a `cfg`-gated method inside the export block generates scaffolding that
  fails to compile off-Apple, so `drain_audio` and friends are unconditional.
- **`Vec<f32>`, not a pointer.** UniFFI copies, and that is accepted deliberately: the copy is
  about 6 KB per tick on the display link's thread, and it is what keeps the render thread out of
  the engine lock. `drain_audio` sizes its allocation from `audio_stats().queued_frames` rather
  than from `max_frames`, so a caller that always asks for the ceiling does not churn 32 KB to
  return 6 KB.
- **`AudioOutput.swift` had to be added to `project.yml`.** The sources list is explicit, so an
  unlisted file is simply never compiled and the app builds, installs, launches and is silent with
  no error anywhere. For a feature whose failure mode is *the absence of a sound*, that is the
  worst possible way to lose it.
- **An `AVAudioSourceNode`'s format is fixed when it is constructed.** A route change can change
  the rate, so route changes and `AVAudioEngineConfigurationChange` rebuild the graph rather than
  poking it. The unplug case is the one that has to work: iOS pauses playback when the old device
  goes away, and an app that ignores the notification is permanently silent afterwards.
- **The render block writes no strings.** Composing one would allocate. It moves counters, and the
  read-out turns those into words on the main thread.

### Diagnostics, because "no sound" and "not implemented" look identical

Every failure path writes a distinct sentence into `AudioOutput.status`, and the HUD shows it. The
audio line reports the real rate, both ring depths summed, and both underrun counters separately,
because an engine underrun means the core did not produce in time while a device underrun means the
tick did not push in time. The single most useful field is `ring.renderedFrames`: zero while the
graph reports itself running means the render block is not being called at all, and the line says
`SILENT (the render block has not run)` rather than printing a plausible latency for a buffer
nobody is reading.

### Verified here

`cargo test` 74, `cargo test --features native-core,uniffi-bindings` 84, `cargo check --profile ios
--target aarch64-apple-ios --features native-core,uniffi-bindings` clean, `cargo check --target
wasm32-unknown-unknown` clean, `cargo clippy --features native-core,uniffi-bindings --all-targets
-- -D warnings` clean. The generated Swift facade was produced from the host cdylib as §16
describes and `swiftc -typecheck`ed, confirming `drainAudio(maxFrames: UInt32) -> [Float]`,
`audioStats() -> AudioStatsSnapshot`, `setOutputSampleRate(rate: UInt32)`, `outputSampleRate() ->
UInt32` and `flushAudio()`. `swiftc -frontend -parse` passes on all fourteen Swift files.

**Not verified here, and it cannot be:** whether sound comes out. `swiftc -frontend -parse` is a
spell-checker (§16), so no AVFoundation call site in `AudioOutput.swift` has been type-checked, and
nothing on this box can run a render callback. The first device run should read the audio line
before anything else: `SILENT` points at the graph, a growing engine-side buffer points at the
pump, and a rising device underrun count points at the tick being late.

---

## Appendix: relocated from README.md

`README.md` was rewritten for the repo owner, who does not write code, so it now covers the
product and not the architecture. Three things lived only in that file and had no equivalent
anywhere else in this document. They are recorded here rather than lost. All three describe the
legacy browser build, and the first two carry over to the iOS UI as unfinished work.

**Browser input mapping.** The web front end bound: arrow keys to the D-pad, `Z` and `X` to B and
A, `A` and `S` to Y and X, `Q` and `W` to L and R, `Enter` to Start, `Shift` to Select, `P` to
pause and `Esc` to back. Controllers were picked up automatically and assigned to the first free
port, and the HUD showed how many were connected. On touch devices the on-screen pad appeared,
with its D-pad tracked as a single surface so diagonals resolved. Keyboard, pad and touch could
all be used at once, because `GamepadBridge` merges them per source in Rust (§3). The iOS touch
overlay has the same job to do and inherits the same merge.

**Audio quality.** The resampler in `src/audio/resample.rs` is linear. A windowed-sinc belongs
there before anyone judges the sound quality on either platform. This is not a bug and nothing is
blocked on it; it is a known ceiling.

**Rewind and fast-forward.** `FramePacer` already supports a speed multiplier, so fast-forward is
close to free. Rewind is not: save states measure roughly 13 KB for NES, 823 KB for SNES, 516 KB
for GBA and 1 MB for Genesis, so a rewind ring needs a memory budget and probably compression
rather than just a deeper buffer. On iOS the increased-memory entitlement changes that arithmetic
but does not remove it.


## 20. The settings become real, rewind arrives, and the web app is deleted

Three changes, in this order because the order mattered.

### 20.1 The engine exports what the settings screen had been apologising for

Everything the Settings screen listed under "NOT WIRED YET" claimed the same thing: the engine can
already do this, it just cannot be reached from Swift. Four of the five claims were true. Scale
mode, filter and the speed multiplier existed on `EmulatorBridge` and were simply missing from
`uniffi_api.rs`. Volume genuinely did not exist at any layer.

`ScaleModeOption` and `ScaleFilterOption` are the crate's first `uniffi::Enum`, and they **mirror**
`gfx::ScaleMode` and `gfx::ScaleFilter` rather than those types carrying the derive themselves. That
is deliberate and should not be "simplified": the graphics layer is not supposed to know a foreign
function interface exists, which is the same reasoning that keeps `MetalHandles` unexported, and it
keeps a settings-screen vocabulary at the boundary instead of a shader vocabulary.

**Two bugs were fixed that would have made these look broken on the first build, and neither was in
the new code.** `FramePacer` is rebuilt by every `launch` and every `stop`, and the renderer's frame
target is released on stop, so a preference written only into those objects reverts to its default
the next time a game starts: the user sets 2x, launches, and silently gets 1x. The wanted values now
live on the bridge and are re-applied in `launch` **and** in `attach_renderer`, in either order,
because whether the host restores its settings before or after Metal is ready is not something the
engine should depend on. **Do not move those re-applications.**

The second was audio under fast-forward. At 2x the core produces twice the samples per wall-clock
second while the device still consumes one second's worth, so the fixed ring overwrote its own
oldest audio several times a second, which is heard as chopping. `set_speed` now also re-declares
the sink's source rate as `native * speed`, so the resampler consumes the surplus and the stream
stays continuous and rises in pitch. `Resampler::set_source_rate` had existed and been called from
nowhere since it was written; this is what it was for.

Volume is applied in `EmulatorBridge::drain_audio`, on the far side of the ring, for two reasons:
scaling on the way in would delay a volume change by the whole buffered backlog, and this runs on
the display link rather than the real-time render thread. It is **ramped** across each drained
block, not stepped. A slider under a finger sets a new target every frame and a gain jump at a block
boundary is a step discontinuity in the waveform, heard as a click sixty times a second.

**The ceiling a UI must respect:** `MAX_CATCH_UP_STEPS = 4` in `timing.rs` means the real speed
limit is about 4x on a 60 Hz screen regardless of the multiplier requested. The excess is forfeited
and shows up as a rising `dropped` count. `EmulationSettings.FastForward` therefore stops at 4x. Do
not add an 8x entry.

### 20.2 Rewind: save states as a tape

`crates/emulator-bridge/src/rewind.rs`. Two properties drove the design.

**The budget is in bytes, not snapshots.** Save-state sizes differ by two orders of magnitude across
these systems, so "keep 600 snapshots" is ten seconds of rewind on NES and an out-of-memory kill on
PS1. iOS terminates a process that grows too large rather than paging it. The budget is the promise;
how much time it buys varies by system, and `EmulationSettings.refreshRewindReadout` divides the
budget by the running core's real state size to say what it actually bought.

**The steady state allocates nothing.** At ten snapshots a second a 500 KB state would be 5 MB of
allocation and 5 MB of free every second, forever. Evicted buffers go to a free list capped at
`MAX_POOLED_BUFFERS` and are handed back out, so a snapshot is a `memcpy` into memory already owned.
`push_with(size, closure)` exists so the core writes straight into the pooled buffer instead of into
a temporary that is then copied.

**Rewinding lives in `EmulatorBridge::tick`, not in Swift.** `tick` diverts to `tick_rewinding` when
the flag is set, and Swift only sets the flag. Driving it from outside, by calling a rewind method
and then `tick`, would advance the core and throw that frame away sixty times a second, and the
picture would judder rather than reverse.

`tick_rewinding` restores a snapshot and then runs **exactly one** core frame. That frame is the
whole trick and must not be removed as an optimisation: a libretro core's framebuffer is whatever
`retro_run` last wrote, and `retro_unserialize` changes the core's memory without redrawing
anything, so loading a state and presenting immediately shows the image from *before* the rewind.
The screen would appear frozen while the emulator silently travelled. Audio from that frame is
produced and then flushed, because not draining the core lets its internal batch buffer grow
unbounded and playing it would be a forward-running fragment under a backward-running picture.

The pacer is re-anchored on **every** rewind tick rather than once when the button is released. The
pacer is not consulted on that path, so its idea of "last tick" would otherwise freeze at the moment
rewind began, and releasing the button after a third of a second would read as a third of a second of
missed emulation and be answered with a sprint forwards.

The tape is cleared by `reset`, `load_state` and `stop`, because every snapshot on it describes a
future that no longer follows from the present.

### 20.3 The on-screen control layout editor

`TouchLayoutEditor.swift`. The pad on that screen **is the real pad**, mounted the way
`PlayerScreen` mounts it with `isEditing` true. A drawing of it would have to reimplement the unit
arithmetic, the cluster templates, the orientation rules and the clamping, and the second copy would
eventually disagree with the first, producing an editor that shows an arrangement you do not get.

Drags report continuously for the preview but only the touch that **ends** a drag is persisted:
binding a drag to the host's `@Published` layout would republish it sixty times a second and
re-evaluate the whole library shell underneath. Landscape writes only x, because the pad overrides
both clusters' y in landscape, and writing a number with no visible effect is a dead control in
disguise. The screen says so rather than hiding it.

`TouchLayout.mirrored` rounds to four decimals, and that is not tidiness: `1.0 - 0.17` is
`0.8300000000000001`, so without rounding `mirrored.mirrored` was not the identity, pressing Swap
sides twice left the layout a hair off, and `isStandard` then answered false about an arrangement
indistinguishable from the default, leaving Reset offering to do something invisible.

`TouchLayout`'s `init(from:)` is deliberately forgiving, using `decodeIfPresent` per field. The
synthesized initialiser throws the moment a single key is missing, which would turn a layout written
by a build that renamed one field into a total reset, and a user reads that as the app forgetting
their arrangement.

### 20.4 The web app is deleted

Deleted: `web/`, `.github/workflows/deploy.yml`, `core-shim/`, `src/wasm.rs`,
`src/cores/wasm_core.rs`, `src/cores/host.rs`, the five Node scripts, and the wasm half of
`scripts/build-core.sh` (962 lines to 547). The wasm dependency block is gone from `Cargo.toml` and
the wasm-only workspace dependencies with it.

**The crate no longer builds for `wasm32`, and that is intentional. Do not add that check back as a
gate.** It was a verification gate for months and its absence is a deliberate scope change, not an
oversight.

Two things about this that are easy to get wrong:

1. **`cores::validate_wasm_module` and `CoreRegistry::attach_module` are NOT dead and were kept.**
   They look like wasm leftovers and they are not: they are how the built-in diagnostic stand-in core
   loads, six tests depend on them, and the magic-header check is a real guard against a truncated
   download. The names are a leftover; the code is live.
2. **The iOS half of `build-core.sh` shares nothing with the deleted half.** That was verified
   mechanically before cutting, by extracting every `ios_*` and `build_ios_*` body and intersecting
   the identifiers against the list of defined functions: the iOS path calls only `ios_*` and
   `build_ios_*`. `ios-names` still prints the same five filenames, which is the contract
   `build-engine.sh` and `package-ipa.sh` read rather than repeating.

`wasm.rs` was the working reference implementation for four of the five knobs exported in 20.1,
which is why the exports were written **first** and the deletion came after. Anyone deleting a
facade should check what is using it as a worked example before removing it.

### 20.5 What is still not wired

The Settings list is down from seven entries to two:

- **Cover art captured from the running game** (artwork tier 4). The whole readback path exists in
  Rust and is unexported: `Renderer::encode_capture` at `gfx/renderer.rs`, `FrameCapture::take_rgba`,
  and `EmulatorBridge::encode_capture`, which uploads the current frame first so a paused session
  still captures correctly. What stands in the way is that `FrameCapture` is not a UniFFI type and
  the buffer map is asynchronous. The shape to copy is the deleted `wasm.rs` `captureFrame`, which
  dropped its borrow **before** awaiting the map; on iOS that borrow is the `Mutex` the display link
  holds, so awaiting while holding it would deadlock. `pollster` is already an Apple-target
  dependency and `attach_metal` already drives a future with it.
- **Physical controllers.** The engine merges input per source already, so a real pad and the
  on-screen pad could be used together, but the only exported input call replaces the whole gamepad
  layer, so the two would fight over it. Needs a per-source apply on the UniFFI surface, not a new
  input system.


## 21. Save states never worked, and the way they failed is the lesson

Reported by the owner, not by any check in this repository: save states were a feature of the
browser build and were supposed to carry over to the `.ipa`. They had never worked, for any core,
since the first real core was integrated.

### 21.1 The trap

`native_core.rs` resolved `retro_serialize_size` but **not** `retro_serialize` or
`retro_unserialize`, and `NativeLibretroCore` never implemented `save_state` or `load_state`. Both
therefore fell through to the `EmulatorCore` trait's defaults, which return
`BridgeError::NotImplemented`.

What makes this worth a section is how it hid. `state_size()` **was** implemented, on the one symbol
that had been resolved, so it returned a real and plausible number. Every layer above concluded the
core supported save states:

- The save button called into it, got an error, and printed it on the status line, where it looked
  like a per-core quirk rather than a missing implementation.
- **The rewind tape recorded nothing at all.** `RewindBuffer::push_with` asks the core to fill a
  buffer, the core returned `NotImplemented`, and `tick` logs a refused snapshot at
  `log::debug!` and carries on by design, because one lost rewind point must not drop a frame. So
  rewind shipped, reported a sensible budget, and silently held zero snapshots.

The general lesson, worth applying to the remaining unwired features: **a capability query answering
truthfully is not evidence that the capability is implemented.** `state_size()` and `save_state()`
came from the same C library through the same loader and disagreed about whether the feature
existed, and nothing in 95 tests could see it because no test loads a real dylib.

### 21.2 Save-state compatibility is a corruption hazard, not a validation nicety

A libretro save state is an opaque dump of a core's internal structs and `retro_unserialize` is not
versioned. Handing a core a state written by a **different build of that same core** does not
reliably fail: it can succeed into a machine whose internals are subtly wrong, surfacing minutes
later as a hang, a corrupted savedata file, or a crash in code with no connection to the load. Cause
and symptom are far enough apart that nobody connects them.

Nothing in the engine can detect this, which is why the checks live in the host where the metadata
is. Four things are recorded beside every state and checked before any load, in this order:

1. the payload file exists,
2. the core id matches `current_core_id()`,
3. the core version matches `core_version()` (this is why `retro_get_system_info` is now read),
4. the byte length matches `save_state_size()` **as the core reports it right now**.

An unknown on either side of a check falls through to the remaining checks rather than refusing,
because a record written by an older build that did not store a version must not become unloadable.
Degrading to a weaker check is correct; loading anyway is not.

### 21.3 Things in the Swift store that look incidental and are not

- **Payload writes are synchronous.** The auto-save fires on `willResignActive`, after which iOS
  gives the app a short and unspecified window before suspending it. Work handed to a task that has
  not run yet is work that may never run: a state written synchronously exists, one dispatched
  asynchronously is a promise.
- **There is no auto-save timer.** `retro_serialize` on a PlayStation state is a megabyte of struct
  copying inside the engine lock, so a periodic save is a periodic hitch. Rewind already covers the
  last few seconds and is built for it, against a memory budget, on the engine's own thread.
- **The index decodes field by field** with `decodeIfPresent`. That decoder runs over the whole
  index, so one field renamed by a later build would wipe every save state on the device rather than
  degrade one record.
- **Storage is Application Support, not Documents.** `UIFileSharingEnabled` has to be on so cue and
  bin tracks can be dropped in, which makes Documents user-visible. An index is a set of claims
  about files, and files anyone can rename underneath it make those claims lies. Unlike artwork,
  neither the save state nor the cheat directory is excluded from backup: a cover is one download, a
  save state is the only thing in this app representing time the user spent.
- **Cheats are pushed whole and in order, including the disabled ones** with their flag set false.
  `retro_cheat_set` is indexed, so omitting a disabled cheat renumbers every cheat after it and the
  core's table stops matching the list on screen.

### 21.4 The other lesson: what each check can and cannot catch

The `Data` versus `[UInt8]` mismatch that failed the first build of this work is a useful calibration
of the verification story:

- `swiftc -frontend -parse` is a **syntax** pass. It cannot see a type mismatch and passed on the
  broken line.
- Verifying that every engine method **exists** in freshly generated bindings, which was done,
  checks names and not argument types.
- **CI remains the only thing in this project that type-checks Swift.** Budget a build for it rather
  than trusting a local pass, and note UniFFI maps a Rust `Vec<u8>` to `Data` on this boundary.


## 22. Starting N64: step 2 done, and the two risks that actually gate it

N64 is **step 6** of the twelve-step sequence in `docs/SET_HW_RENDER_DESIGN.md` §13, not a task
of its own. Step 1 (wgpu owning the `MTLDevice`) was finished long ago. Step 2 is now done. The
order is not bureaucracy: the doc's own reasoning is that steps 1 to 3 involve no emulator core
at all, because they are where the graphics architecture is proven or corrected, and a failure
there is a rectangle in the wrong place rather than a game misbehaving for reasons that could be
anywhere in a hundred thousand lines of core.

### 22.1 What step 2 changed

The composite pass was one fullscreen quad. It is now one instance per screen, each carrying a
source rect and a destination rect, because two things ahead cannot be expressed by a single
quad: the DS and 3DS hand over **one** framebuffer with **two** screens stacked inside it, and a
hardware-rendered core draws into a texture whose shape is not the window's.

`ScreenSplit::Single` is still the only value anything selects, and a test asserts its geometry is
exactly what the one-quad version produced. That test is the regression guard for nine shipping
systems; do not delete it when a second split finally gets used.

Three things in there failed silently by nature and are now pinned by tests:

- **The uniform array is packed into `vec4`s, not `vec2`s.** WGSL requires an array element in
  the uniform address space to be 16-byte aligned. A struct of `vec2`s is 8-byte aligned, so it
  either fails to compile or acquires padding the Rust side then disagrees with, and the symptom
  is garbage geometry with no error from either half.
- **The v flip happens before the source rect is applied.** So a source offset is an ordinary
  top-down texture coordinate, and the top screen of a stacked pair has the **smaller v** while
  also having the **positive** clip-space offset. Those conventions disagreeing is what swaps the
  two screens of a DS, which looks deliberate and is not.
- **Unused array slots hold the identity, not zeroes.** A zeroed slot is a degenerate triangle:
  it draws nothing, which is indistinguishable from a draw that never happened.

### 22.2 The shader is now validated on the build machine

`frame_blit.wgsl` is embedded with `include_str!` and compiled by wgpu at runtime, so **no WGSL
mistake was ever a compile error**: a wrong type, a missing binding or an alignment violation all
built cleanly and then presented a black screen on a phone with no debugger. `naga` is wgpu's own
shader front end and was already in the dependency tree at the same version, so validating the
shader in a unit test costs no extra compilation and moves that failure from a device to `cargo
test`. Two further tests check the entry-point names the pipeline looks up by string, and that the
shader declares as many screens as the engine writes.

That last one closed a real gap: `MAX_SCREENS` is declared once in `renderer.rs` and once in the
shader, and nothing connected them, so raising only the Rust constant would have written
placements past the end of the declared array with both halves still compiling. The guard was
verified by making the two numbers disagree and watching it fail.

**This matters more from here on than it did before.** Every remaining graphics step edits this
shader, and the composite pass is exactly where a mistake is invisible until it is on screen.

### 22.3 RISK ONE: the dynarec has never been switched on

**This is the one that can kill N64 outright, and it has nothing to do with graphics.**

`pcsx_rearmed` currently ships with `DYNAREC=0` (see §17): the interpreter, chosen deliberately to
prove the core path without a JIT dependency to fight. PS1 is playable that way. **An N64
interpreter is not.** Every usable N64 core depends on a recompiler, so N64 needs the dynarec
working on device, which needs the `MAP_JIT` path working on device.

The entitlements are all present and correct in `native/ios/Continuum.entitlements`:
`allow-jit`, `allow-unsigned-executable-memory`, `increased-memory-limit` and
`extended-virtual-addressing`, with a comment explaining why the last two are different jobs. But
**present entitlements are not a working JIT.** On arm64 the sequence is `mmap` with `MAP_JIT`,
then `pthread_jit_write_protect_np` per thread, then `sys_icache_invalidate`, and the last of
those is not optional: omitting it works in the simulator and crashes on device, intermittently.
Nothing in this project has executed that path even once.

**De-risk it before step 3, not at step 6, and de-risk it on a core that already works.** Rebuild
`pcsx_rearmed` with `DYNAREC=1` and see whether PS1 still runs. That is one flag in
`scripts/build-core.sh`, it is testable on a game already verified working, and it answers the
question with the graphics stack entirely uninvolved. If the JIT path is broken, finding out on a
core that runs fine without it is enormously cheaper than finding out underneath a brand new
Vulkan compute renderer where any of five things could be at fault.

### 22.4 RISK TWO: MoltenVK has to get into the bundle

paraLLEl-RDP is a Vulkan **compute** implementation of the N64's RDP and is the only
accurate-and-fast N64 rasteriser that exists. It has no GL equivalent, so the Vulkan path is
mandatory rather than preferred, which means MoltenVK ships inside the `.ipa`: roughly 8 MB, on an
app that is currently 6.2 MB. The alternative core, Mupen64Plus-Next with GLideN64, is GL-only and
would need ANGLE instead at roughly 15 MB. Neither is avoidable and the size should be stated
rather than discovered.

Practical consequence for CI: `ios.yml` currently builds five cores from source and nothing else.
MoltenVK has to be fetched or built as an XCFramework and embedded by `package-ipa.sh`, which is
new machinery in the one part of the build that has no local reproduction. Worth doing as its own
step with no core involved, which is what step 3 already is.

### 22.5 The order from here

1. **Dynarec flag on PS1** (§22.3). Cheap, unrelated to graphics, answers the biggest N64 question.
2. **Step 3**: MoltenVK in-process sharing the one device and queue, rendering a triangle into an
   `MTLTexture` that the compositor samples. No core. This is where the zero-copy handoff is proven
   or corrected.
3. **Step 4**: accept `SET_HW_RENDER` for Vulkan and run **Beetle PSX HW**, the simplest real
   hardware-rendered core, against a system already known to work.
4. **Step 6**: paraLLEl-N64.

Steps 4 and 6 are deliberately separated by a core that is not N64. If the contract is wrong, it
should be wrong on a PlayStation game whose software-rendered version is already verified on this
device, not on the highest-risk core in the sequence.
