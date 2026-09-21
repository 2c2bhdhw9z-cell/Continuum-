//! `EmulatorBridge` — the engine the UI talks to, and the only thing that does.
//!
//! Everything stateful lives here: which cores exist, which one is running, where
//! input goes, how audio is buffered, when frames are presented. The UI is reduced
//! to two responsibilities: forward events in, call [`EmulatorBridge::tick`] once
//! per animation frame.
//!
//! That split is what makes Phase 2 a UI port instead of a rewrite. This file
//! contains no `wasm_bindgen`, no `web_sys`, no JS types — the facades in
//! `wasm.rs` (Phase 1) and the planned UniFFI layer (Phase 2) are thin wrappers
//! over this API, so both platforms inherit the same behaviour rather than
//! re-implementing it.
//!
//! Single-threaded and single-loop by construction: `tick` runs input, core steps,
//! audio submission and the GPU present in that order, and nothing here spawns a
//! thread, a worker or a timer.

use crate::audio::{AudioSink, AudioSpec, AudioStats, NullAudioSink, RingAudioSink};
use crate::cores::{ContentHint, CoreDescriptor, CoreRegistry, CoreState, EmulatorCore};
use crate::error::BridgeError;
use crate::gfx::{Renderer, ScaleFilter, ScaleMode};
use crate::input::{Button, GamepadBridge, PadKind, PadSource};
use crate::timing::FramePacer;

/// Video frames of audio to buffer. Three is the usual compromise between
/// robustness against a slow tick and audible input-to-sound latency.
const AUDIO_LATENCY_FRAMES: usize = 3;

/// What happens to a core when its session ends.
///
/// The default is [`CoreRetention::Drop`], which is the right default for an
/// all-in-one emulator: cores are large (2 MB of module plus 16–32 MB of working
/// memory), a user browsing their library is not using any of it, and Phase 2's iOS
/// target kills processes on memory pressure rather than paging. Re-instantiating on
/// the next launch costs a few hundred milliseconds and the module itself comes from
/// the Cache API, not the network.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreRetention {
    /// Free the core when the session ends.
    Drop,
    /// Keep it instantiated for a fast relaunch. Only sensible for a single-system
    /// build, or a desktop with memory to spare.
    KeepWarm,
}

impl CoreRetention {
    pub const fn as_str(self) -> &'static str {
        match self {
            CoreRetention::Drop => "drop",
            CoreRetention::KeepWarm => "warm",
        }
    }
}

/// Coarse engine state, mirrored in the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BridgeStatus {
    /// No renderer yet: `init_gpu` has not run or WebGPU is unavailable.
    Uninitialised,
    /// GPU ready, no game loaded. The library-browsing state.
    Idle,
    Running,
    Paused,
}

impl BridgeStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            BridgeStatus::Uninitialised => "uninitialised",
            BridgeStatus::Idle => "idle",
            BridgeStatus::Running => "running",
            BridgeStatus::Paused => "paused",
        }
    }
}

/// One tick's worth of telemetry. Cheap to produce; drives the debug HUD and lets
/// the host decide how much audio to pump.
#[derive(Debug, Clone, Copy, Default)]
pub struct TickReport {
    /// Core steps executed this tick. `0` is normal on a high-refresh display.
    pub steps: u32,
    /// Steps abandoned because the catch-up ceiling was hit.
    pub dropped: u32,
    pub presented: bool,
    /// A stall was detected and emulated time re-anchored.
    pub resynced: bool,
    pub display_fps: f64,
    pub frame_count: u64,
    pub audio: AudioStats,
}

struct Session {
    core_id: String,
    content_id: String,
    core: Box<dyn EmulatorCore>,
    paused: bool,
    /// The cheat list currently applied, kept so it can be re-applied after a reset or
    /// a state load. Lives on the session rather than the bridge because cheats belong
    /// to a game: ending the session is what forgets them.
    cheats: Vec<Cheat>,
}

