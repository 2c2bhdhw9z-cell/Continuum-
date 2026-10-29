//! The PRC, the Pokémon Mini's picture chip. Each frame it can draw a background of 8×8 tiles
//! (from a map in RAM at $1360, tiles anywhere in memory) and up to 24 sprites of 16×16 (from
//! RAM at $1300) into the frame buffer at $1000-$12FF, then copy that buffer to the LCD and
//! raise the copy interrupt. A rate setting lets it work only every 2nd to 12th frame. While it
//! draws or copies, the processor waits.
//!
//! Its timing, as PokeMini measures it (the emulator cMini replaces): a counter rising one step
//! every 855 cycles, drawing at step $18, copying at $39, and ending the frame at $42, about 72
//! frames a second. Registers: $2080-$208A.

use crate::minx::Bus;
use serde::{Deserialize, Serialize};

/// The counter's step per cycle, in 8.24 fixed point.
pub const STEP: u32 = 16_777_216 / 855;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Prc {
    /// The frame counter, 8.24 fixed point (its top byte reads at $208A).
    pub count: i32,
    pub bg_base: u32,
    pub spr_base: u32,
    /// 0 off, 1 copy only, 2 draw and copy.
    pub mode: u8,
    /// The rate divider's count (in $2081's high four bits) the chip works at.
    pub rate_match: u8,
    pub map_x: u8,
    pub map_y: u8,
    pub map_w: u8,
    pub map_h: u8,
    /// 1 drawn this frame, 2 copied.
    pub state: u8,
    /// The processor waits.
    pub stall: bool,
}

impl Default for Prc {
    fn default() -> Self {
        Self {
            count: 0,
            bg_base: 0,
            spr_base: 0,
            mode: 0,
            rate_match: 0x10,
            map_x: 0,
            map_y: 0,
            map_w: 0,
            map_h: 0,
            state: 0,
            stall: false,
        }
    }
}

/// What a step of the counter asks of the machine.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    Nothing,
    /// Draw the background and sprites into the frame buffer.
    Draw,
    /// Copy the frame buffer to the LCD and raise the copy interrupt.
    Copy,
    /// The frame is over; `worked` when this was a frame the chip worked in (the frame
    /// interrupt comes with it).
    Frame { worked: bool },
}

/// What a drawing needs, taken from the chip before it reads memory through the machine.
#[derive(Clone, Copy, Debug)]
pub struct Drawing {
    pub bg_base: u32,
    pub spr_base: u32,
    pub map_x: i32,
    pub map_y: i32,
    pub map_w: i32,
    /// $2080: bit 0 inverts the background, 1 draws it, 2 draws the sprites.
    pub mode: u8,
}

impl Prc {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Moves the counter on by `cycles`.
    pub fn advance(&mut self, io: &mut [u8], cycles: u32) -> Event {
        self.count = self.count.wrapping_add(STEP.wrapping_mul(cycles) as i32);
        let top = self.count as u32 & 0xFF00_0000;
        if io[0x81] & 0xF0 >= self.rate_match {
            if self.count < 0x1800_0000 {
                self.state = 0;
            } else if top == 0x1800_0000 {
                if self.state != 1 && self.mode == 2 {
                    self.stall = true;
                    self.state = 1;
                    return Event::Draw;
                }
            } else if top == 0x3900_0000 {
                if self.state != 2 && self.mode != 0 {
                    self.stall = true;
                    self.state = 2;
                    return Event::Copy;
                }
            } else if self.count >= 0x4200_0000 {
                self.stall = false;
                io[0x81] &= 0x0F;
                self.count = 0x0100_0000;
                return Event::Frame { worked: true };
            }
        } else if self.count >= 0x4200_0000 {
            io[0x81] = io[0x81].wrapping_add(0x10);
            self.count = 0x0100_0000;
            return Event::Frame { worked: false };
        }
        Event::Nothing
    }

