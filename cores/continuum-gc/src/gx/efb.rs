//! The pixel engine: depth test, blending and logic operations into the embedded framebuffer
//! (EFB), and EFB copies out to main memory, both to the external framebuffer (XFB, the
//! picture VI shows, in YUV) and to textures (every texture format the hardware can write).

use super::{Gx, Ram};
use std::sync::Arc;

pub const EFB_WIDTH: usize = 640;
pub const EFB_HEIGHT: usize = 528;

/// The EFB's colour or depth words: the drawer's own memory, or (with the GPU drawer) memory the
/// graphics chip draws into and the processor reads directly (gpu/mod.rs), the same words in the
/// same order. Everything else (copies, clears, peeks, save states) sees a plain slice either way,
/// and only after `Gx::sync` has waited for the drawing.
pub struct EfbMem {
    ptr: *mut u32,
    len: usize,
    /// The memory itself: a Vec, or the GPU buffer it lives in (kept alive while it is used).
    _own: Vec<u32>,
    _gpu: Option<Arc<dyn std::any::Any + Send + Sync>>,
}

// SAFETY: `ptr` points into `own` or into the GPU buffer `_gpu` keeps alive; access follows the
// usual borrow rules through Deref/DerefMut.
unsafe impl Send for EfbMem {}
unsafe impl Sync for EfbMem {}

impl EfbMem {
    pub fn owned(mut v: Vec<u32>) -> EfbMem {
        EfbMem { ptr: v.as_mut_ptr(), len: v.len(), _own: v, _gpu: None }
    }

    /// Words in GPU memory.
    ///
    /// # Safety
    /// `ptr` must be valid for `len` words for as long as `keep` lives, and nothing else may use
    /// them while this does (the GPU only while `Gx::sync` is not holding it back).
    pub unsafe fn mapped(ptr: *mut u32, len: usize, keep: Arc<dyn std::any::Any + Send + Sync>) -> EfbMem {
        EfbMem { ptr, len, _own: Vec::new(), _gpu: Some(keep) }
    }

    pub fn is_mapped(&self) -> bool {
        self._gpu.is_some()
    }
}

impl std::ops::Deref for EfbMem {
    type Target = [u32];
    fn deref(&self) -> &[u32] {
        // SAFETY: see the type's notes.
        unsafe { std::slice::from_raw_parts(self.ptr, self.len) }
    }
}

impl std::ops::DerefMut for EfbMem {
    fn deref_mut(&mut self) -> &mut [u32] {
        // SAFETY: see the type's notes.
        unsafe { std::slice::from_raw_parts_mut(self.ptr, self.len) }
    }
}

impl Clone for EfbMem {
    fn clone(&self) -> Self {
        EfbMem::owned(self.to_vec())
    }
}

// The same bytes as the Vec<u32> it used to be, so older save states still load.
impl serde::Serialize for EfbMem {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        (**self).serialize(s)
    }
}

impl<'de> serde::Deserialize<'de> for EfbMem {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<EfbMem, D::Error> {
        Vec::<u32>::deserialize(d).map(EfbMem::owned)
    }
}

/// The pixel engine's settings, unpacked from BP registers once per primitive.
#[derive(Clone, Copy, Default, Debug)]
pub struct PeState {
    pub z_enable: bool,
    pub z_func: u32,
    pub z_update: bool,
    pub z_early: bool,
    pub blend: bool,
    pub logic: bool,
    pub subtract: bool,
    pub src_factor: u32,
    pub dst_factor: u32,
    pub logic_op: u32,
    pub color_update: bool,
    pub alpha_update: bool,
    pub dst_alpha: Option<u8>,
    pub format: u32,
}

#[inline]
pub fn z_pass(func: u32, z: u32, old: u32) -> bool {
    match func {
        0 => false,
        1 => z < old,
        2 => z == old,
        3 => z <= old,
        4 => z > old,
        5 => z != old,
        6 => z >= old,
        _ => true,
    }
}

#[inline]
fn unpack(c: u32) -> [i32; 4] {
    [(c >> 24) as i32, ((c >> 16) & 0xFF) as i32, ((c >> 8) & 0xFF) as i32, (c & 0xFF) as i32]
}