/// One cheat as the user entered it, plus whether it is switched on.
#[derive(Debug, Clone)]
struct Cheat {
    code: String,
    enabled: bool,
}

pub struct EmulatorBridge {
    registry: CoreRegistry,
    renderer: Option<Renderer>,
    session: Option<Session>,
    gamepads: GamepadBridge,
    sink: Box<dyn AudioSink>,
    pacer: FramePacer,
    /// Device sample rate, learned from the host once `AudioContext` exists.
    output_sample_rate: u32,
    muted: bool,
    /// Reused across `save_state` calls so snapshotting does not allocate.
    state_scratch: Vec<u8>,
    retention: CoreRetention,
}

impl Default for EmulatorBridge {
    fn default() -> Self {
        Self::new()
    }
}

impl EmulatorBridge {
    pub fn new() -> Self {
        Self {
            registry: CoreRegistry::new(),
            renderer: None,
            session: None,
            gamepads: GamepadBridge::new(),
            // Until a session starts there is nothing to buffer; a null sink keeps
            // the tick shape identical rather than making audio conditional.
            sink: Box::new(NullAudioSink::new()),
            pacer: FramePacer::new(60.0),
            output_sample_rate: 48_000,
            muted: false,
            state_scratch: Vec::new(),
            retention: CoreRetention::Drop,
        }
    }

    // ---------------------------------------------------------------- lifecycle

    /// Installs the renderer built by the platform layer.
    pub fn attach_renderer(&mut self, renderer: Renderer) {
        log::info!("renderer attached: {}", renderer.adapter_summary());
        self.renderer = Some(renderer);
    }

    pub fn has_renderer(&self) -> bool {
        self.renderer.is_some()
    }

    pub fn status(&self) -> BridgeStatus {
        match (&self.renderer, &self.session) {
            (None, _) => BridgeStatus::Uninitialised,
            (Some(_), None) => BridgeStatus::Idle,
            (Some(_), Some(s)) if s.paused => BridgeStatus::Paused,
            (Some(_), Some(_)) => BridgeStatus::Running,
        }
    }

    pub fn renderer(&self) -> Option<&Renderer> {
        self.renderer.as_ref()
    }

    pub fn renderer_mut(&mut self) -> Option<&mut Renderer> {
        self.renderer.as_mut()
    }

    // ----------------------------------------------------------------- registry

    /// Declares a core. Metadata only — no fetch, no instantiation, no memory
    /// beyond the descriptor itself.
    pub fn declare_core(&mut self, descriptor: CoreDescriptor) {
        log::debug!("declared core '{}'", descriptor.id);
        self.registry.declare(descriptor);
    }

    pub fn core_state(&self, core_id: &str) -> Option<CoreState> {
        self.registry.state(core_id)
    }

    /// The declaration (or, once loaded, the core's own) descriptor.
    pub fn core_descriptor(&self, core_id: &str) -> Option<&CoreDescriptor> {
        self.registry.descriptor(core_id)
    }

    pub fn core_for_system(&self, system_id: &str) -> Option<&CoreDescriptor> {
        self.registry.core_for_system(system_id)
    }

    /// Every core that can run `system_id`, best first. What the UI offers as
    /// alternatives when a system has more than one option.
    pub fn cores_for_system(&self, system_id: &str) -> Vec<&CoreDescriptor> {
        self.registry.cores_for_system(system_id)
    }

    /// The core to launch for `system_id`, honouring a user preference if it applies.
    pub fn resolve_core_for_system(
        &self,
        system_id: &str,
        preferred: Option<&str>,
    ) -> Option<&CoreDescriptor> {
        self.registry.resolve_core_for_system(system_id, preferred)
    }

    pub fn resident_core_count(&self) -> usize {
        self.registry.resident_ids().count()
    }

    /// Instantiates a fetched core module. Called from the launch path only.
    pub fn attach_core_module(&mut self, core_id: &str, bytes: &[u8]) -> Result<(), BridgeError> {
        self.registry.attach_module(core_id, bytes)
    }

