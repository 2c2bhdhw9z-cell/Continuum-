# `SET_HW_RENDER` — driving Metal from hardware-rendered cores

Design document for the largest single piece of Phase 5: accepting
`RETRO_ENVIRONMENT_SET_HW_RENDER` so that PS1, N64, PSP, DS, 3DS and Switch cores can render
on the GPU instead of into a CPU framebuffer.

Assumptions fixed by the project owner and not re-litigated here:

- **Distribution is sideloading with custom signing.** App Store guidelines are out of scope.
- **JIT is unconditional.** `com.apple.security.cs.allow-jit` is always present; there is no
  interpreter fallback path and no runtime capability probe for it.
- **Memory is provisioned with `com.apple.developer.kernel.increased-memory-limit` and
  `com.apple.developer.kernel.extended-virtual-addressing`**, targeting 12 GB-class hardware
  with a 6–8 GB working budget. Switch is in scope. §11 corrects the arithmetic in the
  earlier blueprint, which reasoned from default jetsam caps.

Companion to [`NATIVE_IOS_BLUEPRINT.md`](NATIVE_IOS_BLUEPRINT.md), which covers the UniFFI
boundary, CoreAudio, `dlopen` and the port sequence. This document is only the graphics path.

---

## 1. The contract, exactly

Quoted from the `libretro.h` vendored under `.work/` by the core builds, so these are the
values our cores actually compile against rather than remembered ones.

```c
#define RETRO_ENVIRONMENT_SET_HW_RENDER 14
#define RETRO_ENVIRONMENT_GET_HW_RENDER_INTERFACE                     (41 | EXPERIMENTAL)
#define RETRO_ENVIRONMENT_SET_HW_RENDER_CONTEXT_NEGOTIATION_INTERFACE (43 | EXPERIMENTAL)
#define RETRO_ENVIRONMENT_GET_PREFERRED_HW_RENDER 56

#define RETRO_HW_FRAME_BUFFER_VALID ((void*)-1)

struct retro_hw_render_callback {
   enum retro_hw_context_type context_type;
   retro_hw_context_reset_t   context_reset;            /* frontend → core */
   retro_hw_get_current_framebuffer_t get_current_framebuffer;  /* core → frontend */
   retro_hw_get_proc_address_t get_proc_address;         /* core → frontend */
   bool depth;                  /* deprecated, always honour it anyway */
   bool stencil;                /* deprecated */
   bool bottom_left_origin;     /* GL origin convention — see §8 */
   unsigned version_major;
   unsigned version_minor;
   bool cache_context;          /* see §10 */
   retro_hw_context_reset_t context_destroy;
   bool debug_context;
};

enum retro_hw_context_type {
   NONE = 0, OPENGL = 1, OPENGLES2 = 2, OPENGL_CORE = 3,
   OPENGLES3 = 4, OPENGLES_VERSION = 5, VULKAN = 6,
   D3D11 = 7, D3D10 = 8, D3D12 = 9, D3D9 = 10
};
```

Four obligations fall out of that struct, and all four are new work:

1. **Provide a render target.** `get_current_framebuffer()` is called by the core, usually
   once per frame, and must return a GL framebuffer object name the core can bind. Vulkan
   cores do not use it; they go through `GET_HW_RENDER_INTERFACE` instead.
2. **Resolve symbols.** `get_proc_address("glTexImage2D")` and friends. For a GL core this is
   `eglGetProcAddress` via ANGLE; for Vulkan it is `vkGetInstanceProcAddr` via MoltenVK.
3. **Announce context lifetime.** Call `context_reset` after the context exists and after any
   loss; call `context_destroy` before tearing it down. Cores allocate all their GPU
   resources inside `context_reset`.
4. **Recognise the sentinel.** After `retro_run`, a hardware core calls
   `video_refresh(RETRO_HW_FRAME_BUFFER_VALID, width, height, 0)`. That pointer is
   `(void*)-1`, i.e. `usize::MAX`. `NULL` still means "duplicate the previous frame". Our
   current `video_refresh` handler treats any non-null pointer as pixels and would read
   from address `0xFFFF…FFFF` on the first hardware frame.

The web build refuses command 14 outright, with a comment explaining that a browser cannot
give a core a GL context (`core-runtime.js`, `case ENV.SET_HW_RENDER`). That refusal stays —
it is correct for the PWA and is what keeps the two builds honest about their capabilities.

---

## 2. Why this is not simply "add a backend"

Our renderer is wgpu-on-Metal. A hardware core wants GL or Vulkan. Naïvely that is three
graphics APIs in one process, and the interesting question is not "can we translate" — there
are mature translation layers for both — but **who owns the `MTLDevice`, and who owns the
texture the core draws into.**

Get that wrong and every frame costs a GPU→CPU→GPU round trip, which at 4× internal
resolution on N64 is 2560×1920×4 bytes = 19.6 MB per frame, 1.2 GB/s at 60 fps, and the
feature is dead on arrival.

Get it right and there is **no copy at all**: the core renders into an `MTLTexture` that our
compositor samples directly.

### The single fact that makes it tractable

An iPhone has exactly one GPU. Metal resources are tied to the `MTLDevice` that created
them — but if every layer in the process shares one device rather than creating its own,
there is only one, and every texture is shared by construction.

So the architectural rule, from which everything else follows:

> **There is exactly one `MTLDevice` and one `MTLCommandQueue` in the process, shared by
> wgpu, MoltenVK and ANGLE. Only one component ever creates them.**

> **Correction, from building it (step 1).** This section originally said *Swift* creates the
> device and injects it. That is not implementable on `wgpu` 30, and it would have failed
> silently rather than loudly:
>
> - `wgpu-hal`'s Metal backend has no public constructor accepting an existing `MTLDevice`.
>   `AdapterShared::expose` is private (`src/metal/mod.rs:428`), so
>   `Instance::create_adapter_from_hal` is unreachable with a foreign device. Only the queue
>   half is public (`Queue::queue_from_raw`).
> - `Surface::configure` calls `CAMetalLayer::setDevice` with wgpu's *own* device
>   (`src/metal/surface.rs:276`). An injected device is therefore replaced one call later,
>   leaving Swift holding a device that owns nothing the layer draws — and the first attempt
>   to share a texture between core and compositor would fail with no obvious cause.
>
> So the arrow is reversed: **the engine creates the device, and Swift adopts it** via
> `metalDeviceHandle()`. The invariant above is unchanged — one device, shared — and on iOS
> the device is the same one either way, because `wgpu-hal` enumerates through
> `objc2_metal::MTLCopyAllDevices`, which on iOS is a shim over
> `MTLCreateSystemDefaultDevice()` (`objc2-metal-0.3.2/src/device.rs`). See
> `crates/emulator-bridge/src/gfx/metal.rs`.

---

## 3. Architecture

```text
┌─ Swift ──────────────────────────────────────────────────────────────────────┐
│  layerClass = CAMetalLayer            ← Swift owns the layer, not the device  │
│                                                                              │
│  engine.attachMetal(layer: ptr(metalLayer), width: w, height: h)             │
│  let device = engine.metalDeviceHandle()   ← reads the one device back out    │
│  let queue  = engine.metalQueueHandle()                                      │
└───────────────────────────────┬──────────────────────────────────────────────┘
                                │ UniFFI: opaque u64 handles (§7)
┌───────────────────────────────┴──────────────────────────────────────────────┐
│ libcontinuum.a                                                               │
│                                                                              │
│  gfx/metal.rs           creates the MTLDevice; hands it back to Swift         │
│  gfx/renderer.rs        wgpu device + surface on the CAMetalLayer             │
│      └── composite pass: N source rects → N dest rects (§9)                   │
│                                                                              │
│  gfx/hw/mod.rs          trait HwContext  { begin_frame, end_frame, … }        │
│      ├── hw/vulkan.rs   MoltenVK.  VkImage ⇄ MTLTexture via VK_EXT_metal_objects│
│      └── hw/gles.rs     ANGLE.     MTLTexture → EGL pbuffer → FBO             │
│                                                                              │
│  cores/native_core.rs   recognises RETRO_HW_FRAME_BUFFER_VALID                │
└──────────────────────────────────────────────────────────────────────────────┘
                                │
                    both translation layers submit to
                    the *same* MTLCommandQueue (§6)
```

