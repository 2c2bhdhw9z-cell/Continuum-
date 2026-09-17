# Phase 5 — Wrapping the Rust engine in a native iOS app

Architectural blueprint for taking `crates/emulator-bridge` from a wasm module in a PWA to
a UniFFI-bound static library inside a SwiftUI app, and for what that has to look like
before PS1, N64, PSP, DS, 3DS and Switch cores can run on it.

Written against the audit in `SESSION_HANDOFF.md` §9: the engine already type-checks and
lints clean for `aarch64-apple-ios` and `aarch64-apple-darwin`, 4,896 of 6,308 lines are
platform-neutral, and there are exactly six `cfg` gates. That audit is what makes this a
port rather than a rewrite — but it deliberately proved only that the code *compiles* for
the target. Nothing below has been linked or run.

---

## 1. Why the web build stops here

The browser is not slow at emulation. It is slow at exactly one thing that matters for
this tier: it will not let a process write machine code and then execute it.

| Constraint | Browser | Native iOS |
| --- | --- | --- |
| Recompile guest code to host code | No. wasm pages are never both writable and executable | Yes, with an entitlement |
| Address space | 4 GB hard ceiling on wasm32 | 64-bit, and `mmap` reservations are cheap |
| Threads | `SharedArrayBuffer` + COOP/COEP, no real parallelism guarantees | Real threads, QoS classes |
| GPU | WebGPU, one queue, no compute-shader guarantees on Safari | Metal 3, compute, argument buffers |
| Memory ceiling before the OS kills you | Whatever Safari's JetsamEvent allows a tab | The app's own jetsam budget, larger and measurable |

Every core shipped today (NES, GBA, Master System, SNES) is a pure interpreter and runs
comfortably at 60 fps in wasm. Every core in the heavy tier depends on at least one of the
capabilities in the right-hand column. That is the whole reason for Phase 5, and it is why
the deferral in Phase 4 was correct rather than pessimistic.

---

## 2. Target shape

```text
┌──────────────────────────────────────────────────────────────────────┐
│ SwiftUI app                                                          │
│   LibraryView, PlayerView, SettingsView                              │
│   MetalCanvas (CAMetalLayer-backed UIView)                           │
│   GameController framework → ContinuumEngine.applyGamepad(...)        │
└────────────────────────────┬─────────────────────────────────────────┘
                             │ UniFFI-generated Swift  (§3)
┌────────────────────────────┴─────────────────────────────────────────┐
│ libcontinuum.a          crate-type = ["staticlib", "rlib"]           │
│                                                                       │
│   uniffi.rs        ← NEW: the Swift-facing facade (peer of wasm.rs)   │
│   bridge.rs        ← unchanged. The engine.                           │
│   gfx/renderer.rs  ← unchanged; `from_surface` already exists (§4)    │
│   audio/           ← unchanged trait; NEW CoreAudioSink impl (§5)     │
│   input.rs         ← unchanged. GamepadBridge maps the same layout    │
│   cores/           ← NEW native_core.rs beside wasm_core.rs (§6)      │
└──────────────────────────────────────────────────────────────────────┘
```

The shape of this is already decided by the existing code, and that is the point. `wasm.rs`
is a thin, logic-free facade over `bridge.rs`; `uniffi.rs` is its peer. Anything that ends
up in `uniffi.rs` and is not a type conversion is a signal that logic leaked out of the
engine and should be pushed back down.

### The one Cargo change

```toml
[lib]
crate-type = ["staticlib", "cdylib", "rlib"]
#             ^^^^^^^^^ needed to link into a Swift target
```

`staticlib`, not `cdylib`, for iOS: App Store review rejects unsigned embedded dynamic
libraries, and a static archive lets the linker dead-strip. This was deliberately *not*
added during the Phase 4 audit — `cargo check` does not link, so the edit would have been
unverifiable, and at the time the sandbox could not build wasm32 either, so it risked
breaking the web build with no way to notice before CI. Add it in the same commit that
first links a real binary.

`Cargo.toml` already anticipates the rest: `# Phase 2 will add: uniffi = ["dep:uniffi"]`.

---

## 3. The UniFFI boundary — CRITICAL REQUIREMENT 1

### Why UniFFI rather than swift-bridge or a hand-written C header