#[inline]
fn pack(c: [i32; 4]) -> u32 {
    ((c[0] as u32 & 0xFF) << 24) | ((c[1] as u32 & 0xFF) << 16) | ((c[2] as u32 & 0xFF) << 8) | (c[3] as u32 & 0xFF)
}

/// Rounds a colour to what the EFB's pixel format can hold.
#[inline]
fn quantize(c: [i32; 4], format: u32) -> [i32; 4] {
    match format {
        1 => {
            let q = |v: i32| {
                let s = v >> 2;
                (s << 2) | (s >> 4)
            };
            [q(c[0]), q(c[1]), q(c[2]), q(c[3])]
        }
        2 => {
            let q5 = |v: i32| {
                let s = v >> 3;
                (s << 3) | (s >> 2)
            };
            let q6 = |v: i32| {
                let s = v >> 2;
                (s << 2) | (s >> 4)
            };
            [q5(c[0]), q6(c[1]), q5(c[2]), 255]
        }
        _ => [c[0], c[1], c[2], 255],
    }
}

#[inline]
fn mul(v: i32, f: i32) -> i32 {
    (v * (f + (f >> 7))) >> 8
}

/// Blends one shaded pixel `src` with the EFB pixel `dst_px` (as stored): the new stored value.
#[inline]
pub(super) fn blend(pe: &PeState, dst_px: u32, src: [i32; 4]) -> u32 {
        let dst = unpack(dst_px);
        let has_alpha = pe.format == 1;
        let dst_a = if has_alpha { dst[3] } else { 255 };
        let mut out = src;
        if pe.blend {
            if pe.subtract {
                for c in 0..4 {
                    out[c] = (dst[c] - src[c]).clamp(0, 255);
                }
            } else {
                let sf = |c: usize| -> i32 {
                    match pe.src_factor {
                        0 => 0,
                        1 => 255,
                        2 => dst[c],
                        3 => 255 - dst[c],
                        4 => src[3],
                        5 => 255 - src[3],
                        6 => dst_a,
                        _ => 255 - dst_a,
                    }
                };
                let df = |c: usize| -> i32 {
                    match pe.dst_factor {
                        0 => 0,
                        1 => 255,
                        2 => src[c],
                        3 => 255 - src[c],
                        4 => src[3],
                        5 => 255 - src[3],
                        6 => dst_a,
                        _ => 255 - dst_a,
                    }
                };
                for c in 0..4 {
                    let d = if c == 3 { dst_a } else { dst[c] };
                    out[c] = (mul(src[c], sf(c)) + mul(d, df(c))).clamp(0, 255);
                }
            }
        } else if pe.logic {
            for c in 0..4 {
                let (s, d) = (src[c] as u8, dst[c] as u8);
                out[c] = match pe.logic_op {
                    0 => 0,
                    1 => s & d,
                    2 => s & !d,
                    3 => s,
                    4 => !s & d,
                    5 => d,
                    6 => s ^ d,
                    7 => s | d,
                    8 => !(s | d),
                    9 => !(s ^ d),
                    10 => !d,
                    11 => s | !d,
                    12 => !s,
                    13 => !s | d,
                    14 => !(s & d),
                    _ => 255,
                } as i32;
            }
        }
        if let Some(a) = pe.dst_alpha {
            out[3] = a as i32;
        }
        let q = quantize(out, pe.format);
        let mut result = dst;
        if pe.color_update {
            result[0] = q[0];
            result[1] = q[1];
            result[2] = q[2];
        }
        if pe.alpha_update || pe.dst_alpha.is_some() {
            result[3] = if has_alpha { q[3] } else { 255 };
        }
        pack(result)
}

