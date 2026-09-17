//! Phase 1 facade: exposes [`EmulatorBridge`] to the web UI via `wasm-bindgen`.
//!
//! Intentionally thin. Every method here converts arguments, calls one bridge
//! method, and converts the result — no logic, no state beyond the bridge itself.
//! That discipline is what keeps Phase 2's UniFFI facade a peer rather than a
//! fork: when Swift needs `launch`, it wraps the same [`EmulatorBridge::launch`],
//! and behaviour cannot drift between platforms because there is only one
//! implementation to drift from.
//!
//! Interior mutability note: `wasm_bindgen` cannot export `async fn` taking
//! `&mut self` (the future would have to borrow across an await it does not own),
//! so the bridge sits behind `Rc<RefCell<_>>` and every method takes `&self`.
//! Borrows are never held across an `.await` — the async paths build their result
//! first and only then borrow to install it.

use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::prelude::*;

use crate::bridge::{CoreRetention, EmulatorBridge, TickReport};
use crate::cores::{ContentHint, CoreDescriptor, LibretroRuntimeHandle, WasmCore};
use crate::frame::{FrameGeometry, PixelFormat};
use crate::gfx::{Renderer, ScaleFilter, ScaleMode};
use crate::input::{Button, PadKind, PadSource};

/// Installs a panic hook that prints Rust panics to the browser console, plus a
/// logger. Without the hook a panic surfaces as `unreachable executed`, which is
/// worth precisely nothing when debugging.
#[wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();
    let _ = console_log::init_with_level(log::Level::Info);
    log::info!(
        "emulator-bridge {} initialised (wasm32)",
        env!("CARGO_PKG_VERSION")
    );
}

/// Core metadata handed over from `cores/manifest.json`.
///
/// A declaration, not a load: constructing these for every supported system costs
/// a few hundred bytes and zero network requests.
#[wasm_bindgen]
pub struct CoreDeclaration {
    inner: CoreDescriptor,
}

#[wasm_bindgen]
impl CoreDeclaration {
    /// `systems` is a comma-separated list of system ids this core can run.
    #[allow(clippy::too_many_arguments)]
    #[wasm_bindgen(constructor)]
    pub fn new(
        id: String,
        display_name: String,
        systems: String,
        module_url: String,
        base_width: u32,
        base_height: u32,
        aspect_ratio: f32,
        target_fps: f64,
        audio_sample_rate: u32,
        pixel_format: u32,
    ) -> Result<CoreDeclaration, JsError> {
        let format = PixelFormat::from_u32(pixel_format)
            .ok_or_else(|| JsError::new(&format!("unknown pixel format {pixel_format}")))?;
        Ok(Self {
            inner: CoreDescriptor {
                id,
                display_name,
                systems: systems
                    .split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(String::from)
                    .collect(),
                geometry: FrameGeometry::new(base_width, base_height, aspect_ratio),
                target_fps,
                audio_sample_rate,
                pixel_format: format,
                module_url,
                priority: 0,
            },
        })
    }

    /// Widens the maximum framebuffer the core may emit (PS1 mode switches, N64
    /// hi-res). Sizes the GPU texture once instead of reallocating mid-game.
    #[wasm_bindgen(js_name = withMaxGeometry)]
    pub fn with_max_geometry(mut self, max_width: u32, max_height: u32) -> CoreDeclaration {
        self.inner.geometry = self.inner.geometry.with_max(max_width, max_height);
        self
    }

    /// Sets which core wins when several declare the same system. Higher wins;
    /// the default is 0.
    #[wasm_bindgen(js_name = withPriority)]
    pub fn with_priority(mut self, priority: i32) -> CoreDeclaration {
        self.inner.priority = priority;
        self
    }
}

/// Telemetry slot count. See [`WasmEmulatorBridge::telemetry_layout`] for names.
const TELEMETRY_LEN: usize = 12;

