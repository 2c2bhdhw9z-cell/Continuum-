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
//!     starts, and a fetcher restart that costs a whole tile fetch;
//!   - the **picture registers as the hardware sees them**: a CPU write lands on the dot of its
//!     M-cycle the console lands it on, some in two steps (bus.rs `landing`, `Ppu::land`).
//!
//! Written from the hardware behaviour: the Mealybug Tearoom PPU documentation (which says which
//! fetch step samples which register) and Pan Docs, following the same model SameBoy implements
//! (MIT; read, not copied - see .kiro/notes/docs/CBOY_EXACT_DRAWER.md). Each piece names the behaviour it
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

/// Dots at the start of mode 3, from the dot it shows in STAT, before the fetcher's first step:
/// the line's first fetch begins on the sixth dot after that one. SameBoy's, and measured rather
/// than taken on trust: with it, every SCX, palette and WX write in the 24 Game Boy Mealybug tests
/// lands with the drawer's position, LCD x, fetcher step, object fetch, window and FIFO exactly
/// where SameBoy's are (scripts/sameboy/compare-landings.py; .kiro/notes/docs/CBOY_EXACT_DRAWER.md).
/// The bus hands the drawer each write on the dot the hardware does (`Bus::landing`), so there is
/// no correction on this side.
const DELAY: u16 = 6;

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
    /// WX 166 on a DMG left the window running as the line ended, so the next line starts with
    /// it already active (see `fifo_finish`).
    win_carry: bool,
    /// The Color: WX matched the position mid-fetch, so a read is skipped and a byte repeats.
    wx_glitch: bool,
    /// The tile set the last bitplane address was worked out in (LCDC bit 4 then), and the last
    /// byte read from the object tile area, which a Color's bitplane read glitches to when tile
    /// select is set on its dot (`bitplane_read`). The byte outlives the line, so it is carried
    /// over.
    last_tileset: bool,
    sel_glitch_data: u8,
    /// The row worked out at the tile-number step, which a CGB D or later keeps for the two
    /// bitplane address steps (`fetcher_y`).
    y: u8,
    /// One colour-0 pixel is handed to the LCD instead of the FIFO's next.
    insert_blank: bool,
    /// The window was switched off while its own first fetch ran, so its colour-0 pixel is
    /// suppressed for the rest of the line.
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
        let f = &mut self.fifo;
        let carry = f.win_carry;
        let sel_glitch_data = f.sel_glitch_data;
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
        f.sel_glitch_data = sel_glitch_data;
        // WX 166 on a DMG left the window running as the line before ended, so this line starts
        // with it already active, and with its first tile counted as fetched back then.
        f.win = carry;
        f.win_tile_x = u8::from(carry);
        // The FIFO starts the line holding eight junk pixels. They are dropped before the LCD
        // takes anything, and how many of the first real tile's pixels follow them out is what
        // the low three bits of SCX do.
        f.bg_len = 8;
        let height: i16 = if self.lcdc_seen() & 0x04 != 0 { 16 } else { 8 };
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
            self.ly.wrapping_add(self.seen(0x42))
        }
    }

    fn vram_at(&self, addr: u16) -> u8 {
        self.vram[usize::from(addr) & 0x3FFF]
    }

    /// One step of a CPU write to a picture register reaching the picture hardware, on the dot
    /// the bus says it lands (`Bus::landing`). From here until the bus `settle`s it, the drawer
    /// sees what landed instead of what the register holds.
    pub fn land(&mut self, reg: u8, step: Land) {
        let before = self.seen(reg);
        let now = match step {
            Land::Value(v) | Land::Wx(v) | Land::TileSel(v) => v,
            Land::Or(v) => before | v,
            Land::Hold => before,
            // A Game Boy's first LCDC step, a dot before the whole value: the bits the fetcher
            // samples (tile map, tile select, object size) are new already, while the two the
            // pixel output stage reads - background enable and objects - are still old, and the
            // window enable is the old AND the new, so switching the window off arrives a dot
            // before switching it on. Measured on the Game Boy pictures: SameBoy's own first step
            // (the old value with only the new background enable) misses six of them, and passes
            // them with this one (.kiro/notes/docs/CBOY_EXACT_DRAWER.md).
            Land::LcdcFirst(v) => {
                let mut first = (v & !0x23) | (before & 0x03) | (before & v & 0x20);
                // At the line's first pixel, clearing either of the output stage's two bits -
                // background or objects - reaches it at this first step already. SameBoy has
                // this for objects; for the background it is the one pixel at x=0 that
                // m3_lcdc_bg_en_change blanks on the lines its sprite sits at X=2, where the
                // write's first step lands exactly as the LCD takes its first pixel, and both
                // Game Boy units Mealybug photographed show it (.kiro/notes/docs/CBOY_EXACT_DRAWER.md).
                if self.fifo.active && self.fifo.pos == 0 {
                    first &= v | !0x03;
                }
                // Clearing objects while an object is being fetched does too, because the
                // object fetcher reads that bit as well (SameBoy).
                if v & 0x02 == 0 && self.fifo.active && self.during_object_fetch() {
                    first &= !0x02;
                }
                first
            }
        };
        self.landed.reg[usize::from(reg & 0x0F)] = Some(now);
        if step != Land::Hold {
            self.trace_land(reg, now, before);
        }
        match step {
            Land::Wx(_) => self.landed.wx_fresh = true,
            Land::TileSel(_) => self.landed.tile_sel_glitch = true,
            _ => {}
        }
        if !self.fifo.active {
            return;
        }
        if reg == 0x40 {
            // On a Game Boy, clearing objects while an object is being fetched aborts the fetch
            // outright: the object is never mixed into the FIFO, and the dots the rest of the
            // fetch would have taken are not spent, so mode 3 ends earlier. The object stays
            // queued, and is dropped when the LCD passes its X, because with objects off no new
            // fetch can start.
            if !self.colour_console() && self.during_object_fetch() && before & 0x02 != 0 && now & 0x02 == 0 {
                self.fifo.obj_fetch = false;
                self.fifo.obj_step = 0;
            }
            // Switching the window off while its own first fetch is running suppresses the
            // colour-0 pixel `push_dot` would otherwise insert, for the rest of the line.
            if before & 0x20 != 0 && now & 0x20 == 0 && self.fifo.win_fetching {
                self.fifo.no_blank = true;
            }
        }
    }

    /// The register holds what the picture hardware has seen: the write has fully landed.
    pub fn settle(&mut self, reg: u8) {
        self.landed.reg[usize::from(reg & 0x0F)] = None;
    }

    /// A picture register as the exact drawer sees it right now (for the tests).
    pub fn drawer_sees(&self, reg: u8) -> u8 {
        self.seen(reg)
    }

    /// A picture register ($FF40-$FF4B) as the picture hardware sees it on this dot.
    fn seen(&self, reg: u8) -> u8 {
        if let Some(v) = self.landed.reg[usize::from(reg & 0x0F)] {
            return v;
        }
        match reg {
            0x40 => self.lcdc,
            0x42 => self.scy,
            0x43 => self.scx,
            0x47 => self.bgp,
            0x48 => self.obp0,
            0x49 => self.obp1,
            0x4A => self.wy,
            0x4B => self.wx,
            _ => 0xFF,
        }
    }

    /// The object-fetching state machine, which is what clearing objects interrupts, is running
    /// from the dot the object's X matched until its last read has happened (SameBoy's
    /// `during_object_fetch`).
    fn during_object_fetch(&self) -> bool {
        self.fifo.obj_fetch
    }

    /// The console is a Color (or an Advance), whichever mode the cartridge runs it in. What the
    /// hardware does follows this; what the cartridge's mode changes (the palettes, the tile
    /// attributes, object priority) follows `cgb`.
    fn colour_console(&self) -> bool {
        self.cgb_hardware || self.cgb
    }

    /// A CGB D or later, or an Advance (`ppu::CgbRev`).
    fn newer_colour(&self) -> bool {
        self.colour_console() && self.cgb_rev != super::CgbRev::C
    }

    fn wx_seen(&self) -> u8 {
        self.seen(0x4B)
    }

    fn lcdc_seen(&self) -> u8 {
        self.seen(0x40)
    }

    fn scx_seen(&self) -> u8 {
        self.seen(0x43)
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
        if !self.colour_console() {
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
                self.fifo.y = y;
                let x = if win {
                    self.fifo.win_tile_x
                } else if self.fifo.pos.wrapping_add(16) < 8 {
                    // The line's first tile, fetched before the position counter has caught up.
                    self.scx_seen() >> 3
                } else {
                    // The Color reads the position one dot earlier, except during an object fetch.
                    let early = u16::from(self.colour_console() && !self.fifo.obj_fetch);
                    ((u16::from(self.scx_seen()) + u16::from(self.fifo.pos) + 8 - early) / 8) as u8
                };
                self.fifo.tile_addr = map + u16::from(x & 31) + u16::from(y / 8) * 32;
                self.fifo.state += 1;
            }
            B_R => {
                if !self.fifo.wx_glitch {
                    self.fifo.tile = self.vram_at(self.fifo.tile_addr);
                    // The Color reads the tile number and its attribute byte on the same dot.
                    if self.colour_console() {
                        self.fifo.attr = self.vram_at(0x2000 + self.fifo.tile_addr);
                    }
                }
                self.fifo.state += 1;
            }
            LO_A | HI_A => {
                self.update_wx_glitch();
                let high = self.fifo.state == HI_A;
                // A CGB D or later (and an Advance) reads SCY only at the tile-number step, so a
                // write between the steps cannot mix two rows of a tile (the Mealybug PPU
                // documentation, "SCY"; SameBoy caches the row the same way).
                let y = if self.newer_colour() { self.fifo.y } else { self.fetcher_y() };
                // LCDC bit 4 (tile data select) is sampled on both bitplane address dots, so a
                // write between them takes one bitplane from each of two tile sets.
                self.fifo.last_tileset = self.lcdc_seen() & 0x10 != 0;
                let mut addr = if self.fifo.last_tileset {
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
                    self.fifo.data[0] = self.bitplane_read();
                }
                self.fifo.state += 1;
            }
            HI_R => {
                let glitch = self.fifo.wx_glitch;
                if glitch {
                    self.fifo.data[1] = self.fifo.data[0];
                } else {
                    self.fifo.data[1] = self.bitplane_read();
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

    /// A bitplane read. On a Color (CPU CGB C and earlier), changing tile select on the very dot
    /// of the read glitches it (the Mealybug PPU documentation, "TILE_SEL (bit 4)"):
    ///   - cleared on the read (its address was worked out with tile select set): the fetcher gets
    ///     the tile number instead, unless the tile is in the half of tile memory both settings
    ///     share (SameBoy's `data_for_tile_sel_glitch` has this half);
    ///   - set on the read: the fetcher gets the last byte the PPU read from the object tile area -
    ///     an object's row, or a background tile's fetched with tile select set - which is what
    ///     the documentation calls the bitplane data of the most recently drawn sprite, or of the
    ///     most recently drawn tile as when tile select was last reset. SameBoy glitches on a
    ///     set only in double speed, and keeps every background read; with both that way,
    ///     `m3_lcdc_tile_sel_change`, `_change2`, `m3_lcdc_tile_sel_win_change` and `_win_change2`
    ///     miss by 8, 186, 8 and 179 pixels, and with the documentation's rule all four pass.
    /// The documentation's third source, the read in progress when the previous line's fetch was
    /// cut off at its end, no picture here needs, so it is not modelled. Nor is its CPU CGB D
    /// variant (bitplane 1 read from bitplane 0's address, SameBoy's `cgb_d_glitch`): Mealybug's
    /// photos of a CGB D running `m3_lcdc_tile_sel_change` and `_win_change` show the same picture
    /// as the CGB C's, which this rule draws and SameBoy's D variant misses by 520 and 260 pixels.
    fn bitplane_read(&mut self) -> u8 {
        let read = self.vram_at(self.fifo.data_addr);
        let kept = self.fifo.sel_glitch_data;
        if self.fifo.last_tileset {
            self.fifo.sel_glitch_data = read;
        }
        if !self.landed.tile_sel_glitch {
            return read;
        }
        if !self.fifo.last_tileset {
            return kept;
        }
        if self.fifo.tile & 0x80 == 0 {
            self.fifo.tile
        } else {
            read
        }
    }

    /// The fetcher's push: eight pixels enter the background FIFO, but only once it is empty.
    fn push_dot(&mut self) {
        if self.fifo.bg_len > 0 {
            return;
        }
        // A DMG with the WY match made but the window switched off puts a single colour-0 pixel
        // into the FIFO when WX lands on the position: the window's start without its tiles.
        if !self.colour_console() && self.wy_hit && self.lcdc_seen() & 0x20 == 0 && !self.fifo.no_blank {
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
            // The first byte's row address, and LCDC bit 2 with it, is worked out the dot before
            // the byte is read, as a background bitplane's is; the second byte's is worked out
            // again on its own read dot. So a height change can land between the two and mix two
            // rows into one. Measured on the four pictures that change the height mid-fetch: only
            // this pair passes all four. Both on the read dot (SameBoy's) misses
            // cgb:m3_lcdc_obj_size_change_scx by SameBoy's 15 pixels, and both a dot early misses
            // the two Game Boy ones (.kiro/notes/docs/CBOY_EXACT_DRAWER.md).
            3 => self.fifo.obj_addr = self.obj_row_addr(),
            4 => {
                self.fifo.obj_data[0] = self.vram_at(self.fifo.obj_addr);
                self.fifo.sel_glitch_data = self.fifo.obj_data[0];
            }
            6 => {
                self.fifo.obj_addr = self.obj_row_addr();
                self.fifo.obj_data[1] = self.vram_at(self.fifo.obj_addr + 1);
                self.fifo.sel_glitch_data = self.fifo.obj_data[1];
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
                start = pos == 0xF9 || (pos == START && self.scx_seen() & 7 != 0) || (0xF1..=FIRST).contains(&pos);
            } else if u16::from(self.wx_seen()) < 166 + u16::from(self.colour_console()) {
                if self.wx_seen() == pos.wrapping_add(7) {
                    start = true;
                } else if !self.colour_console() && self.wx_seen() == pos.wrapping_add(6) && !self.landed.wx_fresh {
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
                extra = self.wx_seen() == 0 && self.scx_seen() & 7 != 0 && !self.colour_console();
            } else if !self.colour_console() && self.wx_seen() == 166 && self.wx_seen() == pos.wrapping_add(7) {
                // WX 166 is one past the last position a DMG's window can start at, but the row
                // counter still advances on the dot the position reaches it.
                self.window_line = self.window_line.wrapping_add(1);
            }
        }
        // While the window runs, WX landing on the position again hands the LCD one colour-0
        // pixel and holds the FIFO back a dot. A Color does it too in Game Boy mode: the photos
        // of m3_wx_4_change_sprites on CPU CGB C and D show the pixel, where SameBoy leaves it out
        // on every Color but for WX 0. Nothing photographs a Color in colour mode doing this, so
        // that keeps SameBoy's rule.
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
        if (self.lcdc_seen() & 0x02 != 0 || self.colour_console()) && self.fifo.objs.last().is_some_and(|o| o.x == 0) {
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
            let scx = self.scx_seen();
            if pos & 7 == scx & 7 {
                self.fifo.pos = FIRST;
            } else if self.fifo.win_fetching && pos & 7 == 6 && scx & 7 == 7 {
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
                let pal = self.seen(0x48 + o.pal);
                let s = (pal >> (o.c * 2)) & 3;
                (self.mono_colour(true, o.pal, s), s, 1 + o.pal)
            }
        } else if self.cgb {
            (Self::cgb_colour(&self.bg_pal, usize::from(bg.pal) * 4 + usize::from(c)), 0, 0)
        } else {
            let s = (self.seen(0x47) >> (c * 2)) & 3;
            (self.mono_colour(false, 0, s), s, 0)
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
        if (self.lcdc_seen() & 0x02 != 0 || self.colour_console()) && self.fifo.objs.last().is_some_and(|o| o.x == self.obj_match_x()) {
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
                let s = self.seen(0x47) & 3;
                (if self.cgb { Self::cgb_colour(&self.bg_pal, 0) } else { self.mono_colour(false, 0, s) }, s, 0)
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
        // WX 166 on a DMG starts the window during HBlank, where the position is briefly 160
        // again: the next line begins with the window already running, from its second tile
        // (the first counts as fetched here) and on a further row.
        self.fifo.win_carry = !self.colour_console() && self.wy_hit && self.lcdc_seen() & 0x20 != 0 && self.wx_seen() == 166;
        if self.fifo.win_carry {
            self.window_line = self.window_line.wrapping_add(1);
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

    /// Prints the drawer's state on the dot a picture-register write lands, when the `trace`
    /// cargo feature is on (never in a build players get). With
    /// scripts/sameboy/compare-landings.py that is how this drawer is checked against SameBoy's,
    /// dot for dot.
    pub fn trace_land(&self, reg: u8, now: u8, before: u8) {
        #[cfg(feature = "trace")]
        if TRACE.load(std::sync::atomic::Ordering::Relaxed) && self.ly < 144 {
            let f = &self.fifo;
            eprintln!(
                "L FF{reg:02X}={now:02X} line={:3} pos={:4} lcd_x={:3} fetch={} objs={} during_obj={} wx_trig={} bgfifo={} old={before:02X} active={} obj_step={}",
                self.ly, f.pos as i8, f.lcd_x, f.state, f.objs.len(), u8::from(self.during_object_fetch()), u8::from(f.win), f.bg_len, f.active, f.obj_step
            );
        }
        #[cfg(not(feature = "trace"))]
        let _ = (reg, now, before);
    }

    pub fn write_bgp(&mut self, v: u8) {
        self.bgp = v;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs the exact drawer on line 0 of a Color in Game Boy mode until the fetcher is about to
    /// read the line's first bitplane 0 (the address worked out with tile select set), lands
    /// a write clearing tile select there, and returns the byte the read gave.
    fn read_with_tile_select_cleared(tile: u8, glitch: bool) -> u8 {
        let mut p = Ppu { exact: true, cgb_hardware: true, lcdc: 0x80 | 0x10 | 0x01, ..Ppu::default() };
        p.vram[0x1800..0x1C00].fill(tile);
        // Every bitplane byte in tile memory is $AA, so the tile number stands out.
        p.vram[..0x1800].fill(0xAA);
        for _ in 0..456 {
            if p.fifo.active && p.fifo.state == LO_R {
                break;
            }
            p.tick();
        }
        assert_eq!(p.fifo.state, LO_R, "the line's first bitplane read is next");
        if glitch {
            p.land(0x40, Land::TileSel(0x80 | 0x01));
        }
        p.tick();
        p.fifo.data[0]
    }

    /// On a Color, clearing tile select on the very dot of a bitplane read hands the fetcher the
    /// tile number instead, for a tile in the half of tile memory only tile select's set position
    /// reaches; otherwise the read is the read.
    #[test]
    fn a_colour_reads_the_tile_number_when_tile_select_is_cleared_on_the_read() {
        assert_eq!(read_with_tile_select_cleared(0x15, false), 0xAA);
        assert_eq!(read_with_tile_select_cleared(0x15, true), 0x15, "the tile number");
        assert_eq!(read_with_tile_select_cleared(0x95, true), 0xAA, "a tile both settings share");
    }

    /// Setting tile select on the dot of a read made from the other tile set hands the fetcher
    /// the last byte read from the object tile area, and a read from the other set does not
    /// replace that byte.
    #[test]
    fn a_colour_reads_the_kept_byte_when_tile_select_is_set_on_the_read() {
        let mut p = Ppu { exact: true, cgb_hardware: true, lcdc: 0x80 | 0x01, ..Ppu::default() };
        p.vram[..0x1800].fill(0xAA);
        p.fifo.sel_glitch_data = 0x5C;
        for _ in 0..456 {
            if p.fifo.active && p.fifo.state == LO_R {
                break;
            }
            p.tick();
        }
        assert_eq!(p.fifo.state, LO_R);
        p.land(0x40, Land::TileSel(0x80 | 0x10 | 0x01));
        p.tick();
        assert_eq!(p.fifo.data[0], 0x5C, "the kept byte");
        assert_eq!(p.fifo.sel_glitch_data, 0x5C, "a read from $8800-$97FF is not kept");
    }
}
