//! The "Exact (experimental)" drawer (core option cboy_renderer; Standard, `render_line` in
//! ppu.rs, is the default). Unfinished: it passes acid2, Mooneye and some of the 55 Mealybug
//! pictures, and no real game has been seen on it on a phone.
//!
//! Mode 3, dot by dot, as the hardware's own state machine:
//!
//!   - a background/window **fetcher** of seven steps. Every VRAM access takes two dots: the
//!     address is worked out on the first and the read happens on the second, so a register
//!     written between them is seen by one and not the other. Three accesses (tile number, then
//!     the two bitplanes) and then a push;
//!   - a **background FIFO** of eight pixels, refilled only when it is empty, and a separate
//!     **object FIFO** that object rows are mixed into as they are fetched;
//!   - a **position counter** that starts sixteen pixels before the first one the LCD takes. The
//!     FIFO starts the line holding eight junk pixels; dropping them is what the low three bits
//!     of SCX delay the line by;
//!   - an **object fetch** that stalls the fetcher and the LCD, which is where mode 3's extra
//!     length comes from;
//!   - the **window**, with its own tile counter, its own row counter that advances every time it
//!     starts, and a fetcher restart that costs a whole tile fetch.
//!
//! Written from the hardware behaviour: the Mealybug Tearoom PPU documentation (which says which
//! fetch step samples which register) and Pan Docs, following the same model SameBoy implements
//! (MIT; read, not copied - see docs/CBOY_EXACT_DRAWER.md). Each piece names the behaviour it
//! reproduces.

use super::*;

/// The fetcher's seven steps: `_A` works out an address, `_R` performs the read.
const B_A: u8 = 0;
const B_R: u8 = 1;
const LO_A: u8 = 2;
const LO_R: u8 = 3;
const HI_A: u8 = 4;
const HI_R: u8 = 5;
const PUSH: u8 = 6;

/// cBoy's bus performs a register write at the END of the M-cycle carrying it, where the hardware
/// performs it part-way through: every write reaches the PPU three dots late. Measured, not
/// guessed: of the offsets 0 to 5, three is the only one that makes the Mealybug pictures agree
/// (5, 7, 5, 20, 2, 2 of the 24 DMG pictures). Until bus.rs writes at the hardware's point (see
/// docs/CBOY_EXACT_DRAWER.md) the drawer runs those three dots behind the dot counter, so a
/// mid-line write still lands on the pixel it belongs to. STAT and HBlank keep their own timing
/// from `mode3_length`, so nothing the CPU can see moves.
const WRITE_BIAS: u16 = 3;

/// Dots at the start of mode 3, after it shows in STAT, before the fetcher's first step.
const DELAY: u16 = 5 + WRITE_BIAS;

/// A DMG does not write every register at the same point of the M-cycle carrying the write: WX
/// lands at the end of it, where BGP, OBP0/1, LCDC and SCX land two dots earlier. The fetcher
/// samples the early group, so the window sees WX two dots later than they do.
const WX_LAG: usize = 2;

/// The position counter's value when mode 3 begins: 16 pixels before the LCD's first one.
const START: u8 = 0xF0;
/// Where it jumps to once the line's fine scroll has been accounted for: 8 pixels before.
const FIRST: u8 = 0xF8;
/// The last value in the fine-scroll range (-9).
const LAST_DISCARD: u8 = 0xF7;

/// One pixel in a FIFO.
#[derive(Clone, Copy, Default, Debug)]
pub(super) struct Px {
    /// Colour 0-3, before any palette. 0 is transparent for an object pixel.
    c: u8,
    /// The Color's palette number (attribute bits 0-2). On the DMG an object pixel holds 0 for
    /// OBP0 and 1 for OBP1.
    pal: u8,
    /// Attribute bit 7: for a background pixel "keep priority over objects", for an object pixel
    /// "go behind the background".
    prio: bool,
    /// OAM index, for the Color's priority by index.
    idx: u8,
}

/// An object the OAM scan found on this line, with the Y and X it read then.
#[derive(Clone, Copy, Default, Debug)]
pub(super) struct Obj {
    idx: u8,
    x: u8,
    y: u8,
}

