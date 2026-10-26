//! Fujitsu's MB88 family of 4-bit microcontrollers (MB8842, MB8843, MB8844 and kin): the "Namco
//! custom chips" 50xx, 51xx, 53xx and 54xx on Galaga's, Dig Dug's and Xevious's boards are these,
//! each with Namco's program in its internal ROM (the player's set holds a dump of it).
//!
//! A 4-bit accumulator A, the pointer registers X (3 bits) and Y (4 bits) that address the nibble
//! RAM at X:Y, a 6-bit program counter PC inside a page PA of 64 bytes, a four-entry return stack,
//! the status flag ST that every conditional jump and call tests (and that nearly every instruction
//! sets), zero, carry, timer-overflow (V) and serial-full (S) flags, an 8-bit timer (TH:TL) that
//! counts every 32 cycles or on the TC pin, a 4-bit serial register, and the ports: K (in), O (out,
//! through a PLA, 8 bits), P (out), R0-R3 (in and out, 4 bits each). Every instruction byte takes
//! one cycle, and an interrupt (external, timer or serial, each enabled by a bit of the port
//! enable register) calls 02h, 04h or 06h, saving the flags with the return address.
//!
//! Written from the behaviour of the chip as MAME's mb88 core models it, which is the reference
//! this core's tests compare against instruction for instruction (`tests/mb88_mame.rs`): our own
//! Rust, no MAME code.

use serde::{Deserialize, Serialize};

/// What the chip is wired to.
pub trait Pins {
    /// The K port (4 bits in).
    fn read_k(&mut self) -> u8;
    /// The O port: `value` on the bits of `mask` (one half at a time, through the PLA).
    fn write_o(&mut self, value: u8, mask: u8);
    /// The P port (4 bits out).
    fn write_p(&mut self, value: u8);
    /// R0-R3 (4 bits each, in and out).
    fn read_r(&mut self, port: usize) -> u8;
    fn write_r(&mut self, port: usize, value: u8);
    /// The serial input pin.
    fn read_si(&mut self) -> bool {
        false
    }
}

const INT_SERIAL: u8 = 0x01;
const INT_TIMER: u8 = 0x02;
const INT_EXTERNAL: u8 = 0x04;
/// The internal timer counts once every this many cycles.
const TIMER_PRESCALE: u32 = 32;
/// The serial port shifts once every this many cycles while it runs.
const SERIAL_PRESCALE: u32 = 1;
/// After this many shifts with nobody reading, the serial port stops (as MAME's does).
const SERIAL_GIVE_UP: u32 = 1000;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mb88 {
    pub pc: u8,
    pub pa: u8,
    pub stack: [u16; 4],
    pub si: u8,
    pub a: u8,
    pub x: u8,
    pub y: u8,
    pub st: bool,
    pub zf: bool,
    pub cf: bool,
    pub vf: bool,
    pub sf: bool,
    /// The /IRQ pin's level (as a logic level, high is asserted).
    pub irq_line: bool,
    /// The port enable register: bit 2 external, 1 timer, 0 serial interrupts; 4-5 the serial
    /// port; 6 the TC pin counting; 7 the internal timer.
    pub pio: u8,
    pub th: u8,
    pub tl: u8,
    tp: u32,
    pub sb: u8,
    sb_count: u32,
    serial_on: bool,
    serial_div: u32,
    /// The TC pin's level.
    tc: bool,
    pub o: u8,
    pending: u8,
    in_irq: bool,
    /// Program and data address bits (MB8843/4: 10 and 6; MB8842: 11 and 7).
    program_bits: u8,
    data_bits: u8,
    pub ram: Vec<u8>,
    /// Cycles run since reset.
    pub cycles: u64,
}

impl Mb88 {
    /// A chip with `program_bits` of program address and `data_bits` of RAM address.
    pub fn new(program_bits: u8, data_bits: u8) -> Mb88 {
        let mut c = Mb88 {
            pc: 0,
            pa: 0,
            stack: [0; 4],
            si: 0,
            a: 0,
            x: 0,
            y: 0,
            st: true,
            zf: false,
            cf: false,
            vf: false,
            sf: false,
            irq_line: false,
            pio: 0,
            th: 0,
            tl: 0,
            tp: 0,
            sb: 0,
            sb_count: 0,
            serial_on: false,
            serial_div: 0,
            tc: false,
            o: 0,
            pending: 0,
            in_irq: false,
            program_bits,
            data_bits,
            ram: vec![0; 1 << data_bits],
            cycles: 0,
        };
        c.reset();
        c
    }

