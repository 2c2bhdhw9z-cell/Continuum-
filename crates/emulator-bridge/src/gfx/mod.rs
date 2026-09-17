//! Graphics layer. `wgpu` only — WebGPU in the browser, Metal on iOS.

mod convert;
pub mod hw;
mod renderer;

pub use renderer::{FrameCapture, Renderer, ScaleFilter, ScaleMode};