    /// Installs a core built by the platform layer (a real libretro instance).
    pub fn attach_core(
        &mut self,
        core_id: &str,
        core: Box<dyn EmulatorCore>,
    ) -> Result<(), BridgeError> {
        self.registry.attach_core(core_id, core)
    }

    pub fn unload_core(&mut self, core_id: &str) -> Result<(), BridgeError> {
        self.registry.unload(core_id)
    }

    // ------------------------------------------------------------------ session

    /// Starts a session with an already-attached core.
    ///
    /// Requires a renderer: launching into a void would "work" for minutes and
    /// then fail confusingly, so it fails immediately instead.
    pub fn launch(
        &mut self,
        core_id: &str,
        content_id: &str,
        content: &[u8],
        hint: &ContentHint,
    ) -> Result<(), BridgeError> {
        if self.renderer.is_none() {
            return Err(BridgeError::NoRenderer);
        }

        // End any existing session first, which also frees its core under the default
        // retention policy.
        self.stop();

        // Free every other resident core *before* instantiating this one, so peak
        // memory during a system switch is one core, not two.
        let freed = self.registry.unload_all_except(Some(core_id));
        if freed > 0 {
            log::info!("freed {freed} resident core(s) before launching '{core_id}'");
        }

        let mut core = self.registry.take_for_session(core_id)?;
        if let Err(err) = core.load_content(content, hint) {
            // Hand the core back; a rejected ROM must not cost us the loaded core.
            self.registry.return_from_session(core_id, core);
            return Err(err);
        }

        // Read *after* load_content: a real libretro core only reports its final
        // geometry, refresh rate and sample rate once content is loaded.
        let descriptor = core.descriptor().clone();
        self.pacer = FramePacer::new(descriptor.target_fps);
        self.sink = Box::new(RingAudioSink::new(
            descriptor.audio_sample_rate,
            self.output_sample_rate,
            descriptor.target_fps,
            AUDIO_LATENCY_FRAMES,
        ));
        if let Some(renderer) = &mut self.renderer {
            renderer.set_aspect_ratio(descriptor.geometry.aspect_ratio);
        }
        self.gamepads.release_all();
        self.state_scratch = Vec::with_capacity(core.state_size());

        log::info!(
            "session started: core '{}', content '{}' ({} bytes), {}x{} @ {:.2} fps, {} Hz",
            core_id,
            content_id,
            content.len(),
            descriptor.geometry.base_width,
            descriptor.geometry.base_height,
            descriptor.target_fps,
            descriptor.audio_sample_rate
        );

        self.session = Some(Session {
            core_id: core_id.to_string(),
            content_id: content_id.to_string(),
            core,
            paused: false,
            // A fresh core has no cheats. The front end pushes the game's saved list
            // after launch, which is also what makes cheats survive a relaunch.
            cheats: Vec::new(),
        });
        Ok(())
    }

    /// Ends the session and returns its core to the registry, kept warm so
    /// relaunching the same system does not re-fetch the module.
    pub fn stop(&mut self) {
        if let Some(session) = self.session.take() {
            let core_id = session.core_id.clone();
            log::info!(
                "session ended: '{}' after {} frames",
                session.content_id,
                session.core.frame_count()
            );

            // The core goes back to the registry first so the slot is never left
            // `Bound`, then the retention policy decides whether it survives. Dropping
            // it here runs `WasmCore::drop` → retro_unload_game → retro_deinit → the
            // core module's last handle released.
            self.registry.return_from_session(&core_id, session.core);
            if self.retention == CoreRetention::Drop {
                if let Err(err) = self.registry.unload(&core_id) {
                    log::warn!("could not unload core '{core_id}': {err}");
                }
            }
        }

        // Teardown order matters: audio first (so nothing is still being pumped), then
        // input, then GPU resources.
        self.sink = Box::new(NullAudioSink::new());
        self.gamepads.release_all();
        if let Some(renderer) = &mut self.renderer {
            renderer.release_frame_target();
        }
        // A GBA save state is ~500 KB; keeping that buffer alive while the user browses
        // their library is pure waste.
        self.state_scratch = Vec::new();
        self.pacer = FramePacer::new(60.0);
    }

