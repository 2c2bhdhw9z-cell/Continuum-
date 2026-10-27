//! MARIA, the 7800's graphics chip.
//!
//! Every visible line MARIA halts the processor and reads that line's objects into one of two
//! 160-cell line buffers (direct memory access, DMA), while the other buffer, filled on the line
//! before, goes out to the screen. What it reads comes from a display list list (DLL), which splits
//! the screen into zones of 1 to 16 lines, each with its own display list (DL) of object headers.
//! A 4-byte header gives an object's graphics address, palette, width and position; a 5-byte header
//! also sets the write mode and character (indirect) mode. Each read costs MARIA time, and the
//! processor is held for all of it: 16 clocks to start, 8 per 4-byte header, 10 per 5-byte one, 3
//! per graphics byte (6 more per character in character mode), 6 at the end of a zone and 17 more
//! when the next zone asks for a display list interrupt; past 426 clocks the processor waits for
//! the next line. These are the counts Atari's documents give and MAME 0.289 uses, which were
//! measured on real consoles with a logic analyser.

use serde::{Deserialize, Serialize};

pub const LINERAM: usize = 160;

/// The most MARIA can spend on one line's DMA (a line is 454 clocks, DMA starts at 28).
const MAX_DMA: u32 = 426;

#[derive(Clone, Serialize, Deserialize)]
pub struct Maria {
    /// The 32 colour registers as MARIA indexes them: entry 4k is the background for every k.
    pub palette: [u8; 32],
    /// The two line buffers, one after the other.
    line_ram: Vec<u8>,
    active: usize,
    write_mode: bool,
    dll: u16,
    dl: u16,
    holey: u8,
    offset: u8,
    pub vblank: u8,
    pub dma_on: bool,
    dpp: u16,
    pub wsync: bool,
    color_kill: bool,
    cwidth: bool,
    bcntl: bool,
    kangaroo: bool,
    rm: u8,
    dli: bool,
    charbase: u16,
}

impl Default for Maria {
    fn default() -> Self {
        Maria {
            palette: [0; 32],
            line_ram: vec![0; 2 * LINERAM],
            active: 0,
            write_mode: false,
            dll: 0,
            dl: 0,
            holey: 0,
            offset: 0,
            vblank: 0x80,
            dma_on: false,
            dpp: 0,
            wsync: false,
            color_kill: false,
            cwidth: false,
            bcntl: false,
            kangaroo: false,
            rm: 0,
            dli: false,
            charbase: 0,
        }
    }
}

/// What one line's DMA did, for the machine to act on.
pub struct LineResult {
    /// MARIA clocks spent (0 when DMA is off).
    pub cycles: u32,
    /// DMA ran out of time: the processor waits for the next line.
    pub overrun: bool,
    /// A display list interrupt is due now (an NMI).
    pub nmi: bool,
}

impl Maria {
    pub fn read(&self, offset: u8) -> u8 {
        match offset & 0x1F {
            0x08 => self.vblank,
            _ => 0x00,
        }
    }

    /// A write to $20-$3F. Returns true when it was WSYNC (the processor stops until the next
    /// line begins).
    pub fn write(&mut self, offset: u8, v: u8) -> bool {
        let o = (offset & 0x1F) as usize;
        if o & 3 != 0 {
            self.palette[o] = v;
        }
        match o {
            0x00 => {
                for i in 0..8 {
                    self.palette[4 * i] = v;
                }
            }
            0x04 => {
                self.wsync = true;
                return true;
            }
            0x0C => self.dpp = (self.dpp & 0x00FF) | ((v as u16) << 8),
            0x10 => self.dpp = (self.dpp & 0xFF00) | v as u16,
            0x14 => self.charbase = (v as u16) << 8,
            0x1C => {
                self.color_kill = v & 0x80 != 0;
                match (v >> 5) & 3 {
                    2 => self.dma_on = true,
                    3 => self.dma_on = false,
                    _ => {} // DMA test modes: Atari's documents say never to use them.
                }
                self.cwidth = v & 0x10 != 0;
                self.bcntl = v & 0x08 != 0;
                self.kangaroo = v & 0x04 != 0;
                self.rm = v & 0x03;
            }
            _ => {}
        }
        false
    }

    /// At the start of each line: VBLANK goes low at line 16 and high five lines before the end.
    pub fn line_start(&mut self, line: u32, lines: u32) {
        if line == 16 {
            self.vblank = 0x00;
        }
        if line == lines - 5 {
            self.vblank = 0x80;
        }
    }

    fn holey(&self, addr: u16) -> bool {
        (self.holey & 2 != 0 && addr & 0x9000 == 0x9000) || (self.holey & 1 != 0 && addr & 0x8800 == 0x8800)
    }

    /// One graphics byte into the line buffer being filled. Returns how many cells it covers.
    fn put(&mut self, data: u8, at: u8, pal: u8) -> u8 {
        let pal = pal << 2;
        let base = self.active * LINERAM;
        let buf = &mut self.line_ram[base..base + LINERAM];
        let kangaroo = self.kangaroo;
        let mut store = |pos: u8, c: u8| {
            if (c & 3 != 0 || kangaroo) && (pos as usize) < LINERAM {
                buf[pos as usize] = c;
            }
        };
        if self.write_mode {
            // 160B and 320B/D: two pixels of four bits, the palette's high bit kept.
            store(at, (pal & 0x10) | (data & 0x0C) | (data >> 6));
            store(at.wrapping_add(1), (pal & 0x10) | ((data & 0x03) << 2) | ((data & 0x30) >> 4));
            2
        } else {
            for i in 0..4u8 {
                store(at.wrapping_add(i), pal | ((data >> (6 - 2 * i)) & 0x03));
            }
            4
        }
    }

