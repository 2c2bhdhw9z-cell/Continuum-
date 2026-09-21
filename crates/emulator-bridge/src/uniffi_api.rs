//! The Swift-facing facade. Peer of [`crate::wasm`], and held to the same rule: nothing in
//! here is logic. If a function in this file does anything other than convert a type,
//! acquire the lock and delegate, then something has leaked out of the engine and belongs
//! back in `bridge.rs`.
//!
//! ## Why UniFFI, and what the copy costs
//!
//! UniFFI copies sequences across the boundary, which is its one real cost. Video never pays
//! it: frames go core → staging → Metal entirely inside Rust, so the pixel path does not
//! cross here at all. Control traffic does not care: launch, pause, save state and settings
//! are a few dozen calls a minute, and for those, generated correctness beats hand-written
//! speed.
//!
//! Audio is the one per-frame payload that does cross, through [`ContinuumEngine::drain_audio`],
//! and the copy is accepted deliberately. The alternative would be handing Swift a pointer into
//! the Rust ring, which UniFFI cannot express and which would put the platform's real-time
//! render thread inside this file's `Mutex` - the one place it must never be, because the
//! display link already holds that lock for the whole of every tick. So a tick's worth of PCM
//! is copied out on the display link's thread, roughly 6 KB at 48 kHz, and the render thread
//! reads a lock-free ring on the Swift side that never calls back into Rust. See
//! `native/ios/AudioOutput.swift` for that half.
//!
//! ## `Mutex`, not `RefCell`
//!
//! The wasm build gets away with a `RefCell` because there is one thread. Swift will call
//! `tick` from a `CADisplayLink` and `saveState` from a `Task`, so a `RefCell` there is a
//! panic waiting for a scheduling hiccup. The consequence is that the re-entrancy
//! discipline in `cores/host.rs` and `cores/native_core.rs` — callbacks never reach back
//! into the bridge, because it is already borrowed during a frame — becomes a deadlock
//! hazard rather than a panic, and is therefore load-bearing rather than tidy.
//!
//! See `docs/NATIVE_IOS_BLUEPRINT.md` §3 and `docs/SET_HW_RENDER_DESIGN.md` §7.

use std::sync::Mutex;

use crate::audio::CHANNELS;
use crate::bridge::EmulatorBridge;
use crate::error::BridgeError;

/// Ceiling on one [`ContinuumEngine::drain_audio`] call, in stereo frames.
///
/// 4096 frames is about 85 ms at 48 kHz, which is deliberately the same ceiling `wasm.rs`
/// gives itself through its 8192-sample staging buffer. Far more than one display-link tick
/// can ever owe, so a slow frame cannot be truncated by this bound, and a caller that asks
/// for a million frames gets a clamp rather than a 16 MB allocation.
const MAX_DRAIN_FRAMES: u32 = 4096;

/// Errors as Swift sees them: one variant per `BridgeError` arm that a caller can act on,
/// collapsed where the distinction is meaningless outside Rust.
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum EngineError {
    #[error("no session is running")]
    NoSession,
    #[error("core '{core_id}' is unavailable: {reason}")]
    CoreUnavailable { core_id: String, reason: String },
    #[error("content was rejected: {reason}")]
    ContentRejected { reason: String },
    #[error("save state failed: {reason}")]
    SaveState { reason: String },
    #[error("cheats failed: {reason}")]
    Cheat { reason: String },
    #[error("core option failed: {reason}")]
    CoreOption { reason: String },
    #[error("graphics failure: {reason}")]
    Graphics { reason: String },
    #[error("{reason}")]
    Other { reason: String },
}

impl From<BridgeError> for EngineError {
    fn from(error: BridgeError) -> Self {
        // Every arm is mapped explicitly rather than falling through to `Other`, so adding
        // a `BridgeError` variant is a compile error here instead of silently degrading
        // into an untyped string on the Swift side.
        match error {
            BridgeError::NoSession => Self::NoSession,
            BridgeError::NoRenderer => Self::Graphics {
                reason: "no renderer attached; call attachMetal first".into(),
            },
            BridgeError::Gfx(inner) => Self::Graphics {
                reason: inner.to_string(),
            },
            BridgeError::UnknownCore(core_id) => Self::CoreUnavailable {
                core_id,
                reason: "not declared in the registry".into(),
            },
            BridgeError::CoreNotLoaded { core_id, state } => Self::CoreUnavailable {
                core_id,
                reason: format!("declared but not loaded (state: {state})"),
            },
            BridgeError::CoreBusy(core_id) => Self::CoreUnavailable {
                core_id,
                reason: "already bound to a running session".into(),
            },
            BridgeError::InvalidCoreModule { core_id, reason } => {
                Self::CoreUnavailable { core_id, reason }
            }
            BridgeError::InvalidContent { reason, .. } => Self::ContentRejected { reason },
            BridgeError::SaveState(reason) => Self::SaveState { reason },
            BridgeError::Cheat(reason) => Self::Cheat { reason },
            BridgeError::CoreOption(reason) => Self::CoreOption { reason },
            BridgeError::NotImplemented(what) => Self::Other {
                reason: format!("not implemented: {what}"),
            },
        }
    }
}

/// One tick's telemetry.
///
/// A struct rather than the wasm build's shared-memory scratch array, because there is no
/// shared linear memory here: Swift cannot read into Rust's heap, so the numbers are
/// returned by value. Small and flat, so the copy is a handful of words.
#[derive(Debug, Clone, uniffi::Record)]
pub struct TickTelemetry {
    pub steps: u32,
    pub dropped: u32,
    pub presented: bool,
    pub resynced: bool,
    pub display_fps: f64,
    pub frame_count: u64,
    pub audio_queued_frames: u32,
    pub audio_underruns: u32,
    /// True when the frame just presented came from a hardware-rendered target rather than
    /// a pixel upload. Surfaced so the Swift layer can show which path is live without
    /// guessing from the core's name.
    pub hardware_frame: bool,
}