    /// Sets what happens to a core when its session ends. See [`CoreRetention`].
    pub fn set_core_retention(&mut self, retention: CoreRetention) {
        log::info!("core retention: {}", retention.as_str());
        self.retention = retention;
    }

    pub fn core_retention(&self) -> CoreRetention {
        self.retention
    }

    /// Frees every resident core. Nothing may be running.
    pub fn unload_all_cores(&mut self) -> usize {
        if self.session.is_some() {
            return 0;
        }
        self.registry.unload_all_except(None)
    }

    /// Ids of cores currently holding memory. Diagnostics for the leak checks.
    pub fn resident_core_ids(&self) -> Vec<String> {
        self.registry.resident_ids().map(str::to_string).collect()
    }

    pub fn pause(&mut self) {
        if let Some(session) = &mut self.session {
            if !session.paused {
                session.paused = true;
                // Drop buffered audio: on resume it would play a stale burst.
                self.sink.flush();
            }
        }
    }

    pub fn resume(&mut self, now_ms: f64) {
        if let Some(session) = &mut self.session {
            if session.paused {
                session.paused = false;
                // Re-anchor, or the paused interval becomes emulated-time debt.
                self.pacer.resync(now_ms);
            }
        }
    }

    pub fn reset(&mut self) -> Result<(), BridgeError> {
        let session = self.session.as_mut().ok_or(BridgeError::NoSession)?;
        session.core.reset()?;
        // Cheats are re-pushed after a reset. Several cores clear their cheat list in
        // `retro_reset`, and the ones that do not are unharmed by being told again —
        // whereas a user who resets and silently loses their cheats has no way to tell
        // that is what happened.
        Self::push_cheats(session)?;
        self.sink.flush();
        self.gamepads.release_all();
        Ok(())
    }

    pub fn current_content_id(&self) -> Option<&str> {
        self.session.as_ref().map(|s| s.content_id.as_str())
    }

    pub fn current_core_id(&self) -> Option<&str> {
        self.session.as_ref().map(|s| s.core_id.as_str())
    }

    pub fn frame_count(&self) -> u64 {
        self.session.as_ref().map_or(0, |s| s.core.frame_count())
    }

    /// The running core's own reported characteristics, not the manifest's.
    ///
    /// Worth distinguishing: the manifest is a hint written by hand, while this is
    /// what the core said through `retro_get_system_av_info` after loading content. If
    /// they disagree, this is the truth — the pacer and the renderer are configured
    /// from these values.
    pub fn session_av_info(&self) -> Option<[f64; 5]> {
        let session = self.session.as_ref()?;
        let descriptor = session.core.descriptor();
        Some([
            descriptor.geometry.base_width as f64,
            descriptor.geometry.base_height as f64,
            descriptor.geometry.aspect_ratio as f64,
            descriptor.target_fps,
            descriptor.audio_sample_rate as f64,
        ])
    }

    /// Memory held by the running core, if measurable.
    pub fn session_core_memory_bytes(&self) -> Option<u64> {
        self.session.as_ref()?.core.memory_bytes()
    }

    /// Id of the core backing the running session, if any.
    pub fn session_core_name(&self) -> Option<&str> {
        self.session
            .as_ref()
            .map(|s| s.core.descriptor().display_name.as_str())
    }

    // --------------------------------------------------------------------- tick

