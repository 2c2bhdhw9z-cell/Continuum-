//! Vertex reading (the CP's vertex loader) and the transform unit (XF): matrices, projection,
//! lighting and texture-coordinate generation.
//!
//! A vertex's layout comes from the vertex descriptor (VCD: which attributes, direct or by
//! index) and one of eight attribute formats (VAT: element counts, number formats, fixed-point
//! shifts). Attribute order in the stream is fixed: position/normal matrix index, the eight
//! texture matrix indices, position, normal, colour 0, colour 1, texture coordinates 0-7.

use super::{Gx, Ram};

/// A vertex as the transform unit hands it to the rasteriser.
#[derive(Clone, Copy, Default, Debug)]
pub struct OutVertex {
    /// Clip-space position (after the projection matrix).
    pub clip: [f32; 4],
    /// The two colour channels, RGBA in 0..255.
    pub color: [[f32; 4]; 2],
    /// Texture coordinates (s, t, q) before the perspective divide.
    pub tex: [[f32; 3]; 8],
}

/// One vertex as read from the stream.
#[derive(Clone, Copy, Default)]
struct InVertex {
    pnmtx: u32,
    texmtx: [u32; 8],
    pos: [f32; 3],
    /// Normal, binormal, tangent.
    nrm: [[f32; 3]; 3],
    has_nrm: bool,
    color: [[f32; 4]; 2],
    has_color: [bool; 2],
    tex: [[f32; 2]; 8],
}

const COMP_SIZE: [usize; 8] = [1, 1, 2, 2, 4, 0, 0, 0];
const COLOR_SIZE: [usize; 8] = [2, 3, 4, 2, 3, 4, 4, 4];

/// The layout of one vertex attribute format (one VAT entry), unpacked.
#[derive(Clone, Copy)]
struct Vat {
    pos_elems: usize,
    pos_fmt: u32,
    pos_shift: u32,
    nbt: bool,
    nrm_fmt: u32,
    nrm_index3: bool,
    col_fmt: [u32; 2],
    tex_elems: [usize; 8],
    tex_fmt: [u32; 8],
    tex_shift: [u32; 8],
}

impl Gx {
    fn vat(&self, f: usize) -> Vat {
        let (a, b, c) = (self.vat_a[f], self.vat_b[f], self.vat_c[f]);
        let bits = |v: u32, at: u32, n: u32| (v >> at) & ((1 << n) - 1);
        let mut t = Vat {
            pos_elems: 2 + bits(a, 0, 1) as usize,
            pos_fmt: bits(a, 1, 3),
            pos_shift: bits(a, 4, 5),
            nbt: bits(a, 9, 1) != 0,
            nrm_fmt: bits(a, 10, 3),
            nrm_index3: bits(a, 31, 1) != 0,
            col_fmt: [bits(a, 14, 3), bits(a, 18, 3)],
            tex_elems: [0; 8],
            tex_fmt: [0; 8],
            tex_shift: [0; 8],
        };
        let tex = [
            (bits(a, 21, 1), bits(a, 22, 3), bits(a, 25, 5)),
            (bits(b, 0, 1), bits(b, 1, 3), bits(b, 4, 5)),
            (bits(b, 9, 1), bits(b, 10, 3), bits(b, 13, 5)),
            (bits(b, 18, 1), bits(b, 19, 3), bits(b, 22, 5)),
            (bits(b, 27, 1), bits(b, 28, 3), bits(c, 0, 5)),
            (bits(c, 5, 1), bits(c, 6, 3), bits(c, 9, 5)),
            (bits(c, 14, 1), bits(c, 15, 3), bits(c, 18, 5)),
            (bits(c, 23, 1), bits(c, 24, 3), bits(c, 27, 5)),
        ];
        for (n, (e, f, s)) in tex.into_iter().enumerate() {
            t.tex_elems[n] = 1 + e as usize;
            t.tex_fmt[n] = f;
            t.tex_shift[n] = s;
        }
        t
    }