    pub fn drawing(&self, io: &[u8]) -> Drawing {
        Drawing {
            bg_base: self.bg_base,
            spr_base: self.spr_base,
            map_x: i32::from(self.map_x),
            map_y: i32::from(self.map_y),
            map_w: i32::from(self.map_w),
            mode: io[0x80],
        }
    }

    pub fn read(&self, io: &[u8], reg: usize) -> u8 {
        match reg {
            0x80 => io[reg] & 0x3F,
            0x81 | 0x83 | 0x88 => io[reg],
            0x82 => io[reg] & 0xF8,
            0x84 | 0x89 => io[reg] & 0x1F,
            0x85 | 0x86 => io[reg] & 0x7F,
            0x87 => io[reg] & 0xC0,
            0x8A => (self.count >> 24) as u8,
            _ => 0,
        }
    }

    pub fn write(&mut self, io: &mut [u8], reg: usize, value: u8) {
        match reg {
            0x80 => {
                io[reg] = value & 0x3F;
                self.mode = if value & 0x08 == 0 {
                    0
                } else if value & 0x06 != 0 {
                    2
                } else {
                    1
                };
                (self.map_w, self.map_h) = match value & 0x30 {
                    0x00 => (12, 16),
                    0x10 => (16, 12),
                    0x20 => (24, 8),
                    _ => (24, 16),
                };
            }
            0x81 => {
                io[reg] = if io[reg] & 0x0E != value & 0x0E {
                    value & 0x0F
                } else {
                    (io[reg] & 0xF0) | (value & 0x0F)
                };
                self.rate_match = [0x20, 0x50, 0x80, 0xB0, 0x10, 0x30, 0x50, 0x70][usize::from((value & 0x0E) >> 1)];
            }
            0x82 => {
                io[reg] = value & 0xF8;
                self.bg_base = (self.bg_base & 0x1F_FF00) | u32::from(io[reg]);
            }
            0x83 => {
                io[reg] = value;
                self.bg_base = (self.bg_base & 0x1F_00F8) | u32::from(value) << 8;
            }
            0x84 => {
                io[reg] = value & 0x1F;
                self.bg_base = (self.bg_base & 0x00_FFF8) | u32::from(io[reg]) << 16;
            }
            0x85 => {
                io[reg] = value & 0x7F;
                if i32::from(io[reg]) <= i32::from(self.map_h) * 8 - 64 {
                    self.map_y = io[reg];
                }
            }
            0x86 => {
                io[reg] = value & 0x7F;
                if i32::from(io[reg]) <= i32::from(self.map_w) * 8 - 96 {
                    self.map_x = io[reg];
                }
            }
            0x87 => {
                io[reg] = value & 0xC0;
                self.spr_base = (self.spr_base & 0x1F_FF00) | u32::from(io[reg]);
            }
            0x88 => {
                io[reg] = value;
                self.spr_base = (self.spr_base & 0x1F_00C0) | u32::from(value) << 8;
            }
            0x89 => {
                io[reg] = value & 0x1F;
                self.spr_base = (self.spr_base & 0x00_FFC0) | u32::from(io[reg]) << 16;
            }
            _ => {}
        }
    }
}

impl Drawing {
    /// Draws into the frame buffer, reading and writing through the machine's memory (as the
    /// chip does: what it reads from the I/O block is what the registers give it).
    pub fn draw(&self, bus: &mut impl Bus) {
        if self.mode & 0x02 != 0 {
            self.background(bus);
        }
        if self.mode & 0x04 != 0 {
            self.sprites(bus);
        }
    }