impl Gx {
    pub(super) fn pe_state(&self) -> PeState {
        let z = self.bp[0x40];
        let c0 = self.bp[0x41];
        let c1 = self.bp[0x42];
        let ctl = self.bp[0x43];
        PeState {
            z_enable: z & 1 != 0,
            z_func: (z >> 1) & 7,
            z_update: z & 0x10 != 0,
            z_early: ctl & 0x40 != 0,
            blend: c0 & 1 != 0,
            logic: c0 & 2 != 0,
            subtract: c0 & 0x800 != 0,
            src_factor: (c0 >> 8) & 7,
            dst_factor: (c0 >> 5) & 7,
            logic_op: (c0 >> 12) & 15,
            color_update: c0 & 8 != 0,
            alpha_update: c0 & 0x10 != 0,
            dst_alpha: if c1 & 0x100 != 0 { Some((c1 & 0xFF) as u8) } else { None },
            format: ctl & 7,
        }
    }

    // ------------------------------------------------------------------ EFB copies

    pub(super) fn efb_copy(&mut self, ram: &mut Ram, ctl: u32) {
        // Everything drawn so far must be in the EFB first.
        self.sync();
        let x0 = (self.bp[0x49] & 0x3FF) as usize;
        let y0 = ((self.bp[0x49] >> 10) & 0x3FF) as usize;
        let w = ((self.bp[0x4A] & 0x3FF) + 1) as usize;
        let h = (((self.bp[0x4A] >> 10) & 0x3FF) + 1) as usize;
        let dest = (self.bp[0x4B] & 0x00FF_FFFF) << 5;
        let stride = (self.bp[0x4D] & 0x3FF) << 5;
        if ctl & (1 << 14) != 0 {
            self.stats.xfb_copies += 1;
            self.copy_to_xfb(ram, ctl, x0, y0, w, h, dest, stride);
        } else {
            self.stats.efb_copies += 1;
            self.copy_to_texture(ram, ctl, x0, y0, w, h, dest, stride);
        }
        if ctl & (1 << 11) != 0 {
            self.efb_clear(x0, y0, w, h);
        }
    }

    fn efb_clear(&mut self, x0: usize, y0: usize, w: usize, h: usize) {
        let pe = self.pe_state();
        let ar = self.bp[0x4F];
        let gb = self.bp[0x50];
        let color = [(ar & 0xFF) as i32, ((gb >> 8) & 0xFF) as i32, (gb & 0xFF) as i32, ((ar >> 8) & 0xFF) as i32];
        let q = quantize(color, pe.format);
        let z = self.bp[0x51] & 0xFF_FFFF;
        for y in y0..(y0 + h).min(EFB_HEIGHT) {
            for x in x0..(x0 + w).min(EFB_WIDTH) {
                let i = y * EFB_WIDTH + x;
                let mut c = unpack(self.efb_color[i]);
                if pe.color_update {
                    c[0] = q[0];
                    c[1] = q[1];
                    c[2] = q[2];
                }
                if pe.alpha_update {
                    c[3] = if pe.format == 1 { q[3] } else { 255 };
                }
                self.efb_color[i] = pack(c);
                if pe.z_update {
                    self.efb_depth[i] = z;
                }
            }
        }
    }

    /// The copy filter's three row weights (above, this, below), out of 64.
    fn copy_filter(&self) -> [i32; 3] {
        let a = self.bp[0x53];
        let b = self.bp[0x54];
        let c = |v: u32, k: u32| ((v >> (6 * k)) & 0x3F) as i32;
        [c(a, 0) + c(a, 1), c(a, 2) + c(a, 3) + c(b, 0), c(b, 1) + c(b, 2)]
    }

    /// One source pixel through the vertical copy filter.
    fn filtered(&self, x: usize, y: usize, top: usize, bottom: usize, clamp: (bool, bool), wts: &[i32; 3]) -> [i32; 4] {
        let row = |yy: usize| unpack(self.efb_color[yy.min(EFB_HEIGHT - 1) * EFB_WIDTH + x.min(EFB_WIDTH - 1)]);
        let up = if y == top && clamp.0 { y } else { y.saturating_sub(1) };
        let down = if y + 1 >= bottom && clamp.1 { y } else { (y + 1).min(EFB_HEIGHT - 1) };
        if wts == &[0, 64, 0] {
            return row(y);
        }
        let (a, b, c) = (row(up), row(y), row(down));
        let mut o = [0; 4];
        for k in 0..3 {
            o[k] = ((a[k] * wts[0] + b[k] * wts[1] + c[k] * wts[2]) >> 6).clamp(0, 255);
        }
        o[3] = b[3];
        o
    }

