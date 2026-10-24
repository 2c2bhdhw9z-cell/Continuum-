//! Drawing pixels, on the calling thread or on helper threads (other processor cores).
//!
//! The command processor works out each primitive's shared state (`Setup`) and its triangles in
//! screen space on the CPU's thread, as before; what is left, turning triangles into pixels in
//! the EFB, is a `Job`. Without helpers a job runs at once. With helpers, jobs queue up and the
//! helpers draw them while the CPU carries on, each helper drawing only its own rows of the
//! screen (bands of eight rows, dealt out in turn), every job in order.
//!
//! The picture is exactly the same either way: a pixel is only ever touched by the helper
//! that owns its row, and that helper applies the primitives to it in the order the program
//! sent them. Nothing else reads or writes the EFB until every queued job is done: EFB copies,
//! clears, the CPU's direct EFB access and save states all wait for the helpers first
//! (`Gx::sync`). Drawing reads nothing else that can change: textures are decoded into the
//! job's setup before it is queued.

use super::efb::{z_pass, PeState, EFB_HEIGHT, EFB_WIDTH};
use super::raster::{Screen, Setup};
use super::tev::PixelIn;
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;

/// Rows per band.
const BAND_ROWS: i32 = 8;

/// Where pixels go: the EFB's colour and depth buffers.
#[derive(Clone, Copy)]
pub(super) struct Target {
    color: *mut u32,
    depth: *mut u32,
}

// SAFETY: a `Target` is only used to reach pixels of rows the user owns (see the module notes);
// the buffers outlive every job (the helpers finish before the buffers can go).
unsafe impl Send for Target {}
unsafe impl Sync for Target {}

impl Target {
    pub(super) fn new(color: &mut [u32], depth: &mut [u32]) -> Target {
        assert!(color.len() >= EFB_WIDTH * EFB_HEIGHT && depth.len() >= EFB_WIDTH * EFB_HEIGHT);
        Target { color: color.as_mut_ptr(), depth: depth.as_mut_ptr() }
    }

    #[inline(always)]
    fn color(&self, idx: usize) -> u32 {
        debug_assert!(idx < EFB_WIDTH * EFB_HEIGHT);
        // SAFETY: idx is inside the EFB (the rasteriser clamps to it) and in a row this user owns.
        unsafe { *self.color.add(idx) }
    }
    #[inline(always)]
    fn set_color(&self, idx: usize, v: u32) {
        debug_assert!(idx < EFB_WIDTH * EFB_HEIGHT);
        // SAFETY: as above.
        unsafe { *self.color.add(idx) = v }
    }
    #[inline(always)]
    fn depth(&self, idx: usize) -> u32 {
        debug_assert!(idx < EFB_WIDTH * EFB_HEIGHT);
        // SAFETY: as above.
        unsafe { *self.depth.add(idx) }
    }
    #[inline(always)]
    fn set_depth(&self, idx: usize, v: u32) {
        debug_assert!(idx < EFB_WIDTH * EFB_HEIGHT);
        // SAFETY: as above.
        unsafe { *self.depth.add(idx) = v }
    }

    /// Depth test (and write).
    #[inline]
    pub(super) fn depth_test(&self, pe: &PeState, idx: usize, z: u32) -> bool {
        if !pe.z_enable {
            return true;
        }
        if !z_pass(pe.z_func, z, self.depth(idx)) {
            return false;
        }
        if pe.z_update {
            self.set_depth(idx, z);
        }
        true
    }

    /// Blends one shaded pixel into the EFB.
    #[inline]
    pub(super) fn write_color(&self, pe: &PeState, idx: usize, src: [i32; 4]) {
        let v = super::efb::blend(pe, self.color(idx), src);
        self.set_color(idx, v);
    }
}

/// The rows one rasteriser draws: those whose band (row / 8) is `index` modulo `count`.
#[derive(Clone, Copy)]
pub(super) struct Band {
    pub index: usize,
    pub count: usize,
}

impl Band {
    pub(super) const ALL: Band = Band { index: 0, count: 1 };

    #[inline(always)]
    fn has(&self, row: i32) -> bool {
        self.count == 1 || ((row / BAND_ROWS) as usize) % self.count == self.index
    }

