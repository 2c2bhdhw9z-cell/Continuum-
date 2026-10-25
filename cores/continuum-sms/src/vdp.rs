//! The Master System's picture chip (the 315-5124 VDP and its successors): for now its ports,
//! registers, memory, counters and interrupts, which is everything a program can see of it
//! without looking at the screen. Drawing comes next.
//!
//! Written from Charles MacDonald's "Sega Master System VDP documentation" (SMS Power,
//! Development: "VDP documentation") and the SMS Power pages on the V counter and the line
//! interrupt.

use serde::{Deserialize, Serialize};

/// CPU cycles in a line (the Z80 runs at the picture chip's clock divided by 3, and a line is 342
/// of the chip's pixel clocks, so 228 CPU cycles).
pub const LINE_CYCLES: u32 = 228;
/// Lines in a frame: 262 on a 60 Hz console, 313 on a 50 Hz one.
pub const LINES_NTSC: u16 = 262;
pub const LINES_PAL: u16 = 313;
/// Pixels in a line of the picture.
pub const WIDTH: usize = 256;

#[derive(Clone, Serialize, Deserialize)]
pub struct Vdp {
    pub vram: Vec<u8>,
    pub cram: Vec<u8>,
    pub regs: [u8; 16],
    /// The control port's address and code, and whether its first byte has come.
    pub addr: u16,
    pub code: u8,
    latch: bool,
    first: u8,
    /// What a data-port read returns: the byte read ahead of it.
    buffer: u8,
    /// Status: frame interrupt pending (bit 7), sprite overflow (6), sprite collision (5).
    pub status: u8,
    /// The line interrupt is pending.
    pub line_irq: bool,
    /// The line counter, reloaded from register 10.
    counter: u8,
    /// The line being drawn, 0 at the top of the picture.
    pub line: u16,
    pub pal: bool,
    /// The vertical scroll, taken from register 9 once a frame, at its top.
    #[serde(default)]
    vscroll: u8,
    /// The picture, 256 pixels a line, XRGB8888, as many lines as the mode shows.
    #[serde(skip)]
    pub frame: Vec<u32>,
}

impl Default for Vdp {
    fn default() -> Vdp {
        Vdp {
            vram: vec![0; 0x4000],
            cram: vec![0; 32],
            regs: [0x36, 0x80, 0xFF, 0xFF, 0xFF, 0xFF, 0xFB, 0x00, 0x00, 0x00, 0xFF, 0, 0, 0, 0, 0],
            addr: 0,
            code: 0,
            latch: false,
            first: 0,
            buffer: 0,
            status: 0,
            line_irq: false,
            counter: 0xFF,
            line: 0,
            pal: false,
            vscroll: 0,
            frame: vec![0; WIDTH * 240],
        }
    }
}

impl Vdp {
    /// Lines in a frame.
    pub fn lines(&self) -> u16 {
        if self.pal { LINES_PAL } else { LINES_NTSC }
    }

    /// The picture's height: 192 lines, or 224 or 240 in the Master System II's taller modes.
    pub fn height(&self) -> u16 {
        let m = (self.regs[0] & 0x06) | (self.regs[1] & 0x18);
        match m {
            0x16 => 224,
            0x0E => 240,
            _ => 192,
        }
    }

    /// The V counter as a program reads it (port $7E): the line, folded back where the count
    /// skips, so it never passes $FF (the jumps are the documented ones for each height).
    pub fn v_counter(&self) -> u8 {
        let l = self.line;
        let (top, jump_to) = match (self.pal, self.height()) {
            (false, 224) => (0xEA, 0xE5),
            (false, 240) => (0xFF, 0x00),
            (false, _) => (0xDA, 0xD5),
            (true, 224) => (0x102, 0xCA),
            (true, 240) => (0x10A, 0xD2),
            (true, _) => (0xF2, 0xBA),
        };
        if l <= top {
            l as u8
        } else {
            (jump_to + (l - top - 1)) as u8
        }
    }

