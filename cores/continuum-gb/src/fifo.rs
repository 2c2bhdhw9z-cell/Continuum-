//! The "Exact (experimental)" drawer (core option cboy_renderer; Standard, `render_line` in
//! ppu.rs, is the default). Unfinished: it passes acid2, Mooneye and 5 of the 55 Mealybug
//! pictures, and no real game has been seen on it on a phone.
//!
//! The pixel FIFO: mode 3 dot by dot. A background/window fetcher (tile number, low byte, high
//! byte, two dots each, then push) fills an 8-pixel background FIFO; objects found by the OAM
//! scan stall output while their row is fetched and mixed into an object FIFO; one pixel leaves
//! per dot. Every register is read at the dot hardware reads it, so mid-line writes to SCX, SCY,
//! LCDC, the palettes, WX and so on land on the right pixel. Written from the public behaviour
//! descriptions (Pan Docs, the Mealybug Tearoom PPU notes); no emulator code consulted.

use super::*;

#[derive(Clone, Copy, Default, Debug)]
pub(super) struct ObjPx {
    /// Colour 0-3 (0 transparent).
    c: u8,
    /// OAM attribute byte.
    attr: u8,
    /// OAM index, for the Color's priority by index.
    idx: u8,
}

#[derive(Clone, Default, Debug)]
pub(super) struct Fifo {
    pub active: bool,
    /// Next screen x to output (0-159).
    lx: u8,
    /// Pixels still to drop at the start of the window (WX < 7).
    discard: u8,
    /// Pixels dropped at the start of the line: while fewer than SCX & 7 (read live), drop.
    dropped: u8,
    /// The first pixel has been output (the SCX drop is over).
    out_started: bool,
    /// Low 3 bits of SCX, latched at the first real background tile read. Later writes do not
    /// move this line (Mealybug m3_scx_low_3_bits).
    fine: u8,
    fine_latched: bool,
    /// DMG: a BGP write ORs with the old value for a pixel emitted on the next dot only.
    pub bgp_glitch: Option<u8>,
    bgp_glitch_dot: u16,
    bg: [(u8, u8); 16],
    bg_head: u8,
    bg_len: u8,
    obj: [ObjPx; 8],
    /// Fetcher: step 0-5 fetch, 6 waiting to push.
    step: u8,
    /// High bits of SCX, read when a background tile fetch starts.
    tile_scx: u8,
    /// Tiles fetched on this line (background) or into the window.
    fetch_x: u8,
    window: bool,
    /// Window enable was cleared. Takes effect when the next tile starts, not mid-tile.
    win_stop: bool,
    /// WX was 0 or 1, so one more tile stays the window and a re-enable must not cancel it.
    win_lock: bool,
    win_hold: u8,
    /// Window enable as of the previous dot, and a start deferred because it rose on this pixel.
    win_bit: bool,
    win_late: bool,
    /// One background pixel forced to color 0.
    win_blank: bool,
    /// Pixel x where a mid-line WX write restarts the window. Color 0 only if that pixel
    /// is fetched on the same dot as a new tile.
    wx_zero: Option<u8>,
    wx_armed: bool,
    window_drawn: bool,
    /// The first fetch of a line is thrown away.
    dummy: bool,
    tile: u8,
    attr: u8,
    lo: u8,
    hi: u8,
    /// Objects on this line (OAM indices, OAM order) and whether each has been fetched.
    objs: Vec<u8>,
    obj_count: u8,
    obj_done: u64,
    /// An object row being fetched: (slot in `objs`, dots left).
    obj_fetch: Option<(u8, u8)>,
    /// The window began at a WX below 7 this line (its first pixels are cut off).
    pub dots: u16,
    row: Vec<u32>,
    shade: Vec<u8>,
    source: Vec<u8>,
}

impl Ppu {
    /// Mode 3 begins: scan OAM for this line's objects and reset the fetcher.
    pub(super) fn fifo_start(&mut self) {
        let f = &mut self.fifo;
        let (mut objs, mut row, mut shade, mut source) =
            (std::mem::take(&mut f.objs), std::mem::take(&mut f.row), std::mem::take(&mut f.shade), std::mem::take(&mut f.source));
        objs.clear();
        objs.resize(40, 0);
        row.resize(W, 0);
        shade.resize(W, 0);
        source.resize(W, 0);
        *f = Fifo { objs, row, shade, source, ..Fifo::default() };
        f.active = true;
        f.win_bit = self.lcdc & 0x20 != 0;
        f.dummy = true;
        let height: i16 = if self.lcdc & 0x04 != 0 { 16 } else { 8 };
        let line = i16::from(self.ly);
        let limit = if self.no_sprite_limit { 40 } else { 10 };
        for i in 0..40u8 {
            let y = i16::from(self.oam[usize::from(i) * 4]) - 16;
            if line >= y && line < y + height {
                let f = &mut self.fifo;
                f.objs[usize::from(f.obj_count)] = i;
                f.obj_count += 1;
                if usize::from(f.obj_count) == limit {
                    break;
                }
            }
        }
    }

