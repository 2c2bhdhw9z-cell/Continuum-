//! Primitive assembly, clipping, the viewport, culling and the rasteriser.
//!
//! Triangles are clipped in clip space against the near and far planes (GX keeps z/w between
//! -1 at the near plane and 0 at the far plane) and a wide guard band, then drawn with exact
//! edge functions on a 1/16-pixel grid with the top-left fill rule, sampling at pixel centres.
//! Colours and texture coordinates are interpolated with perspective correction; depth is
//! linear in screen space.

use super::efb::{PeState, EFB_HEIGHT, EFB_WIDTH};
use super::tev::{PixelIn, Pipeline};
use super::vertex::OutVertex;
use super::{Gx, Ram};

/// A vertex after the viewport: EFB pixel position, depth, 1/w and attributes divided by w.
#[derive(Clone, Copy)]
struct Screen {
    x: f32,
    y: f32,
    z: f32,
    inv_w: f32,
    attr: [f32; 32],
}

/// Everything one primitive's pixels share.
struct Setup {
    pipe: Pipeline,
    pe: PeState,
    num_chans: usize,
    num_tex: usize,
    /// Scissor rectangle in EFB pixels (inclusive left/top, exclusive right/bottom).
    sc: (i32, i32, i32, i32),
    off: (f32, f32),
    vp: [f32; 6],
    cull: u32,
    need_lod: [bool; 8],
}

impl Gx {
    pub(super) fn rasterize_primitive(&mut self, ram: &mut Ram, prim: u8, v: &[OutVertex]) {
        let s = self.setup();
        match prim {
            0 | 1 => {
                for q in v.chunks_exact(4) {
                    self.triangle(ram, &s, &q[0], &q[1], &q[2]);
                    self.triangle(ram, &s, &q[0], &q[2], &q[3]);
                }
            }
            2 => {
                for t in v.chunks_exact(3) {
                    self.triangle(ram, &s, &t[0], &t[1], &t[2]);
                }
            }
            3 => {
                for i in 2..v.len() {
                    if i % 2 == 0 {
                        self.triangle(ram, &s, &v[i - 2], &v[i - 1], &v[i]);
                    } else {
                        self.triangle(ram, &s, &v[i - 1], &v[i - 2], &v[i]);
                    }
                }
            }
            4 => {
                for i in 2..v.len() {
                    self.triangle(ram, &s, &v[0], &v[i - 1], &v[i]);
                }
            }
            5 => {
                for l in v.chunks_exact(2) {
                    self.line(ram, &s, &l[0], &l[1]);
                }
            }
            6 => {
                for i in 1..v.len() {
                    self.line(ram, &s, &v[i - 1], &v[i]);
                }
            }
            _ => {
                for p in v {
                    self.point(ram, &s, p);
                }
            }
        }
    }

    fn setup(&self) -> Setup {
        let pipe = self.pipeline();
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

    fn triangle(&mut self, ram: &mut Ram, s: &Setup, a: &OutVertex, b: &OutVertex, c: &OutVertex) {
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
            self.raster_triangle(ram, s, &sv[0], &sv[i], &sv[i + 1]);
        }
    }

    fn line(&mut self, ram: &mut Ram, s: &Setup, a: &OutVertex, b: &OutVertex) {
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
        self.raster_triangle(ram, s, &q[0], &q[1], &q[2]);
        self.raster_triangle(ram, s, &q[0], &q[2], &q[3]);
    }

    fn point(&mut self, ram: &mut Ram, s: &Setup, a: &OutVertex) {
        if a.clip[3] <= 0.0 {
            return;
        }
        let p = self.to_screen(s, a);
        let h = (((self.bp[0x22] >> 8) & 0xFF) as f32 / 6.0).max(1.0 / 6.0) / 2.0;
        let shift = |sx: f32, sy: f32| Screen { x: p.x + sx, y: p.y + sy, ..p };
        let q = [shift(-h, -h), shift(h, -h), shift(h, h), shift(-h, h)];
        self.raster_triangle(ram, s, &q[0], &q[1], &q[2]);
        self.raster_triangle(ram, s, &q[0], &q[2], &q[3]);
    }