    /// Whether any row from `top` to `bottom` (inclusive) is this band's.
    fn any(&self, top: i32, bottom: i32) -> bool {
        if self.count == 1 {
            return true;
        }
        let mut y = top;
        while y <= bottom {
            if self.has(y) {
                return true;
            }
            y = (y / BAND_ROWS + 1) * BAND_ROWS;
        }
        false
    }
}

/// One primitive's pixels: its setup and its triangles.
pub(super) struct Job {
    pub setup: Arc<Setup>,
    pub tris: Vec<[Screen; 3]>,
    pub target: Target,
}

impl Job {
    /// Draws the job's rows in `band`; returns the pixels written.
    pub(super) fn run(&self, band: Band) -> u64 {
        let mut pixels = 0;
        for t in &self.tris {
            raster_triangle(self.target, &self.setup, &t[0], &t[1], &t[2], band, &mut pixels);
        }
        pixels
    }
}

/// Edge functions on a 1/16-pixel grid with the top-left fill rule, sampling at pixel centres;
/// colours and texture coordinates interpolated with perspective correction, depth linear in
/// screen space.
pub(super) fn raster_triangle(target: Target, s: &Setup, v0: &Screen, v1: &Screen, v2: &Screen, band: Band, pixels: &mut u64) {
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
    if px0 > px1 || py0 > py1 || !band.any(py0, py1) {
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
    // Edge values step by -dy*16 per pixel and dx*16 per row.
    let step_x: [i64; 3] = std::array::from_fn(|i| -edges[i].1 * 16);
    let num_chans = s.num_chans.max(1);
    let fx0 = (px0 as f32 + 0.5) - x0;
    for py in py0..=py1 {
        if !band.has(py) {
            continue;
        }
        let cy = (py * 16 + 8) as i64;
        let fy = (py as f32 + 0.5) - y0;
        let cx0 = (px0 * 16 + 8) as i64;
        let mut e = [eval(0, cx0, cy), eval(1, cx0, cy), eval(2, cx0, cy)];
        let mut z = z0 + dzx * fx0 + dzy * fy;
        let mut iw = p0.inv_w + dwx * fx0 + dwy * fy;
        for k in 0..nattr {
            vals[k] = base[k] + ddx[k] * fx0 + ddy[k] * fy;
        }
        let mut inside_seen = false;
        for px in px0..=px1 {
            if px > px0 {
                e[0] += step_x[0];
                e[1] += step_x[1];
                e[2] += step_x[2];
                z += dzx;
                iw += dwx;
                for k in 0..nattr {
                    vals[k] += ddx[k];
                }
            }
            if e[0] < 0 || e[1] < 0 || e[2] < 0 {
                if inside_seen {
                    // Triangles are convex: once out on this row, the rest is out too.
                    break;
                }
                continue;
            }
            inside_seen = true;
            let idx = py as usize * EFB_WIDTH + px as usize;
            let zi = z.clamp(0.0, 16_777_215.0) as u32;
            if s.pe.z_early && s.pe.z_enable && !target.depth_test(&s.pe, idx, zi) {
                continue;
            }
            let w = if iw != 0.0 { 1.0 / iw } else { 0.0 };
            let mut color = [[0i32; 4]; 2];
            for c in 0..num_chans {
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
                let iq = 1.0 / qw;
                tex[t] = [sw * iq * ss, tw * iq * ts, 1.0];
                if s.need_lod[t] {
                    let (sx, sy, tx, ty, qx, qy) = (ddx[k], ddy[k], ddx[k + 1], ddy[k + 1], ddx[k + 2], ddy[k + 2]);
                    let iq2 = iq * iq;
                    let dudx = (sx * qw - sw * qx) * iq2 * ss;
                    let dudy = (sy * qw - sw * qy) * iq2 * ss;
                    let dvdx = (tx * qw - tw * qx) * iq2 * ts;
                    let dvdy = (ty * qw - tw * qy) * iq2 * ts;
                    let rho = (dudx * dudx + dvdx * dvdx).max(dudy * dudy + dvdy * dvdy);
                    lods[t] = if rho > 0.0 { 0.5 * rho.log2() } else { -10.0 };
                }
            }
            let pin = PixelIn { z: zi, color, tex: &tex };
            let Some((out, z2)) = super::tev::shade(&s.pipe, &pin, &lods) else {
                continue;
            };
            *pixels += 1;
            if !s.pe.z_early && !target.depth_test(&s.pe, idx, z2) {
                continue;
            }
            target.write_color(&s.pe, idx, out);
        }
    }
}

// ---------------------------------------------------------------------- helper threads

struct State {
    jobs: Vec<Arc<Job>>,
    /// How many jobs each helper has finished.
    done: Vec<usize>,
    pixels: u64,
    quit: bool,
}

struct Shared {
    state: Mutex<State>,
    /// Signalled when jobs are added (or it is time to stop).
    work: Condvar,
    /// Signalled when a helper finishes some.
    finished: Condvar,
}

/// The helper threads. Not part of save states; a copy of the drawer (for a save state) has
/// none.
#[derive(Default)]
pub struct Threads {
    shared: Option<Arc<Shared>>,
    handles: Vec<JoinHandle<()>>,
}

impl Clone for Threads {
    fn clone(&self) -> Self {
        Threads::default()
    }
}

fn lock(s: &Shared) -> std::sync::MutexGuard<'_, State> {
    s.state.lock().unwrap_or_else(|p| p.into_inner())
}