    /// The reset pin: registers and flags cleared (ST set), the stack, timer and ports' enables
    /// too. RAM, the IRQ pin's level and the O latch keep what they held.
    pub fn reset(&mut self) {
        self.pc = 0;
        self.pa = 0;
        self.stack = [0; 4];
        self.si = 0;
        self.a = 0;
        self.x = 0;
        self.y = 0;
        self.st = true;
        self.zf = false;
        self.cf = false;
        self.vf = false;
        self.sf = false;
        self.pio = 0;
        self.th = 0;
        self.tl = 0;
        self.tp = 0;
        self.sb = 0;
        self.sb_count = 0;
        self.serial_on = false;
        self.pending = 0;
        self.in_irq = false;
    }

    /// The program address the next instruction is fetched from.
    pub fn address(&self) -> u16 {
        u16::from(self.pa) << 6 | u16::from(self.pc)
    }

    /// The /IRQ pin: an external interrupt is latched on its rising (logic) edge when enabled.
    pub fn set_irq(&mut self, on: bool) {
        if !self.irq_line && on && self.pio & INT_EXTERNAL != 0 {
            self.pending |= INT_EXTERNAL;
        }
        self.irq_line = on;
    }

    /// The TC pin: the timer counts on its falling edge when enabled.
    pub fn set_tc(&mut self, on: bool) {
        if self.tc && !on && self.pio & 0x40 != 0 {
            self.count_timer();
        }
        self.tc = on;
    }

    fn count_timer(&mut self) {
        self.tl = (self.tl + 1) & 0x0F;
        if self.tl == 0 {
            self.th = (self.th + 1) & 0x0F;
            if self.th == 0 {
                self.vf = true;
                self.pending |= INT_TIMER;
            }
        }
    }

    fn set_pio(&mut self, new: u8) {
        if (self.pio ^ new) & 0x30 != 0 {
            // 20h runs the serial port; 00h stops it (10h and 30h aren't used by any board).
            self.serial_on = new & 0x30 == 0x20;
            self.serial_div = 0;
        }
        self.pio = new;
    }

    #[inline]
    fn op(&self, rom: &[u8], at: u16) -> u8 {
        let mask = (1usize << self.program_bits) - 1;
        rom.get(usize::from(at) & mask).copied().unwrap_or(0)
    }

    #[inline]
    fn ea(&self) -> usize {
        (usize::from(self.x) << 4 | usize::from(self.y)) & ((1 << self.data_bits) - 1)
    }

    #[inline]
    fn rd(&self, at: usize) -> u8 {
        self.ram[at & (self.ram.len() - 1)] & 0x0F
    }

    #[inline]
    fn wr(&mut self, at: usize, v: u8) {
        let n = self.ram.len() - 1;
        self.ram[at & n] = v & 0x0F;
    }

    #[inline]
    fn inc_pc(&mut self) {
        self.pc += 1;
        if self.pc >= 0x40 {
            self.pc = 0;
            self.pa = self.pa.wrapping_add(1);
        }
    }

    /// The O port through an 8-bit PLA: the carry picks the half, A is written to it.
    fn write_pla(&mut self, pins: &mut impl Pins, index: u8) {
        let shift = if index & 0x10 != 0 { 4 } else { 0 };
        let mask = 0x0F << shift;
        self.o = (self.o & !mask) | ((index << shift) & mask);
        pins.write_o(self.o, mask);
    }

