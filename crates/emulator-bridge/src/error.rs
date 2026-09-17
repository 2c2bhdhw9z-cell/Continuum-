//! Error surface for the bridge.
//!
//! Every fallible bridge operation funnels through [`BridgeError`]. Both facades
//! (wasm-bindgen in Phase 1, UniFFI in Phase 2) convert this single type, so
//! error semantics never diverge between the web and native front ends.

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

    #[error("not implemented yet: {0}")]
    NotImplemented(&'static str),
}

#[derive(Debug, Error)]
pub enum GfxError {
    #[error("this browser does not expose a WebGPU adapter (navigator.gpu missing or blocked)")]
    NoAdapter,

    #[error("failed to create a WebGPU surface for the canvas: {0}")]
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
