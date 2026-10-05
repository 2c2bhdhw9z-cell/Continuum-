//! Error surface for the bridge.
//!
//! Every fallible bridge operation funnels through [`BridgeError`]. The UniFFI facade
//! (`uniffi_api.rs`) converts this single type into the error Swift sees, so the app reports
//! the same failure the engine raised.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum BridgeError {
    #[error("graphics: {0}")]
    Gfx(#[from] GfxError),

    #[error("no renderer attached; call init_gpu() before launching a session")]
    NoRenderer,

    #[error("no active session")]
    NoSession,

    #[error("core '{0}' has not been declared in the registry")]
    UnknownCore(String),

    #[error("core '{core_id}' is not loaded (state: {state})")]
    CoreNotLoaded {
        core_id: String,
        state: &'static str,
    },

    #[error("core '{0}' is already bound to a running session")]
    CoreBusy(String),

    #[error("core module for '{core_id}' is invalid: {reason}")]
    InvalidCoreModule { core_id: String, reason: String },

    #[error("content rejected by core '{core_id}': {reason}")]
    InvalidContent { core_id: String, reason: String },

    #[error("save state error: {0}")]
    SaveState(String),

    #[error("cheat error: {0}")]
    Cheat(String),

    #[error("core option error: {0}")]
    CoreOption(String),

    /// Core memory access: battery saves, RAM search, pokes, achievements.
    #[error("memory error: {0}")]
    Memory(String),

    #[error("achievements: {0}")]
    Achievements(String),

    #[error("not implemented yet: {0}")]
    NotImplemented(&'static str),
}

#[derive(Debug, Error)]
pub enum GfxError {
    #[error("no GPU adapter was found (Metal unavailable)")]
    NoAdapter,

    #[error("failed to create the drawing surface: {0}")]
    SurfaceCreation(String),

    #[error("the adapter cannot present to this surface")]
    SurfaceIncompatible,

    #[error("device request failed: {0}")]
    DeviceRequest(String),

    #[error("surface lost and could not be reconfigured")]
    SurfaceLost,

    #[error("invalid frame: {0}")]
    InvalidFrame(String),
}
