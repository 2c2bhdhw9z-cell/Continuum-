//! Random drawing work for comparing the GPU drawer with the CPU drawer: primitives with random
//! combiner stages, konstants, swap tables, indirect texturing, textures (sizes, wrap modes,
//! filters, mipmaps), alpha tests, fog, z textures, depth modes, blending, logic operations and
//! EFB formats, over a random starting EFB. Used by the check the core runs when it starts the
//! GPU drawer on a phone (`crate::gx::gpu_picture_check`) and by the tests.

use super::super::efb::{PeState, EFB_HEIGHT, EFB_WIDTH};
use super::super::raster::{Screen, Setup};
use super::super::tev::{alpha_operand, color_operand, Pipeline, Stage};
use super::super::texture::{Decoded, TexMap};
use std::sync::Arc;

pub struct Rng(pub u64);

impl Rng {
    pub fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 16) as u32
    }
    pub fn below(&mut self, n: u32) -> u32 {
        self.next() % n.max(1)
    }
    pub fn chance(&mut self, one_in: u32) -> bool {
        self.below(one_in) == 0
    }
    pub fn f(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * (self.next() as f32 / u32::MAX as f32)
    }
}

fn texture(r: &mut Rng, mip: bool) -> (Arc<Decoded>, u32, u32) {
    let pow2 = r.chance(2);
    let dim = |r: &mut Rng| if pow2 { 1u32 << r.below(7) } else { 1 + r.below(70) };
    let (w, h) = (dim(r), dim(r));
    let mut levels = Vec::new();
    let (mut lw, mut lh) = (w, h);
    loop {
        let smooth = r.chance(2);
        let px: Vec<[u8; 4]> = (0..lw * lh)
            .map(|k| {
                if smooth {
                    let (x, y) = (k % lw, k / lw);
                    [(x * 255 / lw.max(1)) as u8, (y * 255 / lh.max(1)) as u8, ((x + y) * 7) as u8, (255 - x * 3) as u8]
                } else {
                    r.next().to_le_bytes()
                }
            })
            .collect();
        levels.push((lw, lh, px));
        if !mip || (lw == 1 && lh == 1) {
            break;
        }
        lw = (lw / 2).max(1);
        lh = (lh / 2).max(1);
    }
    (Arc::new(Decoded { levels }), w, h)
}

fn stage(r: &mut Rng, num_tex: usize, indirect: bool) -> Stage {
    let mut st = Stage {
        c: std::array::from_fn(|_| r.below(16) as u8),
        a: std::array::from_fn(|_| r.below(8) as u8),
        c_bias: if r.chance(6) { 3 } else { r.below(3) as u8 },
        c_sub: r.chance(3),
        c_clamp: !r.chance(4),
        c_scale: if r.chance(3) { r.below(4) as u8 } else { 0 },
        c_dest: r.below(4) as u8,
        a_bias: if r.chance(6) { 3 } else { r.below(3) as u8 },
        a_sub: r.chance(3),
        a_clamp: !r.chance(4),
        a_scale: if r.chance(3) { r.below(4) as u8 } else { 0 },
        a_dest: r.below(4) as u8,
        texmap: r.below(8) as u8,
        texcoord: if num_tex > 0 && !r.chance(8) { r.below(num_tex as u32) as u8 } else { r.below(8) as u8 },
        tex_on: !r.chance(3),
        ras: [0, 0, 1, 5, 6, 7][r.below(6) as usize],
        kc_val: [r.below(256) as i32, r.below(256) as i32, r.below(256) as i32],
        ka_val: r.below(256) as i32,
        ras_swizzle: if r.chance(3) { std::array::from_fn(|_| r.below(4) as usize) } else { [0, 1, 2, 3] },
        tex_swizzle: if r.chance(3) { std::array::from_fn(|_| r.below(4) as usize) } else { [0, 1, 2, 3] },
        ind: if indirect && r.chance(2) { r.next() & 0x1F_FFFF } else { 0 },
        ..Default::default()
    };
    st.c_in = st.c.map(color_operand);
    st.a_in = st.a.map(alpha_operand);
    st
}