#[derive(Clone, Default, Debug)]
pub(super) struct Fifo {
    pub active: bool,
    /// Dots since mode 3 began.
    dots: u16,
    /// Where the PPU is on the line, counted as the hardware counts it (see `START`).
    pos: u8,
    /// Where the LCD is: the screen x the next pixel handed over goes to.
    lcd_x: u8,
    /// A pixel was dropped for the low three bits of SCX on this line.
    fractional: bool,
    /// The background FIFO: eight pixels, refilled only once empty.
    bg: [Px; 8],
    bg_head: u8,
    bg_len: u8,
    /// The object FIFO, as a shift register over the next eight pixels out.
    obj: [Px; 8],
    /// The fetcher's step, `B_A` to `PUSH`.
    state: u8,
    tile: u8,
    attr: u8,
    data: [u8; 2],
    tile_addr: u16,
    data_addr: u16,
    /// The window is being fetched instead of the background (WX has been reached).
    win: bool,
    /// The window's tile counter: its own X, which SCX does not touch.
    win_tile_x: u8,
    /// The window started on this dot (its first fetch is in progress).
    win_fetching: bool,
    /// The Color: WX matched the position mid-fetch, so a read is skipped and a byte repeats.
    wx_glitch: bool,
    /// One colour-0 pixel is handed to the LCD instead of the FIFO's next.
    insert_blank: bool,
    /// BGP, OBP0 and OBP1 as the output stage sees them on this dot, and as they were on the
    /// dot before (see `registers_dot`).
    pal: [u8; 3],
    pal_was: [u8; 3],
    /// LCDC as the drawer sees it on this dot, and as it was on the dot before.
    lcdc: u8,
    lcdc_was: u8,
    /// WX as the window sees it, trailing the register by `WX_LAG` dots, and whether it changed
    /// on this dot (the dot a write lands on, where the window's late start is ignored).
    wx_pipe: [u8; WX_LAG + 1],
    wx_fresh: bool,
    /// LCDC bit 5 on the dot before, and whether its colour-0 pixel is suppressed for this line.
    win_en_was: bool,
    no_blank: bool,
    /// This line's objects, reverse sorted by X: the one the LCD reaches next is the last.
    objs: Vec<Obj>,
    /// An object fetch in progress, and its dot within the fetch (0 while it waits).
    obj_fetch: bool,
    obj_step: u8,
    obj_tile: u8,
    obj_flags: u8,
    obj_addr: u16,
    obj_data: [u8; 2],
    row: Vec<u32>,
    shade: Vec<u8>,
    source: Vec<u8>,
}

impl Ppu {
    /// Mode 3 begins: scan OAM for this line's objects and reset the fetcher.
    pub(super) fn fifo_start(&mut self) {
        let (pal, wx, lcdc) = ([self.bgp, self.obp0, self.obp1], self.wx, self.lcdc);
        let f = &mut self.fifo;
        let (mut objs, row, shade, source) =
            (std::mem::take(&mut f.objs), std::mem::take(&mut f.row), std::mem::take(&mut f.shade), std::mem::take(&mut f.source));
        objs.clear();
        *f = Fifo { objs, row, shade, source, ..Fifo::default() };
        f.row.resize(W, 0);
        f.shade.resize(W, 0);
        f.source.resize(W, 0);
        f.active = true;
        f.pos = START;
        f.state = B_A;
        f.pal = pal;
        f.pal_was = pal;
        f.wx_pipe = [wx; WX_LAG + 1];
        f.lcdc = lcdc;
        f.lcdc_was = lcdc;
        f.win_en_was = lcdc & 0x20 != 0;
        // The FIFO starts the line holding eight junk pixels. They are dropped before the LCD
        // takes anything, and how many of the first real tile's pixels follow them out is what
        // the low three bits of SCX do.
        f.bg_len = 8;
        let height: i16 = if self.lcdc & 0x04 != 0 { 16 } else { 8 };
        let line = i16::from(self.ly);
        let limit = if self.no_sprite_limit { 40 } else { 10 };
        for i in 0..40u8 {
            let y = self.oam[usize::from(i) * 4];
            let x = self.oam[usize::from(i) * 4 + 1];
            let top = i16::from(y) - 16;
            if line < top || line >= top + height {
                continue;
            }
            if self.fifo.objs.len() == limit {
                break;
            }
            // Reverse sorted by X, so the object the LCD reaches next is at the end; among
            // objects sharing an X the lower OAM index is fetched first, and so wins on a DMG.
            let at = self.fifo.objs.iter().position(|o| o.x <= x).unwrap_or(self.fifo.objs.len());
            self.fifo.objs.insert(at, Obj { idx: i, x, y });
        }
    }

