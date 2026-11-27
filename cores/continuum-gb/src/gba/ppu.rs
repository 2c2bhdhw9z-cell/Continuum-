//! The GBA picture: a scanline renderer for modes 0-5 (text and affine backgrounds, the three
//! bitmap modes), sprites (regular and affine, 1D/2D tile mapping, semi-transparent, the object
//! window), windows 0/1/object/outside, mosaic, and colour effects (alpha blend, brighten,
//! darken).

use serde::{Deserialize, Serialize};

pub const W: usize = 240;
pub const H: usize = 160;
const CLEAR: u16 = 0x8000;

#[derive(Clone, Serialize, Deserialize)]
pub struct Ppu {
    /// The affine backgrounds' internal reference points (BG2, BG3), 20.8 fixed point.
    pub ref_x: [i32; 2],
    pub ref_y: [i32; 2],
    /// The affine reference point as of the start of the current vertical-mosaic block,
    /// per background: a BGxX/BGxY rewrite part-way down the screen moves the live
    /// reference, but the lines already latched for this block keep the old one.
    /// Derived state, recomputed every line: not saved, so old states still load.
    #[serde(skip)]
    pub mos_ref: [(i32, i32); 2],
    /// The first line of the block `mos_ref` was latched for, per background (-1: none yet).
    /// Not saved (see above); defaults to "no latch" so the next line re-latches.
    #[serde(skip, default = "no_mosaic_block")]
    pub mos_block: [i32; 2],
    /// Saved with the state, like the Game Boy's (deterministic replays and run-ahead).
    pub frame: Vec<u32>,
    /// Layer switches from the core options: bits 0-3 BG0-BG3, bit 4 sprites. Not saved.
    #[serde(skip, default = "all_layers")]
    pub layers: u8,
    /// Hardware's per-line object drawing budget (off draws every object; not saved).
    #[serde(skip)]
    pub obj_limit: bool,
    #[serde(skip)]
    pub no_render: bool,
    /// Window-Y inside flags: a flip-flop per window, set when VCOUNT reaches WINxV's y1
    /// and cleared when it reaches y2, never reset at the frame start — so a y2 past the
    /// last line leaves the window stuck on, the way the hardware's does. Not saved; the
    /// next render rebuilds it by walking the lines from where it left off.
    #[serde(skip)]
    pub win_y: [bool; 2],
    /// The last line `win_y` was advanced to (-1: none yet). Not saved.
    #[serde(skip)]
    pub win_line: i32,
}

fn all_layers() -> u8 {
    0xFF
}

fn no_mosaic_block() -> [i32; 2] {
    [-1; 2]
}

fn blank() -> Vec<u32> {
    vec![0; W * H]
}

impl Default for Ppu {
    fn default() -> Self {
        // The hardware's per-line sprite budget is on: 1210 cycles a line (954 while
        // DISPCNT frees H-blank for OAM access), past which the rest of the line's sprites
        // are not drawn. A frontend can still switch it off through `obj_limit`.
        Ppu { ref_x: [0; 2], ref_y: [0; 2], mos_ref: [(0, 0); 2], mos_block: [-1; 2], frame: blank(), layers: 0xFF, no_render: false, obj_limit: true, win_y: [false; 2], win_line: -1 }
    }
}

fn r16(m: &[u8], a: usize) -> u16 {
    u16::from(m[a]) | u16::from(m[a + 1]) << 8
}

fn r32(m: &[u8], a: usize) -> u32 {
    u32::from(r16(m, a)) | u32::from(r16(m, a + 2)) << 16
}

/// `to_xrgb` for every colour, worked out once (the speed path: one read a pixel instead
/// of the arithmetic; a frame uses few colours, so the entries it reads stay in the cache).
fn xrgb_table() -> &'static [u32; 32768] {
    static T: std::sync::OnceLock<Box<[u32; 32768]>> = std::sync::OnceLock::new();
    T.get_or_init(|| {
        let mut t = Box::new([0u32; 32768]);
        for (i, v) in t.iter_mut().enumerate() {
            *v = to_xrgb(i as u16);
        }
        t
    })
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
    /// This pixel came from an object with its own mosaic bit set (OAM attribute 0, bit 12).
    mosaic: bool,
}

/// MOSAIC (0x400004C): the block sizes, which the register holds minus one. A size of 1 is no
/// mosaic at all, so a zeroed register leaves every layer exactly as it was drawn.
#[derive(Clone, Copy)]
struct Mosaic {
    bg_h: usize,
    bg_v: usize,
    obj_h: usize,
    obj_v: usize,
}

