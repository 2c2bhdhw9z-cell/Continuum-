//! The texture environment (TEV): up to sixteen colour/alpha combiner stages, with konstant
//! colours, swap tables, indirect texturing, then the alpha test and fog.
//!
//! Every primitive unpacks the BP registers once into a `Pipeline`; each pixel then runs
//! `shade`. Colour values are integers: 0..255 for inputs, signed 11-bit in the registers,
//! like the hardware.

use super::texture::{sample_decoded, Decoded, TexMap};
use super::{Gx, Ram};
use std::sync::Arc;

#[derive(Clone, Copy, Default)]
pub struct Stage {
    pub c: [u8; 4],
    pub c_bias: u8,
    pub c_sub: bool,
    pub c_clamp: bool,
    pub c_scale: u8,
    pub c_dest: u8,
    pub a: [u8; 4],
    pub a_bias: u8,
    pub a_sub: bool,
    pub a_clamp: bool,
    pub a_scale: u8,
    pub a_dest: u8,
    pub ras_swap: u8,
    pub tex_swap: u8,
    pub texmap: u8,
    pub texcoord: u8,
    pub tex_on: bool,
    pub ras: u8,
    pub kc: u8,
    pub ka: u8,
    pub ind: u32,
}

/// What each pixel needs from the rasteriser.
pub struct PixelIn<'a> {
    pub z: u32,
    pub color: [[i32; 4]; 2],
    /// Texture coordinates in texels (after the setup unit's scaling) and their LOD.
    pub tex: &'a [[f32; 3]; 8],
}

#[derive(Clone)]
pub struct Pipeline {
    pub stages: Vec<Stage>,
    pub swap: [[usize; 4]; 4],
    pub konst: [[i32; 4]; 4],
    pub regs: [[i32; 4]; 4],
    pub maps: [TexMap; 8],
    /// The decoded texture for each map a stage or indirect stage uses.
    pub tex: [Option<Arc<Decoded>>; 8],
    pub alpha_ref: [i32; 2],
    pub alpha_comp: [u32; 2],
    pub alpha_logic: u32,
    pub fog_type: u32,
    pub fog_ortho: bool,
    pub fog_a: f32,
    pub fog_b: f32,
    pub fog_c: f32,
    pub fog_color: [i32; 3],
    pub ztex_op: u32,
    pub ztex_fmt: u32,
    pub ztex_bias: u32,
    pub num_ind: usize,
    pub ind_map: [u8; 4],
    pub ind_coord: [u8; 4],
    pub ind_scale: [(u32, u32); 4],
    /// Indirect matrices: 2x3 entries and the power-of-two scale.
    pub ind_mtx: [([[f32; 3]; 2], f32); 3],
    /// Texture coordinate scale (texels per unit) for each coordinate.
    pub tc_scale: [(f32, f32); 8],
}

fn frac_konst(sel: u8) -> i32 {
    match sel {
        0 => 255,
        1 => 223,
        2 => 191,
        3 => 159,
        4 => 128,
        5 => 96,
        6 => 64,
        _ => 32,
    }
}

