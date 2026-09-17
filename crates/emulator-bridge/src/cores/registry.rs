//! The core registry: declarations at boot, binaries on demand.
//!
//! Enforces the dynamic-loading rule structurally. A [`CoreDescriptor`] can be
//! declared for every system the project will ever support without loading a
//! single byte of emulator code; [`CoreRegistry::attach_module`] is the only way a
//! core becomes runnable, and it is called from the launch path alone.

use std::collections::BTreeMap;

use super::{validate_wasm_module, CoreDescriptor, DiagnosticCore, EmulatorCore};
use crate::error::BridgeError;

/// Lifecycle of one registry slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreState {
    /// Metadata only. Nothing has been fetched — the boot state for every core.
    Declared,
    /// Module fetched and instantiated, sitting idle and ready to launch.
    Loaded,
    /// Moved into the active session. Cannot be launched again concurrently.
    Bound,
    /// Load failed; the reason is available via [`CoreRegistry::failure_reason`].
    Failed,
}

impl CoreState {
    pub const fn as_str(self) -> &'static str {
        match self {
            CoreState::Declared => "declared",
            CoreState::Loaded => "loaded",
            CoreState::Bound => "bound",
            CoreState::Failed => "failed",
        }
    }
}

enum Slot {
    Declared,
    Loaded(Box<dyn EmulatorCore>),
    Bound,
    Failed(String),
}

impl Slot {
    fn state(&self) -> CoreState {
        match self {
            Slot::Declared => CoreState::Declared,
            Slot::Loaded(_) => CoreState::Loaded,
            Slot::Bound => CoreState::Bound,
            Slot::Failed(_) => CoreState::Failed,
        }
    }
}

struct Entry {
    descriptor: CoreDescriptor,
    slot: Slot,
    /// Size of the last module handed to `attach_module`, for the debug HUD.
    module_bytes: usize,
}

#[derive(Default)]
pub struct CoreRegistry {
    /// `BTreeMap` for deterministic ordering — the UI lists cores and stable
    /// ordering keeps that list from reshuffling between loads.
    entries: BTreeMap<String, Entry>,
}

