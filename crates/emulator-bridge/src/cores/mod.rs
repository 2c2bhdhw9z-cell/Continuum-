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

// A libretro core from a shared library, which is now the only way a real core loads.
// Feature-gated rather than unconditional so the host test suite does not acquire a
// `libloading` dependency for a module it never exercises.
#[cfg(feature = "native-core")]
pub mod native_core;

pub use diagnostic::DiagnosticCore;
pub use registry::{CoreRegistry, CoreState};

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
    /// Full, openable filesystem path when the caller has one.
    ///
    /// `None` for the web build, which loads from memory and never has a real path, and
    /// for callers that only know a bare filename. A `need_fullpath` native core (PCSX
    /// ReARMed, the Switch containers) hard-requires `retro_game_info::path` to be a real
    /// openable path — `name` alone is only the file stem, which is not one — so the
    /// native loader reads this when the content bytes are empty. Populated by
    /// [`ContentHint::from_filename`] when the input already looks like a path.
    pub full_path: Option<String>,
}

impl ContentHint {
    pub fn new(extension: impl Into<String>, name: impl Into<String>) -> Self {
        let extension = extension.into();
        Self {
            extension: extension.trim_start_matches('.').to_ascii_lowercase(),
            name: name.into(),
            full_path: None,
        }
    }

    /// Derives the hint from a file name.
    ///
    /// When the input carries a directory separator it is a path, not a bare name, so the
    /// verbatim string is retained in [`ContentHint::full_path`] for `need_fullpath` cores.
    /// The extension/name split is unchanged, so the web build sees identical behaviour.
    pub fn from_filename(filename: &str) -> Self {
        let name = filename.rsplit('/').next().unwrap_or(filename);
        let mut hint = match name.rsplit_once('.') {
            Some((stem, extension)) => Self::new(extension, stem),
            None => Self::new("", name),
        };
        if filename.contains('/') {
            hint.full_path = Some(filename.to_string());
        }
        hint
    }
}

use crate::audio::AudioSink;
use crate::error::BridgeError;
use crate::frame::{FrameGeometry, FrameView, PixelFormat};
use crate::input::InputSnapshot;

/// One core option, as the core itself declared it.
///
/// Cores publish these through `RETRO_ENVIRONMENT_SET_VARIABLES` during
/// `retro_set_environment`, which happens before any content is loaded — so the list is
/// known as soon as a core is instantiated, and a settings UI can be built from it
/// without launching a game.
#[derive(Debug, Clone)]
pub struct CoreOption {
    /// The variable name the core reads, e.g. `"mgba_color_correction"`.
    pub key: String,
    /// Human-readable label from the core's own table.
    pub label: String,
    /// Value currently in force. Empty means "unset", i.e. the core's own default.
    pub value: String,
    /// Permitted values, in the order the core listed them. The first is its default.
    pub values: Vec<String>,
}

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
    /// Which core wins when several can run the same system. **Higher wins.**
    ///
    /// The relationship between a system and a core is one-to-many: `gb` can be run
    /// by mGBA or by a dedicated Game Boy core, and neither answer is wrong. Priority
    /// picks the default without hiding the alternatives, so the UI can offer them and
    /// the user can override per system.
    ///
    /// Ties are broken by core id so the ordering is stable — a list that reshuffles
    /// between loads is worse than one in a slightly arbitrary order.
    pub priority: i32,
}

/// The one seam between the engine and an actual emulator.
///
/// Deliberately synchronous and single-threaded: `run_frame` is called from the
/// unified tick, so anything that blocks here stalls the frame. Implementations
/// own their framebuffer and audio scratch space to keep the tick allocation-free.
pub trait EmulatorCore: crate::MaybeSend {
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

    /// The core's own version string, as it reports it.
    ///
    /// Exists for save-state compatibility rather than for display. A state written by one
    /// build of a core is not safely readable by another, and `retro_unserialize` will not
    /// reliably say so, so the host stores this beside every state and refuses a mismatch.
    /// `None` means the core does not report one, in which case that check cannot be made and
    /// the remaining ones have to carry it.
    fn version(&self) -> Option<&str> {
        None
    }

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

    /// Clears every cheat applied to the core (`retro_cheat_reset`).
    fn reset_cheats(&mut self) -> Result<(), BridgeError> {
        Err(BridgeError::NotImplemented("reset_cheats"))
    }

    /// Applies one cheat at `index` (`retro_cheat_set`).
    ///
    /// Signature is deliberately plain — `&str`, `u32`, `bool` — because this trait
    /// compiles for every target, including the native iOS build where there is no
    /// `JsValue`. The code string is whatever the user typed; validating cheat syntax is
    /// the core's job, and each core's format differs (Game Genie for the NES, raw
    /// address:value for the Mega Drive, and so on).
    fn set_cheat(&mut self, _index: u32, _enabled: bool, _code: &str) -> Result<(), BridgeError> {
        Err(BridgeError::NotImplemented("set_cheat"))
    }

    /// Whether this core can apply cheats at all.
    fn supports_cheats(&self) -> bool {
        false
    }

    /// Options this core declared. Empty when it declared none.
    fn core_options(&self) -> Vec<CoreOption> {
        Vec::new()
    }

    /// Sets one option. The core re-reads it on its next update poll.
    fn set_core_option(&mut self, _key: &str, _value: &str) -> Result<(), BridgeError> {
        Err(BridgeError::NotImplemented("set_core_option"))
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