    /// Cycles pass: the timer and serial port run, and a pending interrupt is taken.
    fn burn(&mut self, pins: &mut impl Pins, cycles: u32) {
        self.cycles += u64::from(cycles);
        if self.pio & 0x80 != 0 {
            self.tp += cycles;
            while self.tp >= TIMER_PRESCALE {
                self.tp -= TIMER_PRESCALE;
                self.count_timer();
            }
        }
        if self.serial_on {
            self.serial_div += cycles;
            while self.serial_div >= SERIAL_PRESCALE {
                self.serial_div -= SERIAL_PRESCALE;
                self.sb_count += 1;
                if self.sb_count >= SERIAL_GIVE_UP {
                    self.serial_on = false;
                }
                if !self.sf {
                    self.sb = self.sb >> 1 | if pins.read_si() { 8 } else { 0 };
                    if self.sb_count >= 4 {
                        self.sf = true;
                        self.pending |= INT_SERIAL;
                    }
                }
            }
        }
        if !self.in_irq && self.pending & self.pio != 0 {
            self.in_irq = true;
            let back = self.address() | u16::from(self.cf) << 15 | u16::from(self.zf) << 14 | u16::from(self.st) << 13;
            self.stack[usize::from(self.si)] = back;
            self.si = (self.si + 1) & 3;
            let due = self.pending & self.pio;
            self.pc = if due & INT_EXTERNAL != 0 {
                0x02
            } else if due & INT_TIMER != 0 {
                0x04
            } else {
                0x06
            };
            self.pa = 0;
            self.st = true;
            self.pending = 0;
            self.burn(pins, 3);
        }
    }