`HwContext` is the new seam. It is deliberately shaped like `AudioSink` — a trait with two
implementations chosen at runtime, where the engine holds a `Box<dyn HwContext>` and knows
nothing about which one it has.

```rust
// gfx/hw/mod.rs — platform-neutral in signature, native-only in implementation
pub trait HwContext {
    /// libretro context type this implementation satisfies.
    fn context_type(&self) -> HwContextType;

    /// Acquire this frame's render target. Double-buffered: the compositor may still be
    /// sampling the previous one.
    fn begin_frame(&mut self, width: u32, height: u32) -> Result<HwTarget, GfxError>;

    /// Called after `retro_run` returns. Inserts whatever synchronisation the
    /// translation layer needs before our compositor samples the texture.
    fn end_frame(&mut self) -> Result<(), GfxError>;

    /// GL framebuffer object name, for `get_current_framebuffer`. Vulkan returns 0.
    fn current_framebuffer(&self) -> u64;

    /// Symbol lookup for `get_proc_address`.
    fn proc_address(&self, symbol: &str) -> *const c_void;

    /// Dropped and rebuilt on context loss (§10).
    fn recreate(&mut self) -> Result<(), GfxError>;
}

pub struct HwTarget {
    /// The `MTLTexture` the core will render into, already imported into wgpu.
    pub texture: wgpu::Texture,
    pub width: u32,
    pub height: u32,
    /// GL cores render with the origin at bottom-left; the compositor flips V.
    pub bottom_left_origin: bool,
}
```

And the engine's frame source generalises, which is the one change that reaches `bridge.rs`:

```rust
pub enum FrameSource<'a> {
    /// Today's path: the core handed us pixels, upload them.
    Software(FrameView<'a>),
    /// The core rendered into a texture we own. Nothing to upload.
    Texture { texture: &'a wgpu::Texture, bottom_left_origin: bool },
    /// The core duped the frame. Re-present whatever is already there.
    Duped,
}
```

`Duped` is called out explicitly because the software path currently signals it with
`Option::None` from `host::end_frame`, and once there are two producers an absent frame and a
duplicated frame need to be distinguishable from "the hardware path failed".

---

## 4. Translation layers: which, and why both

| | MoltenVK | ANGLE |
| --- | --- | --- |
| Translates | Vulkan 1.2+ → Metal | GLES 2.0/3.x → Metal |
| Maintained by | Khronos (Brenwill) | Google |
| Shipping precedent on Apple | Dolphin, DuckStation, PPSSPP, RPCS3 | Chrome/Safari-adjacent, Unity |
| Texture interop | `VK_EXT_metal_objects` (import *and* export) | `EGL_ANGLE_metal_texture_client_buffer`, `EGL_ANGLE_device_metal` |
| Shader path | SPIR-V → SPIRV-Cross → MSL | GLSL ES → ANGLE translator → MSL |

Both are needed, because the cores are split:

| Core | Preferred context | Layer |
| --- | --- | --- |
| Beetle PSX HW | Vulkan | MoltenVK |
| SwanStation / DuckStation | Vulkan (GL fallback) | MoltenVK |
| paraLLEl-N64 (paraLLEl-RDP) | **Vulkan compute** | MoltenVK |
| Mupen64Plus-Next (GLideN64) | GLES3 / GL core | ANGLE |
| PPSSPP | Vulkan (GL fallback) | MoltenVK |
| melonDS | GL 3.1+ | ANGLE |
| Citra / Lime3DS / Azahar | GLES3 | ANGLE |
| Switch (non-libretro, §12) | Vulkan or native Metal | MoltenVK or direct |

**Vulkan is the primary path** and ANGLE is the secondary, for three reasons. paraLLEl-RDP is
a Vulkan *compute* implementation of the N64's RDP and is the only accurate-and-fast N64
rasteriser in existence — it has no GL equivalent, so a Vulkan path is mandatory rather than
preferential. `VK_EXT_metal_objects` gives genuinely bidirectional texture sharing, where
ANGLE's is import-only in practice. And `GET_PREFERRED_HW_RENDER` (command 56) lets us tell a
dual-backend core such as PPSSPP or DuckStation to pick Vulkan, which collapses those cores
onto the better-tested path.

ANGLE exists for the three cores that are GL-only: melonDS, Citra, and GLideN64.

Both ship as XCFrameworks inside the bundle. That is roughly 8 MB for MoltenVK and 15 MB for
ANGLE — worth stating, because it doubles the app's binary size and there is no way around it.

---

## 5. Zero-copy handoff into wgpu

This is the mechanism the whole design rests on, so it is worth spelling out in both
directions.

### Vulkan / MoltenVK

MoltenVK backs every `VkImage` with an `MTLTexture`. `VK_EXT_metal_objects` exposes it:

```rust
// after creating the VkImage that the core's swapchain-equivalent renders into
let mut metal_texture_info = VkExportMetalTextureInfoEXT {
    sType: VK_STRUCTURE_TYPE_EXPORT_METAL_TEXTURE_INFO_EXT,
    image: vk_image,
    plane: VK_IMAGE_ASPECT_COLOR_BIT,
    mtlTexture: ptr::null_mut(),   // filled in by the call
    ..default()
};
vkExportMetalObjectsEXT(device, &mut objects_info);   // → metal_texture_info.mtlTexture
```

The reverse — importing *our* `MTLTexture` into Vulkan via `VkImportMetalTextureInfoEXT` — is
also available and is the direction to prefer where the core lets us choose, because then the
frontend allocates and owns the texture and its lifetime is not entangled with the core's
Vulkan objects.

### GLES / ANGLE

ANGLE is handed our `MTLDevice` at display creation and our `MTLTexture` as a client buffer:

```c
EGLAttrib device_attribs[] = { EGL_METAL_DEVICE_ANGLE, (EGLAttrib)mtl_device, EGL_NONE };
EGLDeviceEXT egl_device  = eglCreateDeviceANGLE(EGL_METAL_DEVICE_ANGLE, mtl_device, NULL);
EGLDisplay   display     = eglGetPlatformDisplay(EGL_PLATFORM_DEVICE_EXT, egl_device, ...);

/* our MTLTexture, presented to GL as a pbuffer we can attach to an FBO */
EGLSurface   surface = eglCreatePbufferFromClientBuffer(
    display, EGL_METAL_TEXTURE_ANGLE, mtl_texture, config, NULL);
```

Then a framebuffer object wrapping that surface's colour attachment is what
`get_current_framebuffer()` returns. Note that the frontend must also attach a depth/stencil
buffer: `depth` and `stencil` in the callback struct are marked deprecated in `libretro.h`,
but cores still render depth-tested geometry and simply assume a depth attachment exists.
melonDS and GLideN64 both do.

### Into wgpu

```rust
let hal_texture = <wgpu_hal::api::Metal as wgpu_hal::Api>::Device::texture_from_raw(
    metal_texture,                        // metal::Texture
    wgpu::TextureFormat::Bgra8Unorm,
    metal::MTLTextureType::D2,
    1, 1,                                 // array layers, mip levels
    wgpu_hal::CopyExtent { width, height, depth: 1 },
);
let texture = unsafe {
    device.create_texture_from_hal::<wgpu_hal::api::Metal>(hal_texture, &descriptor)
};
```

Two consequences worth flagging before anyone starts:

- **This adds `wgpu-hal` and `metal` as direct dependencies** of `emulator-bridge`, native
  targets only, and introduces the first genuinely `unsafe` block in the graphics path. Both
  belong behind a `hw-render` feature so the web build's dependency graph is untouched.
