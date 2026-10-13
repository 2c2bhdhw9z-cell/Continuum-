//! `EmulatorBridge` — the engine the UI talks to, and the only thing that does.
//!
//! Everything stateful lives here: which cores exist, which one is running, where
//! input goes, how audio is buffered, when frames are presented. The UI is reduced
//! to two responsibilities: forward events in, call [`EmulatorBridge::tick`] once
//! per animation frame.
//!
//! That split is what keeps the iOS app a thin UI over this engine. This file
//! contains no platform types at all: `uniffi_api.rs` is a thin wrapper over this
//! API, so a second platform inherits this behaviour rather than re-implementing
//! it. That is the property to preserve.
//!
//! Single-threaded and single-loop by construction: `tick` runs input, core steps,
//! audio submission and the GPU present in that order, and nothing here spawns a
//! thread, a worker or a timer.

use crate::audio::{AudioSink, AudioSpec, AudioStats, NullAudioSink, RingAudioSink, CHANNELS};
use crate::cheats::poke::Poke;
use crate::cheats::search::{RamSearch, SearchFilter, SearchHit, SearchWidth};
use crate::battery_layout::{self, BatteryLayout};
use crate::cores::{ContentHint, CoreDescriptor, CoreRegistry, CoreState, EmulatorCore};
use crate::error::BridgeError;
use crate::gfx::screen_layout::{DualScreenConfig, DualScreenGeometry, TouchRegion};
use crate::gfx::{Renderer, ScaleFilter, ScaleMode, SkinHole};
use crate::input::{Button, GamepadBridge, PadKind, PadSource};
use crate::rewind::RewindBuffer;
use crate::timing::FramePacer;

// The lockstep half of online play. A child module so it can reach the private fields above
// without widening them; the protocol itself is in `crate::netplay` and knows nothing of this.
#[path = "netplay/bridge_glue.rs"]
mod netplay_glue;
#[path = "bridge_actions.rs"]
pub mod actions;

// Controller types, remap profiles and the console actions (shake, analog, DS lid, blow, HOME).
// A child module for the same reason as `netplay_glue`.
#[path = "input/bridge_glue.rs"]
mod input_glue;

/// Video frames of audio to buffer. Three is the usual compromise between
/// robustness against a slow tick and audible input-to-sound latency.
const AUDIO_LATENCY_FRAMES: usize = 3;

/// Core frames between rewind snapshots.
///
/// Six is ten snapshots a second at 60 fps, which is a fine enough grain that rewinding
/// feels like scrubbing rather than stepping, while costing a tenth as much memory and
/// serialisation work as snapshotting every frame. It also bounds the cost of the feature:
/// `retro_serialize` is not free, and calling it sixty times a second on a PlayStation
/// state would eat frame budget a phone does not have to spare.
const DEFAULT_REWIND_INTERVAL_FRAMES: u32 = 6;

/// What happens to a core when its session ends.
///
/// The default is [`CoreRetention::Drop`], which is the right default for an
/// all-in-one emulator: cores are large (megabytes of library plus tens of megabytes of
/// working memory), a user browsing their library is not using any of it, and iOS kills
/// processes on memory pressure rather than paging. Loading the core again on the next
/// launch is a `dlopen` from the app bundle, not a download.
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
    /// No renderer yet: no Metal layer has been attached, or no GPU adapter was found.
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
    /// How many of the user's typed codes are on. Not `cheats`' count: one code can be several
    /// core entries (Gearsystem takes each `+` part on its own).
    enabled_codes: usize,
    /// RAM pokes split out of the same list (see [`EmulatorBridge::apply_cheats`]), enabled ones
    /// only. Written into `SYSTEM_RAM` after every `run_frame`.
    pokes: Vec<Poke>,
    /// The RAM search in progress, if one was started. Belongs to the game, so it ends with it.
    search: Option<RamSearch>,
    /// Which memory the search runs over. Meaningless while `search` is `None`.
    search_region: SearchRegion,
    /// The game's file name ("Zelda (USA).nes") and lower-case extension, for the battery-save
    /// files another core may have written for it (`battery_layout::migration_candidates`).
    content_name: String,
    content_ext: String,
    /// How the battery save is shaped for this core (`battery_layout::layout_for`).
    battery_layout: BatteryLayout,
}

/// The memory a RAM search runs over.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SearchRegion {
    /// libretro's `SYSTEM_RAM`, addressed from 0. What every search was before memory maps.
    #[default]
    SystemRam,
    /// One descriptor of the core's memory map, addressed by the console's own addresses. The
    /// start and length are kept so a core republishing a different map mid-search is noticed.
    Mapped { index: usize, start: usize, len: usize },
}