    /// The row of the map the fetcher is reading: the window's own row counter, or the line
    /// plus SCY. Re-read at every address step (as on a DMG and on CPU CGB C), so a write
    /// between two of them mixes two rows into one row of pixels.
    fn fetcher_y(&self) -> u8 {
        if self.fifo.win {
            self.window_line
        } else {
            self.ly.wrapping_add(self.scy)
        }
    }

    fn vram_at(&self, addr: u16) -> u8 {
        self.vram[usize::from(addr) & 0x3FFF]
    }

    /// What the drawer sees of the registers a DMG does not write all at once. Run on every dot
    /// of mode 3, before anything reads them.
    ///
    ///   - a palette register (BGP, OBP0, OBP1) reads as the old value OR the new one for the
    ///     single dot a write lands on, and as the new value from the dot after;
    ///   - LCDC lands in two halves a dot apart: the first carries only bit 0 (background
    ///     enable) of the new value, OR'd into the old register - plus a clear of bit 1
    ///     (objects) if the LCD is at the line's first pixel or an object is being fetched - and
    ///     the second the whole value. What that comes to here, measured against the reference
    ///     pictures and not assumed, is that the two bits the pixel output stage reads - bit 0
    ///     and bit 1 - land one dot after the bits the fetcher samples, both edges of a change
    ///     together (docs/CBOY_EXACT_DRAWER.md);
    ///   - WX is written at the end of its M-cycle, two dots after all of those.
    fn registers_dot(&mut self) {
        let now = [self.bgp, self.obp0, self.obp1];
        let (cgb, wx, lcdc) = (self.cgb, self.wx, self.lcdc);
        let f = &mut self.fifo;
        for i in 0..3 {
            f.pal[i] = if cgb { now[i] } else { now[i] | f.pal_was[i] };
        }
        f.pal_was = now;
        // The bits the output stage reads - bit 0 (background enable) and bit 1 (objects) - take
        // the old value for that one dot, so they reach the LCD a dot after the map and
        // tile-select bits the fetcher samples. The window enable takes the old AND the new, so
        // switching the window off reaches the PPU a dot before switching it on does. The
        // fetcher's own bits are left where cBoy's single-stage bus write puts them: moving them
        // exposes object-stall errors instead (docs/CBOY_EXACT_DRAWER.md).
        let was = f.lcdc_was;
        // The object-fetching state machine, which is what a clear of bit 1 interrupts, is
        // running from the dot the object's X matched to the dot before its last read.
        let during_obj = f.obj_fetch && f.obj_step < 6;
        let mut half = (lcdc & !0x23) | (was & 0x03) | (was & lcdc & 0x20);
        if lcdc & 0x02 == 0 && (f.pos == 0 || during_obj) {
            half &= !0x02;
        }
        f.lcdc = if cgb { lcdc } else { half };
        f.lcdc_was = lcdc;
        // On a DMG, clearing bit 1 while an object is being fetched aborts the fetch outright:
        // the object is never mixed into the FIFO, and the dots the rest of the fetch would have
        // taken are not spent, so mode 3 ends earlier. The object stays queued, and is dropped
        // when the LCD passes its X, because bit 1 being clear stops a new fetch starting.
        if !cgb && during_obj && was & 0x02 != 0 && lcdc & 0x02 == 0 {
            f.obj_fetch = false;
            f.obj_step = 0;
        }
        let seen = f.wx_pipe[0];
        f.wx_pipe.rotate_left(1);
        f.wx_pipe[WX_LAG] = wx;
        f.wx_fresh = f.wx_pipe[0] != seen;
        // Switching the window off while its own first fetch is running suppresses the colour-0
        // pixel `push_dot` would otherwise insert, for the rest of the line.
        let win_en = f.lcdc & 0x20 != 0;
        if f.win_en_was && !win_en && f.win_fetching {
            f.no_blank = true;
        }
        f.win_en_was = win_en;
    }

    /// WX as the window sees it (see `WX_LAG`).
    fn wx_seen(&self) -> u8 {
        self.fifo.wx_pipe[0]
    }

    /// LCDC as the drawer sees it (see `registers_dot`).
    fn lcdc_seen(&self) -> u8 {
        self.fifo.lcdc
    }

