//! Graphics layer. `wgpu` only — WebGPU in the browser, Metal on iOS.

mod convert;
pub mod hw;
mod renderer;

/// Metal surface construction and `MTLDevice` hand-off. iOS and macOS only.
#[cfg(target_vendor = "apple")]
pub mod metal;

pub use renderer::{FrameCapture, Renderer, ScaleFilter, ScaleMode};