| | UniFFI | swift-bridge | hand-rolled `extern "C"` |
| --- | --- | --- | --- |
| Swift enums/structs from Rust | Generated | Generated | Hand-written twice |
| Async | `async fn` → Swift `async` | Manual | Manual |
| Callbacks Swift→Rust | Callback interfaces | Yes | Function pointers by hand |
| Zero-copy byte slices | **No** — `Vec<u8>` is copied | Better | Best |
| Mozilla-maintained, used in Firefox iOS | Yes | Community | — |

The copy is the only real objection, and it is answerable: **nothing on the per-frame path
crosses this boundary.** Frames go core → Rust staging → Metal texture entirely inside
Rust; audio goes core → ring buffer → CoreAudio render callback entirely inside Rust. What
crosses UniFFI is what crosses `wasm.rs` today — launch, pause, save state, settings — a
few dozen calls a minute. For that traffic, generated correctness beats hand-written
performance.

### Interface sketch

```rust
// crates/emulator-bridge/src/uniffi.rs      — peer of wasm.rs, same rule: no logic here
#[derive(uniffi::Object)]
pub struct ContinuumEngine {
    inner: Mutex<EmulatorBridge>,   // Mutex, not RefCell: Swift may call from any actor
}

#[uniffi::export]
impl ContinuumEngine {
    #[uniffi::constructor]
    pub fn new() -> Arc<Self>;

    /// `layer` is a `CAMetalLayer` pointer. See §4 for why this is the shape.
    pub fn attach_surface(&self, layer: u64, width: u32, height: u32) -> Result<(), EngineError>;

    pub fn launch(&self, core_id: String, content_id: String, rom: Vec<u8>, filename: String)
        -> Result<(), EngineError>;

    /// One display-link tick. Returns telemetry as a struct rather than the wasm
    /// build's shared-memory scratch array, because there is no shared memory here.
    pub fn tick(&self, now_millis: f64) -> TickTelemetry;

    pub fn apply_gamepad(&self, port: u32, buttons: Vec<bool>, axes: Vec<f32>);
    pub fn save_state(&self) -> Result<Vec<u8>, EngineError>;
    pub fn apply_cheats(&self, codes: Vec<String>, enabled: Vec<bool>) -> Result<u32, EngineError>;
    pub fn core_options(&self) -> Vec<CoreOption>;      // already a plain struct
    pub fn set_core_option(&self, key: String, value: String) -> Result<(), EngineError>;
}

#[derive(uniffi::Record)]
pub struct TickTelemetry { pub steps: u32, pub dropped: u32, pub presented: bool,
                           pub display_fps: f64, pub frame_count: u64,
                           pub audio_queued_frames: u32 /* … */ }

#[derive(uniffi::Error, thiserror::Error, Debug)]
pub enum EngineError { /* one variant per BridgeError arm; From<BridgeError> */ }
```

Three notes that will save a day each:

- **`Vec<bool>` is fine here.** The wasm build takes `&[u8]` for button states because
  wasm-bindgen has no bool-slice ABI. UniFFI does. Do not copy the byte-array workaround
  into Swift and then wonder why it is there.
- **`CoreOption` needs no new type.** `cores/mod.rs` already defines it as a plain struct of
  `String`/`Vec<String>`, deliberately platform-neutral — add `#[derive(uniffi::Record)]`
  behind the feature flag.
- **`Mutex`, not `RefCell`.** The wasm build gets away with `RefCell` because there is one
  thread. Swift will call `tick` from a `CADisplayLink` and `saveState` from a Task; a
  `RefCell` there is a panic waiting for a scheduling hiccup. The re-entrancy hazard
  documented in `cores/host.rs` — statics because the bridge is already mutably borrowed
  during `retro_run` — becomes a genuine deadlock risk under a `Mutex`, so the native core
  host must keep that same "never re-enter the bridge mid-frame" discipline.

### Build wiring

1. `uniffi-bindgen` generates `continuum.swift` + `continuumFFI.modulemap` at build time.
2. `cargo build --release --target aarch64-apple-ios` (device) and
   `aarch64-apple-ios-sim` (simulator); `lipo`/`xcodebuild -create-xcframework` to bundle.
3. An Xcode "Run Script" phase ahead of Compile Sources, so the generated Swift is present
   before the Swift compiler runs. Check the generated file *out* of source control and
   regenerate — a stale committed binding that silently disagrees with the Rust is the
   single most annoying failure mode in this stack.

---