impl Threads {
    /// `count` helpers (0 or 1: none, everything drawn on the calling thread).
    pub fn start(count: usize) -> Threads {
        if count < 2 {
            return Threads::default();
        }
        let shared = Arc::new(Shared { state: Mutex::new(State { jobs: Vec::new(), done: vec![0; count], pixels: 0, quit: false }), work: Condvar::new(), finished: Condvar::new() });
        let handles = (0..count)
            .map(|index| {
                let shared = shared.clone();
                std::thread::Builder::new().name(format!("gx draw {index}")).spawn(move || helper(&shared, Band { index, count })).expect("drawing thread")
            })
            .collect();
        Threads { shared: Some(shared), handles }
    }

    pub fn count(&self) -> usize {
        self.handles.len()
    }

    /// Draws the job: queued for the helpers, or now when there are none. Returns the pixels
    /// written now (helpers' pixels are counted at `wait`).
    pub(super) fn submit(&self, job: Job) -> u64 {
        match &self.shared {
            None => job.run(Band::ALL),
            Some(s) => {
                lock(s).jobs.push(Arc::new(job));
                s.work.notify_all();
                0
            }
        }
    }

    /// Waits until every queued job is drawn. Returns the pixels the helpers wrote.
    pub fn wait(&self) -> u64 {
        let Some(s) = &self.shared else {
            return 0;
        };
        let mut st = lock(s);
        if st.jobs.is_empty() {
            return std::mem::take(&mut st.pixels);
        }
        while st.done.iter().any(|&d| d < st.jobs.len()) {
            st = s.finished.wait(st).unwrap_or_else(|p| p.into_inner());
        }
        st.jobs.clear();
        st.done.iter_mut().for_each(|d| *d = 0);
        std::mem::take(&mut st.pixels)
    }
}

impl Drop for Threads {
    fn drop(&mut self) {
        // Finish what was queued (it writes into buffers that are still there), then stop.
        self.wait();
        if let Some(s) = &self.shared {
            lock(s).quit = true;
            s.work.notify_all();
        }
        for h in self.handles.drain(..) {
            let _ = h.join();
        }
    }
}

fn helper(s: &Shared, band: Band) {
    let mut st = lock(s);
    loop {
        if st.quit {
            return;
        }
        let next = st.done[band.index];
        if next < st.jobs.len() {
            let batch: Vec<Arc<Job>> = st.jobs[next..].to_vec();
            let end = st.jobs.len();
            drop(st);
            let mut pixels = 0;
            for job in &batch {
                pixels += job.run(band);
            }
            st = lock(s);
            st.done[band.index] = end;
            st.pixels += pixels;
            s.finished.notify_all();
        } else {
            st = s.work.wait(st).unwrap_or_else(|p| p.into_inner());
        }
    }
}