    fn bg_push(&mut self, px: [(u8, u8); 8]) {
        let f = &mut self.fifo;
        for p in px {
            let i = usize::from((f.bg_head + f.bg_len) & 15);
            f.bg[i] = p;
            f.bg_len += 1;
        }
    }

    fn bg_pop(&mut self) -> (u8, u8) {
        let f = &mut self.fifo;
        let p = f.bg[usize::from(f.bg_head)];
        f.bg_head = (f.bg_head + 1) & 15;
        f.bg_len -= 1;
        p
    }

    /// One fetcher step (called once per dot while it runs).
    fn fetcher_dot(&mut self) {
        if self.fifo.win_stop && self.fifo.step == 0 {
            if self.fifo.win_hold > 0 {
                self.fifo.win_hold -= 1;
            } else {
                self.fifo.window = false;
                self.fifo.win_stop = false;
                self.fifo.win_lock = false;
                let ahead = u16::from(self.fifo.lx) + u16::from(self.fifo.bg_len);
                self.fifo.fetch_x = (ahead as u8) >> 3;
            }
        }
        if self.fifo.step == 0 && !self.fifo.window {
            self.fifo.tile_scx = self.scx >> 3;
        }
        let step = self.fifo.step;
        match step {
            1 => {
                // Tile number (and the Color's attribute byte).
                let (map, x, y) = if self.fifo.window {
                    (if self.lcdc & 0x40 != 0 { 0x9C00u16 } else { 0x9800 }, self.fifo.fetch_x, self.window_line)
                } else {
                    (if self.lcdc & 0x08 != 0 { 0x9C00u16 } else { 0x9800 },
                     self.fifo.tile_scx.wrapping_add(self.fifo.fetch_x) & 31, self.ly.wrapping_add(self.scy))
                };
                let at = map + u16::from(y / 8) * 32 + u16::from(x & 31);
                self.fifo.tile = self.vram[(at & 0x1FFF) as usize];
                self.fifo.attr = if self.cgb { self.vram[0x2000 + (at & 0x1FFF) as usize] } else { 0 };
                // The low 3 bits of SCX are read here, on the first real background tile, and
                // not again. A write after this dot does not scroll the line.
                if !self.fifo.window && !self.fifo.dummy && !self.fifo.fine_latched {
                    self.fifo.fine = self.scx & 7;
                    self.fifo.fine_latched = true;
                }
            }
            3 | 5 => {
                let y = if self.fifo.window { self.window_line } else { self.ly.wrapping_add(self.scy) };
                let attr = self.fifo.attr;
                let row = if attr & 0x40 != 0 { 7 - y % 8 } else { y % 8 };
                let bank = usize::from(attr & 0x08 != 0);
                let addr = self.bg_tile_addr(self.fifo.tile);
                let a = bank * 0x2000 + ((addr + u16::from(row) * 2) as usize & 0x1FFF);
                if step == 3 {
                    self.fifo.lo = self.vram[a];
                } else {
                    self.fifo.hi = self.vram[a + 1];
                }
            }
            _ => {}
        }
        if self.fifo.step < 6 {
            self.fifo.step += 1;
        }
        if self.fifo.step == 6 {
            if self.fifo.dummy {
                // The line's first fetch is discarded and the same tile fetched again.
                self.fifo.dummy = false;
                self.fifo.step = 0;
                return;
            }
            if self.fifo.bg_len == 0 {
                let (lo, hi, attr) = (self.fifo.lo, self.fifo.hi, self.fifo.attr);
                let mut px = [(0u8, attr); 8];
                for (i, p) in px.iter_mut().enumerate() {
                    let bit = if attr & 0x20 != 0 { i } else { 7 - i } as u8;
                    p.0 = ((hi >> bit) & 1) << 1 | ((lo >> bit) & 1);
                }
                // On the DMG with LCDC bit 0 off the background is blank.
                if !self.cgb && self.lcdc & 1 == 0 {
                    px.iter_mut().for_each(|p| p.0 = 0);
                }
                if !self.fifo.window && self.layers & 1 == 0 || self.fifo.window && self.layers & 2 == 0 {
                    px.iter_mut().for_each(|p| p.0 = 0);
                }
                self.bg_push(px);
                self.fifo.fetch_x = self.fifo.fetch_x.wrapping_add(1);
                self.fifo.step = 0;
            }
        }
    }