    fn bg_pop(&mut self) -> Px {
        let f = &mut self.fifo;
        let p = f.bg[usize::from(f.bg_head) & 7];
        f.bg_head = (f.bg_head + 1) & 7;
        f.bg_len -= 1;
        p
    }

    /// The Color: WX reached the PPU's position part-way through a fetch, which makes the fetch
    /// skip a read and repeat the byte it already has.
    fn update_wx_glitch(&mut self) {
        if !self.cgb {
            return;
        }
        if self.lcdc_seen() & 0x20 == 0 || !self.wy_hit {
            self.fifo.wx_glitch = false;
            return;
        }
        let pos = self.fifo.pos;
        self.fifo.wx_glitch = if self.wx_seen() == 0 {
            pos.wrapping_add(16) <= 8 || (pos == 0xF9 && self.fifo.fractional)
        } else {
            pos.wrapping_add(7).wrapping_add(u8::from(self.fifo.win_fetching)) == self.wx_seen()
        };
    }

    /// One step of the background fetcher (one dot).
    fn fetcher_dot(&mut self) {
        match self.fifo.state {
            B_A => {
                self.update_wx_glitch();
                // LCDC bit 5 is sampled here and nowhere else: clearing it mid-line lets the
                // window finish the tile in progress, and the background resumes at the next.
                if self.lcdc_seen() & 0x20 == 0 {
                    self.fifo.win = false;
                }
                let win = self.fifo.win;
                let map = if (self.lcdc_seen() & 0x08 != 0 && !win) || (self.lcdc_seen() & 0x40 != 0 && win) { 0x1C00u16 } else { 0x1800 };
                let y = self.fetcher_y();
                let x = if win {
                    self.fifo.win_tile_x
                } else if self.fifo.pos.wrapping_add(16) < 8 {
                    // The line's first tile, fetched before the position counter has caught up.
                    self.scx >> 3
                } else {
                    // The Color reads the position one dot earlier, except during an object fetch.
                    let early = u16::from(self.cgb && !self.fifo.obj_fetch);
                    ((u16::from(self.scx) + u16::from(self.fifo.pos) + 8 - early) / 8) as u8
                };
                self.fifo.tile_addr = map + u16::from(x & 31) + u16::from(y / 8) * 32;
                self.fifo.state += 1;
            }
            B_R => {
                if !self.fifo.wx_glitch {
                    self.fifo.tile = self.vram_at(self.fifo.tile_addr);
                    // The Color reads the tile number and its attribute byte on the same dot.
                    if self.cgb {
                        self.fifo.attr = self.vram_at(0x2000 + self.fifo.tile_addr);
                    }
                }
                self.fifo.state += 1;
            }
            LO_A | HI_A => {
                self.update_wx_glitch();
                let high = self.fifo.state == HI_A;
                let y = self.fetcher_y();
                // LCDC bit 4 (tile data select) is sampled on both bitplane address dots, so a
                // write between them takes one bitplane from each of two tile sets.
                let mut addr = if self.lcdc_seen() & 0x10 != 0 {
                    u16::from(self.fifo.tile) * 16
                } else {
                    (0x1000i32 + i32::from(self.fifo.tile as i8) * 16) as u16
                };
                if self.fifo.attr & 0x08 != 0 {
                    addr += 0x2000;
                }
                let flip = if self.fifo.attr & 0x40 != 0 { 7 } else { 0 };
                self.fifo.data_addr = addr + u16::from((y & 7) ^ flip) * 2 + u16::from(high);
                self.fifo.state += 1;
            }
            LO_R => {
                if self.fifo.wx_glitch {
                    self.fifo.data[0] = self.fifo.data[1];
                } else {
                    self.fifo.data[0] = self.vram_at(self.fifo.data_addr);
                }
                self.fifo.state += 1;
            }
            HI_R => {
                let glitch = self.fifo.wx_glitch;
                if glitch {
                    self.fifo.data[1] = self.fifo.data[0];
                } else {
                    self.fifo.data[1] = self.vram_at(self.fifo.data_addr);
                }
                // The window's tile counter advances on the last read of the fetch.
                if self.fifo.win {
                    self.fifo.win_tile_x = (self.fifo.win_tile_x + 1) & 31;
                }
                self.fifo.state = PUSH;
                if !glitch {
                    // The push is attempted on the same dot as the last read.
                    self.push_dot();
                }
            }
            _ => self.push_dot(),
        }
    }

