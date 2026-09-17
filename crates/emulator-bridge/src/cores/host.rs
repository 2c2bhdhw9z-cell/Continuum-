//! The frontend half of the libretro callback contract, on the Rust side.
//!
//! ## Why this is not just methods on the bridge
//!
//! A frame runs like this:
//!
//! ```text
//!   bridge.tick()  →  WasmCore::run_frame()  →  JS runtime.run()
//!                        →  core.retro_run()  →  host.video_refresh (JS import)
//!                        →  CoreHost.videoReady()  ←── back into Rust, mid-tick
//! ```
//!
//! By the time the core calls back, `EmulatorBridge` is already mutably borrowed by
//! the tick. Anything reachable from JS during `retro_run` therefore must not touch
//! the bridge, or the `RefCell` panics on the second borrow. So the per-frame
//! exchange lives in a thread-local of its own, and [`CoreHost`]'s methods are
//! static — there is no `&self` to borrow.
//!
//! ## How pixels cross the boundary
//!
//! The core has its own linear memory; Rust cannot reference into it. Each frame the
//! active [`super::WasmCore`] publishes the address and capacity of its staging
//! buffers here, JS copies the core's framebuffer into that address through a typed
//! array over Rust's memory, and then reports the geometry. One copy, no allocation,
//! and the buffers belong to the core object rather than to this module.
//!
//! Single-threaded by construction: wasm has one thread, `retro_run` is synchronous,
//! and the publish/consume window is exactly the duration of one `run()` call.

use core::cell::RefCell;

use wasm_bindgen::prelude::*;

use crate::frame::PixelFormat;
use crate::input::InputSnapshot;

/// Geometry of a frame the core just produced.
#[derive(Debug, Clone, Copy)]
pub(crate) struct VideoArrival {
    pub width: u32,
    pub height: u32,
    /// Row stride in bytes, as reported by the core. Rarely `width * bpp`.
    pub pitch: usize,
    pub format: PixelFormat,
}

/// A mid-frame geometry change (`RETRO_ENVIRONMENT_SET_GEOMETRY`).
#[derive(Debug, Clone, Copy)]
pub(crate) struct GeometryChange {
    pub width: u32,
    pub height: u32,
    pub aspect_ratio: f32,
}

#[derive(Default)]
struct FrameExchange {
    /// True only between `begin_frame` and `end_frame`.
    active: bool,
    video_ptr: u32,
    video_len: u32,
    audio_ptr: u32,
    audio_len: u32,
    input: Option<InputSnapshot>,
    // --- filled by the core during `retro_run` ---
    video: Option<VideoArrival>,
    audio_frames: u32,
    geometry: Option<GeometryChange>,
    // --- diagnostics ---
    oversized_frames: u64,
    audio_overflow: u64,
}

thread_local! {
    static EXCHANGE: RefCell<FrameExchange> = RefCell::new(FrameExchange::default());
}

/// Publishes staging buffers and the frame's input, then opens the window in which
/// the core may call back.
pub(crate) fn begin_frame(
    video_ptr: u32,
    video_len: u32,
    audio_ptr: u32,
    audio_len: u32,
    input: InputSnapshot,
) {
    EXCHANGE.with(|cell| {
        let mut exchange = cell.borrow_mut();
        exchange.active = true;
        exchange.video_ptr = video_ptr;
        exchange.video_len = video_len;
        exchange.audio_ptr = audio_ptr;
        exchange.audio_len = audio_len;
        exchange.input = Some(input);
        exchange.video = None;
        exchange.audio_frames = 0;
        exchange.geometry = None;
    });
}

/// Closes the window and reports what the core produced.
pub(crate) fn end_frame() -> (Option<VideoArrival>, u32, Option<GeometryChange>) {
    EXCHANGE.with(|cell| {
        let mut exchange = cell.borrow_mut();
        exchange.active = false;
        exchange.video_ptr = 0;
        exchange.video_len = 0;
        exchange.audio_ptr = 0;
        exchange.audio_len = 0;
        exchange.input = None;
        (
            exchange.video.take(),
            exchange.audio_frames,
            exchange.geometry.take(),
        )
    })
}

/// Frames the host had to drop because they exceeded the staging buffer.
pub(crate) fn oversized_frames() -> u64 {
    EXCHANGE.with(|cell| cell.borrow().oversized_frames)
}

/// Audio batches truncated because they exceeded the staging buffer.
pub(crate) fn audio_overflows() -> u64 {
    EXCHANGE.with(|cell| cell.borrow().audio_overflow)
}

