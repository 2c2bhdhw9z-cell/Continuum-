# Continuum — Session Handoff

State of the project at tag `v0.6.0-library`, written to be the only document a new session
needs to read before changing anything.

Continuum is an all-in-one emulator PWA. Four real libretro cores run as standalone
WebAssembly modules — three C, one C++; a Rust engine owns pacing, input, audio and
presentation; the front end is a strictly virtualised Netflix-style library. The same
Rust crate is intended to compile for native ARM64 later, which is why so much logic
that could have lived in JavaScript does not.

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
cargo test                           # 70 unit tests
cargo fmt --all --check
cargo clippy --all-targets           # zero warnings
node scripts/core-abi-test.mjs       # 64 checks, 4 cores, no browser
node scripts/capture-frames.mjs      # docs/frame-{nes,gba,sms,snes}[-a].png

# Browser suite (95 checks). One shell invocation: /tmp and background jobs
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