impl CoreRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers (or re-registers) a core's metadata. Cheap: no I/O, no code.
    ///
    /// Re-declaring a core that is currently loaded keeps the loaded instance and
    /// only refreshes metadata, so a manifest refresh cannot yank a running game.
    pub fn declare(&mut self, descriptor: CoreDescriptor) {
        let id = descriptor.id.clone();
        match self.entries.get_mut(&id) {
            Some(existing) => existing.descriptor = descriptor,
            None => {
                self.entries.insert(
                    id,
                    Entry {
                        descriptor,
                        slot: Slot::Declared,
                        module_bytes: 0,
                    },
                );
            }
        }
    }

    pub fn is_declared(&self, core_id: &str) -> bool {
        self.entries.contains_key(core_id)
    }

    pub fn descriptor(&self, core_id: &str) -> Option<&CoreDescriptor> {
        self.entries.get(core_id).map(|e| &e.descriptor)
    }

    pub fn state(&self, core_id: &str) -> Option<CoreState> {
        self.entries.get(core_id).map(|e| e.slot.state())
    }

    pub fn failure_reason(&self, core_id: &str) -> Option<&str> {
        match self.entries.get(core_id).map(|e| &e.slot) {
            Some(Slot::Failed(reason)) => Some(reason.as_str()),
            _ => None,
        }
    }

    pub fn module_bytes(&self, core_id: &str) -> usize {
        self.entries.get(core_id).map_or(0, |e| e.module_bytes)
    }

    pub fn declared_ids(&self) -> impl Iterator<Item = &str> {
        self.entries.keys().map(String::as_str)
    }

    /// Cores currently holding an instantiated module — i.e. actual memory in use.
    pub fn resident_ids(&self) -> impl Iterator<Item = &str> {
        self.entries
            .iter()
            .filter(|(_, e)| matches!(e.slot, Slot::Loaded(_) | Slot::Bound))
            .map(|(k, _)| k.as_str())
    }

    /// Finds a declared core able to run `system_id`.
    pub fn core_for_system(&self, system_id: &str) -> Option<&CoreDescriptor> {
        self.entries
            .values()
            .find(|e| e.descriptor.systems.iter().any(|s| s == system_id))
            .map(|e| &e.descriptor)
    }

    /// Instantiates a fetched core module.
    ///
    /// The *only* transition from declared to runnable. Called from the game-launch
    /// path after the host has fetched the bytes.
    pub fn attach_module(&mut self, core_id: &str, bytes: &[u8]) -> Result<(), BridgeError> {
        let entry = self
            .entries
            .get_mut(core_id)
            .ok_or_else(|| BridgeError::UnknownCore(core_id.to_string()))?;

        if matches!(entry.slot, Slot::Bound) {
            return Err(BridgeError::CoreBusy(core_id.to_string()));
        }

        if let Err(err) = validate_wasm_module(core_id, bytes) {
            entry.slot = Slot::Failed(err.to_string());
            return Err(err);
        }

        match instantiate(&entry.descriptor, bytes) {
            Ok(core) => {
                entry.module_bytes = bytes.len();
                entry.slot = Slot::Loaded(core);
                log::info!("core '{}' instantiated from {} bytes", core_id, bytes.len());
                Ok(())
            }
            Err(err) => {
                entry.slot = Slot::Failed(err.to_string());
                Err(err)
            }
        }
    }

    /// Moves a loaded core out of the registry and into a session.
    pub fn take_for_session(
        &mut self,
        core_id: &str,
    ) -> Result<Box<dyn EmulatorCore>, BridgeError> {
        let entry = self
            .entries
            .get_mut(core_id)
            .ok_or_else(|| BridgeError::UnknownCore(core_id.to_string()))?;

        match core::mem::replace(&mut entry.slot, Slot::Bound) {
            Slot::Loaded(core) => Ok(core),
            other => {
                let state = other.state();
                // Restore: taking failed, so the slot must not be left as Bound.
                entry.slot = other;
                if state == CoreState::Bound {
                    Err(BridgeError::CoreBusy(core_id.to_string()))
                } else {
                    Err(BridgeError::CoreNotLoaded {
                        core_id: core_id.to_string(),
                        state: state.as_str(),
                    })
                }
            }
        }
    }

    /// Returns a core when its session ends, keeping it warm for a quick relaunch.
    pub fn return_from_session(&mut self, core_id: &str, core: Box<dyn EmulatorCore>) {
        if let Some(entry) = self.entries.get_mut(core_id) {
            entry.slot = Slot::Loaded(core);
        }
    }

    /// Frees a core's memory, dropping back to declared. The registry keeps the
    /// declaration so the same core can be re-fetched later.
    pub fn unload(&mut self, core_id: &str) -> Result<(), BridgeError> {
        let entry = self
            .entries
            .get_mut(core_id)
            .ok_or_else(|| BridgeError::UnknownCore(core_id.to_string()))?;
        if matches!(entry.slot, Slot::Bound) {
            return Err(BridgeError::CoreBusy(core_id.to_string()));
        }
        entry.slot = Slot::Declared;
        entry.module_bytes = 0;
        Ok(())
    }
}