/// A core the app knows about but has not loaded.
///
/// Peer of `wasm::CoreDeclaration`, and a plain record rather than a constructed object
/// because UniFFI generates a Swift struct. Note `systems` is a real `Vec<String>`: the wasm
/// facade takes a comma-separated string only because wasm-bindgen would otherwise emit a
/// wrapper class per element, and that workaround should not be copied here.
///
/// Declaring is not loading. It costs a few hundred bytes and touches no filesystem, which
/// is what lets rule 5 hold — nothing is resident until something asks for it.
#[derive(Debug, Clone, uniffi::Record)]
pub struct CoreDeclaration {
    pub id: String,
    pub display_name: String,
    pub systems: Vec<String>,
    /// Path within the app bundle. Native cores are `dlopen`ed from here.
    pub module_path: String,
    pub base_width: u32,
    pub base_height: u32,
    pub max_width: u32,
    pub max_height: u32,
    pub aspect_ratio: f32,
    pub target_fps: f64,
    pub audio_sample_rate: u32,
    /// `0` = RGB565, `1` = XRGB8888, `2` = RGBA8888. Matches `PixelFormat::as_u32`.
    pub pixel_format: u32,
    /// Higher wins when several cores can run the same system.
    pub priority: i32,
}

/// The audio ring, as Swift sees it.
///
/// A snapshot rather than a subscription: the numbers are read when a HUD asks for them, and
/// the per-frame mirror in [`TickTelemetry`] carries the two a HUD wants every frame so that
/// showing them costs no extra trip through the engine lock.
///
/// Every field is a fact the Swift side cannot work out for itself. `queued_frames` against
/// `capacity_frames` is how full the Rust ring is; `overruns` counts audio the device was too
/// slow to take, `underruns` counts silence the ring could not fill, and `source_rate` against
/// `output_rate` says what the resampler is actually doing. Which matters because on iOS the
/// output rate is whatever the hardware said it was, not a constant.
#[derive(Debug, Clone, uniffi::Record)]
pub struct AudioStatsSnapshot {
    /// Interleaved stereo frames waiting to be drained.
    pub queued_frames: u32,
    /// The ring's size, fixed when the session started.
    pub capacity_frames: u32,
    /// Times the ring filled and the oldest audio was dropped.
    pub overruns: u64,
    /// Times a drain asked for more than was queued.
    pub underruns: u64,
    pub frames_submitted: u64,
    pub frames_drained: u64,
    /// The rate the core produces at.
    pub source_rate: u32,
    /// The rate the ring is resampled to, which is the device's real rate once Swift has
    /// reported it through [`ContinuumEngine::set_output_sample_rate`].
    pub output_rate: u32,
    pub channels: u32,
}

/// One core option, as the core itself declared it.
#[derive(Debug, Clone, uniffi::Record)]
pub struct CoreOptionRecord {
    pub key: String,
    pub label: String,
    pub value: String,
    pub values: Vec<String>,
}

/// How the game's image is fitted to the screen.
///
/// A mirror of [`crate::gfx::ScaleMode`] rather than that type carrying a `uniffi::Enum`
/// derive itself, and deliberately so. The graphics layer is not supposed to know a foreign
/// function interface exists - the same reasoning that keeps `MetalHandles` unexported at
/// the foot of this file - and `renderer.rs` also compiles for targets where `uniffi` is
/// not a dependency at all. Two four-line enums and a `From` impl is a cheaper price than
/// coupling those layers, and it means this boundary can name things for the person reading
/// a settings screen rather than for the person reading a shader.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ScaleModeOption {
    /// Fills as much of the screen as possible while keeping the correct shape, leaving
    /// black bars on whichever axis runs out first.
    AspectFit,
    /// Rounds down to a whole multiple of the game's own pixel grid, so every emulated
    /// pixel is the same size as every other. Wastes more of the screen and is the reason
    /// people ask for it.
    IntegerScale,
    /// Fills the screen and accepts the distortion.
    Stretch,
}

impl From<ScaleModeOption> for crate::gfx::ScaleMode {
    fn from(mode: ScaleModeOption) -> Self {
        match mode {
            ScaleModeOption::AspectFit => Self::AspectFit,
            ScaleModeOption::IntegerScale => Self::IntegerScale,
            ScaleModeOption::Stretch => Self::Stretch,
        }
    }
}

impl From<crate::gfx::ScaleMode> for ScaleModeOption {
    fn from(mode: crate::gfx::ScaleMode) -> Self {
        match mode {
            crate::gfx::ScaleMode::AspectFit => Self::AspectFit,
            crate::gfx::ScaleMode::IntegerScale => Self::IntegerScale,
            crate::gfx::ScaleMode::Stretch => Self::Stretch,
        }
    }
}

/// How a game's pixels are sampled when scaled up. See [`ScaleModeOption`] on why this
/// mirrors [`crate::gfx::ScaleFilter`] instead of being it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ScaleFilterOption {
    /// Hard pixel edges. What the games were drawn for, and the default.
    Nearest,
    /// Smooths between pixels.
    Linear,
}

impl From<ScaleFilterOption> for crate::gfx::ScaleFilter {
    fn from(filter: ScaleFilterOption) -> Self {
        match filter {
            ScaleFilterOption::Nearest => Self::Nearest,
            ScaleFilterOption::Linear => Self::Linear,
        }
    }
}

impl From<crate::gfx::ScaleFilter> for ScaleFilterOption {
    fn from(filter: crate::gfx::ScaleFilter) -> Self {
        match filter {
            crate::gfx::ScaleFilter::Nearest => Self::Nearest,
            crate::gfx::ScaleFilter::Linear => Self::Linear,
        }
    }
}