    /// The fetcher's push: eight pixels enter the background FIFO, but only once it is empty.
    fn push_dot(&mut self) {
        if self.fifo.bg_len > 0 {
            return;
        }
        // A DMG with the WY match made but the window switched off puts a single colour-0 pixel
        // into the FIFO when WX lands on the position: the window's start without its tiles.
        if !self.cgb && self.wy_hit && self.lcdc_seen() & 0x20 == 0 && !self.fifo.no_blank {
            let logical = self.fifo.pos.wrapping_add(7);
            let logical = if logical > 167 { 0 } else { logical };
            if self.wx_seen() == logical {
                self.fifo.bg_head = 0;
                self.fifo.bg[0] = Px::default();
                self.fifo.bg_len = 1;
                return;
            }
        }
        let (lo, hi, attr) = (self.fifo.data[0], self.fifo.data[1], self.fifo.attr);
        let blank = if self.fifo.win { self.layers & 2 == 0 } else { self.layers & 1 == 0 };
        self.fifo.bg_head = 0;
        self.fifo.bg_len = 8;
        for i in 0..8usize {
            let bit = if attr & 0x20 != 0 { i } else { 7 - i } as u8;
            self.fifo.bg[i] = Px {
                c: if blank { 0 } else { ((hi >> bit) & 1) << 1 | ((lo >> bit) & 1) },
                pal: attr & 7,
                prio: attr & 0x80 != 0,
                idx: 0,
            };
        }
        self.fifo.state = B_A;
    }

    /// The X an object has to have to be fetched now. Objects at 0 are fetched while the line's
    /// discard is still running, which is why they hold the LCD up.
    fn obj_match_x(&self) -> u8 {
        let r = self.fifo.pos.wrapping_add(8);
        if r > START { 0 } else { r }
    }

    /// Reads one object's row over six dots, with the background fetcher stalled. It cannot
    /// start until the fetcher has reached its last read with pixels in the FIFO, so the wait is
    /// longer when an object lands early in a tile: that wait is most of mode 3's extra length.
    fn obj_fetch_dot(&mut self) {
        if self.fifo.obj_step == 0 {
            if self.fifo.state < HI_R || self.fifo.bg_len == 0 {
                self.fetcher_dot();
                return;
            }
            self.fifo.obj_step = 1;
        }
        // Six dots: the fetcher takes two more steps, then the object's own three reads, each
        // of which takes two dots like the fetcher's.
        match self.fifo.obj_step {
            1 => self.fetcher_dot(),
            2 => {
                self.fetcher_dot();
                let Some(o) = self.fifo.objs.last().copied() else { return };
                self.fifo.obj_tile = self.oam[usize::from(o.idx) * 4 + 2];
                self.fifo.obj_flags = self.oam[usize::from(o.idx) * 4 + 3];
            }
            4 => {
                self.fifo.obj_addr = self.obj_row_addr();
                self.fifo.obj_data[0] = self.vram_at(self.fifo.obj_addr);
            }
            6 => {
                // The row address is worked out again for the second byte, so LCDC bit 2 can
                // change the object's height between the two and mix two rows.
                self.fifo.obj_addr = self.obj_row_addr();
                self.fifo.obj_data[1] = self.vram_at(self.fifo.obj_addr + 1);
                self.obj_overlay();
                self.fifo.obj_fetch = false;
                self.fifo.obj_step = 0;
                self.fifo.objs.pop();
                return;
            }
            _ => {}
        }
        self.fifo.obj_step += 1;
    }

    /// The address of the object row on this line. LCDC bit 2 is read here, not at the OAM scan.
    fn obj_row_addr(&self) -> u16 {
        let Some(o) = self.fifo.objs.last() else { return 0 };
        let tall = self.lcdc_seen() & 0x04 != 0;
        let mask = if tall { 0x0F } else { 0x07 };
        let mut row = self.ly.wrapping_sub(o.y) & mask;
        if self.fifo.obj_flags & 0x40 != 0 {
            row ^= mask;
        }
        let tile = if tall { self.fifo.obj_tile & 0xFE } else { self.fifo.obj_tile };
        let bank = if self.cgb && self.fifo.obj_flags & 0x08 != 0 { 0x2000 } else { 0 };
        bank + u16::from(tile) * 16 + u16::from(row) * 2
    }

