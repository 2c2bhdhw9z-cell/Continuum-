//! # emulator-bridge
//!
//! The manager/engine layer of the emulator project: everything between the UI and
//! the emulator cores.
//!
//! ## Why this crate exists
//!
//! The product is a native iOS app. If the UI owned the loop, the timing, the input
//! mapping or the audio buffering, all of it would be written in Swift, and the next
//! platform would have to write it all again. So this crate owns all of it and the
//! platform contributes only a facade:
//!
//! ```text
//!   SwiftUI ──▶ uniffi_api.rs ──▶ EmulatorBridge ──▶ EmulatorCore (dlopen'd libretro)
//!                                       │
//!                                       └──▶ gfx::Renderer (wgpu ──▶ Metal)
//! ```
//!
//! There was a second facade, `wasm.rs`, behind a browser build, and this crate's shape is
//! the reason that cost so little to remove: the engine never knew about it. That is worth
//! knowing because an Android build is planned, and it arrives as a third facade over this
//! same engine rather than as a port of it.
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
//! - [`rewind`] — the bounded tape of save states behind the rewind button.
//! - [`uniffi_api`] — the Swift-facing facade (feature `uniffi-bindings`).
//!
//! ## Invariants
//!
//! 1. Cores are loaded from the launch path and never at boot. A real core enters through
//!    [`bridge::EmulatorBridge::attach_core`], which is what the iOS app uses after
//!    `dlopen`; [`bridge::EmulatorBridge::attach_core_module`] is the other door and now
//!    serves only the built-in diagnostic stand-in.
//! 2. Video is presented only through [`gfx::Renderer`].
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
pub mod jit_probe;
pub mod rewind;
pub mod timing;

/// `Send`, and now unconditionally so.
///
/// The engine holds `Box<dyn EmulatorCore>` and `Box<dyn AudioSink>`, and both must be
/// `Send`, because the Swift layer holds the bridge behind a `Mutex` and UniFFI requires an
/// exported object to be `Send + Sync`.
///
/// This used to be conditional, and the alias survives its condition on purpose. The wasm
/// build could not satisfy `Send`: its core held a `JsValue`, which is deliberately not
/// `Send` because a JS value belongs to one agent. That build is gone, so the bound is now
/// the same everywhere. The name is kept rather than substituted through the code because it
/// is the one place a future target that cannot be `Send` would be expressed again, and
/// collapsing it into a bare `Send` at every use site would scatter that decision across the
/// crate instead of leaving it here.
pub trait MaybeSend: Send {}
impl<T: ?Sized + Send> MaybeSend for T {}

// The Swift-facing facade, and the only facade. Feature-gated so the host test suite does
// not acquire a uniffi dependency for a module it never uses.
//
// `setup_scaffolding!` has to be at the crate root rather than beside the exported types:
// it defines the `UniFfiTag` the derive macros reference, and that name is resolved from
// the crate root regardless of where the deriving type lives.
#[cfg(feature = "uniffi-bindings")]
uniffi::setup_scaffolding!();

#[cfg(feature = "uniffi-bindings")]
pub mod uniffi_api;

pub use bridge::{BridgeStatus, EmulatorBridge, TickReport};
pub use error::{BridgeError, GfxError};