    /// Runs one instruction; returns its cycles (an interrupt taken after it adds 3).
    pub fn step(&mut self, rom: &[u8], pins: &mut impl Pins) -> u32 {
        let start = self.cycles;
        let opcode = self.op(rom, self.address());
        self.inc_pc();
        let mut len = 1;
        // ST after a carry out of bit 3 (or a borrow): clear on carry, set otherwise.
        let st_c = |v: u8| v & 0x10 == 0;
        match opcode {
            0x00 => self.st = true,
            0x01 => {
                let i = u8::from(self.cf) << 4 | self.a;
                self.write_pla(pins, i);
                self.st = true;
            }
            0x02 => {
                pins.write_p(self.a);
                self.st = true;
            }
            0x03 => {
                pins.write_r(usize::from(self.y & 3), self.a);
                self.st = true;
            }
            0x04 => {
                self.y = self.a;
                self.st = true;
            }
            0x05 => {
                self.th = self.a;
                self.st = true;
            }
            0x06 => {
                self.tl = self.a;
                self.st = true;
            }
            0x07 => {
                self.sb = self.a;
                self.st = true;
            }
            0x08 => {
                let y = self.y + 1;
                self.st = st_c(y);
                self.y = y & 0x0F;
                self.zf = self.y == 0;
            }
            0x09 => {
                let ea = self.ea();
                let v = self.rd(ea) + 1;
                self.st = st_c(v);
                let v = v & 0x0F;
                self.zf = v == 0;
                self.wr(ea, v);
            }
            0x0A => {
                let ea = self.ea();
                self.wr(ea, self.a);
                let y = self.y + 1;
                self.st = st_c(y);
                self.y = y & 0x0F;
                self.zf = self.y == 0;
            }
            0x0B => {
                let ea = self.ea();
                let v = self.rd(ea);
                self.wr(ea, self.a);
                self.a = v;
                self.zf = self.a == 0;
                self.st = true;
            }
            0x0C => {
                let v = self.a << 1 | u8::from(self.cf);
                self.st = st_c(v);
                self.cf = !self.st;
                self.a = v & 0x0F;
                self.zf = self.a == 0;
            }
            0x0D => {
                self.a = self.rd(self.ea());
                self.zf = self.a == 0;
                self.st = true;
            }
            0x0E => {
                let v = self.rd(self.ea()) + self.a + u8::from(self.cf);
                self.st = st_c(v);
                self.cf = !self.st;
                self.a = v & 0x0F;
                self.zf = self.a == 0;
            }
            0x0F => {
                self.a &= self.rd(self.ea());
                self.zf = self.a == 0;
                self.st = !self.zf;
            }
            0x10 => {
                // DAA: the carry out of a BCD add goes to C and clears ST. Z isn't changed.
                if self.cf || self.a > 9 {
                    self.a += 6;
                }
                self.st = st_c(self.a);
                self.cf = !self.st;
                self.a &= 0x0F;
            }
            0x11 => {
                if self.cf || self.a > 9 {
                    self.a += 10;
                }
                self.st = st_c(self.a);
                self.cf = !self.st;
                self.a &= 0x0F;
            }
            0x12 => {
                self.a = pins.read_k() & 0x0F;
                self.zf = self.a == 0;
                self.st = true;
            }
            0x13 => {
                self.a = pins.read_r(usize::from(self.y & 3)) & 0x0F;
                self.zf = self.a == 0;
                self.st = true;
            }
            0x14 => {
                self.a = self.y;
                self.zf = self.a == 0;
                self.st = true;
            }
            0x15 => {
                self.a = self.th;
                self.zf = self.a == 0;
                self.st = true;
            }
            0x16 => {
                self.a = self.tl;
                self.zf = self.a == 0;
                self.st = true;
            }
            0x17 => {
                self.a = self.sb;
                self.zf = self.a == 0;
                self.st = true;
            }
            0x18 => {
                // DCY: Z isn't changed.
                let y = self.y.wrapping_sub(1);
                self.st = st_c(y);
                self.y = y & 0x0F;
            }
            0x19 => {
                let ea = self.ea();
                let v = self.rd(ea).wrapping_sub(1);
                self.st = st_c(v);
                let v = v & 0x0F;
                self.zf = v == 0;
                self.wr(ea, v);
            }
            0x1A => {
                let ea = self.ea();
                self.wr(ea, self.a);
                let y = self.y.wrapping_sub(1);
                self.st = st_c(y);
                self.y = y & 0x0F;
                self.zf = self.y == 0;
            }
            0x1B => {
                std::mem::swap(&mut self.x, &mut self.a);
                self.zf = self.a == 0;
                self.st = true;
            }
            0x1C => {
                let v = self.a | u8::from(self.cf) << 4;
                self.st = st_c(v << 4);
                self.cf = !self.st;
                self.a = (v >> 1) & 0x0F;
                self.zf = self.a == 0;
            }
            0x1D => {
                let ea = self.ea();
                self.wr(ea, self.a);
                self.st = true;
            }
            0x1E => {
                let v = self.rd(self.ea()).wrapping_sub(self.a).wrapping_sub(u8::from(self.cf));
                self.st = st_c(v);
                self.cf = !self.st;
                self.a = v & 0x0F;
                self.zf = self.a == 0;
            }
            0x1F => {
                self.a |= self.rd(self.ea());
                self.zf = self.a == 0;
                self.st = !self.zf;
            }
            0x20 => {
                let p = usize::from(self.y >> 2);
                let v = pins.read_r(p) & 0x0F;
                pins.write_r(p, v | 1 << (self.y & 3));
                self.st = true;
            }
            0x21 => {
                self.cf = true;
                self.st = true;
            }
            0x22 => {
                let p = usize::from(self.y >> 2);
                let v = pins.read_r(p) & 0x0F;
                pins.write_r(p, v & !(1 << (self.y & 3)));
                self.st = true;
            }
            0x23 => {
                self.cf = false;
                self.st = true;
            }
            0x24 => {
                let v = pins.read_r(usize::from(self.y >> 2)) & 0x0F;
                self.st = v & 1 << (self.y & 3) == 0;
            }
            0x25 => self.st = !self.irq_line,
            0x26 => {
                self.st = !self.vf;
                self.vf = false;
            }
            0x27 => {
                self.st = !self.sf;
                if self.sf {
                    if self.sb_count >= SERIAL_GIVE_UP && self.pio & 0x30 == 0x20 {
                        self.serial_on = true;
                    }
                    self.sb_count = 0;
                }
                self.sf = false;
            }
            0x28 => self.st = !self.cf,
            0x29 => self.st = !self.zf,
            0x2A => {
                let ea = self.ea();
                self.wr(ea, self.sb);
                self.zf = self.sb == 0;
                self.st = true;
            }
            0x2B => {
                self.sb = self.rd(self.ea());
                self.zf = self.sb == 0;
                self.st = true;
            }
            0x2C => {
                self.si = self.si.wrapping_sub(1) & 3;
                let r = self.stack[usize::from(self.si)];
                self.pc = (r & 0x3F) as u8;
                self.pa = (r >> 6 & 0x1F) as u8;
                self.st = true;
            }
            0x2D => {
                self.a = (!self.a).wrapping_add(1) & 0x0F;
                self.st = self.a != 0;
            }
            0x2E => {
                // C: compare memory with A. Its carry, its zero (ST), and Z the opposite of ST.
                let v = self.rd(self.ea()).wrapping_sub(self.a);
                self.cf = v & 0x10 != 0;
                self.st = v & 0x0F != 0;
                self.zf = !self.st;
            }
            0x2F => {
                self.a ^= self.rd(self.ea());
                self.st = self.a != 0;
                self.zf = !self.st;
            }
            0x30..=0x33 => {
                let ea = self.ea();
                let v = self.rd(ea);
                self.wr(ea, v | 1 << (opcode & 3));
                self.st = true;
            }
            0x34..=0x37 => {
                let ea = self.ea();
                let v = self.rd(ea);
                self.wr(ea, v & !(1 << (opcode & 3)));
                self.st = true;
            }
            0x38..=0x3B => {
                let v = self.rd(self.ea());
                self.st = v & 1 << (opcode & 3) == 0;
            }
            0x3C => {
                self.in_irq = false;
                self.si = self.si.wrapping_sub(1) & 3;
                let r = self.stack[usize::from(self.si)];
                self.pc = (r & 0x3F) as u8;
                self.pa = (r >> 6 & 0x1F) as u8;
                self.st = r >> 13 & 1 != 0;
                self.zf = r >> 14 & 1 != 0;
                self.cf = r >> 15 & 1 != 0;
            }
            0x3D => {
                // JPA: the page from the next byte, the place in it from A (four bytes a step).
                self.pa = self.op(rom, self.address()) & 0x1F;
                self.pc = self.a * 4;
                len += 1;
                self.st = true;
            }
            0x3E | 0x3F => {
                let imm = self.op(rom, self.address());
                self.inc_pc();
                len += 1;
                let new = if opcode == 0x3E { self.pio | imm } else { self.pio & !imm };
                self.set_pio(new);
                self.st = true;
            }
            0x40..=0x43 => {
                let v = pins.read_r(0) & 0x0F;
                pins.write_r(0, v | 1 << (opcode & 3));
                self.st = true;
            }
            0x44..=0x47 => {
                let v = pins.read_r(0) & 0x0F;
                pins.write_r(0, v & !(1 << (opcode & 3)));
                self.st = true;
            }
            0x48..=0x4B => {
                let v = pins.read_r(2) & 0x0F;
                self.st = v & 1 << (opcode & 3) == 0;
            }
            0x4C..=0x4F => self.st = self.a & 1 << (opcode & 3) == 0,
            0x50..=0x53 => {
                let at = usize::from(opcode & 3);
                let v = self.rd(at);
                self.wr(at, self.a);
                self.a = v;
                self.zf = self.a == 0;
                self.st = true;
            }
            0x54..=0x57 => {
                let at = usize::from(opcode & 3) + 4;
                let v = self.rd(at);
                self.wr(at, self.y);
                self.y = v;
                self.zf = self.y == 0;
                self.st = true;
            }
            0x58..=0x5F => {
                self.x = opcode & 7;
                self.zf = self.x == 0;
                self.st = true;
            }
            0x60..=0x6F => {
                let arg = self.op(rom, self.address());
                self.inc_pc();
                len += 1;
                if self.st {
                    if opcode < 0x68 {
                        self.stack[usize::from(self.si)] = self.address();
                        self.si = (self.si + 1) & 3;
                    }
                    self.pc = arg & 0x3F;
                    self.pa = (opcode & 7) << 2 | arg >> 6;
                }
                self.st = true;
            }
            0x70..=0x7F => {
                let v = (opcode & 0x0F) + self.a;
                self.st = st_c(v);
                self.cf = !self.st;
                self.a = v & 0x0F;
                self.zf = self.a == 0;
            }
            0x80..=0x8F => {
                self.y = opcode & 0x0F;
                self.zf = self.y == 0;
                self.st = true;
            }
            0x90..=0x9F => {
                self.a = opcode & 0x0F;
                self.zf = self.a == 0;
                self.st = true;
            }
            0xA0..=0xAF => {
                let v = (opcode & 0x0F).wrapping_sub(self.y);
                self.cf = v & 0x10 != 0;
                self.st = v & 0x0F != 0;
                self.zf = !self.st;
            }
            0xB0..=0xBF => {
                let v = (opcode & 0x0F).wrapping_sub(self.a);
                self.cf = v & 0x10 != 0;
                self.st = v & 0x0F != 0;
                self.zf = !self.st;
            }
            _ => {
                // JMP within the page.
                if self.st {
                    self.pc = opcode & 0x3F;
                }
                self.st = true;
            }
        }
        self.burn(pins, len);
        (self.cycles - start) as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Board {
        k: u8,
        r: [u8; 4],
        o: Vec<(u8, u8)>,
        p: Vec<u8>,
    }

    impl Pins for Board {
        fn read_k(&mut self) -> u8 {
            self.k
        }
        fn write_o(&mut self, v: u8, m: u8) {
            self.o.push((v, m));
        }
        fn write_p(&mut self, v: u8) {
            self.p.push(v);
        }
        fn read_r(&mut self, p: usize) -> u8 {
            self.r[p]
        }
        fn write_r(&mut self, p: usize, v: u8) {
            self.r[p] = v;
        }
    }

    fn run(program: &[u8], steps: usize) -> (Mb88, Board) {
        let mut rom = vec![0u8; 1024];
        rom[..program.len()].copy_from_slice(program);
        let mut c = Mb88::new(10, 6);
        let mut b = Board::default();
        for _ in 0..steps {
            c.step(&rom, &mut b);
        }
        (c, b)
    }

    #[test]
    fn arithmetic_and_flags() {
        // LI 9; AI 8 -> A=1, carry; DAA isn't needed here.
        let (c, _) = run(&[0x99, 0x78], 2);
        assert_eq!((c.a, c.cf, c.st), (1, true, false));
        // LI 5; CI 5 -> equal: ST clear (zero), Z set, no borrow.
        let (c, _) = run(&[0x95, 0xB5], 2);
        assert_eq!((c.st, c.zf, c.cf), (false, true, false));
        // LI 7; CI 5 -> 5 - 7 borrows.
        let (c, _) = run(&[0x97, 0xB5], 2);
        assert!(c.cf && c.st);
    }

    #[test]
    fn jumps_test_st_and_calls_return() {
        // CALL 0:10h; (at 10h) LI 3; RTS -> back at 2.
        let mut p = vec![0x60, 0x10];
        p.resize(0x10, 0);
        p.extend([0x93, 0x2C]);
        let (c, _) = run(&p, 3);
        assert_eq!((c.address(), c.a, c.si), (2, 3, 0));
        // A JMP with ST clear falls through (and sets ST).
        let (c, _) = run(&[0x95, 0xB5, 0xC8, 0x91], 4);
        assert_eq!(c.a, 1);
    }

    #[test]
    fn the_timer_interrupt() {
        // EN 82h (timer and its interrupt), then loop; the timer overflows after 256 counts.
        let mut p = vec![0x3E, 0x82, 0xC2];
        p.resize(4, 0);
        let mut rom = vec![0u8; 1024];
        rom[..p.len()].copy_from_slice(&p);
        rom[4] = 0x3C; // RTI at the timer's vector
        let mut c = Mb88::new(10, 6);
        let mut b = Board::default();
        let mut hit = false;
        for _ in 0..20_000 {
            c.step(&rom, &mut b);
            if c.address() == 4 {
                hit = true;
                break;
            }
        }
        assert!(hit);
        assert!(c.vf);
        assert!(c.cycles >= 256 * 32);
    }

    #[test]
    fn ports() {
        // LI 0Ah; OUTP; SETC; OUTO; LY 2; OUTR
        let (_, b) = run(&[0x9A, 0x02, 0x21, 0x01, 0x82, 0x03], 6);
        assert_eq!(b.p, [0x0A]);
        assert_eq!(b.o, [(0xA0, 0xF0)]);
        assert_eq!(b.r[2], 0x0A);
    }
}