    #[allow(clippy::too_many_arguments)]
    fn copy_to_xfb(&mut self, ram: &mut Ram, ctl: u32, x0: usize, y0: usize, w: usize, h: usize, dest: u32, stride: u32) {
        let clamp = (ctl & 1 != 0, ctl & 2 != 0);
        let gamma = match (ctl >> 7) & 3 {
            1 => 1.7f32,
            2 => 2.2,
            _ => 1.0,
        };
        let mut lut = [0i32; 256];
        for (v, l) in lut.iter_mut().enumerate() {
            *l = if gamma == 1.0 { v as i32 } else { ((v as f32 / 255.0).powf(1.0 / gamma) * 255.0 + 0.5) as i32 };
        }
        let wts = self.copy_filter();
        let f2f = (ctl >> 12) & 3;
        let (first, step, lines_in) = if f2f >= 2 { ((f2f - 2) as usize, 2, h.div_ceil(2)) } else { (0, 1, h) };
        let yscale = if ctl & (1 << 10) != 0 { (self.bp[0x4E] & 0x1FF).max(1) as usize } else { 256 };
        let lines_out = if yscale == 256 { lines_in } else { ((lines_in - 1) * 256 / yscale + 1).min(1024) };
        // Line by line, each from its three EFB rows (the filter's above, this and below), the
        // same arithmetic as `filtered` pixel by pixel. Big copies are shared out over threads:
        // a picture a frame, on the CPU's own thread, was a sixth of the time in 2D games.
        let pass = wts == [0, 64, 0];
        let efb = &self.efb_color;
        let line_of = |j: usize, line: &mut [u8], rgb: &mut Vec<[i32; 3]>| {
            let src_line = (j * yscale) >> 8;
            let sy = y0 + first + src_line.min(lines_in - 1) * step;
            let up = if sy == y0 && clamp.0 { sy } else { sy.saturating_sub(1) };
            let down = if sy + 1 >= y0 + h && clamp.1 { sy } else { (sy + 1).min(EFB_HEIGHT - 1) };
            let row = |yy: usize| &efb[yy.min(EFB_HEIGHT - 1) * EFB_WIDTH..yy.min(EFB_HEIGHT - 1) * EFB_WIDTH + EFB_WIDTH];
            let (ra, rb, rc) = (row(up), row(sy), row(down));
            rgb.clear();
            for x in 0..w {
                let xx = (x0 + x).min(EFB_WIDTH - 1);
                let b = unpack(rb[xx]);
                let c = if pass {
                    [b[0], b[1], b[2]]
                } else {
                    let (a, cc) = (unpack(ra[xx]), unpack(rc[xx]));
                    let f = |k: usize| ((a[k] * wts[0] + b[k] * wts[1] + cc[k] * wts[2]) >> 6).clamp(0, 255);
                    [f(0), f(1), f(2)]
                };
                rgb.push([lut[c[0] as usize], lut[c[1] as usize], lut[c[2] as usize]]);
            }
            let y = |c: [i32; 3]| ((66 * c[0] + 129 * c[1] + 25 * c[2] + 128) >> 8) + 16;
            for xp in (0..w).step_by(2) {
                let a = rgb[xp];
                let b = if xp + 1 < w { rgb[xp + 1] } else { a };
                let (ra, ga, ba) = ((a[0] + b[0]) / 2, (a[1] + b[1]) / 2, (a[2] + b[2]) / 2);
                let u = ((-38 * ra - 74 * ga + 112 * ba + 128) >> 8) + 128;
                let v = ((112 * ra - 94 * ga - 18 * ba + 128) >> 8) + 128;
                line[xp * 2] = y(a).clamp(0, 255) as u8;
                line[xp * 2 + 1] = u.clamp(0, 255) as u8;
                if xp + 1 < w {
                    line[xp * 2 + 2] = y(b).clamp(0, 255) as u8;
                    line[xp * 2 + 3] = v.clamp(0, 255) as u8;
                }
            }
        };
        let bytes = w * 2;
        let mut out = vec![0u8; lines_out * bytes];
        let threads = if lines_out * w >= 100_000 { (self.draw.count() + 1).min(8) } else { 1 };
        if threads > 1 && bytes > 0 {
            let per = lines_out.div_ceil(threads);
            std::thread::scope(|sc| {
                for (t, chunk) in out.chunks_mut(per * bytes).enumerate() {
                    let line_of = &line_of;
                    sc.spawn(move || {
                        super::draw::interactive_thread();
                        let mut rgb = Vec::with_capacity(w);
                        for (k, line) in chunk.chunks_mut(bytes).enumerate() {
                            line_of(t * per + k, line, &mut rgb);
                        }
                    });
                }
            });
        } else if bytes > 0 {
            let mut rgb = Vec::with_capacity(w);
            for (j, line) in out.chunks_mut(bytes).enumerate() {
                line_of(j, line, &mut rgb);
            }
        }
        for j in 0..lines_out {
            ram.write(dest + j as u32 * stride, &out[j * bytes..(j + 1) * bytes]);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn copy_to_texture(&mut self, ram: &mut Ram, ctl: u32, x0: usize, y0: usize, w: usize, h: usize, dest: u32, stride: u32) {
        let fmt = ((ctl >> 4) & 7) | (ctl & 8);
        let intensity = ctl & (1 << 15) != 0;
        let half = ctl & (1 << 9) != 0;
        let depth = self.pe_state().format == 3;
        let clamp = (ctl & 1 != 0, ctl & 2 != 0);
        let wts = self.copy_filter();
        let (ow, oh) = if half { (w.div_ceil(2), h.div_ceil(2)) } else { (w, h) };
        // Source sample at output (x, y): RGBA, or depth as (z >> 16, z >> 8, z, 0).
        let src = |g: &Gx, x: usize, y: usize| -> [i32; 4] {
            let one = |xx: usize, yy: usize| -> [i32; 4] {
                if depth {
                    let z = g.efb_depth[yy.min(EFB_HEIGHT - 1) * EFB_WIDTH + xx.min(EFB_WIDTH - 1)];
                    [((z >> 16) & 255) as i32, ((z >> 8) & 255) as i32, (z & 255) as i32, 255]
                } else {
                    g.filtered(xx, yy, y0, y0 + h, clamp, &wts)
                }
            };
            if half {
                let (sx, sy) = (x0 + 2 * x, y0 + 2 * y);
                let (a, b, c, d) = (one(sx, sy), one(sx + 1, sy), one(sx, sy + 1), one(sx + 1, sy + 1));
                let mut o = [0; 4];
                for k in 0..4 {
                    o[k] = (a[k] + b[k] + c[k] + d[k] + 2) >> 2;
                }
                o
            } else {
                one(x0 + x, y0 + y)
            }
        };
        let lum = |c: &[i32; 4]| (((66 * c[0] + 129 * c[1] + 25 * c[2] + 128) >> 8) + 16).clamp(0, 255);
        // Bits per texel and the block shape the format is stored in.
        let (bw, bh, bytes) = match fmt {
            0 => (8, 8, 32),
            1 | 7 | 8 | 9 | 10 => (8, 4, 32),
            2 => (8, 4, 32),
            6 => (4, 4, 64),
            _ => (4, 4, 32),
        };
        let blocks_x = ow.div_ceil(bw);
        let blocks_y = oh.div_ceil(bh);
        let mut block = vec![0u8; bytes];
        for by in 0..blocks_y {
            for bx in 0..blocks_x {
                block.iter_mut().for_each(|b| *b = 0);
                for iy in 0..bh {
                    for ix in 0..bw {
                        let (x, y) = (bx * bw + ix, by * bh + iy);
                        let c = src(self, x.min(ow - 1), y.min(oh - 1));
                        let i = iy * bw + ix;
                        let one8 = |c: &[i32; 4]| -> u8 {
                            if depth {
                                match fmt {
                                    9 => c[1] as u8,
                                    10 => c[2] as u8,
                                    _ => c[0] as u8,
                                }
                            } else if intensity {
                                lum(c) as u8
                            } else {
                                match fmt {
                                    7 => c[3] as u8,
                                    9 => c[1] as u8,
                                    10 => c[2] as u8,
                                    _ => c[0] as u8,
                                }
                            }
                        };
                        match fmt {
                            0 => {
                                let v = one8(&c) >> 4;
                                block[i / 2] |= if ix & 1 == 0 { v << 4 } else { v };
                            }
                            1 | 7 | 8 | 9 | 10 => block[i] = one8(&c),
                            2 => {
                                let iv = if intensity { lum(&c) } else { c[0] } as u8;
                                block[i] = ((c[3] as u8) & 0xF0) | (iv >> 4);
                            }
                            3 | 11 | 12 => {
                                let (hi, lo) = if depth {
                                    match fmt {
                                        12 => (c[1] as u8, c[2] as u8),
                                        _ => (c[0] as u8, c[1] as u8),
                                    }
                                } else {
                                    match fmt {
                                        3 => (c[3] as u8, if intensity { lum(&c) } else { c[0] } as u8),
                                        11 => (c[1] as u8, c[0] as u8),
                                        _ => (c[2] as u8, c[1] as u8),
                                    }
                                };
                                block[i * 2] = hi;
                                block[i * 2 + 1] = lo;
                            }
                            4 => {
                                let v = (((c[0] as u16) >> 3) << 11) | (((c[1] as u16) >> 2) << 5) | ((c[2] as u16) >> 3);
                                block[i * 2..i * 2 + 2].copy_from_slice(&v.to_be_bytes());
                            }
                            5 => {
                                let v = if c[3] >= 0xE0 {
                                    0x8000 | (((c[0] as u16) >> 3) << 10) | (((c[1] as u16) >> 3) << 5) | ((c[2] as u16) >> 3)
                                } else {
                                    (((c[3] as u16) >> 5) << 12) | (((c[0] as u16) >> 4) << 8) | (((c[1] as u16) >> 4) << 4) | ((c[2] as u16) >> 4)
                                };
                                block[i * 2..i * 2 + 2].copy_from_slice(&v.to_be_bytes());
                            }
                            6 => {
                                let c = if intensity && !depth {
                                    // YUVA8.
                                    let y = lum(&c);
                                    let u = (((-38 * c[0] - 74 * c[1] + 112 * c[2] + 128) >> 8) + 128).clamp(0, 255);
                                    let v = (((112 * c[0] - 94 * c[1] - 18 * c[2] + 128) >> 8) + 128).clamp(0, 255);
                                    [y, u, v, c[3]]
                                } else {
                                    c
                                };
                                block[i * 2] = c[3] as u8;
                                block[i * 2 + 1] = c[0] as u8;
                                block[32 + i * 2] = c[1] as u8;
                                block[32 + i * 2 + 1] = c[2] as u8;
                            }
                            _ => {}
                        }
                    }
                }
                let at = dest + by as u32 * stride + (bx * bytes) as u32;
                ram.write(at, &block);
            }
        }
    }

    /// The CPU's direct EFB access (physical 0x08000000): colour or (bit 22) depth.
    pub fn efb_peek(&mut self, pa: u32) -> u32 {
        self.sync();
        let x = ((pa >> 2) & 0x3FF) as usize;
        let y = ((pa >> 12) & 0x3FF) as usize;
        if x >= EFB_WIDTH || y >= EFB_HEIGHT {
            return 0;
        }
        let i = y * EFB_WIDTH + x;
        if pa & (1 << 22) != 0 {
            self.efb_depth[i]
        } else {
            let c = self.efb_color[i];
            // ARGB.
            c.rotate_right(8)
        }
    }

    pub fn efb_poke(&mut self, pa: u32, v: u32) {
        self.sync();
        let x = ((pa >> 2) & 0x3FF) as usize;
        let y = ((pa >> 12) & 0x3FF) as usize;
        if x >= EFB_WIDTH || y >= EFB_HEIGHT {
            return;
        }
        let i = y * EFB_WIDTH + x;
        if pa & (1 << 22) != 0 {
            self.efb_depth[i] = v & 0xFF_FFFF;
        } else {
            self.efb_color[i] = v.rotate_left(8);
        }
    }
}
