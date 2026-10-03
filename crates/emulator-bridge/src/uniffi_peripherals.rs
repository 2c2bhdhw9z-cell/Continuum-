//! The Swift-facing half of `peripherals`: microphone, camera and Amiibo.
//!
//! A second `#[uniffi::export] impl ContinuumEngine` block rather than more methods in
//! uniffi_api.rs, so it merges cleanly beside other work on that file. UniFFI allows any number of
//! export blocks for one object.
//!
//! THE MICROPHONE AND CAMERA METHODS NEVER TAKE THE ENGINE LOCK. They are called from
//! AVAudioEngine's tap thread and AVCaptureSession's queue, while the display link holds that lock
//! for the whole of every tick; a capture thread waiting on it would drop audio and frames every
//! frame. Everything they touch is in `peripherals`, behind atomics and capture-only mutexes.
//! Only the Amiibo methods read the running core id, which does take the lock, and they are
//! called from a menu tap on the main thread.

use crate::peripherals::{self, amiibo, camera, mic};
use crate::uniffi_api::ContinuumEngine;

/// What Swift should have running. Polled once per display-link frame; atomic reads only.
#[derive(Debug, Clone, uniffi::Record)]
pub struct PeripheralRequests {
    /// Run the AVAudioEngine input tap and use the `.playAndRecord` session category.
    pub microphone_wanted: bool,
    /// The rate the core reads at. Informational: Rust resamples whatever rate is pushed.
    pub microphone_rate: u32,
    /// Run the AVCaptureSession.
    pub camera_wanted: bool,
    /// The size the core asked for. Informational: Rust scales whatever size is pushed.
    pub camera_width: u32,
    pub camera_height: u32,
}

/// One Amiibo file, checked.
#[derive(Debug, Clone, uniffi::Record)]
pub struct AmiiboInspection {
    /// False means the file is not a usable Amiibo dump; `message` says why.
    pub ok: bool,
    /// A sentence for the screen either way.
    pub message: String,
    pub uid_hex: String,
    pub amiibo_id_hex: String,
    pub warnings: Vec<String>,
}

/// Whether the running core can be handed an Amiibo.
#[derive(Debug, Clone, uniffi::Record)]
pub struct AmiiboSupport {
    pub supported: bool,
    pub reason: String,
}

#[uniffi::export]
impl ContinuumEngine {
    /// The Settings switch "Allow microphone in games". Off by default.
    pub fn set_microphone_allowed(&self, allowed: bool) {
        mic::set_allowed(allowed);
    }

    /// Swift tells the engine its input tap is running (true) or has stopped (false). Until it
    /// is running, a core reading the mic gets silence rather than an error.
    pub fn set_microphone_capturing(&self, capturing: bool) {
        mic::hub().set_capturing(capturing);
    }

    /// Mono float samples from the input tap, at the tap's own rate. Resampled in Rust to the
    /// rate the core asked for and queued for `read_mic`. Returns how many samples were queued,
    /// which is 0 whenever the core is not listening. Never takes the engine lock.
    pub fn push_microphone_samples(&self, samples: Vec<f32>, sample_rate: u32) -> u32 {
        mic::hub().push_float(&samples, sample_rate) as u32
    }

    /// The Settings switch "Allow camera in games", ANDed in Swift with iOS camera permission.
    pub fn set_camera_allowed(&self, allowed: bool) {
        camera::hub().set_allowed(allowed);
    }

    /// One BGRA camera frame (`kCVPixelFormatType_32BGRA`) of any size. Scaled in Rust to the
    /// size the core asked for and handed to the core on its own thread before its next frame.
    /// False when the core is not running the camera or the geometry does not fit the buffer.
    /// Never takes the engine lock.
    pub fn push_camera_frame(
        &self,
        bgra: Vec<u8>,
        width: u32,
        height: u32,
        stride: u32,
        mirror: bool,
    ) -> bool {
        camera::hub().push_bgra(&bgra, width, height, stride, mirror)
    }

    /// What the running core currently wants from the phone. Atomic reads only.
    pub fn peripheral_requests(&self) -> PeripheralRequests {
        let requests = peripherals::requests();
        PeripheralRequests {
            microphone_wanted: requests.microphone_wanted,
            microphone_rate: requests.microphone_rate,
            camera_wanted: requests.camera_wanted,
            camera_width: requests.camera_width,
            camera_height: requests.camera_height,
        }
    }

    /// The microphone and camera HUD line. Never empty.
    pub fn peripheral_status(&self) -> String {
        peripherals::status_line()
    }

    /// Checks an Amiibo dump before it is imported.
    pub fn inspect_amiibo(&self, data: Vec<u8>) -> AmiiboInspection {
        match amiibo::inspect(&data) {
            Ok(info) => AmiiboInspection {
                ok: true,
                message: if info.warnings.is_empty() {
                    format!("Amiibo {} ({} bytes)", info.amiibo_id_hex, info.size)
                } else {
                    format!(
                        "Amiibo {} ({} bytes), with warnings: {}",
                        info.amiibo_id_hex,
                        info.size,
                        info.warnings.join("; ")
                    )
                },
                uid_hex: info.uid_hex,
                amiibo_id_hex: info.amiibo_id_hex,
                warnings: info.warnings,
            },
            Err(reason) => AmiiboInspection {
                ok: false,
                message: reason,
                uid_hex: String::new(),
                amiibo_id_hex: String::new(),
                warnings: Vec::new(),
            },
        }
    }

    /// Whether the running core can receive an Amiibo, with the reason in words.
    pub fn amiibo_support(&self) -> AmiiboSupport {
        let core_id = self.current_core_id().unwrap_or_default();
        let (supported, reason) = amiibo::support_for_core(&core_id);
        AmiiboSupport { supported, reason }
    }

    /// "Taps" an Amiibo on the running game. Returns the status line to show, always.
    pub fn tap_amiibo(&self, file_name: String, data: Vec<u8>) -> String {
        let core_id = self.current_core_id().unwrap_or_default();
        let line = amiibo::tap(&core_id, &file_name, &data);
        log::info!("{line}");
        line
    }
}