/// Static entry points the JS core runtime calls during `retro_run`.
///
/// Exported as `CoreHost` with static methods, deliberately: an instance method
/// would need a borrow of engine state that the running tick already holds.
#[wasm_bindgen]
pub struct CoreHost;

#[wasm_bindgen]
impl CoreHost {
    /// Address of the video staging buffer in wasm memory, or 0 outside a frame.
    /// The host writes the core's framebuffer here.
    #[wasm_bindgen(js_name = videoStagingPtr)]
    pub fn video_staging_ptr() -> u32 {
        EXCHANGE.with(|cell| {
            let exchange = cell.borrow();
            if exchange.active {
                exchange.video_ptr
            } else {
                0
            }
        })
    }

    /// Capacity of the video staging buffer in bytes. The host must not exceed it.
    #[wasm_bindgen(js_name = videoStagingLen)]
    pub fn video_staging_len() -> u32 {
        EXCHANGE.with(|cell| cell.borrow().video_len)
    }

    #[wasm_bindgen(js_name = audioStagingPtr)]
    pub fn audio_staging_ptr() -> u32 {
        EXCHANGE.with(|cell| {
            let exchange = cell.borrow();
            if exchange.active {
                exchange.audio_ptr
            } else {
                0
            }
        })
    }

    /// Capacity of the audio staging buffer, in `i16` samples (not frames).
    #[wasm_bindgen(js_name = audioStagingLen)]
    pub fn audio_staging_len() -> u32 {
        EXCHANGE.with(|cell| cell.borrow().audio_len)
    }

    /// Reports a frame written into the video staging buffer.
    ///
    /// `format` uses libretro's numbering (1 = XRGB8888, 2 = RGB565); anything else
    /// is refused rather than guessed at.
    #[wasm_bindgen(js_name = videoReady)]
    pub fn video_ready(width: u32, height: u32, pitch: u32, format: u32) {
        let Some(format) = PixelFormat::from_libretro(format) else {
            log::warn!("core reported unsupported libretro pixel format {format}");
            return;
        };
        EXCHANGE.with(|cell| {
            let mut exchange = cell.borrow_mut();
            if !exchange.active {
                // A core calling video_refresh outside retro_run would be a core bug;
                // dropping the frame is safer than writing into a stale pointer.
                log::warn!("videoReady outside a frame window; dropping");
                return;
            }
            let needed = pitch as u64 * height as u64;
            if needed > exchange.video_len as u64 {
                exchange.oversized_frames += 1;
                log::warn!(
                    "frame needs {needed} bytes but staging holds {}; dropping",
                    exchange.video_len
                );
                return;
            }
            exchange.video = Some(VideoArrival {
                width,
                height,
                pitch: pitch as usize,
                format,
            });
        });
    }

    /// Reports `frames` stereo frames written into the audio staging buffer.
    /// Accumulates, because a core may call the batch callback several times per
    /// `retro_run`.
    #[wasm_bindgen(js_name = audioReady)]
    pub fn audio_ready(frames: u32) {
        EXCHANGE.with(|cell| {
            let mut exchange = cell.borrow_mut();
            if !exchange.active {
                return;
            }
            let total_samples = (exchange.audio_frames + frames) as u64 * 2;
            if total_samples > exchange.audio_len as u64 {
                exchange.audio_overflow += 1;
                return;
            }
            exchange.audio_frames += frames;
        });
    }

    /// Answers the core's `input_state` callback for the frame's frozen input.
    #[wasm_bindgen(js_name = inputState)]
    pub fn input_state(port: u32, device: u32, index: u32, id: u32) -> i16 {
        EXCHANGE.with(|cell| {
            let exchange = cell.borrow();
            match exchange.input.as_ref() {
                Some(input) => input.libretro_state(port, device, index, id),
                // Outside a frame the answer is "nothing pressed", not an error: some
                // cores poll input during load.
                None => 0,
            }
        })
    }

    /// Reports `RETRO_ENVIRONMENT_SET_GEOMETRY`, applied after the frame completes.
    #[wasm_bindgen(js_name = geometryChanged)]
    pub fn geometry_changed(width: u32, height: u32, aspect_ratio: f32) {
        EXCHANGE.with(|cell| {
            cell.borrow_mut().geometry = Some(GeometryChange {
                width,
                height,
                aspect_ratio,
            });
        });
    }

    /// Number of audio samples the staging buffer can hold, for the host to size its
    /// copies. Exposed separately from `audioStagingLen` so a host can query it
    /// before a frame starts.
    #[wasm_bindgen(js_name = isFrameActive)]
    pub fn is_frame_active() -> bool {
        EXCHANGE.with(|cell| cell.borrow().active)
    }
}