impl Gx {
    pub(super) fn pipeline(&mut self, ram: &Ram) -> Pipeline {
        let gen = self.bp[0];
        let nstages = (((gen >> 10) & 15) + 1) as usize;
        let mut stages = Vec::with_capacity(nstages);
        for s in 0..nstages {
            let ce = self.bp[0xC0 + 2 * s];
            let ae = self.bp[0xC1 + 2 * s];
            let order = self.bp[0x28 + s / 2] >> if s & 1 == 1 { 12 } else { 0 };
            let ksel = self.bp[0xF6 + s / 2] >> if s & 1 == 1 { 10 } else { 0 };
            stages.push(Stage {
                c: [((ce >> 12) & 15) as u8, ((ce >> 8) & 15) as u8, ((ce >> 4) & 15) as u8, (ce & 15) as u8],
                c_bias: ((ce >> 16) & 3) as u8,
                c_sub: ce & (1 << 18) != 0,
                c_clamp: ce & (1 << 19) != 0,
                c_scale: ((ce >> 20) & 3) as u8,
                c_dest: ((ce >> 22) & 3) as u8,
                a: [((ae >> 13) & 7) as u8, ((ae >> 10) & 7) as u8, ((ae >> 7) & 7) as u8, ((ae >> 4) & 7) as u8],
                a_bias: ((ae >> 16) & 3) as u8,
                a_sub: ae & (1 << 18) != 0,
                a_clamp: ae & (1 << 19) != 0,
                a_scale: ((ae >> 20) & 3) as u8,
                a_dest: ((ae >> 22) & 3) as u8,
                ras_swap: (ae & 3) as u8,
                tex_swap: ((ae >> 2) & 3) as u8,
                texmap: (order & 7) as u8,
                texcoord: ((order >> 3) & 7) as u8,
                tex_on: order & 0x40 != 0,
                ras: ((order >> 7) & 7) as u8,
                kc: ((ksel >> 4) & 31) as u8,
                ka: ((ksel >> 9) & 31) as u8,
                ind: self.bp[0x10 + s],
            });
        }
        let mut swap = [[0usize; 4]; 4];
        for (t, sw) in swap.iter_mut().enumerate() {
            let a = self.bp[0xF6 + 2 * t];
            let b = self.bp[0xF7 + 2 * t];
            *sw = [(a & 3) as usize, ((a >> 2) & 3) as usize, (b & 3) as usize, ((b >> 2) & 3) as usize];
        }
        let mut konst = [[0i32; 4]; 4];
        let mut regs = [[0i32; 4]; 4];
        for k in 0..4 {
            for c in 0..4 {
                konst[k][c] = self.tev_konst[k][c] as i32;
                regs[k][c] = self.tev_regs[k][c] as i32;
            }
        }
        let mut maps = [TexMap::default(); 8];
        for (n, m) in maps.iter_mut().enumerate() {
            *m = self.texmap(n);
        }
        let ac = self.bp[0xF3];
        // Fog: A and C are floats with the low 12 mantissa bits dropped; B is a mantissa and
        // an exponent.
        let f = |v: u32| f32::from_bits((v & 0xF_FFFF) << 12);
        let fog_b_m = (self.bp[0xEF] & 0xFF_FFFF) as f32 / 8_388_608.0;
        let fog_b_e = (self.bp[0xF0] & 31) as i32;
        let mut ind_mtx = [([[0.0f32; 3]; 2], 0.0f32); 3];
        for (m, im) in ind_mtx.iter_mut().enumerate() {
            let r = [self.bp[0x06 + 3 * m], self.bp[0x07 + 3 * m], self.bp[0x08 + 3 * m]];
            let s11 = |v: u32| (((v & 0x7FF) as i32) << 21 >> 21) as f32 / 1024.0;
            let exp = ((r[0] >> 22) & 3) | (((r[1] >> 22) & 3) << 2) | (((r[2] >> 22) & 3) << 4);
            im.0 = [[s11(r[0]), s11(r[1]), s11(r[2])], [s11(r[0] >> 11), s11(r[1] >> 11), s11(r[2] >> 11)]];
            im.1 = 2f32.powi(exp as i32 - 17);
        }
        let iref = self.bp[0x27];
        let mut ind_map = [0u8; 4];
        let mut ind_coord = [0u8; 4];
        let mut ind_scale = [(0u32, 0u32); 4];
        for i in 0..4 {
            ind_map[i] = ((iref >> (6 * i)) & 7) as u8;
            ind_coord[i] = ((iref >> (6 * i + 3)) & 7) as u8;
            let ss = self.bp[0x25 + i / 2] >> (8 * (i % 2));
            ind_scale[i] = (ss & 15, (ss >> 4) & 15);
        }
        let mut tc_scale = [(1.0f32, 1.0f32); 8];
        for (t, sc) in tc_scale.iter_mut().enumerate() {
            *sc = (((self.bp[0x30 + 2 * t] & 0xFFFF) + 1) as f32, ((self.bp[0x31 + 2 * t] & 0xFFFF) + 1) as f32);
        }
        let mut tex: [Option<Arc<Decoded>>; 8] = Default::default();
        let num_ind = ((gen >> 16) & 7) as usize;
        for st in &stages {
            if st.tex_on && tex[st.texmap as usize].is_none() {
                tex[st.texmap as usize] = Some(self.decoded(ram, &maps[st.texmap as usize]));
            }
        }
        for i in 0..num_ind {
            let m = ind_map[i] as usize;
            if tex[m].is_none() {
                tex[m] = Some(self.decoded(ram, &maps[m]));
            }
        }
        Pipeline {
            tex,
            stages,
            swap,
            konst,
            regs,
            maps,
            alpha_ref: [(ac & 0xFF) as i32, ((ac >> 8) & 0xFF) as i32],
            alpha_comp: [(ac >> 16) & 7, (ac >> 19) & 7],
            alpha_logic: (ac >> 22) & 3,
            fog_type: (self.bp[0xF1] >> 21) & 7,
            fog_ortho: self.bp[0xF1] & (1 << 20) != 0,
            fog_a: f(self.bp[0xEE]),
            fog_b: fog_b_m * 2f32.powi(fog_b_e - 1),
            fog_c: f(self.bp[0xF1]),
            fog_color: [((self.bp[0xF2] >> 16) & 0xFF) as i32, ((self.bp[0xF2] >> 8) & 0xFF) as i32, (self.bp[0xF2] & 0xFF) as i32],
            ztex_op: (self.bp[0xF5] >> 2) & 3,
            ztex_fmt: self.bp[0xF5] & 3,
            ztex_bias: self.bp[0xF4] & 0xFF_FFFF,
            num_ind: ((gen >> 16) & 7) as usize,
            ind_map,
            ind_coord,
            ind_scale,
            ind_mtx,
            tc_scale,
        }
    }

}