/// One captured frame: the image, and the size it came back at.
///
/// The size is returned rather than assumed because the caller is allowed to ask for `0, 0`
/// meaning "whatever the screen is", and because a request is not a promise: the renderer frames
/// the capture for its own dimensions, so a caller that guessed would eventually guess wrong and
/// read the bytes at the wrong stride, which looks like a sheared image rather than an error.
#[derive(Debug, Clone, uniffi::Record)]
pub struct CapturedFrame {
    pub width: u32,
    pub height: u32,
    /// Tightly packed RGBA8, `width * height * 4` bytes, top row first. No padding: the engine
    /// has already removed the GPU's row alignment, so this can be handed straight to an image
    /// constructor.
    pub rgba: Vec<u8>,
}

/// Which thing produced a poll of input.
///
/// The engine holds one independent layer per source and merges them when the core reads
/// input, which is what lets the on-screen pad and a physical controller be used together,
/// including on the same button in the same frame. Mirrors `input::PadSource` for the reason
/// [`ScaleModeOption`] mirrors its gfx counterpart: the input layer should not have to know
/// an FFI exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum InputSource {
    /// A physical controller.
    Gamepad,
    /// A hardware keyboard.
    Keyboard,
    /// The on-screen pad.
    Touch,
}

impl From<InputSource> for crate::input::PadSource {
    fn from(source: InputSource) -> Self {
        match source {
            InputSource::Gamepad => Self::Gamepad,
            InputSource::Keyboard => Self::Keyboard,
            InputSource::Touch => Self::Touch,
        }
    }
}

/// The rewind tape, as Swift sees it.
///
/// `bytes` against `budget_bytes` is how full the tape is, and dividing `snapshots` by the
/// snapshot rate is how many seconds of rewind are actually available - which is the number
/// a user cares about and the one that cannot be stated up front, because it depends on how
/// large the running core's save states turn out to be.
#[derive(Debug, Clone, uniffi::Record)]
pub struct RewindStatsSnapshot {
    /// Snapshots currently held.
    pub snapshots: u32,
    /// Bytes those snapshots occupy.
    pub bytes: u64,
    /// Snapshots discarded to stay inside the budget. Climbing steadily is the tape
    /// working, not failing.
    pub evicted: u64,
    /// The ceiling. `0` means rewind is switched off.
    pub budget_bytes: u64,
    /// Core frames between snapshots.
    pub interval_frames: u32,
}

/// The engine, as Swift holds it.
#[derive(uniffi::Object)]
pub struct ContinuumEngine {
    inner: Mutex<EmulatorBridge>,
}

#[uniffi::export]
impl ContinuumEngine {
    #[uniffi::constructor]
    pub fn new() -> std::sync::Arc<Self> {
        std::sync::Arc::new(Self {
            inner: Mutex::new(EmulatorBridge::new()),
        })
    }

    /// Builds the renderer against Swift's `CAMetalLayer`.
    ///
    /// **The device travels the other way.** The blueprint had Swift create the `MTLDevice`
    /// and pass it in; that is not implementable on `wgpu` 30 and, worse, would have looked
    /// like it worked. `wgpu`'s Metal backend exposes no public constructor taking an
    /// existing device, and `Surface::configure` reassigns `CAMetalLayer.device` to its own
    /// device regardless — so an injected device ends up owning nothing the layer draws. The
    /// reasoning, with source references, is in [`crate::gfx::metal`].
    ///
    /// The invariant is unchanged: one `MTLDevice` for wgpu, Swift and later MoltenVK, so
    /// every texture is shareable by construction. Swift reads it back with
    /// [`Self::metal_device_handle`] instead of supplying it. Nothing else in the process
    /// calls `MTLCreateSystemDefaultDevice()`.
    ///
    /// `layer` is the address of a live `CAMetalLayer`; `width`/`height` are its
    /// `drawableSize` in device pixels, not points.
    ///
    /// The platform split is inside the body rather than on the method, because
    /// `#[uniffi::export]` generates scaffolding for every method in the block without
    /// honouring `#[cfg]` on them — a `cfg`-gated method here fails to compile off-Apple
    /// with "method not found". Keeping the signature unconditional also guarantees the
    /// generated Swift is identical no matter which target's library the generator read.
    pub fn attach_metal(&self, layer: u64, width: u32, height: u32) -> Result<(), EngineError> {
        #[cfg(target_vendor = "apple")]
        {
            // SAFETY: Swift passes the backing layer of a live UIView, which outlives the
            // renderer. Checked for null inside.
            let renderer = unsafe {
                crate::gfx::metal::renderer_from_metal_layer(
                    layer as *mut core::ffi::c_void,
                    width,
                    height,
                )
            }
            .map_err(|error| EngineError::Graphics {
                reason: error.to_string(),
            })?;

            let mut guard = self.lock();
            guard.attach_renderer(renderer);
            Ok(())
        }
        #[cfg(not(target_vendor = "apple"))]
        {
            let _ = (layer, width, height);
            Err(EngineError::Graphics {
                reason: "Metal is only available on Apple targets".into(),
            })
        }
    }

    /// The `MTLDevice` the renderer created, as an `id<MTLDevice>` address, or 0.
    ///
    /// Swift uses this instead of `MTLCreateSystemDefaultDevice()`; MoltenVK will be
    /// initialised on it when the Vulkan core path lands. Zero means `attach_metal` has not
    /// run or did not land on the Metal backend, and a caller must treat it as a failure
    /// rather than a default.
    pub fn metal_device_handle(&self) -> u64 {
        #[cfg(target_vendor = "apple")]
        {
            self.metal_handles().map(|h| h.device).unwrap_or(0)
        }
        #[cfg(not(target_vendor = "apple"))]
        {
            0
        }
    }

