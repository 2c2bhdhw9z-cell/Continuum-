//! # emulator-bridge
//!
//! The manager/engine layer of the emulator project: everything between the UI and
//! the emulator cores.
//!
//! ## Why this crate exists
//!
//! Phase 1 ships a PWA; Phase 2 wraps the same engine in a native iOS app. If the
//! UI owned the loop, the timing, the input mapping or the audio buffering, all of
//! it would be rewritten in Swift — and the two would drift. So this crate owns all
//! of it, and each platform contributes only a facade:
//!
//! ```text
//!   Phase 1: HTML/JS UI ──▶ wasm.rs (wasm-bindgen) ──┐
//!                                                     ├──▶ EmulatorBridge ──▶ EmulatorCore
//!   Phase 2: SwiftUI    ──▶ uniffi facade (planned) ──┘        │
//!                                                              └──▶ gfx::Renderer (wgpu)
//!                                                                     WebGPU | Metal
//! ```
//!
//! ## Module map
//!
//! - [`bridge`] — the engine: session lifecycle and the unified tick.
//! - [`cores`] — the [`cores::EmulatorCore`] seam plus the lazy-loading registry.
//! - [`gfx`] — `wgpu` renderer. The only thing that draws emulator output.
//! - [`audio`] — [`audio::AudioSink`], ring buffer, resampler.
//! - [`input`] — port/button state and per-tick snapshots.
//! - [`timing`] — frame pacing between display refresh and core refresh.
//! - [`frame`] — geometry and pixel formats.
//! - `wasm` — the Phase 1 wasm-bindgen facade (wasm32 only).
//!
//! ## Invariants
//!
//! 1. Cores are loaded only via [`bridge::EmulatorBridge::attach_core_module`],
//!    which the UI calls from the launch path — never at boot.
//! 2. Video is presented only through [`gfx::Renderer`]; no 2D canvas or DOM path
//!    exists in this crate or the UI.
//! 3. [`bridge::EmulatorBridge::tick`] is the only driver. No timers, no threads,
//!    no workers.
//! 4. The steady-state tick does not allocate: framebuffers, audio rings and
//!    conversion scratch are sized when a session starts.

pub mod audio;
pub mod bridge;
pub mod cores;
pub mod error;
pub mod frame;
pub mod gfx;
pub mod input;
pub mod rewind;
pub mod timing;

/// `Send` on native targets, and nothing on wasm.
///
/// The engine holds `Box<dyn EmulatorCore>` and `Box<dyn AudioSink>`. Natively those must
/// be `Send`, because the Swift layer holds the bridge behind a `Mutex` and UniFFI requires
/// an exported object to be `Send + Sync`. On wasm they cannot be: `WasmCore` holds a
/// `LibretroRuntimeHandle`, which is a `JsValue`, and `JsValue` is deliberately not `Send`
/// because a JS value belongs to one agent.
///
/// So the bound is conditional rather than absent, and expressed once here rather than by
/// duplicating two trait definitions per target.
#[cfg(target_arch = "wasm32")]
pub trait MaybeSend {}
#[cfg(target_arch = "wasm32")]
impl<T: ?Sized> MaybeSend for T {}

#[cfg(not(target_arch = "wasm32"))]
pub trait MaybeSend: Send {}
#[cfg(not(target_arch = "wasm32"))]
impl<T: ?Sized + Send> MaybeSend for T {}

// Phase 5: the Swift-facing facade. Feature-gated so neither the web build nor the host
// test suite acquires a uniffi dependency for a module they never use.
//
// `setup_scaffolding!` has to be at the crate root rather than beside the exported types:
// it defines the `UniFfiTag` the derive macros reference, and that name is resolved from
// the crate root regardless of where the deriving type lives.
#[cfg(all(not(target_arch = "wasm32"), feature = "uniffi-bindings"))]
uniffi::setup_scaffolding!();

#[cfg(all(not(target_arch = "wasm32"), feature = "uniffi-bindings"))]
pub mod uniffi_api;

#[cfg(target_arch = "wasm32")]
pub mod wasm;

pub use bridge::{BridgeStatus, EmulatorBridge, TickReport};
pub use error::{BridgeError, GfxError};