    /// Bytes one vertex of attribute format `f` takes in the stream.
    pub(super) fn vertex_size(&self, f: usize) -> usize {
        let v = self.vat(f);
        let lo = self.vcd_lo;
        let hi = self.vcd_hi;
        let mut n = 0;
        if lo & 1 != 0 {
            n += 1;
        }
        for t in 0..8 {
            if lo & (2 << t) != 0 {
                n += 1;
            }
        }
        let idx = |kind: u32| if kind == 2 { 1 } else { 2 };
        let pos = (lo >> 9) & 3;
        if pos == 1 {
            n += v.pos_elems * COMP_SIZE[v.pos_fmt as usize];
        } else if pos > 1 {
            n += idx(pos);
        }
        let nrm = (lo >> 11) & 3;
        if nrm == 1 {
            n += if v.nbt { 9 } else { 3 } * COMP_SIZE[v.nrm_fmt as usize];
        } else if nrm > 1 {
            n += idx(nrm) * if v.nbt && v.nrm_index3 { 3 } else { 1 };
        }
        for c in 0..2 {
            let k = (lo >> (13 + 2 * c)) & 3;
            if k == 1 {
                n += COLOR_SIZE[v.col_fmt[c as usize] as usize];
            } else if k > 1 {
                n += idx(k);
            }
        }
        for t in 0..8 {
            let k = (hi >> (2 * t)) & 3;
            if k == 1 {
                n += v.tex_elems[t] * COMP_SIZE[v.tex_fmt[t] as usize];
            } else if k > 1 {
                n += idx(k);
            }
        }
        n
    }

    /// Reads and transforms `count` vertices of format `fmt`, then hands the primitive on.
    pub(super) fn draw(&mut self, ram: &mut Ram, prim: u8, fmt: usize, count: usize, data: &[u8]) {
        let size = self.vertex_size(fmt);
        if count == 0 || data.len() < size * count {
            return;
        }
        self.stats.vertices += count as u64;
        let vat = self.vat(fmt);
        let mut out = Vec::with_capacity(count);
        for k in 0..count {
            let v = self.read_vertex(ram, &vat, &data[k * size..(k + 1) * size]);
            out.push(self.transform(&v));
        }
        self.rasterize_primitive(ram, prim, &out);
    }

    fn read_vertex(&self, ram: &Ram, vat: &Vat, d: &[u8]) -> InVertex {
        let lo = self.vcd_lo;
        let hi = self.vcd_hi;
        let mut p = 0usize;
        let mut v = InVertex::default();
        v.pnmtx = if lo & 1 != 0 {
            p += 1;
            d[p - 1] as u32 & 0x3F
        } else {
            self.matidx_a & 0x3F
        };
        for t in 0..8 {
            v.texmtx[t] = if lo & (2 << t) != 0 {
                p += 1;
                d[p - 1] as u32 & 0x3F
            } else if t < 4 {
                (self.matidx_a >> (6 + 6 * t)) & 0x3F
            } else {
                (self.matidx_b >> (6 * (t - 4))) & 0x3F
            };
        }
        // Where an attribute's data is: in the stream (direct) or in an array (by index).
        // Returns the bytes to decode, copied into a small buffer.
        let fetch = |kind: u32, array: usize, size: usize, p: &mut usize, buf: &mut [u8; 36]| {
            match kind {
                1 => {
                    buf[..size].copy_from_slice(&d[*p..*p + size]);
                    *p += size;
                }
                2 | 3 => {
                    let index = if kind == 2 {
                        *p += 1;
                        d[*p - 1] as u32
                    } else {
                        *p += 2;
                        u16::from_be_bytes([d[*p - 2], d[*p - 1]]) as u32
                    };
                    let addr = self.array_base[array].wrapping_add(index.wrapping_mul(self.array_stride[array]));
                    ram.read_into(addr, &mut buf[..size]);
                }
                _ => {}
            }
        };
        let mut buf = [0u8; 36];
        let pos = (lo >> 9) & 3;
        if pos != 0 {
            let cs = COMP_SIZE[vat.pos_fmt as usize];
            fetch(pos, 0, vat.pos_elems * cs, &mut p, &mut buf);
            let scale = 1.0 / (1u32 << vat.pos_shift) as f32;
            for e in 0..vat.pos_elems {
                v.pos[e] = component(&buf[e * cs..], vat.pos_fmt, scale);
            }
        }
        let nrm = (lo >> 11) & 3;
        if nrm != 0 {
            v.has_nrm = true;
            let cs = COMP_SIZE[vat.nrm_fmt as usize];
            let scale = match vat.nrm_fmt {
                0 => 1.0 / 128.0,
                1 => 1.0 / 64.0,
                2 => 1.0 / 32768.0,
                3 => 1.0 / 16384.0,
                _ => 1.0,
            };
            let vectors = if vat.nbt { 3 } else { 1 };
            if nrm > 1 && vat.nbt && vat.nrm_index3 {
                // Three separate indices, one per vector.
                for (k, vec) in v.nrm.iter_mut().enumerate().take(3) {
                    let mut b = [0u8; 36];
                    let index = if nrm == 2 {
                        p += 1;
                        d[p - 1] as u32
                    } else {
                        p += 2;
                        u16::from_be_bytes([d[p - 2], d[p - 1]]) as u32
                    };
                    let addr = self.array_base[1].wrapping_add(index.wrapping_mul(self.array_stride[1])).wrapping_add((k * 3 * cs) as u32);
                    ram.read_into(addr, &mut b[..3 * cs]);
                    for (e, x) in vec.iter_mut().enumerate() {
                        *x = component(&b[e * cs..], vat.nrm_fmt, scale);
                    }
                }
            } else {
                fetch(nrm, 1, vectors * 3 * cs, &mut p, &mut buf);
                for k in 0..vectors {
                    for e in 0..3 {
                        v.nrm[k][e] = component(&buf[(k * 3 + e) * cs..], vat.nrm_fmt, scale);
                    }
                }
            }
        }
        for c in 0..2 {
            let kind = (lo >> (13 + 2 * c)) & 3;
            if kind != 0 {
                let f = vat.col_fmt[c];
                fetch(kind, 2 + c, COLOR_SIZE[f as usize], &mut p, &mut buf);
                v.color[c] = color(&buf, f);
                v.has_color[c] = true;
            }
        }
        for t in 0..8 {
            let kind = (hi >> (2 * t)) & 3;
            if kind != 0 {
                let cs = COMP_SIZE[vat.tex_fmt[t] as usize];
                fetch(kind, 4 + t, vat.tex_elems[t] * cs, &mut p, &mut buf);
                let scale = 1.0 / (1u32 << vat.tex_shift[t]) as f32;
                for e in 0..vat.tex_elems[t] {
                    v.tex[t][e] = component(&buf[e * cs..], vat.tex_fmt[t], scale);
                }
            }
        }
        v
    }

