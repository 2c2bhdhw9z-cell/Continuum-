//! Hardware-rendered cores: the seam between a core that owns a texture and a compositor
//! that samples one.
//!
//! Today's path is "the core handed us pixels, upload them". PS1, N64, PSP, DS, 3DS and
//! Switch cores do not hand over pixels — they render into a target the frontend provides
//! and then say "the frame is in there". That is `RETRO_ENVIRONMENT_SET_HW_RENDER`, and it
//! needs one new abstraction rather than a second renderer.
//!
//! `HwContext` is deliberately shaped like [`crate::audio::AudioSink`]: a trait with
//! implementations chosen at runtime, where the engine holds a `Box<dyn HwContext>` and
//! never learns which one it has. The two eventual implementations are MoltenVK (Vulkan)
//! and ANGLE (GLES); both end up as Metal textures underneath, which is what makes the
//! handover to the compositor a zero-copy import rather than a readback.
//!
//! See `docs/SET_HW_RENDER_DESIGN.md`.

use crate::error::GfxError;

/// Which libretro hardware context an implementation satisfies.
///
/// Values match `enum retro_hw_context_type` in `libretro.h`, so a core's declared
/// `context_type` can be compared directly without a translation table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum HwContextType {
    None = 0,
    OpenGl = 1,
    OpenGlEs2 = 2,
    OpenGlCore = 3,
    OpenGlEs3 = 4,
    OpenGlEsVersion = 5,
    Vulkan = 6,
}

impl HwContextType {
    /// Parses libretro's enum value, rejecting the Direct3D variants rather than mapping
    /// them to something plausible.
    pub fn from_libretro(value: u32) -> Option<Self> {
        match value {
            0 => Some(Self::None),
            1 => Some(Self::OpenGl),
            2 => Some(Self::OpenGlEs2),
            3 => Some(Self::OpenGlCore),
            4 => Some(Self::OpenGlEs3),
            5 => Some(Self::OpenGlEsVersion),
            6 => Some(Self::Vulkan),
            _ => None,
        }
    }

    pub fn is_gl(self) -> bool {
        matches!(
            self,
            Self::OpenGl
                | Self::OpenGlEs2
                | Self::OpenGlCore
                | Self::OpenGlEs3
                | Self::OpenGlEsVersion
        )
    }
}

/// A render target handed to a core for one frame.
///
/// The texture is opaque here on purpose. `wgpu` types would drag the graphics stack into
/// every target that merely *mentions* a hardware frame, including the unit tests, so what
/// crosses this boundary is an identifier the renderer resolves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HwTarget {
    /// Index into the context's own target pool. Double-buffered, so this alternates.
    pub index: u32,
    pub width: u32,
    pub height: u32,
    /// GL cores render with the origin at bottom-left; Metal is top-left. The compositor
    /// flips V rather than blitting through an intermediate texture.
    pub bottom_left_origin: bool,
}

/// What produced the frame the compositor is about to present.
///
/// `Duped` is a distinct variant rather than an absent value because, with two producers,
/// "the core repeated its last frame" and "the hardware path failed" are different things
/// and only one of them is normal. libretro signals a dupe by passing `NULL` to
/// `video_refresh`, which is legal and common — a core with nothing new to show says so.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameSourceKind {
    /// The core wrote pixels into the staging buffer; upload them.
    Software,
    /// The core rendered into `HwTarget`; nothing to upload.
    Texture(HwTarget),
    /// Present whatever is already there.
    Duped,
}

/// The sentinel a hardware core passes to `video_refresh` in place of a pixel pointer.
///
/// `RETRO_HW_FRAME_BUFFER_VALID` is `((void*)-1)`. It must be tested for *before* the
/// pointer is treated as data: the software path accepts any non-null pointer, so a
/// hardware frame would otherwise be read from `usize::MAX`.
pub const HW_FRAME_BUFFER_VALID: usize = usize::MAX;

/// Classifies what a `video_refresh` callback was given.
///
/// One function so both the wasm and native core hosts make the same decision, rather than
/// each re-deriving the three-way distinction between a dupe, a hardware frame and pixels.
pub fn classify_video_refresh(data: usize) -> VideoRefreshKind {
    match data {
        0 => VideoRefreshKind::Duped,
        HW_FRAME_BUFFER_VALID => VideoRefreshKind::Hardware,
        _ => VideoRefreshKind::Pixels,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoRefreshKind {
    /// `NULL` — repeat the previous frame.
    Duped,
    /// `RETRO_HW_FRAME_BUFFER_VALID` — the frame is in the target we provided.
    Hardware,
    /// A real pointer to pixels.
    Pixels,
}

/// A hardware rendering context, from the engine's point of view.
///
/// Implementations live in the platform layer, because creating one needs a Vulkan or EGL
/// stack the engine deliberately knows nothing about.
pub trait HwContext {
    fn context_type(&self) -> HwContextType;

    /// Acquires this frame's target. Double-buffered: the compositor may still be sampling
    /// the previous one.
    fn begin_frame(&mut self, width: u32, height: u32) -> Result<HwTarget, GfxError>;

    /// Called after the core's `retro_run` returns. Places whatever synchronisation the
    /// translation layer needs before the compositor samples the texture.
    fn end_frame(&mut self) -> Result<(), GfxError>;

    /// GL framebuffer object name, for `get_current_framebuffer`. Vulkan returns 0 — it
    /// hands frames over through `set_image` instead.
    fn current_framebuffer(&self) -> u64 {
        0
    }

    /// Symbol lookup for the core's `get_proc_address`.
    fn proc_address(&self, symbol: &str) -> Option<usize>;

    /// Rebuilt after context loss. On iOS the `MTLDevice` survives backgrounding but
    /// drawables do not, so this runs on the foreground transition and the core's
    /// `context_reset` follows it.
    fn recreate(&mut self) -> Result<(), GfxError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hardware_sentinel_is_not_mistaken_for_pixels() {
        // The bug this exists to prevent: reading pixels from (void*)-1.
        assert_eq!(
            classify_video_refresh(HW_FRAME_BUFFER_VALID),
            VideoRefreshKind::Hardware
        );
        assert_eq!(classify_video_refresh(0), VideoRefreshKind::Duped);
        assert_eq!(classify_video_refresh(0x1000), VideoRefreshKind::Pixels);
    }

    #[test]
    fn context_types_match_libretro_values() {
        assert_eq!(HwContextType::from_libretro(6), Some(HwContextType::Vulkan));
        assert_eq!(
            HwContextType::from_libretro(4),
            Some(HwContextType::OpenGlEs3)
        );
        // Direct3D is refused rather than mapped onto something plausible.
        assert_eq!(HwContextType::from_libretro(7), None);
        assert_eq!(HwContextType::from_libretro(11), None);
    }

    #[test]
    fn gl_contexts_are_distinguished_from_vulkan() {
        assert!(HwContextType::OpenGlEs3.is_gl());
        assert!(HwContextType::OpenGlCore.is_gl());
        assert!(!HwContextType::Vulkan.is_gl());
        assert!(!HwContextType::None.is_gl());
    }

    #[test]
    fn a_duped_frame_is_distinct_from_a_software_one() {
        // Not the same thing, and conflating them is how "the hardware path silently did
        // nothing" looks identical to "the core had nothing new to show".
        assert_ne!(FrameSourceKind::Duped, FrameSourceKind::Software);
        let target = HwTarget {
            index: 1,
            width: 1280,
            height: 720,
            bottom_left_origin: false,
        };
        assert_ne!(FrameSourceKind::Texture(target), FrameSourceKind::Duped);
    }
}
