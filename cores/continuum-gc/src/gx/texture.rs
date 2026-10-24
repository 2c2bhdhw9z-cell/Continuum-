//! Texture units: the texture formats (tiled, as the hardware stores them), palettes, wrap
//! modes, filtering and mipmaps.
//!
//! Textures are read straight from main memory (or from TMEM when the program preloaded them),
//! so there is no texture cache to get out of date.

use super::{Gx, Ram};
use std::sync::Arc;

/// A texture decoded to RGBA once (every level), reused until the program invalidates the
/// texture cache (BP 0x66), loads a palette or preloads TMEM, or an EFB copy writes a texture.
pub struct Decoded {
    pub levels: Vec<(u32, u32, Vec<[u8; 4]>)>,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct TexKey {
    addr: u32,
    fmt: u32,
    w: u32,
    h: u32,
    tlut: u32,
    preloaded: bool,
    odd: u32,
    levels: u32,
}

/// One texture map's settings, unpacked from its BP registers once per primitive.
#[derive(Clone, Copy, Default, Debug)]
pub struct TexMap {
    pub width: u32,
    pub height: u32,
    pub fmt: u32,
    pub addr: u32,
    pub preloaded: bool,
    pub tmem_even: u32,
    pub tmem_odd: u32,
    pub wrap_s: u32,
    pub wrap_t: u32,
    pub mag_linear: bool,
    pub min_linear: bool,
    /// 0 no mipmaps, 1 nearest level, 2 blend two levels.
    pub mip: u32,
    pub lod_bias: f32,
    pub min_lod: f32,
    pub max_lod: f32,
    pub tlut_base: u32,
    pub tlut_fmt: u32,
}

/// Block width, height (texels) and bytes for each format.
pub fn block_shape(fmt: u32) -> (u32, u32, u32) {
    match fmt {
        0 | 8 | 14 => (8, 8, 32),
        1 | 2 | 9 => (8, 4, 32),
        6 => (4, 4, 64),
        _ => (4, 4, 32),
    }
}

/// Bytes a whole level of `w` x `h` texels takes.
pub fn level_bytes(fmt: u32, w: u32, h: u32) -> u32 {
    let (bw, bh, bb) = block_shape(fmt);
    w.div_ceil(bw) * h.div_ceil(bh) * bb
}

#[inline]
fn x5(v: u32) -> u8 {
    ((v << 3) | (v >> 2)) as u8
}
#[inline]
fn x6(v: u32) -> u8 {
    ((v << 2) | (v >> 4)) as u8
}
#[inline]
fn x4(v: u32) -> u8 {
    ((v << 4) | v) as u8
}
#[inline]
fn x3(v: u32) -> u8 {
    ((v << 5) | (v << 2) | (v >> 1)) as u8
}

pub fn rgb565(v: u16) -> [u8; 4] {
    let v = v as u32;
    [x5(v >> 11), x6((v >> 5) & 63), x5(v & 31), 255]
}

pub fn rgb5a3(v: u16) -> [u8; 4] {
    let v = v as u32;
    if v & 0x8000 != 0 {
        [x5((v >> 10) & 31), x5((v >> 5) & 31), x5(v & 31), 255]
    } else {
        [x4((v >> 8) & 15), x4((v >> 4) & 15), x4(v & 15), x3((v >> 12) & 7)]
    }
}

pub fn ia8(v: u16) -> [u8; 4] {
    let i = (v & 0xFF) as u8;
    [i, i, i, (v >> 8) as u8]
}

impl Gx {
    /// Texture map `n`'s settings from its BP registers.
    pub(super) fn texmap(&self, n: usize) -> TexMap {
        // Maps 0-3 live at 0x80.., maps 4-7 at 0xA0..
        let base = if n < 4 { n } else { 0x20 + n - 4 };
        let mode0 = self.bp[0x80 + base];
        let mode1 = self.bp[0x84 + base];
        let img0 = self.bp[0x88 + base];
        let img1 = self.bp[0x8C + base];
        let img2 = self.bp[0x90 + base];
        let img3 = self.bp[0x94 + base];
        let tlut = self.bp[0x98 + base];
        let min = (mode0 >> 5) & 7;
        let bias = (((mode0 >> 9) & 0xFF) as i8) as f32 / 32.0;
        TexMap {
            width: (img0 & 0x3FF) + 1,
            height: ((img0 >> 10) & 0x3FF) + 1,
            fmt: (img0 >> 20) & 0xF,
            addr: (img3 & 0x00FF_FFFF) << 5,
            preloaded: img1 & (1 << 21) != 0,
            tmem_even: (img1 & 0x7FFF) << 5,
            tmem_odd: (img2 & 0x7FFF) << 5,
            wrap_s: mode0 & 3,
            wrap_t: (mode0 >> 2) & 3,
            mag_linear: mode0 & 0x10 != 0,
            min_linear: min & 4 != 0,
            mip: match min & 3 {
                1 => 1,
                2 => 2,
                _ => 0,
            },
            lod_bias: bias,
            min_lod: (mode1 & 0xFF) as f32 / 16.0,
            max_lod: ((mode1 >> 8) & 0xFF) as f32 / 16.0,
            tlut_base: 0x80000 + ((tlut & 0x3FF) << 9),
            tlut_fmt: (tlut >> 10) & 3,
        }
    }

