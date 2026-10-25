//! Primitive assembly, clipping, the viewport and culling.
//!
//! Triangles are clipped in clip space against the near and far planes (GX keeps z/w between
//! -1 at the near plane and 0 at the far plane) and a wide guard band, then handed to the
//! rasteriser (draw.rs) in screen space, one job per primitive.

use super::draw::{Job, Target};
use super::efb::{PeState, EFB_HEIGHT, EFB_WIDTH};
use super::tev::Pipeline;
use super::vertex::OutVertex;
use super::{Gx, Ram};
use std::sync::Arc;

/// A vertex after the viewport: EFB pixel position, depth, 1/w and attributes divided by w.
#[derive(Clone, Copy)]
pub(super) struct Screen {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub inv_w: f32,
    pub attr: [f32; 32],
}

/// Everything one primitive's pixels share.
pub(super) struct Setup {
    pub pipe: Pipeline,
    pub pe: PeState,
    pub num_chans: usize,
    pub num_tex: usize,
    /// Scissor rectangle in EFB pixels (inclusive left/top, exclusive right/bottom).
    pub sc: (i32, i32, i32, i32),
    pub off: (f32, f32),
    pub vp: [f32; 6],
    pub cull: u32,
    pub need_lod: [bool; 8],
}

impl Gx {
    pub(super) fn rasterize_primitive(&mut self, ram: &mut Ram, prim: u8, v: &[OutVertex]) {
        let s = self.setup(ram);
        let mut tris: Vec<[Screen; 3]> = Vec::new();
        match prim {
            0 | 1 => {
                for q in v.chunks_exact(4) {
                    self.triangle(&s, &q[0], &q[1], &q[2], &mut tris);
                    self.triangle(&s, &q[0], &q[2], &q[3], &mut tris);
                }
            }
            2 => {
                for t in v.chunks_exact(3) {
                    self.triangle(&s, &t[0], &t[1], &t[2], &mut tris);
                }
            }
            3 => {
                for i in 2..v.len() {
                    if i % 2 == 0 {
                        self.triangle(&s, &v[i - 2], &v[i - 1], &v[i], &mut tris);
                    } else {
                        self.triangle(&s, &v[i - 1], &v[i - 2], &v[i], &mut tris);
                    }
                }
            }
            4 => {
                for i in 2..v.len() {
                    self.triangle(&s, &v[0], &v[i - 1], &v[i], &mut tris);
                }
            }
            5 => {
                for l in v.chunks_exact(2) {
                    self.line(&s, &l[0], &l[1], &mut tris);
                }
            }
            6 => {
                for i in 1..v.len() {
                    self.line(&s, &v[i - 1], &v[i], &mut tris);
                }
            }
            _ => {
                for p in v {
                    self.point(&s, p, &mut tris);
                }
            }
        }
        if tris.is_empty() {
            return;
        }
        // The graphics chip draws them, when it is drawing; if it fails, the CPU takes over from
        // this primitive on.
        if let Some(g) = self.gpu.get() {
            match g.submit(&s, &tris) {
                Ok(()) => return,
                Err(e) => self.gpu_failed(&e),
            }
        }
        let target = Target::new(&mut self.efb_color, &mut self.efb_depth);
        let n = self.draw.submit(Job { setup: Arc::new(s), tris, target });
        self.stats.pixels += n;
    }

    fn setup(&mut self, ram: &Ram) -> Setup {
        let pipe = self.pipeline(ram);
        let gen = self.bp[0];
        let tl = self.bp[0x20];
        let br = self.bp[0x21];
        let so = self.bp[0x59];
        let offx = ((so & 0x3FF) as i32) * 2;
        let offy = (((so >> 10) & 0x3FF) as i32) * 2;
        let left = ((tl >> 12) & 0x7FF) as i32 - offx;
        let top = (tl & 0x7FF) as i32 - offy;
        let right = ((br >> 12) & 0x7FF) as i32 - offx + 1;
        let bottom = (br & 0x7FF) as i32 - offy + 1;
        let sc = (left.max(0), top.max(0), right.min(EFB_WIDTH as i32), bottom.min(EFB_HEIGHT as i32));
        let vp = [0x101A, 0x101B, 0x101C, 0x101D, 0x101E, 0x101F].map(|r| f32::from_bits(self.xfreg(r)));
        let num_tex = (gen & 15) as usize;
        let mut need_lod = [false; 8];
        for st in &pipe.stages {
            if st.tex_on {
                let m = &pipe.maps[st.texmap as usize];
                if m.mip != 0 || m.mag_linear != m.min_linear || m.min_lod > 0.0 || m.lod_bias != 0.0 {
                    need_lod[st.texcoord as usize] = true;
                }
            }
        }
        for i in 0..pipe.num_ind {
            need_lod[pipe.ind_coord[i] as usize] = true;
        }
        Setup {
            pe: self.pe_state(),
            num_chans: ((gen >> 4) & 7).min(2) as usize,
            num_tex: num_tex.min(8),
            sc,
            off: (offx as f32, offy as f32),
            vp,
            cull: (gen >> 14) & 3,
            need_lod,
            pipe,
        }
    }

