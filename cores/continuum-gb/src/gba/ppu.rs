//! The GBA picture: a scanline renderer for modes 0-5 (text and affine backgrounds, the three
//! bitmap modes), sprites (regular and affine, 1D/2D tile mapping, semi-transparent, the object
//! window), windows 0/1/object/outside, and colour effects (alpha blend, brighten, darken).
//! Not yet: mosaic.

use serde::{Deserialize, Serialize};

pub const W: usize = 240;
pub const H: usize = 160;
const CLEAR: u16 = 0x8000;

#[derive(Clone, Serialize, Deserialize)]
pub struct Ppu {
    /// The affine backgrounds' internal reference points (BG2, BG3), 20.8 fixed point.
    pub ref_x: [i32; 2],
    pub ref_y: [i32; 2],
    #[serde(skip, default = "blank")]
    pub frame: Vec<u32>,
    /// Layer switches from the core options: bits 0-3 BG0-BG3, bit 4 sprites. Not saved.
    #[serde(skip, default = "all_layers")]
    pub layers: u8,
}

fn all_layers() -> u8 {
    0x1F
}

fn blank() -> Vec<u32> {
    vec![0; W * H]
}

impl Default for Ppu {
    fn default() -> Self {
        Ppu { ref_x: [0; 2], ref_y: [0; 2], frame: blank(), layers: 0x1F }
    }
}

fn r16(m: &[u8], a: usize) -> u16 {
    u16::from(m[a]) | u16::from(m[a + 1]) << 8
}

fn r32(m: &[u8], a: usize) -> u32 {
    u32::from(r16(m, a)) | u32::from(r16(m, a + 2)) << 16
}

pub fn to_xrgb(c: u16) -> u32 {
    let ch = |v: u16| -> u32 {
        let v = u32::from(v & 31);
        (v << 3) | (v >> 2)
    };
    ch(c) << 16 | ch(c >> 5) << 8 | ch(c >> 10)
}

/// Sprite sizes by [shape][size].
const OBJ_DIMS: [[(i32, i32); 4]; 3] = [
    [(8, 8), (16, 16), (32, 32), (64, 64)],
    [(16, 8), (32, 8), (32, 16), (64, 32)],
    [(8, 16), (8, 32), (16, 32), (32, 64)],
];

#[derive(Clone, Copy)]
struct ObjPx {
    color: u16,
    prio: u8,
    semi: bool,
}

impl Ppu {
    /// Reloads the affine reference points from BGxX/BGxY (at VBlank, and when they are written).
    pub fn reload_affine(&mut self, io: &[u8]) {
        for i in 0..2 {
            let base = 0x28 + i * 0x10;
            self.ref_x[i] = ((r32(io, base) << 4) as i32) >> 4;
            self.ref_y[i] = ((r32(io, base + 4) << 4) as i32) >> 4;
        }
    }

    fn text_bg(&self, io: &[u8], vram: &[u8], pal: &[u8], bg: usize, y: usize, out: &mut [u16; W]) {
        let cnt = r16(io, 0x08 + bg * 2);
        let hofs = (r16(io, 0x10 + bg * 4) & 0x1FF) as usize;
        let vofs = (r16(io, 0x12 + bg * 4) & 0x1FF) as usize;
        let size = (cnt >> 14) as usize;
        let w = if size & 1 != 0 { 512 } else { 256 };
        let h = if size & 2 != 0 { 512 } else { 256 };
        let char_base = ((cnt >> 2) & 3) as usize * 0x4000;
        let screen_base = ((cnt >> 8) & 0x1F) as usize * 0x800;
        let bpp8 = cnt & 0x80 != 0;
        let py = (y + vofs) % h;
        for (x, o) in out.iter_mut().enumerate() {
            let px = (x + hofs) % w;
            let sbb = px / 256 + (py / 256) * (w / 256);
            let map = screen_base + sbb * 0x800 + ((py % 256) / 8) * 64 + ((px % 256) / 8) * 2;
            let entry = r16(vram, map & 0xFFFF);
            let tile = (entry & 0x3FF) as usize;
            let tx = if entry & 0x400 != 0 { 7 - px % 8 } else { px % 8 };
            let ty = if entry & 0x800 != 0 { 7 - py % 8 } else { py % 8 };
            *o = if bpp8 {
                let a = char_base + tile * 64 + ty * 8 + tx;
                if a >= 0x10000 { CLEAR } else {
                    let c = vram[a] as usize;
                    if c == 0 { CLEAR } else { r16(pal, c * 2) & 0x7FFF }
                }
            } else {
                let a = char_base + tile * 32 + ty * 4 + tx / 2;
                if a >= 0x10000 { CLEAR } else {
                    let b = vram[a];
                    let c = if tx & 1 != 0 { b >> 4 } else { b & 15 } as usize;
                    if c == 0 { CLEAR } else {
                        r16(pal, ((entry >> 12) as usize * 16 + c) * 2) & 0x7FFF
                    }
                }
            };
        }
    }