    /// This line's DMA: the zone's display list read into the line buffer, its cost added up.
    fn dma<F: FnMut(u16) -> u8>(&mut self, mem: &mut F) -> u32 {
        let mut cycles = 16u32;
        let mut dl = self.dl;
        while mem(dl.wrapping_add(1)) & 0x5F != 0 && cycles < MAX_DMA {
            let b1 = mem(dl.wrapping_add(1));
            let (gfx, width, hpos, pal, ind);
            if b1 & 0x1F == 0 {
                let b3 = mem(dl.wrapping_add(3));
                gfx = ((mem(dl.wrapping_add(2)) as u16) << 8) | mem(dl) as u16;
                width = ((b3 ^ 0xFF) & 0x1F) as u16 + 1;
                hpos = mem(dl.wrapping_add(4));
                pal = b3 >> 5;
                self.write_mode = b1 & 0x80 != 0;
                ind = b1 & 0x20 != 0;
                dl = dl.wrapping_add(5);
                cycles += 10;
            } else {
                gfx = ((mem(dl.wrapping_add(2)) as u16) << 8) | mem(dl) as u16;
                width = ((b1 ^ 0xFF) & 0x1F) as u16 + 1;
                hpos = mem(dl.wrapping_add(3));
                pal = b1 >> 5;
                ind = false;
                dl = dl.wrapping_add(4);
                cycles += 8;
            }
            let mut at = hpos;
            for x in 0..width {
                if cycles >= MAX_DMA {
                    break;
                }
                if ind {
                    let c = mem(gfx.wrapping_add(x));
                    let data_addr = (self.charbase | c as u16).wrapping_add((self.offset as u16) << 8);
                    if self.holey(data_addr) {
                        continue;
                    }
                    cycles += 3;
                    if self.cwidth {
                        let n = self.put(mem(data_addr), at, pal);
                        at = at.wrapping_add(n);
                        let n = self.put(mem(data_addr.wrapping_add(1)), at, pal);
                        at = at.wrapping_add(n);
                        cycles += 6;
                    } else {
                        let n = self.put(mem(data_addr), at, pal);
                        at = at.wrapping_add(n);
                        cycles += 3;
                    }
                } else {
                    let data_addr = gfx.wrapping_add(x).wrapping_add((self.offset as u16) << 8);
                    if self.holey(data_addr) {
                        continue;
                    }
                    let n = self.put(mem(data_addr), at, pal);
                    at = at.wrapping_add(n);
                    cycles += 3;
                }
            }
        }
        // The last line of a zone costs more: shutting down, and the interrupt when the next zone
        // asks for one.
        if self.offset == 0 {
            cycles += 6;
            if mem(self.dll.wrapping_add(3)) & 0x80 != 0 {
                cycles += 17;
            }
        }
        cycles
    }

    /// Reads the display list list entry at `self.dll`.
    fn load_zone<F: FnMut(u16) -> u8>(&mut self, mem: &mut F) {
        self.dl = ((mem(self.dll.wrapping_add(1)) as u16) << 8) | mem(self.dll.wrapping_add(2)) as u16;
        let header = mem(self.dll);
        self.offset = header & 0x0F;
        self.holey = (header & 0x60) >> 5;
        self.dli = header & 0x80 != 0;
    }

    /// MARIA's work for one line, at the moment its DMA starts (seven processor cycles into the
    /// line). Fills `row` (320 pixels, MARIA colour values) with the line that goes out now.
    pub fn line<F: FnMut(u16) -> u8>(&mut self, line: u32, lines: u32, mem: &mut F, row: &mut [u8]) -> LineResult {
        let mut res = LineResult { cycles: 0, overrun: false, nmi: false };
        if line == 16 && self.dma_on {
            self.dll = self.dpp;
            self.load_zone(mem);
        }
        if line > 15 && line < lines - 5 {
            if self.dma_on {
                let c = self.dma(mem);
                res.cycles = c;
                if c >= MAX_DMA {
                    res.overrun = true;
                    self.wsync = true;
                }
            }
            self.output(row);
        }
        if line > 16 && line < lines - 5 && self.dma_on {
            if self.offset == 0 {
                self.dll = self.dll.wrapping_add(3);
                self.load_zone(mem);
            } else {
                self.offset -= 1;
            }
        }
        if self.dli {
            res.nmi = true;
            self.dli = false;
        }
        res
    }

    /// Swaps the line buffers and sends out the one filled on the line before, in the read mode
    /// the control register picks, then clears it for the next line's DMA.
    fn output(&mut self, row: &mut [u8]) {
        self.active ^= 1;
        let base = self.active * LINERAM;
        let buf = &mut self.line_ram[base..base + LINERAM];
        let pal = &self.palette;
        for (i, cell) in buf.iter_mut().enumerate() {
            let c = *cell;
            let (a, b) = match self.rm {
                0 | 1 => (pal[c as usize], pal[c as usize]),
                2 => (
                    pal[((c & 0x10) | (c & 0x02) | ((c >> 3) & 1)) as usize],
                    pal[((c & 0x10) | ((c << 1) & 0x02) | ((c >> 2) & 1)) as usize],
                ),
                _ => (pal[((c & 0x1C) | (c & 0x02)) as usize], pal[((c & 0x1C) | ((c << 1) & 0x02)) as usize]),
            };
            let (a, b) = if self.color_kill { (a & 0x0F, b & 0x0F) } else { (a, b) };
            if 2 * i + 1 < row.len() {
                row[2 * i] = a;
                row[2 * i + 1] = b;
            }
            *cell = 0;
        }
    }
}