    /// Mixes the fetched row into the object FIFO. A pixel already there keeps its place unless
    /// it is transparent, or the Color is ranking by OAM index and this object's is lower.
    fn obj_overlay(&mut self) {
        let Some(o) = self.fifo.objs.last().copied() else { return };
        let (lo, hi, flags) = (self.fifo.obj_data[0], self.fifo.obj_data[1], self.fifo.obj_flags);
        let pal = if self.cgb { flags & 7 } else { u8::from(flags & 0x10 != 0) };
        let by_index = self.cgb && self.opri & 1 == 0;
        let idx = if by_index { o.idx } else { 0 };
        for i in 0..8usize {
            let bit = if flags & 0x20 != 0 { i } else { 7 - i } as u8;
            let c = ((hi >> bit) & 1) << 1 | ((lo >> bit) & 1);
            let t = &mut self.fifo.obj[i];
            if c != 0 && (t.c == 0 || t.idx > idx) {
                *t = Px { c, pal, prio: flags & 0x80 != 0, idx };
            }
        }
    }

    /// The window's start. Returns true when the start cost this dot outright.
    fn window_dot(&mut self) -> bool {
        let pos = self.fifo.pos;
        let mut extra = false;
        if !self.fifo.win && self.wy_hit && self.lcdc_seen() & 0x20 != 0 {
            let mut start = false;
            if self.wx_seen() == 0 {
                // WX 0 starts the window as the line's discard ends, a dot later when the low
                // bits of SCX dropped a pixel.
                start = pos == 0xF9 || (pos == START && self.scx & 7 != 0) || (0xF1..=FIRST).contains(&pos);
            } else if u16::from(self.wx_seen()) < 166 + u16::from(self.cgb) {
                if self.wx_seen() == pos.wrapping_add(7) {
                    start = true;
                } else if !self.cgb && self.wx_seen() == pos.wrapping_add(6) && !self.fifo.wx_fresh {
                    // A DMG also starts the window a pixel late, which is how it starts at all
                    // when the fine-scroll jump skipped the pixel WX names, or when the window
                    // was switched off on that pixel. Not on the dot WX itself was written.
                    start = true;
                }
            }
            if start {
                // The window's row counter advances every time the window starts, not once a
                // line: switch it off and on again and the next row is drawn on the same line.
                self.window_line = self.window_line.wrapping_add(1);
                self.fifo.win = true;
                self.fifo.win_tile_x = 0;
                self.fifo.win_fetching = true;
                // The fetcher restarts and the FIFO is thrown away: a whole tile fetch of dots.
                self.fifo.state = B_A;
                self.fifo.bg_len = 0;
                extra = self.wx_seen() == 0 && self.scx & 7 != 0 && !self.cgb;
            }
        }
        // While the window runs, WX landing on the position again hands the LCD one colour-0
        // pixel and holds the FIFO back a dot.
        if self.wx_seen() == self.fifo.pos.wrapping_add(7)
            && (!self.cgb || self.wx_seen() == 0)
            && self.fifo.win
            && !self.fifo.win_fetching
            && self.fifo.state == B_A
            && self.fifo.bg_len == 8
        {
            self.fifo.insert_blank = true;
        }
        extra
    }