/// Runs the TEV, the alpha test and fog for one pixel. Returns the colour and depth, or None
/// when the alpha test throws the pixel away.
pub(super) fn shade(p: &Pipeline, px: &PixelIn, lods: &[f32; 8]) -> Option<([i32; 4], u32)> {
    {
        let mut regs = p.regs;
        // Indirect texture lookups, one per indirect stage.
        let mut ind = [[0i32; 3]; 4];
        for (i, out) in ind.iter_mut().enumerate().take(p.num_ind) {
            let tc = p.ind_coord[i] as usize;
            let (ss, ts) = p.ind_scale[i];
            let t = &px.tex[tc];
            let m = p.ind_map[i] as usize;
            let s = match &p.tex[m] {
                Some(d) => sample_decoded(d, &p.maps[m], t[0] / (1u32 << ss) as f32, t[1] / (1u32 << ts) as f32, lods[tc] - ss.max(ts) as f32),
                None => [0; 4],
            };
            *out = [s[3] as i32, s[2] as i32, s[1] as i32];
        }
        let mut last_tex = [0i32; 4];
        let mut prev_off = [0f32; 2];
        let mut bump_alpha = 0i32;
        let mut last_dest = (0usize, 0usize);
        for st in &p.stages {
            // Indirect offset for this stage's coordinates.
            let tc = st.texcoord as usize;
            let mut uv = [px.tex[tc][0], px.tex[tc][1]];
            if st.ind != 0 || p.num_ind > 0 {
                let cmd = st.ind;
                let bt = (cmd & 3) as usize;
                let fmt = (cmd >> 2) & 3;
                let bias = (cmd >> 4) & 7;
                let bs = (cmd >> 7) & 3;
                let mid = (cmd >> 9) & 15;
                let sw = (cmd >> 13) & 7;
                let tw = (cmd >> 16) & 7;
                let add_prev = cmd & (1 << 20) != 0;
                let mut c = if bt < p.num_ind { ind[bt] } else { [0; 3] };
                let (mask, bias_v) = match fmt {
                    0 => (0xFF, -128),
                    1 => (0x1F, 1),
                    2 => (0x0F, 1),
                    _ => (0x07, 1),
                };
                bump_alpha = match bs {
                    1 => c[0],
                    2 => c[1],
                    3 => c[2],
                    _ => 0,
                } & !mask & 0xF8;
                for (k, v) in c.iter_mut().enumerate() {
                    *v &= mask;
                    if bias & (1 << k) != 0 {
                        *v += bias_v;
                    }
                }
                let off = match mid {
                    1..=3 => {
                        let (m, sc) = &p.ind_mtx[(mid - 1) as usize];
                        let f = [c[0] as f32, c[1] as f32, c[2] as f32];
                        [(m[0][0] * f[0] + m[0][1] * f[1] + m[0][2] * f[2]) * sc, (m[1][0] * f[0] + m[1][1] * f[1] + m[1][2] * f[2]) * sc]
                    }
                    5..=7 => {
                        let sc = p.ind_mtx[(mid - 5) as usize].1;
                        [uv[0] * c[0] as f32 * sc / 256.0, uv[1] * c[0] as f32 * sc / 256.0]
                    }
                    9..=11 => {
                        let sc = p.ind_mtx[(mid - 9) as usize].1;
                        [uv[0] * c[1] as f32 * sc / 256.0, uv[1] * c[1] as f32 * sc / 256.0]
                    }
                    _ => [0.0, 0.0],
                };
                let wrapc = |v: f32, w: u32| -> f32 {
                    match w {
                        0 => v,
                        6 => 0.0,
                        _ => {
                            let size = (256 >> (w - 1)) as f32;
                            v.rem_euclid(size)
                        }
                    }
                };
                uv = [wrapc(uv[0], sw), wrapc(uv[1], tw)];
                let mut total = off;
                if add_prev {
                    total[0] += prev_off[0];
                    total[1] += prev_off[1];
                }
                uv[0] += total[0];
                uv[1] += total[1];
                prev_off = total;
            }
            let tex = if st.tex_on {
                let m = st.texmap as usize;
                let s = match &p.tex[m] {
                    Some(d) => sample_decoded(d, &p.maps[m], uv[0], uv[1], lods[tc]),
                    None => [0; 4],
                };
                let sw = &p.swap[st.tex_swap as usize];
                [s[sw[0]] as i32, s[sw[1]] as i32, s[sw[2]] as i32, s[sw[3]] as i32]
            } else {
                [0; 4]
            };
            last_tex = tex;
            let ras_raw = match st.ras {
                0 => px.color[0],
                1 => px.color[1],
                5 => [bump_alpha; 4],
                6 => [bump_alpha | (bump_alpha >> 5); 4],
                _ => [0; 4],
            };
            let rs = &p.swap[st.ras_swap as usize];
            let ras = [ras_raw[rs[0]], ras_raw[rs[1]], ras_raw[rs[2]], ras_raw[rs[3]]];
            let kc = match st.kc {
                0..=7 => [frac_konst(st.kc); 3],
                12..=15 => {
                    let k = &p.konst[(st.kc - 12) as usize];
                    [k[0], k[1], k[2]]
                }
                16..=31 => {
                    let k = &p.konst[((st.kc - 16) % 4) as usize];
                    [k[((st.kc - 16) / 4) as usize]; 3]
                }
                _ => [0; 3],
            };
            let ka = match st.ka {
                0..=7 => frac_konst(st.ka),
                16..=31 => p.konst[((st.ka - 16) % 4) as usize][((st.ka - 16) / 4) as usize],
                _ => 0,
            };
            let cin = |sel: u8, ch: usize| -> i32 {
                match sel {
                    0 => regs[0][ch],
                    1 => regs[0][3],
                    2 => regs[1][ch],
                    3 => regs[1][3],
                    4 => regs[2][ch],
                    5 => regs[2][3],
                    6 => regs[3][ch],
                    7 => regs[3][3],
                    8 => tex[ch],
                    9 => tex[3],
                    10 => ras[ch],
                    11 => ras[3],
                    12 => 255,
                    13 => 128,
                    14 => kc[ch],
                    _ => 0,
                }
            };
            let ain = |sel: u8| -> i32 {
                match sel {
                    0 => regs[0][3],
                    1 => regs[1][3],
                    2 => regs[2][3],
                    3 => regs[3][3],
                    4 => tex[3],
                    5 => ras[3],
                    6 => ka,
                    _ => 0,
                }
            };
            let ca: [i32; 3] = [cin(st.c[0], 0), cin(st.c[0], 1), cin(st.c[0], 2)];
            let cb: [i32; 3] = [cin(st.c[1], 0), cin(st.c[1], 1), cin(st.c[1], 2)];
            let cc: [i32; 3] = [cin(st.c[2], 0), cin(st.c[2], 1), cin(st.c[2], 2)];
            let cd: [i32; 3] = [cin(st.c[3], 0), cin(st.c[3], 1), cin(st.c[3], 2)];
            let (aa, ab, acc, ad) = (ain(st.a[0]), ain(st.a[1]), ain(st.a[2]), ain(st.a[3]));
            let mut col = [0i32; 3];
            if st.c_bias == 3 {
                let mode = (st.c_scale << 1) | st.c_sub as u8;
                for ch in 0..3 {
                    let pass = compare(mode, &ca, &cb, ch);
                    col[ch] = cd[ch] + if pass { cc[ch] & 255 } else { 0 };
                }
            } else {
                for ch in 0..3 {
                    col[ch] = combine(ca[ch], cb[ch], cc[ch], cd[ch], st.c_bias, st.c_sub, st.c_scale);
                }
            }
            let alpha = if st.a_bias == 3 {
                let mode = (st.a_scale << 1) | st.a_sub as u8;
                let pass = if mode >= 6 {
                    let (x, y) = (aa & 255, ab & 255);
                    if mode == 6 {
                        x > y
                    } else {
                        x == y
                    }
                } else {
                    compare(mode, &ca, &cb, 0)
                };
                ad + if pass { acc & 255 } else { 0 }
            } else {
                combine(aa, ab, acc, ad, st.a_bias, st.a_sub, st.a_scale)
            };
            let clampc = |v: i32, c: bool| if c { v.clamp(0, 255) } else { v.clamp(-1024, 1023) };
            let cdst = st.c_dest as usize;
            let adst = st.a_dest as usize;
            for ch in 0..3 {
                regs[cdst][ch] = clampc(col[ch], st.c_clamp);
            }
            regs[adst][3] = clampc(alpha, st.a_clamp);
            last_dest = (cdst, adst);
        }
        let mut out = [regs[last_dest.0][0], regs[last_dest.0][1], regs[last_dest.0][2], regs[last_dest.1][3]];
        for v in out.iter_mut() {
            *v &= 255;
        }

        // Alpha test.
        let a = out[3];
        let test = |f: u32, r: i32| match f {
            0 => false,
            1 => a < r,
            2 => a == r,
            3 => a <= r,
            4 => a > r,
            5 => a != r,
            6 => a >= r,
            _ => true,
        };
        let t0 = test(p.alpha_comp[0], p.alpha_ref[0]);
        let t1 = test(p.alpha_comp[1], p.alpha_ref[1]);
        let pass = match p.alpha_logic {
            0 => t0 && t1,
            1 => t0 || t1,
            2 => t0 != t1,
            _ => t0 == t1,
        };
        if !pass {
            return None;
        }

        // Z texture.
        let mut z = px.z;
        if p.ztex_op != 0 {
            let t = last_tex;
            let zt = match p.ztex_fmt {
                0 => t[3] as u32,
                1 => ((t[3] as u32) << 8) | t[0] as u32,
                _ => ((t[0] as u32) << 16) | ((t[1] as u32) << 8) | t[2] as u32,
            };
            let zt = zt.wrapping_add(p.ztex_bias) & 0xFF_FFFF;
            z = if p.ztex_op == 1 { (z + zt).min(0xFF_FFFF) } else { zt };
        }

        // Fog.
        if p.fog_type >= 2 {
            let zf = px.z as f32 / 16_777_215.0;
            let ze = if p.fog_ortho {
                p.fog_a * zf
            } else {
                let d = p.fog_b - zf;
                if d.abs() < 1e-12 {
                    0.0
                } else {
                    p.fog_a / d
                }
            };
            let fv = (ze - p.fog_c).clamp(0.0, 1.0);
            let f = match p.fog_type {
                2 => fv,
                4 => 1.0 - 2f32.powf(-8.0 * fv),
                5 => 1.0 - 2f32.powf(-8.0 * fv * fv),
                6 => 2f32.powf(-8.0 * (1.0 - fv)),
                7 => 2f32.powf(-8.0 * (1.0 - fv) * (1.0 - fv)),
                _ => fv,
            };
            let fi = (f.clamp(0.0, 1.0) * 256.0) as i32;
            for ch in 0..3 {
                out[ch] = (out[ch] * (256 - fi) + p.fog_color[ch] * fi) >> 8;
            }
        }
        Some((out, z))
    }
}