- ~~**Building a wgpu `Device` *from* an injected `MTLDevice` is the part to prototype
  first.**~~ **Answered by step 1: it is not expressible through the public API.** The
  constructor is private and `configure` overwrites the layer's device anyway (§2). Neither
  contingency named here was needed, though: the composite pass did *not* have to be
  rewritten in raw Metal and `wgpu-hal` did *not* have to be patched, because reversing the
  direction — engine creates, Swift adopts — satisfies the same one-device invariant using
  only public API. The prototype-first instinct was right; the predicted failure mode was
  wrong.
- Consequently `wgpu-hal` is **not** a direct dependency. `wgpu` re-exports it as
  `wgpu::hal` (gated on `wgpu_core`), which is enough to read the backend objects back out
  through `Device::as_hal`, and no `metal`/`objc2` crate is needed directly either — the
  pointer casts infer their types. The only new dependency is `pollster`, to drive
  `request_adapter`/`request_device` to completion from a synchronous `attach_metal`.

---

## 6. Synchronisation

Three parties now touch the same texture: the translation layer writes it, our compositor
reads it, Core Animation presents the result.

**Share the command queue.** If MoltenVK, ANGLE and wgpu all submit to the one
`MTLCommandQueue` — the one wgpu created and `metal_queue_handle()` hands out (§2) — Metal's
own in-order guarantee per queue does the work and no explicit fence is needed between the
core's render and our composite.

Where a layer insists on its own queue — MoltenVK can be configured either way — the fallback
is an `MTLSharedEvent`: the core's submission signals value *n*, our composite pass waits on
*n* before sampling. That is one `encodeWaitForEvent` per frame, cheap, and is what
`HwContext::end_frame` exists to place.

Then, at the frame level:

- **Double-buffer the target.** Two textures, alternating. Without this the compositor can be
  sampling frame *n* while the core is already writing frame *n+1*, which on a tiler
  manifests as tearing inside a single presented frame rather than the clean tear you would
  expect.
- **`maximumDrawableCount = 2`** on the `CAMetalLayer`. The default of 3 buys throughput we do
  not need and costs a frame of latency, which for an emulator is the wrong trade.
- **Never block the display link on GPU completion.** `FramePacer::plan()` already takes a
  timestamp rather than reading a clock, so `CADisplayLink.targetTimestamp` feeds it directly
  and the pacer stays the single authority on when a frame is due.

---

## 7. Engine and UniFFI changes

### Frame lifecycle, before and after

```text
software (today)                    hardware (new)
────────────────────────            ──────────────────────────────────────────
host::begin_frame(staging)          hw.begin_frame(w, h) → HwTarget
core.retro_run()                    core.retro_run()
  └ video_refresh(pixels)             ├ get_current_framebuffer() → FBO
      └ memcpy → staging              └ video_refresh(FRAME_BUFFER_VALID, w, h, 0)
host::end_frame() → FrameView       hw.end_frame() → fence
renderer.present(Software(view))    renderer.present(Texture{ .. })
```

The `retro_run` call is unchanged, and so is everything above `present`. That is the whole
point of having had `FrameView` behind an enum-shaped seam.

### UniFFI surface

Only one new call, plus one changed one:

```rust
#[uniffi::export]
impl ContinuumEngine {
    /// Replaces `attach_surface`. Swift passes only its CAMetalLayer: the device and queue
    /// are *created here* and read back by Swift, for the reasons in §2.
    pub fn attach_metal(&self, layer: u64, width: u32, height: u32)
                        -> Result<(), EngineError>;

    /// The one MTLDevice / MTLCommandQueue, as `id<...>` addresses. Zero before
    /// `attach_metal` succeeds, and a caller must treat zero as a failure.
    pub fn metal_device_handle(&self) -> u64;
    pub fn metal_queue_handle(&self) -> u64;

    /// The screen arrangement for dual-screen systems (§9). Also drives touch mapping.
    pub fn set_screen_layout(&self, layout: ScreenLayout) -> Result<(), EngineError>;

    /// Guest-space coordinates for a touch at a point on the drawable, or None if the
    /// touch landed outside every screen rect.
    pub fn map_touch(&self, x: f32, y: f32) -> Option<GuestTouch>;
}
```

`u64` rather than a typed pointer because UniFFI has no pointer type, which is fine: these
are opaque handles that cross once at startup. Swift side is
`UInt(bitPattern: Unmanaged.passUnretained(device).toOpaque())`.

Note what does **not** change: `tick`, `launch`, `apply_gamepad`, `save_state`,
`apply_cheats`, `core_options`. Nothing on the per-frame data path crosses UniFFI, which is
why the copying that UniFFI does is still irrelevant.

---

## 8. Origin, format and colour

Three small things that each cost an afternoon if discovered late.

**`bottom_left_origin`.** GL renders with the origin at bottom-left, Metal at top-left. A GL
core sets this flag and the image arrives vertically flipped. Handle it as a per-instance
uniform in the composite shader — a sign flip on the V coordinate — not by blitting through
an intermediate texture, and certainly not by asking the core to flip.

**Format.** Cores overwhelmingly render `RGBA8`/`BGRA8`. `CAMetalLayer` wants
`.bgra8Unorm`. Matching them avoids a conversion pass; where a core insists on RGBA, the
swizzle is free in the sampler.

**Colour space.** `CAMetalLayer.colorspace` defaults to the display's. Emulated output is
sRGB-ish at best and these consoles predate colour management entirely, so pin the layer to
sRGB and do not opt into extended dynamic range. Left alone, the same ROM looks different on
an XDR display than on an LCD, and the P3 rendering of a saturated NES red is not what the
hardware produced.

---

## 9. Dual screen: DS and 3DS

This is the second structural change, and it is independent of `SET_HW_RENDER` — it would be
needed even for a software DS core.

### What the cores actually emit

Both melonDS and Citra emit **one framebuffer containing both screens**, with the arrangement
selected by a core option. They do not offer two separate `video_refresh` calls.

| System | Screen | Native | Combined framebuffer |
| --- | --- | --- | --- |
| DS | top / bottom, both 256×192 | equal | 256×384 stacked, or 512×192 side-by-side |
| 3DS | top 400×240 (800×240 stereo), bottom 320×240 | **unequal widths** | 400×480 with the bottom screen letterboxed |

The 3DS asymmetry is the awkward one: a stacked layout cannot assume a common width, so the
bottom screen sits inside a 400-wide band with 40 px of padding either side. Slicing needs
the exact rects, not a "split it in half" assumption.

### The design: pin the core's layout, slice it ourselves

We could let the core lay the screens out. We should not, because the arrangement wants to be
interactive — drag, resize, picture-in-picture — and a core option is set once per session.
So:

1. **Pin the core's layout option to a known value** at launch, using the core-options
   machinery already shipped in the previous phase. For melonDS that is
   `melonds_screen_layout = "Top/Bottom"`; for Citra, `citra_layout_option = "Default Top-Bottom Screen"`.
   This is exactly why core options landed before this work: without the ability to *set* an
   option, the framebuffer arrangement would be whatever the core last remembered, and the
   slicing maths would be guesswork.
2. **Describe the framebuffer** with a per-system descriptor, so the split is data rather
   than branching:

```rust
pub struct ScreenGeometry {
    /// Source rects within the core's framebuffer, in core pixels.
    pub screens: Vec<ScreenRect>,
}

pub struct ScreenRect {
    pub id: ScreenId,          // Top | Bottom
    pub source: Rect,          // where it lives in the core's framebuffer
    pub touch: bool,           // DS: the bottom screen. 3DS: the bottom screen.
}
```

3. **Arrange them** with the layout the user chose:

```rust
pub enum ScreenLayout {
    Stacked { gap: u32 },
    SideBySide { swapped: bool },
    PrimaryOnly(ScreenId),
    PictureInPicture { inset: ScreenId, corner: Corner, scale: f32 },
}
```

### The composite pass

Today's composite is one fullscreen textured quad. It becomes an **instanced quad, one
instance per screen**, with per-instance source rect, destination rect and V-flip. That is a
vertex-buffer change and about ten lines of WGSL, and it subsumes the single-screen case as
`screens.len() == 1`. Picture-in-picture then costs nothing extra beyond ordering the
instances back-to-front.

Integer scaling needs care here: with two screens at different scales the layout is no longer
uniform, so `ScaleMode::Integer` must compute one shared multiplier from the *most
constrained* screen and apply it to both, or the two halves end up at different pixel sizes
and the seam is obvious.

### Touch mapping

The single most important consequence, and the reason the layout lives in Rust rather than
Swift: a touch has to be inverted back through the destination→source transform to reach
guest coordinates.

```rust
pub fn map_touch(&self, x: f32, y: f32) -> Option<GuestTouch> {
    let layout = self.screen_layout.resolve(self.drawable_size, &self.geometry);
    let screen = layout.screens.iter().find(|s| s.touch && s.dest.contains(x, y))?;
    let u = (x - screen.dest.x) / screen.dest.w;
    let v = (y - screen.dest.y) / screen.dest.h;
    Some(GuestTouch {
        x: (u * screen.source.w) as u16,
        y: (v * screen.source.h) as u16,
    })
}
```

Deriving this in Swift instead would mean the same arithmetic in two languages, kept in step
by hand — which is precisely how a touch ends up one pixel off on one device and nowhere
else. It is also the same reasoning that put the gamepad mapping in Rust in Phase 1b, and
that mapping is now reused unchanged by `GameController`.

Because the pointer device already exists in libretro (`RETRO_DEVICE_POINTER`), the input
path needs no new plumbing: `GamepadBridge` gains a pointer port and the core queries it
through the existing `input_state` callback.

---

## 10. Context loss and backgrounding

`cache_context` tells the frontend whether the core can survive losing its context. Most
cores set it `false`, meaning: on loss, call `context_destroy`, rebuild, then `context_reset`,
and the core reallocates everything.

On iOS the `MTLDevice` survives backgrounding but drawables do not, and the app may be asked
to release GPU resources under pressure. The sequence to implement:

```text
willResignActive   → pause the session; hw.end_frame() completes in flight work
didEnterBackground → core.context_destroy(); drop drawables; keep the MTLDevice
willEnterForeground→ hw.recreate(); core.context_reset()
didBecomeActive    → resume
```

Two traps:

- **`context_reset` must not be called while a `retro_run` is in flight.** The existing
  discipline in `cores/host.rs` — statics rather than re-entering the bridge, because the
  bridge is already mutably borrowed during a frame — applies here for the same reason, and
  under a `Mutex` instead of a `RefCell` the consequence is a deadlock rather than a panic.
- **Save state before `context_destroy`, not after.** Several cores keep emulated GPU state
  in host GPU resources; once the context is gone, that state is unrecoverable and a state
  written afterwards is subtly incomplete. The auto-save on `visibilitychange` that the PWA
  already does maps onto `willResignActive`, and it needs to run *first*.

---

## 11. Memory: corrected arithmetic

The earlier blueprint reasoned from default jetsam caps and concluded Switch was not viable.
That was wrong, because it ignored the entitlements.

The two do different jobs, and both are needed:

| Entitlement | What it raises | Why this project needs it |
| --- | --- | --- |
| `com.apple.developer.kernel.increased-memory-limit` | The **jetsam** cap — resident, dirty pages | Switch guest RAM (4 GB) plus GPU resources plus recompiled code caches |
| `com.apple.developer.kernel.extended-virtual-addressing` | The **virtual** address space, beyond 4 GB | The sparse fast-mem reservation every dynarec wants — a reservation is address space, not resident memory |

The distinction matters and is easy to conflate: `extended-virtual-addressing` costs nothing
at runtime and does not affect jetsam risk, because a `PROT_NONE` reservation has no resident
pages. It is what allows a 512 GB sparse guest address space to be reserved so that guest
loads compile to one host instruction. `increased-memory-limit` is what stops the app being
killed for the pages it actually touches.

Revised budget on 12 GB hardware, 6–8 GB working target:

| System | Guest RAM | GPU resources at 4× | Code cache | Realistic total |
| --- | --- | --- | --- | --- |
| PS1 | 2 MB + 1 MB VRAM | ~120 MB | ~32 MB | < 300 MB |
| N64 | 8 MB | ~400 MB (paraLLEl-RDP buffers) | ~64 MB | < 700 MB |
| PSP | 64 MB | ~300 MB | ~96 MB | < 600 MB |
| DS | 4 MB | ~80 MB | ~48 MB (two CPUs) | < 300 MB |
| 3DS | 128 MB | ~500 MB | ~128 MB | ~1 GB |
| **Switch** | **4 GB** | **1–2 GB** | **~512 MB** | **6–7 GB** |

Switch fits, with the entitlements, on 12 GB hardware, and only there. On an 8 GB device it
will be tight to impossible — so `SystemTier` should gate on `os_proc_available_memory()` at
launch rather than on a device allowlist, and report honestly when a title will not fit.

`CoreRetention::Drop` stops being a tidiness policy and becomes load-bearing: two resident
cores at these sizes is an immediate kill.

---

## 12. Switch: a custom C++ libretro wrapper around a standalone ARM64 engine

Existing Switch engines are standalone applications, not plugins. We bridge that with a
purpose-built C++ translation unit — `continuum_switch_libretro.cpp` — that implements the
`libretro.h` contract on the outside and drives the engine's native execution loop on the
inside. Once it exports the standard `retro_*` symbols it is, as far as the rest of this
system is concerned, a core: `NativeCore` `dlopen`s it, `CoreRegistry` holds it,
`EmulatorCore` abstracts it, and `bridge.rs` never learns that anything unusual happened.

```text
┌────────────── libcontinuum.a (Rust) ──────────────┐
│  NativeCore  →  dlopen("switch_libretro.dylib")   │
└───────────────────────┬───────────────────────────┘
                        │  the 25 retro_* entry points
┌───────────────────────┴───────────────────────────────────────────────┐
│ continuum_switch_libretro.cpp        ← the wrapper                     │
│                                                                        │
│   FrameGate         inversion of control            (§12.1)            │
│   EntryPoints       retro_run / retro_load_game     (§12.2, §12.3)     │
│   ThreadAffinity    callback thread discipline      (§12.4)            │
│   VulkanBridge      negotiation + set_image         (§12.5)            │
│   CapabilityShim    MoltenVK feature mapping        (§12.6)            │
│                                                                        │
│   class ISwitchEngine  ← the only thing the wrapper knows about the    │
│                          engine. One virtual interface, ~12 methods.   │
└───────────────────────┬───────────────────────────────────────────────┘
                        │
┌───────────────────────┴───────────────────────────────────────────────┐
│ Standalone ARM64 Switch engine                                         │
│   ARM64 JIT · NVN→SPIR-V shader translation · Vulkan renderer          │
│   own CPU threads · own GPU command processor thread · own audio thread │
└────────────────────────────────────────────────────────────────────────┘
```

`ISwitchEngine` is the seam that keeps this maintainable. The wrapper is written against an
abstract interface, not against a particular codebase's internals, so the engine underneath
can be replaced or updated without the libretro surface changing:

