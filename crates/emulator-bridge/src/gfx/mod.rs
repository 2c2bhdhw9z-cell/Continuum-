//! Graphics layer. `wgpu` only, targeting Metal, and written against a surface rather
//! than against Metal so the next backend is a configuration change.

mod convert;
pub mod gl_hw;
pub mod hw;
pub mod moltenvk;
pub mod moltenvk_device;
mod renderer;
pub mod vulkan_hw;

/// Metal surface construction and `MTLDevice` hand-off. iOS and macOS only.
#[cfg(target_vendor = "apple")]
pub mod metal;

pub use renderer::{FrameCapture, Renderer, ScaleFilter, ScaleMode, ScreenSplit};