    /// The unified step: input → core → audio → GPU. Called once per
    /// `requestAnimationFrame` and from nowhere else.
    pub fn tick(&mut self, now_ms: f64) -> Result<TickReport, BridgeError> {
        // Destructured so the core (borrowed from `session`) and the renderer can
        // be used together without fighting the borrow checker.
        let Self {
            session,
            renderer,
            sink,
            pacer,
            gamepads,
            ..
        } = self;

        let Some(session) = session.as_mut() else {
            // Idle: clear once so a stopped session does not leave its last frame
            // frozen on the canvas.
            let presented = match renderer.as_mut() {
                Some(r) => {
                    r.present(None)?;
                    true
                }
                None => false,
            };
            return Ok(TickReport {
                presented,
                ..Default::default()
            });
        };

        if session.paused {
            // Re-present the existing texture: still responsive to resizes, but no
            // emulation and no audio.
            let presented = match renderer.as_mut() {
                Some(r) => {
                    r.present(None)?;
                    true
                }
                None => false,
            };
            return Ok(TickReport {
                presented,
                frame_count: session.core.frame_count(),
                audio: sink.stats(),
                display_fps: pacer.display_fps(),
                ..Default::default()
            });
        }

        // 1. Input — snapshot once so every catch-up step of this tick sees a
        //    coherent controller state, and so a real core querying `input_state`
        //    mid-frame cannot observe a button changing underneath it.
        let snapshot = gamepads.snapshot();

        // 2. Core steps (0..=4, decided by the pacer).
        let plan = pacer.plan(now_ms);
        for _ in 0..plan.steps {
            session.core.run_frame(&snapshot)?;
            // 3. Audio — drained straight into the ring, no intermediate buffer.
            session.core.drain_audio(sink.as_mut());
        }

        // 4. GPU. With zero steps there is no new frame, so the previous texture is
        //    re-presented rather than skipping present entirely (which would stall
        //    the compositor's expectations).
        let mut presented = false;
        if let Some(renderer) = renderer.as_mut() {
            let frame = if plan.steps > 0 {
                session.core.video()
            } else {
                None
            };
            renderer.present(frame)?;
            presented = true;
        }

        Ok(TickReport {
            steps: plan.steps,
            dropped: plan.dropped,
            presented,
            resynced: plan.resynced,
            display_fps: pacer.display_fps(),
            frame_count: session.core.frame_count(),
            audio: sink.stats(),
        })
    }

    // -------------------------------------------------------------------- input

    /// Sets a button for one input source. Sources are independent layers, merged at
    /// snapshot time, so the keyboard and an idle controller cannot fight.
    pub fn set_button(&mut self, port: usize, source: PadSource, button: Button, pressed: bool) {
        self.gamepads.set_button(port, source, button, pressed);
    }

    pub fn set_axis(&mut self, port: usize, source: PadSource, axis: usize, value: f32) {
        self.gamepads.set_axis(port, source, axis, value);
    }

    /// Releases one source, e.g. when the touch overlay is dismissed.
    pub fn release_input_source(&mut self, source: PadSource) {
        self.gamepads.release_source(source);
    }

    /// Applies one poll of a W3C standard gamepad. See
    /// [`GamepadBridge::apply_standard_gamepad`].
    pub fn apply_gamepad(&mut self, port: usize, buttons: &[bool], axes: &[f32]) {
        self.gamepads.apply_standard_gamepad(port, buttons, axes);
    }

    pub fn connect_pad(&mut self, port: usize, kind: PadKind, label: &str) -> bool {
        self.gamepads.connect(port, kind, label)
    }

    pub fn disconnect_pad(&mut self, port: usize) {
        self.gamepads.disconnect(port);
    }

    pub fn connected_pads(&self) -> usize {
        self.gamepads.connected_count()
    }

    pub fn pad_label(&self, port: usize) -> Option<&str> {
        self.gamepads.pad_label(port)
    }

    pub fn pad_kind(&self, port: usize) -> Option<PadKind> {
        self.gamepads.pad_kind(port)
    }

    pub fn first_free_pad_port(&self) -> Option<usize> {
        self.gamepads.first_free_port()
    }

    /// Releases everything. Call on blur / visibility change so a held key cannot
    /// stick down while the tab is in the background.
    pub fn release_all_input(&mut self) {
        self.gamepads.release_all();
    }

    // -------------------------------------------------------------------- audio

