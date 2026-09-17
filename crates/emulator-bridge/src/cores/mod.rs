//! Core abstraction and the dynamic-loading registry.
//!
//! Architectural rule: **no core is loaded at boot.** The registry is populated
//! at startup with *declarations* only — cheap metadata describing what could be
//! loaded (a few hundred bytes each). The binary module behind a declaration is
//! fetched by the host and handed to [`CoreRegistry::attach_module`] the first
//! time a game using it is launched.
//!
//! The [`EmulatorCore`] trait is the seam the libretro FFI will implement in
//! Phase 1b. Nothing above this trait knows a core is C code, a wasm module, or
//! a natively linked archive — which is what lets the same engine drive a
//! browser and an iOS app.

mod diagnostic;
mod registry;

#[cfg(target_arch = "wasm32")]
pub(crate) mod host;
#[cfg(target_arch = "wasm32")]
mod wasm_core;

pub use diagnostic::DiagnosticCore;
pub use registry::{CoreRegistry, CoreState};

#[cfg(target_arch = "wasm32")]
pub use host::CoreHost;
#[cfg(target_arch = "wasm32")]
pub use wasm_core::{LibretroRuntimeHandle, WasmCore};

/// What the frontend knows about the content being loaded.
///
/// Real libretro cores need more than bytes. Modern cores declare
/// `need_fullpath = true` globally and then *override* it per file extension, and
/// they discover which override applies by asking the frontend for extended game
/// info — which includes the extension and a display name. Without this, fceumm
/// rejects a perfectly valid `.nes` file.
#[derive(Debug, Clone, Default)]
pub struct ContentHint {
    /// Lower-case, no leading dot, e.g. `"nes"`.
    pub extension: String,
    /// Display name, usually the file stem. Cores may use it for save file naming.
    pub name: String,
}

impl ContentHint {
    pub fn new(extension: impl Into<String>, name: impl Into<String>) -> Self {
        let extension = extension.into();
        Self {
            extension: extension.trim_start_matches('.').to_ascii_lowercase(),
            name: name.into(),
        }
    }

    /// Derives the hint from a file name.
    pub fn from_filename(filename: &str) -> Self {
        let name = filename.rsplit('/').next().unwrap_or(filename);
        match name.rsplit_once('.') {
            Some((stem, extension)) => Self::new(extension, stem),
            None => Self::new("", name),
        }
    }
}

use crate::audio::AudioSink;
use crate::error::BridgeError;
use crate::frame::{FrameGeometry, FrameView, PixelFormat};
use crate::input::InputSnapshot;

/// Everything the engine needs to know about a core before it exists in memory.
///
/// Declared up front by the host from `cores/manifest.json`; also returned by a
/// loaded core so the engine can detect a mismatch between manifest and reality.
#[derive(Debug, Clone)]
pub struct CoreDescriptor {
    /// Stable identifier, e.g. `"nestopia"`. Used as the registry key.
    pub id: String,
    pub display_name: String,
    /// System ids this core can run, e.g. `["nes", "fds"]`.
    pub systems: Vec<String>,
    pub geometry: FrameGeometry,
    /// Native refresh rate. Drives [`crate::timing::FramePacer`].
    pub target_fps: f64,
    pub audio_sample_rate: u32,
    pub pixel_format: PixelFormat,
    /// Relative URL / bundle path of the module. Fetched lazily, never at boot.
    pub module_url: String,
}

/// The one seam between the engine and an actual emulator.
///
/// Deliberately synchronous and single-threaded: `run_frame` is called from the
/// unified tick, so anything that blocks here stalls the frame. Implementations
/// own their framebuffer and audio scratch space to keep the tick allocation-free.
pub trait EmulatorCore {
    fn descriptor(&self) -> &CoreDescriptor;

    /// Hands ROM/disc content to the core (`retro_load_game`).
    ///
    /// `hint` carries the file extension and name, which real cores require to
    /// resolve their content-info overrides.
    fn load_content(&mut self, content: &[u8], hint: &ContentHint) -> Result<(), BridgeError>;

    /// Advances emulation by exactly one frame (`retro_run`).
    fn run_frame(&mut self, input: &InputSnapshot) -> Result<(), BridgeError>;

    /// Video produced by the last `run_frame`. `None` means the core duped the
    /// previous frame, which is legal and means "present the last texture again".
    fn video(&self) -> Option<FrameView<'_>>;

    /// Pushes the audio produced by the last `run_frame` into the sink.
    ///
    /// Cores write straight into the sink rather than returning a buffer, so there
    /// is no intermediate allocation per frame. Libretro's `audio_sample_batch`
    /// callback forwards its `i16` batch here verbatim.
    fn drain_audio(&mut self, sink: &mut dyn AudioSink);

    fn reset(&mut self) -> Result<(), BridgeError>;

    /// Save-state size in bytes; `0` means the core has no state support.
    fn state_size(&self) -> usize {
        0
    }

    fn save_state(&self, _dst: &mut [u8]) -> Result<usize, BridgeError> {
        Err(BridgeError::NotImplemented("save_state"))
    }

    fn load_state(&mut self, _src: &[u8]) -> Result<(), BridgeError> {
        Err(BridgeError::NotImplemented("load_state"))
    }

    /// Frames emulated since load. Used for the HUD and state metadata.
    fn frame_count(&self) -> u64;

    /// Bytes of memory this core holds, if it can be measured.
    ///
    /// `None` for cores whose footprint is not separable from the engine's (the
    /// diagnostic stand-in lives in the engine's own memory). A real core reports its
    /// wasm module's linear memory plus the staging buffers dedicated to it, which is
    /// what a multi-core memory budget actually needs to track.
    fn memory_bytes(&self) -> Option<u64> {
        None
    }
}

/// Verifies a fetched module looks like a WebAssembly binary before instantiating.
///
/// Cheap guard against a CDN 404 page or a truncated download being handed to the
/// instantiator, where the failure would be far less legible.
pub(crate) fn validate_wasm_module(core_id: &str, bytes: &[u8]) -> Result<(), BridgeError> {
    const WASM_MAGIC: &[u8; 4] = b"\0asm";
    if bytes.len() < 8 {
        return Err(BridgeError::InvalidCoreModule {
            core_id: core_id.to_string(),
            reason: format!("module is only {} bytes", bytes.len()),
        });
    }
    if &bytes[0..4] != WASM_MAGIC {
        return Err(BridgeError::InvalidCoreModule {
            core_id: core_id.to_string(),
            reason: "missing wasm magic header (fetched an HTML error page?)".into(),
        });
    }
    let version = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    if version != 1 {
        return Err(BridgeError::InvalidCoreModule {
            core_id: core_id.to_string(),
            reason: format!("unsupported wasm binary version {version}"),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_minimal_module() {
        assert!(validate_wasm_module("x", b"\0asm\x01\0\0\0").is_ok());
    }

    #[test]
    fn rejects_html_error_page() {
        let err = validate_wasm_module("x", b"<!DOCTYPE html><html>404").unwrap_err();
        assert!(matches!(err, BridgeError::InvalidCoreModule { .. }));
    }

    #[test]
    fn rejects_truncated_download() {
        assert!(validate_wasm_module("x", b"\0asm").is_err());
    }
}