    pub fn write_control(&mut self, v: u8) {
        if !self.latch {
            self.first = v;
            self.addr = (self.addr & 0x3F00) | u16::from(v);
            self.latch = true;
            return;
        }
        self.latch = false;
        self.addr = u16::from(v & 0x3F) << 8 | u16::from(self.first);
        self.code = v >> 6;
        match self.code {
            0 => {
                // A read: the first byte is fetched straight away.
                self.buffer = self.vram[usize::from(self.addr)];
                self.addr = (self.addr + 1) & 0x3FFF;
            }
            2 => {
                let r = usize::from(v & 0x0F);
                self.regs[r] = self.first;
            }
            _ => {}
        }
    }

    pub fn write_data(&mut self, v: u8) {
        self.latch = false;
        if self.code == 3 {
            let n = self.cram.len();
            self.cram[usize::from(self.addr) % n] = v;
        } else {
            self.vram[usize::from(self.addr)] = v;
        }
        // Writing the data port also leaves the byte in the read buffer.
        self.buffer = v;
        self.addr = (self.addr + 1) & 0x3FFF;
    }

    pub fn read_data(&mut self) -> u8 {
        self.latch = false;
        let v = self.buffer;
        self.buffer = self.vram[usize::from(self.addr)];
        self.addr = (self.addr + 1) & 0x3FFF;
        v
    }

    /// Reading the status port clears it and the interrupts it raised.
    pub fn read_status(&mut self) -> u8 {
        self.latch = false;
        let v = self.status | 0x1F;
        self.status = 0;
        self.line_irq = false;
        v
    }

    /// The interrupt line to the Z80: a frame interrupt with register 1 bit 5 on, or a line
    /// interrupt with register 0 bit 4 on.
    pub fn irq(&self) -> bool {
        (self.status & 0x80 != 0 && self.regs[1] & 0x20 != 0) || (self.line_irq && self.regs[0] & 0x10 != 0)
    }

    /// The start of a line: the line counter counts down through the picture and the line after
    /// it, and is reloaded from register 10 everywhere else; the frame interrupt comes on the
    /// line after the last of the picture.
    pub fn start_line(&mut self, line: u16) {
        self.line = line;
        let h = self.height();
        if line <= h {
            let (c, under) = self.counter.overflowing_sub(1);
            self.counter = c;
            if under {
                self.counter = self.regs[10];
                self.line_irq = true;
            }
        } else {
            self.counter = self.regs[10];
        }
        if line == h + 1 {
            self.status |= 0x80;
        }
        if line == 0 {
            self.vscroll = self.regs[9];
        }
        if line < h {
            self.draw_line(line);
        }
    }

    /// A colour from colour memory: the Master System's six bits, two each of blue, green and red.
    fn colour(&self, index: usize) -> u32 {
        let c = self.cram[index & 31];
        let level = |v: u8| u32::from(v & 3) * 85;
        level(c) << 16 | level(c >> 2) << 8 | level(c >> 4)
    }