/// One combiner channel: d +/- lerp(a, b, c), plus bias, times scale.
#[inline]
fn combine(a: i32, b: i32, c: i32, d: i32, bias: u8, sub: bool, scale: u8) -> i32 {
    let (a, b, c) = (a & 255, b & 255, c & 255);
    let c = c + (c >> 7);
    let lerp = (a * (256 - c) + b * c + 128) >> 8;
    let mut v = if sub { d - lerp } else { d + lerp };
    v += match bias {
        1 => 128,
        2 => -128,
        _ => 0,
    };
    match scale {
        1 => v * 2,
        2 => v * 4,
        3 => v >> 1,
        _ => v,
    }
}

/// The comparison modes (bias 3): R8, GR16, BGR24 against all channels, RGB8 per channel.
#[inline]
fn compare(mode: u8, a: &[i32; 3], b: &[i32; 3], ch: usize) -> bool {
    let a8 = [a[0] & 255, a[1] & 255, a[2] & 255];
    let b8 = [b[0] & 255, b[1] & 255, b[2] & 255];
    match mode {
        0 => a8[0] > b8[0],
        1 => a8[0] == b8[0],
        2 => (a8[1] << 8 | a8[0]) > (b8[1] << 8 | b8[0]),
        3 => (a8[1] << 8 | a8[0]) == (b8[1] << 8 | b8[0]),
        4 => (a8[2] << 16 | a8[1] << 8 | a8[0]) > (b8[2] << 16 | b8[1] << 8 | b8[0]),
        5 => (a8[2] << 16 | a8[1] << 8 | a8[0]) == (b8[2] << 16 | b8[1] << 8 | b8[0]),
        6 => a8[ch] > b8[ch],
        _ => a8[ch] == b8[ch],
    }
}