/// Staging capacity for [`WasmEmulatorBridge::drain_audio_into`], in samples.
/// 8192 interleaved samples = 4096 stereo frames ≈ 85 ms at 48 kHz — far more than
/// one tick ever needs, so a slow frame cannot truncate a drain.
const AUDIO_STAGING_SAMPLES: usize = 8192;

/// Telemetry indices. Kept in one place and published to JS via
/// `telemetryLayout()` so the two sides cannot drift apart.
mod telemetry {
    pub const STEPS: usize = 0;
    pub const DROPPED: usize = 1;
    pub const PRESENTED: usize = 2;
    pub const RESYNCED: usize = 3;
    pub const DISPLAY_FPS: usize = 4;
    pub const FRAME_COUNT: usize = 5;
    pub const AUDIO_QUEUED_FRAMES: usize = 6;
    pub const AUDIO_CAPACITY_FRAMES: usize = 7;
    pub const AUDIO_OVERRUNS: usize = 8;
    pub const AUDIO_UNDERRUNS: usize = 9;
    /// Cumulative counters. Instantaneous queue depth is near zero right after the
    /// host drains, so only these prove that audio is actually flowing.
    pub const AUDIO_FRAMES_SUBMITTED: usize = 10;
    pub const AUDIO_FRAMES_DRAINED: usize = 11;
}

/// The object the web UI holds. One per page.
///
/// ## Zero-copy hot path
///
/// `tick` and `drainAudioInto` write into two buffers owned by this struct, which
/// JS reads through `Float64Array`/`Float32Array` views over wasm memory. The
/// obvious alternatives both cost an allocation per frame — returning a
/// `#[wasm_bindgen]` struct hands JS a wasm pointer it must `.free()`, and taking a
/// `&mut [f32]` makes wasm-bindgen malloc, copy in, copy out and free on every
/// call. At 60 Hz forever, neither is acceptable, and both are easy to get subtly
/// wrong (a missed `.free()` is a slow leak with no symptom until it is a big one).
///
/// Both buffers are `Box<[T]>` and are never resized, so the pointers stay valid
/// for the lifetime of the bridge. The only way a view can go stale is wasm memory
/// growth detaching the `ArrayBuffer`; the host guards against that by checking
/// `buffer.byteLength === 0` and re-creating the view.
#[wasm_bindgen(js_name = EmulatorBridge)]
pub struct WasmEmulatorBridge {
    inner: Rc<RefCell<EmulatorBridge>>,
    /// Fixed-size, never reallocated: see the struct docs.
    telemetry: RefCell<Box<[f64]>>,
    audio_staging: RefCell<Box<[f32]>>,
}