impl Mosaic {
    fn new(io: &[u8]) -> Mosaic {
        let m = r16(io, 0x4C);
        Mosaic {
            bg_h: usize::from(m & 0xF) + 1,
            bg_v: usize::from((m >> 4) & 0xF) + 1,
            obj_h: usize::from((m >> 8) & 0xF) + 1,
            obj_v: usize::from((m >> 12) & 0xF) + 1,
        }
    }

    /// The background sizes, if this background has mosaic switched on in BGxCNT (bit 6).
    fn bg(&self, cnt: u16) -> (usize, usize) {
        if cnt & 0x40 != 0 { (self.bg_h, self.bg_v) } else { (1, 1) }
    }
}

/// The start of the mosaic block `v` sits in: every pixel in a block shows the block's first one.
fn block(v: usize, size: usize) -> usize {
    v - v % size
}

impl Ppu {
    /// Reloads the affine reference points from BGxX/BGxY (at VBlank, and when they are written).
    /// A new frame starts a new mosaic block, so the latch is dropped; a rewrite part-way down
    /// the screen leaves the current block's latch alone (its lines were already latched).
    pub fn reload_affine(&mut self, io: &[u8], new_frame: bool) {
        for i in 0..2 {
            let base = 0x28 + i * 0x10;
            self.ref_x[i] = ((r32(io, base) << 4) as i32) >> 4;
            self.ref_y[i] = ((r32(io, base + 4) << 4) as i32) >> 4;
        }
        if new_frame {
            self.mos_block = [-1; 2];
        }
    }

