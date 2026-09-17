//! `WasmCore` — an [`EmulatorCore`] backed by a real libretro core compiled to wasm.
//!
//! This is the Phase 1b payoff: everything above the [`EmulatorCore`] trait — the
//! pacer, the renderer, the audio sink, the session lifecycle, the UI — is unchanged
//! from the scaffold. Swapping the diagnostic stand-in for fceumm is this one file
//! plus a registry line.
//!
//! The core itself lives in a separate wasm module with its own memory, driven
//! through the JS runtime in `web/src/engine/core-runtime.js`. Rust owns the
//! sequence, the input, and the staging buffers; JS owns nothing but the marshalling.
//!
//! ```text
//!   run_frame(input)
//!     ├─ host::begin_frame(staging pointers, input snapshot)
//!     ├─ runtime.run()            ─► retro_run() ─► callbacks ─► CoreHost::*
//!     └─ host::end_frame()        ◄─ geometry + audio frame count
//! ```

use js_sys::{Float64Array, Uint8Array};
use wasm_bindgen::prelude::*;

use super::host::{self, VideoArrival};
use super::{ContentHint, CoreDescriptor, EmulatorCore};
use crate::audio::AudioSink;
use crate::error::BridgeError;
use crate::frame::{FrameView, PixelFormat};
use crate::input::InputSnapshot;

/// Audio staging capacity in `i16` samples. A 60 Hz core at 48 kHz emits ~1600
/// samples per frame; 16384 leaves room for a core that batches several frames of
/// audio at once without ever truncating.
const AUDIO_STAGING_SAMPLES: usize = 16_384;

/// Bytes per pixel to reserve for video staging. Sized for the widest format
/// (XRGB8888) so a core switching format mid-session cannot outgrow the buffer.
const STAGING_BYTES_PER_PIXEL: usize = 4;

#[wasm_bindgen]
extern "C" {
    /// The JS `LibretroRuntime` from `web/src/engine/core-runtime.js`.
    ///
    /// Declared as an imported type rather than passed as an opaque `JsValue`, so the
    /// method names and signatures are checked at compile time instead of failing as
    /// `undefined is not a function` at frame 1.
    #[wasm_bindgen(js_name = Object)]
    pub type LibretroRuntimeHandle;

    #[wasm_bindgen(method, js_name = run)]
    fn js_run(this: &LibretroRuntimeHandle);

    #[wasm_bindgen(method, js_name = reset)]
    fn js_reset(this: &LibretroRuntimeHandle);

    #[wasm_bindgen(method, catch, js_name = loadGame)]
    fn js_load_game(
        this: &LibretroRuntimeHandle,
        rom: &[u8],
        extension: &str,
        name: &str,
    ) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(method, js_name = unloadGame)]
    fn js_unload_game(this: &LibretroRuntimeHandle);

    /// `[base_w, base_h, max_w, max_h, aspect, fps, sample_rate]`.
    #[wasm_bindgen(method, js_name = avInfoArray)]
    fn js_av_info(this: &LibretroRuntimeHandle) -> Float64Array;

    /// libretro's pixel-format enum value.
    #[wasm_bindgen(method, getter, js_name = pixelFormat)]
    fn js_pixel_format(this: &LibretroRuntimeHandle) -> u32;

    #[wasm_bindgen(method, js_name = serializeSize)]
    fn js_serialize_size(this: &LibretroRuntimeHandle) -> usize;

    #[wasm_bindgen(method, catch, js_name = serialize)]
    fn js_serialize(this: &LibretroRuntimeHandle) -> Result<Uint8Array, JsValue>;

    #[wasm_bindgen(method, catch, js_name = unserialize)]
    fn js_unserialize(this: &LibretroRuntimeHandle, state: &[u8]) -> Result<(), JsValue>;

    #[wasm_bindgen(method, js_name = destroy)]
    fn js_destroy(this: &LibretroRuntimeHandle);

    /// Linear memory the core module currently occupies.
    #[wasm_bindgen(method, getter, js_name = memoryBytes)]
    fn js_memory_bytes(this: &LibretroRuntimeHandle) -> f64;
}

pub struct WasmCore {
    descriptor: CoreDescriptor,
    runtime: LibretroRuntimeHandle,
    /// Receives the core's framebuffer each frame, written by JS.
    video_staging: Vec<u8>,
    /// Receives the core's PCM each frame, written by JS.
    audio_staging: Vec<i16>,
    last_frame: Option<VideoArrival>,
    audio_frames: usize,
    frame_count: u64,
    content_loaded: bool,
}