/// Turns module bytes into a live core.
///
/// **Phase 1 placeholder.** Returns a [`DiagnosticCore`] carrying the declared
/// descriptor, so the launch path, tick loop, renderer, audio graph and UI can all
/// be exercised and profiled before any emulator exists.
///
/// TODO(phase1b): instantiate the real libretro core here —
/// `retro_set_environment` → `retro_init` → `retro_get_system_av_info`, then wrap
/// the instance in an `EmulatorCore` impl. On wasm the module is instantiated by
/// the host and passed in as an imports object; natively it is dlopen'd or
/// statically linked. Everything above this function is already agnostic to which.
fn instantiate(
    descriptor: &CoreDescriptor,
    _bytes: &[u8],
) -> Result<Box<dyn EmulatorCore>, BridgeError> {
    Ok(Box::new(DiagnosticCore::new(descriptor.clone())))
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
            pixel_format: PixelFormat::Xrgb8888,
            module_url: format!("/cores/{id}.wasm"),
        }
    }

    #[test]
    fn declaration_loads_no_code() {
        let mut reg = CoreRegistry::new();
        reg.declare(descriptor("nestopia", "nes"));
        assert_eq!(reg.state("nestopia"), Some(CoreState::Declared));
        assert_eq!(reg.resident_ids().count(), 0);
        assert_eq!(reg.module_bytes("nestopia"), 0);
    }

    #[test]
    fn attach_then_take_then_return() {
        let mut reg = CoreRegistry::new();
        reg.declare(descriptor("nestopia", "nes"));
        reg.attach_module("nestopia", MODULE).unwrap();
        assert_eq!(reg.state("nestopia"), Some(CoreState::Loaded));

        let core = reg.take_for_session("nestopia").unwrap();
        assert_eq!(reg.state("nestopia"), Some(CoreState::Bound));
        assert!(reg.take_for_session("nestopia").is_err());

        reg.return_from_session("nestopia", core);
        assert_eq!(reg.state("nestopia"), Some(CoreState::Loaded));
    }

    #[test]
    fn taking_a_declared_core_reports_state_not_busy() {
        let mut reg = CoreRegistry::new();
        reg.declare(descriptor("mgba", "gba"));
        // `unwrap_err` is unavailable here: the Ok type is a trait object, which
        // cannot implement Debug.
        let err = match reg.take_for_session("mgba") {
            Err(err) => err,
            Ok(_) => panic!("a declared core must not be takeable"),
        };
        match err {
            BridgeError::CoreNotLoaded { state, .. } => assert_eq!(state, "declared"),
            other => panic!("unexpected error: {other}"),
        }
        // The failed take must not have left the slot bound.
        assert_eq!(reg.state("mgba"), Some(CoreState::Declared));
    }

    #[test]
    fn bad_module_marks_slot_failed() {
        let mut reg = CoreRegistry::new();
        reg.declare(descriptor("mednafen_psx", "ps1"));
        assert!(reg
            .attach_module("mednafen_psx", b"<html>404</html>")
            .is_err());
        assert_eq!(reg.state("mednafen_psx"), Some(CoreState::Failed));
        assert!(reg.failure_reason("mednafen_psx").is_some());
    }

    #[test]
    fn unknown_core_is_rejected() {
        let mut reg = CoreRegistry::new();
        assert!(matches!(
            reg.attach_module("ghost", MODULE),
            Err(BridgeError::UnknownCore(_))
        ));
    }

    #[test]
    fn system_lookup_finds_declared_core() {
        let mut reg = CoreRegistry::new();
        reg.declare(descriptor("snes9x", "snes"));
        assert_eq!(
            reg.core_for_system("snes").map(|d| d.id.as_str()),
            Some("snes9x")
        );
        assert!(reg.core_for_system("n64").is_none());
    }

    #[test]
    fn unload_frees_memory_but_keeps_declaration() {
        let mut reg = CoreRegistry::new();
        reg.declare(descriptor("snes9x", "snes"));
        reg.attach_module("snes9x", MODULE).unwrap();
        reg.unload("snes9x").unwrap();
        assert_eq!(reg.state("snes9x"), Some(CoreState::Declared));
        assert!(reg.is_declared("snes9x"));
    }

    #[test]
    fn cannot_unload_a_bound_core() {
        let mut reg = CoreRegistry::new();
        reg.declare(descriptor("snes9x", "snes"));
        reg.attach_module("snes9x", MODULE).unwrap();
        let _core = reg.take_for_session("snes9x").unwrap();
        assert!(matches!(
            reg.unload("snes9x"),
            Err(BridgeError::CoreBusy(_))
        ));
    }
}