    /// One line of the Master System's own mode (mode 4): the background with its scrolling,
    /// flips, second palette and priority, then up to eight sprites, with the overflow and
    /// collision flags. From Charles MacDonald's VDP documentation, "Mode 4".
    fn draw_line(&mut self, line: u16) {
        let mut row = [0u32; WIDTH];
        let backdrop = self.colour(16 + usize::from(self.regs[7] & 15));
        if self.regs[1] & 0x40 == 0 {
            row.fill(backdrop);
            self.put_row(line, &row);
            return;
        }
        let tall = self.height() > 192;
        // The background: which colour each pixel has (0-31) and whether it is in front.
        let mut bg = [0u8; WIDTH];
        let mut front = [false; WIDTH];
        let names = if tall {
            (usize::from(self.regs[2] & 0x0C) << 10) | 0x0700
        } else {
            usize::from(self.regs[2] & 0x0E) << 10
        };
        let rows = if tall { 256 } else { 224 };
        for x in 0..WIDTH {
            let col = x / 8;
            let hs = if self.regs[0] & 0x40 != 0 && line < 16 { 0 } else { self.regs[8] };
            let vs = if self.regs[0] & 0x80 != 0 && col >= 24 { 0 } else { self.vscroll };
            let sx = (x as u16).wrapping_sub(u16::from(hs)) as u8;
            let sy = (u16::from(line) + u16::from(vs)) % rows;
            let entry_at = names + (usize::from(sy / 8) * 32 + usize::from(sx / 8)) * 2;
            let entry = u16::from(self.vram[entry_at & 0x3FFF]) | u16::from(self.vram[(entry_at + 1) & 0x3FFF]) << 8;
            let tile = usize::from(entry & 0x1FF);
            let mut ty = usize::from(sy % 8);
            let mut tx = usize::from(sx % 8);
            if entry & 0x400 != 0 {
                ty = 7 - ty;
            }
            if entry & 0x200 != 0 {
                tx = 7 - tx;
            }
            let c = self.tile_pixel(tile, ty, tx);
            bg[x] = c | if entry & 0x800 != 0 { 16 } else { 0 };
            front[x] = entry & 0x1000 != 0 && c != 0;
        }
        // Sprites: the first eight on the line, in table order (the first drawn wins).
        let sat = usize::from(self.regs[5] & 0x7E) << 7;
        let size = if self.regs[1] & 0x02 != 0 { 16 } else { 8 };
        let zoom = if self.regs[1] & 0x01 != 0 { 2 } else { 1 };
        let mut spr = [0u8; WIDTH];
        let mut taken = [false; WIDTH];
        let mut found = 0;
        for n in 0..64 {
            let y = self.vram[sat + n];
            if y == 0xD0 && !tall {
                break;
            }
            let top = i32::from(y) + 1;
            let top = if top > 240 { top - 256 } else { top };
            let dy = i32::from(line) - top;
            if dy < 0 || dy >= size * zoom {
                continue;
            }
            if found == 8 {
                self.status |= 0x40;
                break;
            }
            found += 1;
            let x0 = i32::from(self.vram[sat + 0x80 + n * 2]) - if self.regs[0] & 0x08 != 0 { 8 } else { 0 };
            let mut tile = usize::from(self.vram[sat + 0x81 + n * 2]) | usize::from(self.regs[6] & 0x04) << 6;
            if size == 16 {
                tile &= !1;
            }
            let ty = (dy / zoom) as usize;
            let tile = tile + ty / 8;
            for px in 0..8 * zoom {
                let x = x0 + px;
                if !(0..WIDTH as i32).contains(&x) {
                    continue;
                }
                let c = self.tile_pixel(tile, ty % 8, (px / zoom) as usize);
                if c == 0 {
                    continue;
                }
                let x = x as usize;
                if taken[x] {
                    self.status |= 0x20;
                    continue;
                }
                taken[x] = true;
                spr[x] = 16 + c;
            }
        }
        for x in 0..WIDTH {
            row[x] = if taken[x] && !front[x] { self.colour(usize::from(spr[x])) } else { self.colour(usize::from(bg[x])) };
        }
        // The leftmost eight pixels in the backdrop colour, when register 0 bit 5 asks.
        if self.regs[0] & 0x20 != 0 {
            row[..8].fill(backdrop);
        }
        self.put_row(line, &row);
    }

    /// Colour 0-15 of one pixel of a tile: four bitplanes, one byte each per row.
    fn tile_pixel(&self, tile: usize, y: usize, x: usize) -> u8 {
        let at = (tile * 32 + y * 4) & 0x3FFF;
        let bit = 7 - x;
        (0..4).fold(0, |c, p| c | ((self.vram[(at + p) & 0x3FFF] >> bit) & 1) << p)
    }