    /// Tells the bridge the device's real sample rate.
    ///
    /// Not knowable before the user gesture that unlocks `AudioContext`, so this
    /// arrives after `launch` and reconfigures the live sink.
    pub fn set_output_sample_rate(&mut self, rate: u32) {
        if rate == 0 || rate == self.output_sample_rate {
            return;
        }
        log::info!("output sample rate: {rate} Hz");
        self.output_sample_rate = rate;
        self.sink.set_output_rate(rate);
    }

    pub fn output_sample_rate(&self) -> u32 {
        self.output_sample_rate
    }

    pub fn audio_spec(&self) -> AudioSpec {
        self.sink.spec()
    }

    pub fn audio_stats(&self) -> AudioStats {
        self.sink.stats()
    }

    /// Fills `dst` with queued PCM, returning the sample count written. The host
    /// calls this at the end of each tick and forwards the result to the device.
    pub fn drain_audio(&mut self, dst: &mut [f32]) -> usize {
        if self.muted {
            return 0;
        }
        self.sink.drain(dst)
    }

    /// Discards the queued backlog without touching the session.
    ///
    /// For a host whose output device changed underneath it: headphones unplugged, a Bluetooth
    /// route gone. Those samples were resampled for the device that just went away, and the
    /// host has thrown away its own buffer at the same moment, so keeping them would only make
    /// the reported latency a lie. `load_state` already does this for the same reason, one
    /// timeline over instead of one device over.
    pub fn flush_audio(&mut self) {
        self.sink.flush();
    }

    pub fn set_muted(&mut self, muted: bool) {
        if muted != self.muted {
            self.muted = muted;
            if muted {
                // Drop the backlog, otherwise unmuting replays stale audio.
                self.sink.flush();
            }
        }
    }

    pub fn is_muted(&self) -> bool {
        self.muted
    }

    // ------------------------------------------------------------------- output

    pub fn resize(&mut self, width: u32, height: u32) {
        if let Some(renderer) = &mut self.renderer {
            renderer.resize(width, height);
        }
    }

    pub fn set_filter(&mut self, filter: ScaleFilter) {
        if let Some(renderer) = &mut self.renderer {
            renderer.set_filter(filter);
        }
    }

    pub fn set_scale_mode(&mut self, mode: ScaleMode) {
        if let Some(renderer) = &mut self.renderer {
            renderer.set_scale_mode(mode);
        }
    }

    pub fn set_speed(&mut self, speed: f64) {
        self.pacer.set_speed(speed);
    }

    pub fn speed(&self) -> f64 {
        self.pacer.speed()
    }

    /// Submits a GPU readback of the presented image.
    ///
    /// `width`/`height` of zero mean "the current surface size". The caller awaits
    /// the buffer map and then calls [`crate::gfx::FrameCapture::take_rgba`].
    pub fn encode_capture(
        &mut self,
        width: u32,
        height: u32,
    ) -> Result<crate::gfx::FrameCapture, BridgeError> {
        let Self {
            renderer, session, ..
        } = self;
        let renderer = renderer.as_mut().ok_or(BridgeError::NoRenderer)?;

        // Refresh from the core first, so a capture shows the current image even
        // when nothing has been presented yet (offscreen thumbnails, or a paused
        // session whose last present predates a load-state).
        if let Some(session) = session.as_ref() {
            if let Some(frame) = session.core.video() {
                renderer.upload_frame(&frame)?;
            }
        }

        let (surface_width, surface_height) = renderer.surface_size();
        let width = if width == 0 { surface_width } else { width };
        let height = if height == 0 { surface_height } else { height };
        Ok(renderer.encode_capture(width, height)?)
    }

    // --------------------------------------------------------------- save state

    pub fn save_state(&mut self) -> Result<Vec<u8>, BridgeError> {
        let session = self.session.as_ref().ok_or(BridgeError::NoSession)?;
        let size = session.core.state_size();
        if size == 0 {
            return Err(BridgeError::SaveState(
                "this core does not support save states".into(),
            ));
        }
        self.state_scratch.clear();
        self.state_scratch.resize(size, 0);
        let written = session.core.save_state(&mut self.state_scratch)?;
        Ok(self.state_scratch[..written].to_vec())
    }