    /// The `MTLCommandQueue` the renderer created, or 0.
    pub fn metal_queue_handle(&self) -> u64 {
        #[cfg(target_vendor = "apple")]
        {
            self.metal_handles().map(|h| h.queue).unwrap_or(0)
        }
        #[cfg(not(target_vendor = "apple"))]
        {
            0
        }
    }

    /// A human-readable description of the adapter, for a HUD.
    ///
    /// Exists because the only way to observe this build is on a phone: "Apple A19 Pro,
    /// Metal" on screen is the difference between a working graphics path and a black view
    /// that might be a black frame.
    pub fn renderer_summary(&self) -> Option<String> {
        self.lock().renderer().map(|r| r.adapter_summary())
    }

    pub fn has_renderer(&self) -> bool {
        self.lock().has_renderer()
    }

    /// Reports a drawable size change. Called from `layoutSubviews`.
    pub fn resize_surface(&self, width: u32, height: u32) {
        self.lock().resize(width, height);
    }

    /// One display-link tick: input → core → audio → GPU, all inside Rust.
    ///
    /// `now_millis` is `CADisplayLink.targetTimestamp * 1000` — when the frame will be
    /// *shown*, not when the callback fired. `FramePacer::plan()` takes a timestamp rather
    /// than reading a clock precisely so this can be handed the display's own schedule.
    ///
    /// A tick that fails is reported as a dropped frame rather than an error, because
    /// throwing into a display-link callback sixty times a second would be unhelpful: the
    /// telemetry already carries `dropped`, which is where a Swift HUD should look.
    pub fn tick(&self, now_millis: f64) -> TickTelemetry {
        let mut guard = self.lock();
        match guard.tick(now_millis) {
            Ok(report) => TickTelemetry {
                steps: report.steps,
                dropped: report.dropped,
                presented: report.presented,
                resynced: report.resynced,
                display_fps: report.display_fps,
                frame_count: report.frame_count,
                audio_queued_frames: report.audio.queued_frames,
                audio_underruns: report.audio.underruns as u32,
                hardware_frame: false,
            },
            Err(error) => {
                log::debug!("tick failed: {error}");
                TickTelemetry {
                    steps: 0,
                    dropped: 1,
                    presented: false,
                    resynced: false,
                    display_fps: 0.0,
                    frame_count: guard.frame_count(),
                    audio_queued_frames: 0,
                    audio_underruns: 0,
                    hardware_frame: false,
                }
            }
        }
    }

    /// Drops the drawable-dependent graphics state on backgrounding.
    ///
    /// The `MTLDevice` survives; drawables do not, and the system may ask for GPU memory
    /// back. A hardware-rendered core is told through its `context_destroy` so it can
    /// release its own resources, and rebuilds them in `context_reset` afterwards.
    ///
    /// Ordering is the part worth being careful about: the caller must checkpoint *before*
    /// this, because a core keeping emulated GPU state in host GPU resources loses it here
    /// and a state written afterwards is quietly incomplete.
    pub fn release_graphics(&self) {
        let mut guard = self.lock();
        guard.pause();
        if let Some(renderer) = guard.renderer_mut() {
            // The framebuffer texture is the large allocation and the one the system is
            // most likely to want back; the device, instance and pipelines survive, because
            // rebuilding those on every foreground transition would cost a visible stall.
            renderer.release_frame_target();
            renderer.invalidate_surface();
        }
        log::info!("graphics released for backgrounding");
    }

    pub fn restore_graphics(&self) {
        let mut guard = self.lock();
        if let Some(renderer) = guard.renderer_mut() {
            // Reconfigure before the next present rather than after the first one fails.
            // `resize` alone would not do it: the drawable size is usually unchanged across
            // a background/foreground cycle, so `resize` returns early while the drawables
            // behind the layer are gone.
            renderer.invalidate_surface();
        }
        log::info!("graphics restored; swapchain reconfigures on the next frame");
    }

    /// Frames emulated in the current session.
    pub fn frame_count(&self) -> u64 {
        self.lock().frame_count()
    }

    pub fn current_core_id(&self) -> Option<String> {
        self.lock().current_core_id().map(str::to_string)
    }

    pub fn current_content_id(&self) -> Option<String> {
        self.lock().current_content_id().map(str::to_string)
    }

    pub fn resident_core_count(&self) -> u32 {
        self.lock().resident_core_count() as u32
    }

    // -------------------------------------------------------------- declaration

    /// Registers a core the app can later load.
    ///
    /// Required before [`Self::load_native_core`], which refuses anything undeclared rather
    /// than inventing a descriptor — the declaration is the authority on geometry, and a
    /// session started against invented numbers would silently correct itself one frame in.
    pub fn declare_core(&self, declaration: CoreDeclaration) -> Result<(), EngineError> {
        let pixel_format = crate::frame::PixelFormat::from_u32(declaration.pixel_format)
            .ok_or_else(|| EngineError::Other {
                reason: format!("unknown pixel format {}", declaration.pixel_format),
            })?;

        let descriptor = crate::cores::CoreDescriptor {
            id: declaration.id,
            display_name: declaration.display_name,
            systems: declaration.systems,
            geometry: crate::frame::FrameGeometry::new(
                declaration.base_width,
                declaration.base_height,
                declaration.aspect_ratio,
            )
            .with_max(declaration.max_width, declaration.max_height),
            target_fps: declaration.target_fps,
            audio_sample_rate: declaration.audio_sample_rate,
            pixel_format,
            module_url: declaration.module_path,
            priority: declaration.priority,
        };

        self.lock().declare_core(descriptor);
        Ok(())
    }