fn setup(r: &mut Rng) -> Setup {
    let num_tex = r.below(9) as usize;
    let num_ind = if r.chance(4) { 1 + r.below(4) as usize } else { 0 };
    let nstages = if r.chance(4) { 1 + r.below(16) as usize } else { 1 + r.below(3) as usize };
    let stages: Vec<Stage> = (0..nstages).map(|_| stage(r, num_tex, num_ind > 0)).collect();
    let mut maps = [TexMap::default(); 8];
    let mut tex: [Option<Arc<Decoded>>; 8] = Default::default();
    let mut tc_scale = [(1.0f32, 1.0f32); 8];
    let mut ind_map = [0u8; 4];
    let mut ind_coord = [0u8; 4];
    let mut ind_scale = [(0u32, 0u32); 4];
    for i in 0..4 {
        ind_map[i] = r.below(8) as u8;
        ind_coord[i] = r.below(8) as u8;
        ind_scale[i] = (r.below(4), r.below(4));
    }
    let used = |m: usize| stages.iter().any(|s| s.tex_on && s.texmap as usize == m) || ind_map[..num_ind].contains(&(m as u8));
    for m in 0..8 {
        if !used(m) || r.chance(10) {
            continue;
        }
        let mip = r.chance(2);
        let (d, w, h) = texture(r, mip);
        let max_lod = (d.levels.len() - 1) as f32;
        maps[m] = TexMap {
            width: w,
            height: h,
            fmt: 6,
            wrap_s: r.below(4),
            wrap_t: r.below(4),
            mag_linear: r.chance(2),
            min_linear: r.chance(2),
            mip: if mip { 1 + r.below(2) } else { 0 },
            lod_bias: if r.chance(3) { r.f(-2.0, 2.0) } else { 0.0 },
            min_lod: if r.chance(4) { r.f(0.0, 2.0) } else { 0.0 },
            max_lod: if r.chance(4) { r.f(0.0, max_lod) } else { max_lod },
            ..Default::default()
        };
        tex[m] = Some(d);
        let _ = (w, h);
    }
    for (t, sc) in tc_scale.iter_mut().enumerate() {
        let _ = t;
        *sc = (1.0 + r.below(128) as f32, 1.0 + r.below(128) as f32);
    }
    let mut need_lod = [false; 8];
    for st in &stages {
        if st.tex_on {
            let m = &maps[st.texmap as usize];
            if m.mip != 0 || m.mag_linear != m.min_linear || m.min_lod > 0.0 || m.lod_bias != 0.0 {
                need_lod[st.texcoord as usize] = true;
            }
        }
    }
    for i in 0..num_ind {
        need_lod[ind_coord[i] as usize] = true;
    }
    let reg = |r: &mut Rng| if r.chance(2) { r.below(256) as i32 } else { r.below(2048) as i32 - 1024 };
    let fog_type = if r.chance(3) { r.below(8) } else { 0 };
    let ind_mtx = std::array::from_fn(|_| {
        let e = |r: &mut Rng| (r.below(2048) as i32 - 1024) as f32 / 1024.0;
        ([[e(r), e(r), e(r)], [e(r), e(r), e(r)]], 2f32.powi(r.below(12) as i32 - 6))
    });
    let pipe = Pipeline {
        stages,
        regs: std::array::from_fn(|_| std::array::from_fn(|_| reg(r))),
        maps,
        tex,
        alpha_ref: [r.below(256) as i32, r.below(256) as i32],
        alpha_comp: if r.chance(3) { [r.below(8), r.below(8)] } else { [7, 7] },
        alpha_logic: r.below(4),
        fog_type,
        fog_ortho: r.chance(2),
        fog_a: r.f(0.0, 4.0),
        fog_b: r.f(0.5, 2.0),
        fog_c: r.f(-0.5, 0.5),
        fog_color: [r.below(256) as i32, r.below(256) as i32, r.below(256) as i32],
        ztex_op: if r.chance(6) { 1 + r.below(2) } else { 0 },
        ztex_fmt: r.below(3),
        ztex_bias: r.next() & 0xFF_FFFF,
        num_ind,
        ind_map,
        ind_coord,
        ind_scale,
        ind_mtx,
        tc_scale,
    };
    let pe = PeState {
        z_enable: !r.chance(4),
        z_func: if r.chance(2) { 3 } else { r.below(8) },
        z_update: !r.chance(3),
        z_early: r.chance(3),
        blend: r.chance(2),
        logic: r.chance(4),
        subtract: r.chance(6),
        src_factor: r.below(8),
        dst_factor: r.below(8),
        logic_op: r.below(16),
        color_update: !r.chance(8),
        alpha_update: r.chance(2),
        dst_alpha: if r.chance(5) { Some(r.below(256) as u8) } else { None },
        format: [0, 0, 1, 1, 2, 3][r.below(6) as usize],
    };
    let sc = if r.chance(4) {
        let x0 = r.below(400) as i32;
        let y0 = r.below(300) as i32;
        (x0, y0, (x0 + 1 + r.below(400) as i32).min(EFB_WIDTH as i32), (y0 + 1 + r.below(300) as i32).min(EFB_HEIGHT as i32))
    } else {
        (0, 0, EFB_WIDTH as i32, EFB_HEIGHT as i32)
    };
    Setup { pipe, pe, num_chans: r.below(3) as usize, num_tex, sc, off: (0.0, 0.0), vp: [0.0; 6], cull: 0, need_lod }
}

