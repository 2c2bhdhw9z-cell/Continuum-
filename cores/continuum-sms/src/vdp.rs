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