```cpp
class ISwitchEngine {
public:
  virtual ~ISwitchEngine() = default;

  virtual bool Initialise(const EngineConfig&) = 0;
  virtual bool MountContent(const char* path) = 0;      // NSP / XCI / NCA / NRO
  virtual bool BootTitle() = 0;
  virtual void Shutdown() = 0;

  /// Runs CPU and GPU until the engine reaches its presentation point.
  /// Blocks. Called only from the engine's own driver thread.
  virtual void RunUntilPresent() = 0;

  /// Injected by the wrapper before Initialise: the engine must render into these
  /// rather than creating a swapchain of its own (§12.5).
  virtual void SetExternalVulkanContext(const ExternalVulkanContext&) = 0;
  virtual void SetPresentSink(PresentSink*) = 0;

  virtual void SetInputSnapshot(const InputSnapshot&) = 0;   // §12.9
  virtual size_t DrainAudio(int16_t* dst, size_t max_frames) = 0;
  virtual ScreenInfo GetScreenInfo() const = 0;
};
```

### 12.1 Inversion of control: the frame gate

The whole mismatch reduces to one sentence. **A standalone engine owns its loop; libretro
requires the frontend to own it.** `retro_run()` must advance exactly one frame, synchronously,
and return.

We do not restructure the engine into a step function — that would be a permanent fork of its
scheduler. Instead the engine keeps its threads and its loop, and we install a **rendezvous at
its presentation point**. The engine's `Present` becomes the place it blocks until the frontend
asks for another frame.

```cpp
/// Two monotonic counters under one condition variable.
class FrameGate {
public:
  /// Frontend thread, from retro_run. Returns false if the frame did not arrive
  /// within `budget`, in which case the caller must dupe (§12.2).
  bool PumpFrame(std::chrono::milliseconds budget) {
    uint64_t target;
    {
      std::lock_guard lock(mutex_);
      // At most one frame in flight, and `requested_` is recomputed from `completed_`
      // rather than incremented.
      //
      // This is the subtle part. A naive `++requested_` leaks a request on every
      // timeout: the engine eventually catches up, the count is now two ahead, and it
      // runs two frames for the next single retro_run — so the frontend silently drops
      // every other frame and the game runs at double speed with half the frames shown.
      target = completed_ + 1;
      requested_ = std::max(requested_, target);
    }
    cv_.notify_all();

    std::unique_lock lock(mutex_);
    return cv_.wait_for(lock, budget,
                        [&] { return completed_ >= target || quitting_; });
  }

  /// Engine driver thread. Blocks until the frontend wants frame `served + 1`.
  bool AwaitRequest(uint64_t served) {
    std::unique_lock lock(mutex_);
    cv_.wait(lock, [&] { return requested_ > served || quitting_; });
    return !quitting_;
  }

  void PublishFrame() {
    { std::lock_guard lock(mutex_); ++completed_; }
    cv_.notify_all();
  }

  /// Must be called before joining the engine thread, or it parks in AwaitRequest
  /// forever and retro_unload_game deadlocks.
  void Shutdown() {
    { std::lock_guard lock(mutex_); quitting_ = true; }
    cv_.notify_all();
  }

  uint64_t Completed() const { std::lock_guard lock(mutex_); return completed_; }

private:
  mutable std::mutex mutex_;
  std::condition_variable cv_;
  uint64_t requested_ = 0;
  uint64_t completed_ = 0;
  bool quitting_ = false;
};
```

The engine side is a five-line driver thread, and the only patch into the engine proper is
redirecting its present call:

```cpp
void SwitchCore::DriverThread() {
  uint64_t served = 0;
  while (gate_.AwaitRequest(served)) {
    engine_->RunUntilPresent();     // engine's own CPU + GPU work, its own threads
    ++served;                       // PublishFrame happens inside OnPresent, below
  }
}

/// The engine's presentation callback, replacing its swapchain present.
void SwitchCore::OnPresent(const PresentedFrame& frame) {
  pending_ = frame;                 // VkImageView + layout + completion semaphore
  gate_.PublishFrame();
}
```

Three consequences to implement deliberately:

- **Disable the engine's own frame limiter.** It will otherwise throttle to 60 Hz *inside* a
  gate that is already throttling to the display link, and the two beat against each other
  into visible judder. The engine runs unlimited; the gate and `FramePacer` are the only
  pacing.
- **The timeout is a feature, not a safety net.** NVN→SPIR-V→MSL shader compilation stalls the
  first time a title reaches new geometry, and can take hundreds of milliseconds. Blocking
  `retro_run` for that long freezes audio and input. Instead the gate times out, the wrapper
  dupes the frame, and the engine finishes compiling in the background — audio keeps draining
  and the UI stays alive. libretro explicitly permits a duplicate: passing `NULL` to
  `video_refresh` is a frame dupe.
- **Budget the timeout from the frame interval, not a constant.** Two frame intervals
  (~33 ms at 60 Hz) tolerates jitter without letting a stall accumulate latency.

### 12.2 `retro_run`

Everything the frontend is entitled to see happens on the frontend's own thread, in this
order:

```cpp
void retro_run(void) {
  // 1. Input, once per frame, snapshotted so the engine's threads never call back out.
  input_poll_cb();
  g_core->SnapshotInput();                       // §12.9

  // 2. Core options the user changed since the last frame.
  bool updated = false;
  if (environ_cb(RETRO_ENVIRONMENT_GET_VARIABLE_UPDATE, &updated) && updated) {
    g_core->ReloadOptions();
  }

  // 3. Advance exactly one frame, or give up and dupe.
  const auto budget = std::chrono::milliseconds(
      static_cast<int>(2000.0 / g_core->RefreshRate()));

  if (g_core->Gate().PumpFrame(budget)) {
    const PresentedFrame& frame = g_core->Pending();

    // Hand the finished image to the frontend, then tell it a frame exists.
    // Order matters: set_image must precede video_refresh.
    g_vulkan_iface->set_image(g_vulkan_iface->handle,
                              &frame.retro_image,
                              frame.wait_semaphore_count,
                              frame.wait_semaphores,
                              frame.src_queue_family);
    video_cb(RETRO_HW_FRAME_BUFFER_VALID, frame.width, frame.height, 0);
  } else {
    // Shader compile, load stall, or a title that simply took longer. A dupe keeps
    // audio and input alive; the engine is still working and will publish later.
    video_cb(nullptr, g_core->Width(), g_core->Height(), 0);
    g_core->CountDupe();
  }

  // 4. Audio, drained on this thread regardless of which frame branch ran.
  g_core->PumpAudio();                           // §12.4
}
```

`RETRO_HW_FRAME_BUFFER_VALID` is `((void*)-1)` — `usize::MAX`, not a pointer to be
dereferenced. `NativeCore`'s `video_refresh` handler must test for it before anything else,
because the software path treats any non-null pointer as pixels and would read from
`0xFFFF…FFFF` on the first hardware frame.

### 12.3 `retro_load_game`: content, keys, firmware

Switch content is not a ROM. It is a signed container — NSP, XCI, NCA or NRO — that the engine
mounts as a filesystem, and it needs sibling material: `prod.keys`, `title.keys`, and system
firmware for most retail titles.

```cpp
bool retro_load_game(const struct retro_game_info* info) {
  if (!info || !info->path) {                      // need_fullpath is declared, see below
    Log(RETRO_LOG_ERROR, "Switch content must be a file path");
    return false;
  }

  // Directories come from the frontend, not from guesses about the sandbox layout.
  const char* system_dir = nullptr;
  const char* save_dir   = nullptr;
  environ_cb(RETRO_ENVIRONMENT_GET_SYSTEM_DIRECTORY, &system_dir);
  environ_cb(RETRO_ENVIRONMENT_GET_SAVE_DIRECTORY,   &save_dir);

  // Fail with a sentence a user can act on, rather than crashing three layers down
  // inside a decryption routine.
  KeyStatus keys = g_core->ValidateKeys(system_dir);
  if (!keys.ok) {
    struct retro_message message { keys.message, 300 };
    environ_cb(RETRO_ENVIRONMENT_SET_MESSAGE, &message);
    return false;
  }

  EngineConfig config{ .system_dir = system_dir, .save_dir = save_dir,
                       .jit = true, .memory_budget_bytes = g_core->MemoryBudget() };
  if (!g_core->Engine().Initialise(config))  return false;
  if (!g_core->Engine().MountContent(info->path)) return false;
  if (!g_core->Engine().BootTitle())         return false;

  g_core->StartDriverThread();
  return true;
}
```