    // ------------------------------------------------------------------ transform unit

    fn mat_row(&self, addr: usize) -> [f32; 4] {
        [self.xf_float(addr), self.xf_float(addr + 1), self.xf_float(addr + 2), self.xf_float(addr + 3)]
    }

    fn transform(&self, v: &InVertex) -> OutVertex {
        let mut o = OutVertex::default();
        let m = (v.pnmtx as usize) * 4;
        let p = [v.pos[0], v.pos[1], v.pos[2], 1.0];
        let r0 = self.mat_row(m);
        let r1 = self.mat_row(m + 4);
        let r2 = self.mat_row(m + 8);
        let view = [dot4(&r0, &p), dot4(&r1, &p), dot4(&r2, &p)];

        // Normals (normal matrix: 3x3 rows at 0x400 + index*3).
        let nm = 0x400 + (v.pnmtx as usize & 31) * 3;
        let nrow = |k: usize| [self.xf_float(nm + 3 * k), self.xf_float(nm + 3 * k + 1), self.xf_float(nm + 3 * k + 2)];
        let (n0, n1, n2) = (nrow(0), nrow(1), nrow(2));
        let tn = |n: &[f32; 3]| [dot3(&n0, n), dot3(&n1, n), dot3(&n2, n)];
        let normal = normalize(tn(&v.nrm[0]));
        let binormal = tn(&v.nrm[1]);
        let tangent = tn(&v.nrm[2]);

        // Projection (XF 0x1020-0x1026).
        let pp: Vec<f32> = (0..6).map(|k| f32::from_bits(self.xfreg(0x1020 + k))).collect();
        let ortho = self.xfreg(0x1026) & 1 != 0;
        o.clip = if ortho {
            [pp[0] * view[0] + pp[1], pp[2] * view[1] + pp[3], pp[4] * view[2] + pp[5], 1.0]
        } else {
            [pp[0] * view[0] + pp[1] * view[2], pp[2] * view[1] + pp[3] * view[2], pp[4] * view[2] + pp[5], -view[2]]
        };

        // Colour channels.
        for c in 0..2 {
            let vc = if v.has_color[c] { v.color[c] } else { [255.0; 4] };
            let ctl_c = self.xfreg(0x100E + c as u32);
            let ctl_a = self.xfreg(0x1010 + c as u32);
            let amb = unpack_rgba(self.xfreg(0x100A + c as u32));
            let mat = unpack_rgba(self.xfreg(0x100C + c as u32));
            let col = self.light_channel(ctl_c, &vc, &amb, &mat, &view, &normal, false);
            let alp = self.light_channel(ctl_a, &vc, &amb, &mat, &view, &normal, true);
            o.color[c] = [col[0], col[1], col[2], alp[3]];
        }

        // Texture coordinates.
        let numtex = (self.xfreg(0x103F) & 15).min(8) as usize;
        let dual = self.xfreg(0x1012) & 1 != 0;
        for t in 0..numtex {
            let info = self.xfreg(0x1040 + t as u32);
            let stq = info & 2 != 0;
            let abc1 = info & 4 != 0;
            let kind = (info >> 4) & 7;
            let src = (info >> 7) & 31;
            let input: [f32; 4] = match src {
                0 => [v.pos[0], v.pos[1], v.pos[2], 1.0],
                1 => [v.nrm[0][0], v.nrm[0][1], v.nrm[0][2], 1.0],
                2 => [o.color[0][0] / 255.0, o.color[0][1] / 255.0, 1.0, 1.0],
                3 => [v.nrm[1][0], v.nrm[1][1], v.nrm[1][2], 1.0],
                4 => [v.nrm[2][0], v.nrm[2][1], v.nrm[2][2], 1.0],
                5..=12 => [v.tex[(src - 5) as usize][0], v.tex[(src - 5) as usize][1], 1.0, 1.0],
                _ => [0.0, 0.0, 1.0, 1.0],
            };
            let input = if abc1 { input } else { [input[0], input[1], 1.0, 1.0] };
            let mut tc = match kind {
                0 => {
                    let mi = v.texmtx[t] as usize * 4;
                    let s = dot4(&self.mat_row(mi), &input);
                    let tt = dot4(&self.mat_row(mi + 4), &input);
                    let q = if stq { dot4(&self.mat_row(mi + 8), &input) } else { 1.0 };
                    [s, tt, q]
                }
                1 => {
                    // Emboss: shift a previous coordinate along the light direction.
                    let source = ((info >> 12) & 7) as usize;
                    let light = ((info >> 15) & 7) as usize;
                    let base = o.tex[source];
                    let lp = self.light_pos(light);
                    let ldir = normalize([lp[0] - view[0], lp[1] - view[1], lp[2] - view[2]]);
                    [base[0] + dot3(&ldir, &binormal), base[1] + dot3(&ldir, &tangent), base[2]]
                }
                2 => [o.color[0][0] / 255.0, o.color[0][1] / 255.0, 1.0],
                3 => [o.color[1][0] / 255.0, o.color[1][1] / 255.0, 1.0],
                _ => [0.0, 0.0, 1.0],
            };
            if dual && kind == 0 {
                let post = self.xfreg(0x1050 + t as u32);
                if post & 0x100 != 0 {
                    let len = (tc[0] * tc[0] + tc[1] * tc[1] + tc[2] * tc[2]).sqrt();
                    if len > 0.0 {
                        tc = [tc[0] / len, tc[1] / len, tc[2] / len];
                    }
                }
                let pm = 0x500 + (post as usize & 0x3F) * 4;
                let inp = [tc[0], tc[1], tc[2], 1.0];
                tc = [dot4(&self.mat_row(pm), &inp), dot4(&self.mat_row(pm + 4), &inp), dot4(&self.mat_row(pm + 8), &inp)];
            }
            o.tex[t] = tc;
        }
        o
    }