    /// Fetches one object's row and mixes it into the object FIFO.
    fn obj_fetch_row(&mut self, slot: u8) {
        let i = usize::from(self.fifo.objs[usize::from(slot)]);
        let tall = self.lcdc & 0x04 != 0;
        let height: i16 = if tall { 16 } else { 8 };
        let y = i16::from(self.oam[i * 4]) - 16;
        let ox = i16::from(self.oam[i * 4 + 1]) - 8;
        let mut tile = self.oam[i * 4 + 2];
        let attr = self.oam[i * 4 + 3];
        let mut r = (i16::from(self.ly) - y).rem_euclid(16) as u8;
        if attr & 0x40 != 0 {
            r = (height as u8).wrapping_sub(1).wrapping_sub(r) & 15;
        }
        if tall {
            tile &= 0xFE;
        } else {
            r &= 7;
        }
        let bank = usize::from(self.cgb && attr & 0x08 != 0);
        let (lo, hi) = self.tile_row(bank, 0x8000 + u16::from(tile) * 16, r);
        let cgb_order = self.cgb && self.opri & 1 == 0;
        for px in 0..8i16 {
            let k = ox + px - i16::from(self.fifo.lx);
            if !(0..8).contains(&k) {
                continue;
            }
            let bit = if attr & 0x20 != 0 { px } else { 7 - px } as u8;
            let c = ((hi >> bit) & 1) << 1 | ((lo >> bit) & 1);
            let cur = self.fifo.obj[k as usize];
            let take = c != 0 && (cur.c == 0 || (cgb_order && (i as u8) < cur.idx));
            if take {
                self.fifo.obj[k as usize] = ObjPx { c, attr, idx: i as u8 };
            }
        }
    }

    /// One dot of mode 3 (or of a line still drawing after STAT shows mode 0).
    pub(super) fn fifo_dot(&mut self) {
        if !self.fifo.active {
            return;
        }
        self.fifo.dots += 1;
        // The low bits of the sideways scroll are read on this dot of the line. Delaying the
        // fetcher must not move that read.
        if self.fifo.dots == 8 && !self.fifo.fine_latched {
            self.fifo.fine = self.scx & 7;
            self.fifo.fine_latched = true;
        }
        if self.fifo.dots <= delay() {
            return;
        }
        // An object row being fetched holds everything else.
        if let Some((slot, left)) = self.fifo.obj_fetch {
            if left > 1 {
                self.fifo.obj_fetch = Some((slot, left - 1));
            } else {
                self.obj_fetch_row(slot);
                self.fifo.obj_done |= 1 << slot;
                self.fifo.obj_fetch = None;
            }
            return;
        }
        let bit = self.lcdc & 0x20 != 0;
        let just = bit && !self.fifo.win_bit;
        self.fifo.win_bit = bit;
        let at = u16::from(self.fifo.lx) + 7 == u16::from(self.wx).max(7);
        if !self.fifo.window && bit && self.wy_hit && self.fifo.discard == 0 && !self.fifo.dummy && (self.fifo.out_started || self.wx < 7 || self.scx & 7 == 0)
            && (at || self.fifo.win_late) && self.wx <= 166
        {
            if just && at && !self.fifo.win_late {
                // Enable rose on this exact pixel. The window starts on the next one.
                self.fifo.win_late = true;
            } else {
                self.fifo.win_late = false;
                self.fifo.window = true;
                self.fifo.window_drawn = true;
                self.fifo.fetch_x = 0;
                self.fifo.step = 0;
                self.fifo.bg_len = 0;
                if self.wx < 7 {
                    self.fifo.discard = 7 - self.wx;
                }
            }
        } else if self.fifo.window && !bit {
            // Turned off mid-line: finish this tile, then the background carries on.
            // WX 0 and 1 were one tile short, and turning the window back on must not erase that.
            if !self.fifo.win_stop && self.wx < 2 {
                self.fifo.win_hold = 1;
                self.fifo.win_lock = true;
            }
            self.fifo.win_stop = true;
        } else if self.fifo.win_stop && !self.fifo.win_lock && bit {
            self.fifo.win_stop = false;
        } else if !self.cgb && !self.fifo.window && !bit && self.wy_hit && self.fifo.discard == 0 && !self.fifo.dummy
            && self.fifo.step == 6 && u16::from(self.fifo.lx) + 7 == u16::from(self.wx) && self.fifo.lx % 8 == 0
        {
            // Disabled on a tile boundary at the pixel the window would have started. One color-0 pixel.
            self.fifo.win_blank = true;
        }
        self.fetcher_dot();
        // An object at this x: wait for the background fetch to have pixels, then fetch it.
        if self.fifo.bg_len > 0 && self.fifo.discard == 0 && (self.fifo.out_started || self.fifo.dropped >= self.fifo.fine) && (self.lcdc & 0x02 != 0 || self.cgb) {
            for s in 0..self.fifo.obj_count {
                if self.fifo.obj_done & (1 << s) != 0 {
                    continue;
                }
                let ox = self.oam[usize::from(self.fifo.objs[usize::from(s)]) * 4 + 1];
                if ox >= 168 {
                    self.fifo.obj_done |= 1 << s;
                    continue;
                }
                if u16::from(ox) <= u16::from(self.fifo.lx) + 8 {
                    self.fifo.obj_fetch = Some((s, 6));
                    return;
                }
            }
        }
        if self.fifo.bg_len == 0 {
            return;
        }
        if !self.fifo.out_started && !self.fifo.window {
            if self.fifo.dropped < self.fifo.fine {
                self.bg_pop();
                self.fifo.dropped += 1;
                return;
            }
            self.fifo.out_started = true;
        }
        let (mut c, attr) = self.bg_pop();
        if self.fifo.win_blank {
            self.fifo.win_blank = false;
            c = 0;
        }
        if self.fifo.wx_zero == Some(self.fifo.lx) {
            self.fifo.wx_zero = None;
            if self.fifo.step == 0 {
                c = 0;
            }
        }
        if self.fifo.discard > 0 {
            self.fifo.discard -= 1;
            return;
        }
        let o = self.fifo.obj[0];
        self.fifo.obj.copy_within(1..8, 0);
        self.fifo.obj[7] = ObjPx::default();
        let x = usize::from(self.fifo.lx);
        let cgb = self.cgb;
        let obj_on = self.lcdc & 0x02 != 0 && self.layers & 4 != 0 && o.c != 0;
        let behind = if cgb {
            self.lcdc & 1 != 0 && c != 0 && (o.attr & 0x80 != 0 || attr & 0x80 != 0)
        } else {
            o.attr & 0x80 != 0 && c != 0
        };
        let (px, shade, source) = if obj_on && !behind {
            if cgb {
                (Self::cgb_colour(&self.obj_pal, usize::from(o.attr & 7) * 4 + usize::from(o.c)), 0, 0)
            } else {
                let pal = if o.attr & 0x10 != 0 { self.obp1 } else { self.obp0 };
                let s = (pal >> (o.c * 2)) & 3;
                (SHADES[s as usize], s, 1 + u8::from(o.attr & 0x10 != 0))
            }
        } else if cgb {
            (Self::cgb_colour(&self.bg_pal, usize::from(attr & 7) * 4 + usize::from(c)), 0, 0)
        } else {
            let fresh = self.fifo.bgp_glitch_dot.abs_diff(self.dot) <= 1;
            let bgp = if fresh { self.fifo.bgp_glitch.unwrap_or(self.bgp) } else { self.bgp };
            let s = (bgp >> (c * 2)) & 3;
            (SHADES[s as usize], s, 0)
        };
        self.fifo.bgp_glitch = None;
        self.fifo.row[x] = px;
        self.fifo.shade[x] = shade;
        self.fifo.source[x] = source;
        self.fifo.lx += 1;
        if usize::from(self.fifo.lx) == W {
            self.fifo_finish();
        }
    }