Three environment commands this depends on, and their current state in this project:

| Command | Needed for | Status |
| --- | --- | --- |
| `SET_CONTENT_INFO_OVERRIDE` (65) | declaring `nsp\|xci\|nca\|nro` as `need_fullpath = true` | **already implemented** — built in Phase 1b for fceumm |
| `GET_GAME_INFO_EXT` (66) | the core reading the mounted path | **already implemented** |
| `GET_SYSTEM_DIRECTORY` (9) | keys, firmware | **currently refused** — must be implemented natively |
| `GET_SAVE_DIRECTORY` (31) | Switch savedata | **currently refused** — must be implemented natively |

`need_fullpath = true` is the right call rather than a concession: an XCI is tens of gigabytes,
the engine wants random access into it, and materialising it in memory to satisfy a
`data`+`size` contract would defeat the memory budget before the title even boots. The
content-store already anticipated this — its own note says large content should stream rather
than materialise as a blob — so on iOS the library gains a file-backed mode for
Switch-class content, and the blob store keeps serving cartridge-sized systems.

### 12.4 Thread affinity: the rule that breaks everything if ignored

libretro's callbacks — `video_refresh`, `audio_batch`, `input_state`, `environment` — may only
be called from the thread that called `retro_run`. A standalone engine has at least three
threads that would all naturally want to call them.

So the wrapper enforces a strict direction: **nothing inside the engine ever calls a libretro
callback.** Each crossing gets a specific mechanism:

| Engine produces | Naïve approach | What the wrapper does instead |
| --- | --- | --- |
| A presented frame, on the GPU thread | call `video_refresh` | publish through `FrameGate`; `retro_run` calls it |
| Audio, on the audio thread | call `audio_batch` | write into an SPSC ring; `retro_run` drains and calls it |
| An input query, on a HID thread | call `input_state` | read an immutable per-frame snapshot the wrapper wrote |
| A log line, anywhere | call the log interface | enqueue; flushed from `retro_run` |

The audio ring is the same discipline as the Rust `AudioSink` one layer up, for the same
reason: the producer is a real-time thread that must not allocate, lock or log.

```cpp
void SwitchCore::PumpAudio() {                    // frontend thread only
  // 48 kHz stereo; two frame intervals of headroom is ample.
  static constexpr size_t kChunk = 2048;
  int16_t buffer[kChunk * 2];
  size_t frames;
  while ((frames = engine_->DrainAudio(buffer, kChunk)) > 0) {
    audio_batch_cb(buffer, frames);
  }
}
```

Input goes the other way, and snapshotting it at the gate has a second benefit beyond thread
safety: the engine sees exactly one input state per emulated frame, which is what makes frame
timing reproducible.

### 12.5 Graphics: Vulkan out of the engine, Metal into our compositor

The engine renders with Vulkan. Our compositor is wgpu-on-Metal. MoltenVK sits between them,
and libretro already has the exact negotiation machinery needed to make them share one device.

The chain, end to end:

```text
Switch engine (Vulkan renderer, NVN→SPIR-V shaders)
   │  renders into a VkImage the wrapper allocated
   ▼
wrapper: set_image(VkImageView + layout + completion semaphore)
   │
   ▼
frontend (Rust): VkImage ──VK_EXT_metal_objects──▶ MTLTexture
   │
   ▼
wgpu::create_texture_from_hal  →  instanced composite pass  →  CAMetalLayer
```

There is **no pixel copy anywhere in that chain.** Both sides are Metal underneath, and
`VK_EXT_metal_objects` is the seam that says so out loud.

#### Device negotiation, in order

The engine must not create its own `VkDevice`. libretro's negotiation interface exists
precisely for this:

```cpp
// Declared during retro_set_environment, before retro_init.
static const struct retro_hw_render_context_negotiation_interface_vulkan negotiation = {
  .interface_type    = RETRO_HW_RENDER_CONTEXT_NEGOTIATION_INTERFACE_VULKAN,
  .interface_version = RETRO_HW_RENDER_CONTEXT_NEGOTIATION_INTERFACE_VULKAN_VERSION,
  .get_application_info = GetApplicationInfo,   // we want Vulkan 1.2
  .create_device        = CreateDevice,         // §12.6 — where the shim lives
  .destroy_device       = DestroyDevice,
};
environ_cb(RETRO_ENVIRONMENT_SET_HW_RENDER_CONTEXT_NEGOTIATION_INTERFACE,
           (void*)&negotiation);

static struct retro_hw_render_callback hw_render = {
  .context_type       = RETRO_HW_CONTEXT_VULKAN,
  .version_major      = 1,
  .version_minor      = 2,
  .context_reset      = ContextReset,
  .context_destroy    = ContextDestroy,
  .cache_context      = false,        // we rebuild on loss; see §10
  .bottom_left_origin = false,        // Vulkan is top-left, like Metal
  .depth = true, .stencil = true,
};
environ_cb(RETRO_ENVIRONMENT_SET_HW_RENDER, &hw_render);
```

Then, inside `context_reset`, the wrapper asks for the frontend's Vulkan objects and injects
them into the engine:

```cpp
static void ContextReset(void) {
  const struct retro_hw_render_interface* iface = nullptr;
  if (!environ_cb(RETRO_ENVIRONMENT_GET_HW_RENDER_INTERFACE, &iface) || !iface) {
    Log(RETRO_LOG_ERROR, "frontend provided no Vulkan render interface");
    return;
  }
  g_vulkan_iface = reinterpret_cast<const retro_hw_render_interface_vulkan*>(iface);

  // Everything the engine needs, and nothing it may create for itself.
  ExternalVulkanContext ctx {
    .instance           = g_vulkan_iface->instance,
    .physical_device    = g_vulkan_iface->gpu,
    .device             = g_vulkan_iface->device,
    .queue              = g_vulkan_iface->queue,
    .queue_family_index = g_vulkan_iface->queue_index,
    .get_instance_proc  = g_vulkan_iface->get_instance_proc_addr,
    .get_device_proc    = g_vulkan_iface->get_device_proc_addr,
    // The engine wraps every vkQueueSubmit in these. See below — this is the
    // single most important line in the section.
    .lock_queue         = [] { g_vulkan_iface->lock_queue(g_vulkan_iface->handle); },
    .unlock_queue       = [] { g_vulkan_iface->unlock_queue(g_vulkan_iface->handle); },
  };
  g_core->Engine().SetExternalVulkanContext(ctx);
  g_core->Engine().SetPresentSink(g_core->PresentSink());
  g_core->AllocateTargets();                 // double-buffered, §6
}
```

> **Verified against the real header.** `libretro_vulkan.h` is now fetched into `.work/hdr/`
> and the wrapper compiles against it, so the accesses above are checked rather than assumed.
> Two things that a from-memory transcription gets wrong, and which are load-bearing:
>
> - `void *handle` is the **third** field, immediately after `interface_type` and
>   `interface_version` — not a trailing addition. It is the frontend's opaque backend pointer
>   and every function pointer on the interface takes it as its first argument, which is why
>   the `lock_queue`/`unlock_queue` lambdas above must capture and pass
>   `g_vulkan_iface->handle`. Calling them with anything else is a wild pointer dereference
>   inside the frontend.
> - The order is `queue` **then** `queue_index`, and `get_device_proc_addr` comes **before**
>   `get_instance_proc_addr` — the reverse of the conventional instance-then-device idiom.
>
> Both only matter if you ever build this struct positionally; reading fields by name, as
> above, is order-independent and is the pattern the wrapper actually uses.
> `RETRO_HW_RENDER_INTERFACE_VULKAN_VERSION` is `5`, and the negotiation interface version
> is `2`.