    #[inline]
    fn tex_byte(&self, ram: &Ram, t: &TexMap, addr: u32) -> u8 {
        if t.preloaded {
            self.tmem[(addr as usize) & 0xFFFFF]
        } else {
            ram.read8(addr)
        }
    }

    #[inline]
    fn tex_u16(&self, ram: &Ram, t: &TexMap, addr: u32) -> u16 {
        ((self.tex_byte(ram, t, addr) as u16) << 8) | self.tex_byte(ram, t, addr + 1) as u16
    }

    fn palette(&self, t: &TexMap, index: u32) -> [u8; 4] {
        let a = (t.tlut_base + (index & 0x3FFF) * 2) as usize & 0xFFFFE;
        let v = u16::from_be_bytes([self.tmem[a], self.tmem[a + 1]]);
        match t.tlut_fmt {
            0 => ia8(v),
            1 => rgb565(v),
            _ => rgb5a3(v),
        }
    }

    /// One texel of level `level` at (x, y), already inside the level.
    pub(super) fn texel(&self, ram: &Ram, t: &TexMap, level_base: u32, level_w: u32, x: u32, y: u32) -> [u8; 4] {
        let (bw, bh, bb) = block_shape(t.fmt);
        let blocks_x = level_w.div_ceil(bw);
        let block = (y / bh) * blocks_x + (x / bw);
        let (ix, iy) = (x % bw, y % bh);
        let b = level_base + block * bb;
        match t.fmt {
            0 => {
                let byte = self.tex_byte(ram, t, b + (iy * 8 + ix) / 2);
                let i = if ix & 1 == 0 { byte >> 4 } else { byte & 15 } as u32;
                let v = x4(i);
                [v, v, v, v]
            }
            1 => {
                let v = self.tex_byte(ram, t, b + iy * 8 + ix);
                [v, v, v, v]
            }
            2 => {
                let byte = self.tex_byte(ram, t, b + iy * 8 + ix) as u32;
                let i = x4(byte & 15);
                [i, i, i, x4(byte >> 4)]
            }
            3 => {
                let o = b + (iy * 4 + ix) * 2;
                let a = self.tex_byte(ram, t, o);
                let i = self.tex_byte(ram, t, o + 1);
                [i, i, i, a]
            }
            4 => rgb565(self.tex_u16(ram, t, b + (iy * 4 + ix) * 2)),
            5 => rgb5a3(self.tex_u16(ram, t, b + (iy * 4 + ix) * 2)),
            6 => {
                let o = (iy * 4 + ix) * 2;
                if t.preloaded {
                    // AR halves in the even bank, GB halves in the odd bank.
                    let blk = block * 32;
                    let ar = (t.tmem_even + (level_base - t.tmem_even) / 2 + blk + o) as usize & 0xFFFFF;
                    let gb = (t.tmem_odd + (level_base - t.tmem_even) / 2 + blk + o) as usize & 0xFFFFF;
                    [self.tmem[ar + 1], self.tmem[gb], self.tmem[gb + 1], self.tmem[ar]]
                } else {
                    let a = ram.read8(b + o);
                    let r = ram.read8(b + o + 1);
                    let g = ram.read8(b + 32 + o);
                    let bl = ram.read8(b + 32 + o + 1);
                    [r, g, bl, a]
                }
            }
            8 => {
                let byte = self.tex_byte(ram, t, b + (iy * 8 + ix) / 2);
                let i = if ix & 1 == 0 { byte >> 4 } else { byte & 15 };
                self.palette(t, i as u32)
            }
            9 => {
                let i = self.tex_byte(ram, t, b + iy * 8 + ix);
                self.palette(t, i as u32)
            }
            10 => {
                let i = self.tex_u16(ram, t, b + (iy * 4 + ix) * 2) & 0x3FFF;
                self.palette(t, i as u32)
            }
            14 => {
                // Four 4x4 DXT1-style sub-blocks: top-left, top-right, bottom-left, bottom-right.
                let sub = (iy / 4) * 2 + (ix / 4);
                let s = b + sub * 8;
                let c0 = self.tex_u16(ram, t, s);
                let c1 = self.tex_u16(ram, t, s + 2);
                let (sx, sy) = (ix % 4, iy % 4);
                let row = self.tex_byte(ram, t, s + 4 + sy);
                let sel = (row >> (6 - 2 * sx)) & 3;
                let a = rgb565(c0);
                let bb = rgb565(c1);
                // The hardware blends at 3/8 and 5/8 rather than thirds.
                let mix = |p: u8, q: u8| ((p as u32 * 5 + q as u32 * 3) >> 3) as u8;
                let half = |p: u8, q: u8| ((p as u32 + q as u32) >> 1) as u8;
                match sel {
                    0 => a,
                    1 => bb,
                    2 => {
                        if c0 > c1 {
                            [mix(a[0], bb[0]), mix(a[1], bb[1]), mix(a[2], bb[2]), 255]
                        } else {
                            [half(a[0], bb[0]), half(a[1], bb[1]), half(a[2], bb[2]), 255]
                        }
                    }
                    _ => {
                        if c0 > c1 {
                            [mix(bb[0], a[0]), mix(bb[1], a[1]), mix(bb[2], a[2]), 255]
                        } else {
                            [half(a[0], bb[0]), half(a[1], bb[1]), half(a[2], bb[2]), 0]
                        }
                    }
                }
            }
            _ => [255, 0, 255, 255],
        }
    }