    fn affine_bg(&self, io: &[u8], vram: &[u8], pal: &[u8], bg: usize, mode: u16, out: &mut [u16; W]) {
        let i = bg - 2;
        let cnt = r16(io, 0x08 + bg * 2);
        let pa = i32::from(r16(io, 0x20 + i * 0x10) as i16);
        let pc = i32::from(r16(io, 0x24 + i * 0x10) as i16);
        let (rx, ry) = (self.ref_x[i], self.ref_y[i]);
        let frame = if r16(io, 0) & 0x10 != 0 { 0xA000 } else { 0 };
        for (x, o) in out.iter_mut().enumerate() {
            let tx = (rx + pa * x as i32) >> 8;
            let ty = (ry + pc * x as i32) >> 8;
            *o = match mode {
                3 => {
                    if (0..240).contains(&tx) && (0..160).contains(&ty) {
                        r16(vram, (ty as usize * 240 + tx as usize) * 2) & 0x7FFF
                    } else { CLEAR }
                }
                4 => {
                    if (0..240).contains(&tx) && (0..160).contains(&ty) {
                        let c = vram[frame + ty as usize * 240 + tx as usize] as usize;
                        if c == 0 { CLEAR } else { r16(pal, c * 2) & 0x7FFF }
                    } else { CLEAR }
                }
                5 => {
                    if (0..160).contains(&tx) && (0..128).contains(&ty) {
                        r16(vram, frame + (ty as usize * 160 + tx as usize) * 2) & 0x7FFF
                    } else { CLEAR }
                }
                _ => {
                    let size = 128i32 << (cnt >> 14);
                    let (mut tx, mut ty) = (tx, ty);
                    if cnt & 0x2000 != 0 {
                        tx = tx.rem_euclid(size);
                        ty = ty.rem_euclid(size);
                    } else if !(0..size).contains(&tx) || !(0..size).contains(&ty) {
                        *o = CLEAR;
                        continue;
                    }
                    let char_base = ((cnt >> 2) & 3) as usize * 0x4000;
                    let screen_base = ((cnt >> 8) & 0x1F) as usize * 0x800;
                    let map = screen_base + (ty / 8 * (size / 8) + tx / 8) as usize;
                    let tile = vram[map & 0xFFFF] as usize;
                    let a = char_base + tile * 64 + (ty % 8) as usize * 8 + (tx % 8) as usize;
                    let c = if a < 0x10000 { vram[a] as usize } else { 0 };
                    if c == 0 { CLEAR } else { r16(pal, c * 2) & 0x7FFF }
                }
            };
        }
    }