    fn background(&self, bus: &mut impl Bus) {
        let mut out = 0x1000u32;
        let mut last = -1i32;
        let (mut top, mut bottom) = (0u32, 0u32);
        for row in 0..8 {
            let ty = row * 8 + self.map_y;
            let shift = (ty & 7) as u32;
            for col in 0..96 {
                let tx = col + self.map_x;
                let at = 0x1360 + (ty >> 3) * self.map_w + (tx >> 3);
                if at != last {
                    top = self.bg_base.wrapping_add(u32::from(bus.read(at as u32)) * 8);
                    bottom = self.bg_base.wrapping_add(u32::from(bus.read((at + self.map_w) as u32)) * 8);
                    last = at;
                }
                let column = (tx & 7) as u32;
                let upper = u32::from(bus.read(top + column)) >> shift;
                let lower = u32::from(bus.read(bottom + column)) << (8 - shift);
                let data = (upper | lower) as u8;
                bus.write(out, if self.mode & 0x01 != 0 { !data } else { data });
                out += 1;
            }
        }
    }

    fn sprites(&self, bus: &mut impl Bus) {
        let mut at = 0x1300u32 + 24 * 4;
        loop {
            at -= 1;
            let cfg = bus.read(at);
            at -= 1;
            let tile = i32::from(bus.read(at)) * 8;
            at -= 1;
            let y = i32::from(bus.read(at) & 0x7F) - 16;
            at -= 1;
            let x = i32::from(bus.read(at) & 0x7F) - 16;
            if cfg & 0x08 != 0 {
                let fx = if cfg & 0x01 != 0 { 8 } else { 0 };
                let fy = if cfg & 0x02 != 0 { 8 } else { 0 };
                self.quarter(bus, cfg, x + fx, y + fy, tile + 2, tile);
                self.quarter(bus, cfg, x + fx, y + 8 - fy, tile + 3, tile + 1);
                self.quarter(bus, cfg, x + 8 - fx, y + fy, tile + 6, tile + 4);
                self.quarter(bus, cfg, x + 8 - fx, y + 8 - fy, tile + 7, tile + 5);
            }
            if at <= 0x1300 {
                break;
            }
        }
    }

    /// One 8×8 quarter of a sprite at (x, y): `draw` and `mask` are its tiles (a mask bit of 1
    /// leaves the background showing). `cfg` bit 0 mirrors it, 1 flips it, 2 inverts it.
    fn quarter(&self, bus: &mut impl Bus, cfg: u8, mut x: i32, y: i32, draw: i32, mask: i32) {
        if !(-7..96).contains(&x) || !(-7..64).contains(&y) {
            return;
        }
        let shift = y & 7;
        let mut vaddr = 0x1000 + (y >> 3) * 96 + x;
        let fetch = |bus: &mut dyn FnMut(u32) -> u8, xc: i32| {
            let xp = if cfg & 0x01 != 0 { 7 - xc } else { xc };
            let mut data = bus(self.spr_base.wrapping_add((draw * 8 + xp) as u32));
            let mut cover = bus(self.spr_base.wrapping_add((mask * 8 + xp) as u32));
            if cfg & 0x02 != 0 {
                data = data.reverse_bits();
                cover = cover.reverse_bits();
            }
            if cfg & 0x04 != 0 {
                data = !data;
            }
            (i32::from(data), i32::from(cover))
        };
        if y >= 0 {
            for xc in 0..8 {
                if (0..96).contains(&x) {
                    let here = (vaddr + xc) as u32;
                    let v = i32::from(bus.read(here));
                    let (data, cover) = fetch(&mut |a| bus.read(a), xc);
                    let kept = v & ((cover << shift) | (0xFF >> (8 - shift)));
                    bus.write(here, (kept | ((data & !cover) << shift)) as u8);
                }
                x += 1;
            }
            x -= 8;
        }
        vaddr += 96;
        if y < 56 && shift != 0 {
            for xc in 0..8 {
                if (0..96).contains(&x) {
                    let here = (vaddr + xc) as u32;
                    let v = i32::from(bus.read(here));
                    let (data, cover) = fetch(&mut |a| bus.read(a), xc);
                    let kept = v & ((cover >> (8 - shift)) | (0xFF << shift));
                    bus.write(here, (kept | ((data & !cover) >> (8 - shift))) as u8);
                }
                x += 1;
            }
        }
    }
}
