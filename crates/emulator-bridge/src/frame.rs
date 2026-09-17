//! Frame geometry and pixel formats shared by cores and the renderer.
//!
//! Libretro cores emit one of three pixel formats. The bridge normalises them to
//! RGBA8 exactly once, on the CPU side of the upload, so the GPU pipeline stays
//! format-agnostic and the shader never branches.

/// Pixel layout of a core's video output, mirroring `retro_pixel_format`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelFormat {
    /// 16-bit `RGB565`, little-endian. Default for many 2D-era cores.
    Rgb565,
    /// 32-bit `XRGB8888` (alpha ignored). Most common for 3D-era cores.
    Xrgb8888,
    /// 32-bit `RGBA8888`, already in GPU-native order: uploaded without conversion.
    Rgba8888,
}

impl PixelFormat {
    pub const fn bytes_per_pixel(self) -> usize {
        match self {
            PixelFormat::Rgb565 => 2,
            PixelFormat::Xrgb8888 | PixelFormat::Rgba8888 => 4,
        }
    }

    /// Stable numeric encoding for the FFI facades (wasm-bindgen / UniFFI).
    pub const fn as_u32(self) -> u32 {
        match self {
            PixelFormat::Rgb565 => 0,
            PixelFormat::Xrgb8888 => 1,
            PixelFormat::Rgba8888 => 2,
        }
    }

    pub const fn from_u32(v: u32) -> Option<Self> {
        match v {
            0 => Some(PixelFormat::Rgb565),
            1 => Some(PixelFormat::Xrgb8888),
            2 => Some(PixelFormat::Rgba8888),
            _ => None,
        }
    }
}

/// Static video characteristics a core reports up front (`retro_get_system_av_info`).
#[derive(Debug, Clone, Copy)]
pub struct FrameGeometry {
    /// Nominal framebuffer size, e.g. 256x240 for NES.
    pub base_width: u32,
    pub base_height: u32,
    /// Largest framebuffer the core can ever emit. Sizes the GPU texture, so the
    /// texture is allocated once per session instead of per resolution change.
    pub max_width: u32,
    pub max_height: u32,
    /// Display aspect ratio (4:3 = 1.3333). Drives letterboxing, not the texture.
    pub aspect_ratio: f32,
}

impl FrameGeometry {
    pub fn new(base_width: u32, base_height: u32, aspect_ratio: f32) -> Self {
        Self {
            base_width,
            base_height,
            max_width: base_width,
            max_height: base_height,
            aspect_ratio,
        }
    }

    pub fn with_max(mut self, max_width: u32, max_height: u32) -> Self {
        self.max_width = max_width.max(self.base_width);
        self.max_height = max_height.max(self.base_height);
        self
    }
}

/// A borrowed view of one finished frame, handed from a core to the renderer.
///
/// Borrowed rather than owned: cores keep their own framebuffer and the renderer
/// uploads straight out of it. Zero per-frame allocation, zero copies beyond the
/// one the GPU upload requires.
#[derive(Debug, Clone, Copy)]
pub struct FrameView<'a> {
    pub data: &'a [u8],
    pub width: u32,
    pub height: u32,
    /// Row stride in bytes. Cores routinely pad rows, so this is not `width * bpp`.
    pub stride_bytes: usize,
    pub format: PixelFormat,
}

impl<'a> FrameView<'a> {
    /// Validates that `data` actually covers `height` rows of `stride_bytes`.
    pub fn validate(&self) -> Result<(), crate::error::GfxError> {
        use crate::error::GfxError;
        let min_stride = self.width as usize * self.format.bytes_per_pixel();
        if self.width == 0 || self.height == 0 {
            return Err(GfxError::InvalidFrame("zero-sized frame".into()));
        }
        if self.stride_bytes < min_stride {
            return Err(GfxError::InvalidFrame(format!(
                "stride {} is narrower than {} bytes/row",
                self.stride_bytes, min_stride
            )));
        }
        let needed = self.stride_bytes * (self.height as usize - 1) + min_stride;
        if self.data.len() < needed {
            return Err(GfxError::InvalidFrame(format!(
                "buffer holds {} bytes, needs {}",
                self.data.len(),
                needed
            )));
        }
        Ok(())
    }
}