    pub fn load_state(&mut self, data: &[u8]) -> Result<(), BridgeError> {
        let session = self.session.as_mut().ok_or(BridgeError::NoSession)?;
        session.core.load_state(data)?;
        // A save state can carry the memory a cheat was patching, so the list is
        // re-pushed for the restored timeline.
        Self::push_cheats(session)?;
        // The audio backlog belongs to the abandoned timeline.
        self.sink.flush();
        Ok(())
    }

    // ------------------------------------------------------------------- cheats

    /// Replaces the whole cheat list.
    ///
    /// Whole-list rather than incremental, because `retro_cheat_set` is indexed: a core
    /// keeps cheats in a numbered table, and removing the second of five would leave a
    /// hole that every later index has to be shifted around. Resetting and re-pushing is
    /// what RetroArch does too, it costs microseconds, and it makes the applied state a
    /// pure function of the list the user is looking at.
    ///
    /// `enabled` is a parallel byte array rather than `&[bool]` for the same reason as
    /// [`Self::apply_gamepad`]: wasm-bindgen has no bool-slice ABI.
    ///
    /// @returns how many cheats are switched on
    pub fn apply_cheats(
        &mut self,
        codes: Vec<String>,
        enabled: &[u8],
    ) -> Result<usize, BridgeError> {
        let session = self.session.as_mut().ok_or(BridgeError::NoSession)?;
        if !session.core.supports_cheats() {
            return Err(BridgeError::Cheat(format!(
                "core '{}' does not support cheats",
                session.core_id
            )));
        }

        // Blank lines are dropped here rather than skipped during the push, because
        // skipping would leave gaps in the index sequence the core is given.
        session.cheats = codes
            .into_iter()
            .enumerate()
            .filter_map(|(i, code)| {
                let code = code.trim().to_string();
                if code.is_empty() {
                    return None;
                }
                Some(Cheat {
                    code,
                    enabled: enabled.get(i).copied().unwrap_or(0) != 0,
                })
            })
            .collect();

        Self::push_cheats(session)?;
        Ok(session.cheats.iter().filter(|cheat| cheat.enabled).count())
    }

    /// Clears every cheat, in the core and in our record of it.
    pub fn clear_cheats(&mut self) -> Result<(), BridgeError> {
        let session = self.session.as_mut().ok_or(BridgeError::NoSession)?;
        session.cheats.clear();
        if session.core.supports_cheats() {
            session.core.reset_cheats()?;
        }
        Ok(())
    }

    /// Whether the running core can apply cheats at all.
    pub fn cheats_supported(&self) -> bool {
        self.session
            .as_ref()
            .is_some_and(|session| session.core.supports_cheats())
    }

    /// How many cheats are currently switched on.
    pub fn active_cheat_count(&self) -> usize {
        self.session.as_ref().map_or(0, |session| {
            session.cheats.iter().filter(|cheat| cheat.enabled).count()
        })
    }

    // ------------------------------------------------------------- core options

    /// Options the running core declared, with their current values.
    pub fn core_options(&self) -> Vec<crate::cores::CoreOption> {
        self.session
            .as_ref()
            .map(|session| session.core.core_options())
            .unwrap_or_default()
    }

    /// Sets one option on the running core.
    ///
    /// Takes effect without a relaunch for options the core re-reads on its update poll,
    /// which is most of them. A few are only consulted while loading content; those need
    /// the game restarted, and the core is the only thing that knows which is which.
    pub fn set_core_option(&mut self, key: &str, value: &str) -> Result<(), BridgeError> {
        let session = self.session.as_mut().ok_or(BridgeError::NoSession)?;
        session.core.set_core_option(key, value)
    }