## 4. Rendering — CRITICAL REQUIREMENT 2

### The good news, measured

`gfx/renderer.rs` has exactly **one** `cfg`-gated function: `from_canvas`. `from_surface`
is already shared, and the Phase 4 audit confirmed wgpu's Metal backend compiles for both
Apple targets, pulling in `objc2-metal`, `objc2-quartz-core`, `raw-window-metal` and
`wgpu-core-deps-apple`. The shader (`frame_blit.wgsl`) is WGSL and needs no change: wgpu
compiles it to MSL. So presentation is:

```rust
let surface = unsafe {
    instance.create_surface_unsafe(
        wgpu::SurfaceTargetUnsafe::from_metal_layer(layer_ptr as *mut _)
    )?
};
Renderer::from_surface(surface, width, height)   // already exists
```

Swift side: a `UIView` subclass whose `layerClass` is `CAMetalLayer`, handing
`Unmanaged.passUnretained(layer).toOpaque()` across UniFFI as a `u64`. Set
`layer.contentsScale = window.screen.nativeScale` or the image is soft on every device
since the 6 Plus, and pin `maximumDrawableCount = 2` to keep latency at one frame rather
than letting Core Animation buffer three.

### What the heavy tier actually needs from the renderer

Today's pipeline is *one* texture upload and *one* fullscreen blit with aspect-fit
letterboxing. That is sufficient for every 2D core and insufficient for the tier below in
three distinct ways.

#### (a) Hardware-rendered cores