    /// `"declared" | "loaded" | "bound" | "failed"`, or `None` if the id is unknown.
    pub fn core_state(&self, core_id: String) -> Option<String> {
        self.lock()
            .core_state(&core_id)
            .map(|state| state.as_str().to_string())
    }

    // ------------------------------------------------------------- native cores

    /// Loads a libretro core from a shared library in the app bundle and declares it.
    ///
    /// Step 10 loads `libcontinuum_switch.dylib` — the C++ wrapper around a stub engine —
    /// which is why this exists before any real core does.
    ///
    /// `system_dir` and `save_dir` are handed straight to the core through
    /// `GET_SYSTEM_DIRECTORY` and `GET_SAVE_DIRECTORY`. The web build refuses both, because
    /// a browser has no filesystem to offer; natively they are how a core finds its keys and
    /// writes its savedata, so they are required rather than optional for Switch content.
    #[cfg(feature = "native-core")]
    pub fn load_native_core(
        &self,
        core_id: String,
        library_path: String,
        system_dir: Option<String>,
        save_dir: Option<String>,
    ) -> Result<(), EngineError> {
        use crate::cores::native_core::NativeLibretroCore;

        let mut guard = self.lock();
        // Declared first, always. Fabricating a descriptor here would mean the engine
        // starting a session against invented geometry and then silently correcting itself
        // once the core reported the truth — the manifest is the authority, and a core that
        // was never declared is a configuration bug worth naming.
        let descriptor = guard.core_descriptor(&core_id).cloned().ok_or_else(|| {
            EngineError::CoreUnavailable {
                core_id: core_id.clone(),
                reason: "not declared; declare it from the manifest first".into(),
            }
        })?;

        // SAFETY: the path comes from `Bundle.main`, so the library is co-signed and
        // shipped with the app. iOS refuses to `dlopen` anything else regardless, but the
        // caller's obligation is worth naming at the boundary rather than assuming.
        let core = unsafe {
            NativeLibretroCore::load(
                descriptor,
                std::path::Path::new(&library_path),
                system_dir.as_deref(),
                save_dir.as_deref(),
            )
        }?;
        guard.attach_core(&core_id, Box::new(core))?;
        Ok(())
    }

    /// Starts a session.
    ///
    /// `rom` may be empty for content the core declared `need_fullpath` for — which Switch
    /// containers do, because an XCI is tens of gigabytes and the engine mounts it rather
    /// than reading it. In that case `filename` is the path and carries the whole payload.
    pub fn launch(
        &self,
        core_id: String,
        content_id: String,
        rom: Vec<u8>,
        filename: String,
    ) -> Result<(), EngineError> {
        // `ContentHint::from_filename` splits the extension off, which is what real cores
        // use to resolve their content-info overrides — and for Genesis Plus GX was the
        // difference between a Master System cart booting as a Master System and as a Mega
        // Drive. The same parsing serves the Switch container extensions.
        let hint = crate::cores::ContentHint::from_filename(&filename);
        Ok(self.lock().launch(&core_id, &content_id, &rom, &hint)?)
    }

    // ------------------------------------------------------------------- session

    pub fn pause(&self) {
        self.lock().pause();
    }

    /// `now_millis` is `CADisplayLink.targetTimestamp * 1000`.
    ///
    /// The pacer takes a timestamp rather than reading a clock — which is what made the
    /// engine portable in the first place — so resuming has to say *when*, or the first
    /// frame after a pause is paced against a stale reference and the session catches up in
    /// a burst.
    pub fn resume(&self, now_millis: f64) {
        self.lock().resume(now_millis);
    }

    pub fn reset(&self) -> Result<(), EngineError> {
        Ok(self.lock().reset()?)
    }

    pub fn stop(&self) {
        self.lock().stop();
    }

    // ---------------------------------------------------------------- save state

    pub fn save_state(&self) -> Result<Vec<u8>, EngineError> {
        Ok(self.lock().save_state()?)
    }

    pub fn load_state(&self, data: Vec<u8>) -> Result<(), EngineError> {
        Ok(self.lock().load_state(&data)?)
    }

    // -------------------------------------------------------------------- cheats

    /// `Vec<bool>` rather than the wasm build's `&[u8]`.
    ///
    /// That byte array exists only because wasm-bindgen has no bool-slice ABI. UniFFI does,
    /// so the workaround should not be copied into Swift — someone would later wonder why
    /// it was there.
    pub fn apply_cheats(&self, codes: Vec<String>, enabled: Vec<bool>) -> Result<u32, EngineError> {
        let flags: Vec<u8> = enabled.into_iter().map(u8::from).collect();
        Ok(self.lock().apply_cheats(codes, &flags)? as u32)
    }

    pub fn clear_cheats(&self) -> Result<(), EngineError> {
        Ok(self.lock().clear_cheats()?)
    }

    pub fn cheats_supported(&self) -> bool {
        self.lock().cheats_supported()
    }

    pub fn active_cheat_count(&self) -> u32 {
        self.lock().active_cheat_count() as u32
    }

    // -------------------------------------------------------------- core options

    /// Structured, unlike the wasm facade's flat string vector.
    ///
    /// `wasm.rs` flattens to groups of four because wasm-bindgen would otherwise generate a
    /// wrapper class per element. UniFFI generates a Swift struct, so the list crosses in
    /// the shape the UI actually wants.
    pub fn core_options(&self) -> Vec<CoreOptionRecord> {
        self.lock()
            .core_options()
            .into_iter()
            .map(|option| CoreOptionRecord {
                key: option.key,
                label: option.label,
                value: option.value,
                values: option.values,
            })
            .collect()
    }

    pub fn set_core_option(&self, key: String, value: String) -> Result<(), EngineError> {
        Ok(self.lock().set_core_option(&key, &value)?)
    }

    // --------------------------------------------------------------------- input