#[wasm_bindgen(js_class = EmulatorBridge)]
impl WasmEmulatorBridge {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            inner: Rc::new(RefCell::new(EmulatorBridge::new())),
            telemetry: RefCell::new(vec![0.0; TELEMETRY_LEN].into_boxed_slice()),
            audio_staging: RefCell::new(vec![0.0; AUDIO_STAGING_SAMPLES].into_boxed_slice()),
        }
    }

    // ------------------------------------------------------- shared hot buffers

    /// Pointer to the telemetry block in wasm memory. Stable for the bridge's
    /// lifetime; read it once at boot and keep the view.
    #[wasm_bindgen(getter, js_name = telemetryPtr)]
    pub fn telemetry_ptr(&self) -> *const f64 {
        self.telemetry.borrow().as_ptr()
    }

    #[wasm_bindgen(getter, js_name = telemetryLen)]
    pub fn telemetry_len(&self) -> usize {
        TELEMETRY_LEN
    }

    /// Slot names to indices, so JS never hard-codes the layout.
    #[wasm_bindgen(js_name = telemetryLayout)]
    pub fn telemetry_layout() -> js_sys::Object {
        let obj = js_sys::Object::new();
        let set = |name: &str, index: usize| {
            let _ = js_sys::Reflect::set(
                &obj,
                &JsValue::from_str(name),
                &JsValue::from_f64(index as f64),
            );
        };
        set("steps", telemetry::STEPS);
        set("dropped", telemetry::DROPPED);
        set("presented", telemetry::PRESENTED);
        set("resynced", telemetry::RESYNCED);
        set("displayFps", telemetry::DISPLAY_FPS);
        set("frameCount", telemetry::FRAME_COUNT);
        set("audioQueuedFrames", telemetry::AUDIO_QUEUED_FRAMES);
        set("audioCapacityFrames", telemetry::AUDIO_CAPACITY_FRAMES);
        set("audioOverruns", telemetry::AUDIO_OVERRUNS);
        set("audioUnderruns", telemetry::AUDIO_UNDERRUNS);
        set("audioFramesSubmitted", telemetry::AUDIO_FRAMES_SUBMITTED);
        set("audioFramesDrained", telemetry::AUDIO_FRAMES_DRAINED);
        obj
    }

    /// Pointer to the audio staging buffer, filled by
    /// [`Self::drain_audio_into`]. Stable for the bridge's lifetime.
    #[wasm_bindgen(getter, js_name = audioBufferPtr)]
    pub fn audio_buffer_ptr(&self) -> *const f32 {
        self.audio_staging.borrow().as_ptr()
    }

    #[wasm_bindgen(getter, js_name = audioBufferLen)]
    pub fn audio_buffer_len(&self) -> usize {
        AUDIO_STAGING_SAMPLES
    }

    /// Initialises WebGPU against `canvas`.
    ///
    /// `width`/`height` are physical pixels (CSS size x `devicePixelRatio`). wgpu
    /// writes them onto the canvas element, so JS must not set `canvas.width`
    /// itself.
    ///
    /// Rejects when WebGPU is unavailable. There is no WebGL2 fallback by design —
    /// the UI reports the situation rather than silently degrading.
    #[wasm_bindgen(js_name = initGpu)]
    pub async fn init_gpu(
        &self,
        canvas: web_sys::HtmlCanvasElement,
        width: u32,
        height: u32,
    ) -> Result<(), JsError> {
        // Built before borrowing: no RefCell borrow may span an await.
        let renderer = Renderer::from_canvas(canvas, width, height)
            .await
            .map_err(to_js_error)?;
        self.inner.borrow_mut().attach_renderer(renderer);
        Ok(())
    }

    #[wasm_bindgen(getter, js_name = hasGpu)]
    pub fn has_gpu(&self) -> bool {
        self.inner.borrow().has_renderer()
    }

    /// `"uninitialised" | "idle" | "running" | "paused"`.
    #[wasm_bindgen(getter)]
    pub fn status(&self) -> String {
        self.inner.borrow().status().as_str().to_string()
    }

    #[wasm_bindgen(getter, js_name = adapterInfo)]
    pub fn adapter_info(&self) -> Option<String> {
        self.inner.borrow().renderer().map(|r| r.adapter_summary())
    }

    // ----------------------------------------------------------------- registry

    /// Declares a core. Loads nothing.
    #[wasm_bindgen(js_name = declareCore)]
    pub fn declare_core(&self, declaration: CoreDeclaration) {
        self.inner.borrow_mut().declare_core(declaration.inner);
    }

    /// `"declared" | "loaded" | "bound" | "failed"`, or `undefined` if unknown.
    #[wasm_bindgen(js_name = coreState)]
    pub fn core_state(&self, core_id: &str) -> Option<String> {
        self.inner
            .borrow()
            .core_state(core_id)
            .map(|s| s.as_str().to_string())
    }

    /// Core id able to run `system_id`, for the launch path to fetch.
    #[wasm_bindgen(js_name = coreForSystem)]
    pub fn core_for_system(&self, system_id: &str) -> Option<String> {
        self.inner
            .borrow()
            .core_for_system(system_id)
            .map(|d| d.id.clone())
    }

    /// Ids of every core that can run `system_id`, best first.
    ///
    /// The subcore relationship is one-to-many, and the registry is the only thing
    /// that knows the mapping — so the UI asks rather than duplicating the rule. An
    /// empty result means no declared core handles the system; a single entry means
    /// there is nothing to choose between and the UI should not offer a picker.
    #[wasm_bindgen(js_name = coresForSystem)]
    pub fn cores_for_system(&self, system_id: &str) -> Vec<String> {
        self.inner
            .borrow()
            .cores_for_system(system_id)
            .iter()
            .map(|d| d.id.clone())
            .collect()
    }

    /// Resolves the core to launch, given an optional stored preference.
    ///
    /// Pass the user's choice for the system; a preference that no longer applies is
    /// ignored in favour of the default rather than raising, because it comes from
    /// persisted UI state that can outlive a manifest change.
    #[wasm_bindgen(js_name = resolveCoreForSystem)]
    pub fn resolve_core_for_system(
        &self,
        system_id: &str,
        preferred: Option<String>,
    ) -> Option<String> {
        self.inner
            .borrow()
            .resolve_core_for_system(system_id, preferred.as_deref())
            .map(|d| d.id.clone())
    }

    /// Human-readable name for a declared core, for labelling a picker.
    #[wasm_bindgen(js_name = coreDisplayName)]
    pub fn core_display_name(&self, core_id: &str) -> Option<String> {
        self.inner
            .borrow()
            .core_descriptor(core_id)
            .map(|d| d.display_name.clone())
    }

    /// Hands fetched module bytes to the registry, making the core runnable.
    #[wasm_bindgen(js_name = attachCoreModule)]
    pub fn attach_core_module(&self, core_id: &str, bytes: &[u8]) -> Result<(), JsError> {
        self.inner
            .borrow_mut()
            .attach_core_module(core_id, bytes)
            .map_err(to_js_error)
    }

    /// Frees a core's memory. Declaration survives, so it can be re-fetched.
    #[wasm_bindgen(js_name = unloadCore)]
    pub fn unload_core(&self, core_id: &str) -> Result<(), JsError> {
        self.inner
            .borrow_mut()
            .unload_core(core_id)
            .map_err(to_js_error)
    }

    #[wasm_bindgen(getter, js_name = residentCoreCount)]
    pub fn resident_core_count(&self) -> usize {
        self.inner.borrow().resident_core_count()
    }

    /// Ids of cores currently holding memory. Empty while browsing, under the default
    /// retention policy.
    #[wasm_bindgen(js_name = residentCoreIds)]
    pub fn resident_core_ids(&self) -> Vec<String> {
        self.inner.borrow().resident_core_ids()
    }

    /// `"drop"` (default) frees a core when its session ends; `"warm"` keeps it
    /// instantiated for a faster relaunch at the cost of tens of megabytes.
    #[wasm_bindgen(js_name = setCoreRetention)]
    pub fn set_core_retention(&self, retention: &str) {
        let retention = match retention {
            "warm" | "keep" => CoreRetention::KeepWarm,
            _ => CoreRetention::Drop,
        };
        self.inner.borrow_mut().set_core_retention(retention);
    }

    #[wasm_bindgen(getter, js_name = coreRetention)]
    pub fn core_retention(&self) -> String {
        self.inner.borrow().core_retention().as_str().to_string()
    }

    /// Frees every resident core. No-op while a session is running.
    #[wasm_bindgen(js_name = unloadAllCores)]
    pub fn unload_all_cores(&self) -> usize {
        self.inner.borrow_mut().unload_all_cores()
    }

    /// Installs an instantiated libretro core.
    ///
    /// The platform layer owns instantiation because the core is a *separate* wasm
    /// module with its own memory and imports, which only JS can wire up (see
    /// `web/src/engine/core-runtime.js`). What crosses this boundary is a handle to
    /// that runtime; everything after — pacing, input, audio, presentation — is Rust.
    ///
    /// The manifest's geometry is passed as a starting point and is superseded by
    /// whatever the core reports once content is loaded.
    #[wasm_bindgen(js_name = attachCoreRuntime)]
    pub fn attach_core_runtime(
        &self,
        core_id: &str,
        runtime: LibretroRuntimeHandle,
    ) -> Result<(), JsError> {
        let mut bridge = self.inner.borrow_mut();
        let descriptor = bridge
            .core_descriptor(core_id)
            .ok_or_else(|| JsError::new(&format!("core '{core_id}' was never declared")))?
            .clone();
        let core = WasmCore::new(descriptor, runtime);
        bridge
            .attach_core(core_id, Box::new(core))
            .map_err(to_js_error)
    }

    // -------------------------------------------------------------- gamepads

    /// Applies one poll of a W3C standard gamepad.
    ///
    /// `buttons` is `navigator.getGamepads()[i].buttons.map(b => b.pressed)` and
    /// `axes` is that pad's `axes` array. The button *layout* mapping lives in Rust
    /// (see `GamepadBridge`), so Phase 2's `GameController` code maps the same way
    /// instead of reinventing it in Swift.
    #[wasm_bindgen(js_name = applyGamepad)]
    pub fn apply_gamepad(&self, port: u32, buttons: &[u8], axes: &[f32]) {
        // `&[u8]` rather than `&[bool]`: wasm-bindgen has no bool-slice ABI, and a
        // Uint8Array is what JS can hand over without a per-element conversion.
        let pressed: Vec<bool> = buttons.iter().map(|b| *b != 0).collect();
        self.inner
            .borrow_mut()
            .apply_gamepad(port as usize, &pressed, axes);
    }

    /// Registers a controller on a port. `kind` is `"gamepad"`, `"keyboard"` or
    /// `"touch"`.
    #[wasm_bindgen(js_name = connectPad)]
    pub fn connect_pad(&self, port: u32, kind: &str, label: &str) -> bool {
        let kind = match kind {
            "keyboard" => PadKind::Keyboard,
            "touch" => PadKind::Touch,
            "gamepad-unmapped" => PadKind::UnmappedGamepad,
            _ => PadKind::StandardGamepad,
        };
        self.inner
            .borrow_mut()
            .connect_pad(port as usize, kind, label)
    }

    #[wasm_bindgen(js_name = disconnectPad)]
    pub fn disconnect_pad(&self, port: u32) {
        self.inner.borrow_mut().disconnect_pad(port as usize);
    }

    #[wasm_bindgen(getter, js_name = connectedPads)]
    pub fn connected_pads(&self) -> usize {
        self.inner.borrow().connected_pads()
    }

    #[wasm_bindgen(js_name = padLabel)]
    pub fn pad_label(&self, port: u32) -> Option<String> {
        self.inner
            .borrow()
            .pad_label(port as usize)
            .map(str::to_string)
    }

    /// First unoccupied port, for auto-assigning a newly connected controller.
    #[wasm_bindgen(js_name = firstFreePadPort)]
    pub fn first_free_pad_port(&self) -> Option<u32> {
        self.inner
            .borrow()
            .first_free_pad_port()
            .map(|port| port as u32)
    }

    // ------------------------------------------------------------------ session

    /// Starts emulating. The core must already be attached.
    ///
    /// `filename` is the content's original name (e.g. `"smb.nes"`). It is not
    /// cosmetic: real cores resolve their `need_fullpath` content-info overrides from
    /// the extension, and reject content whose extension they cannot see.
    pub fn launch(
        &self,
        core_id: &str,
        content_id: &str,
        content: &[u8],
        filename: &str,
    ) -> Result<(), JsError> {
        let hint = ContentHint::from_filename(filename);
        self.inner
            .borrow_mut()
            .launch(core_id, content_id, content, &hint)
            .map_err(to_js_error)
    }

    pub fn stop(&self) {
        self.inner.borrow_mut().stop();
        // Zero the shared telemetry block. Otherwise it keeps reporting the finished
        // session's frame count and audio queue, and any reader — the HUD, a test —
        // sees numbers for a game that is no longer running.
        self.write_telemetry(&TickReport::default());
    }

    pub fn pause(&self) {
        self.inner.borrow_mut().pause();
    }

    /// `now_ms` should be the current `performance.now()`, used to re-anchor
    /// emulated time so the paused interval is not treated as debt.
    pub fn resume(&self, now_ms: f64) {
        self.inner.borrow_mut().resume(now_ms);
    }

    pub fn reset(&self) -> Result<(), JsError> {
        self.inner.borrow_mut().reset().map_err(to_js_error)
    }

    /// `[baseWidth, baseHeight, aspectRatio, fps, sampleRate]` as reported by the
    /// running core, or `undefined` when idle.
    ///
    /// These are the core's numbers, not the manifest's: a real core only knows its
    /// geometry and timing once content is loaded, and what it says wins.
    #[wasm_bindgen(js_name = sessionAvInfo)]
    pub fn session_av_info(&self) -> Option<Vec<f64>> {
        self.inner
            .borrow()
            .session_av_info()
            .map(|info| info.to_vec())
    }

    /// Memory held by the running core in bytes (module + staging), or `undefined`.
    #[wasm_bindgen(getter, js_name = sessionCoreMemoryBytes)]
    pub fn session_core_memory_bytes(&self) -> Option<f64> {
        self.inner
            .borrow()
            .session_core_memory_bytes()
            .map(|bytes| bytes as f64)
    }

    /// Display name of the core running the session, e.g. `"FCEUmm"`.
    #[wasm_bindgen(getter, js_name = sessionCoreName)]
    pub fn session_core_name(&self) -> Option<String> {
        self.inner.borrow().session_core_name().map(str::to_string)
    }

    #[wasm_bindgen(getter, js_name = currentContentId)]
    pub fn current_content_id(&self) -> Option<String> {
        self.inner.borrow().current_content_id().map(str::to_string)
    }

    // --------------------------------------------------------------------- tick

    /// The whole engine, one animation frame's worth: input → core → audio → GPU.
    ///
    /// `now_ms` is the `DOMHighResTimeStamp` handed to the rAF callback. Passing the
    /// callback's own timestamp, rather than calling `performance.now()` here, keeps
    /// pacing aligned with the compositor's frame clock.
    ///
    /// Results land in the telemetry buffer (see [`Self::telemetry_ptr`]) instead of
    /// being returned, so the hot path allocates nothing on either side.
    pub fn tick(&self, now_ms: f64) -> Result<(), JsError> {
        let report = self.inner.borrow_mut().tick(now_ms).map_err(to_js_error)?;
        self.write_telemetry(&report);
        Ok(())
    }

    // -------------------------------------------------------------------- input

    /// `button` is a `RETRO_DEVICE_ID_JOYPAD_*` index (0–15). `source` is
    /// `"keyboard"`, `"touch"` or `"gamepad"`; it selects which input layer to write,
    /// so an idle controller poll cannot clear a held key.
    #[wasm_bindgen(js_name = setButton)]
    pub fn set_button(&self, port: u32, button: u32, pressed: bool, source: Option<String>) {
        let source = source
            .as_deref()
            .map(PadSource::from_str_or_keyboard)
            .unwrap_or(PadSource::Keyboard);
        if let Some(button) = Button::from_u32(button) {
            self.inner
                .borrow_mut()
                .set_button(port as usize, source, button, pressed);
        }
    }

    /// `axis`: 0 = left X, 1 = left Y, 2 = right X, 3 = right Y. Range -1..=1.
    #[wasm_bindgen(js_name = setAxis)]
    pub fn set_axis(&self, port: u32, axis: u32, value: f32, source: Option<String>) {
        let source = source
            .as_deref()
            .map(PadSource::from_str_or_keyboard)
            .unwrap_or(PadSource::Keyboard);
        self.inner
            .borrow_mut()
            .set_axis(port as usize, source, axis as usize, value);
    }

    /// Releases everything one source was holding. Used when the touch overlay is
    /// hidden, or when a keyboard loses focus while a controller keeps playing.
    #[wasm_bindgen(js_name = releaseInputSource)]
    pub fn release_input_source(&self, source: &str) {
        self.inner
            .borrow_mut()
            .release_input_source(PadSource::from_str_or_keyboard(source));
    }

    /// Releases every button on every port. Call on blur / `visibilitychange`.
    #[wasm_bindgen(js_name = releaseAllInput)]
    pub fn release_all_input(&self) {
        self.inner.borrow_mut().release_all_input();
    }

    // -------------------------------------------------------------------- audio

    /// Reports the real device rate, known only after `AudioContext` is unlocked.
    #[wasm_bindgen(js_name = setOutputSampleRate)]
    pub fn set_output_sample_rate(&self, rate: u32) {
        self.inner.borrow_mut().set_output_sample_rate(rate);
    }

    #[wasm_bindgen(getter, js_name = outputSampleRate)]
    pub fn output_sample_rate(&self) -> u32 {
        self.inner.borrow().output_sample_rate()
    }

    /// Drains up to `max_samples` of queued PCM into the shared audio buffer,
    /// returning how many interleaved samples were written.
    ///
    /// The host reads them through its `Float32Array` view over
    /// [`Self::audio_buffer_ptr`]. Returning fewer than requested is normal — it
    /// simply means the ring is empty, not that anything failed.
    #[wasm_bindgen(js_name = drainAudioInto)]
    pub fn drain_audio_into(&self, max_samples: usize) -> usize {
        let mut staging = self.audio_staging.borrow_mut();
        let limit = max_samples.min(staging.len());
        self.inner.borrow_mut().drain_audio(&mut staging[..limit])
    }

    /// Frames currently queued in the Rust audio ring, read live rather than from the
    /// per-tick telemetry mirror — which is only refreshed while the loop runs.
    #[wasm_bindgen(getter, js_name = audioQueuedFrames)]
    pub fn audio_queued_frames(&self) -> u32 {
        self.inner.borrow().audio_stats().queued_frames
    }

    #[wasm_bindgen(js_name = setMuted)]
    pub fn set_muted(&self, muted: bool) {
        self.inner.borrow_mut().set_muted(muted);
    }

    #[wasm_bindgen(getter, js_name = isMuted)]
    pub fn is_muted(&self) -> bool {
        self.inner.borrow().is_muted()
    }

    // ------------------------------------------------------------------- output

    /// Reconfigures the swapchain. Physical pixels, not CSS pixels.
    pub fn resize(&self, width: u32, height: u32) {
        self.inner.borrow_mut().resize(width, height);
    }

    /// `"nearest"` or `"linear"`.
    #[wasm_bindgen(js_name = setFilter)]
    pub fn set_filter(&self, filter: &str) {
        let filter = match filter {
            "linear" => ScaleFilter::Linear,
            _ => ScaleFilter::Nearest,
        };
        self.inner.borrow_mut().set_filter(filter);
    }

    /// `"aspect"`, `"integer"` or `"stretch"`.
    #[wasm_bindgen(js_name = setScaleMode)]
    pub fn set_scale_mode(&self, mode: &str) {
        let mode = match mode {
            "integer" => ScaleMode::IntegerScale,
            "stretch" => ScaleMode::Stretch,
            _ => ScaleMode::AspectFit,
        };
        self.inner.borrow_mut().set_scale_mode(mode);
    }

    /// Emulation speed multiplier, clamped to 0.05–16.0.
    #[wasm_bindgen(js_name = setSpeed)]
    pub fn set_speed(&self, speed: f64) {
        self.inner.borrow_mut().set_speed(speed);
    }

    /// Reads the presented image back from the GPU as tightly packed RGBA8.
    ///
    /// `width`/`height` of `0` mean the current canvas size. Not a hot path: it
    /// allocates a texture and a staging buffer, and stalls on a buffer map. Use it
    /// for screenshots, save-state thumbnails, and for verifying the render path
    /// without relying on the browser to composite a WebGPU canvas — which software
    /// and headless GPU stacks often will not do.
    #[wasm_bindgen(js_name = captureFrame)]
    pub async fn capture_frame(&self, width: u32, height: u32) -> Result<Vec<u8>, JsError> {
        // Scoped so the RefCell borrow is released before the await.
        let capture = {
            let mut bridge = self.inner.borrow_mut();
            bridge.encode_capture(width, height).map_err(to_js_error)?
        };

        // `map_async` is callback-based; wrap it in a Promise so it can be awaited.
        // `resolve`/`reject` are owned parameters, so moving them into the callback
        // is legal even though the outer closure is `FnMut`.
        let promise = js_sys::Promise::new(&mut |resolve, reject| {
            capture
                .buffer()
                .slice(..)
                .map_async(wgpu::MapMode::Read, move |result| match result {
                    Ok(()) => {
                        let _ = resolve.call0(&JsValue::NULL);
                    }
                    Err(err) => {
                        let _ = reject.call1(&JsValue::NULL, &JsValue::from_str(&err.to_string()));
                    }
                });
        });

        wasm_bindgen_futures::JsFuture::from(promise)
            .await
            .map_err(|err| JsError::new(&format!("frame capture map failed: {err:?}")))?;

        capture.take_rgba().map_err(to_js_error)
    }

    // --------------------------------------------------------------- save state

    #[wasm_bindgen(js_name = saveState)]
    pub fn save_state(&self) -> Result<Vec<u8>, JsError> {
        self.inner.borrow_mut().save_state().map_err(to_js_error)
    }

    #[wasm_bindgen(js_name = loadState)]
    pub fn load_state(&self, data: &[u8]) -> Result<(), JsError> {
        self.inner
            .borrow_mut()
            .load_state(data)
            .map_err(to_js_error)
    }
}