impl WasmCore {
    /// Wraps an instantiated runtime. Content is loaded later, through
    /// [`EmulatorCore::load_content`], so the engine controls the sequence.
    pub fn new(descriptor: CoreDescriptor, runtime: LibretroRuntimeHandle) -> Self {
        let pixels = descriptor
            .geometry
            .max_width
            .max(descriptor.geometry.base_width) as usize
            * descriptor
                .geometry
                .max_height
                .max(descriptor.geometry.base_height) as usize;
        // Allocated once per session. A frame that would not fit is dropped with a
        // warning rather than reallocating mid-tick.
        let video_bytes = (pixels * STAGING_BYTES_PER_PIXEL).max(4096);

        log::info!(
            "wasm core '{}' ready: {}x{} max, {} KB video staging",
            descriptor.id,
            descriptor.geometry.max_width,
            descriptor.geometry.max_height,
            video_bytes / 1024
        );

        Self {
            descriptor,
            runtime,
            video_staging: vec![0; video_bytes],
            audio_staging: vec![0; AUDIO_STAGING_SAMPLES],
            last_frame: None,
            audio_frames: 0,
            frame_count: 0,
            content_loaded: false,
        }
    }

    /// Re-reads the core's declared A/V info and pixel format.
    ///
    /// Called after `retro_load_game`, because that is when the values become real:
    /// the manifest's numbers are only a hint for the loading UI, and a core may
    /// report different geometry, refresh rate or sample rate once it has seen the
    /// content. The core always wins.
    fn sync_descriptor_from_core(&mut self) {
        let info = self.runtime.js_av_info();
        if info.length() < 7 {
            log::warn!("core returned a short av_info array; keeping manifest values");
            return;
        }
        let base_width = info.get_index(0) as u32;
        let base_height = info.get_index(1) as u32;
        let max_width = info.get_index(2) as u32;
        let max_height = info.get_index(3) as u32;
        let aspect_ratio = info.get_index(4) as f32;
        let fps = info.get_index(5);
        let sample_rate = info.get_index(6);

        if base_width > 0 && base_height > 0 {
            self.descriptor.geometry.base_width = base_width;
            self.descriptor.geometry.base_height = base_height;
            self.descriptor.geometry.max_width = max_width.max(base_width);
            self.descriptor.geometry.max_height = max_height.max(base_height);
        }
        // A core reporting 0.0 means "derive from geometry", per libretro.
        if aspect_ratio > 0.0 {
            self.descriptor.geometry.aspect_ratio = aspect_ratio;
        } else if base_width > 0 && base_height > 0 {
            self.descriptor.geometry.aspect_ratio = base_width as f32 / base_height as f32;
        }
        if fps > 1.0 {
            self.descriptor.target_fps = fps;
        }
        if sample_rate > 1000.0 {
            self.descriptor.audio_sample_rate = sample_rate as u32;
        }
        if let Some(format) = PixelFormat::from_libretro(self.runtime.js_pixel_format()) {
            self.descriptor.pixel_format = format;
        }

        log::info!(
            "core av_info: {}x{} (max {}x{}), aspect {:.3}, {:.4} fps, {} Hz, {:?}",
            self.descriptor.geometry.base_width,
            self.descriptor.geometry.base_height,
            self.descriptor.geometry.max_width,
            self.descriptor.geometry.max_height,
            self.descriptor.geometry.aspect_ratio,
            self.descriptor.target_fps,
            self.descriptor.audio_sample_rate,
            self.descriptor.pixel_format,
        );

        // Growing the staging buffer here is safe: no frame is in flight during load.
        let needed = self.descriptor.geometry.max_width as usize
            * self.descriptor.geometry.max_height as usize
            * STAGING_BYTES_PER_PIXEL;
        if needed > self.video_staging.len() {
            log::info!("growing video staging to {} KB", needed / 1024);
            self.video_staging.resize(needed, 0);
        }
    }
}

impl EmulatorCore for WasmCore {
    fn descriptor(&self) -> &CoreDescriptor {
        &self.descriptor
    }

    fn load_content(&mut self, content: &[u8], hint: &ContentHint) -> Result<(), BridgeError> {
        if content.is_empty() {
            return Err(BridgeError::InvalidContent {
                core_id: self.descriptor.id.clone(),
                reason: "content is empty".into(),
            });
        }

        self.runtime
            .js_load_game(content, &hint.extension, &hint.name)
            .map_err(|err| BridgeError::InvalidContent {
                core_id: self.descriptor.id.clone(),
                reason: describe_js_error(&err),
            })?;

        self.content_loaded = true;
        self.frame_count = 0;
        self.last_frame = None;
        self.sync_descriptor_from_core();
        Ok(())
    }