    fn objects(io: &[u8], vram: &[u8], pal: &[u8], oam: &[u8], y: i32, bitmap: bool,
               out: &mut [Option<ObjPx>; W], objwin: &mut [bool; W]) {
        let one_d = r16(io, 0) & 0x40 != 0;
        for i in 0..128 {
            let a0 = r16(oam, i * 8);
            let a1 = r16(oam, i * 8 + 2);
            let a2 = r16(oam, i * 8 + 4);
            let affine = a0 & 0x100 != 0;
            if !affine && a0 & 0x200 != 0 {
                continue;
            }
            let mode = (a0 >> 10) & 3;
            if mode == 3 {
                continue;
            }
            let shape = (a0 >> 14) as usize;
            if shape == 3 {
                continue;
            }
            let (w, h) = OBJ_DIMS[shape][(a1 >> 14) as usize];
            let double = affine && a0 & 0x200 != 0;
            let (bw, bh) = if double { (w * 2, h * 2) } else { (w, h) };
            let oy = i32::from(a0 & 0xFF);
            let dy = (y - oy).rem_euclid(256);
            if dy >= bh {
                continue;
            }
            let mut ox = i32::from(a1 & 0x1FF);
            if ox >= 240 {
                ox -= 512;
            }
            let bpp8 = a0 & 0x2000 != 0;
            let tile = i32::from(a2 & 0x3FF);
            let prio = ((a2 >> 10) & 3) as u8;
            let palbank = (a2 >> 12) as usize;
            let (pa, pb, pc, pd) = if affine {
                let p = ((a1 >> 9) & 31) as usize * 32;
                (
                    i32::from(r16(oam, p + 6) as i16),
                    i32::from(r16(oam, p + 14) as i16),
                    i32::from(r16(oam, p + 22) as i16),
                    i32::from(r16(oam, p + 30) as i16),
                )
            } else {
                (0, 0, 0, 0)
            };
            for sx in 0..bw {
                let x = ox + sx;
                if !(0..W as i32).contains(&x) {
                    continue;
                }
                let (tx, ty) = if affine {
                    let (cx, cy) = (sx - bw / 2, dy - bh / 2);
                    (((pa * cx + pb * cy) >> 8) + w / 2, ((pc * cx + pd * cy) >> 8) + h / 2)
                } else {
                    let tx = if a1 & 0x1000 != 0 { w - 1 - sx } else { sx };
                    let ty = if a1 & 0x2000 != 0 { h - 1 - dy } else { dy };
                    (tx, ty)
                };
                if !(0..w).contains(&tx) || !(0..h).contains(&ty) {
                    continue;
                }
                let step = if bpp8 { 2 } else { 1 };
                let t = if one_d {
                    tile + ((ty / 8) * (w / 8) + tx / 8) * step
                } else {
                    tile + (ty / 8) * 32 + (tx / 8) * step
                };
                let base = 0x10000 + (t as usize & 0x3FF) * 32;
                if bitmap && base < 0x14000 {
                    continue;
                }
                let c = if bpp8 {
                    let c = vram[(base + (ty % 8) as usize * 8 + (tx % 8) as usize) & 0x17FFF] as usize;
                    if c == 0 { continue }
                    r16(pal, 0x200 + c * 2)
                } else {
                    let b = vram[(base + (ty % 8) as usize * 4 + (tx % 8) as usize / 2) & 0x17FFF];
                    let c = if tx & 1 != 0 { b >> 4 } else { b & 15 } as usize;
                    if c == 0 { continue }
                    r16(pal, 0x200 + (palbank * 16 + c) * 2)
                };
                let x = x as usize;
                if mode == 2 {
                    objwin[x] = true;
                    continue;
                }
                if out[x].map_or(true, |p| prio < p.prio) {
                    out[x] = Some(ObjPx { color: c & 0x7FFF, prio, semi: mode == 1 });
                }
            }
        }
    }