    pub fn apply_gamepad(&self, port: u32, buttons: Vec<bool>, axes: Vec<f32>) {
        self.lock().apply_gamepad(port as usize, &buttons, &axes);
    }

    /// As [`ContinuumEngine::apply_gamepad`], but says which input layer the poll came from.
    ///
    /// **Use this rather than `apply_gamepad` as soon as there is more than one thing
    /// producing input, and that is not a style preference.** The engine keeps one layer per
    /// source and merges them when the core reads input, so a poll REPLACES its own layer
    /// rather than adding to it. Send the on-screen pad and a physical controller to the same
    /// layer and the quiet one wins whichever wrote last: the overlay reporting "nothing
    /// held" sixty times a second would cancel out a real controller, and the symptom is a
    /// controller that only works while no finger is near the glass.
    ///
    /// Buttons are in W3C standard gamepad order, the same as `apply_gamepad`, which is NOT
    /// libretro order. See `input/gamepad.rs`.
    pub fn apply_gamepad_from(
        &self,
        port: u32,
        source: InputSource,
        buttons: Vec<bool>,
        axes: Vec<f32>,
    ) {
        self.lock()
            .apply_gamepad_from(port as usize, source.into(), &buttons, &axes);
    }

    /// Moves a pointer, which is how a touch screen reaches a core.
    ///
    /// **`x` and `y` are fractions of the WHOLE framebuffer, `0.0` to `1.0`, origin top left.**
    /// Not pixels, and not one screen: on the Nintendo DS the framebuffer is both screens stacked,
    /// so the touch screen is the lower half and its top edge is `y = 0.5`. The host does that
    /// conversion because only the host knows where on the display it drew the picture, and the
    /// engine would have to guess.
    ///
    /// The coordinates are REMEMBERED when `pressed` is false rather than cleared, so a release
    /// leaves the stylus where it was lifted. Clearing them would put a jump to the top-left corner
    /// at the end of every stroke, which a game reads as a real input.
    ///
    /// Call it on the `touch` source for an on-screen stylus. A pressed pointer on any layer beats
    /// an unpressed one; two positions are never averaged, because two fingers in different places
    /// have no meaningful midpoint.
    pub fn apply_pointer(&self, port: u32, source: InputSource, x: f32, y: f32, pressed: bool) {
        self.lock()
            .set_pointer(port as usize, source.into(), x, y, pressed);
    }

    /// Releases one layer, leaving the others untouched.
    ///
    /// For a controller being unplugged, or the on-screen pad going away when the player
    /// leaves. Without it, whatever that layer was holding stays held for the rest of the
    /// session: a controller disconnected mid-press leaves its button down forever, because
    /// no further poll is ever coming to say otherwise.
    pub fn release_input_source(&self, source: InputSource) {
        self.lock().release_input_source(source.into());
    }

    pub fn connected_pads(&self) -> u32 {
        self.lock().connected_pads() as u32
    }

    // --------------------------------------------------------------------- audio

    /// Tells the engine the rate the device actually runs at, so resampling happens here.
    ///
    /// The peer of `wasm.rs`'s `setOutputSampleRate`, and it exists for the same reason: the
    /// hardware rate is not knowable until the platform's audio graph is up. `EmulatorBridge`
    /// defaults to 48000 because something has to be assumed before then, and on iOS that
    /// assumption is wrong often enough to matter. `AVAudioSession` reports 48000 on a modern
    /// iPhone speaker, 44100 on some Bluetooth routes, and a headset can negotiate something
    /// else again. So Swift reads the session's real rate after activating it and reports it
    /// here.
    ///
    /// Resampling stays on this side of the boundary deliberately. `audio/resample.rs` already
    /// reconciles a core's rate with an output rate, is already unit tested, and is already
    /// the code the browser build has been running for a year. A second implementation in
    /// Swift would be a second thing to keep correct, and getting it subtly wrong sounds like
    /// a slightly out of tune game rather than like a bug.
    ///
    /// A rate change drops whatever is queued, because those samples were resampled for the
    /// old rate and playing them at the new one would pitch shift the tail.
    pub fn set_output_sample_rate(&self, rate: u32) {
        self.lock().set_output_sample_rate(rate);
    }

    /// The rate the ring is currently being filled at.
    pub fn output_sample_rate(&self) -> u32 {
        self.lock().output_sample_rate()
    }

    /// Drains up to `max_frames` stereo frames of queued PCM, interleaved as L, R, L, R.
    ///
    /// Returns however many samples were there, which is normally fewer than asked for and is
    /// not a failure: an empty vector means the ring is empty, nothing more. The count is
    /// always a multiple of two, because the ring is written a whole frame at a time.
    ///
    /// **By value, because UniFFI copies.** There is no way to hand Swift a pointer into the
    /// ring through this boundary, and the shapes that avoid the copy all end somewhere worse:
    /// a callback would run Swift code while this file's `Mutex` is held, and an out-parameter
    /// does not exist in UniFFI's type system. So the cost is one allocation and one copy per
    /// tick, sized to what is actually queued rather than to `max_frames`, which at 48 kHz is
    /// about 6 KB of PCM sixteen times a second. That is bought with something worth much
    /// more: the platform's real-time render thread never touches this lock.
    ///
    /// **Called from the display link, immediately after `tick`.** Never from an audio render
    /// callback. The display link already holds this lock for the whole of each tick, so a
    /// render callback taking it too would be a real-time thread blocking on the main thread,
    /// which is heard as clicks and dropouts rather than seen as a stall. The Swift side
    /// pushes into its own lock-free ring here and pulls from that ring on the audio thread.
    pub fn drain_audio(&self, max_frames: u32) -> Vec<f32> {
        let ceiling = max_frames.min(MAX_DRAIN_FRAMES) as usize * CHANNELS;
        if ceiling == 0 {
            return Vec::new();
        }
        let mut guard = self.lock();
        // Sized to what is queued rather than to the ceiling, so a caller that always asks for
        // the maximum does not churn a 32 KB allocation per tick to return 6 KB of it. Reading
        // the depth first is free: the guard is already held, so nothing can push in between.
        let queued = guard.audio_stats().queued_frames as usize * CHANNELS;
        let wanted = ceiling.min(queued);
        if wanted == 0 {
            return Vec::new();
        }
        let mut out = vec![0.0f32; wanted];
        let written = guard.drain_audio(&mut out);
        // Muting drains nothing at all, so this is a truncate to zero rather than a no-op.
        out.truncate(written);
        out
    }