impl Default for WasmEmulatorBridge {
    fn default() -> Self {
        Self::new()
    }
}

/// Plain-Rust helpers, deliberately outside the exported block.
impl WasmEmulatorBridge {
    fn write_telemetry(&self, report: &TickReport) {
        let mut t = self.telemetry.borrow_mut();
        t[telemetry::STEPS] = report.steps as f64;
        t[telemetry::DROPPED] = report.dropped as f64;
        t[telemetry::PRESENTED] = report.presented as u8 as f64;
        t[telemetry::RESYNCED] = report.resynced as u8 as f64;
        t[telemetry::DISPLAY_FPS] = report.display_fps;
        t[telemetry::FRAME_COUNT] = report.frame_count as f64;
        t[telemetry::AUDIO_QUEUED_FRAMES] = report.audio.queued_frames as f64;
        t[telemetry::AUDIO_CAPACITY_FRAMES] = report.audio.capacity_frames as f64;
        t[telemetry::AUDIO_OVERRUNS] = report.audio.overruns as f64;
        t[telemetry::AUDIO_UNDERRUNS] = report.audio.underruns as f64;
        t[telemetry::AUDIO_FRAMES_SUBMITTED] = report.audio.frames_submitted as f64;
        t[telemetry::AUDIO_FRAMES_DRAINED] = report.audio.frames_drained as f64;
    }
}

/// Preserves the full error chain in the JS message; `thiserror`'s `Display` only
/// prints the outermost layer, which usually hides the interesting part.
fn to_js_error(err: impl std::error::Error) -> JsError {
    let mut message = err.to_string();
    let mut source = err.source();
    while let Some(inner) = source {
        message.push_str(": ");
        message.push_str(&inner.to_string());
        source = inner.source();
    }
    JsError::new(&message)
}