/// One region the RAM search can be pointed at, for the picker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchRegionInfo {
    /// "system", or "map:N" for descriptor N of the core's memory map.
    pub key: String,
    /// What to call it on screen: "System RAM", "IWRAM", ...
    pub name: String,
    /// The console address of its first byte (0 for system RAM).
    pub start: u64,
    pub size: u64,
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
    /// Device sample rate, learned from the host once its audio engine is running.
    output_sample_rate: u32,
    muted: bool,
    /// Reused across `save_state` calls so snapshotting does not allocate.
    state_scratch: Vec<u8>,
    retention: CoreRetention,

    // The user's preferences, held here rather than only on the objects that consume
    // them, because those objects do not outlive a session. `FramePacer` is rebuilt by
    // every `launch` and every `stop`, and the renderer's frame target is released on
    // stop, so a preference written only into the pacer or only into the renderer
    // quietly reverts to its default the next time a game starts. Storing the wanted
    // value here and re-applying it in `launch` is what makes a setting a setting
    // rather than a per-session accident.
    /// Wanted speed multiplier. Survives the pacer being rebuilt.
    speed: f64,
    /// Wanted scale mode. Survives the renderer being re-targeted.
    scale_mode: ScaleMode,
    /// Wanted scaling filter. Survives the renderer being re-targeted.
    filter: ScaleFilter,
    /// The core's own declared sample rate, before any speed adjustment. Kept so the
    /// fast-forward ratio is always computed from the native rate rather than compounded
    /// off the last adjusted one.
    native_audio_rate: u32,
    /// Wanted output gain in `0.0..=1.0`.
    volume: f32,
    /// The gain actually applied to the last sample of the previous drain. Ramping from
    /// here to `volume` across a block is what stops a dragged volume slider from
    /// clicking sixty times a second.
    gain: f32,
    /// Bounded tape of save states. Disabled until given a budget. See [`RewindBuffer`].
    rewind: RewindBuffer,
    /// Core frames between rewind snapshots. See [`DEFAULT_REWIND_INTERVAL_FRAMES`].
    rewind_interval: u32,
    /// Core frames stepped since the last snapshot. Counts steps rather than ticks,
    /// because a tick can run up to four of them.
    frames_since_snapshot: u32,
    /// True while the rewind button is held. Diverts `tick` entirely.
    rewinding: bool,
    /// Skin holes last requested. Kept even when the renderer is not attached yet, so the
    /// first layout — which can land before the Metal layer — is not thrown away.
    skin_holes: Vec<crate::gfx::SkinHole>,
    /// RetroAchievements. Created on the first achievements call and kept across sessions, so a
    /// login survives leaving a game. Native builds only: rcheevos is compiled with the core host.
    #[cfg(feature = "native-core")]
    achievements: Option<crate::achievements::Achievements>,
    /// Two-screen layout, swap and TV choice. Survives the renderer and every session, and is
    /// pushed into the renderer whenever either changes. See `gfx::screen_layout`.
    dual_config: DualScreenConfig,
    /// The running core's two-screen geometry, from its declared systems. `None` otherwise.
    dual_geometry: Option<DualScreenGeometry>,
    /// Two-player online session, if one exists. While it is live the tick runs in lockstep
    /// (see `netplay_glue`), and rewind, fast forward, reset, cheats and state loads are refused,
    /// because any of them on one phone and not the other is a desync.
    netplay: Option<crate::netplay::NetplaySession>,
    /// Speed presets, console switches, rotation, the look and the TV's choices. See
    /// `bridge_actions.rs`.
    actions: actions::ActionState,
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
            speed: 1.0,
            scale_mode: ScaleMode::AspectFit,
            filter: ScaleFilter::Nearest,
            native_audio_rate: 0,
            volume: 1.0,
            gain: 1.0,
            rewind: RewindBuffer::new(),
            rewind_interval: DEFAULT_REWIND_INTERVAL_FRAMES,
            frames_since_snapshot: 0,
            rewinding: false,
            skin_holes: Vec::new(),
            #[cfg(feature = "native-core")]
            achievements: None,
            dual_config: DualScreenConfig {
                touch_on_phone: true,
                ..DualScreenConfig::default()
            },
            dual_geometry: None,
            netplay: None,
            actions: actions::ActionState::default(),
        }
    }

    // ---------------------------------------------------------------- lifecycle

    /// Installs the renderer built by the platform layer.
    pub fn attach_renderer(&mut self, mut renderer: Renderer) {
        log::info!("renderer attached: {}", renderer.adapter_summary());
        // A renderer arrives with its own defaults, which are not necessarily the ones the
        // user chose. On iOS the host restores saved settings during launch, and whether
        // that lands before or after the Metal layer is ready is not something this side
        // should have to depend on, so the wanted values are pushed in either order.
        renderer.set_scale_mode(self.scale_mode);
        renderer.set_filter(self.filter);
        renderer.set_skin_holes(self.skin_holes.clone());
        renderer.set_dual_config(self.dual_config);
        renderer.set_dual_geometry(self.dual_geometry);
        self.renderer = Some(renderer);
        self.push_actions_to_renderer();
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

    /// Test-only: attaches the built-in [`crate::cores::DiagnosticCore`] stand-in so session,
    /// memory and registry behaviour can be tested without a real core. The bytes are a
    /// placeholder checked by [`CoreRegistry::attach_module`]. The app never calls this: a real
    /// core is dlopened and arrives through [`Self::attach_core`].
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
        // Online play belongs to one game. Whatever the last one left behind (a finished or
        // abandoned session kept only for its status line) is forgotten here.
        self.netplay = None;

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
        self.native_audio_rate = descriptor.audio_sample_rate;
        // Both objects above were just replaced, taking the user's speed and video
        // preferences with them. Put them back before the first frame, so a game does not
        // start at 1x with the wrong filter and then visibly correct itself.
        self.pacer.set_speed(self.speed);
        self.apply_audio_speed();
        // A DS or a 3DS has two screens in its one framebuffer. Read from what the core says it
        // runs rather than from its id, so a second DS core needs nothing here.
        self.dual_geometry = descriptor
            .systems
            .iter()
            .find_map(|system| DualScreenGeometry::for_system(system));
        if let Some(renderer) = &mut self.renderer {
            renderer.set_aspect_ratio(descriptor.geometry.aspect_ratio);
            renderer.set_scale_mode(self.scale_mode);
            renderer.set_filter(self.filter);
            renderer.set_dual_geometry(self.dual_geometry);
            renderer.set_dual_config(self.dual_config);
            renderer.set_frame_hint(
                descriptor.geometry.base_width,
                descriptor.geometry.base_height,
            );
        }
        self.gamepads.release_all();
        // A new session starts with every motor still. See `input::rumble::Rumble::clear`.
        crate::input::rumble::RUMBLE.clear();
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

        let content_name = hint
            .full_path
            .as_deref()
            .and_then(|p| std::path::Path::new(p).file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| format!("{}.{}", hint.name, hint.extension));
        let battery_layout = Self::battery_layout_for(core_id, content, hint);
        self.session = Some(Session {
            core_id: core_id.to_string(),
            content_id: content_id.to_string(),
            core,
            paused: false,
            // A fresh core has no cheats. The front end pushes the game's saved list
            // after launch, which is also what makes cheats survive a relaunch.
            cheats: Vec::new(),
            enabled_codes: 0,
            pokes: Vec::new(),
            search: None,
            search_region: SearchRegion::SystemRam,
            content_name: content_name.clone(),
            content_ext: hint.extension.to_ascii_lowercase(),
            battery_layout,
        });
        self.actions.new_session(&content_name);
        // Slow motion and a speed preset are the user's, and survive a new game.
        let wanted = self.actions.effective_speed();
        if (wanted - 1.0).abs() > 1e-9 {
            self.set_speed(wanted);
        }
        self.push_actions_to_renderer();
        Ok(())
    }

    /// Ends the session and returns its core to the registry. Under the default
    /// [`CoreRetention::Drop`] the core is then freed; `KeepWarm` keeps it resident.
    pub fn stop(&mut self) {
        // Before the core goes: rcheevos holds pointers into its memory until told otherwise.
        #[cfg(feature = "native-core")]
        if let Some(achievements) = self.achievements.as_mut() {
            achievements.clear_memory();
            if achievements.is_game_loaded() {
                achievements.unload_game();
            }
        }
        if let Some(netplay) = self.netplay.as_mut() {
            // Kept, not dropped, so the host can still flush the goodbye and show why the
            // session ended. `netplay_stop` is what forgets it.
            netplay.leave("the game was closed");
        }
        if let Some(session) = self.session.take() {
            let core_id = session.core_id.clone();
            log::info!(
                "session ended: '{}' after {} frames",
                session.content_id,
                session.core.frame_count()
            );

            // The core goes back to the registry first so the slot is never left
            // `Bound`, then the retention policy decides whether it survives. Dropping
            // a real core here runs `NativeLibretroCore::drop` → retro_unload_game →
            // retro_deinit, and its library handle is released with it.
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
        // A core torn down mid-rumble never sends the zero that would end it.
        crate::input::rumble::RUMBLE.clear();
        if let Some(renderer) = &mut self.renderer {
            renderer.release_frame_target();
            renderer.set_dual_geometry(None);
        }
        self.dual_geometry = None;
        // A GBA save state is ~500 KB; keeping that buffer alive while the user browses
        // their library is pure waste.
        self.state_scratch = Vec::new();
        self.pacer = FramePacer::new(60.0);
        // The replacement pacer is at 1x; the user's choice is not forgotten just because
        // a session ended.
        self.pacer.set_speed(self.speed);
        self.native_audio_rate = 0;
        // Volume ramps from wherever the last session left off, and the next session's
        // first block should not fade in from a stale gain.
        self.gain = self.volume;
        // The tape belongs to the game that just ended. `launch` calls `stop` first, so
        // this is also what stops one game's history leaking into the next one's.
        self.rewind.clear();
        self.frames_since_snapshot = 0;
        // A session that ended while the button was held must not leave the next one
        // rewinding into an empty tape from its first frame.
        self.rewinding = false;
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
                // A core that runs on threads of its own has to be told, or it plays on behind
                // the pause menu. Everything else ignores this.
                session.core.set_paused(true);
                // Drop buffered audio: on resume it would play a stale burst.
                self.sink.flush();
            }
        }
    }

    pub fn resume(&mut self, now_ms: f64) {
        if let Some(session) = &mut self.session {
            if session.paused {
                session.paused = false;
                session.core.set_paused(false);
                // Re-anchor, or the paused interval becomes emulated-time debt.
                self.pacer.resync(now_ms);
            }
        }
    }

    pub fn reset(&mut self) -> Result<(), BridgeError> {
        self.refuse_during_netplay("reset")?;
        let session = self.session.as_mut().ok_or(BridgeError::NoSession)?;
        session.core.reset()?;
        // Cheats are re-pushed after a reset. Several cores clear their cheat list in
        // `retro_reset`, and the ones that do not are unharmed by being told again —
        // whereas a user who resets and silently loses their cheats has no way to tell
        // that is what happened.
        Self::push_cheats(session)?;
        self.sink.flush();
        self.gamepads.release_all();
        self.achievements_reset();
        crate::input::rumble::RUMBLE.clear();
        // Everything on the rewind tape is from before the reset, so rewinding would undo
        // the reset itself.
        self.rewind.clear();
        self.frames_since_snapshot = 0;
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

    /// Display name of the core backing the running session, if any.
    pub fn session_core_name(&self) -> Option<&str> {
        self.session
            .as_ref()
            .map(|s| s.core.descriptor().display_name.as_str())
    }

    /// One plain sentence saying what speed the running game is at and which region that is right
    /// for, or "" when there is nothing worth saying. See [`crate::game_region`]: the rate is the
    /// core's own, so this reports what the game IS running as rather than what the file claims.
    pub fn session_region_note(&self) -> String {
        match self.session.as_ref() {
            Some(session) => crate::game_region::note(
                &session.content_ext,
                session.core.descriptor().target_fps,
            ),
            None => String::new(),
        }
    }

    // --------------------------------------------------------------------- tick

    /// The unified step: input → core → audio → GPU. Called once per display-link
    /// callback (`MetalCanvas.swift`) and from nowhere else.
    pub fn tick(&mut self, now_ms: f64) -> Result<TickReport, BridgeError> {
        // Actions remapped buttons pressed since the last tick (shake, DS lid, profile cycle...),
        // before this tick's snapshot so their effect lands on this frame.
        self.process_input_actions();
        // Online play owns the whole step while it is live: frames run only when both players'
        // inputs have arrived. See `netplay_glue`.
        if self.netplay_owns_tick() {
            return self.tick_netplay(now_ms);
        }
        // Rewind replaces the normal step entirely rather than running alongside it. Doing
        // it from the host instead - calling a rewind method and then `tick` - would advance
        // the core and then jump it back within the same frame, so the two would fight and
        // the picture would judder rather than reverse.
        if self.rewinding && self.rewind.is_enabled() && self.session.is_some() {
            return self.tick_rewinding(now_ms);
        }

        // Destructured so the core (borrowed from `session`) and the renderer can
        // be used together without fighting the borrow checker.
        let Self {
            session,
            renderer,
            sink,
            pacer,
            gamepads,
            rewind,
            rewind_interval,
            frames_since_snapshot,
            #[cfg(feature = "native-core")]
            achievements,
            actions,
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
            #[cfg(feature = "native-core")]
            if let Some(achievements) = achievements.as_mut() {
                if achievements.is_game_loaded() {
                    achievements.idle();
                }
            }
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
        let mut snapshot = gamepads.snapshot();

        // 2. Core steps (0..=4, decided by the pacer).
        let plan = pacer.plan(now_ms);
        for _ in 0..plan.steps {
            // Turbo is the one part of input that is meant to differ between catch-up steps:
            // it pulses per CORE frame, so its rate holds at 120 Hz and under fast forward.
            let mut step = gamepads.turbo_step(&snapshot);
            // Presses the engine makes itself: the Atari switches and the FDS side flip.
            actions.pulse_step(&mut step);
            // PSP only (strict sync is PPSSPP's session flag): where a frame died, for the
            // crash report. Sparse; see `feedback::frame_phase`.
            let phases = crate::gfx::vulkan_hw::strict_sync();
            if phases {
                crate::feedback::frame_phase(session.core.frame_count(), "run");
            }
            // Last of all: the buttons as this core reads them (BlastEm's Mega Drive layout).
            crate::input::core_layout::translate_for(&session.core_id, &mut step);
            session.core.run_frame(&step)?;
            if phases {
                crate::feedback::frame_phase(session.core.frame_count(), "ran");
            }
            // 2a. Achievements, against memory exactly as the game left it this frame, before
            //     any poke rewrites it.
            #[cfg(feature = "native-core")]
            Self::achievements_frame(achievements.as_mut(), session);
            // 2b. RAM pokes, AFTER the frame. The game wrote its own value during the frame;
            //     writing ours afterwards means the value the game reads at the start of the next
            //     frame is ours, which is what "infinite lives" has to mean.
            Self::apply_pokes(session);
            // Relative mouse motion belongs to the first step only. See
            // `InputSnapshot::consume_mouse_motion`.
            snapshot.consume_mouse_motion();
            // 3. Audio — drained straight into the ring, no intermediate buffer.
            session.core.drain_audio(sink.as_mut());
        }
        // Motion a frame read is spent. With no step this tick it is kept for the next one,
        // so a drag during a skipped frame is not lost.
        if plan.steps > 0 {
            gamepads.end_mouse_frame();
            // A core can change its own refresh rate in the middle of a game, and one of ours
            // does: FCEUmm re-reads its av info and publishes it the moment the NES region option
            // moves between NTSC and PAL. Until this, the pacer kept the rate the game started
            // at, so a game switched to Europe still ran at 60 steps a second (and the (i) line
            // still called it an American copy). The core's figure is the authority either way;
            // see `cores::native_core::sync_descriptor_from_core`.
            let core_fps = session.core.descriptor().target_fps;
            if core_fps > 1.0 && (core_fps - pacer.target_fps()).abs() > 0.01 {
                log::info!(
                    "core '{}' changed its refresh rate to {core_fps:.4} fps; pacing follows it",
                    session.core_id
                );
                pacer.set_target_fps(core_fps);
            }
        }

        // 3b. Rewind tape. Counted in core steps rather than ticks, so the spacing between
        //     snapshots stays even while fast-forwarding, and skipped entirely when the
        //     pacer ran no steps, since that would record a state identical to the last.
        if plan.steps > 0 && rewind.is_enabled() {
            *frames_since_snapshot += plan.steps;
            if *frames_since_snapshot >= *rewind_interval {
                *frames_since_snapshot = 0;
                let size = session.core.state_size();
                let core = &session.core;
                // A refused snapshot must not fail the tick. Losing one rewind point is a
                // far smaller thing than dropping a frame, so this is logged at debug and
                // the tick carries on.
                if let Err(err) = rewind.push_with(size, |dst| core.save_state(dst)) {
                    log::debug!("rewind snapshot skipped: {err}");
                }
            }
        }

        // 4. GPU. With zero steps there is no new frame, so the previous texture is
        //    re-presented rather than skipping present entirely (which would stall
        //    the compositor's expectations).
        //
        // Hardware path: if Beetle (or another Vulkan core) called set_image, try to
        // export the VkImage via VK_EXT_metal_objects and adopt it before present.
        // Soft failure leaves the previous texture up — honest Partial until a device
        // frame proves the full chain.
        let mut presented = false;
        if let Some(renderer) = renderer.as_mut() {
            // A core may call SET_ROTATION at any time; these are plain field writes.
            actions.push_to(renderer, session.core.rotation());
            let phases = plan.steps > 0 && crate::gfx::vulkan_hw::strict_sync();
            if plan.steps > 0 {
                if phases {
                    crate::feedback::frame_phase(session.core.frame_count(), "adopt");
                }
                if let Err(err) = crate::gfx::vulkan_hw::apply_pending_to_renderer(renderer) {
                    log::debug!("vulkan HW adopt skipped: {err}");
                }
            }
            let frame = if plan.steps > 0 {
                session.core.video()
            } else {
                None
            };
            if phases {
                crate::feedback::frame_phase(session.core.frame_count(), "present");
            }
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

    /// Moves a pointer: the Nintendo DS touch screen, and any later system with one.
    ///
    /// `x` and `y` are fractions of the WHOLE framebuffer in `0.0..=1.0` from the top left, not of
    /// one screen. That matters for the DS, whose framebuffer is both screens stacked: the touch
    /// screen is the lower half, so a tap at the very top of it is `y = 0.5`. The host does that
    /// arithmetic because only the host knows where it drew the picture.
    pub fn set_pointer(&mut self, port: usize, source: PadSource, x: f32, y: f32, pressed: bool) {
        self.gamepads.set_pointer(port, source, x, y, pressed);
    }

    /// Relative mouse motion, buttons and wheel for one layer. See `input::MouseState`.
    pub fn add_mouse_motion(&mut self, port: usize, source: PadSource, dx: f32, dy: f32) {
        self.gamepads.add_mouse_motion(port, source, dx, dy);
    }

    pub fn set_mouse_buttons(
        &mut self,
        port: usize,
        source: PadSource,
        left: bool,
        right: bool,
        middle: bool,
    ) {
        self.gamepads
            .set_mouse_buttons(port, source, left, right, middle);
    }

    pub fn add_mouse_wheel(&mut self, port: usize, source: PadSource, vertical: i32, horizontal: i32) {
        self.gamepads.add_mouse_wheel(port, source, vertical, horizontal);
    }

    /// Tells the running core which device is plugged into a port. See
    /// [`EmulatorCore::set_controller_port_device`].
    pub fn set_controller_port_device(&mut self, port: u32, device: u32) -> Result<(), BridgeError> {
        // Plugging a different device into the emulated console on one phone is a different
        // machine. Every port switch (mouse mode, the analog toggle, the controller type) comes
        // through here, so this is the one guard that covers them all.
        self.refuse_during_netplay("changing the controller type")?;
        let session = self.session.as_mut().ok_or(BridgeError::NoSession)?;
        session.core.set_controller_port_device(port, device)
    }

    /// The device ids the running core declared for a port through `SET_CONTROLLER_INFO`.
    pub fn controller_types(&self, port: u32) -> Vec<(String, u32)> {
        self.session
            .as_ref()
            .map(|s| s.core.controller_types(port))
            .unwrap_or_default()
    }

    /// Switches a port between the joypad and a mouse for the touch screen's trackpad mode.
    ///
    /// Returns a plain line for the HUD either way. The input layer answers `RETRO_DEVICE_MOUSE`
    /// queries whatever the port's device is, so a core that reads the mouse without being told
    /// (melonDS in its mouse touch mode, a computer core) works even when the port switch is
    /// refused. A core that DECLARED a mouse type for the port (the SNES mouse, the PlayStation
    /// mouse) is switched to it, because those only read the mouse once told it is plugged in.
    pub fn set_mouse_mode(&mut self, port: u32, enabled: bool) -> String {
        if self.session.is_none() {
            return "mouse mode: no game is running".into();
        }
        // Answered here rather than left to `set_controller_port_device`, whose refusal would
        // arrive wrapped in a "could not switch" line that hides the reason.
        if let Some(line) = self.netplay_refusal("mouse mode") {
            return line;
        }
        let types = self.controller_types(port);
        let choice = if enabled {
            crate::cores::pick_mouse_device(&types)
        } else {
            Some(crate::cores::pick_joypad_device(&types))
        };
        let player = port + 1;
        match choice {
            None => format!(
                "mouse mode on; this core declares no mouse for player {player}, so only games that read the mouse directly will see it"
            ),
            Some((name, device)) => match self.switch_port_device(port, device) {
                Ok(()) if enabled => format!("mouse mode on: player {player} is now \"{name}\""),
                Ok(()) => format!("mouse mode off: player {player} is back to \"{name}\""),
                Err(error) => format!("mouse mode: could not switch player {player} to \"{name}\": {error}"),
            },
        }
    }

    /// Releases one source, e.g. when the touch overlay is dismissed.
    pub fn release_input_source(&mut self, source: PadSource) {
        self.gamepads.release_source(source);
    }

    /// Which buttons the on-screen pad is holding as TURBO on `port`, W3C standard order. See
    /// [`GamepadBridge::apply_turbo_standard`].
    pub fn apply_turbo(&mut self, port: usize, buttons: &[bool]) {
        self.gamepads.apply_turbo_standard(port, buttons);
    }

    /// Core frames down, then up, per turbo cycle. See
    /// [`GamepadBridge::set_turbo_half_period`].
    pub fn set_turbo_half_period(&mut self, frames: u32) {
        self.gamepads.set_turbo_half_period(frames);
    }

    pub fn turbo_half_period(&self) -> u32 {
        self.gamepads.turbo_half_period()
    }

    /// Applies one poll of a W3C standard gamepad. See
    /// [`GamepadBridge::apply_standard_gamepad`].
    pub fn apply_gamepad(&mut self, port: usize, buttons: &[bool], axes: &[f32]) {
        self.gamepads.apply_standard_gamepad(port, buttons, axes);
    }

    /// Applies one poll of a W3C standard gamepad to a named layer. See
    /// [`GamepadBridge::apply_standard_gamepad_from`] for why the layer matters.
    pub fn apply_gamepad_from(
        &mut self,
        port: usize,
        source: PadSource,
        buttons: &[bool],
        axes: &[f32],
    ) {
        self.gamepads
            .apply_standard_gamepad_from(port, source, buttons, axes);
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
    /// Not knowable before the host's audio engine has started, so this
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
        let written = self.sink.drain(dst);
        self.apply_gain(&mut dst[..written]);
        written
    }

    /// Scales a drained block by the wanted volume, ramping rather than stepping.
    ///
    /// Applied here, on the far side of the ring, for two reasons. The ring holds audio
    /// that was queued up to a few frames ago, so scaling on the way *in* would mean a
    /// volume change did not take effect until that backlog drained. And this runs on the
    /// display link, never on the platform's real-time render thread, so the arithmetic is
    /// nowhere near the callback that must not miss its deadline.
    ///
    /// The ramp matters. A volume slider under a finger produces a new target every frame,
    /// and jumping the gain at a block boundary puts a step discontinuity into the
    /// waveform, which is heard as a click - sixty of them a second while dragging. Gliding
    /// across the block instead spreads the change over its samples and is inaudible.
    fn apply_gain(&mut self, block: &mut [f32]) {
        let target = self.volume;
        // Already there: one multiply per sample, or none at all at unity.
        if (self.gain - target).abs() <= f32::EPSILON {
            self.gain = target;
            if target < 1.0 {
                for sample in block.iter_mut() {
                    *sample *= target;
                }
            }
            return;
        }
        let frames = block.len() / CHANNELS;
        if frames == 0 {
            // Nothing to ramp across. Taking the new value here would be a step change on
            // the next non-empty block, so the glide is left pending instead.
            return;
        }
        let step = (target - self.gain) / frames as f32;
        let mut gain = self.gain;
        for frame in block.chunks_mut(CHANNELS) {
            gain += step;
            for sample in frame.iter_mut() {
                *sample *= gain;
            }
        }
        self.gain = target;
    }

    /// Sets the output gain. `0.0` is silence, `1.0` is the core's own level.
    ///
    /// Clamped rather than rejected: a host sending `1.5` wants "as loud as possible", and
    /// amplifying past unity would clip a core that is already mixing near full scale.
    pub fn set_volume(&mut self, volume: f32) {
        self.volume = if volume.is_finite() {
            volume.clamp(0.0, 1.0)
        } else {
            // NaN compares false against everything, so `clamp` would panic on it.
            1.0
        };
    }

    pub fn volume(&self) -> f32 {
        self.volume
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
        self.filter = filter;
        if let Some(renderer) = &mut self.renderer {
            renderer.set_filter(filter);
        }
    }

    pub fn filter(&self) -> ScaleFilter {
        self.filter
    }

    pub fn set_scale_mode(&mut self, mode: ScaleMode) {
        self.scale_mode = mode;
        if let Some(renderer) = &mut self.renderer {
            renderer.set_scale_mode(mode);
        }
    }

    pub fn scale_mode(&self) -> ScaleMode {
        self.scale_mode
    }

    /// Skin screen holes for the current orientation. Empty restores aspect-fit.
    pub fn set_skin_holes(&mut self, holes: Vec<SkinHole>) {
        self.skin_holes = holes.clone();
        if let Some(renderer) = &mut self.renderer {
            renderer.set_skin_holes(holes);
        }
    }

    // ------------------------------------------------------------ two screens

    /// Sets the two-screen layout, swap, orientation hint and TV choice in one go.
    ///
    /// Remembered across sessions and renderer re-targets. Harmless for one-screen systems: with
    /// no geometry the renderer never reads it.
    pub fn set_dual_screen_config(&mut self, config: DualScreenConfig) {
        self.dual_config = config;
        if let Some(renderer) = &mut self.renderer {
            renderer.set_dual_config(config);
        }
    }

    pub fn dual_screen_config(&self) -> DualScreenConfig {
        self.dual_config
    }

    /// The one-tap swap. Returns the new swapped state.
    pub fn toggle_screen_swap(&mut self) -> bool {
        let mut config = self.dual_config;
        config.swapped = !config.swapped;
        self.set_dual_screen_config(config);
        config.swapped
    }

    /// Whether the running game has two screens.
    pub fn has_dual_screens(&self) -> bool {
        self.dual_geometry.is_some()
    }

    /// Whether the current skin has a top hole and a bottom hole for the swap to trade.
    pub fn skin_can_swap(&self) -> bool {
        self.renderer.as_ref().is_some_and(Renderer::skin_can_swap)
    }

    /// Where the touch screen is on the phone's picture view of `view_w x view_h`.
    pub fn touch_region(&self, view_w: f32, view_h: f32) -> Option<TouchRegion> {
        self.renderer.as_ref()?.touch_region(view_w, view_h)
    }

    /// A point on the phone's picture view, as fractions of it, to the framebuffer fraction the
    /// pointer API takes. `None` when it is not on the touch screen and `clamp` is false.
    pub fn map_touch(
        &self,
        view_w: f32,
        view_h: f32,
        x: f32,
        y: f32,
        clamp: bool,
    ) -> Option<(f32, f32)> {
        let region = self.touch_region(view_w, view_h)?;
        crate::gfx::screen_layout::map_touch(&region, x, y, clamp)
    }

    /// Width over height of what the phone shows, when that is not the core's own aspect.
    pub fn phone_arrangement_aspect(&self) -> Option<f32> {
        self.renderer.as_ref()?.phone_arrangement_aspect()
    }

    /// Width over height the running core says its own picture is, which is the shape the
    /// renderer draws it with. `None` when nothing is running.
    ///
    /// The core is the authority here, not the manifest: this is what it answered through
    /// `retro_get_system_av_info` once the game was loaded, and
    /// `cores::native_core::sync_descriptor_from_core` keeps it current if the core changes
    /// shape mid-game. Worth asking for a core whose shape depends on the cartridge or on its
    /// own settings — cBoy runs a Super Game Boy cartridge at 256x224 (8:7) and a plain Game
    /// Boy at 160x144 (10:9), which its settings alone cannot tell apart.
    pub fn core_picture_aspect(&self) -> Option<f32> {
        let aspect = self.session.as_ref()?.core.descriptor().geometry.aspect_ratio;
        (aspect.is_finite() && aspect > 0.0).then_some(aspect)
    }

    /// Whether a TV is the main target.
    pub fn external_display_active(&self) -> bool {
        self.renderer.as_ref().is_some_and(Renderer::external_active)
    }

    /// Drops the TV's surface. Returns whether there was one.
    pub fn detach_external_display(&mut self) -> bool {
        self.renderer
            .as_mut()
            .is_some_and(Renderer::detach_external_surface)
    }

    pub fn resize_external_display(&mut self, width: u32, height: u32) {
        if let Some(renderer) = &mut self.renderer {
            renderer.resize_external(width, height);
        }
    }

    /// Sets the speed multiplier. `1.0` is native; above it is fast-forward.
    ///
    /// Note the real ceiling is lower than the accepted one. `FramePacer` clamps to
    /// `0.05..=16.0`, but it also refuses to run more than `MAX_CATCH_UP_STEPS` core steps
    /// in a single display tick, which on a 60 Hz screen puts the achievable rate at about
    /// 4x however large a multiplier is asked for. Anything beyond that is forfeited and
    /// shows up as a climbing dropped-step count rather than as extra speed.
    pub fn set_speed(&mut self, speed: f64) {
        // Fast forward on one phone would run that phone ahead of the other; lockstep would
        // only stall it, so the request is ignored rather than half-honoured.
        let speed = if self.netplay_is_live() { 1.0 } else { speed };
        self.pacer.set_speed(speed);
        // Read back rather than storing the argument, so what is remembered is what the
        // pacer actually accepted after clamping.
        self.speed = self.pacer.speed();
        self.apply_audio_speed();
    }

    pub fn speed(&self) -> f64 {
        self.pacer.speed()
    }

    /// Re-declares the sink's source rate as `native * speed`. See
    /// [`crate::audio::AudioSink::set_source_rate`] for why this is what fast-forward
    /// needs, and note it is a no-op until a session exists to have a native rate.
    fn apply_audio_speed(&mut self) {
        if self.native_audio_rate == 0 {
            return;
        }
        let adjusted = (f64::from(self.native_audio_rate) * self.speed).round();
        // A sample rate is a u32 and the pacer's 16x ceiling cannot overflow one from any
        // realistic core rate, but clamping keeps the cast total rather than merely
        // very likely to be fine.
        let adjusted = adjusted.clamp(1.0, f64::from(u32::MAX)) as u32;
        self.sink.set_source_rate(adjusted);
    }

    /// Size in bytes of a save state for the running core, or `0` if it has none.
    ///
    /// Queried live rather than cached because libretro permits the figure to change
    /// during a session - a disc swap is the usual reason. A rewind buffer sizing itself
    /// from this must therefore cope with the answer moving under it.
    pub fn state_size(&self) -> usize {
        self.session
            .as_ref()
            .map_or(0, |session| session.core.state_size())
    }

    /// Captures the current frame as tightly packed RGBA8, returning its width and height too.
    ///
    /// The game's picture in the shape it is shown at, with the player's filter, look and
    /// rotation, and without the skin or two-screen layout around it (see
    /// `Renderer::encode_capture` for why). A zero `width` or `height` is taken from that shape;
    /// both zero is its natural size, scaled up to a sharp size.
    ///
    /// **This blocks until the GPU has finished, and that is safe here.** A readback is
    /// inherently two-step, because the copy has to complete before the bytes can be read, so
    /// something has to wait.
    ///
    /// The wait is a synchronous device poll rather than an await. Nothing else runs on this
    /// thread while it blocks, so there is no re-entrancy to guard against: the display link, the
    /// UI and this call are all the main thread, and the one thread that is NOT the main thread,
    /// the audio render callback, is specifically designed never to touch this engine. So the
    /// simple version is correct, and the cost is a few milliseconds of main thread on a
    /// deliberate user action rather than on any frame path.
    pub fn capture_rgba(
        &mut self,
        width: u32,
        height: u32,
    ) -> Result<(u32, u32, Vec<u8>), BridgeError> {
        // Submits the copy, and ends the mutable borrow of the renderer before the device is
        // borrowed again below.
        let capture = self.encode_capture(width, height)?;
        let renderer = self.renderer.as_ref().ok_or(BridgeError::NoRenderer)?;

        // The callback is empty on purpose. Its result would say whether the mapping succeeded,
        // and `take_rgba` already answers that by failing with "capture not mapped" when it did
        // not, so plumbing the result through a channel would add a second way to learn the same
        // thing.
        capture.buffer().slice(..).map_async(wgpu::MapMode::Read, |_| {});
        // `submission_index: None` waits for everything queued rather than for one specific
        // submission, which is what is wanted: `encode_capture` submitted the copy immediately
        // before this. `timeout: None` blocks until the GPU is done rather than giving up after an
        // interval, because a capture that silently returned a half-copied buffer would be worse
        // than one that took a few milliseconds longer.
        renderer
            .wgpu_device()
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: None,
            })
            .map_err(|err| {
                BridgeError::Gfx(crate::error::GfxError::InvalidFrame(format!(
                    "waiting for the capture to finish failed: {err}"
                )))
            })?;

        // Read before `take_rgba`, which consumes the capture so a buffer cannot be read twice or
        // left mapped.
        let (width, height) = (capture.width(), capture.height());
        Ok((width, height, capture.take_rgba()?))
    }

    /// The running core's own version string, for save-state compatibility.
    ///
    /// See [`crate::cores::EmulatorCore::version`]. A host storing save states should record
    /// this beside each one and refuse to load a state whose recorded version differs, because
    /// `retro_unserialize` will not reliably refuse it itself.
    pub fn core_version(&self) -> Option<String> {
        self.session
            .as_ref()
            .and_then(|session| session.core.version())
            .map(str::to_owned)
    }

    /// Submits a GPU readback of the presented image.
    ///
    /// A zero `width` or `height` means "from the game's own shape" (both zero: its natural
    /// size, scaled up; see `Renderer::capture_size`). The caller awaits the buffer map and then
    /// calls [`crate::gfx::FrameCapture::take_rgba`].
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

        // A zero side means "the game's own shape", resolved by the renderer AFTER the upload
        // above, so it is the current frame's size that decides it. See
        // `Renderer::capture_size`.
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
        self.refuse_during_netplay("loading a state")?;
        self.load_state_unchecked(data)
    }

    /// The load itself, shared by [`Self::load_state`] and the netplay handshake (which is the
    /// one state load online play needs).
    fn load_state_unchecked(&mut self, data: &[u8]) -> Result<(), BridgeError> {
        let session = self.session.as_mut().ok_or(BridgeError::NoSession)?;
        session.core.load_state(data)?;
        // A save state can carry the memory a cheat was patching, so the list is
        // re-pushed for the restored timeline.
        Self::push_cheats(session)?;
        // The audio backlog belongs to the abandoned timeline.
        self.sink.flush();
        // Achievement progress was measured on that timeline too.
        self.achievements_reset();
        // So does the rewind tape. Every snapshot on it describes a future that no longer
        // follows from the present, and rewinding into it would jump the player somewhere
        // they never were.
        self.rewind.clear();
        self.frames_since_snapshot = 0;
        Ok(())
    }

    // ------------------------------------------------------------------- rewind

    /// One tick spent going backwards instead of forwards.
    ///
    /// Restores the newest snapshot and then runs **exactly one** core frame. That single
    /// frame is not a rounding error, it is the point: a libretro core's framebuffer is
    /// whatever `retro_run` last wrote, and `retro_unserialize` changes the core's memory
    /// without redrawing anything. Loading a state and presenting immediately would show the
    /// image from before the rewind, so the screen would freeze while the emulator silently
    /// travelled. One frame turns the restored state into a picture.
    ///
    /// Net motion is therefore one snapshot interval minus one frame per tick, which at the
    /// default spacing is a brisk scrub backwards rather than a mirror of real time. That is
    /// what rewind is normally for.
    ///
    /// Audio is produced by that frame and then dropped on the floor. The alternative, not
    /// draining the core at all, lets the core's internal batch buffer grow unbounded, and
    /// playing it would be a forward-running fragment several times a second while the
    /// picture runs backwards. Silence while rewinding is what most emulators do and is
    /// easily the least strange of the three.
    fn tick_rewinding(&mut self, now_ms: f64) -> Result<TickReport, BridgeError> {
        let stepped_back = self.rewind_step()?;
        let snapshot = self.gamepads.snapshot();

        // Re-anchored every rewind tick, not once when the button comes up. The pacer is not
        // consulted on this path, so without this its idea of "last tick" would stay frozen
        // at the moment rewind began; releasing the button after a third of a second would
        // then look like a third of a second of missed emulation and be answered with a
        // sprint forwards. Keeping it current means the first normal tick after a rewind sees
        // an ordinary one-frame gap.
        self.pacer.resync(now_ms);

        let Self {
            session,
            renderer,
            sink,
            pacer,
            gamepads,
            ..
        } = self;
        let session = session
            .as_mut()
            .expect("the caller checked a session exists and nothing above clears it");

        if stepped_back {
            let mut snapshot = snapshot;
            crate::input::core_layout::translate_for(&session.core_id, &mut snapshot);
            session.core.run_frame(&snapshot)?;
            Self::apply_pokes(session);
            session.core.drain_audio(sink.as_mut());
            sink.flush();
            gamepads.end_mouse_frame();
        }

        // Presented either way. Reaching the start of the tape stops the motion but must not
        // stop the display, or a held button at the end of history would look like a hang.
        let mut presented = false;
        if let Some(renderer) = renderer.as_mut() {
            let frame = if stepped_back {
                session.core.video()
            } else {
                None
            };
            renderer.present(frame)?;
            presented = true;
        }

        Ok(TickReport {
            steps: u32::from(stepped_back),
            presented,
            display_fps: pacer.display_fps(),
            frame_count: session.core.frame_count(),
            audio: sink.stats(),
            ..Default::default()
        })
    }

    /// Starts or stops walking backwards. Held-button shaped: set it true on press, false on
    /// release, and the engine handles the rest inside its own tick.
    pub fn set_rewinding(&mut self, rewinding: bool) {
        self.rewinding = rewinding && !self.netplay_is_live();
    }

    pub fn is_rewinding(&self) -> bool {
        self.rewinding
    }

    /// Sets the rewind memory ceiling in bytes. `0` disables rewind.
    ///
    /// How much time the budget buys depends on the system: the same 64 MB is minutes of
    /// NES and seconds of PlayStation, because their save states differ by two orders of
    /// magnitude. [`RewindBuffer`] explains why the budget is expressed in memory rather
    /// than in seconds.
    pub fn set_rewind_budget_bytes(&mut self, budget_bytes: usize) {
        self.rewind.set_budget_bytes(budget_bytes);
    }

    pub fn rewind_budget_bytes(&self) -> usize {
        self.rewind.budget_bytes()
    }

    /// Core frames between snapshots. Clamped to at least 1; 0 would snapshot every frame
    /// twice over and is meaningless.
    pub fn set_rewind_interval(&mut self, frames: u32) {
        self.rewind_interval = frames.max(1);
    }

    pub fn rewind_interval(&self) -> u32 {
        self.rewind_interval
    }

    /// Snapshots held, bytes held, and how many have been evicted to stay in budget.
    pub fn rewind_stats(&self) -> (u32, u64, u64) {
        (
            self.rewind.len() as u32,
            self.rewind.bytes() as u64,
            self.rewind.evicted(),
        )
    }

    /// Steps one snapshot backwards. Returns `false` when the tape is empty.
    ///
    /// An empty tape is not an error: it is the beginning of recorded history, and a held
    /// rewind button reaching it should stop rather than throw. The snapshot is consumed,
    /// so rewinding walks backwards and does not sit on one frame.
    ///
    /// The audio backlog is flushed for the same reason [`EmulatorBridge::load_state`]
    /// flushes it - those samples belong to the timeline just abandoned. The tape itself is
    /// deliberately *not* cleared here, which is what separates rewinding from loading a
    /// state: one walks back along recorded history, the other leaves it.
    pub fn rewind_step(&mut self) -> Result<bool, BridgeError> {
        if self.session.is_none() {
            return Err(BridgeError::NoSession);
        }
        self.refuse_during_netplay("rewind")?;
        let Some(snapshot) = self.rewind.pop() else {
            return Ok(false);
        };

        // The buffer goes back to the pool whether or not the load worked, so a core that
        // rejects a state does not leak it out of the tape's memory budget.
        let outcome = {
            let session = self
                .session
                .as_mut()
                .expect("checked immediately above, and nothing here can clear it");
            session
                .core
                .load_state(&snapshot)
                .and_then(|()| Self::push_cheats(session))
        };
        self.rewind.recycle(snapshot);
        outcome?;

        self.sink.flush();
        self.achievements_reset();
        // The next snapshot is a full interval away from the point just restored, not from
        // wherever the counter happened to be when the button was pressed.
        self.frames_since_snapshot = 0;
        Ok(true)
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
    /// `enabled` is a parallel byte array (non-zero means on). The Swift-facing `apply_cheats`
    /// in `uniffi_api.rs` takes a bool list and converts, so this form never reaches Swift.
    ///
    /// @returns how many cheats are switched on
    pub fn apply_cheats(
        &mut self,
        codes: Vec<String>,
        enabled: &[u8],
    ) -> Result<usize, BridgeError> {
        if self.netplay_is_live() {
            return Err(BridgeError::Cheat(
                "changing cheats is switched off during online play".into(),
            ));
        }
        let session = self.session.as_mut().ok_or(BridgeError::NoSession)?;
        if !session.core.supports_cheats() {
            return Err(BridgeError::Cheat(format!(
                "core '{}' does not support cheats",
                session.core_id
            )));
        }

        // RAM pokes (`poke:ADDRESS:VALUE:BYTES`, see `cheats::poke`) live in the same list the
        // user sees, and are split out HERE so the core never receives a code it would misread.
        // They are not part of the core's indexed table at all, so taking them out cannot
        // renumber it: the core's table is still the typed codes, whole and in their list order.
        //
        // Every poke is parsed before anything is changed, so one malformed entry refuses the
        // whole update and leaves the previous list in force rather than half-applying a new one.
        let mut core_cheats = Vec::new();
        let mut pokes = Vec::new();
        let mut enabled_pokes = 0usize;
        let mut enabled_codes = 0usize;
        for (i, code) in codes.into_iter().enumerate() {
            let code = code.trim().to_string();
            // Blank lines are dropped here rather than skipped during the push, because
            // skipping would leave gaps in the index sequence the core is given.
            if code.is_empty() {
                continue;
            }
            let on = enabled.get(i).copied().unwrap_or(0) != 0;
            if Poke::is_poke_code(&code) {
                let poke = Poke::parse(&code).map_err(BridgeError::Cheat)?;
                if on {
                    pokes.push(poke);
                    enabled_pokes += 1;
                }
                continue;
            }
            // Written the way this core reads it (a raw code as Pro Action Replay on Gearsystem
            // and bsnes, one entry per part on Gearsystem). Pushed in order, so the core's index
            // sequence stays contiguous.
            for part in crate::cheats::formats::normalise_for_core(&session.core_id, &code) {
                core_cheats.push(Cheat { code: part, enabled: on });
            }
            enabled_codes += on as usize;
        }
        session.cheats = core_cheats;
        session.enabled_codes = enabled_codes;
        session.pokes = pokes;

        Self::push_cheats(session)?;
        Ok(enabled_codes + enabled_pokes)
    }

    /// Clears every cheat, in the core and in our record of it.
    pub fn clear_cheats(&mut self) -> Result<(), BridgeError> {
        // Refused online for the same reason `apply_cheats` is: the other phone keeps its cheats.
        self.refuse_during_netplay("clearing cheats")?;
        let session = self.session.as_mut().ok_or(BridgeError::NoSession)?;
        session.cheats.clear();
        session.enabled_codes = 0;
        session.pokes.clear();
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

    /// How many cheats are currently switched on: the user's codes (not the core entries they
    /// became) and pokes, the same count `apply_cheats` returns.
    pub fn active_cheat_count(&self) -> usize {
        self.session.as_ref().map_or(0, |session| session.enabled_codes + session.pokes.len())
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
        // A core reads its options as it runs, so one phone's change is a different machine.
        self.refuse_during_netplay("changing a core setting")?;
        let session = self.session.as_mut().ok_or(BridgeError::NoSession)?;
        session.core.set_core_option(key, value)
    }

    // ------------------------------------------------------------------- memory

    /// Size in bytes of one of the running core's memory regions, `0` when it has none.
    pub fn memory_size(&self, region: u32) -> usize {
        self.session
            .as_ref()
            .and_then(|session| session.core.memory_region(region))
            .map_or(0, <[u8]>::len)
    }

    /// Copies `len` bytes of a memory region starting at `offset`.
    pub fn read_memory(&self, region: u32, offset: u64, len: u64) -> Result<Vec<u8>, BridgeError> {
        let session = self.session.as_ref().ok_or(BridgeError::NoSession)?;
        let memory = session
            .core
            .memory_region(region)
            .ok_or_else(|| Self::no_region(session, region))?;
        let range = crate::memory::checked_range(memory.len(), offset, len)
            .map_err(|why| BridgeError::Memory(format!("{}: {why}", crate::memory::region_name(region))))?;
        Ok(memory[range].to_vec())
    }

    /// Writes `bytes` into a memory region at `offset`. All or nothing: a write that would run
    /// past the end is refused before a byte is changed.
    pub fn write_memory(&mut self, region: u32, offset: u64, bytes: &[u8]) -> Result<(), BridgeError> {
        self.refuse_during_netplay("writing to the game's memory")?;
        let session = self.session.as_mut().ok_or(BridgeError::NoSession)?;
        let core_name = session.core.descriptor().display_name.clone();
        let Some(memory) = session.core.memory_region_mut(region) else {
            return Err(BridgeError::Memory(format!(
                "{core_name} exposes no {}",
                crate::memory::region_name(region)
            )));
        };
        let range = crate::memory::checked_range(memory.len(), offset, bytes.len() as u64)
            .map_err(|why| BridgeError::Memory(format!("{}: {why}", crate::memory::region_name(region))))?;
        memory[range].copy_from_slice(bytes);
        Ok(())
    }

    fn no_region(session: &Session, region: u32) -> BridgeError {
        BridgeError::Memory(format!(
            "{} exposes no {}",
            session.core.descriptor().display_name,
            crate::memory::region_name(region)
        ))
    }

    // ------------------------------------------------------------ battery save

    /// The running game's battery save (`SAVE_RAM`), byte for byte. This is the `.srm` file
    /// every libretro frontend writes, so it is also what an export hands to another emulator.
    ///
    /// For BlastEm on a Mega Drive cartridge this is the GPGX layout every older Mega Drive save
    /// is in (see
    /// `battery_layout`), so the file is the same whichever core wrote it.
    pub fn battery_save(&self) -> Result<Vec<u8>, BridgeError> {
        let size = self.memory_size(crate::memory::MEMORY_SAVE_RAM);
        if size == 0 {
            let session = self.session.as_ref().ok_or(BridgeError::NoSession)?;
            return Err(Self::no_region(session, crate::memory::MEMORY_SAVE_RAM));
        }
        let live = self.read_memory(crate::memory::MEMORY_SAVE_RAM, 0, size as u64)?;
        match self.session.as_ref().map(|s| s.battery_layout) {
            Some(BatteryLayout::MegaDriveGpgx(sram)) => Ok(battery_layout::live_to_gpgx(&live, &sram)),
            _ => Ok(live),
        }
    }

    /// The ROM's first bytes (de-interleaved for `.smd`) decide the battery layout. Read from the
    /// bytes handed to the core, or from the file when the core was given a path. A Sonic & Knuckles
    /// ROM on BlastEm is read whole, since its save is described in the locked-on half.
    fn battery_layout_for(core_id: &str, content: &[u8], hint: &ContentHint) -> BatteryLayout {
        const HEAD: usize = 0x4200;
        // Past a 4 MB lock-on ROM and an .smd header: nothing larger is one.
        const WHOLE: usize = 0x40_0200;
        let read = |limit: usize| -> Option<Vec<u8>> {
            if !content.is_empty() {
                return Some(content[..content.len().min(limit)].to_vec());
            }
            use std::io::Read;
            let file = std::fs::File::open(hint.full_path.as_deref()?).ok()?;
            let mut buf = Vec::with_capacity(limit.min(HEAD));
            let _ = file.take(limit as u64).read_to_end(&mut buf);
            Some(buf)
        };
        let Some(head) = read(HEAD) else {
            return BatteryLayout::Raw;
        };
        let mut head = battery_layout::deinterleave_smd(&hint.extension, &head);
        if core_id == "blastem" && battery_layout::needs_whole_rom(&head) {
            // One byte past the limit, so a file too big to be the lock-on ROM is told apart.
            if let Some(whole) = read(WHOLE + 1).filter(|w| w.len() <= WHOLE) {
                head = battery_layout::deinterleave_smd(&hint.extension, &whole);
            }
        }
        battery_layout::layout_for(core_id, &hint.extension, &head)
    }

    /// Replaces the running game's battery save. Returns the region's size.
    ///
    /// A file SHORTER than the region is accepted and written over its start, because several
    /// emulators trim trailing unused bytes from `.srm` files and a 32 KB SRAM cartridge saved by
    /// one of them is still that cartridge's save. A file LONGER than the region is refused: it is
    /// almost certainly a different game's save (or a save state), and truncating it would be
    /// writing garbage into the cartridge.
    ///
    /// A game reads its battery RAM when it boots, so a restore into a game that is already past
    /// its title screen generally needs a reset before the game notices. The caller says so.
    pub fn restore_battery_save(&mut self, data: &[u8]) -> Result<usize, BridgeError> {
        // Checked here as well as in `write_memory` so the sentence names what the user did.
        self.refuse_during_netplay("restoring a battery save")?;
        let size = self.memory_size(crate::memory::MEMORY_SAVE_RAM);
        if size == 0 {
            let session = self.session.as_ref().ok_or(BridgeError::NoSession)?;
            return Err(Self::no_region(session, crate::memory::MEMORY_SAVE_RAM));
        }
        if data.is_empty() {
            return Err(BridgeError::Memory("that battery save is empty".into()));
        }
        let (core_id, layout, ext) = match self.session.as_ref() {
            Some(session) => (
                session.core_id.clone(),
                session.battery_layout,
                session.content_ext.clone(),
            ),
            None => return Err(BridgeError::NoSession),
        };
        // BlastEm on a Mega Drive cartridge: the file is in the GPGX layout.
        if let BatteryLayout::MegaDriveGpgx(sram) = layout {
            let live = battery_layout::gpgx_to_live(data, &sram, size).map_err(BridgeError::Memory)?;
            self.write_memory(crate::memory::MEMORY_SAVE_RAM, 0, &live)?;
            return Ok(size);
        }
        // Padding another emulator added past the end of the save RAM (0xFF, or 0x00 for bsnes).
        let data = battery_layout::fit_padded(data, size, &core_id);
        // A Game Boy save written by mGBA or VBA for a cart with a clock (MBC3, e.g. Pokemon
        // Gold) carries the clock after the RAM. A core that keeps the clock elsewhere (cBoy) has
        // save RAM of exactly the cart's size, so the RAM part is taken and the clock footer left
        // out rather than refusing the save. Game Boy files only: see `battery_layout::clock_footer`.
        let data = if battery_layout::clock_footer(data.len(), size, &ext).is_some() {
            &data[..size]
        } else {
            data
        };
        if data.len() > size {
            return Err(BridgeError::Memory(format!(
                "that battery save is {} bytes and this game's save RAM is {size}, so it belongs \
                 to a different game or is not a battery save",
                data.len()
            )));
        }
        self.write_memory(crate::memory::MEMORY_SAVE_RAM, 0, data)?;
        Ok(size)
    }

    /// Writes the battery save to `path` atomically (a temporary file, then a rename), so an app
    /// killed mid-write leaves the previous save intact rather than a truncated one.
    ///
    /// THE FRONTEND OWNS THIS FILE. libretro hands battery RAM to the frontend through
    /// `retro_get_memory_data(RETRO_MEMORY_SAVE_RAM)` and expects the frontend to persist it; most
    /// cores (fceumm, mGBA, bsnes, Gearsystem) never write a save file of their own. Until
    /// this existed, an in-game save survived only inside a save state.
    ///
    /// Returns the bytes written, `0` when the game has no battery RAM (not an error: most NES
    /// games have none).
    ///
    /// THE CARTRIDGE CLOCK goes next to it, as `<game>.rtc` (the name RetroArch uses), for a core
    /// that keeps the clock apart from save RAM in `RETRO_MEMORY_RTC`: cBoy does for MBC3 and HuC3
    /// cartridges (Pokemon Gold, Silver, Crystal). Same triggers as the `.srm`. Without it the
    /// clock lived only inside the auto-save state, froze while the game was closed and went back
    /// to day 0 whenever that state was missing or refused.
    pub fn persist_battery_save(&self, path: &str) -> Result<usize, BridgeError> {
        let mut written = 0;
        if self.memory_size(crate::memory::MEMORY_SAVE_RAM) > 0 {
            let data = self.keep_clock_footer(path, self.battery_save()?);
            written += Self::write_save_file(std::path::Path::new(path), &data, "srm.writing")?;
        }
        let clock_size = self.memory_size(crate::memory::MEMORY_RTC);
        if clock_size > 0 {
            let clock = self.read_memory(crate::memory::MEMORY_RTC, 0, clock_size as u64)?;
            written += Self::write_save_file(&Self::clock_path(path), &clock, "rtc.writing")?;
        }
        Ok(written)
    }

    /// `data` with the cartridge-clock footer the file at `path` already has put back after it
    /// (see `battery_layout::clock_footer`), or `data` unchanged when there is no footer.
    ///
    /// An imported mGBA or VBA Game Boy save keeps its clock after the RAM, and the core keeps its
    /// own clock elsewhere (`<game>.rtc`), so writing only the RAM back would throw those bytes
    /// away on the first periodic flush, even if the player never saved. They are kept both
    /// because the file is still the file another emulator can read, and because an unchanged save
    /// then compares equal to what is on disk, so nothing is rewritten at all.
    fn keep_clock_footer(&self, path: &str, data: Vec<u8>) -> Vec<u8> {
        let Some(session) = self.session.as_ref() else {
            return data;
        };
        let Ok(old) = std::fs::read(path) else {
            return data;
        };
        match battery_layout::clock_footer(old.len(), data.len(), &session.content_ext) {
            Some(footer) => {
                let mut out = data;
                out.extend_from_slice(&old[old.len() - footer..]);
                out
            }
            None => data,
        }
    }

    /// Keeps `bytes` (the battery file at `path`) as `<path>.<suffix>`. Never over an existing
    /// copy: with `numbered`, the next free `.2`, `.3`, ... is used, unless one of the copies
    /// already holds these exact bytes; without it, an existing copy means nothing is written.
    fn keep_copy(path: &str, bytes: &[u8], suffix: &str, numbered: bool) -> Result<(), BridgeError> {
        let first = std::path::PathBuf::from(format!("{path}.{suffix}"));
        let mut target = first.clone();
        let mut n = 1;
        while target.exists() {
            if !numbered || std::fs::read(&target).map(|b| b == bytes).unwrap_or(false) {
                return Ok(());
            }
            n += 1;
            target = std::path::PathBuf::from(format!("{path}.{suffix}.{n}"));
        }
        std::fs::write(&target, bytes)
            .map_err(|err| BridgeError::Memory(format!("could not keep a copy at {}: {err}", target.display())))
    }

    /// `<game>.srm` → `<game>.rtc`, beside it.
    fn clock_path(battery_path: &str) -> std::path::PathBuf {
        std::path::Path::new(battery_path).with_extension("rtc")
    }

    /// Writes `data` to `target` atomically, returning its length, or `0` when the file already
    /// holds exactly these bytes.
    fn write_save_file(target: &std::path::Path, data: &[u8], scratch_ext: &str) -> Result<usize, BridgeError> {
        // Unchanged: nothing is written (0). The frontend now also flushes on a timer while a game
        // runs, so a crash does not lose an in-game save; rewriting an identical file each time
        // would only bump its mtime and make the sync upload it again.
        if std::fs::read(target).map(|old| old == data).unwrap_or(false) {
            return Ok(0);
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|err| {
                BridgeError::Memory(format!("could not create {}: {err}", parent.display()))
            })?;
        }
        let temporary = target.with_extension(scratch_ext);
        std::fs::write(&temporary, data)
            .and_then(|()| std::fs::rename(&temporary, target))
            .map_err(|err| {
                BridgeError::Memory(format!("could not write {}: {err}", target.display()))
            })?;
        Ok(data.len())
    }

    /// Reads a battery save from `path` into the running game. `Ok(None)` when there is no file,
    /// which is a game that has never saved rather than a failure.
    pub fn load_battery_save_file(&mut self, path: &str) -> Result<Option<usize>, BridgeError> {
        self.load_battery_save_file_migrating(path, None)
    }

    /// [`Self::load_battery_save_file`], and also picks up a battery file another core wrote for
    /// this game in `support_dir` (the folder the cores are given as their system and save
    /// folder): Mesen 2's `<game>.sav` for the NES, or the Sega CD's `scd_U/E/J.brm`.
    ///
    /// The newest such file of exactly the save RAM's size is loaded instead of `path` when
    /// `path` is missing, or when that file is newer and different. `path` is then first copied to
    /// `<path>.before-<core>` (never over an earlier copy), and the next persist replaces it. The
    /// other core's file is never written or deleted.
    ///
    /// For BlastEm on a Mega Drive cartridge, the first load of an existing `path` also keeps a
    /// copy as `<path>.before-blastem`.
    pub fn load_battery_save_file_migrating(
        &mut self,
        path: &str,
        support_dir: Option<&str>,
    ) -> Result<Option<usize>, BridgeError> {
        // First, before the file is even looked for, so the answer online does not depend on
        // whether this phone happens to have a save on disk.
        self.refuse_during_netplay("loading a battery save")?;
        let mut loaded = None;
        let size = self.memory_size(crate::memory::MEMORY_SAVE_RAM);
        if size > 0 {
            let (core_id, layout, candidates) = {
                let session = self.session.as_ref().ok_or(BridgeError::NoSession)?;
                let candidates = support_dir
                    .map(|dir| {
                        battery_layout::migration_candidates(
                            &session.core_id,
                            &session.content_ext,
                            &session.content_name,
                            std::path::Path::new(dir),
                        )
                    })
                    .unwrap_or_default();
                (session.core_id.clone(), session.battery_layout, candidates)
            };
            let own = match std::fs::read(path) {
                Ok(data) => Some(data),
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
                Err(err) => {
                    return Err(BridgeError::Memory(format!("could not read {path}: {err}")));
                }
            };
            let own_time = std::fs::metadata(path).and_then(|m| m.modified()).ok();
            // The newest other-core file of exactly this game's save RAM size.
            let other = candidates
                .iter()
                .filter_map(|p| {
                    let meta = std::fs::metadata(p).ok()?;
                    (meta.is_file() && meta.len() as usize == size).then(|| (p.clone(), meta.modified().ok()))
                })
                .max_by_key(|(_, time)| *time);
            let take_other = match (&other, &own) {
                (None, _) => None,
                (Some((p, _)), None) => Some(p.clone()),
                (Some((p, time)), Some(own_bytes)) => {
                    let newer = matches!((time, own_time), (Some(t), Some(o)) if *t > o);
                    let differs = std::fs::read(p).map(|b| &b != own_bytes).unwrap_or(false);
                    (newer && differs).then(|| p.clone())
                }
            };
            if let Some(other_path) = take_other {
                let data = std::fs::read(&other_path).map_err(|err| {
                    BridgeError::Memory(format!("could not read {}: {err}", other_path.display()))
                })?;
                if let Some(own_bytes) = &own {
                    Self::keep_copy(path, own_bytes, &format!("before-{core_id}"), true)?;
                }
                self.write_memory(crate::memory::MEMORY_SAVE_RAM, 0, &data)?;
                log::info!("battery save for this game taken from {}", other_path.display());
                loaded = Some(data.len());
            } else if let Some(data) = own {
                if matches!(layout, BatteryLayout::MegaDriveGpgx(_)) {
                    Self::keep_copy(path, &data, "before-blastem", false)?;
                }
                self.restore_battery_save(&data)?;
                loaded = Some(data.len());
            }
        }
        // The cartridge clock (see `persist_battery_save`). Before the first frame, which is
        // when a core reads it. A file of another size was written by a core that lays its clock
        // out differently: it is left alone, and this game's clock simply starts where the core
        // starts it.
        let clock_size = self.memory_size(crate::memory::MEMORY_RTC);
        if clock_size > 0 {
            let clock_path = Self::clock_path(path);
            match std::fs::read(&clock_path) {
                Ok(clock) if clock.len() == clock_size => {
                    self.write_memory(crate::memory::MEMORY_RTC, 0, &clock)?;
                    loaded = Some(loaded.unwrap_or(0) + clock.len());
                }
                Ok(_) => {}
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                Err(err) => {
                    return Err(BridgeError::Memory(format!(
                        "could not read {}: {err}",
                        clock_path.display()
                    )));
                }
            }
        }
        Ok(loaded)
    }

    // --------------------------------------------------------------- RAM search

    /// Starts a RAM search over `SYSTEM_RAM`, replacing any search in progress.
    /// Returns the number of candidates, which is every address.
    pub fn search_start(&mut self, width: SearchWidth, aligned: bool) -> Result<usize, BridgeError> {
        self.search_start_in("system", width, aligned)
    }

    /// The memory the RAM search can run over: system RAM when the core exposes it, then every
    /// distinct writable region of the core's memory map (the GBA's IWRAM and EWRAM, a SNES core's
    /// work RAM, the Sega CD's PRG RAM). Empty with no game running.
    pub fn search_regions(&self) -> Vec<SearchRegionInfo> {
        let Some(session) = self.session.as_ref() else {
            return Vec::new();
        };
        let mut regions = Vec::new();
        if let Some(ram) = session.core.memory_region(crate::memory::MEMORY_SYSTEM_RAM) {
            regions.push(SearchRegionInfo {
                key: "system".into(),
                name: "System RAM".into(),
                start: 0,
                size: ram.len() as u64,
            });
        }
        for region in crate::memory_maps::searchable(session.core.memory_map()) {
            regions.push(SearchRegionInfo {
                key: format!("map:{}", region.index),
                name: region.name,
                start: region.start as u64,
                size: region.len as u64,
            });
        }
        regions
    }

    /// Resolves a region key from [`Self::search_regions`].
    fn search_region_for(session: &Session, key: &str) -> Result<SearchRegion, BridgeError> {
        if key == "system" {
            return Ok(SearchRegion::SystemRam);
        }
        let index = key
            .strip_prefix("map:")
            .and_then(|n| n.parse::<usize>().ok())
            .ok_or_else(|| BridgeError::Memory(format!("'{key}' is not a memory region name")))?;
        crate::memory_maps::searchable(session.core.memory_map())
            .into_iter()
            .find(|r| r.index == index)
            .map(|r| SearchRegion::Mapped {
                index,
                start: r.start,
                len: r.len,
            })
            .ok_or_else(|| {
                BridgeError::Memory(format!(
                    "{} has no searchable memory region {index} (its memory map changed?)",
                    session.core.descriptor().display_name
                ))
            })
    }

    /// The bytes a search region covers right now.
    fn search_bytes(session: &Session, region: SearchRegion) -> Option<&[u8]> {
        match region {
            SearchRegion::SystemRam => session.core.memory_region(crate::memory::MEMORY_SYSTEM_RAM),
            SearchRegion::Mapped { index, start, len } => {
                let desc = session.core.memory_map().get(index)?;
                // Start AND length are re-checked against the current map. A core that republished
                // the descriptor shorter (same start, smaller buffer) would otherwise have `len`
                // bytes read past the end of its new buffer.
                let current = crate::memory_maps::searchable(session.core.memory_map())
                    .into_iter()
                    .find(|r| r.index == index)?;
                if desc.start != start || current.start != start || current.len != len {
                    return None;
                }
                // SAFETY: the descriptor is from the core's current map, the core is not running
                // (every caller holds the engine lock between frames), and libretro.h:4238 says
                // the buffer stays valid for the session. The slice is tied to `session`.
                unsafe { crate::memory_maps::descriptor_bytes(desc, len) }
            }
        }
    }

    /// The console address of offset 0 of a search region.
    fn search_base(region: SearchRegion) -> u32 {
        match region {
            SearchRegion::SystemRam => 0,
            SearchRegion::Mapped { start, .. } => start as u32,
        }
    }

    /// Starts a RAM search over one region from [`Self::search_regions`] ("system" or "map:N"),
    /// replacing any search in progress. Returns the number of candidates.
    pub fn search_start_in(
        &mut self,
        key: &str,
        width: SearchWidth,
        aligned: bool,
    ) -> Result<usize, BridgeError> {
        let session = self.session.as_mut().ok_or(BridgeError::NoSession)?;
        let region = Self::search_region_for(session, key)?;
        let search = {
            let ram = Self::search_bytes(session, region).ok_or_else(|| match region {
                SearchRegion::SystemRam => Self::no_region(session, crate::memory::MEMORY_SYSTEM_RAM),
                SearchRegion::Mapped { .. } => {
                    BridgeError::Memory(format!("memory region '{key}' could not be read"))
                }
            })?;
            RamSearch::start(ram, width, aligned).map_err(BridgeError::Memory)?
        };
        let count = search.count();
        session.search = Some(search);
        session.search_region = region;
        Ok(count)
    }

    /// The key of the region the running search covers, or `None` when no search is running.
    pub fn search_region_key(&self) -> Option<String> {
        let session = self.session.as_ref()?;
        session.search.as_ref()?;
        Some(match session.search_region {
            SearchRegion::SystemRam => "system".into(),
            SearchRegion::Mapped { index, .. } => format!("map:{index}"),
        })
    }

    /// Applies one filter to the search in progress. Returns how many candidates survive.
    pub fn search_filter(&mut self, filter: SearchFilter) -> Result<usize, BridgeError> {
        let session = self.session.as_mut().ok_or(BridgeError::NoSession)?;
        let region = session.search_region;
        let mut search = session
            .search
            .take()
            .ok_or_else(|| BridgeError::Memory("no RAM search is running; start one first".into()))?;
        let result = match Self::search_bytes(session, region) {
            Some(ram) => search.filter(ram, filter).map_err(BridgeError::Memory),
            None => Err(BridgeError::Memory(
                "the memory being searched went away during the search".into(),
            )),
        };
        session.search = Some(search);
        result
    }

    /// Candidates left, or `None` when no search is running.
    pub fn search_count(&self) -> Option<usize> {
        self.session.as_ref()?.search.as_ref().map(RamSearch::count)
    }

    /// The width the running search reads at.
    pub fn search_width(&self) -> Option<SearchWidth> {
        self.session.as_ref()?.search.as_ref().map(RamSearch::width)
    }

    /// Up to `limit` surviving addresses with their current and previous values. For a mapped
    /// region the addresses are the console's own (`$03001234` on a GBA), not offsets.
    pub fn search_results(&self, limit: usize) -> Vec<SearchHit> {
        let Some(session) = self.session.as_ref() else {
            return Vec::new();
        };
        let region = session.search_region;
        let (Some(search), Some(ram)) = (session.search.as_ref(), Self::search_bytes(session, region))
        else {
            return Vec::new();
        };
        let base = Self::search_base(region);
        search
            .results(ram, limit)
            .into_iter()
            .map(|hit| SearchHit {
                address: hit.address.wrapping_add(base),
                ..hit
            })
            .collect()
    }

    /// The cheat-list code that pins `value` at `address`, an address from
    /// [`Self::search_results`]: a plain poke for system RAM, a bus poke for a mapped region.
    pub fn search_poke_code(&self, address: u32, value: u32, bytes: u8) -> Result<String, BridgeError> {
        let region = self
            .session
            .as_ref()
            .filter(|s| s.search.is_some())
            .map_or(SearchRegion::SystemRam, |s| s.search_region);
        let poke = match region {
            SearchRegion::SystemRam => Poke::new(address, value, bytes),
            SearchRegion::Mapped { .. } => Poke::new_bus(address, value, bytes),
        };
        poke.map(|p| p.code()).map_err(BridgeError::Cheat)
    }

    /// Ends the search and frees its two snapshots.
    pub fn search_clear(&mut self) {
        if let Some(session) = self.session.as_mut() {
            session.search = None;
        }
    }

    /// Writes every enabled poke into the core's memory. Called after each `run_frame`, which is
    /// also where RetroArch runs `cheat_manager_apply_retro_cheats`.
    ///
    /// A poke that does not fit the region is skipped rather than failing the frame, for the
    /// reason on [`Poke::apply`].
    fn apply_pokes(session: &mut Session) {
        if session.pokes.is_empty() {
            return;
        }
        let Session { core, pokes, .. } = session;
        // A cheat-file poke's address is in RetroArch's cheat address space: the memory map's
        // SYSTEM_RAM-flagged buffers end to end when it has any, else SYSTEM_RAM itself (see
        // `memory_maps::cheat_buffers`). Decided again every frame from the current map, because
        // RetroArch rebuilds that space whenever a core publishes a new one.
        let cheats_through_map =
            pokes.iter().any(|p| p.cht) && crate::memory_maps::has_cheat_ram(core.memory_map());
        // Plain pokes, and cheat-file pokes whenever RetroArch would use SYSTEM_RAM for them too.
        let into_system_ram = |p: &&Poke| if p.cht { !cheats_through_map } else { !p.bus };
        if let Some(ram) = core.memory_region_mut(crate::memory::MEMORY_SYSTEM_RAM) {
            for poke in pokes.iter().filter(into_system_ram) {
                poke.apply(ram);
            }
        }
        // Bus pokes go through the memory map. A map that does not cover the address (a different
        // core, or a game without that memory) skips the poke, as an out-of-range plain one is.
        let map = core.memory_map();
        for poke in pokes.iter().filter(|p| p.bus) {
            // SAFETY: the map is the core's current one, the core is not running, and nothing else
            // holds a slice of its memory: `ram` above has gone out of scope.
            unsafe { crate::memory_maps::write_through(map, poke.address, &poke.value_bytes()) };
        }
        if cheats_through_map {
            for poke in pokes.iter().filter(|p| p.cht) {
                // SAFETY: as for the bus pokes above.
                unsafe { crate::memory_maps::write_cheat(map, poke.address, &poke.value_bytes()) };
            }
        }
    }

    // ------------------------------------------------------------- achievements

    /// One `rc_client_do_frame`, with its memory pointed at the core's regions as they are now.
    /// Refreshed every frame because a core may move a region (a disc swap, a mapper change), and
    /// three slice lookups cost nothing next to a frame.
    #[cfg(feature = "native-core")]
    fn achievements_frame(
        achievements: Option<&mut crate::achievements::Achievements>,
        session: &Session,
    ) {
        let Some(achievements) = achievements else {
            return;
        };
        if !achievements.is_game_loaded() {
            return;
        }
        let core = &session.core;
        achievements.set_memory(core.memory_map(), |id| {
            core.memory_region(id).map(|r| (r.as_ptr(), r.len()))
        });
        achievements.do_frame();
    }

    /// Tells rcheevos the machine jumped (reset, state load). A no-op without a loaded set.
    fn achievements_reset(&mut self) {
        #[cfg(feature = "native-core")]
        if let Some(achievements) = self.achievements.as_mut() {
            if achievements.is_game_loaded() {
                achievements.reset();
            }
        }
    }

    /// The achievements client, created on first use.
    #[cfg(feature = "native-core")]
    pub fn achievements_mut(
        &mut self,
    ) -> Result<&mut crate::achievements::Achievements, BridgeError> {
        if self.achievements.is_none() {
            let created = crate::achievements::Achievements::new().map_err(BridgeError::Achievements)?;
            self.achievements = Some(created);
        }
        Ok(self
            .achievements
            .as_mut()
            .expect("created immediately above"))
    }

    /// The achievements client if one exists, without creating it.
    #[cfg(feature = "native-core")]
    pub fn achievements(&self) -> Option<&crate::achievements::Achievements> {
        self.achievements.as_ref()
    }

    /// Identifies the running game and loads its set. Needs a session (so the memory being read
    /// is the game's) and a login (rcheevos refuses otherwise, and says so through an event).
    #[cfg(feature = "native-core")]
    pub fn achievements_load_game(&mut self, system: &str, path: &str) -> Result<(), BridgeError> {
        if self.session.is_none() {
            return Err(BridgeError::NoSession);
        }
        let achievements = self.achievements_mut()?;
        if achievements.is_game_loaded() {
            achievements.unload_game();
        }
        achievements
            .load_game(system, path, &[])
            .map_err(BridgeError::Achievements)
    }

    /// Starts a session with no renderer, for tests that need a running core.
    #[cfg(test)]
    pub(crate) fn launch_headless_for_test(
        &mut self,
        core_id: &str,
        content: &[u8],
    ) -> Result<(), BridgeError> {
        self.launch_headless_named_for_test(core_id, content, "test.rom")
    }

    /// [`Self::launch_headless_for_test`] with the game's file name, which picks the battery
    /// layout and the other cores' save files.
    #[cfg(test)]
    pub(crate) fn launch_headless_named_for_test(
        &mut self,
        core_id: &str,
        content: &[u8],
        file_name: &str,
    ) -> Result<(), BridgeError> {
        let hint = ContentHint::from_filename(file_name);
        let mut core = self.registry.take_for_session(core_id)?;
        core.load_content(content, &hint)?;
        self.session = Some(Session {
            core_id: core_id.to_string(),
            content_id: file_name.into(),
            core,
            paused: false,
            cheats: Vec::new(),
            enabled_codes: 0,
            pokes: Vec::new(),
            search: None,
            search_region: SearchRegion::SystemRam,
            content_name: file_name.into(),
            content_ext: hint.extension.to_ascii_lowercase(),
            battery_layout: Self::battery_layout_for(core_id, content, &hint),
        });
        Ok(())
    }

    /// Runs `frames` core frames with no renderer, the way `tick` does (frame, then pokes).
    #[cfg(test)]
    pub(crate) fn step_headless_for_test(&mut self, frames: u32) -> Result<(), BridgeError> {
        let mut snapshot = self.gamepads.snapshot();
        let session = self.session.as_mut().ok_or(BridgeError::NoSession)?;
        crate::input::core_layout::translate_for(&session.core_id, &mut snapshot);
        for _ in 0..frames {
            session.core.run_frame(&snapshot)?;
            Self::apply_pokes(session);
        }
        Ok(())
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
        // ONLY THE CHEATS THAT ARE ON, numbered from 0 without gaps, each sent as enabled. That is
        // exactly what RetroArch does (`cheat_manager_apply_cheats`), and cores are written
        // against it: mGBA's `retro_cheat_set` ignores both the index and the enabled flag and
        // adds every code it is given, so sending a switched-off cheat with `false` left it
        // running on the GBA and the Game Boy, and the switch did nothing. The list the user sees
        // is kept whole in `session.cheats`; only what the core is told changed.
        //
        // Collected first so the loop does not hold a borrow of `session.cheats` while calling
        // `&mut` methods on `session.core`.
        let list: Vec<String> = session
            .cheats
            .iter()
            .filter(|cheat| cheat.enabled)
            .map(|cheat| cheat.code.clone())
            .collect();
        for (index, code) in list.iter().enumerate() {
            session.core.set_cheat(index as u32, true, code)?;
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
        bridge.declare_core(descriptor("bsnes", "snes"));
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

    fn running_diagnostic() -> EmulatorBridge {
        let mut bridge = EmulatorBridge::new();
        bridge.declare_core(descriptor("diag", "test"));
        bridge.attach_core_module("diag", MODULE).unwrap();
        bridge.launch_headless_for_test("diag", b"rom").unwrap();
        bridge
    }

    #[test]
    fn memory_access_needs_a_session_and_a_region() {
        use crate::memory::*;
        let mut bridge = EmulatorBridge::new();
        assert!(matches!(
            bridge.read_memory(MEMORY_SYSTEM_RAM, 0, 1),
            Err(BridgeError::NoSession)
        ));
        assert_eq!(bridge.memory_size(MEMORY_SYSTEM_RAM), 0);
        bridge = running_diagnostic();
        assert_eq!(bridge.memory_size(MEMORY_SYSTEM_RAM), 2048);
        assert_eq!(bridge.memory_size(MEMORY_VIDEO_RAM), 0);
        let err = bridge.read_memory(MEMORY_VIDEO_RAM, 0, 1).unwrap_err();
        assert!(err.to_string().contains("video RAM"), "{err}");
    }

    #[test]
    fn read_and_write_memory_round_trip_and_bounds_check() {
        use crate::memory::*;
        let mut bridge = running_diagnostic();
        bridge.write_memory(MEMORY_SYSTEM_RAM, 0x100, &[1, 2, 3]).unwrap();
        assert_eq!(bridge.read_memory(MEMORY_SYSTEM_RAM, 0x100, 3).unwrap(), vec![1, 2, 3]);
        assert!(bridge.write_memory(MEMORY_SYSTEM_RAM, 2047, &[1, 2]).is_err());
        assert!(bridge.read_memory(MEMORY_SYSTEM_RAM, 2040, 9).is_err());
        // A refused write changed nothing.
        assert_eq!(bridge.read_memory(MEMORY_SYSTEM_RAM, 2047, 1).unwrap(), vec![0]);
    }

    #[test]
    fn battery_save_with_an_rtc_footer_loads_the_ram_part() {
        // A Game Boy cartridge with a clock, as mGBA saved it: the RAM and 48 bytes of clock.
        let mut bridge = running_as("cboy", "gbc", b"rom", "Pokemon Gold.gbc");
        let size = bridge.memory_size(crate::memory::MEMORY_SAVE_RAM);
        let mut data = vec![0x5A; size];
        data.extend(vec![1u8; 48]);
        assert_eq!(bridge.restore_battery_save(&data).unwrap(), size);
        assert_eq!(bridge.battery_save().unwrap(), vec![0x5A; size]);
        data.extend(vec![1u8; 7]);
        assert!(bridge.restore_battery_save(&data).is_err());
        // No other system writes its clock there, so a file 48 bytes too long belongs to another
        // game and is refused instead of being cut down to fit.
        let mut nes = running_as("fceumm", "nes", b"rom", "Zelda.nes");
        let mut other_game = vec![0x5A; size];
        other_game.extend(vec![1u8; 48]);
        assert!(nes.restore_battery_save(&other_game).is_err());
    }

    #[test]
    fn an_imported_clock_footer_is_kept_through_a_save() {
        let dir = scratch_dir("gb-clock-footer");
        let path = dir.join("Pokemon Gold.gbc.srm");
        let path = path.to_str().unwrap().to_string();
        let mut bridge = running_as("cboy", "gbc", b"rom", "Pokemon Gold.gbc");
        let size = bridge.memory_size(crate::memory::MEMORY_SAVE_RAM);
        let mut file = vec![0x5A; size];
        file.extend(vec![0xC3; 48]);
        std::fs::write(&path, &file).unwrap();
        assert_eq!(bridge.load_battery_save_file(&path).unwrap(), Some(size + 48));
        // The periodic flush, with nothing saved in the game: the file is left exactly as it was,
        // clock and all, instead of being rewritten without it.
        assert_eq!(bridge.persist_battery_save(&path).unwrap(), 0);
        assert_eq!(std::fs::read(&path).unwrap(), file);
        // An in-game save writes the RAM part and keeps the clock after it.
        bridge
            .write_memory(crate::memory::MEMORY_SAVE_RAM, 0, &[0xA5, 0xA5])
            .unwrap();
        assert_eq!(bridge.persist_battery_save(&path).unwrap(), size + 48);
        let on_disk = std::fs::read(&path).unwrap();
        assert_eq!(on_disk.len(), size + 48);
        assert_eq!(&on_disk[..2], &[0xA5, 0xA5]);
        assert_eq!(&on_disk[size..], &[0xC3; 48]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn battery_save_round_trips_through_a_file() {
        let dir = std::env::temp_dir().join(format!("continuum-srm-{}", std::process::id()));
        let path = dir.join("game.srm");
        let path = path.to_str().unwrap().to_string();
        let mut bridge = running_diagnostic();
        assert_eq!(bridge.load_battery_save_file(&path).unwrap(), None);
        bridge
            .write_memory(crate::memory::MEMORY_SAVE_RAM, 0, &[0x5A, 0xA5])
            .unwrap();
        assert_eq!(bridge.persist_battery_save(&path).unwrap(), 8192);
        // Unchanged since the last write: nothing is rewritten.
        assert_eq!(bridge.persist_battery_save(&path).unwrap(), 0);
        let mut fresh = running_diagnostic();
        assert_eq!(fresh.battery_save().unwrap()[0], 0);
        assert_eq!(fresh.load_battery_save_file(&path).unwrap(), Some(8192));
        assert_eq!(&fresh.battery_save().unwrap()[..2], &[0x5A, 0xA5]);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// A running diagnostic core declared under `id`, playing `file_name` with `content`.
    fn running_as(id: &str, system: &str, content: &[u8], file_name: &str) -> EmulatorBridge {
        let mut bridge = EmulatorBridge::new();
        bridge.declare_core(descriptor(id, system));
        bridge.attach_core_module(id, MODULE).unwrap();
        bridge.launch_headless_named_for_test(id, content, file_name).unwrap();
        bridge
    }

    fn scratch_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("continuum-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn set_age(path: &std::path::Path, seconds_ago: u64) {
        let when = std::time::SystemTime::now() - std::time::Duration::from_secs(seconds_ago);
        std::fs::File::options().write(true).open(path).unwrap().set_modified(when).unwrap();
    }

    #[test]
    fn a_mega_drive_save_stays_in_gpgx_layout_on_blastem() {
        // An odd-byte SRAM cart, $200001-$203FFF: 8192 bytes live, the diagnostic SAVE_RAM size.
        let mut rom = vec![0u8; 0x200];
        rom[0x100..0x10F].copy_from_slice(b"SEGA MEGA DRIVE");
        rom[0x1B0..0x1BC].copy_from_slice(&[b'R', b'A', 0xF8, 0x20, 0x00, 0x20, 0x00, 0x01, 0x00, 0x20, 0x3F, 0xFF]);
        let dir = scratch_dir("md-gpgx");
        let path = dir.join("Sonic.srm");
        let path_str = path.to_str().unwrap().to_string();
        // What GPGX wrote: 0x22+i at the odd addresses, trimmed.
        let mut gpgx = vec![0xFF; 0x20];
        for i in 0..16 {
            gpgx[1 + 2 * i] = 0x22 + i as u8;
        }
        std::fs::write(&path, &gpgx).unwrap();

        let mut bridge = running_as("blastem", "genesis", &rom, "Sonic.md");
        assert_eq!(bridge.load_battery_save_file_migrating(&path_str, None).unwrap(), Some(0x20));
        let live = bridge.read_memory(crate::memory::MEMORY_SAVE_RAM, 0, 8192).unwrap();
        assert_eq!(&live[..4], &[0x22, 0x23, 0x24, 0x25]);
        assert_eq!(live[16], 0xFF);
        // A copy of the pre-swap file, once.
        let before = dir.join("Sonic.srm.before-blastem");
        assert_eq!(std::fs::read(&before).unwrap(), gpgx);
        // Out again in GPGX's layout: the whole $0000-$3FFF window, filler at the even bytes.
        assert_eq!(bridge.persist_battery_save(&path_str).unwrap(), 0x4000);
        let on_disk = std::fs::read(&path).unwrap();
        assert_eq!(on_disk.len(), 0x4000);
        assert_eq!(&on_disk[..0x20], &gpgx[..]);
        assert!(on_disk[0x20..].iter().all(|b| *b == 0xFF));
        // Reloading gives the same live bytes, and the first copy is never replaced.
        std::fs::write(&before, b"first").unwrap();
        let mut fresh = running_as("blastem", "genesis", &rom, "Sonic.md");
        fresh.load_battery_save_file_migrating(&path_str, None).unwrap();
        assert_eq!(fresh.read_memory(crate::memory::MEMORY_SAVE_RAM, 0, 8192).unwrap(), live);
        assert_eq!(std::fs::read(&before).unwrap(), b"first");
        // The same file on another core is loaded byte for byte (and refused, being too long).
        let mut other = running_as("gearsystem", "genesis", &rom, "Sonic.md");
        assert!(other.restore_battery_save(&vec![1; 0x4000]).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_headerless_sram_save_carries_to_blastem() {
        // Buck Rogers (T-50286): no RA header, 8 KB of odd-byte SRAM in both cores.
        let mut rom = vec![0x20u8; 0x200];
        rom[0x100..0x110].copy_from_slice(b"SEGA GENESIS    ");
        rom[0x180..0x18E].copy_from_slice(b"GM T-50286 -00");
        let dir = scratch_dir("md-headerless");
        let path = dir.join("Buck Rogers.srm");
        let path_str = path.to_str().unwrap().to_string();
        // GPGX's file, reaching past the first 4 KB of SRAM (the case that used to be refused).
        let mut gpgx = vec![0xFF; 0x3002];
        gpgx[1] = 0x41;
        gpgx[0x3001] = 0x7A;
        std::fs::write(&path, &gpgx).unwrap();
        let mut bridge = running_as("blastem", "genesis", &rom, "Buck Rogers.md");
        assert_eq!(bridge.load_battery_save_file_migrating(&path_str, None).unwrap(), Some(0x3002));
        let live = bridge.read_memory(crate::memory::MEMORY_SAVE_RAM, 0, 8192).unwrap();
        assert_eq!((live[0], live[1], live[0x1800]), (0x41, 0xFF, 0x7A));
        assert_eq!(std::fs::read(dir.join("Buck Rogers.srm.before-blastem")).unwrap(), gpgx);
        // Written back in GPGX's layout, the same bytes where they were.
        assert_eq!(bridge.persist_battery_save(&path_str).unwrap(), 0x4000);
        let on_disk = std::fs::read(&path).unwrap();
        assert_eq!(&on_disk[..gpgx.len()], &gpgx[..]);
        assert!(on_disk[gpgx.len()..].iter().all(|b| *b == 0xFF));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn sonic_3_and_knuckles_is_read_whole_from_its_file() {
        let mut rom = vec![0x20u8; 0x40_0000];
        rom[0x100..0x110].copy_from_slice(b"SEGA GENESIS    ");
        rom[0x150..0x160].copy_from_slice(b"SONIC & KNUCKLES");
        rom[0x200100..0x200110].copy_from_slice(b"SEGA GENESIS    ");
        rom[0x2001B0..0x2001BC].copy_from_slice(&[b'R', b'A', 0xF8, 0x20, 0x00, 0x20, 0x00, 0x01, 0x00, 0x20, 0x03, 0xFF]);
        let dir = scratch_dir("md-lockon");
        let file = dir.join("Sonic 3 & Knuckles.md");
        std::fs::write(&file, &rom).unwrap();
        let mut hint = ContentHint::from_filename("Sonic 3 & Knuckles.md");
        hint.full_path = Some(file.to_str().unwrap().to_string());
        let s3 = battery_layout::MdSram { lane: battery_layout::SramLane::Odd, start: 0x200000, end: 0x2003FF };
        // BlastEm is handed the path, not the bytes.
        assert_eq!(EmulatorBridge::battery_layout_for("blastem", &[], &hint), BatteryLayout::MegaDriveGpgx(s3));
        assert_eq!(EmulatorBridge::battery_layout_for("blastem", &rom, &hint), BatteryLayout::MegaDriveGpgx(s3));
        // Another core keeps its bytes; a bigger file is not the lock-on ROM and is not read whole.
        assert_eq!(EmulatorBridge::battery_layout_for("gearsystem", &[], &hint), BatteryLayout::Raw);
        rom.resize(0x40_0400, 0);
        std::fs::write(&file, &rom).unwrap();
        assert!(matches!(
            EmulatorBridge::battery_layout_for("blastem", &[], &hint),
            BatteryLayout::MegaDriveGpgx(battery_layout::MdSram { lane: battery_layout::SramLane::Linear, .. })
        ));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn nes_saves_mesen_wrote_are_picked_up_and_the_old_one_kept() {
        let dir = scratch_dir("nes-migrate");
        let support = dir.join("support");
        std::fs::create_dir_all(&support).unwrap();
        let support_str = support.to_str().unwrap().to_string();
        let srm = dir.join("Zelda.srm");
        let srm_str = srm.to_str().unwrap().to_string();
        let sav = support.join("Zelda.sav");
        let first_byte = |b: &EmulatorBridge| b.read_memory(crate::memory::MEMORY_SAVE_RAM, 0, 1).unwrap()[0];

        // Only Mesen's .sav: it is loaded.
        std::fs::write(&sav, vec![0x11; 8192]).unwrap();
        let mut bridge = running_as("mesen2", "nes", b"rom", "Zelda.nes");
        assert_eq!(bridge.load_battery_save_file_migrating(&srm_str, Some(&support_str)).unwrap(), Some(8192));
        assert_eq!(first_byte(&bridge), 0x11);

        // The app's .srm is newer: it wins, and nothing is copied.
        std::fs::write(&srm, vec![0x22; 8192]).unwrap();
        set_age(&sav, 600);
        let mut bridge = running_as("mesen2", "nes", b"rom", "Zelda.nes");
        bridge.load_battery_save_file_migrating(&srm_str, Some(&support_str)).unwrap();
        assert_eq!(first_byte(&bridge), 0x22);
        assert!(!dir.join("Zelda.srm.before-mesen2").exists());

        // The .sav is newer and different: it wins, and the .srm is kept first.
        set_age(&srm, 1200);
        let mut bridge = running_as("mesen2", "nes", b"rom", "Zelda.nes");
        bridge.load_battery_save_file_migrating(&srm_str, Some(&support_str)).unwrap();
        assert_eq!(first_byte(&bridge), 0x11);
        let kept = dir.join("Zelda.srm.before-mesen2");
        assert_eq!(std::fs::read(&kept).unwrap(), vec![0x22; 8192]);
        // A second call keeps that copy as it is and makes no second one of the same bytes.
        let mut again = running_as("mesen2", "nes", b"rom", "Zelda.nes");
        again.load_battery_save_file_migrating(&srm_str, Some(&support_str)).unwrap();
        assert_eq!(std::fs::read(&kept).unwrap(), vec![0x22; 8192]);
        assert!(!dir.join("Zelda.srm.before-mesen2.2").exists());
        // Mesen's file itself is never touched.
        assert_eq!(std::fs::read(&sav).unwrap(), vec![0x11; 8192]);

        // A .sav of another size is not this game's save RAM: ignored.
        std::fs::write(&sav, vec![0x33; 100]).unwrap();
        let mut bridge = running_as("mesen2", "nes", b"rom", "Zelda.nes");
        bridge.load_battery_save_file_migrating(&srm_str, Some(&support_str)).unwrap();
        assert_eq!(first_byte(&bridge), 0x22);

        // Without a support folder, nothing but the app's own file is looked at.
        std::fs::write(&sav, vec![0x44; 8192]).unwrap();
        let mut bridge = running_as("mesen2", "nes", b"rom", "Zelda.nes");
        bridge.load_battery_save_file(&srm_str).unwrap();
        assert_eq!(first_byte(&bridge), 0x22);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_sega_cd_game_starts_from_the_newest_backup_ram() {
        let dir = scratch_dir("scd-migrate");
        let support = dir.join("support");
        std::fs::create_dir_all(&support).unwrap();
        let (us, eu, jp) = (support.join("scd_U.brm"), support.join("scd_E.brm"), support.join("scd_J.brm"));
        std::fs::write(&us, vec![0x00u8; 8192]).unwrap();
        std::fs::write(&eu, vec![0xE0; 8192]).unwrap();
        std::fs::write(&jp, vec![0x70; 4096]).unwrap();
        set_age(&us, 900);
        set_age(&eu, 60);
        let srm = dir.join("Sonic CD.srm");
        let mut bridge = running_as("blastem", "segacd", b"cue", "Sonic CD.cue");
        let got = bridge
            .load_battery_save_file_migrating(srm.to_str().unwrap(), Some(support.to_str().unwrap()))
            .unwrap();
        assert_eq!(got, Some(8192));
        assert_eq!(bridge.read_memory(crate::memory::MEMORY_SAVE_RAM, 0, 8192).unwrap(), vec![0xE0; 8192]);
        // The system files are left exactly as they were.
        assert_eq!(std::fs::read(&eu).unwrap(), vec![0xE0; 8192]);
        assert_eq!(std::fs::read(&jp).unwrap(), vec![0x70; 4096]);
        assert!(!srm.exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn padding_past_the_save_ram_is_trimmed_before_the_size_rule() {
        let mut bridge = running_diagnostic();
        let mut data = vec![0x5A; 8192];
        data.extend_from_slice(&[0xFF; 8192]);
        assert_eq!(bridge.restore_battery_save(&data).unwrap(), 8192);
        data[9000] = 0;
        assert!(bridge.restore_battery_save(&data).is_err());
        let mut zeros = vec![0x5A; 8192];
        zeros.extend_from_slice(&[0x00; 100]);
        assert!(bridge.restore_battery_save(&zeros).is_err());
        let mut bsnes = running_as("bsnes", "snes", b"rom", "Mario.sfc");
        assert_eq!(bsnes.restore_battery_save(&zeros).unwrap(), 8192);
    }

    #[test]
    fn the_cartridge_clock_is_kept_next_to_the_battery_save() {
        use crate::memory::MEMORY_RTC;
        let dir = std::env::temp_dir().join(format!("continuum-rtc-{}", std::process::id()));
        let path = dir.join("Gold.gbc.srm");
        let path = path.to_str().unwrap().to_string();
        let clock_file = dir.join("Gold.gbc.rtc");
        let with_clock = || {
            let mut bridge = EmulatorBridge::new();
            bridge.declare_core(descriptor("diag", "test"));
            bridge.attach_core_module("diag", MODULE).unwrap();
            bridge.launch_headless_for_test("diag", b"rtc rom").unwrap();
            bridge
        };
        let mut bridge = with_clock();
        let clock: Vec<u8> = (1..=24).collect();
        bridge.write_memory(MEMORY_RTC, 0, &clock).unwrap();
        assert_eq!(bridge.persist_battery_save(&path).unwrap(), 8192 + 24);
        assert_eq!(std::fs::read(&clock_file).unwrap(), clock);
        // Neither changed: nothing is rewritten.
        assert_eq!(bridge.persist_battery_save(&path).unwrap(), 0);
        // A fresh session gets the clock back before its first frame.
        let mut fresh = with_clock();
        assert_eq!(fresh.load_battery_save_file(&path).unwrap(), Some(8192 + 24));
        assert_eq!(fresh.read_memory(MEMORY_RTC, 0, 24).unwrap(), clock);
        // A clock file of another size (another core's layout) is left alone, not an error.
        std::fs::write(&clock_file, [9u8; 48]).unwrap();
        let mut other = with_clock();
        assert_eq!(other.load_battery_save_file(&path).unwrap(), Some(8192));
        assert_eq!(other.read_memory(MEMORY_RTC, 0, 24).unwrap(), vec![0; 24]);
        // A clock with no battery save yet is still restored.
        std::fs::remove_file(&path).unwrap();
        std::fs::write(&clock_file, &clock).unwrap();
        let mut only_clock = with_clock();
        assert_eq!(only_clock.load_battery_save_file(&path).unwrap(), Some(24));
        // A core with no clock writes no clock file.
        let plain_path = dir.join("Red.gb.srm");
        running_diagnostic().persist_battery_save(plain_path.to_str().unwrap()).unwrap();
        assert!(!dir.join("Red.gb.rtc").exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn battery_restore_accepts_shorter_and_refuses_longer() {
        let mut bridge = running_diagnostic();
        assert_eq!(bridge.restore_battery_save(&[7; 100]).unwrap(), 8192);
        assert_eq!(bridge.battery_save().unwrap()[99], 7);
        assert!(bridge.restore_battery_save(&vec![1; 8193]).is_err());
        assert!(bridge.restore_battery_save(&[]).is_err());
    }

    #[test]
    fn ram_search_finds_the_health_byte_through_the_bridge() {
        use crate::cores::DiagnosticCore;
        let mut bridge = running_diagnostic();
        bridge.step_headless_for_test(1).unwrap();
        assert_eq!(bridge.search_start(SearchWidth::Bits8, false).unwrap(), 2048);
        bridge.step_headless_for_test(1).unwrap();
        bridge.search_filter(SearchFilter::DecreasedBy(1)).unwrap();
        bridge.step_headless_for_test(3).unwrap();
        let left = bridge.search_filter(SearchFilter::LessThanPrevious).unwrap();
        // Health drops one a frame; the frame counter's low byte rises. Only health is left.
        assert_eq!(left, 1);
        let hits = bridge.search_results(10);
        assert_eq!(hits[0].address as usize, DiagnosticCore::HEALTH_OFFSET);
        assert_eq!(hits[0].current, 95);
        bridge.search_clear();
        assert_eq!(bridge.search_count(), None);
        assert!(bridge.search_filter(SearchFilter::Changed).is_err());
    }

    #[test]
    fn a_poke_pins_the_value_every_frame_and_is_not_sent_to_the_core() {
        use crate::cores::DiagnosticCore;
        use crate::memory::MEMORY_SYSTEM_RAM;
        let mut bridge = running_diagnostic();
        let poke = Poke::new(DiagnosticCore::HEALTH_OFFSET as u32, 99, 1).unwrap();
        // The diagnostic core does not take core cheats, so a list of only pokes must still be
        // refused there: that is the existing contract for cores without cheat support.
        assert!(bridge.apply_cheats(vec![poke.code()], &[1]).is_err());
        let health = DiagnosticCore::HEALTH_OFFSET as u64;
        bridge.session.as_mut().unwrap().pokes = vec![poke];
        bridge.step_headless_for_test(10).unwrap();
        assert_eq!(bridge.read_memory(MEMORY_SYSTEM_RAM, health, 1).unwrap(), vec![99]);
        assert_eq!(bridge.active_cheat_count(), 1);
        bridge.session.as_mut().unwrap().pokes.clear();
        bridge.step_headless_for_test(1).unwrap();
        assert_eq!(bridge.read_memory(MEMORY_SYSTEM_RAM, health, 1).unwrap(), vec![98]);
    }

    /// A core that takes cheats and records every call, so the table the engine builds is visible.
    struct CheatRecorder {
        descriptor: CoreDescriptor,
        calls: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    }

    impl EmulatorCore for CheatRecorder {
        fn descriptor(&self) -> &CoreDescriptor {
            &self.descriptor
        }
        fn load_content(&mut self, _: &[u8], _: &ContentHint) -> Result<(), BridgeError> {
            Ok(())
        }
        fn run_frame(&mut self, _: &crate::input::InputSnapshot) -> Result<(), BridgeError> {
            Ok(())
        }
        fn video(&self) -> Option<crate::frame::FrameView<'_>> {
            None
        }
        fn drain_audio(&mut self, _: &mut dyn AudioSink) {}
        fn reset(&mut self) -> Result<(), BridgeError> {
            Ok(())
        }
        fn reset_cheats(&mut self) -> Result<(), BridgeError> {
            self.calls.lock().unwrap().push("reset".into());
            Ok(())
        }
        fn set_cheat(&mut self, index: u32, enabled: bool, code: &str) -> Result<(), BridgeError> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("{index}:{enabled}:{code}"));
            Ok(())
        }
        fn supports_cheats(&self) -> bool {
            true
        }
        fn frame_count(&self) -> u64 {
            0
        }
    }

    /// A core with mGBA's GBA memory map and nothing else: IWRAM, EWRAM, save and the rest, with
    /// `SYSTEM_RAM` being EWRAM as mGBA's is. Each frame bumps IWRAM byte $10 (a "timer").
    struct MappedGba {
        descriptor: CoreDescriptor,
        buffers: crate::memory_maps::fixtures::Buffers,
        map: Vec<crate::memory_maps::MemoryDescriptor>,
    }

    impl MappedGba {
        fn new() -> Self {
            let (buffers, raw) = crate::memory_maps::fixtures::mgba_gba(0x80_0000, 0x8000);
            let map = crate::memory_maps::fixtures::copy(&raw);
            Self {
                descriptor: descriptor("gbamap", "test"),
                buffers,
                map,
            }
        }
    }

    impl EmulatorCore for MappedGba {
        fn descriptor(&self) -> &CoreDescriptor {
            &self.descriptor
        }
        fn load_content(&mut self, _: &[u8], _: &ContentHint) -> Result<(), BridgeError> {
            Ok(())
        }
        fn run_frame(&mut self, _: &crate::input::InputSnapshot) -> Result<(), BridgeError> {
            let timer = &mut self.buffers.blocks[0][0x10];
            *timer = timer.wrapping_add(1);
            Ok(())
        }
        fn video(&self) -> Option<crate::frame::FrameView<'_>> {
            None
        }
        fn drain_audio(&mut self, _: &mut dyn AudioSink) {}
        fn reset(&mut self) -> Result<(), BridgeError> {
            Ok(())
        }
        fn supports_cheats(&self) -> bool {
            true
        }
        fn reset_cheats(&mut self) -> Result<(), BridgeError> {
            Ok(())
        }
        fn set_cheat(&mut self, _: u32, _: bool, _: &str) -> Result<(), BridgeError> {
            Ok(())
        }
        fn memory_region(&self, id: u32) -> Option<&[u8]> {
            (id == crate::memory::MEMORY_SYSTEM_RAM).then(|| &self.buffers.blocks[1][..])
        }
        fn memory_region_mut(&mut self, id: u32) -> Option<&mut [u8]> {
            (id == crate::memory::MEMORY_SYSTEM_RAM).then(|| &mut self.buffers.blocks[1][..])
        }
        fn memory_map(&self) -> &[crate::memory_maps::MemoryDescriptor] {
            &self.map
        }
        fn frame_count(&self) -> u64 {
            0
        }
    }

    fn running_mapped_gba() -> EmulatorBridge {
        let mut bridge = EmulatorBridge::new();
        bridge.declare_core(descriptor("gbamap", "test"));
        bridge.attach_core("gbamap", Box::new(MappedGba::new())).unwrap();
        bridge.launch_headless_for_test("gbamap", b"rom").unwrap();
        bridge
    }

    /// A core that changes its own refresh rate on its third frame, which is what FCEUmm does when
    /// the NES region option moves from NTSC to PAL: it re-reads its av info, publishes it with
    /// `SET_SYSTEM_AV_INFO`, and `sync_descriptor_from_core` writes the new rate into the
    /// descriptor. That is the step this stands in for.
    struct RegionSwitcher {
        descriptor: CoreDescriptor,
        frames: u64,
    }

    /// 50.0070 and 60.0988: `NesConsole::GetFps` in Mesen 2, `NES_PAL_FPS`/`NES_NTSC_FPS` in FCEUmm.
    const NES_PAL_FPS: f64 = 50.006_978_908_188_6;
    const NES_NTSC_FPS: f64 = 60.098_811_862_348_4;

    impl EmulatorCore for RegionSwitcher {
        fn descriptor(&self) -> &CoreDescriptor {
            &self.descriptor
        }
        fn load_content(&mut self, _: &[u8], _: &ContentHint) -> Result<(), BridgeError> {
            Ok(())
        }
        fn run_frame(&mut self, _: &crate::input::InputSnapshot) -> Result<(), BridgeError> {
            self.frames += 1;
            if self.frames == 3 {
                self.descriptor.target_fps = NES_PAL_FPS;
            }
            Ok(())
        }
        fn video(&self) -> Option<crate::frame::FrameView<'_>> {
            None
        }
        fn drain_audio(&mut self, _: &mut dyn AudioSink) {}
        fn reset(&mut self) -> Result<(), BridgeError> {
            Ok(())
        }
        fn frame_count(&self) -> u64 {
            self.frames
        }
    }

    #[test]
    fn an_nes_game_says_which_region_it_is_running_as() {
        let bridge = running_as("mesen2", "nes", b"rom", "Smurfs, The (E).nes");
        // The diagnostic descriptor runs at 60, so this copy is reported as a 60 Hz one. What the
        // file name says is not consulted: the rate the core reports is the only authority.
        let line = bridge.session_region_note();
        assert!(line.contains("60 frames a second"), "{line}");
        assert!(line.contains("American or Japanese"), "{line}");
        // Nothing is said for a system where there is only one speed, or with no game running.
        assert!(running_as("mgba", "gb", b"rom", "Tetris.gb").session_region_note().is_empty());
        assert!(EmulatorBridge::new().session_region_note().is_empty());
    }

    #[test]
    fn the_pacer_follows_a_core_that_changes_its_refresh_rate_mid_game() {
        let mut bridge = EmulatorBridge::new();
        let mut spec = descriptor("fceumm", "nes");
        spec.target_fps = NES_NTSC_FPS;
        bridge.declare_core(spec.clone());
        bridge
            .attach_core("fceumm", Box::new(RegionSwitcher { descriptor: spec, frames: 0 }))
            .unwrap();
        bridge.launch_headless_named_for_test("fceumm", b"rom", "Smurfs, The.nes").unwrap();
        bridge.pacer.set_target_fps(NES_NTSC_FPS);
        assert!(bridge.session_region_note().contains("60 frames a second"));

        // Five ticks at 60 Hz, which is a step each: the switch happens on the third.
        let mut now = 0.0;
        for _ in 0..5 {
            bridge.tick(now).unwrap();
            now += 1000.0 / 60.0;
        }

        // The game is paced at the rate the core now reports, and the plain line follows it. Before
        // this, both stayed at 60 for the rest of the session.
        assert!((bridge.pacer.target_fps() - NES_PAL_FPS).abs() < 0.001);
        let line = bridge.session_region_note();
        assert!(line.contains("50 frames a second"), "{line}");
        assert!(line.contains("European"), "{line}");
    }

    /// A core that puts a border around its picture once the game is loaded, which is what cBoy
    /// does with a Super Game Boy cartridge: 160x144 (10:9) becomes 256x224 (8:7). The app sizes
    /// the picture from the shape the core reports, so this is the shape it has to follow.
    struct BorderSwitcher {
        descriptor: CoreDescriptor,
        frames: u64,
    }

    impl EmulatorCore for BorderSwitcher {
        fn descriptor(&self) -> &CoreDescriptor {
            &self.descriptor
        }
        fn load_content(&mut self, _: &[u8], _: &ContentHint) -> Result<(), BridgeError> {
            self.descriptor.geometry = FrameGeometry::new(256, 224, 8.0 / 7.0);
            Ok(())
        }
        fn run_frame(&mut self, _: &crate::input::InputSnapshot) -> Result<(), BridgeError> {
            self.frames += 1;
            Ok(())
        }
        fn video(&self) -> Option<crate::frame::FrameView<'_>> {
            None
        }
        fn drain_audio(&mut self, _: &mut dyn AudioSink) {}
        fn reset(&mut self) -> Result<(), BridgeError> {
            Ok(())
        }
        fn frame_count(&self) -> u64 {
            self.frames
        }
    }

    #[test]
    fn the_picture_shape_is_the_one_the_running_core_reports() {
        // Nothing running: the app falls back to the core's declared shape.
        assert_eq!(EmulatorBridge::new().core_picture_aspect(), None);
        // A plain session reports what it was declared with.
        assert_eq!(running_diagnostic().core_picture_aspect(), Some(4.0 / 3.0));

        // A Game Boy core declared at 10:9 that loads a Super Game Boy cartridge: the shape the
        // app is given is the core's 8:7, not the declaration's. Before this, the app worked the
        // shape out from the core's settings, which say "Auto" and cannot tell.
        let mut bridge = EmulatorBridge::new();
        let mut spec = descriptor("continuum_gb", "gb");
        spec.geometry = FrameGeometry::new(160, 144, 10.0 / 9.0);
        bridge.declare_core(spec.clone());
        bridge
            .attach_core("continuum_gb", Box::new(BorderSwitcher { descriptor: spec, frames: 0 }))
            .unwrap();
        bridge.launch_headless_named_for_test("continuum_gb", b"rom", "Donkey Kong.gb").unwrap();
        assert_eq!(bridge.core_picture_aspect(), Some(8.0 / 7.0));
    }

    #[test]
    fn search_regions_list_system_ram_then_the_mapped_ones() {
        let bridge = running_mapped_gba();
        let regions = bridge.search_regions();
        let keys: Vec<&str> = regions.iter().map(|r| r.key.as_str()).collect();
        assert_eq!(keys[..3], ["system", "map:0", "map:1"]);
        assert_eq!((regions[1].start, regions[1].size), (0x0300_0000, 0x8000));
        assert_eq!((regions[2].start, regions[2].size), (0x0200_0000, 0x40000));
        assert!(EmulatorBridge::new().search_regions().is_empty());
    }

    #[test]
    fn a_search_over_iwram_finds_the_timer_at_its_console_address() {
        let mut bridge = running_mapped_gba();
        assert_eq!(bridge.search_start_in("map:0", SearchWidth::Bits8, false).unwrap(), 0x8000);
        assert_eq!(bridge.search_region_key().as_deref(), Some("map:0"));
        bridge.step_headless_for_test(1).unwrap();
        assert_eq!(bridge.search_filter(SearchFilter::IncreasedBy(1)).unwrap(), 1);
        let hits = bridge.search_results(10);
        assert_eq!(hits[0].address, 0x0300_0010);

        // The cheat made from it is a bus poke, and it pins IWRAM, not EWRAM at the same offset.
        let code = bridge.search_poke_code(hits[0].address, 7, 1).unwrap();
        assert_eq!(code, "poke:03000010:07:1:bus");
        bridge.apply_cheats(vec![code], &[1]).unwrap();
        bridge.step_headless_for_test(3).unwrap();
        let session = bridge.session.as_ref().unwrap();
        let iwram = crate::memory_maps::searchable(session.core.memory_map())[0].clone();
        let byte = unsafe { *((iwram.host + 0x10) as *const u8) };
        assert_eq!(byte, 7);
        assert_ne!(bridge.read_memory(crate::memory::MEMORY_SYSTEM_RAM, 0x10, 1).unwrap(), vec![7]);
    }

    #[test]
    fn a_cht_ram_cheat_lands_where_retroarch_puts_it_on_mgba() {
        use crate::memory::MEMORY_SYSTEM_RAM;
        // RetroArch's cheat space on mGBA's GBA is IWRAM ($8000 bytes) then EWRAM, so the file's
        // $8010 is EWRAM byte $10 (SYSTEM_RAM offset $10), and its $10 is IWRAM's timer byte.
        let file = crate::cheats::cht::parse(
            "cheats = 2\n\
             cheat0_handler = 1\ncheat0_address = 0x8010\ncheat0_value = 0x63\n\
             cheat0_memory_search_size = 3\ncheat0_enable = true\n\
             cheat1_handler = 1\ncheat1_address = 0x10\ncheat1_value = 7\n\
             cheat1_memory_search_size = 3\ncheat1_enable = true\n",
        );
        assert!(file.warnings.is_empty(), "{:?}", file.warnings);
        let codes: Vec<String> = file.cheats.into_iter().map(|c| c.code).collect();
        assert_eq!(codes, ["poke:8010:63:1:cht", "poke:0010:07:1:cht"]);

        let mut bridge = running_mapped_gba();
        let untouched = bridge.read_memory(MEMORY_SYSTEM_RAM, 0x8010, 1).unwrap();
        assert_eq!(bridge.apply_cheats(codes, &[1, 1]).unwrap(), 2);
        bridge.step_headless_for_test(3).unwrap();
        assert_eq!(
            bridge.read_memory(MEMORY_SYSTEM_RAM, 0x10, 1).unwrap(),
            vec![0x63]
        );
        assert_eq!(
            bridge.read_memory(MEMORY_SYSTEM_RAM, 0x8010, 1).unwrap(),
            untouched,
            "the old reading, a SYSTEM_RAM offset, would have landed 32 KB too far into EWRAM"
        );
        let session = bridge.session.as_ref().unwrap();
        let iwram = crate::memory_maps::cheat_buffers(session.core.memory_map())
            .next()
            .unwrap();
        // SAFETY: the fixture's IWRAM buffer, alive for the core's lifetime; the core is idle.
        let timer = unsafe { *((iwram.host + 0x10) as *const u8) };
        assert_eq!(timer, 7, "pinned although the core bumps it every frame");
    }

    #[test]
    fn a_cht_poke_falls_back_to_system_ram_on_a_core_without_cheat_ram_in_its_map() {
        use crate::cores::DiagnosticCore;
        use crate::memory::MEMORY_SYSTEM_RAM;
        let mut bridge = running_diagnostic();
        let session = bridge.session.as_ref().unwrap();
        assert!(session.core.memory_map().is_empty());
        let health = DiagnosticCore::HEALTH_OFFSET as u64;
        let poke = Poke::parse(&format!("poke:{health:04X}:63:1:cht")).unwrap();
        assert!(poke.cht);
        // The diagnostic core takes no core cheats, so the poke is installed directly, as in
        // `a_poke_pins_the_value_every_frame_and_is_not_sent_to_the_core`.
        bridge.session.as_mut().unwrap().pokes = vec![poke];
        bridge.step_headless_for_test(5).unwrap();
        assert_eq!(
            bridge.read_memory(MEMORY_SYSTEM_RAM, health, 1).unwrap(),
            vec![0x63]
        );
    }

    #[test]
    fn unknown_or_stale_region_keys_are_refused_with_a_sentence() {
        let mut bridge = running_mapped_gba();
        assert!(bridge.search_start_in("map:3", SearchWidth::Bits8, false).is_err(), "ROM");
        assert!(bridge.search_start_in("bogus", SearchWidth::Bits8, false).is_err());
        assert!(bridge.search_start_in("map:99", SearchWidth::Bits8, false).is_err());
        // A plain search still means system RAM and makes plain pokes.
        bridge.search_start(SearchWidth::Bits8, false).unwrap();
        assert_eq!(bridge.search_region_key().as_deref(), Some("system"));
        assert_eq!(bridge.search_poke_code(0x10, 1, 1).unwrap(), "poke:0010:01:1");
    }

    #[test]
    fn pokes_are_split_out_and_the_core_table_stays_whole_and_in_order() {
        let calls = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut bridge = EmulatorBridge::new();
        bridge.declare_core(descriptor("rec", "test"));
        bridge
            .attach_core(
                "rec",
                Box::new(CheatRecorder {
                    descriptor: descriptor("rec", "test"),
                    calls: calls.clone(),
                }),
            )
            .unwrap();
        bridge.launch_headless_for_test("rec", b"rom").unwrap();
        let codes = vec![
            "AAAA".to_string(),
            "poke:0010:63:1".to_string(),
            "BBBB".to_string(),
            "poke:0020:01:1".to_string(),
            "CCCC".to_string(),
        ];
        // The second poke is off, the middle code is off.
        let active = bridge.apply_cheats(codes, &[1, 1, 0, 0, 1]).unwrap();
        assert_eq!(active, 3, "two codes and one poke are on");
        assert_eq!(
            *calls.lock().unwrap(),
            vec!["reset", "0:true:AAAA", "1:true:CCCC"],
            "the core sees only the codes that are on, numbered without gaps (RetroArch's way)"
        );
        // Switching the last one off leaves the core with no cheats at all.
        calls.lock().unwrap().clear();
        bridge
            .apply_cheats(vec!["AAAA".into(), "CCCC".into()], &[1, 0])
            .unwrap();
        assert_eq!(*calls.lock().unwrap(), vec!["reset", "0:true:AAAA"]);
        calls.lock().unwrap().clear();
        let again = vec![
            "AAAA".to_string(),
            "poke:0010:63:1".to_string(),
            "BBBB".to_string(),
            "poke:0020:01:1".to_string(),
            "CCCC".to_string(),
        ];
        assert_eq!(bridge.apply_cheats(again, &[1, 1, 0, 0, 1]).unwrap(), 3);
        assert_eq!(bridge.session.as_ref().unwrap().pokes.len(), 1);

        // A malformed poke refuses the whole update and leaves the previous list in force.
        calls.lock().unwrap().clear();
        assert!(bridge
            .apply_cheats(vec!["DDDD".into(), "poke:nope".into()], &[1, 1])
            .is_err());
        assert!(calls.lock().unwrap().is_empty());
        assert_eq!(bridge.active_cheat_count(), 3);
    }

    #[test]
    fn a_split_gearsystem_code_still_counts_as_one_cheat() {
        let calls = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut bridge = EmulatorBridge::new();
        bridge.declare_core(descriptor("gearsystem", "sms"));
        bridge
            .attach_core(
                "gearsystem",
                Box::new(CheatRecorder { descriptor: descriptor("gearsystem", "sms"), calls: calls.clone() }),
            )
            .unwrap();
        bridge.launch_headless_for_test("gearsystem", b"rom").unwrap();
        let codes = vec!["C000:FF+C001:01".to_string(), "poke:0010:63:1".to_string(), "C002:02".to_string()];
        assert_eq!(bridge.apply_cheats(codes, &[1, 1, 0]).unwrap(), 2);
        // The core gets one entry per part; the screen and the other phone hear 2: one code, one poke.
        assert_eq!(*calls.lock().unwrap(), vec!["reset", "0:true:00C000FF", "1:true:00C00101"]);
        assert_eq!(bridge.active_cheat_count(), 2);
        assert_eq!(bridge.session.as_ref().unwrap().enabled_codes, 1);
        bridge.clear_cheats().unwrap();
        assert_eq!(bridge.active_cheat_count(), 0);
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