    /// Everything the ring knows about itself. See [`AudioStatsSnapshot`].
    pub fn audio_stats(&self) -> AudioStatsSnapshot {
        let guard = self.lock();
        let stats = guard.audio_stats();
        let spec = guard.audio_spec();
        AudioStatsSnapshot {
            queued_frames: stats.queued_frames,
            capacity_frames: stats.capacity_frames,
            overruns: stats.overruns,
            underruns: stats.underruns,
            frames_submitted: stats.frames_submitted,
            frames_drained: stats.frames_drained,
            source_rate: spec.source_rate,
            output_rate: spec.output_rate,
            channels: spec.channels,
        }
    }

    /// Drops everything queued.
    ///
    /// For the host rebuilding its audio graph, which is what a route change forces: the
    /// buffered tail was resampled for a device configuration that no longer exists, and the
    /// Swift ring is thrown away at the same moment. Flushing both keeps the two ends
    /// agreeing about how much latency there is, which is the number the HUD reports.
    pub fn flush_audio(&self) {
        self.lock().flush_audio();
    }

    /// Sets output gain. `0.0` is silence, `1.0` is the core's own level.
    ///
    /// Applied inside the engine rather than on the platform's mixer for one reason worth
    /// stating: it is the same volume on every platform this engine ever runs on, including
    /// the Android build that does not exist yet. Out-of-range values are clamped rather
    /// than rejected, and the change is ramped across the next drained block so that
    /// dragging a slider does not click. Safe to call every frame.
    pub fn set_volume(&self, volume: f32) {
        self.lock().set_volume(volume);
    }

    pub fn volume(&self) -> f32 {
        self.lock().volume()
    }

    /// Silences output without stopping emulation.
    ///
    /// Distinct from `set_volume(0.0)`: muting also drops the queued backlog, so unmuting
    /// resumes at the present moment rather than replaying the second of audio that
    /// accumulated while it was silent.
    pub fn set_muted(&self, muted: bool) {
        self.lock().set_muted(muted);
    }

    pub fn is_muted(&self) -> bool {
        self.lock().is_muted()
    }

    // --------------------------------------------------------------------- video

    /// Sets how the image is fitted to the screen. See [`ScaleModeOption`].
    ///
    /// A 32-byte uniform write, so this is free to call whenever and takes effect on the
    /// next presented frame. Remembered by the engine across launches, so it does not need
    /// re-applying every time a game starts.
    pub fn set_scale_mode(&self, mode: ScaleModeOption) {
        self.lock().set_scale_mode(mode.into());
    }

    pub fn scale_mode(&self) -> ScaleModeOption {
        self.lock().scale_mode().into()
    }

    /// Sets pixel sampling. See [`ScaleFilterOption`].
    ///
    /// Rebuilds one bind group, which is cheap and is not a pipeline rebuild. Safe before a
    /// game launches, safe while one runs, and remembered across launches.
    pub fn set_filter(&self, filter: ScaleFilterOption) {
        self.lock().set_filter(filter.into());
    }

    pub fn filter(&self) -> ScaleFilterOption {
        self.lock().filter().into()
    }

    // --------------------------------------------------------------------- speed

    /// Sets the speed multiplier. `1.0` is native.
    ///
    /// **The achievable ceiling is lower than the accepted one, and a caller building a UI
    /// needs to know it.** The engine accepts `0.05` to `16.0`, but it also refuses to run
    /// more than four core frames in one display tick, which on a 60 Hz screen puts the real
    /// limit near 4x no matter what is asked for. Beyond that the surplus is forfeited and
    /// appears as a climbing `dropped` count in [`TickTelemetry`] rather than as more speed,
    /// so offering 8x in a menu would be offering something the engine cannot deliver.
    ///
    /// Audio follows the speed change and stays continuous, shifting up in pitch the way
    /// fast-forward has always sounded, rather than being chopped by ring overruns.
    pub fn set_speed(&self, speed: f64) {
        self.lock().set_speed(speed);
    }

    /// The multiplier actually in force, after clamping.
    pub fn speed(&self) -> f64 {
        self.lock().speed()
    }

    // -------------------------------------------------------------------- rewind

    /// Sets the rewind memory ceiling in bytes. `0` switches rewind off and frees the tape.
    ///
    /// Expressed in memory rather than in seconds because save-state sizes differ by two
    /// orders of magnitude between the systems Continuum runs, so the same budget is minutes
    /// of NES and seconds of PlayStation. Read [`ContinuumEngine::rewind_stats`] to show a
    /// user what their budget actually bought them on the game in front of them.
    pub fn set_rewind_budget_bytes(&self, budget_bytes: u64) {
        // usize on every target this ships to is 64-bit, but the cast is saturating rather
        // than lossy so a 32-bit build would clamp instead of wrapping to a tiny budget.
        let budget = usize::try_from(budget_bytes).unwrap_or(usize::MAX);
        self.lock().set_rewind_budget_bytes(budget);
    }