#### `lock_queue` is not optional

A `VkQueue` is not thread-safe, and under MoltenVK a `VkQueue` *is* an `MTLCommandQueue`. The
engine's GPU thread and our compositor will both submit to it. libretro provides
`lock_queue`/`unlock_queue` on the interface for exactly this, and every engine submission
must be wrapped:

```cpp
void EngineVulkanBackend::Submit(VkSubmitInfo& info, VkFence fence) {
  ctx_.lock_queue();
  vkQueueSubmit(ctx_.queue, 1, &info, fence);
  ctx_.unlock_queue();
}
```

Most engines funnel submissions through one scheduler object — a `MasterSemaphore`, a
`CommandScheduler`, or similar — which means this is a **single patch site**, not a scattered
change. Finding that chokepoint is the first task when integrating a given engine.

This is also what makes §6's "one `MTLCommandQueue` for everything" safe rather than
optimistic: the lock is what serialises three producers onto one queue.

#### Handing over the frame

`set_image` takes a `retro_vulkan_image`, plus the semaphores the frontend must wait on before
sampling:

```cpp
struct retro_vulkan_image {
  VkImageView          image_view;
  VkImageLayout        image_layout;
  VkImageViewCreateInfo create_info;
};
```

The wrapper's present sink fills that in and hands over the engine's completion semaphore, so
the frontend's composite pass waits on the GPU rather than the CPU:

```cpp
void PresentSink::OnEnginePresent(VkImage image, VkImageView view,
                                  VkSemaphore done, uint32_t width, uint32_t height) {
  PresentedFrame frame{};
  frame.retro_image.image_view   = view;
  // The engine leaves it ready to sample; declaring anything else costs a barrier.
  frame.retro_image.image_layout = VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL;
  frame.retro_image.create_info  = MakeViewCreateInfo(image);
  frame.wait_semaphores          = &done;
  frame.wait_semaphore_count     = 1;
  frame.src_queue_family         = ctx_.queue_family_index;
  frame.width = width; frame.height = height;

  core_->SetPending(frame);
  core_->Gate().PublishFrame();     // unblocks retro_run; §12.1
}
```

On the Rust side this arrives as a `VkImage`, and §5's export turns it into the `MTLTexture`
that `HwContext::begin_frame` hands back as an `HwTarget`. From `bridge.rs` upward it is
indistinguishable from a PS1 frame.

#### Resolution and geometry

Switch titles render at up to 1080p docked, and the engine may upscale further. Declared
geometry is therefore dynamic: the wrapper reports a conservative `base_width`/`base_height`
in `retro_get_system_av_info` and then issues `SET_GEOMETRY` (command 37) whenever the engine's
target changes — which our renderer already handles, because `SET_GEOMETRY` was implemented in
Phase 1 and plumbs through `CoreHost::geometryChanged` to resize staging. `max_width` and
`max_height` must be declared generously up front, because they size the frontend's target
allocation and cannot grow mid-session.

### 12.6 The MoltenVK capability shim

MoltenVK implements Vulkan on Metal, not all of Vulkan. A Switch engine asks for a demanding
feature set, so `create_device` in the negotiation interface is where the wrapper reconciles
the two — and, crucially, where it reports a precise reason rather than failing opaquely.

```cpp
static bool CreateDevice(struct retro_vulkan_context* context,
                         VkInstance instance, VkPhysicalDevice gpu,
                         VkSurfaceKHR surface,
                         PFN_vkGetInstanceProcAddr get_instance_proc_addr,
                         const char** required_device_extensions,
                         unsigned num_required_device_extensions,
                         const char** required_device_layers,
                         unsigned num_required_device_layers,
                         const VkPhysicalDeviceFeatures* required_features) {
  CapabilityReport report = ProbeDevice(gpu, get_instance_proc_addr);
  if (!report.satisfiable) {
    // Names the missing extension. A user-visible "your device cannot run this"
    // beats a validation-layer abort inside the engine's pipeline cache.
    Log(RETRO_LOG_ERROR, "unsupported Vulkan device: %s", report.detail.c_str());
    return false;
  }
  return BuildDevice(context, instance, gpu, report);
}
```

The features that actually decide whether a title runs, and the disposition for each:

| Requirement | Under MoltenVK | Disposition |
| --- | --- | --- |
| `VK_KHR_portability_subset` | Always reported | **Accept it.** An engine that asserts on a non-conformant device must have that assert relaxed — this is the first patch. |
| Timeline semaphores | Vulkan 1.2 core, supported | Use directly; also how frame completion is signalled. |
| `VK_EXT_descriptor_indexing` | Supported via Metal argument buffers | Requires `MVK_CONFIG_USE_METAL_ARGUMENT_BUFFERS=1`. Set it in the shim, not in the engine. |
| `VK_KHR_push_descriptor` | Supported | Direct. |
| `VK_EXT_extended_dynamic_state` 1/2/3 | Partial | Where a state is not dynamic, the engine falls back to more pipeline permutations. Costs cache size, not correctness. |
| **Geometry shaders** | **Not available — Metal has none** | Translate to compute at the shader-translation stage. Switch engines already carry this path for other Metal targets; the shim's job is to advertise the capability as absent so that path is selected. |
| `VK_EXT_transform_feedback` | Not available | Same treatment: compute-shader emulation. |
| `shaderInt64` | Supported on Apple GPUs via Metal | Direct. |
| 8-/16-bit storage | Supported | Direct. |
| `VK_EXT_robustness2` / `nullDescriptor` | Supported | Direct; the engine relies on null descriptors heavily. |

Two configuration values belong in the shim rather than anywhere else, because they change
MoltenVK's behaviour globally and the engine should not be aware of them:

```cpp
// Before vkCreateInstance.
setenv("MVK_CONFIG_USE_METAL_ARGUMENT_BUFFERS", "1", 1);   // descriptor indexing
setenv("MVK_CONFIG_PREFILL_METAL_COMMAND_BUFFERS", "1", 1); // lower submit latency
```

### 12.7 Shader cache

NVN → SPIR-V → MSL is two translations in series, and the second is a Metal pipeline
compilation. Uncached, a title stutters on first encounter with every shader. Two caches,
persisted separately, both keyed on everything that can invalidate them:

```text
cache key = title_id · engine_version · moltenvk_version · metal_family · wrapper_abi
```

- The **engine's own** NVN→SPIR-V cache, in its native format.
- MoltenVK's `VkPipelineCache`, serialised via `vkGetPipelineCacheData` on shutdown.

Omitting `moltenvk_version` from the key is the trap: a MoltenVK upgrade changes the MSL it
generates, and a stale cache then serves shaders compiled against different semantics.

Precompilation on first boot is worth doing where the engine supports it — it converts
mid-game stutter into a one-time progress bar, which is the right trade for something the
frontend can show honestly.

### 12.8 Save states and savedata

Switch save states are not solved in any existing engine. libretro treats state support as
optional, and this project already handles its absence correctly:

```cpp
size_t retro_serialize_size(void) { return 0; }             // "unsupported"
bool   retro_serialize(void*, size_t)   { return false; }
bool   retro_unserialize(const void*, size_t) { return false; }
```

This path is already handled correctly end to end, which was worth checking rather than
assuming:

- `bridge.rs` turns `state_size() == 0` into
  `BridgeError::SaveState("this core does not support save states")`.
- `player-view.js` degrades a failed capture to a `console.warn` on the automatic paths, and
  the *manual* Save button already raises `toast('Nothing to save', 'This core does not
  support save states.')` — so a user who presses Save is told, rather than watching nothing
  happen.
- The resume path treats "no auto state" as a normal cold boot.

So the only gap is discoverability *before* the user tries: the detail sheet's save-state
section should read "unavailable for this system" instead of "none yet". One conditional, and
it stops someone believing their progress is checkpointed when it is not.

