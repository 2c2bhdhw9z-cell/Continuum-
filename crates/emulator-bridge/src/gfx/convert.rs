//! CPU-side normalisation of core pixel formats to RGBA8.
//!
//! Done once, on the way to the GPU, into a scratch buffer that is allocated per
//! session rather than per frame. The alternative — sampling native formats in the
//! shader — would mean a pipeline variant per format and per-frame branching, for
//! a conversion that costs well under a millisecond at these resolutions.
//!
//! Also packs padded rows: cores frequently hand back a stride wider than the
//! visible width, and `writeTexture` wants tight rows.

use crate::frame::{FrameView, PixelFormat};

/// Normalises `frame` into `scratch` as tightly packed RGBA8.
///
/// Returns a slice of exactly `width * height * 4` bytes. When the frame is
/// already tightly packed RGBA8, `scratch` is left untouched and the core's own
/// buffer is returned — the common fast path for 32-bit cores.
pub fn to_rgba8<'a>(frame: &FrameView<'a>, scratch: &'a mut Vec<u8>) -> &'a [u8] {
    let width = frame.width as usize;
    let height = frame.height as usize;
    let tight_row = width * 4;
    let needed = tight_row * height;

    if frame.format == PixelFormat::Rgba8888 && frame.stride_bytes == tight_row {
        return &frame.data[..needed.min(frame.data.len())];
    }

    if scratch.len() != needed {
        scratch.resize(needed, 0);
    }

    match frame.format {
        PixelFormat::Rgba8888 => {
            for y in 0..height {
                let src = y * frame.stride_bytes;
                let dst = y * tight_row;
                scratch[dst..dst + tight_row].copy_from_slice(&frame.data[src..src + tight_row]);
            }
        }
        PixelFormat::Xrgb8888 => {
            // Little-endian XRGB8888 lands in memory as B, G, R, X.
            for y in 0..height {
                let src_row = y * frame.stride_bytes;
                let dst_row = y * tight_row;
                for x in 0..width {
                    let s = src_row + x * 4;
                    let d = dst_row + x * 4;
                    scratch[d] = frame.data[s + 2];
                    scratch[d + 1] = frame.data[s + 1];
                    scratch[d + 2] = frame.data[s];
                    scratch[d + 3] = 255;
                }
            }
        }
        PixelFormat::Rgb565 => {
            for y in 0..height {
                let src_row = y * frame.stride_bytes;
                let dst_row = y * tight_row;
                for x in 0..width {
                    let s = src_row + x * 2;
                    let px = u16::from_le_bytes([frame.data[s], frame.data[s + 1]]);
                    let r5 = ((px >> 11) & 0x1f) as u32;
                    let g6 = ((px >> 5) & 0x3f) as u32;
                    let b5 = (px & 0x1f) as u32;
                    // Bit-replication rather than a multiply: maps 0x1f to 0xff
                    // exactly, so whites stay white.
                    let d = dst_row + x * 4;
                    scratch[d] = ((r5 << 3) | (r5 >> 2)) as u8;
                    scratch[d + 1] = ((g6 << 2) | (g6 >> 4)) as u8;
                    scratch[d + 2] = ((b5 << 3) | (b5 >> 2)) as u8;
                    scratch[d + 3] = 255;
                }
            }
        }
    }

    scratch
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packed_rgba_is_zero_copy() {
        let data = vec![7u8; 4 * 2 * 2];
        let frame = FrameView {
            data: &data,
            width: 2,
            height: 2,
            stride_bytes: 8,
            format: PixelFormat::Rgba8888,
        };
        let mut scratch = Vec::new();
        let out = to_rgba8(&frame, &mut scratch);
        assert_eq!(out.len(), 16);
        assert!(
            scratch.is_empty(),
            "fast path must not touch the scratch buffer"
        );
    }

    #[test]
    fn xrgb_channels_are_reordered() {
        // One pixel: B=0x10, G=0x20, R=0x30, X=0xff
        let data = vec![0x10, 0x20, 0x30, 0xff];
        let frame = FrameView {
            data: &data,
            width: 1,
            height: 1,
            stride_bytes: 4,
            format: PixelFormat::Xrgb8888,
        };
        let mut scratch = Vec::new();
        let out = to_rgba8(&frame, &mut scratch);
        assert_eq!(out, &[0x30, 0x20, 0x10, 0xff]);
    }

    #[test]
    fn rgb565_white_stays_white() {
        let data = 0xffffu16.to_le_bytes().to_vec();
        let frame = FrameView {
            data: &data,
            width: 1,
            height: 1,
            stride_bytes: 2,
            format: PixelFormat::Rgb565,
        };
        let mut scratch = Vec::new();
        let out = to_rgba8(&frame, &mut scratch);
        assert_eq!(out, &[0xff, 0xff, 0xff, 0xff]);
    }

    #[test]
    fn padded_rows_are_packed() {
        // 2x2 frame with a 3-pixel stride; the third column is padding.
        let mut data = vec![0u8; 3 * 4 * 2];
        for y in 0..2usize {
            for x in 0..2usize {
                let i = (y * 3 + x) * 4;
                data[i] = (10 * (y * 2 + x)) as u8; // B
                data[i + 3] = 0x00;
            }
        }
        let frame = FrameView {
            data: &data,
            width: 2,
            height: 2,
            stride_bytes: 12,
            format: PixelFormat::Xrgb8888,
        };
        let mut scratch = Vec::new();
        let out = to_rgba8(&frame, &mut scratch);
        assert_eq!(out.len(), 16);
        // Blue channel of each source pixel landed in the B slot of a tight row.
        assert_eq!(out[2], 0);
        assert_eq!(out[6], 10);
        assert_eq!(out[10], 20);
        assert_eq!(out[14], 30);
        // Alpha forced opaque despite the source X byte being zero.
        assert!(out.chunks(4).all(|p| p[3] == 255));
    }

    #[test]
    fn scratch_is_reused_across_frames() {
        let data = vec![0u8; 16];
        let frame = FrameView {
            data: &data,
            width: 2,
            height: 2,
            stride_bytes: 8,
            format: PixelFormat::Xrgb8888,
        };
        let mut scratch = Vec::new();
        to_rgba8(&frame, &mut scratch);
        let cap = scratch.capacity();
        for _ in 0..100 {
            to_rgba8(&frame, &mut scratch);
        }
        assert_eq!(scratch.capacity(), cap);
    }
}
