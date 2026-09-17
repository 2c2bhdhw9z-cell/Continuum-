//! The Swift-facing facade. Peer of [`crate::wasm`], and held to the same rule: nothing in
//! here is logic. If a function in this file does anything other than convert a type,
//! acquire the lock and delegate, then something has leaked out of the engine and belongs
//! back in `bridge.rs`.
//!
//! ## Why UniFFI, and why the copy does not matter
//!
//! UniFFI copies `Vec<u8>` across the boundary, which is its one real cost. It does not
//! matter here because **nothing on the per-frame data path crosses this boundary**. Frames
//! go core → staging → Metal entirely inside Rust; audio goes core → ring → CoreAudio
//! entirely inside Rust. What crosses is what crosses `wasm.rs` today — launch, pause, save
//! state, settings — a few dozen calls a minute. For that traffic, generated correctness
//! beats hand-written speed.
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

use crate::bridge::EmulatorBridge;
use crate::error::BridgeError;

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

    /// Adopts the Metal objects Swift created.
    ///
    /// Three opaque handles rather than typed pointers, because UniFFI has no pointer type
    /// and these cross exactly once at startup. The important half is the contract, not the
    /// signature: **Swift creates one `MTLDevice` and one `MTLCommandQueue` and passes them
    /// here.** Nothing in this process calls `MTLCreateSystemDefaultDevice()` for itself,
    /// because an iPhone has one GPU and every layer — wgpu, MoltenVK, ANGLE — must share
    /// the same device for a texture to be shareable at all.
    pub fn attach_metal(
        &self,
        device: u64,
        queue: u64,
        layer: u64,
        width: u32,
        height: u32,
    ) -> Result<(), EngineError> {
        let _ = (device, queue, layer, width, height);
        // Step 1 of the Phase 5 sequence is what fills this in: building a wgpu device from
        // an injected MTLDevice is the one unknown in the graphics plan, and it is
        // deliberately answered before anything depends on the answer.
        let _guard = self.lock();
        Err(EngineError::Graphics {
            reason: "attach_metal is implemented by Phase 5 step 1 (wgpu device adoption)".into(),
        })
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
        // Phase 5 step 1 attaches the renderer, and this is where its teardown goes.
        log::info!("graphics released for backgrounding");
    }

    pub fn restore_graphics(&self) {
        log::info!("graphics restored");
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
}

impl ContinuumEngine {
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