    /// The decoded texture for map `t`, from the cache or decoded now.
    pub(super) fn decoded(&mut self, ram: &Ram, t: &TexMap) -> Arc<Decoded> {
        let nlev = if t.mip != 0 { levels(t).min(((t.max_lod.ceil() as u32) + 1).max(1)) } else { 1 };
        let tlut = if matches!(t.fmt, 8..=10) { t.tlut_base | (t.tlut_fmt << 30) } else { 0 };
        let key = TexKey { addr: if t.preloaded { t.tmem_even } else { t.addr }, fmt: t.fmt, w: t.width, h: t.height, tlut, preloaded: t.preloaded, odd: t.tmem_odd, levels: nlev };
        if let Some(d) = self.tex_cache.get(&key) {
            return d.clone();
        }
        if self.tex_cache.len() > 1024 {
            self.tex_cache.clear();
        }
        let mut levels = Vec::with_capacity(nlev as usize);
        let (mut base, mut w, mut h) = (key.addr, t.width, t.height);
        for _ in 0..nlev {
            let mut px = Vec::with_capacity((w * h) as usize);
            for y in 0..h {
                for x in 0..w {
                    px.push(self.texel(ram, t, base, w, x, y));
                }
            }
            levels.push((w, h, px));
            base += level_bytes(t.fmt, w, h);
            w = (w / 2).max(1);
            h = (h / 2).max(1);
        }
        let d = Arc::new(Decoded { levels });
        self.tex_cache.insert(key, d.clone());
        d
    }

}

/// How many levels a texture has (down to 1x1).
fn levels(t: &TexMap) -> u32 {
    let m = t.width.max(t.height);
    32 - m.leading_zeros()
}

#[inline]
fn wrap(c: i32, size: u32, mode: u32) -> u32 {
    let s = size as i32;
    match mode {
        // For a power-of-two size (what repeat and mirror are for) the remainder is a mask:
        // the same value as rem_euclid, negative coordinates included, without a division.
        1 if size.is_power_of_two() => (c & (s - 1)) as u32,
        1 => c.rem_euclid(s) as u32,
        2 => {
            let p = if size.is_power_of_two() { c & (2 * s - 1) } else { c.rem_euclid(2 * s) };
            (if p >= s { 2 * s - 1 - p } else { p }) as u32
        }
        _ => c.clamp(0, s - 1) as u32,
    }
}

/// Samples a decoded texture: the same filtering as `Gx::sample`, without decoding texels.
#[inline]
pub fn sample_decoded(d: &Decoded, t: &TexMap, u: f32, v: f32, lod: f32) -> [u8; 4] {
    let lod = (lod + t.lod_bias).clamp(t.min_lod, t.max_lod.max(t.min_lod));
    let magnify = lod <= 0.0;
    let linear = if magnify { t.mag_linear } else { t.min_linear };
    let max_level = d.levels.len() as u32 - 1;
    if magnify || t.mip == 0 || max_level == 0 {
        return level_sample(d, 0, t, u, v, linear);
    }
    if t.mip == 1 {
        let l = (lod + 0.5).floor().min(max_level as f32) as u32;
        return level_sample(d, l, t, u, v, linear);
    }
    let l0 = (lod.floor() as u32).min(max_level);
    let l1 = (l0 + 1).min(max_level);
    let f = ((lod - lod.floor()) * 256.0) as u32;
    let a = level_sample(d, l0, t, u, v, linear);
    if l1 == l0 || f == 0 {
        return a;
    }
    let b = level_sample(d, l1, t, u, v, linear);
    let mut o = [0u8; 4];
    for c in 0..4 {
        o[c] = ((a[c] as u32 * (256 - f) + b[c] as u32 * f) >> 8) as u8;
    }
    o
}

#[inline]
fn level_sample(d: &Decoded, level: u32, t: &TexMap, u: f32, v: f32, linear: bool) -> [u8; 4] {
    let (w, h, px) = &d.levels[level as usize];
    let (w, h) = (*w, *h);
    // Dividing by a power of two and multiplying by its reciprocal (also exact in floating
    // point) give the same result; the multiply is several times quicker.
    let inv = 1.0 / (1u32 << level) as f32;
    let (u, v) = (u * inv, v * inv);
    let at = |x: u32, y: u32| px[(y * w + x) as usize];
    if !linear {
        return at(wrap(u.floor() as i32, w, t.wrap_s), wrap(v.floor() as i32, h, t.wrap_t));
    }
    let (uu, vv) = (u - 0.5, v - 0.5);
    let (fx, fy) = (uu.floor(), vv.floor());
    let ax = ((uu - fx) * 128.0) as u32;
    let ay = ((vv - fy) * 128.0) as u32;
    let (x0, y0) = (fx as i32, fy as i32);
    let xa = wrap(x0, w, t.wrap_s);
    let xb = wrap(x0 + 1, w, t.wrap_s);
    let ya = wrap(y0, h, t.wrap_t);
    let yb = wrap(y0 + 1, h, t.wrap_t);
    let (p00, p10, p01, p11) = (at(xa, ya), at(xb, ya), at(xa, yb), at(xb, yb));
    let mut o = [0u8; 4];
    for c in 0..4 {
        let top = p00[c] as u32 * (128 - ax) + p10[c] as u32 * ax;
        let bot = p01[c] as u32 * (128 - ax) + p11[c] as u32 * ax;
        o[c] = ((top * (128 - ay) + bot * ay + 8192) >> 14) as u8;
    }
    o
}