What does preserve progress is **savedata**: the Switch's own save filesystem, written through
`GET_SAVE_DIRECTORY`. The wrapper must flush it at three points — `retro_unload_game`,
`retro_deinit`, and on the `willResignActive` transition of §10 — because an app that is
backgrounded and later killed by the OS never gets a clean shutdown. That flush ordering is
the same reasoning as saving state before `context_destroy`.

### 12.9 Input

One snapshot per frame, written by the wrapper on the frontend thread and read by the engine's
HID emulation:

```cpp
void SwitchCore::SnapshotInput() {
  InputSnapshot snapshot{};
  for (unsigned port = 0; port < kMaxPlayers; ++port) {
    for (unsigned id = 0; id < 16; ++id) {
      snapshot.buttons[port] |=
          input_state_cb(port, RETRO_DEVICE_JOYPAD, 0, id) ? (1u << id) : 0u;
    }
    snapshot.left[port]  = { AnalogAxis(port, RETRO_DEVICE_INDEX_ANALOG_LEFT,  0),
                             AnalogAxis(port, RETRO_DEVICE_INDEX_ANALOG_LEFT,  1) };
    snapshot.right[port] = { AnalogAxis(port, RETRO_DEVICE_INDEX_ANALOG_RIGHT, 0),
                             AnalogAxis(port, RETRO_DEVICE_INDEX_ANALOG_RIGHT, 1) };
  }
  // Handheld-mode touch, through the same pointer plumbing as DS and 3DS (§9).
  snapshot.touch_pressed = input_state_cb(0, RETRO_DEVICE_POINTER, 0,
                                          RETRO_DEVICE_ID_POINTER_PRESSED) != 0;
  snapshot.touch_x = input_state_cb(0, RETRO_DEVICE_POINTER, 0, RETRO_DEVICE_ID_POINTER_X);
  snapshot.touch_y = input_state_cb(0, RETRO_DEVICE_POINTER, 0, RETRO_DEVICE_ID_POINTER_Y);

  engine_->SetInputSnapshot(snapshot);            // atomic swap, lock-free for readers
}
```

The button *layout* mapping is already settled: `GamepadBridge` in Rust owns the
standard-gamepad mapping, deadzones and stick-to-D-pad synthesis, and Apple's
`GameController` framework reports the same layout a browser did — so `apply_gamepad` is
called with the same numbers, and the wrapper receives already-normalised libretro IDs.
Six-axis motion needs `GET_SENSOR_INTERFACE` fed from CoreMotion, which is additive and can
follow.

### 12.10 Building and linking

The wrapper is C++20 and links as a single dynamic library inside the app bundle, which is
what `NativeCore` `dlopen`s. Two shapes depending on the engine's language:

| Engine language | Binding | Notes |
| --- | --- | --- |
| **C++** | Direct static link into the wrapper's dylib | Simplest. `ISwitchEngine` implemented by a thin adapter over the engine's own classes. |
| **C#** | NativeAOT to a static library, `[UnmanagedCallersOnly]` exports | The wrapper links that archive and calls through a generated C header. Requires `PublishAot` with `InvariantGlobalization`; GC and JIT coexist, so the JIT entitlement covers both. |

Either way the exported surface is identical, and the wrapper's own `ISwitchEngine` is what
absorbs the difference. Build wiring:

```text
switch_libretro.dylib
  ├── continuum_switch_libretro.cpp      (this design)
  ├── engine adapter                     (ISwitchEngine implementation)
  ├── engine archive                     (.a from C++ build, or NativeAOT output)
  └── links against: MoltenVK.xcframework
```

Signed with the app, embedded in `Frameworks/`, loaded by path at launch. `RPATH` must be
`@executable_path/Frameworks` or `dlopen` fails on device while working in the simulator.

---

## 13. Sequence

Each step is verifiable on its own, and the risky question is answered first.

| # | Step | Proves | Risk |
| --- | --- | --- | --- |
| 1 | ~~wgpu device adopted from an injected `MTLDevice`~~ → **done**, inverted: wgpu creates the device, Swift adopts it (§2) | The §5 unknown, before anything depends on it | ~~High~~ — resolved without raw Metal and without patching `wgpu-hal` |
| 2 | Instanced composite pass; one screen, then two with a hardcoded split | Rendering generalises before any HW core exists | Low |
| 3 | MoltenVK in-process, sharing device and queue; render a triangle into an `MTLTexture` and composite it | The whole zero-copy path, with no core involved | Medium |
| 4 | `SET_HW_RENDER` accepted for Vulkan; `GET_HW_RENDER_INTERFACE`; **Beetle PSX HW** | The full contract against the simplest real core | Medium |
| 5 | JIT enabled for the PS1 dynarec; measure against step 4 | The reason for the whole phase | Low, now unconditional |
| 6 | **paraLLEl-N64** | Vulkan *compute*, and the TLB fast-mem work | High |
| 7 | ANGLE alongside MoltenVK; **melonDS**; `ScreenGeometry` + touch mapping | GL path and dual screen together | Medium |
| 8 | **Citra**, unequal screen widths | The 3DS asymmetry the DS does not expose | Low after step 7 |
| 9 | **PPSSPP** | Confirms `GET_PREFERRED_HW_RENDER` steering a dual-backend core | Low |
| 10 | **Switch wrapper**, stage 1: `retro_*` skeleton + `FrameGate` against a stub engine that clears to a colour | The inversion of control, with no emulator involved | Medium |
| 11 | Switch wrapper, stage 2: real engine behind `ISwitchEngine`, `lock_queue` patched into its scheduler, homebrew NRO booting | The engine actually driving our pipeline | High |
| 12 | Switch wrapper, stage 3: capability shim, geometry-shader-to-compute path, shader cache, retail content | Retail titles | High |

Steps 1–3 involve no emulator core at all, which is deliberate: they are where the graphics
architecture is either proven or corrected, and they are cheap to iterate on because a
failure is a triangle that does not appear rather than a game that misbehaves.

Step 10 deserves the same treatment for the same reason. A stub `ISwitchEngine` that does
nothing but clear its target to a rotating colour and call `PublishFrame` exercises the entire
wrapper — the gate, the thread affinity, `set_image`, the Vulkan handover, the timeout-and-dupe
path — with zero emulator variables in play. Build that before wiring a real engine in, and
every bug found in step 11 is unambiguously an engine-integration bug.

Homebrew NRO before retail content in step 11, likewise: an NRO needs no keys, no firmware and
no decryption, so it isolates the execution path from the content path.

---

## 14. Open questions

Genuinely unresolved, listed so they are not mistaken for decided:

1. **Can wgpu adopt a pre-existing `MTLDevice` through its public API?** Step 1 answers it.
   Fallback is a raw-Metal composite pass, which is contained but means maintaining one
   Metal shader alongside the WGSL one.
2. **One command queue for all three layers, or per-layer queues with an `MTLSharedEvent`?**
   Measure both; the shared-queue version is simpler and probably fast enough, but MoltenVK's
   internal queue management may not cooperate.
3. **Does any target core require GL features ANGLE's Metal backend does not implement?**
   GLideN64 leans on some older desktop-GL behaviour. Worth an early audit rather than
   discovering it at step 7.
4. **Where is the engine's queue-submission chokepoint?** `lock_queue` has to wrap every
   `vkQueueSubmit` (§12.5). If the engine submits from more than one place the patch surface
   grows, and finding out is the first task of step 11, not step 12.
5. **Does the engine's Vulkan backend tolerate an injected device?** Some assume they created
   the instance and hold onto creation-time state. The negotiation interface is designed for
   this, but engines not previously used as libretro cores may need the assumption unpicked.
6. **How much of the geometry-shader-to-compute path already exists** in the chosen engine?
   Every Switch engine with a Metal or MoltenVK target has had to solve it; the amount of that
   work already done is the largest single swing in the step 12 estimate.