    pub fn render_line(&mut self, io: &[u8], vram: &[u8], pal: &[u8], oam: &[u8], y: usize) {
        let dispcnt = r16(io, 0);
        if dispcnt & 0x80 != 0 {
            self.frame[y * W..(y + 1) * W].iter_mut().for_each(|p| *p = 0x00FF_FFFF);
            return;
        }
        let mode = dispcnt & 7;
        let mut bg_on = [false; 4];
        let (bitmap, list): (bool, &[usize]) = match mode {
            0 => (false, &[0, 1, 2, 3]),
            1 => (false, &[0, 1, 2]),
            2 => (false, &[2, 3]),
            3..=5 => (true, &[2]),
            _ => (false, &[]),
        };
        let this = &*self;
        let mut lines: [[u16; W]; 4] = [[CLEAR; W]; 4];
        for &bg in list {
            if dispcnt & (0x100 << bg) == 0 || self.layers & (1 << bg) == 0 {
                continue;
            }
            bg_on[bg] = true;
            if bitmap {
                this.affine_bg(io, vram, pal, bg, mode, &mut lines[bg]);
            } else if mode == 0 || (mode == 1 && bg < 2) {
                this.text_bg(io, vram, pal, bg, y, &mut lines[bg]);
            } else {
                this.affine_bg(io, vram, pal, bg, mode, &mut lines[bg]);
            }
        }
        let mut objs: [Option<ObjPx>; W] = [None; W];
        let mut objwin = [false; W];
        if dispcnt & 0x1000 != 0 && self.layers & 0x10 != 0 {
            Self::objects(io, vram, pal, oam, y as i32, bitmap, &mut objs, &mut objwin);
        }
        // Windows.
        let any_win = dispcnt & 0xE000 != 0;
        let inside = |h: u16, v: u16, x: usize| -> bool {
            let (x1, x2) = ((h >> 8) as usize, (h & 0xFF) as usize);
            let (y1, y2) = ((v >> 8) as usize, (v & 0xFF) as usize);
            let hx = if x1 <= x2 { x >= x1 && x < x2 } else { x >= x1 || x < x2 };
            let vy = if y1 <= y2 { y >= y1 && y < y2 } else { y >= y1 || y < y2 };
            hx && vy
        };
        let winin = r16(io, 0x48);
        let winout = r16(io, 0x4A);
        let bldcnt = r16(io, 0x50);
        let alpha = r16(io, 0x52);
        let eva = u32::from(alpha & 0x1F).min(16);
        let evb = u32::from((alpha >> 8) & 0x1F).min(16);
        let evy = u32::from(r16(io, 0x54) & 0x1F).min(16);
        let backdrop = r16(pal, 0) & 0x7FFF;
        let prios: [u8; 4] = std::array::from_fn(|i| (r16(io, 0x08 + i * 2) & 3) as u8);
        for x in 0..W {
            let mask = if any_win {
                let mut m = winout & 0x3F;
                if dispcnt & 0x8000 != 0 && objwin[x] {
                    m = (winout >> 8) & 0x3F;
                }
                if dispcnt & 0x4000 != 0 && inside(r16(io, 0x42), r16(io, 0x46), x) {
                    m = (winin >> 8) & 0x3F;
                }
                if dispcnt & 0x2000 != 0 && inside(r16(io, 0x40), r16(io, 0x44), x) {
                    m = winin & 0x3F;
                }
                m
            } else {
                0x3F
            };
            // The top two layers: (colour, layer id 0-3 BG / 4 OBJ / 5 backdrop).
            let mut top: [(u16, u8); 2] = [(backdrop, 5), (backdrop, 5)];
            let mut found = 0;
            let mut semi = false;
            'p: for p in 0..4u8 {
                if let Some(o) = objs[x] {
                    if o.prio == p && mask & 0x10 != 0 {
                        top[found] = (o.color, 4);
                        if found == 0 {
                            semi = o.semi;
                        }
                        found += 1;
                        if found == 2 {
                            break 'p;
                        }
                    }
                }
                for bg in 0..4 {
                    if bg_on[bg] && prios[bg] == p && mask & (1 << bg) != 0 && lines[bg][x] != CLEAR {
                        top[found] = (lines[bg][x], bg as u8);
                        found += 1;
                        if found == 2 {
                            break 'p;
                        }
                    }
                }
            }
            let (a, la) = top[0];
            let (b, lb) = top[1];
            let first = bldcnt & (1 << la) != 0;
            let second = bldcnt & (0x100 << lb) != 0;
            let blend = |a: u16, b: u16| -> u16 {
                let mut out = 0u16;
                for s in [0, 5, 10] {
                    let ca = u32::from((a >> s) & 31);
                    let cb = u32::from((b >> s) & 31);
                    out |= (((ca * eva + cb * evb) >> 4).min(31) as u16) << s;
                }
                out
            };
            let color = if semi && second {
                blend(a, b)
            } else if mask & 0x20 != 0 && first {
                match (bldcnt >> 6) & 3 {
                    1 if second => blend(a, b),
                    2 => {
                        let mut out = 0;
                        for s in [0, 5, 10] {
                            let c = u32::from((a >> s) & 31);
                            out |= ((c + ((31 - c) * evy >> 4)) as u16) << s;
                        }
                        out
                    }
                    3 => {
                        let mut out = 0;
                        for s in [0, 5, 10] {
                            let c = u32::from((a >> s) & 31);
                            out |= ((c - (c * evy >> 4)) as u16) << s;
                        }
                        out
                    }
                    _ => a,
                }
            } else {
                a
            };
            self.frame[y * W + x] = to_xrgb(color);
        }
        // The affine reference points step by PB/PD once a line.
        for i in 0..2 {
            self.ref_x[i] += i32::from(r16(io, 0x22 + i * 0x10) as i16);
            self.ref_y[i] += i32::from(r16(io, 0x26 + i * 0x10) as i16);
        }
    }
}