    fn put_row(&mut self, line: u16, row: &[u32; WIDTH]) {
        let at = usize::from(line) * WIDTH;
        if self.frame.len() < WIDTH * 240 {
            self.frame.resize(WIDTH * 240, 0);
        }
        self.frame[at..at + WIDTH].copy_from_slice(row);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_v_counter_jumps_back_where_the_documents_say() {
        let mut v = Vdp::default();
        let seen: Vec<u8> = (0..LINES_NTSC).map(|l| { v.line = l; v.v_counter() }).collect();
        assert_eq!(seen[0xDA], 0xDA);
        assert_eq!(seen[0xDB], 0xD5);
        assert_eq!(seen[261], 0xFF, "the frame ends on $FF");
    }

    #[test]
    fn a_register_write_and_vram_round_trip() {
        let mut v = Vdp::default();
        v.write_control(0x20);
        v.write_control(0x81);
        assert_eq!(v.regs[1], 0x20);
        v.write_control(0x00);
        v.write_control(0x40 | 0x12);
        v.write_data(0xAB);
        v.write_control(0x00);
        v.write_control(0x12);
        assert_eq!(v.read_data(), 0xAB);
    }

    /// A screen with tile 1 everywhere, tile 1 a single colour-3 dot in its top-left corner,
    /// palette entry 3 bright red.
    fn dotted() -> Vdp {
        let mut v = Vdp::default();
        v.regs[0] = 0; // the left column not masked
        v.regs[1] = 0x40;
        v.regs[2] = 0xFF; // names at $3800
        v.regs[5] = 0xFF; // sprites at $3F00
        for n in 0..32 * 28 {
            v.vram[0x3800 + n * 2] = 1;
        }
        v.vram[32] = 0x80; // tile 1, row 0, bitplane 0
        v.vram[33] = 0x80; // bitplane 1: colour 3
        v.cram[3] = 0x03;
        v.vram[0x3F00] = 0xD0; // no sprites
        v
    }

    fn px(v: &Vdp, x: usize, y: usize) -> u32 {
        v.frame[y * WIDTH + x]
    }

    #[test]
    fn the_background_draws_and_scrolls() {
        let mut v = dotted();
        v.start_line(0);
        v.start_line(1);
        assert_eq!((px(&v, 0, 0), px(&v, 1, 0), px(&v, 8, 0), px(&v, 0, 1)), (0xFF0000, 0, 0xFF0000, 0));
        v.regs[8] = 3;
        v.start_line(0);
        assert_eq!((px(&v, 3, 0), px(&v, 0, 0)), (0xFF0000, 0), "scrolled three pixels right");
        v.regs[9] = 1;
        v.start_line(0);
        v.start_line(7);
        assert_eq!(px(&v, 3, 7), 0xFF0000, "and a line up (the next row's dot)");
    }

    #[test]
    fn a_sprite_draws_over_the_background_and_flags_overflow_and_collision() {
        let mut v = dotted();
        v.cram[16 + 3] = 0x30; // sprite palette entry 3: blue
        for n in 0..9 {
            v.vram[0x3F00 + n] = 9; // all on line 10
            v.vram[0x3F80 + n * 2] = 100; // all at x = 100
            v.vram[0x3F81 + n * 2] = 1;
        }
        v.vram[0x3F09] = 0xD0;
        v.start_line(10);
        assert_eq!(px(&v, 100, 10), 0x0000FF);
        v.regs[0] = 0x20;
        v.start_line(0);
        assert_eq!((px(&v, 0, 0), px(&v, 8, 0)), (v.colour(16), 0xFF0000), "the left column masked");
        assert_eq!(v.status & 0x60, 0x60, "nine on one line: overflow; overlapping: collision");
    }

    #[test]
    fn the_frame_interrupt_comes_after_the_picture_and_reading_status_clears_it() {
        let mut v = Vdp::default();
        v.regs[1] = 0x20;
        for l in 0..=192 {
            v.start_line(l);
            assert!(!v.irq(), "line {l}");
        }
        v.start_line(193);
        assert!(v.irq());
        assert_eq!(v.read_status() & 0x80, 0x80);
        assert!(!v.irq());
    }
}
