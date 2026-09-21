//! Graphics layer. `wgpu` only, targeting Metal, and written against a surface rather
//! than against Metal so the next backend is a configuration change.

mod convert;
pub mod hw;
mod renderer;

/// Metal surface construction and `MTLDevice` hand-off. iOS and macOS only.
#[cfg(target_vendor = "apple")]
pub mod metal;

pub use renderer::{FrameCapture, Renderer, ScaleFilter, ScaleMode, ScreenSplit};