    fn run_frame(&mut self, input: &InputSnapshot) -> Result<(), BridgeError> {
        if !self.content_loaded {
            return Err(BridgeError::NoSession);
        }

        // Pointers are taken in a scope that ends before the JS call, so no Rust
        // reference into these buffers is alive while the host writes to them.
        let (video_ptr, video_len) = {
            let staging = &mut self.video_staging;
            (staging.as_mut_ptr() as u32, staging.len() as u32)
        };
        let (audio_ptr, audio_len) = {
            let staging = &mut self.audio_staging;
            (staging.as_mut_ptr() as u32, staging.len() as u32)
        };

        host::begin_frame(video_ptr, video_len, audio_ptr, audio_len, *input);
        self.runtime.js_run();
        let (video, audio_frames, geometry) = host::end_frame();

        // `None` means the core duped the frame; the previous one stays valid and the
        // renderer re-presents its existing texture.
        if let Some(arrival) = video {
            self.last_frame = Some(arrival);
        }
        self.audio_frames = audio_frames as usize;

        if let Some(change) = geometry {
            if change.width > 0 && change.height > 0 {
                self.descriptor.geometry.base_width = change.width;
                self.descriptor.geometry.base_height = change.height;
            }
            if change.aspect_ratio > 0.0 {
                self.descriptor.geometry.aspect_ratio = change.aspect_ratio;
            }
            log::debug!(
                "core changed geometry to {}x{} aspect {:.3}",
                change.width,
                change.height,
                change.aspect_ratio
            );
        }

        self.frame_count += 1;
        Ok(())
    }

    fn video(&self) -> Option<FrameView<'_>> {
        let arrival = self.last_frame?;
        Some(FrameView {
            data: &self.video_staging,
            width: arrival.width,
            height: arrival.height,
            stride_bytes: arrival.pitch,
            format: arrival.format,
        })
    }

    fn drain_audio(&mut self, sink: &mut dyn AudioSink) {
        let samples = self.audio_frames * 2;
        if samples == 0 {
            return;
        }
        let samples = samples.min(self.audio_staging.len());
        // Straight from staging into the sink: libretro's native i16 needs no
        // intermediate buffer.
        sink.submit_i16(&self.audio_staging[..samples]);
        self.audio_frames = 0;
    }

    fn reset(&mut self) -> Result<(), BridgeError> {
        if !self.content_loaded {
            return Err(BridgeError::NoSession);
        }
        self.runtime.js_reset();
        self.frame_count = 0;
        Ok(())
    }

    fn state_size(&self) -> usize {
        if self.content_loaded {
            self.runtime.js_serialize_size()
        } else {
            0
        }
    }

    fn save_state(&self, dst: &mut [u8]) -> Result<usize, BridgeError> {
        let state = self
            .runtime
            .js_serialize()
            .map_err(|err| BridgeError::SaveState(describe_js_error(&err)))?;
        let length = state.length() as usize;
        if dst.len() < length {
            return Err(BridgeError::SaveState(format!(
                "buffer of {} bytes is too small for {length} bytes of state",
                dst.len()
            )));
        }
        state.copy_to(&mut dst[..length]);
        Ok(length)
    }

    fn load_state(&mut self, src: &[u8]) -> Result<(), BridgeError> {
        self.runtime
            .js_unserialize(src)
            .map_err(|err| BridgeError::SaveState(describe_js_error(&err)))?;
        Ok(())
    }

    fn frame_count(&self) -> u64 {
        self.frame_count
    }

    fn memory_bytes(&self) -> Option<u64> {
        // Includes the core's own heap, so it grows once content is loaded. This is the
        // number that matters for a memory budget, and the one the status bar shows.
        Some(
            self.runtime.js_memory_bytes() as u64
                + self.video_staging.len() as u64
                + (self.audio_staging.len() * 2) as u64,
        )
    }
}

impl Drop for WasmCore {
    fn drop(&mut self) {
        // Releases the core's own allocations (ROM, save-state scratch) and calls
        // retro_deinit. Without this, relaunching leaks a whole core instance.
        self.runtime.js_destroy();
        log::info!(
            "wasm core '{}' released after {} frames ({} oversized, {} audio overflows)",
            self.descriptor.id,
            self.frame_count,
            host::oversized_frames(),
            host::audio_overflows()
        );
    }
}

/// Turns a thrown JS value into something readable in a Rust error.
fn describe_js_error(err: &JsValue) -> String {
    if let Some(text) = err.as_string() {
        return text;
    }
    if let Some(error) = err.dyn_ref::<js_sys::Error>() {
        return String::from(error.message());
    }
    format!("{err:?}")
}