    /// Attributes are packed as: colour 0 RGBA, colour 1 RGBA, then (s, t, q) per coordinate.
    fn to_screen(&self, s: &Setup, v: &OutVertex) -> Screen {
        let w = v.clip[3];
        let inv_w = 1.0 / w;
        let mut attr = [0f32; 32];
        let mut k = 0;
        for c in 0..2 {
            for ch in 0..4 {
                attr[k] = v.color[c][ch] * inv_w;
                k += 1;
            }
        }
        for t in 0..s.num_tex {
            for e in 0..3 {
                attr[k] = v.tex[t][e] * inv_w;
                k += 1;
            }
        }
        Screen {
            x: v.clip[0] * inv_w * s.vp[0] + s.vp[3] - s.off.0,
            y: v.clip[1] * inv_w * s.vp[1] + s.vp[4] - s.off.1,
            z: v.clip[2] * inv_w * s.vp[2] + s.vp[5],
            inv_w,
            attr,
        }
    }

    fn triangle(&mut self, s: &Setup, a: &OutVertex, b: &OutVertex, c: &OutVertex, out: &mut Vec<[Screen; 3]>) {
        self.stats.triangles += 1;
        let poly = clip(&[*a, *b, *c]);
        if poly.len() < 3 {
            return;
        }
        let sv: Vec<Screen> = poly.iter().map(|v| self.to_screen(s, v)).collect();
        // Culling by the screen winding (y down): area > 0 is clockwise, the front.
        let mut area = 0.0f32;
        for i in 0..sv.len() {
            let (p, q) = (&sv[i], &sv[(i + 1) % sv.len()]);
            area += p.x * q.y - q.x * p.y;
        }
        let front = area > 0.0;
        match s.cull {
            1 if !front => return,
            2 if front => return,
            3 => return,
            _ => {}
        }
        for i in 1..sv.len() - 1 {
            out.push([sv[0], sv[i], sv[i + 1]]);
        }
    }

    fn line(&mut self, s: &Setup, a: &OutVertex, b: &OutVertex, out: &mut Vec<[Screen; 3]>) {
        if a.clip[3] <= 0.0 || b.clip[3] <= 0.0 {
            return;
        }
        let (pa, pb) = (self.to_screen(s, a), self.to_screen(s, b));
        let width = ((self.bp[0x22] & 0xFF) as f32 / 6.0).max(1.0 / 6.0);
        let (dx, dy) = (pb.x - pa.x, pb.y - pa.y);
        // The width goes along the minor axis.
        let (ox, oy) = if dx.abs() >= dy.abs() { (0.0, width / 2.0) } else { (width / 2.0, 0.0) };
        let shift = |p: &Screen, sx: f32, sy: f32| Screen { x: p.x + sx, y: p.y + sy, ..*p };
        let q = [shift(&pa, -ox, -oy), shift(&pb, -ox, -oy), shift(&pb, ox, oy), shift(&pa, ox, oy)];
        out.push([q[0], q[1], q[2]]);
        out.push([q[0], q[2], q[3]]);
    }

    fn point(&mut self, s: &Setup, a: &OutVertex, out: &mut Vec<[Screen; 3]>) {
        if a.clip[3] <= 0.0 {
            return;
        }
        let p = self.to_screen(s, a);
        let h = (((self.bp[0x22] >> 8) & 0xFF) as f32 / 6.0).max(1.0 / 6.0) / 2.0;
        let shift = |sx: f32, sy: f32| Screen { x: p.x + sx, y: p.y + sy, ..p };
        let q = [shift(-h, -h), shift(h, -h), shift(h, h), shift(-h, h)];
        out.push([q[0], q[1], q[2]]);
        out.push([q[0], q[2], q[3]]);
    }
}

/// Clips a polygon against the near plane (z >= -w), the far plane (z <= 0), w > 0 and a guard
/// band of eight times the screen in x and y.
fn clip(poly: &[OutVertex]) -> Vec<OutVertex> {
    let planes: [fn(&OutVertex) -> f32; 7] = [
        |v| v.clip[2] + v.clip[3] * 1.000_001 + 1e-7,
        |v| -v.clip[2] + v.clip[3] * 1e-6 + 1e-7,
        |v| v.clip[3] - 1e-7,
        |v| 8.0 * v.clip[3] - v.clip[0],
        |v| 8.0 * v.clip[3] + v.clip[0],
        |v| 8.0 * v.clip[3] - v.clip[1],
        |v| 8.0 * v.clip[3] + v.clip[1],
    ];
    if poly.iter().all(|v| planes.iter().all(|p| p(v) >= 0.0)) {
        return poly.to_vec();
    }
    let mut cur = poly.to_vec();
    for plane in planes {
        if cur.is_empty() {
            break;
        }
        let mut next = Vec::with_capacity(cur.len() + 2);
        for i in 0..cur.len() {
            let (a, b) = (&cur[i], &cur[(i + 1) % cur.len()]);
            let (da, db) = (plane(a), plane(b));
            if da >= 0.0 {
                next.push(*a);
            }
            if (da >= 0.0) != (db >= 0.0) {
                let t = da / (da - db);
                next.push(lerp(a, b, t));
            }
        }
        cur = next;
    }
    cur
}

fn lerp(a: &OutVertex, b: &OutVertex, t: f32) -> OutVertex {
    let mut o = OutVertex::default();
    for k in 0..4 {
        o.clip[k] = a.clip[k] + (b.clip[k] - a.clip[k]) * t;
    }
    for c in 0..2 {
        for k in 0..4 {
            o.color[c][k] = a.color[c][k] + (b.color[c][k] - a.color[c][k]) * t;
        }
    }
    for n in 0..8 {
        for k in 0..3 {
            o.tex[n][k] = a.tex[n][k] + (b.tex[n][k] - a.tex[n][k]) * t;
        }
    }
    o
}