    /// Core frames between snapshots. Lower is finer-grained rewind and more work per
    /// second; the default is 6, which is ten snapshots a second at 60 fps.
    pub fn set_rewind_interval(&self, frames: u32) {
        self.lock().set_rewind_interval(frames);
    }

    /// Everything the tape knows about itself. See [`RewindStatsSnapshot`].
    pub fn rewind_stats(&self) -> RewindStatsSnapshot {
        let guard = self.lock();
        let (snapshots, bytes, evicted) = guard.rewind_stats();
        RewindStatsSnapshot {
            snapshots,
            bytes,
            evicted,
            budget_bytes: guard.rewind_budget_bytes() as u64,
            interval_frames: guard.rewind_interval(),
        }
    }

    /// Starts or stops rewinding. Set on button press, clear on release.
    ///
    /// **This is the whole of the rewind UI contract, and the reason there is nothing to call
    /// per frame.** While set, the engine's own tick goes backwards instead of forwards, so
    /// the host keeps calling `tick` exactly as it always does. Driving it from Swift
    /// instead, by rewinding and then ticking, would advance the core and then throw that
    /// frame away sixty times a second, and the picture would judder rather than reverse.
    pub fn set_rewinding(&self, rewinding: bool) {
        self.lock().set_rewinding(rewinding);
    }

    pub fn is_rewinding(&self) -> bool {
        self.lock().is_rewinding()
    }

    /// Steps one snapshot backwards, returning `false` when the tape is empty.
    ///
    /// An empty tape is not an error - it is simply the start of recorded history, which a
    /// held rewind button will reach - so this returns a flag rather than throwing.
    ///
    /// Exported for a "step back once" control rather than for held-button rewind, which
    /// should use [`ContinuumEngine::set_rewinding`] instead.
    pub fn rewind_step(&self) -> Result<bool, EngineError> {
        Ok(self.lock().rewind_step()?)
    }

    /// Size in bytes of one save state for the running core, or `0` if it has none.
    ///
    /// Exposed so a settings screen can say what a rewind budget is worth on the game
    /// actually running, instead of quoting an average across systems that is wrong for all
    /// of them. Also the fourth and last of the checks a stored save state should be matched
    /// against before being loaded; see [`ContinuumEngine::core_version`].
    pub fn save_state_size(&self) -> u64 {
        self.lock().state_size() as u64
    }

    /// Captures what is on screen as RGBA8. See [`CapturedFrame`].
    ///
    /// `width` and `height` of `0` mean the current surface size. The capture goes through the
    /// same pipeline as a present, so the scale mode, the filter and the aspect ratio all apply:
    /// it is a picture of the game as displayed, not the core's raw framebuffer.
    ///
    /// **Blocks for a few milliseconds while the GPU finishes.** A readback cannot be instant,
    /// because the copy has to complete before the bytes exist. Call it from a deliberate user
    /// action, never from a frame path, and note it works on a paused session: the engine
    /// refreshes from the core before capturing, so a paused game captures the frame it is sitting
    /// on rather than whatever was last presented.
    pub fn capture_frame(&self, width: u32, height: u32) -> Result<CapturedFrame, EngineError> {
        let (width, height, rgba) = self.lock().capture_rgba(width, height)?;
        Ok(CapturedFrame {
            width,
            height,
            rgba,
        })
    }

    /// Whether this app can write instructions into memory and execute them, as a sentence.
    ///
    /// **Ask this once, at startup, and show the answer.** It is the question every N64 core
    /// depends on and the one this project has never answered: an N64 interpreter is far too slow
    /// to be playable, so N64 needs a recompiler, and a recompiler needs a working JIT. The
    /// entitlements have been present since the first build and have never been exercised, and on
    /// a sideloaded app they are only as good as the signature that carried them, so this is a
    /// property of the installed build rather than of the source.
    ///
    /// Deliberately not part of any core. See [`crate::jit_probe`] for why switching a core's
    /// recompiler on would not have answered it: the PlayStation core has no Apple JIT support to
    /// enable, so forcing it would have failed in a way that looked like a broken core.
    ///
    /// Takes no lock and touches no session. Costs one page mapped and unmapped.
    pub fn jit_probe(&self) -> String {
        crate::jit_probe::describe()
    }

    /// The running core's own version string, or `None` if it does not report one.
    ///
    /// **Record this beside every save state you store, and refuse to load a state whose
    /// recorded version differs from this.** A libretro save state is an opaque dump of the
    /// core's internal structs and `retro_unserialize` is not versioned, so a state written by
    /// a different build of the same core can be ACCEPTED and leave the emulated machine
    /// quietly corrupt, to crash later somewhere with no visible connection to the load. The
    /// engine cannot tell the difference, so the host has to, and the four things worth storing
    /// are the core id, this version, the exact byte length and the game it belongs to.
    pub fn core_version(&self) -> Option<String> {
        self.lock().core_version()
    }
}

impl ContinuumEngine {
    /// Reads the backend handles out of the attached renderer.
    ///
    /// Not exported: `MetalHandles` is a graphics-layer type, and UniFFI would need a record
    /// for it. Swift wants two integers, so it gets two accessors.
    #[cfg(target_vendor = "apple")]
    fn metal_handles(&self) -> Option<crate::gfx::metal::MetalHandles> {
        self.lock()
            .renderer()
            .and_then(crate::gfx::metal::metal_handles)
    }

    /// One place that decides what a poisoned lock means.
    ///
    /// Recovered rather than propagated: a panic in one call must not make the engine
    /// permanently unusable, and every method here re-reads state from the bridge anyway,
    /// so there is no invariant spanning two calls for the poison to have broken.
    fn lock(&self) -> std::sync::MutexGuard<'_, EmulatorBridge> {
        match self.inner.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                log::warn!("engine lock was poisoned by a previous panic; recovering");
                poisoned.into_inner()
            }
        }
    }
}