    fn fifo_finish(&mut self) {
        self.fifo.active = false;
        if self.fifo.window_drawn {
            self.window_line = self.window_line.wrapping_add(1);
        }
        let base = usize::from(self.ly) * W;
        self.frame[base..base + W].copy_from_slice(&self.fifo.row);
        if self.shade.len() == W * H {
            self.shade[base..base + W].copy_from_slice(&self.fifo.shade);
            self.source[base..base + W].copy_from_slice(&self.fifo.source);
        }
    }
}

impl Ppu {
    /// The line ended with the pipeline still running (cannot happen on hardware timing; a guard).
    pub(super) fn fifo_force_end(&mut self) {
        if self.fifo.active {
            let mut n = 0;
            while self.fifo.active && n < 2000 {
                self.fifo_dot();
                n += 1;
            }
            if self.fifo.active {
                self.fifo_finish();
            }
        }
    }
}

/// Dots at the start of mode 3 before the fetcher runs.
const fn delay() -> u16 {
    5
}

impl Ppu {
    pub fn note_wx(&mut self, v: u8) {
        if self.fifo.active && self.fifo.window && v != self.wx {
            if !self.fifo.wx_armed && v >= 7 {
                let at = v - 7;
                if at >= self.fifo.lx {
                    self.fifo.wx_armed = true;
                    self.fifo.wx_zero = Some(at);
                }
            } else {
                self.fifo.wx_zero = None;
            }
        }
    }

    pub fn write_bgp(&mut self, v: u8) {
        if !self.cgb && self.fifo.active {
            self.fifo.bgp_glitch = Some(self.bgp | v);
            self.fifo.bgp_glitch_dot = self.dot;
        }
        self.bgp = v;
    }
}