    fn light_pos(&self, l: usize) -> [f32; 3] {
        let b = 0x600 + l * 16;
        [self.xf_float(b + 10), self.xf_float(b + 11), self.xf_float(b + 12)]
    }

    /// One colour or alpha channel through the lighting unit. Returns RGBA 0..255 (only the
    /// RGB or the A part is meaningful, by `alpha`).
    #[allow(clippy::too_many_arguments)]
    fn light_channel(&self, ctl: u32, vc: &[f32; 4], amb: &[f32; 4], mat: &[f32; 4], pos: &[f32; 3], n: &[f32; 3], alpha: bool) -> [f32; 4] {
        let material = if ctl & 1 != 0 { *vc } else { *mat };
        if ctl & 2 == 0 {
            return material;
        }
        let ambient = if ctl & 0x40 != 0 { *vc } else { *amb };
        let mut lit = [ambient[0] / 255.0, ambient[1] / 255.0, ambient[2] / 255.0, ambient[3] / 255.0];
        let mask = ((ctl >> 2) & 0xF) | (((ctl >> 11) & 0xF) << 4);
        let diff_fn = (ctl >> 7) & 3;
        let atten_on = ctl & 0x200 != 0;
        let spot = ctl & 0x400 != 0;
        for l in 0..8 {
            if mask & (1 << l) == 0 {
                continue;
            }
            let b = 0x600 + l * 16;
            let lc = unpack_rgba(self.xfmem[b + 3]);
            let a = [self.xf_float(b + 4), self.xf_float(b + 5), self.xf_float(b + 6)];
            let k = [self.xf_float(b + 7), self.xf_float(b + 8), self.xf_float(b + 9)];
            let lpos = [self.xf_float(b + 10), self.xf_float(b + 11), self.xf_float(b + 12)];
            let ldir_reg = [self.xf_float(b + 13), self.xf_float(b + 14), self.xf_float(b + 15)];
            let (ldir, attn) = if !atten_on {
                (normalize([lpos[0] - pos[0], lpos[1] - pos[1], lpos[2] - pos[2]]), 1.0)
            } else if spot {
                let d = [lpos[0] - pos[0], lpos[1] - pos[1], lpos[2] - pos[2]];
                let dist2 = dot3(&d, &d);
                let dist = dist2.sqrt();
                let ld = if dist > 0.0 { [d[0] / dist, d[1] / dist, d[2] / dist] } else { [0.0, 0.0, 0.0] };
                let cos = dot3(&ld, &ldir_reg).max(0.0);
                let num = (a[0] + a[1] * cos + a[2] * cos * cos).max(0.0);
                let den = k[0] + k[1] * dist + k[2] * dist2;
                (ld, if den != 0.0 { num / den } else { 0.0 })
            } else {
                // Specular: the light position holds the direction, the direction the half-angle.
                let ld = normalize(lpos);
                let ndl = dot3(n, &ld);
                let cos = if ndl >= 0.0 { dot3(n, &ldir_reg).max(0.0) } else { 0.0 };
                let num = (a[0] + a[1] * cos + a[2] * cos * cos).max(0.0);
                let den = k[0] + k[1] * cos + k[2] * cos * cos;
                (ld, if den != 0.0 { num / den } else { 0.0 })
            };
            let diffuse = match diff_fn {
                0 => 1.0,
                1 => dot3(n, &ldir),
                _ => dot3(n, &ldir).max(0.0),
            };
            let f = attn * diffuse;
            for c in 0..4 {
                lit[c] += f * lc[c] / 255.0;
            }
        }
        let mut out = [0.0; 4];
        for c in 0..4 {
            out[c] = material[c] * lit[c].clamp(0.0, 1.0);
        }
        if alpha {
            out
        } else {
            [out[0], out[1], out[2], material[3]]
        }
    }
}