    // One scanline of one layer: the machine's own memories go in, a line of colours comes out.
    #[allow(clippy::too_many_arguments)]
    fn text_bg(&self, io: &[u8], vram: &[u8], pal: &[u8], bg: usize, y: usize, mos: Mosaic, out: &mut [u16; W]) {
        let cnt = r16(io, 0x08 + bg * 2);
        let hofs = (r16(io, 0x10 + bg * 4) & 0x1FF) as usize;
        let vofs = (r16(io, 0x12 + bg * 4) & 0x1FF) as usize;
        let size = (cnt >> 14) as usize;
        let w = if size & 1 != 0 { 512 } else { 256 };
        let h = if size & 2 != 0 { 512 } else { 256 };
        let char_base = ((cnt >> 2) & 3) as usize * 0x4000;
        let screen_base = ((cnt >> 8) & 0x1F) as usize * 0x800;
        let bpp8 = cnt & 0x80 != 0;
        // Mosaic blocks the screen coordinate, before scrolling is added: the blocks sit still on
        // the screen while the background slides through them, which is what the hardware does.
        let (mh, mv) = mos.bg(cnt);
        // The sizes are powers of two, so the wrap is a mask (the same numbers as `% w`).
        let (wmask, hmask) = (w - 1, h - 1);
        let py = (block(y, mv) + vofs) & hmask;
        let row = screen_base + (py / 256) * (w / 256) * 0x800 + ((py % 256) / 8) * 64;
        let ty_line = py % 8;
        if mh == 1 {
            // No mosaic: a tile's row at a time. Within one tile the pixels can't cross a map
            // entry or the background's wrap (both fall on multiples of 8), so each pixel comes
            // out exactly as the per-pixel loop below would make it.
            let mut x = 0usize;
            while x < W {
                let px = (x + hofs) & wmask;
                let map = (row + (px / 256) * 0x800 + ((px % 256) / 8) * 2) & 0xFFFF;
                let entry = r16(vram, map);
                let tile = (entry & 0x3FF) as usize;
                let hflip = entry & 0x400 != 0;
                let ty = if entry & 0x800 != 0 { 7 - ty_line } else { ty_line };
                let start = px % 8;
                let n = (8 - start).min(W - x);
                let o = &mut out[x..x + n];
                if bpp8 {
                    let base = char_base + tile * 64 + ty * 8;
                    for (k, o) in o.iter_mut().enumerate() {
                        let tx = if hflip { 7 - (start + k) } else { start + k };
                        let a = base + tx;
                        *o = if a >= 0x10000 {
                            CLEAR
                        } else {
                            let c = vram[a] as usize;
                            if c == 0 { CLEAR } else { r16(pal, c * 2) & 0x7FFF }
                        };
                    }
                } else if n == 8 && char_base + tile * 32 + ty * 4 + 4 <= 0x10000 {
                    // A whole tile row inside the background's memory (the usual case): its four
                    // bytes read once, the eight pixels taken out of them, the same colours as
                    // the loop below.
                    let base = char_base + tile * 32 + ty * 4;
                    let bank = (entry >> 12) as usize * 16;
                    let bits = u32::from_le_bytes([vram[base], vram[base + 1], vram[base + 2], vram[base + 3]]);
                    if bits == 0 {
                        o.fill(CLEAR);
                    } else {
                        for (k, o) in o.iter_mut().enumerate() {
                            let tx = if hflip { 7 - k } else { k };
                            let c = ((bits >> (tx * 4)) & 15) as usize;
                            *o = if c == 0 { CLEAR } else { r16(pal, (bank + c) * 2) & 0x7FFF };
                        }
                    }
                } else {
                    let base = char_base + tile * 32 + ty * 4;
                    let bank = (entry >> 12) as usize * 16;
                    for (k, o) in o.iter_mut().enumerate() {
                        let tx = if hflip { 7 - (start + k) } else { start + k };
                        let a = base + tx / 2;
                        *o = if a >= 0x10000 {
                            CLEAR
                        } else {
                            let b = vram[a];
                            let c = if tx & 1 != 0 { b >> 4 } else { b & 15 } as usize;
                            if c == 0 { CLEAR } else { r16(pal, (bank + c) * 2) & 0x7FFF }
                        };
                    }
                }
                x += n;
            }
            return;
        }
        // A map entry covers 8 pixels in a row: read it once for all of them.
        let mut last_map = usize::MAX;
        let mut entry = 0u16;
        for (x, o) in out.iter_mut().enumerate() {
            let bx = if mh == 1 { x } else { block(x, mh) };
            let px = (bx + hofs) & wmask;
            let map = (row + (px / 256) * 0x800 + ((px % 256) / 8) * 2) & 0xFFFF;
            if map != last_map {
                entry = r16(vram, map);
                last_map = map;
            }
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

    #[allow(clippy::too_many_arguments)]
    fn affine_bg(&self, io: &[u8], vram: &[u8], pal: &[u8], bg: usize, mode: u16, mos: Mosaic, out: &mut [u16; W]) {
        let i = bg - 2;
        let cnt = r16(io, 0x08 + bg * 2);
        let pa = i32::from(r16(io, 0x20 + i * 0x10) as i16);
        let pc = i32::from(r16(io, 0x24 + i * 0x10) as i16);
        let (mh, _) = mos.bg(cnt);
        // The reference point for this line: the latch taken at the start of its vertical
        // mosaic block (with mosaic off, every line is its own block, so this is the live
        // reference). A mid-frame BGxX/BGxY rewrite moves the live reference, but the latch
        // holds the block's lines on the reference they started with.
        let (rx, ry) = self.mos_ref[i];
        let frame = if r16(io, 0) & 0x10 != 0 { 0xA000 } else { 0 };
        for (x, o) in out.iter_mut().enumerate() {
            let mx = block(x, mh) as i32;
            let tx = (rx + pa * mx) >> 8;
            let ty = (ry + pc * mx) >> 8;
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

    #[allow(clippy::too_many_arguments)]
    fn objects(io: &[u8], vram: &[u8], pal: &[u8], oam: &[u8], y: i32, bitmap: bool, limit: bool,
               mos: Mosaic, out: &mut [Option<ObjPx>; W], objwin: &mut [bool; W]) {
        let one_d = r16(io, 0) & 0x40 != 0;
        // Object render cycles per line: 1210, or 954 when DISPCNT frees H-blank for OAM access.
        // A plain object costs its width; an affine one 10 plus twice its (doubled) width.
        let mut budget: i32 = if r16(io, 0) & 0x20 != 0 { 954 } else { 1210 };
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
            // An object with its own mosaic bit is drawn as it was on the first line of the
            // vertical block, so a block of lines all show that same row of the object.
            let mosaic = a0 & 0x1000 != 0;
            let my = if mosaic { block(y as usize, mos.obj_v) as i32 } else { y };
            let dy = (my - oy).rem_euclid(256);
            if dy >= bh {
                continue;
            }
            if limit {
                budget -= if affine { 10 + 2 * bw } else { w };
                if budget < 0 {
                    break;
                }
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
                    out[x] = Some(ObjPx { color: c & 0x7FFF, prio, semi: mode == 1, mosaic });
                }
            }
        }
        Self::obj_mosaic(mos.obj_h, out);
    }

    /// Horizontal object mosaic, done where the hardware does it: on the finished object line, not
    /// on each object as it is drawn. One pixel is held and repeated, and it is replaced at the
    /// start of each block, whenever either side of the pair is not a mosaic object (so the edges
    /// of a mosaic object, and plain objects beside it, stay where they are), and whenever a
    /// higher-priority object pixel turns up part-way through a block.
    fn obj_mosaic(size: usize, out: &mut [Option<ObjPx>; W]) {
        if size <= 1 {
            return;
        }
        let mut held: Option<ObjPx> = None;
        for (x, o) in out.iter_mut().enumerate() {
            let cur = *o;
            let mosaic = |p: &Option<ObjPx>| p.is_some_and(|p| p.mosaic);
            let beats = match (cur, held) {
                (Some(c), Some(h)) => c.prio < h.prio,
                _ => false,
            };
            if x % size == 0 || !mosaic(&cur) || !mosaic(&held) || beats {
                held = cur;
            }
            *o = held;
        }
    }

    /// Walks the window-Y flip-flops forward to line `y` (wrapping through VBlank, 228
    /// lines a frame): each window's flag is set on the line its WINxV y1 names and cleared
    /// on the line its y2 names, set winning when the two name the same line. A y2 the
    /// counter never reaches leaves the flag set, so the window stays on into the next
    /// frame — the hardware's offscreen-reset behaviour, not a range check.
    fn advance_win_y(&mut self, io: &[u8], y: usize) {
        let (v0, v1) = (r16(io, 0x44), r16(io, 0x46));
        let (y10, y20) = ((v0 >> 8) as i32, (v0 & 0xFF) as i32);
        let (y11, y21) = ((v1 >> 8) as i32, (v1 & 0xFF) as i32);
        let mut l = self.win_line;
        while l != y as i32 {
            l = (l + 1).rem_euclid(228);
            if l == y10 {
                self.win_y[0] = true;
            } else if l == y20 {
                self.win_y[0] = false;
            }
            if l == y11 {
                self.win_y[1] = true;
            } else if l == y21 {
                self.win_y[1] = false;
            }
        }
        self.win_line = y as i32;
    }

    /// One whole line, with the background enables as written (no enable delay): the
    /// unit tests' entry point, and what a line with no mid-line writes renders as.
    pub fn render_line(&mut self, io: &[u8], vram: &[u8], pal: &[u8], oam: &[u8], y: usize) {
        let bg_eff = r16(io, 0) & 0xF00;
        self.render_span(io, vram, pal, oam, y, 0, W, bg_eff);
    }

    /// Dots `[x0, x1)` of line `y`, with the background enables `bg_eff` (DISPCNT bits
    /// 8-11 as they stand for this line, after any enable delay) and the object data
    /// `oam` (the snapshot taken at the previous line's HBlank, for one-scanline-ahead
    /// sprite evaluation). The layer lines are computed whole and only the span is
    /// composited, so a line with no mid-line writes costs exactly one `render_line`.
    /// The affine reference points step only when the span completes the line, and the
    /// vertical-mosaic latch fires only on the first span of a block.
    pub fn render_span(&mut self, io: &[u8], vram: &[u8], pal: &[u8], oam: &[u8], y: usize, x0: usize, x1: usize, bg_eff: u16) {
        let dispcnt = r16(io, 0);
        if dispcnt & 0x80 != 0 {
            self.frame[y * W + x0..y * W + x1].iter_mut().for_each(|p| *p = 0x00FF_FFFF);
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
        let mos = Mosaic::new(io);
        // Vertical mosaic latches each affine background's reference point at the start of a
        // block of lines, so a BGxX/BGxY rewrite part-way down the screen moves the live
        // reference but not the lines already latched for this block (the old code wound the
        // live reference back by `y % mv` lines, which is wrong once a rewrite has moved it).
        for i in 0..2 {
            let (_, mv) = mos.bg(r16(io, 0x08 + (i + 2) * 2));
            let block_start = (y - y % mv) as i32;
            if self.mos_block[i] != block_start {
                self.mos_block[i] = block_start;
                self.mos_ref[i] = (self.ref_x[i], self.ref_y[i]);
            }
        }
        // The window-Y flip-flops advance once a line, on its first span.
        if self.win_line != y as i32 {
            self.advance_win_y(io, y);
        }
        let this = &*self;
        let mut lines: [[u16; W]; 4] = [[CLEAR; W]; 4];
        for &bg in list {
            if bg_eff & (0x100 << bg) == 0 || self.layers & (1 << bg) == 0 {
                continue;
            }
            bg_on[bg] = true;
            if bitmap {
                this.affine_bg(io, vram, pal, bg, mode, mos, &mut lines[bg]);
            } else if mode == 0 || (mode == 1 && bg < 2) {
                this.text_bg(io, vram, pal, bg, y, mos, &mut lines[bg]);
            } else {
                this.affine_bg(io, vram, pal, bg, mode, mos, &mut lines[bg]);
            }
        }
        let mut objs: [Option<ObjPx>; W] = [None; W];
        let mut objwin = [false; W];
        if dispcnt & 0x1000 != 0 && self.layers & 0x10 != 0 {
            Self::objects(io, vram, pal, oam, y as i32, bitmap, self.obj_limit, mos, &mut objs, &mut objwin);
        }
        // Windows.
        // Debug layer switches for WIN0, WIN1 and the object window (layers bits 5-7).
        let dispcnt = dispcnt & !(u16::from((!self.layers >> 5) & 7) << 13);
        let any_win = dispcnt & 0xE000 != 0;
        // The Y half of a window test is the flip-flop advanced above, not a range check:
        // a window whose y2 the counter never reaches stays on past its y1.
        let wy = self.win_y;
        let inside = |h: u16, wy: bool, x: usize| -> bool {
            let (x1, x2) = ((h >> 8) as usize, (h & 0xFF) as usize);
            let hx = if x1 <= x2 { x >= x1 && x < x2 } else { x >= x1 || x < x2 };
            hx && wy
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
        // The backgrounds that are on, sorted once a line by priority (lowest number first, and
        // in background order within one): the same order the pixel loop used to search in.
        let mut by_prio = [[0u8; 4]; 4];
        let mut in_prio = [0usize; 4];
        for bg in 0..4 {
            if bg_on[bg] {
                let p = prios[bg] as usize;
                by_prio[p][in_prio[p]] = bg as u8;
                in_prio[p] += 1;
            }
        }
        // Only an alpha blend (or a see-through object) looks at the layer under the top one;
        // without either, the search stops at the first layer it finds.
        let need_second = (bldcnt >> 6) & 3 == 1 || objs.iter().any(|o| o.is_some_and(|o| o.semi));
        // Every line: the layers are painted from the back to the front, one whole layer at a
        // time, in the reverse of the order the hardware's search looks in (priority 0 first;
        // at one priority the object, then the backgrounds in order). So each pixel ends with
        // the first layer that search finds on top and the next one under it, exactly the two
        // the per-pixel search used to find, windows and all.
        let mut masks = [0x3Fu8; W];
        if any_win {
            let (w0h, w1h) = (r16(io, 0x40), r16(io, 0x42));
            for x in x0..x1 {
                let m = &mut masks[x];
                let mut v = (winout & 0x3F) as u8;
                if dispcnt & 0x8000 != 0 && objwin[x] {
                    v = ((winout >> 8) & 0x3F) as u8;
                }
                if dispcnt & 0x4000 != 0 && inside(w1h, wy[1], x) {
                    v = ((winin >> 8) & 0x3F) as u8;
                }
                if dispcnt & 0x2000 != 0 && inside(w0h, wy[0], x) {
                    v = (winin & 0x3F) as u8;
                }
                *m = v;
            }
        }
        let mut top = [backdrop; W];
        let mut tid = [5u8; W];
        let mut under = [backdrop; W];
        let mut uid = [5u8; W];
        let mut semis = [false; W];
        let any_obj = objs.iter().any(|o| o.is_some());
        for p in (0..4u8).rev() {
            for &bg in by_prio[p as usize][..in_prio[p as usize]].iter().rev() {
                let line = &lines[bg as usize];
                let bit = 1u8 << bg;
                if need_second {
                    for x in x0..x1 {
                        let c = line[x];
                        if c != CLEAR && masks[x] & bit != 0 {
                            under[x] = top[x];
                            uid[x] = tid[x];
                            top[x] = c;
                            tid[x] = bg;
                            semis[x] = false;
                        }
                    }
                } else {
                    // nothing looks under the top layer on this line
                    for x in x0..x1 {
                        let c = line[x];
                        if c != CLEAR && masks[x] & bit != 0 {
                            top[x] = c;
                            tid[x] = bg;
                        }
                    }
                }
            }
            if any_obj {
                for x in x0..x1 {
                    if let Some(o) = objs[x] {
                        if o.prio == p && masks[x] & 0x10 != 0 {
                            under[x] = top[x];
                            uid[x] = tid[x];
                            top[x] = o.color;
                            tid[x] = 4;
                            semis[x] = o.semi;
                        }
                    }
                }
            }
        }
        let blend = |a: u16, b: u16| -> u16 {
            let mut out = 0u16;
            for s in [0, 5, 10] {
                let ca = u32::from((a >> s) & 31);
                let cb = u32::from((b >> s) & 31);
                out |= (((ca * eva + cb * evb) >> 4).min(31) as u16) << s;
            }
            out
        };
        let mode = (bldcnt >> 6) & 3;
        let out = &mut self.frame[y * W..(y + 1) * W];
        let lut = xrgb_table();
        if mode == 0 && !need_second {
            for x in x0..x1 {
                out[x] = lut[usize::from(top[x] & 0x7FFF)];
            }
        } else {
            for x in x0..x1 {
                let (a, la) = (top[x], tid[x]);
                let (b, lb) = (under[x], uid[x]);
                let first = bldcnt & (1 << la) != 0;
                let second = bldcnt & (0x100 << lb) != 0;
                let color = if semis[x] && second {
                    blend(a, b)
                } else if masks[x] & 0x20 != 0 && first {
                    match mode {
                        1 if second => blend(a, b),
                        2 => {
                            let mut v = 0;
                            for s in [0, 5, 10] {
                                let c = u32::from((a >> s) & 31);
                                v |= ((c + ((31 - c) * evy >> 4)) as u16) << s;
                            }
                            v
                        }
                        3 => {
                            let mut v = 0;
                            for s in [0, 5, 10] {
                                let c = u32::from((a >> s) & 31);
                                v |= ((c - (c * evy >> 4)) as u16) << s;
                            }
                            v
                        }
                        _ => a,
                    }
                } else {
                    a
                };
                out[x] = lut[usize::from(color & 0x7FFF)];
            }
        }
        // The affine reference points step by PB/PD once a line, when the span completes it.
        if x1 == W {
            for i in 0..2 {
                self.ref_x[i] += i32::from(r16(io, 0x22 + i * 0x10) as i16);
                self.ref_y[i] += i32::from(r16(io, 0x26 + i * 0x10) as i16);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mosaic 4 wide and 2 tall for backgrounds, 8 wide and 4 tall for objects. Each size is
    /// different, so a test cannot pass with the two axes, or the two layers, swapped over.
    const MOS: u16 = 0x3713;
    const BG_H: usize = 4;
    const BG_V: usize = 2;
    const OBJ_H: usize = 8;
    const OBJ_V: usize = 4;

    struct Scene {
        io: Vec<u8>,
        vram: Vec<u8>,
        pal: Vec<u8>,
        oam: Vec<u8>,
    }

    impl Scene {
        fn new() -> Scene {
            Scene { io: vec![0; 0x400], vram: vec![0; 0x18000], pal: vec![0; 0x400], oam: vec![0; 0x400] }
        }

        /// Every pixel of the screen, drawn a line at a time the way the machine does it.
        fn frame(&self, mosaic: u16) -> Vec<u32> {
            self.frame_limit(mosaic, Ppu::default().obj_limit)
        }

        /// The same picture, with the per-line sprite budget on or off.
        fn frame_limit(&self, mosaic: u16, limit: bool) -> Vec<u32> {
            let mut io = self.io.clone();
            w16(&mut io, 0x4C, mosaic);
            let mut ppu = Ppu::default();
            ppu.obj_limit = limit;
            ppu.reload_affine(&io, true);
            for y in 0..H {
                ppu.render_line(&io, &self.vram, &self.pal, &self.oam, y);
            }
            ppu.frame
        }
    }

    fn w16(m: &mut [u8], a: usize, v: u16) {
        m[a] = v as u8;
        m[a + 1] = (v >> 8) as u8;
    }

    /// What a mosaic of `mh` by `mv` must turn a plainly drawn picture into: every pixel of a
    /// block shows the pixel at the block's top-left corner.
    fn blocks(frame: &[u32], mh: usize, mv: usize) -> Vec<u32> {
        let mut out = vec![0; W * H];
        for y in 0..H {
            for x in 0..W {
                out[y * W + x] = frame[block(y, mv) * W + block(x, mh)];
            }
        }
        out
    }

    /// 64 different colours in one 8x8 tile, which every map entry of the background uses: the
    /// picture then changes at every pixel, in both directions, so blocking it is visible.
    fn tile_and_palette(s: &mut Scene, map_entries: usize) {
        for t in 0..map_entries {
            w16(&mut s.vram, t * 2, 1);
        }
        for ty in 0..8 {
            for tx in 0..8 {
                s.vram[0x4000 + 64 + ty * 8 + tx] = (ty * 8 + tx + 1) as u8;
            }
        }
        for c in 1..=64u16 {
            w16(&mut s.pal, usize::from(c) * 2, c * 37);
        }
    }

    fn text_scene() -> Scene {
        let mut s = Scene::new();
        w16(&mut s.io, 0, 0x0100); // mode 0, BG0 on
        w16(&mut s.io, 0x08, 0x0084 | 0x40); // 256 colours, tiles at 0x4000, map at 0, mosaic on
        tile_and_palette(&mut s, 32 * 32);
        s
    }

    fn rotated_scene() -> Scene {
        let mut s = Scene::new();
        w16(&mut s.io, 0, 0x0402); // mode 2, BG2 on
        w16(&mut s.io, 0x0C, 0x0004 | 0x2000 | 0x40); // tiles at 0x4000, map at 0, wrap, mosaic on
        w16(&mut s.io, 0x20, 0x0100); // PA: one texel across per pixel
        w16(&mut s.io, 0x24, 0x0040); // PC: and a quarter down, so the layer is sheared
        w16(&mut s.io, 0x26, 0x0100); // PD: one texel down per line
        tile_and_palette(&mut s, 0);
        for t in 0..16 * 16 {
            s.vram[t] = 1; // the affine map is one byte a tile
        }
        s
    }

    fn bitmap_scene() -> Scene {
        let mut s = Scene::new();
        w16(&mut s.io, 0, 0x0403); // mode 3, BG2 on
        w16(&mut s.io, 0x0C, 0x40); // mosaic on
        w16(&mut s.io, 0x20, 0x0100);
        w16(&mut s.io, 0x26, 0x0100);
        for y in 0..H {
            for x in 0..W {
                let c = ((y as u16 & 31) << 5) | (x as u16 & 31) | 0x4000;
                w16(&mut s.vram, (y * W + x) * 2, c);
            }
        }
        s
    }

    /// A 64x64 object of 256 colours, 1D tiles, every pixel a different colour, at x, y.
    fn object(s: &mut Scene, slot: usize, x: u16, y: u16, mosaic: bool) {
        w16(&mut s.oam, slot * 8, y | 0x2000 | if mosaic { 0x1000 } else { 0 });
        w16(&mut s.oam, slot * 8 + 2, x | 0xC000); // 64x64
        w16(&mut s.oam, slot * 8 + 4, 0);
        for ty in 0..64usize {
            for tx in 0..64usize {
                let t = ((ty / 8) * 8 + tx / 8) * 2;
                s.vram[0x10000 + t * 32 + (ty % 8) * 8 + tx % 8] = ((ty * 64 + tx) % 255 + 1) as u8;
            }
        }
        for c in 1..=255u16 {
            w16(&mut s.pal, 0x200 + usize::from(c) * 2, c * 37);
        }
    }

    fn object_scene() -> Scene {
        let mut s = Scene::new();
        w16(&mut s.io, 0, 0x1040); // mode 0, 1D object tiles, objects on
        object(&mut s, 0, 0, 0, true);
        s
    }

    /// Both ways of having no mosaic: sizes of one in the register, and the layers' own switches
    /// off. Each must give the plain picture, so no game that leaves MOSAIC alone is touched.
    #[test]
    fn no_mosaic_either_way_changes_nothing() {
        for (name, s) in [("text", text_scene()), ("rotated", rotated_scene()),
                          ("bitmap", bitmap_scene()), ("objects", object_scene())] {
            let mut switched_off = Scene {
                io: s.io.clone(),
                vram: s.vram.clone(),
                pal: s.pal.clone(),
                oam: s.oam.clone(),
            };
            w16(&mut switched_off.io, 0x08, r16(&s.io, 0x08) & !0x40);
            w16(&mut switched_off.io, 0x0C, r16(&s.io, 0x0C) & !0x40);
            w16(&mut switched_off.oam, 0, r16(&s.oam, 0) & !0x1000);
            assert_eq!(s.frame(0), switched_off.frame(MOS), "{name}: no mosaic either way");
        }
    }

    #[test]
    fn background_mosaic_blocks_the_pixels() {
        for (name, s) in [("text", text_scene()), ("rotated", rotated_scene()), ("bitmap", bitmap_scene())] {
            let plain = s.frame(0);
            let mosaic = s.frame(MOS);
            assert_ne!(plain, mosaic, "{name}: the mosaic did nothing");
            assert_eq!(mosaic, blocks(&plain, BG_H, BG_V), "{name}: wrong blocks");
        }
    }

    /// A BGxX/BGxY rewrite part-way down the screen moves the affine reference, but a
    /// vertical-mosaic block already latched keeps the reference it started with: the rewrite
    /// only takes effect at the next block. (Winding the live reference back by `y % mv`
    /// instead would move the block the rewrite landed in.)
    #[test]
    fn mosaic_block_keeps_its_reference_across_a_rewrite() {
        let s = rotated_scene();
        let mut io = s.io.clone();
        w16(&mut io, 0x4C, MOS); // mosaic 4 wide, 2 tall
        let mut ppu = Ppu::default();
        ppu.reload_affine(&io, true);
        for y in 0..H {
            ppu.render_line(&io, &s.vram, &s.pal, &s.oam, y);
            if y == 0 {
                // A rewrite of BG2Y eight texels down, in the middle of the first block.
                w16(&mut io, 0x2C, 0x0800);
                w16(&mut io, 0x2E, 0);
                ppu.reload_affine(&io, false);
            }
        }
        let line = |y: usize| &ppu.frame[y * W..(y + 1) * W];
        assert_eq!(line(1), line(0), "the rewrite moved the block it landed in");
        assert_ne!(line(2), line(0), "the rewrite never took effect");
    }

    #[test]
    fn object_mosaic_blocks_the_pixels() {
        let s = object_scene();
        let plain = s.frame(0);
        let mosaic = s.frame(MOS);
        assert_ne!(plain, mosaic, "the mosaic did nothing");
        assert_eq!(mosaic, blocks(&plain, OBJ_H, OBJ_V), "wrong blocks");
    }

    /// The object mosaic is applied to the finished object line, and a pixel is only carried on
    /// while both sides of the pair are mosaic objects: a plain object next to a mosaic one, and
    /// the empty screen past the end of one, come out exactly as they were drawn.
    #[test]
    fn object_mosaic_leaves_its_neighbours_alone() {
        let mut s = object_scene();
        object(&mut s, 1, 64, 0, false);
        let plain = s.frame(0);
        let mosaic = s.frame(MOS);
        let want = blocks(&plain, OBJ_H, OBJ_V);
        for y in 0..H {
            for x in 0..W {
                let i = y * W + x;
                // The mosaic object fills the left 64 columns; the plain one the next 64.
                let want = if x < 64 { want[i] } else { plain[i] };
                assert_eq!(mosaic[i], want, "pixel {x},{y}");
            }
        }
        assert_ne!(mosaic[..64], plain[..64], "the mosaic object was not blocked");
    }

    /// The sprite budget is on by default: eighteen 64-wide sprites fill a line's 1210
    /// cycles, so the nineteenth is not drawn. Switched off, every sprite is drawn.
    #[test]
    fn sprite_budget_is_on_by_default() {
        assert!(Ppu::default().obj_limit, "the budget must be on unless switched off");
        let mut s = Scene::new();
        w16(&mut s.io, 0, 0x1040); // mode 0, 1D object tiles, objects on
        // Every object tile is colour 1 throughout, so the tiles themselves cannot tell
        // one sprite from another; each sprite's own palette bank does that instead.
        s.vram[0x10000..0x18000].fill(0x11);
        for i in 0..20usize {
            // The palette bank is four bits, so banks repeat every 16 sprites; that is
            // fine here, since only sprites 18 and 19 reach pixel 200,0.
            let bank = i & 0xF;
            w16(&mut s.oam, i * 8, 0); // y 0, 16 colours
            w16(&mut s.oam, i * 8 + 2, (i * 8) as u16 | 0xC000); // 64x64 at x = i*8
            w16(&mut s.oam, i * 8 + 4, (i * 64) as u16 | (bank as u16) << 12); // tile, bank
            w16(&mut s.pal, 0x200 + bank * 32 + 2, (bank + 1) as u16);
        }
        // Pixel 200,0 is covered by sprites 18 and 19 and by no earlier one (17 ends at
        // 200), so it shows sprite 18's colour only when the budget is off: bank 2,
        // colour 1, value 3.
        let colour18 = to_xrgb(3);
        assert_eq!(s.frame_limit(0, false)[200], colour18, "budget off draws sprite 18");
        assert_eq!(s.frame_limit(0, true)[200], 0, "budget on leaves it the backdrop");
        assert_eq!(s.frame(0)[200], 0, "and the default is the budget on");
    }
}