    /// Hands one pixel to the LCD, if there is one to hand over.
    fn render_pixel(&mut self) {
        // An object still waiting at X 0 holds everything, the line's discard included.
        if (self.lcdc_seen() & 0x02 != 0 || self.cgb) && self.fifo.objs.last().is_some_and(|o| o.x == 0) {
            return;
        }
        if self.fifo.bg_len == 0 {
            return;
        }
        let bg = if self.fifo.insert_blank {
            self.fifo.insert_blank = false;
            Px::default()
        } else {
            self.bg_pop()
        };
        let o = self.fifo.obj[0];
        self.fifo.obj.copy_within(1..8, 0);
        self.fifo.obj[7] = Px::default();
        // The eight junk pixels at the head of the line: the counter jumps forward on the dot
        // its low three bits match SCX's, so the pixels after that point are the fine scroll.
        let pos = self.fifo.pos;
        if pos.wrapping_add(16) < 8 {
            if pos & 7 == self.scx & 7 {
                self.fifo.pos = FIRST;
            } else if self.fifo.win_fetching && pos & 7 == 6 && self.scx & 7 == 7 {
                self.fifo.pos = FIRST;
            } else if pos == LAST_DISCARD {
                // SCX moved under the comparison and it never matched: start the range again.
                self.fifo.pos = START;
                return;
            } else {
                self.fifo.fractional = true;
            }
        }
        self.fifo.win_fetching = false;
        if self.fifo.pos >= 160 {
            self.fifo.pos = self.fifo.pos.wrapping_add(1);
            return;
        }
        // Mixing. On the DMG LCDC bit 0 blanks the background at this dot, not at the fetch; on
        // the Color it only takes the background's priority over objects away.
        let mut prio = bg.prio;
        let mut c = bg.c;
        if self.lcdc_seen() & 1 == 0 {
            if self.cgb {
                prio = false;
            } else {
                c = 0;
            }
        }
        let mut draw_obj = o.c != 0 && self.lcdc_seen() & 0x02 != 0 && self.layers & 4 != 0;
        if draw_obj {
            prio |= o.prio;
        }
        if c != 0 && prio {
            draw_obj = false;
        }
        let (px, shade, source) = if draw_obj {
            if self.cgb {
                (Self::cgb_colour(&self.obj_pal, usize::from(o.pal) * 4 + usize::from(o.c)), 0, 0)
            } else {
                let pal = self.fifo.pal[1 + usize::from(o.pal)];
                let s = (pal >> (o.c * 2)) & 3;
                (SHADES[s as usize], s, 1 + o.pal)
            }
        } else if self.cgb {
            (Self::cgb_colour(&self.bg_pal, usize::from(bg.pal) * 4 + usize::from(c)), 0, 0)
        } else {
            let s = (self.fifo.pal[0] >> (c * 2)) & 3;
            (SHADES[s as usize], s, 0)
        };
        let x = usize::from(self.fifo.lcd_x);
        if x < W {
            self.fifo.row[x] = px;
            self.fifo.shade[x] = shade;
            self.fifo.source[x] = source;
            self.fifo.lcd_x += 1;
        }
        self.fifo.pos = self.fifo.pos.wrapping_add(1);
    }

    /// One dot of mode 3.
    pub(super) fn fifo_dot(&mut self) {
        if !self.fifo.active {
            return;
        }
        self.fifo.dots += 1;
        self.registers_dot();
        if self.fifo.dots <= DELAY {
            return;
        }
        if self.fifo.obj_fetch {
            self.obj_fetch_dot();
            return;
        }
        if self.window_dot() {
            return;
        }
        // Objects the LCD has gone past are dropped; one whose X has come up is fetched now.
        while self.fifo.objs.last().is_some_and(|o| o.x < self.obj_match_x()) {
            self.fifo.objs.pop();
        }
        if (self.lcdc_seen() & 0x02 != 0 || self.cgb) && self.fifo.objs.last().is_some_and(|o| o.x == self.obj_match_x()) {
            self.fifo.obj_fetch = true;
            self.fifo.obj_step = 0;
            self.obj_fetch_dot();
            return;
        }
        self.render_pixel();
        self.fetcher_dot();
        if self.fifo.pos == 160 {
            self.fifo_finish();
        }
    }

    fn fifo_finish(&mut self) {
        self.fifo.active = false;
        // The PPU and the LCD can come out of step; the LCD holds the last colour it was given.
        let mut x = usize::from(self.fifo.lcd_x);
        while x < W {
            let (px, shade, source) = if x == 0 {
                let s = self.fifo.pal[0] & 3;
                (if self.cgb { Self::cgb_colour(&self.bg_pal, 0) } else { SHADES[s as usize] }, s, 0)
            } else {
                (self.fifo.row[x - 1], self.fifo.shade[x - 1], self.fifo.source[x - 1])
            };
            self.fifo.row[x] = px;
            self.fifo.shade[x] = shade;
            self.fifo.source[x] = source;
            x += 1;
        }
        let base = usize::from(self.ly) * W;
        self.frame[base..base + W].copy_from_slice(&self.fifo.row);
        if self.shade.len() == W * H {
            self.shade[base..base + W].copy_from_slice(&self.fifo.shade);
            self.source[base..base + W].copy_from_slice(&self.fifo.source);
        }
    }

    /// The line ended with the pipeline still running (a guard; mode 0 is long enough that
    /// hardware timing cannot reach here).
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

    /// WX was written. The drawer watches the register itself (see `WX_LAG`), so nothing to do;
    /// bus.rs calls this on every WX write.
    pub fn note_wx(&mut self, _v: u8) {}

    pub fn write_bgp(&mut self, v: u8) {
        self.bgp = v;
    }
}