#[inline]
fn dot4(a: &[f32; 4], b: &[f32; 4]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3]
}

#[inline]
fn dot3(a: &[f32; 3], b: &[f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn normalize(v: [f32; 3]) -> [f32; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if l > 0.0 {
        [v[0] / l, v[1] / l, v[2] / l]
    } else {
        v
    }
}

fn unpack_rgba(v: u32) -> [f32; 4] {
    [(v >> 24) as f32, ((v >> 16) & 0xFF) as f32, ((v >> 8) & 0xFF) as f32, (v & 0xFF) as f32]
}

#[inline]
fn component(b: &[u8], fmt: u32, scale: f32) -> f32 {
    match fmt {
        0 => b[0] as f32 * scale,
        1 => b[0] as i8 as f32 * scale,
        2 => u16::from_be_bytes([b[0], b[1]]) as f32 * scale,
        3 => i16::from_be_bytes([b[0], b[1]]) as f32 * scale,
        _ => f32::from_bits(u32::from_be_bytes([b[0], b[1], b[2], b[3]])),
    }
}

fn color(b: &[u8], fmt: u32) -> [f32; 4] {
    let x5 = |v: u32| ((v << 3) | (v >> 2)) as f32;
    let x6 = |v: u32| ((v << 2) | (v >> 4)) as f32;
    let x4 = |v: u32| ((v << 4) | v) as f32;
    match fmt {
        0 => {
            let v = u16::from_be_bytes([b[0], b[1]]) as u32;
            [x5(v >> 11), x6((v >> 5) & 63), x5(v & 31), 255.0]
        }
        1 | 2 => [b[0] as f32, b[1] as f32, b[2] as f32, 255.0],
        3 => {
            let v = u16::from_be_bytes([b[0], b[1]]) as u32;
            [x4(v >> 12), x4((v >> 8) & 15), x4((v >> 4) & 15), x4(v & 15)]
        }
        4 => {
            let v = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
            [x6(v >> 18), x6((v >> 12) & 63), x6((v >> 6) & 63), x6(v & 63)]
        }
        _ => [b[0] as f32, b[1] as f32, b[2] as f32, b[3] as f32],
    }
}
