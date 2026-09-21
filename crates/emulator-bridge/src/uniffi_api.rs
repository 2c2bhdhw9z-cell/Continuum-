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