    /// Pushes the session's recorded list into the core: reset, then set each in order.
    ///
    /// Disabled cheats are still handed over, with `enabled = false`. That is the
    /// libretro contract — a core is entitled to keep the code and simply not apply it —
    /// and it keeps the indices stable so toggling one does not renumber the rest.
    fn push_cheats(session: &mut Session) -> Result<(), BridgeError> {
        if session.cheats.is_empty() {
            // Still worth resetting: this is also the path that turns the last cheat off.
            if session.core.supports_cheats() {
                session.core.reset_cheats()?;
            }
            return Ok(());
        }
        session.core.reset_cheats()?;
        // Collected first so the loop does not hold a borrow of `session.cheats` while
        // calling `&mut` methods on `session.core`.
        let list: Vec<(String, bool)> = session
            .cheats
            .iter()
            .map(|cheat| (cheat.code.clone(), cheat.enabled))
            .collect();
        for (index, (code, enabled)) in list.iter().enumerate() {
            session.core.set_cheat(index as u32, *enabled, code)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::{FrameGeometry, PixelFormat};

    const MODULE: &[u8] = b"\0asm\x01\0\0\0";

    fn descriptor(id: &str, system: &str) -> CoreDescriptor {
        CoreDescriptor {
            id: id.into(),
            display_name: id.into(),
            systems: vec![system.into()],
            geometry: FrameGeometry::new(256, 240, 4.0 / 3.0),
            target_fps: 60.0,
            audio_sample_rate: 48_000,
            pixel_format: PixelFormat::Rgba8888,
            module_url: format!("/cores/{id}.wasm"),
            priority: 0,
        }
    }

    #[test]
    fn boots_with_no_cores_resident() {
        let mut bridge = EmulatorBridge::new();
        bridge.declare_core(descriptor("nestopia", "nes"));
        bridge.declare_core(descriptor("snes9x", "snes"));
        assert_eq!(bridge.status(), BridgeStatus::Uninitialised);
        assert_eq!(bridge.resident_core_count(), 0);
        assert_eq!(bridge.core_state("nestopia"), Some(CoreState::Declared));
    }

    #[test]
    fn launch_without_renderer_fails_immediately() {
        let mut bridge = EmulatorBridge::new();
        bridge.declare_core(descriptor("nestopia", "nes"));
        bridge.attach_core_module("nestopia", MODULE).unwrap();
        let err = bridge
            .launch(
                "nestopia",
                "smb.nes",
                b"rom",
                &ContentHint::from_filename("smb.nes"),
            )
            .unwrap_err();
        assert!(matches!(err, BridgeError::NoRenderer));
    }

    #[test]
    fn tick_without_session_reports_nothing_emulated() {
        let mut bridge = EmulatorBridge::new();
        let report = bridge.tick(0.0).unwrap();
        assert_eq!(report.steps, 0);
        assert!(!report.presented); // no renderer in a headless test
    }

    #[test]
    fn attach_module_makes_core_resident() {
        let mut bridge = EmulatorBridge::new();
        bridge.declare_core(descriptor("mgba", "gba"));
        assert_eq!(bridge.resident_core_count(), 0);
        bridge.attach_core_module("mgba", MODULE).unwrap();
        assert_eq!(bridge.resident_core_count(), 1);
        assert_eq!(bridge.core_state("mgba"), Some(CoreState::Loaded));
    }

    #[test]
    fn save_state_requires_a_session() {
        let mut bridge = EmulatorBridge::new();
        assert!(matches!(bridge.save_state(), Err(BridgeError::NoSession)));
    }

    #[test]
    fn output_rate_change_propagates_to_sink() {
        let mut bridge = EmulatorBridge::new();
        bridge.set_output_sample_rate(44_100);
        assert_eq!(bridge.output_sample_rate(), 44_100);
        assert_eq!(bridge.audio_spec().output_rate, 44_100);
    }

    #[test]
    fn muting_discards_queued_audio() {
        let mut bridge = EmulatorBridge::new();
        bridge.set_muted(true);
        let mut buf = [0.0; 16];
        assert_eq!(bridge.drain_audio(&mut buf), 0);
        assert!(bridge.is_muted());
    }
}