fn vertex(r: &mut Rng, cx: f32, cy: f32, size: f32, num_tex: usize, flat_z: Option<f32>) -> Screen {
    let inv_w = r.f(0.25, 2.0);
    let mut attr = [0f32; 32];
    for a in attr.iter_mut().take(8) {
        *a = r.f(0.0, 300.0) * inv_w;
    }
    for t in 0..num_tex {
        let q = r.f(0.5, 2.0);
        attr[8 + 3 * t] = r.f(-3.0, 3.0) * q * inv_w;
        attr[8 + 3 * t + 1] = r.f(-3.0, 3.0) * q * inv_w;
        attr[8 + 3 * t + 2] = q * inv_w;
    }
    Screen {
        x: cx + r.f(-size, size),
        y: cy + r.f(-size, size),
        z: flat_z.unwrap_or_else(|| r.f(-1000.0, 16_778_000.0)),
        inv_w,
        attr,
    }
}

/// A starting EFB and `primitives` random primitives.
#[allow(clippy::type_complexity)]
pub(in crate::gx) fn random_work(seed: u64, primitives: usize) -> (Vec<u32>, Vec<u32>, Vec<(Arc<Setup>, Vec<[Screen; 3]>)>) {
    let mut r = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let n = EFB_WIDTH * EFB_HEIGHT;
    let color: Vec<u32> = (0..n).map(|_| r.next()).collect();
    let depth: Vec<u32> = (0..n).map(|_| r.next() & 0xFF_FFFF).collect();
    let mut work = Vec::new();
    for _ in 0..primitives {
        let s = setup(&mut r);
        let ntris = 1 + r.below(4) as usize;
        let size = if r.chance(5) { 400.0 } else { r.f(2.0, 90.0) };
        let (cx, cy) = (r.f(-20.0, EFB_WIDTH as f32 + 20.0), r.f(-20.0, EFB_HEIGHT as f32 + 20.0));
        // Some primitives are flat in depth, so that equal depths meet (depth tests with "equal").
        let flat = if r.chance(4) { Some(r.f(0.0, 16_777_215.0)) } else { None };
        let tris = (0..ntris).map(|_| std::array::from_fn(|_| vertex(&mut r, cx, cy, size, s.num_tex, flat))).collect();
        work.push((Arc::new(s), tris));
    }
    (color, depth, work)
}