    fn raster_triangle(&mut self, ram: &mut Ram, s: &Setup, v0: &Screen, v1: &Screen, v2: &Screen) {
        let fx = |v: f32| (v * 16.0).round() as i64;
        let (p0, mut p1, mut p2) = (v0, v1, v2);
        let mut x = [fx(p0.x), fx(p1.x), fx(p2.x)];
        let mut y = [fx(p0.y), fx(p1.y), fx(p2.y)];
        let mut area = (x[1] - x[0]) * (y[2] - y[0]) - (y[1] - y[0]) * (x[2] - x[0]);
        if area == 0 {
            return;
        }
        if area < 0 {
            std::mem::swap(&mut p1, &mut p2);
            x.swap(1, 2);
            y.swap(1, 2);
            area = -area;
        }
        let minx = x[0].min(x[1]).min(x[2]);
        let maxx = x[0].max(x[1]).max(x[2]);
        let miny = y[0].min(y[1]).min(y[2]);
        let maxy = y[0].max(y[1]).max(y[2]);
        // Pixels whose centres (16 * px + 8) are inside the box.
        let px0 = (((minx - 8) as f64 / 16.0).ceil() as i32).max(s.sc.0);
        let px1 = (((maxx - 8) as f64 / 16.0).floor() as i32).min(s.sc.2 - 1);
        let py0 = (((miny - 8) as f64 / 16.0).ceil() as i32).max(s.sc.1);
        let py1 = (((maxy - 8) as f64 / 16.0).floor() as i32).min(s.sc.3 - 1);
        if px0 > px1 || py0 > py1 {
            return;
        }
        // Edge i runs from vertex i to vertex i+1; the function is positive inside.
        let edges: [(i64, i64, i64); 3] = std::array::from_fn(|i| {
            let (j, k) = (i, (i + 1) % 3);
            let (dx, dy) = (x[k] - x[j], y[k] - y[j]);
            let top_left = (dy == 0 && dx > 0) || dy < 0;
            (dx, dy, if top_left { 0 } else { -1 })
        });
        let eval = |i: usize, px: i64, py: i64| -> i64 {
            let (dx, dy, bias) = edges[i];
            dx * (py - y[i]) - dy * (px - x[i]) + bias
        };

        // Attribute planes: value(px, py) = base + ddx * (px - x0) + ddy * (py - y0), in pixels.
        let nattr = 8 + 3 * s.num_tex;
        let fa = area as f32;
        let (x0, y0) = (x[0] as f32 / 16.0, y[0] as f32 / 16.0);
        let (e1x, e1y) = ((x[1] - x[0]) as f32 / 16.0, (y[1] - y[0]) as f32 / 16.0);
        let (e2x, e2y) = ((x[2] - x[0]) as f32 / 16.0, (y[2] - y[0]) as f32 / 16.0);
        let inv_area = 256.0 / fa;
        let plane = |a0: f32, a1: f32, a2: f32| -> (f32, f32) {
            let (d1, d2) = (a1 - a0, a2 - a0);
            ((d1 * e2y - d2 * e1y) * inv_area, (d2 * e1x - d1 * e2x) * inv_area)
        };
        let mut base = [0f32; 34];
        let mut ddx = [0f32; 34];
        let mut ddy = [0f32; 34];
        let (z0, z1, z2) = (p0.z, p1.z, p2.z);
        let (dzx, dzy) = plane(z0, z1, z2);
        let (dwx, dwy) = plane(p0.inv_w, p1.inv_w, p2.inv_w);
        for k in 0..nattr {
            let (dx, dy) = plane(p0.attr[k], p1.attr[k], p2.attr[k]);
            base[k] = p0.attr[k];
            ddx[k] = dx;
            ddy[k] = dy;
        }
        let mut tex = [[0f32; 3]; 8];
        let mut lods = [0f32; 8];
        let mut vals = [0f32; 34];
        for py in py0..=py1 {
            let cy = (py * 16 + 8) as i64;
            let fy = (py as f32 + 0.5) - y0;
            for px in px0..=px1 {
                let cx = (px * 16 + 8) as i64;
                if eval(0, cx, cy) < 0 || eval(1, cx, cy) < 0 || eval(2, cx, cy) < 0 {
                    continue;
                }
                let fxp = (px as f32 + 0.5) - x0;
                let idx = py as usize * EFB_WIDTH + px as usize;
                let z = (z0 + dzx * fxp + dzy * fy).clamp(0.0, 16_777_215.0) as u32;
                if s.pe.z_early && s.pe.z_enable && !self.depth_test(&s.pe, idx, z) {
                    continue;
                }
                let iw = p0.inv_w + dwx * fxp + dwy * fy;
                let w = if iw != 0.0 { 1.0 / iw } else { 0.0 };
                for k in 0..nattr {
                    vals[k] = base[k] + ddx[k] * fxp + ddy[k] * fy;
                }
                let mut color = [[0i32; 4]; 2];
                for c in 0..s.num_chans.max(1) {
                    for ch in 0..4 {
                        color[c][ch] = (vals[c * 4 + ch] * w).clamp(0.0, 255.0) as i32;
                    }
                }
                for t in 0..s.num_tex {
                    let k = 8 + 3 * t;
                    let (sw, tw, mut qw) = (vals[k], vals[k + 1], vals[k + 2]);
                    if qw == 0.0 {
                        qw = 1e-20;
                    }
                    let (ss, ts) = s.pipe.tc_scale[t];
                    tex[t] = [sw / qw * ss, tw / qw * ts, 1.0];
                    if s.need_lod[t] {
                        let (sx, sy, tx, ty, qx, qy) = (ddx[k], ddy[k], ddx[k + 1], ddy[k + 1], ddx[k + 2], ddy[k + 2]);
                        let q2 = qw * qw;
                        let dudx = (sx * qw - sw * qx) / q2 * ss;
                        let dudy = (sy * qw - sw * qy) / q2 * ss;
                        let dvdx = (tx * qw - tw * qx) / q2 * ts;
                        let dvdy = (ty * qw - tw * qy) / q2 * ts;
                        let rho = (dudx * dudx + dvdx * dvdx).max(dudy * dudy + dvdy * dvdy);
                        lods[t] = if rho > 0.0 { 0.5 * rho.log2() } else { -10.0 };
                    }
                }
                let pin = PixelIn { z, color, tex: &tex };
                let Some((out, z2)) = self.shade(ram, &s.pipe, &pin, &lods) else {
                    continue;
                };
                self.stats.pixels += 1;
                if !s.pe.z_early && !self.depth_test(&s.pe, idx, z2) {
                    continue;
                }
                self.write_color(&s.pe, idx, out);
            }
        }
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