PS1, N64, PSP, DS and 3DS cores do not hand you a framebuffer. They ask the frontend for a
GL/Vulkan context and render into it — libretro's `SET_HW_RENDER`, which
`core-runtime.js:_environment` currently **refuses** on purpose ("this project's cores must
be software-rendered"). Native has to accept it, and that is the single largest piece of
work in Phase 5. Three options, in ascending order of effort and quality:

| Path | How | Cost |
| --- | --- | --- |
| Software renderers only | Force `beetle-psx` (software), `mupen64plus` with the `angrylion` RDP plugin, `desmume`, PPSSPP's software rasteriser | Cheapest; loses upscaling entirely and angrylion is brutally CPU-heavy |
| GL → Metal translation | Ship an ANGLE-style GLES→Metal layer, or MoltenVK for Vulkan cores | One large dependency; well-trodden (Dolphin, PPSSPP, DuckStation all did it) |
| Native Metal backends | Use cores' own Metal paths where they exist (PPSSPP), write one where they do not | Best result, most work, per-core |

The pragmatic sequence: **software first for PS1 and N64** to get the tier running and the
memory model proven, then MoltenVK for the cores that speak Vulkan, then per-core Metal.
Whichever path, `Renderer` needs a second input mode — "the core owns a texture, present
it" — beside today's "here are pixels, upload them". That is a new `enum FrameSource
{ Software(FrameView), Texture(wgpu::Texture) }`, and it is a change to the *engine*, not
just the platform layer, which is why it belongs in this document rather than being
discovered later.

#### (b) Dual-screen: DS and 3DS

Two framebuffers per frame, different sizes, and a user-controllable arrangement. This is
not a shader tweak; it changes the renderer's contract from "a frame" to "a screen set".

```rust
pub enum ScreenLayout {
    Stacked { gap: u32 },          // DS default: 256×192 over 256×192
    SideBySide { swapped: bool },  // landscape
    PrimaryOnly(ScreenId),         // one screen, other hidden
    PictureInPicture { inset: ScreenId, corner: Corner, scale: f32 },
}
```

Specifics that bite:

- **Nintendo DS** — 256×192 twin screens. The touch screen is the *bottom* one, so
  `ScreenLayout` must expose, for each screen, the rect it occupies on the drawable, or a
  touch cannot be mapped back into guest coordinates. Make that a method on the layout
  (`fn touch_target(&self, drawable: Size) -> Rect`) rather than something the Swift layer
  re-derives — deriving it twice is how touch drifts one pixel off on one device.
- **Nintendo 3DS** — top screen is 400×240 (800×240 in stereoscopic mode), bottom is
  320×240. Different widths, so a stacked layout cannot assume a common width. Stereoscopy
  is best refused at first: it doubles top-screen bandwidth for something no iPhone can
  display.
- Both need **two texture uploads per frame**, and `WasmCore`'s single `video_staging` Vec
  becomes an array. Size it from the *sum* of both screens' max geometry.
- libretro cores present dual screens as one tall framebuffer plus a core option for
  layout. Honour that where it exists rather than slicing the image ourselves — the core's
  own layout option is what its users expect, and re-implementing it means fighting it.

#### (c) Upscaling and integer scaling at high internal resolution

A PS1 core at 4× internal resolution renders 1280×960 and an N64 core at 4× renders
2560×1920 — on a device whose drawable is maybe 2556×1179. The existing `ScaleMode`
(`aspect` / `integer` / `stretch`) still applies, but "integer" has to mean integer
multiples of the *guest* resolution, not of the upscaled buffer, or it silently becomes
"stretch". Worth fixing while the aspect-fit maths is already open.

---

## 5. Audio

`audio/mod.rs` defines `trait AudioSink` and the resampler in `audio/resample.rs` is pure
Rust and already handles the 48000/65536/44100 core rates. So this is one new
implementation, not a redesign.

```rust
// audio/coreaudio_sink.rs      (native only)
pub struct CoreAudioSink { ring: Arc<SpscRing<i16>>, unit: AudioUnit }
impl AudioSink for CoreAudioSink { fn submit_i16(&mut self, samples: &[i16]) { … } }
```

- **`AVAudioSession` category `.playback`**, `.mixWithOthers` off. Get this wrong and audio
  dies the first time the user takes a call, silently, and only on device.
- The render callback is a **real-time thread**: no allocation, no locks, no logging. The
  existing ring buffer's SPSC discipline is exactly right; keep it and let the callback
  read.
- Buffer at 2× the frame budget (~32 ms at 60 fps). The web build's underrun counters
  (`AUDIO_UNDERRUNS` in telemetry) carry straight over and are the fastest way to see
  whether the display link and audio clock are fighting.
- `FramePacer::plan()` already takes a timestamp rather than reading a clock — the audit
  called this out specifically — so `CADisplayLink.targetTimestamp` drops straight in.

---

## 6. Cores: from wasm modules to dynamic libraries

Today a core is a wasm module instantiated by JS, and `WasmCore` reaches it through a
`LibretroRuntimeHandle`. Natively a core is a `.dylib` inside the app bundle, and the
equivalent is:

```rust
// cores/native_core.rs      — peer of wasm_core.rs, same EmulatorCore impl
pub struct NativeCore { handle: *mut c_void, symbols: RetroSymbols, /* … */ }
```

- `dlopen` the core from the bundle, `dlsym` the ~25 `retro_*` entry points into a struct
  of function pointers — the same set `scripts/build-core.sh` already lists in
  `RETRO_EXPORTS`, which is the authoritative list and already includes
  `retro_cheat_reset`/`retro_cheat_set`.
- **App Store: `dlopen` of a bundled, co-signed framework is allowed; downloading a core at
  runtime is not.** For a sideloaded build it does not matter. For a hypothetical store
  build, cores must ship in the bundle. Static linking with a symbol-prefix per core is the
  alternative and costs binary size.
- The `EmulatorCore` trait needs **no change**. That is the payoff of the abstraction, and
  the reason `set_cheat`/`set_core_option` were added with plain `&str`/`u32`/`bool`
  signatures rather than `JsValue` — they compile for iOS today.
- `CoreRetention::Drop` and the whole memory policy carry over unchanged, and matter *more*
  natively: a PS1 core with 4× upscaling holds hundreds of megabytes, so "one core resident
  at a time" stops being a nicety.

---

## 7. JIT: what the entitlement does and does not buy

This is the reason for the whole phase, so it is worth being exact.

### The mechanism

A dynarec needs a page that is first writable and then executable. On iOS, on an A12 or
later, the way to get that is:

```
com.apple.security.cs.allow-jit = true
→ mmap(..., PROT_READ | PROT_WRITE | PROT_EXEC, MAP_JIT | MAP_PRIVATE | MAP_ANON, ...)
→ pthread_jit_write_protect_np(0)   // make W, per-thread
   … emit code …
→ pthread_jit_write_protect_np(1)   // make X
→ sys_icache_invalidate(ptr, len)   // REQUIRED on arm64, silently wrong without it
```

Non-obvious, in rough order of how much time each will cost you:

1. **`pthread_jit_write_protect_np` is per-thread state.** A recompiler that emits on a
   worker and executes on the main thread must toggle on the right one.
2. **`sys_icache_invalidate` is not optional on arm64.** Omit it and you execute stale
   instruction-cache lines: works in the simulator, crashes on device, intermittently.
3. **W^X means you cannot patch running code in place.** Self-modifying fast paths — which
   several dynarecs use for block linking — need a different strategy on Apple silicon.

### Who can actually ship this

| Distribution | JIT | Notes |
| --- | --- | --- |
| Xcode / free dev cert | Yes | 7-day expiry, re-sign weekly. The realistic path here |
| Apple Developer Program ($99) | Yes | 1-year, up to 100 devices |
| AltStore / SideStore | Yes | Same signing, refreshed over the network |
| TestFlight | Yes | Entitlement is accepted |
| App Store | **Effectively no** | The entitlement is not granted for general apps. Since 2024 the EU-only "retro game emulator" rule permits emulators, but not the JIT entitlement |
| EU alternative marketplaces | Yes | Notarisation, not review |

So the honest framing: **this is a sideload-first product.** That should be stated in the
app, not discovered by a user whose build stops launching after seven days. The web PWA
remains the zero-friction path for the 2D tier — which is exactly why the two share an
engine, and why the PWA is worth having kept working.

### An interpreter fallback is not optional

Every heavy core needs a non-JIT path, because the same binary will run on a device whose
provisioning has lapsed. `beetle-psx` interprets; `mupen64plus` has `cached_interp`; DS and
PSP cores have interpreter modes. Detect at startup — attempt one `MAP_JIT` mapping and
fall back — rather than trusting the entitlement to be honoured.

---

## 8. Memory translation, per system — CRITICAL REQUIREMENT 2

This is where "it compiles for aarch64" stops being the interesting question. Each of these
imposes a different requirement on the engine, and they are not interchangeable.

| System | Guest RAM | Renderer | Memory model the host must accommodate | Tier |
| --- | --- | --- | --- | --- |
| **PS1** | 2 MB + 1 MB VRAM | Software or HW | Flat, no MMU. Fast-mem needs a 32-bit reservation; unaligned loads must trap. GTE is fixed-point, no FP needed | Easiest |
| **N64** | 4/8 MB RDRAM | HW (RDP) | **TLB.** 32-entry, variable page size, per-process. RSP microcode differs per game | Hardest of the classic three |
| **PSP** | 32/64 MB | HW | 32-bit MIPS with a real MMU; VFPU SIMD maps well to NEON. Encrypted EBOOTs need decryption before load | Moderate |
| **DS** | 4 MB | 2D + 3D | **Two CPUs** (ARM9 + ARM7) sharing RAM, plus a cache coherency problem between them. Dual screen | Moderate, awkward |
| **3DS** | 128 MB | HW (PICA200) | 64-bit-ish address space, ASLR, per-process page tables. Very JIT-dependent | Hard |
| **Switch** | 4 GB | HW (Maxwell) | **Needs more RAM than any iPhone will give one app.** Full 64-bit ARM with a real MMU. NVN→Metal shader translation | Not viable |

### The one mechanism they all share: fast-mem

Every dynarec wants guest loads and stores to compile to a single host instruction. The
trick is to reserve a contiguous host region the size of the guest's address space, map the
guest's real RAM into it at the right offsets, leave the rest unmapped, and let the MMU
turn out-of-range accesses into `SIGSEGV` — then handle that signal and interpret the
instruction slowly.

```
reserve 4 GB PROT_NONE      ← cheap: reservation, not commitment
  ├─ mmap RAM   at guest 0x8000_0000
  ├─ mmap VRAM  at guest 0xA000_0000
  └─ everything else unmapped → SIGSEGV → slow path
```

Consequences for this engine specifically:

- **A `SIGSEGV`/`SIGBUS` handler is a process-global resource.** With several cores
  potentially loaded, and Swift/Metal also running, installing one carelessly will
  intercept a crash that belongs to something else. `CoreRetention::Drop` keeping exactly
  one core resident is what makes this tractable — another argument for the existing policy.
- **`mach_vm_allocate` reservations count against the jetsam limit differently from dirty
  pages.** Reserving 4 GB `PROT_NONE` is fine; touching it is not. Budget by *dirty* pages
  and watch `os_proc_available_memory()`, which is the only number that predicts a jetsam
  kill.
- **N64's TLB cannot use fast-mem naively.** Guest virtual addresses are remapped by a
  32-entry TLB that games reprogram at runtime, so a fixed host mapping is wrong. Either
  invalidate compiled blocks on TLB writes, or route TLB-mapped segments through a slow
  path and reserve fast-mem for the direct-mapped ones (KSEG0/KSEG1). This is the single
  biggest reason N64 is harder than PS1 despite being a similar era.
- **DS needs two guest address spaces**, one per CPU, with shared regions aliased into both
  and a cache-coherency story where the ARM9's data cache is not visible to the ARM7.
- **Switch is out.** A 4 GB guest on a device that will kill an app for using 3 GB is not an
  optimisation problem. Keep it listed as unsupported rather than "planned", and say why —
  the honest answer is more useful than a roadmap entry that will not happen.

### `SystemTier`, and why it belongs in the engine

The engine should classify systems rather than leaving the UI to guess, so that one place
decides what a device can attempt:

```rust
pub enum SystemTier {
    Interpreted,          // NES, SNES, GB/GBA, MD/SMS — the web tier, runs anywhere
    JitRecommended,       // PS1, DS         — interpretable, slowly
    JitRequired,          // N64, PSP, 3DS   — unplayable without a dynarec
    Unsupported,          // Switch          — with a reason string
}
```

The PWA already reserves `phase: 2` systems in `web/src/data/systems.js` and refuses to
launch them, with the launch path saying why. That mechanism generalises directly: the same
field becomes a tier, and the native build enables what the device and its entitlement can
actually deliver.

---

## 9. What carries over untouched

Worth stating plainly, because it is the return on the architecture:

- `bridge.rs` — the entire engine: session lifecycle, pacing, retention policy, cheats,
  core options.
- `input.rs` — `GamepadBridge`, the standard-gamepad mapping, deadzones, stick-to-D-pad
  synthesis, per-source merge. Apple's `GameController` reports the same layout, so
  `applyGamepad` is called with the same numbers a browser produced. This was the reason
  the mapping was put in Rust rather than JS in Phase 1b.
- `audio/resample.rs`, `frame.rs`, `timing.rs` (`FramePacer::plan()` takes a timestamp),
  `cores/mod.rs`, `cores/registry.rs`, `cores/diagnostic.rs`, `error.rs`.
- All 70 unit tests. None is wasm-gated, so they run in the same configuration iOS uses —
  which is what makes the port verifiable at each step rather than at the end.
- `styles/tokens.css` → a Swift `Theme` struct. The four themes added in this phase are
  each about twenty semantic colour overrides, which is deliberately the shape a Swift
  struct wants.

## 10. Sequence

Ordered so that each step is verifiable before the next depends on it.

1. **Link something.** Add `staticlib`, `uniffi.rs` behind the feature flag, generate
   bindings, and get a SwiftUI app that constructs `ContinuumEngine` and prints the core
   manifest. No rendering. This proves the boundary.
2. **Present a frame.** `CAMetalLayer` → `from_surface`, then run the existing
   `DiagnosticCore` and see its pattern. Proves wgpu-on-Metal end to end with no core
   involved.
3. **One real core, statically linked.** fceumm, the same NES test cart the web suite uses,
   and the same assertions — the cart's idle colour is green and turns red when A is held,
   which is exactly as checkable in Swift as in Playwright.
4. **Audio and input.** `CoreAudioSink`, `GameController`. At this point the native app
   equals the PWA in capability.
5. **`dlopen` and the ABI test.** Port `scripts/core-abi-test.mjs`'s assertions to a native
   harness — it needs no GPU, so it is the cheapest possible regression net for step 6.
6. **PS1, software-rendered, interpreted.** No JIT, no `SET_HW_RENDER`. Proves the content
   pipeline for disc images (which is also the first time `need_fullpath` matters, and the
   PWA's content-override plumbing already documents that trap).
7. **JIT.** Entitlement, `MAP_JIT`, `pthread_jit_write_protect_np`, icache invalidation, and
   the interpreter fallback. Measure PS1 before and after.
8. **`SET_HW_RENDER`.** The large one. Unblocks N64 and PSP properly.
9. **Dual screen.** `ScreenLayout` plus touch mapping. Unblocks DS, then 3DS.

Steps 1–5 are a port of proven code. Steps 6–9 are new engineering, and step 8 is the one
that deserves its own design document before anyone starts.
