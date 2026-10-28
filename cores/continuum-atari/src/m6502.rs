//! The MOS 6502 as the Atari 7800 has it: Atari's "SALLY", an NMOS 6502C with a HALT input.
//!
//! Every bus cycle happens in the order the real chip performs it, dummy reads and dummy writes
//! included, one `Bus::read` or `Bus::write` per cycle. That is what lets the machine around it
//! stretch a cycle (the 7800 slows down for TIA and RIOT accesses) or halt the processor between
//! any two cycles (MARIA's DMA, through SALLY's HALT pin).
//!
//! The behaviour follows the NMOS 6502 as documented by MOS's programming manual, the visual6502
//! project's transistor-level findings and the "No More Secrets" description of the undocumented
//! opcodes; it is checked case by case against SingleStepTests' 6502 suite, every bus cycle of
//! 2,560,000 cases (tests/m6502_singlestep.rs), and against Klaus Dormann's functional test.

use serde::{Deserialize, Serialize};

/// What the processor sees around it. One call is one bus cycle.
pub trait Bus {
    fn read(&mut self, addr: u16) -> u8;
    fn write(&mut self, addr: u16, value: u8);
    /// The IRQ line after the cycle that just ran (level, true = asserted). It may bring a chip
    /// up to the moment first, so it takes the bus mutably.
    fn irq(&mut self) -> bool {
        false
    }
    /// True once for each new assertion (falling edge) of NMI since the last call.
    fn take_nmi(&mut self) -> bool {
        false
    }
}

pub const FLAG_C: u8 = 0x01;
pub const FLAG_Z: u8 = 0x02;
pub const FLAG_I: u8 = 0x04;
pub const FLAG_D: u8 = 0x08;
pub const FLAG_B: u8 = 0x10;
pub const FLAG_U: u8 = 0x20;
pub const FLAG_V: u8 = 0x40;
pub const FLAG_N: u8 = 0x80;

/// The constant the unstable XAA ($8B) and LXA ($AB) opcodes OR into A on most NMOS 6502s.
const MAGIC: u8 = 0xEE;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct M6502 {
    pub a: u8,
    pub x: u8,
    pub y: u8,
    pub s: u8,
    pub p: u8,
    pub pc: u16,
    /// An NMI edge has been seen (stays set until serviced).
    nmi_cur: bool,
    /// The same as of the end of the previous cycle: what the next instruction boundary acts on,
    /// because the 6502 polls its interrupt lines before the last cycle of each instruction.
    nmi_prev: bool,
    /// IRQ asserted and not masked, as of the cycle that just ran.
    irq_cur: bool,
    /// The same one cycle earlier.
    irq_prev: bool,
    /// Halted by one of the twelve JAM opcodes; only a reset wakes it.
    pub jammed: bool,
    /// Bus cycles the processor has made while jammed (they follow a fixed pattern).
    jam_cycles: u8,
    /// Bus cycles performed since power-on.
    pub cycles: u64,
}

impl M6502 {
    pub fn new() -> Self {
        M6502 { s: 0xFD, p: FLAG_U | FLAG_I, ..Default::default() }
    }

    /// The reset sequence: seven cycles, the three stack "pushes" are reads (the R/W line stays
    /// high during reset), then the vector at $FFFC.
    pub fn reset<B: Bus>(&mut self, bus: &mut B) {
        self.jammed = false;
        self.jam_cycles = 0;
        self.nmi_cur = false;
        self.nmi_prev = false;
        self.irq_cur = false;
        self.irq_prev = false;
        self.rd(bus, self.pc);
        self.rd(bus, self.pc);
        self.rd(bus, 0x0100 | self.s as u16);
        self.s = self.s.wrapping_sub(1);
        self.rd(bus, 0x0100 | self.s as u16);
        self.s = self.s.wrapping_sub(1);
        self.rd(bus, 0x0100 | self.s as u16);
        self.s = self.s.wrapping_sub(1);
        self.p |= FLAG_I | FLAG_U;
        let lo = self.rd(bus, 0xFFFC);
        let hi = self.rd(bus, 0xFFFD);
        self.pc = u16::from_le_bytes([lo, hi]);
        // A reset is not an instruction boundary that services an interrupt seen during it.
        self.nmi_cur = false;
        self.nmi_prev = false;
    }

    // ---- one bus cycle --------------------------------------------------------------------

    #[inline]
    fn end_cycle<B: Bus>(&mut self, bus: &mut B) {
        self.cycles += 1;
        self.nmi_prev = self.nmi_cur;
        if bus.take_nmi() {
            self.nmi_cur = true;
        }
        self.irq_prev = self.irq_cur;
        self.irq_cur = bus.irq() && self.p & FLAG_I == 0;
    }

    #[inline]
    fn rd<B: Bus>(&mut self, bus: &mut B, addr: u16) -> u8 {
        let v = bus.read(addr);
        self.end_cycle(bus);
        v
    }

    #[inline]
    fn wr<B: Bus>(&mut self, bus: &mut B, addr: u16, v: u8) {
        bus.write(addr, v);
        self.end_cycle(bus);
    }

    #[inline]
    fn fetch<B: Bus>(&mut self, bus: &mut B) -> u8 {
        let v = self.rd(bus, self.pc);
        self.pc = self.pc.wrapping_add(1);
        v
    }

    #[inline]
    fn push<B: Bus>(&mut self, bus: &mut B, v: u8) {
        self.wr(bus, 0x0100 | self.s as u16, v);
        self.s = self.s.wrapping_sub(1);
    }

    #[inline]
    fn set_nz(&mut self, v: u8) {
        self.p = (self.p & !(FLAG_N | FLAG_Z)) | (v & FLAG_N) | if v == 0 { FLAG_Z } else { 0 };
    }

    #[inline]
    fn set_flag(&mut self, f: u8, on: bool) {
        if on {
            self.p |= f;
        } else {
            self.p &= !f;
        }
    }

    // ---- addressing: each returns the effective address after the cycles that find it -----

    fn a_zp<B: Bus>(&mut self, bus: &mut B) -> u16 {
        self.fetch(bus) as u16
    }

    /// zp,X and zp,Y: the base is read once (a dummy read) while the index is added.
    fn a_zpi<B: Bus>(&mut self, bus: &mut B, idx: u8) -> u16 {
        let base = self.fetch(bus);
        self.rd(bus, base as u16);
        base.wrapping_add(idx) as u16
    }

    fn a_abs<B: Bus>(&mut self, bus: &mut B) -> u16 {
        let lo = self.fetch(bus);
        let hi = self.fetch(bus);
        u16::from_le_bytes([lo, hi])
    }

    /// abs,X and abs,Y. `always` makes the read at the not-yet-carried address happen even
    /// without a page crossing (stores and read-modify-writes); a plain read only pays it on a
    /// crossing. Returns (effective address, base high byte, page crossed).
    fn a_absi<B: Bus>(&mut self, bus: &mut B, idx: u8, always: bool) -> (u16, u8, bool) {
        let lo = self.fetch(bus);
        let hi = self.fetch(bus);
        let sum = lo as u16 + idx as u16;
        let crossed = sum > 0xFF;
        let unfixed = ((hi as u16) << 8) | (sum & 0xFF);
        if crossed || always {
            self.rd(bus, unfixed);
        }
        (((hi as u16) << 8).wrapping_add(sum), hi, crossed)
    }

    /// (zp,X): the pointer is read once (dummy) while X is added; the pointer wraps in page zero.
    fn a_izx<B: Bus>(&mut self, bus: &mut B) -> u16 {
        let ptr = self.fetch(bus);
        self.rd(bus, ptr as u16);
        let p = ptr.wrapping_add(self.x);
        let lo = self.rd(bus, p as u16);
        let hi = self.rd(bus, p.wrapping_add(1) as u16);
        u16::from_le_bytes([lo, hi])
    }

    /// (zp),Y, with the same dummy-read rule as abs,Y.
    fn a_izy<B: Bus>(&mut self, bus: &mut B, always: bool) -> (u16, u8, bool) {
        let ptr = self.fetch(bus);
        let lo = self.rd(bus, ptr as u16);
        let hi = self.rd(bus, ptr.wrapping_add(1) as u16);
        let sum = lo as u16 + self.y as u16;
        let crossed = sum > 0xFF;
        let unfixed = ((hi as u16) << 8) | (sum & 0xFF);
        if crossed || always {
            self.rd(bus, unfixed);
        }
        (((hi as u16) << 8).wrapping_add(sum), hi, crossed)
    }

    // ---- arithmetic ----------------------------------------------------------------------

    fn adc(&mut self, v: u8) {
        let a = self.a;
        let c = (self.p & FLAG_C) as u16;
        let bin = a as u16 + v as u16 + c;
        if self.p & FLAG_D == 0 {
            self.set_flag(FLAG_C, bin > 0xFF);
            self.set_flag(FLAG_V, (!(a ^ v) & (a ^ bin as u8) & 0x80) != 0);
            self.a = bin as u8;
            self.set_nz(self.a);
            return;
        }
        // NMOS decimal mode: Z comes from the binary sum, N and V from the sum after the low
        // digit is adjusted but before the high one is, C from the decimal result.
        let mut lo = (a & 0x0F) as u16 + (v & 0x0F) as u16 + c;
        if lo >= 0x0A {
            lo = ((lo + 0x06) & 0x0F) + 0x10;
        }
        let mut sum = (a & 0xF0) as u16 + (v & 0xF0) as u16 + lo;
        let signed = (a & 0xF0) as i8 as i16 + (v & 0xF0) as i8 as i16 + lo as i16;
        self.set_flag(FLAG_N, sum & 0x80 != 0);
        self.set_flag(FLAG_V, !(-128..=127).contains(&signed));
        if sum >= 0xA0 {
            sum += 0x60;
        }
        self.set_flag(FLAG_C, sum >= 0x100);
        self.set_flag(FLAG_Z, bin as u8 == 0);
        self.a = sum as u8;
    }

    fn sbc(&mut self, v: u8) {
        let a = self.a;
        let borrow = 1 - (self.p & FLAG_C) as i16;
        let bin = a as i16 - v as i16 - borrow;
        // Every flag comes from the binary subtraction, in decimal mode too.
        self.set_flag(FLAG_C, bin >= 0);
        self.set_flag(FLAG_V, ((a ^ v) & (a ^ bin as u8) & 0x80) != 0);
        self.set_nz(bin as u8);
        if self.p & FLAG_D == 0 {
            self.a = bin as u8;
            return;
        }
        let mut lo = (a & 0x0F) as i16 - (v & 0x0F) as i16 - borrow;
        if lo < 0 {
            lo = ((lo - 0x06) & 0x0F) - 0x10;
        }
        let mut res = (a & 0xF0) as i16 - (v & 0xF0) as i16 + lo;
        if res < 0 {
            res -= 0x60;
        }
        self.a = res as u8;
    }

    fn cmp(&mut self, r: u8, v: u8) {
        let d = r.wrapping_sub(v);
        self.set_flag(FLAG_C, r >= v);
        self.set_nz(d);
    }

    fn asl(&mut self, v: u8) -> u8 {
        self.set_flag(FLAG_C, v & 0x80 != 0);
        let r = v << 1;
        self.set_nz(r);
        r
    }

    fn lsr(&mut self, v: u8) -> u8 {
        self.set_flag(FLAG_C, v & 0x01 != 0);
        let r = v >> 1;
        self.set_nz(r);
        r
    }

    fn rol(&mut self, v: u8) -> u8 {
        let c = self.p & FLAG_C;
        self.set_flag(FLAG_C, v & 0x80 != 0);
        let r = (v << 1) | c;
        self.set_nz(r);
        r
    }

    fn ror(&mut self, v: u8) -> u8 {
        let c = self.p & FLAG_C;
        self.set_flag(FLAG_C, v & 0x01 != 0);
        let r = (v >> 1) | (c << 7);
        self.set_nz(r);
        r
    }

    // ---- instruction shapes ----------------------------------------------------------------

    /// Read-modify-write on memory: read, write the old value back (the NMOS dummy write), then
    /// write the new one.
    fn rmw<B: Bus>(&mut self, bus: &mut B, addr: u16, op: fn(&mut Self, u8) -> u8) {
        let v = self.rd(bus, addr);
        self.wr(bus, addr, v);
        let r = op(self, v);
        self.wr(bus, addr, r);
    }

    /// A taken or not-taken relative branch, with the 6502's own interrupt polling: before the
    /// operand fetch, and again before the high-byte fix-up when a page is crossed, but not
    /// before the last cycle of a taken branch that stays in its page.
    fn branch<B: Bus>(&mut self, bus: &mut B, taken: bool) {
        let early_nmi = self.nmi_cur;
        let early_irq = self.irq_cur;
        let off = self.fetch(bus) as i8;
        if !taken {
            return;
        }
        self.rd(bus, self.pc);
        let target = self.pc.wrapping_add(off as i16 as u16);
        if target & 0xFF00 == self.pc & 0xFF00 {
            self.pc = target;
            self.nmi_prev = early_nmi;
            self.irq_prev = early_irq;
            return;
        }
        self.rd(bus, (self.pc & 0xFF00) | (target & 0x00FF));
        self.pc = target;
        self.irq_prev |= early_irq;
    }

    /// BRK, IRQ and NMI share one sequence. An NMI seen before the vector is fetched takes the
    /// sequence over (it uses the NMI vector), as on the real chip.
    fn interrupt<B: Bus>(&mut self, bus: &mut B, brk: bool, nmi: bool) {
        if brk {
            self.fetch(bus); // the padding byte after BRK
        } else {
            self.rd(bus, self.pc);
            self.rd(bus, self.pc);
        }
        let [lo, hi] = self.pc.to_le_bytes();
        self.push(bus, hi);
        self.push(bus, lo);
        let pushed = if brk { self.p | FLAG_B | FLAG_U } else { (self.p & !FLAG_B) | FLAG_U };
        self.push(bus, pushed);
        self.p |= FLAG_I;
        let use_nmi = nmi || self.nmi_cur;
        if use_nmi {
            self.nmi_cur = false;
        }
        let vector = if use_nmi { 0xFFFA } else { 0xFFFE };
        let vlo = self.rd(bus, vector);
        let vhi = self.rd(bus, vector + 1);
        self.pc = u16::from_le_bytes([vlo, vhi]);
        // The first instruction of the handler always runs before another interrupt is taken.
        self.nmi_prev = false;
        self.irq_prev = false;
    }

    /// Runs one instruction, or the interrupt sequence the last one left pending, or one bus
    /// cycle while jammed.
    pub fn step<B: Bus>(&mut self, bus: &mut B) {
        if self.jammed {
            // What a jammed NMOS 6502 keeps doing on its bus (as SingleStepTests records it).
            let addr = match self.jam_cycles {
                0 => 0xFFFF,
                1 | 2 => 0xFFFE,
                _ => 0xFFFF,
            };
            self.jam_cycles = self.jam_cycles.saturating_add(1);
            self.rd(bus, addr);
            return;
        }
        if self.nmi_prev {
            self.nmi_prev = false;
            self.nmi_cur = false;
            self.interrupt(bus, false, true);
            return;
        }
        if self.irq_prev {
            self.interrupt(bus, false, false);
            return;
        }
        let op = self.fetch(bus);
        self.execute(bus, op);
    }

    fn execute<B: Bus>(&mut self, bus: &mut B, op: u8) {
        match op {
            // ---- loads, logic and arithmetic that read memory ----
            0xA9 | 0xA5 | 0xB5 | 0xAD | 0xBD | 0xB9 | 0xA1 | 0xB1 => {
                let v = self.read_operand(bus, op);
                self.a = v;
                self.set_nz(v);
            }
            0xA2 | 0xA6 | 0xB6 | 0xAE | 0xBE => {
                let v = match op {
                    0xA2 => self.fetch(bus),
                    0xA6 => {
                        let a = self.a_zp(bus);
                        self.rd(bus, a)
                    }
                    0xB6 => {
                        let a = self.a_zpi(bus, self.y);
                        self.rd(bus, a)
                    }
                    0xAE => {
                        let a = self.a_abs(bus);
                        self.rd(bus, a)
                    }
                    _ => {
                        let (a, _, _) = self.a_absi(bus, self.y, false);
                        self.rd(bus, a)
                    }
                };
                self.x = v;
                self.set_nz(v);
            }
            0xA0 | 0xA4 | 0xB4 | 0xAC | 0xBC => {
                let v = match op {
                    0xA0 => self.fetch(bus),
                    0xA4 => {
                        let a = self.a_zp(bus);
                        self.rd(bus, a)
                    }
                    0xB4 => {
                        let a = self.a_zpi(bus, self.x);
                        self.rd(bus, a)
                    }
                    0xAC => {
                        let a = self.a_abs(bus);
                        self.rd(bus, a)
                    }
                    _ => {
                        let (a, _, _) = self.a_absi(bus, self.x, false);
                        self.rd(bus, a)
                    }
                };
                self.y = v;
                self.set_nz(v);
            }
            0x09 | 0x05 | 0x15 | 0x0D | 0x1D | 0x19 | 0x01 | 0x11 => {
                let v = self.read_operand(bus, op);
                self.a |= v;
                self.set_nz(self.a);
            }
            0x29 | 0x25 | 0x35 | 0x2D | 0x3D | 0x39 | 0x21 | 0x31 => {
                let v = self.read_operand(bus, op);
                self.a &= v;
                self.set_nz(self.a);
            }
            0x49 | 0x45 | 0x55 | 0x4D | 0x5D | 0x59 | 0x41 | 0x51 => {
                let v = self.read_operand(bus, op);
                self.a ^= v;
                self.set_nz(self.a);
            }
            0x69 | 0x65 | 0x75 | 0x6D | 0x7D | 0x79 | 0x61 | 0x71 => {
                let v = self.read_operand(bus, op);
                self.adc(v);
            }
            0xE9 | 0xEB | 0xE5 | 0xF5 | 0xED | 0xFD | 0xF9 | 0xE1 | 0xF1 => {
                let v = self.read_operand(bus, if op == 0xEB { 0xE9 } else { op });
                self.sbc(v);
            }
            0xC9 | 0xC5 | 0xD5 | 0xCD | 0xDD | 0xD9 | 0xC1 | 0xD1 => {
                let v = self.read_operand(bus, op);
                self.cmp(self.a, v);
            }
            0xE0 | 0xE4 | 0xEC => {
                let v = match op {
                    0xE0 => self.fetch(bus),
                    0xE4 => {
                        let a = self.a_zp(bus);
                        self.rd(bus, a)
                    }
                    _ => {
                        let a = self.a_abs(bus);
                        self.rd(bus, a)
                    }
                };
                self.cmp(self.x, v);
            }
            0xC0 | 0xC4 | 0xCC => {
                let v = match op {
                    0xC0 => self.fetch(bus),
                    0xC4 => {
                        let a = self.a_zp(bus);
                        self.rd(bus, a)
                    }
                    _ => {
                        let a = self.a_abs(bus);
                        self.rd(bus, a)
                    }
                };
                self.cmp(self.y, v);
            }
            0x24 | 0x2C => {
                let a = if op == 0x24 { self.a_zp(bus) } else { self.a_abs(bus) };
                let v = self.rd(bus, a);
                self.set_flag(FLAG_Z, self.a & v == 0);
                self.p = (self.p & !(FLAG_N | FLAG_V)) | (v & (FLAG_N | FLAG_V));
            }

            // ---- stores ----
            0x85 | 0x95 | 0x8D | 0x9D | 0x99 | 0x81 | 0x91 => {
                let a = self.write_address(bus, op);
                self.wr(bus, a, self.a);
            }
            0x86 | 0x96 | 0x8E => {
                let a = match op {
                    0x86 => self.a_zp(bus),
                    0x96 => self.a_zpi(bus, self.y),
                    _ => self.a_abs(bus),
                };
                self.wr(bus, a, self.x);
            }
            0x84 | 0x94 | 0x8C => {
                let a = match op {
                    0x84 => self.a_zp(bus),
                    0x94 => self.a_zpi(bus, self.x),
                    _ => self.a_abs(bus),
                };
                self.wr(bus, a, self.y);
            }

            // ---- read-modify-write ----
            0x0A => {
                self.rd(bus, self.pc);
                self.a = self.asl(self.a);
            }
            0x4A => {
                self.rd(bus, self.pc);
                self.a = self.lsr(self.a);
            }
            0x2A => {
                self.rd(bus, self.pc);
                self.a = self.rol(self.a);
            }
            0x6A => {
                self.rd(bus, self.pc);
                self.a = self.ror(self.a);
            }
            0x06 | 0x16 | 0x0E | 0x1E => {
                let a = self.rmw_address(bus, op);
                self.rmw(bus, a, Self::asl);
            }
            0x46 | 0x56 | 0x4E | 0x5E => {
                let a = self.rmw_address(bus, op);
                self.rmw(bus, a, Self::lsr);
            }
            0x26 | 0x36 | 0x2E | 0x3E => {
                let a = self.rmw_address(bus, op);
                self.rmw(bus, a, Self::rol);
            }
            0x66 | 0x76 | 0x6E | 0x7E => {
                let a = self.rmw_address(bus, op);
                self.rmw(bus, a, Self::ror);
            }
            0xE6 | 0xF6 | 0xEE | 0xFE => {
                let a = self.rmw_address(bus, op);
                self.rmw(bus, a, |c, v| {
                    let r = v.wrapping_add(1);
                    c.set_nz(r);
                    r
                });
            }
            0xC6 | 0xD6 | 0xCE | 0xDE => {
                let a = self.rmw_address(bus, op);
                self.rmw(bus, a, |c, v| {
                    let r = v.wrapping_sub(1);
                    c.set_nz(r);
                    r
                });
            }

            // ---- register moves, flags, increments ----
            0xAA => self.implied(bus, |c| {
                c.x = c.a;
                c.set_nz(c.x)
            }),
            0xA8 => self.implied(bus, |c| {
                c.y = c.a;
                c.set_nz(c.y)
            }),
            0x8A => self.implied(bus, |c| {
                c.a = c.x;
                c.set_nz(c.a)
            }),
            0x98 => self.implied(bus, |c| {
                c.a = c.y;
                c.set_nz(c.a)
            }),
            0xBA => self.implied(bus, |c| {
                c.x = c.s;
                c.set_nz(c.x)
            }),
            0x9A => self.implied(bus, |c| c.s = c.x),
            0xE8 => self.implied(bus, |c| {
                c.x = c.x.wrapping_add(1);
                c.set_nz(c.x)
            }),
            0xC8 => self.implied(bus, |c| {
                c.y = c.y.wrapping_add(1);
                c.set_nz(c.y)
            }),
            0xCA => self.implied(bus, |c| {
                c.x = c.x.wrapping_sub(1);
                c.set_nz(c.x)
            }),
            0x88 => self.implied(bus, |c| {
                c.y = c.y.wrapping_sub(1);
                c.set_nz(c.y)
            }),
            0x18 => self.implied(bus, |c| c.p &= !FLAG_C),
            0x38 => self.implied(bus, |c| c.p |= FLAG_C),
            0x58 => self.implied(bus, |c| c.p &= !FLAG_I),
            0x78 => self.implied(bus, |c| c.p |= FLAG_I),
            0xB8 => self.implied(bus, |c| c.p &= !FLAG_V),
            0xD8 => self.implied(bus, |c| c.p &= !FLAG_D),
            0xF8 => self.implied(bus, |c| c.p |= FLAG_D),
            0xEA | 0x1A | 0x3A | 0x5A | 0x7A | 0xDA | 0xFA => self.implied(bus, |_| {}),

            // ---- stack ----
            0x48 => {
                self.rd(bus, self.pc);
                self.push(bus, self.a);
            }
            0x08 => {
                self.rd(bus, self.pc);
                self.push(bus, self.p | FLAG_B | FLAG_U);
            }
            0x68 => {
                self.rd(bus, self.pc);
                self.rd(bus, 0x0100 | self.s as u16);
                self.s = self.s.wrapping_add(1);
                let v = self.rd(bus, 0x0100 | self.s as u16);
                self.a = v;
                self.set_nz(v);
            }
            0x28 => {
                self.rd(bus, self.pc);
                self.rd(bus, 0x0100 | self.s as u16);
                self.s = self.s.wrapping_add(1);
                let v = self.rd(bus, 0x0100 | self.s as u16);
                self.p = (v & !FLAG_B) | FLAG_U;
            }

            // ---- jumps, calls, returns ----
            0x4C => {
                self.pc = self.a_abs(bus);
            }
            0x6C => {
                let ptr = self.a_abs(bus);
                let lo = self.rd(bus, ptr);
                // The pointer's high byte is read from the same page (the famous JMP ($xxFF) bug).
                let hi = self.rd(bus, (ptr & 0xFF00) | (ptr.wrapping_add(1) & 0x00FF));
                self.pc = u16::from_le_bytes([lo, hi]);
            }
            0x20 => {
                let lo = self.fetch(bus);
                self.rd(bus, 0x0100 | self.s as u16);
                let [pcl, pch] = self.pc.to_le_bytes();
                self.push(bus, pch);
                self.push(bus, pcl);
                let hi = self.rd(bus, self.pc);
                self.pc = u16::from_le_bytes([lo, hi]);
            }
            0x60 => {
                self.rd(bus, self.pc);
                self.rd(bus, 0x0100 | self.s as u16);
                self.s = self.s.wrapping_add(1);
                let lo = self.rd(bus, 0x0100 | self.s as u16);
                self.s = self.s.wrapping_add(1);
                let hi = self.rd(bus, 0x0100 | self.s as u16);
                self.pc = u16::from_le_bytes([lo, hi]);
                self.fetch(bus);
            }
            0x40 => {
                self.rd(bus, self.pc);
                self.rd(bus, 0x0100 | self.s as u16);
                self.s = self.s.wrapping_add(1);
                let p = self.rd(bus, 0x0100 | self.s as u16);
                self.p = (p & !FLAG_B) | FLAG_U;
                self.s = self.s.wrapping_add(1);
                let lo = self.rd(bus, 0x0100 | self.s as u16);
                self.s = self.s.wrapping_add(1);
                let hi = self.rd(bus, 0x0100 | self.s as u16);
                self.pc = u16::from_le_bytes([lo, hi]);
            }
            0x00 => self.interrupt(bus, true, false),

            // ---- branches ----
            0x10 => self.branch(bus, self.p & FLAG_N == 0),
            0x30 => self.branch(bus, self.p & FLAG_N != 0),
            0x50 => self.branch(bus, self.p & FLAG_V == 0),
            0x70 => self.branch(bus, self.p & FLAG_V != 0),
            0x90 => self.branch(bus, self.p & FLAG_C == 0),
            0xB0 => self.branch(bus, self.p & FLAG_C != 0),
            0xD0 => self.branch(bus, self.p & FLAG_Z == 0),
            0xF0 => self.branch(bus, self.p & FLAG_Z != 0),

            // ---- undocumented: read-modify-write combinations ----
            0x07 | 0x17 | 0x0F | 0x1F | 0x1B | 0x03 | 0x13 => {
                let a = self.rmw_address(bus, op);
                self.rmw(bus, a, |c, v| {
                    let r = c.asl(v);
                    c.a |= r;
                    c.set_nz(c.a);
                    r
                });
            }
            0x27 | 0x37 | 0x2F | 0x3F | 0x3B | 0x23 | 0x33 => {
                let a = self.rmw_address(bus, op);
                self.rmw(bus, a, |c, v| {
                    let r = c.rol(v);
                    c.a &= r;
                    c.set_nz(c.a);
                    r
                });
            }
            0x47 | 0x57 | 0x4F | 0x5F | 0x5B | 0x43 | 0x53 => {
                let a = self.rmw_address(bus, op);
                self.rmw(bus, a, |c, v| {
                    let r = c.lsr(v);
                    c.a ^= r;
                    c.set_nz(c.a);
                    r
                });
            }
            0x67 | 0x77 | 0x6F | 0x7F | 0x7B | 0x63 | 0x73 => {
                let a = self.rmw_address(bus, op);
                self.rmw(bus, a, |c, v| {
                    let r = c.ror(v);
                    c.adc(r);
                    r
                });
            }
            0xC7 | 0xD7 | 0xCF | 0xDF | 0xDB | 0xC3 | 0xD3 => {
                let a = self.rmw_address(bus, op);
                self.rmw(bus, a, |c, v| {
                    let r = v.wrapping_sub(1);
                    c.cmp(c.a, r);
                    r
                });
            }
            0xE7 | 0xF7 | 0xEF | 0xFF | 0xFB | 0xE3 | 0xF3 => {
                let a = self.rmw_address(bus, op);
                self.rmw(bus, a, |c, v| {
                    let r = v.wrapping_add(1);
                    c.sbc(r);
                    r
                });
            }

            // ---- undocumented: loads and stores of A and X together ----
            0xA7 | 0xB7 | 0xAF | 0xBF | 0xA3 | 0xB3 => {
                let v = match op {
                    0xA7 => {
                        let a = self.a_zp(bus);
                        self.rd(bus, a)
                    }
                    0xB7 => {
                        let a = self.a_zpi(bus, self.y);
                        self.rd(bus, a)
                    }
                    0xAF => {
                        let a = self.a_abs(bus);
                        self.rd(bus, a)
                    }
                    0xBF => {
                        let (a, _, _) = self.a_absi(bus, self.y, false);
                        self.rd(bus, a)
                    }
                    0xA3 => {
                        let a = self.a_izx(bus);
                        self.rd(bus, a)
                    }
                    _ => {
                        let (a, _, _) = self.a_izy(bus, false);
                        self.rd(bus, a)
                    }
                };
                self.a = v;
                self.x = v;
                self.set_nz(v);
            }
            0x87 | 0x97 | 0x8F | 0x83 => {
                let a = match op {
                    0x87 => self.a_zp(bus),
                    0x97 => self.a_zpi(bus, self.y),
                    0x8F => self.a_abs(bus),
                    _ => self.a_izx(bus),
                };
                self.wr(bus, a, self.a & self.x);
            }

            // ---- undocumented: immediate oddities ----
            0x0B | 0x2B => {
                let v = self.fetch(bus);
                self.a &= v;
                self.set_nz(self.a);
                self.set_flag(FLAG_C, self.a & 0x80 != 0);
            }
            0x4B => {
                let v = self.fetch(bus);
                let t = self.a & v;
                self.a = self.lsr(t);
            }
            0x6B => {
                let v = self.fetch(bus);
                self.arr(v);
            }
            0xCB => {
                let v = self.fetch(bus);
                let t = self.a & self.x;
                self.set_flag(FLAG_C, t >= v);
                self.x = t.wrapping_sub(v);
                self.set_nz(self.x);
            }
            0x8B => {
                let v = self.fetch(bus);
                self.a = (self.a | MAGIC) & self.x & v;
                self.set_nz(self.a);
            }
            0xAB => {
                let v = self.fetch(bus);
                self.a = (self.a | MAGIC) & v;
                self.x = self.a;
                self.set_nz(self.a);
            }
            0xBB => {
                let (a, _, _) = self.a_absi(bus, self.y, false);
                let v = self.rd(bus, a) & self.s;
                self.a = v;
                self.x = v;
                self.s = v;
                self.set_nz(v);
            }

            // ---- undocumented: stores ANDed with the address's high byte plus one ----
            0x93 => {
                let (a, hi, crossed) = self.a_izy(bus, true);
                let v = self.a & self.x & hi.wrapping_add(1);
                self.unstable_store(bus, a, v, crossed);
            }
            0x9F => {
                let (a, hi, crossed) = self.a_absi(bus, self.y, true);
                let v = self.a & self.x & hi.wrapping_add(1);
                self.unstable_store(bus, a, v, crossed);
            }
            0x9E => {
                let (a, hi, crossed) = self.a_absi(bus, self.y, true);
                let v = self.x & hi.wrapping_add(1);
                self.unstable_store(bus, a, v, crossed);
            }
            0x9C => {
                let (a, hi, crossed) = self.a_absi(bus, self.x, true);
                let v = self.y & hi.wrapping_add(1);
                self.unstable_store(bus, a, v, crossed);
            }
            0x9B => {
                let (a, hi, crossed) = self.a_absi(bus, self.y, true);
                self.s = self.a & self.x;
                let v = self.s & hi.wrapping_add(1);
                self.unstable_store(bus, a, v, crossed);
            }

            // ---- undocumented: NOPs that read ----
            0x80 | 0x82 | 0x89 | 0xC2 | 0xE2 => {
                self.fetch(bus);
            }
            0x04 | 0x44 | 0x64 => {
                let a = self.a_zp(bus);
                self.rd(bus, a);
            }
            0x14 | 0x34 | 0x54 | 0x74 | 0xD4 | 0xF4 => {
                let a = self.a_zpi(bus, self.x);
                self.rd(bus, a);
            }
            0x0C => {
                let a = self.a_abs(bus);
                self.rd(bus, a);
            }
            0x1C | 0x3C | 0x5C | 0x7C | 0xDC | 0xFC => {
                let (a, _, _) = self.a_absi(bus, self.x, false);
                self.rd(bus, a);
            }

            // ---- JAM: the processor stops until reset ----
            0x02 | 0x12 | 0x22 | 0x32 | 0x42 | 0x52 | 0x62 | 0x72 | 0x92 | 0xB2 | 0xD2 | 0xF2 => {
                self.rd(bus, self.pc);
                self.jammed = true;
                self.jam_cycles = 0;
            }
        }
    }

    /// Two-cycle implied instructions: the second cycle reads the next byte and throws it away.
    #[inline]
    fn implied<B: Bus>(&mut self, bus: &mut B, f: impl FnOnce(&mut Self)) {
        self.rd(bus, self.pc);
        f(self);
    }

    /// The operand of the eight "group one" read instructions (ORA AND EOR ADC LDA CMP SBC), by
    /// the addressing mode in the opcode's middle bits.
    fn read_operand<B: Bus>(&mut self, bus: &mut B, op: u8) -> u8 {
        match op & 0x1F {
            0x09 => self.fetch(bus),
            0x05 => {
                let a = self.a_zp(bus);
                self.rd(bus, a)
            }
            0x15 => {
                let a = self.a_zpi(bus, self.x);
                self.rd(bus, a)
            }
            0x0D => {
                let a = self.a_abs(bus);
                self.rd(bus, a)
            }
            0x1D => {
                let (a, _, _) = self.a_absi(bus, self.x, false);
                self.rd(bus, a)
            }
            0x19 => {
                let (a, _, _) = self.a_absi(bus, self.y, false);
                self.rd(bus, a)
            }
            0x01 => {
                let a = self.a_izx(bus);
                self.rd(bus, a)
            }
            _ => {
                let (a, _, _) = self.a_izy(bus, false);
                self.rd(bus, a)
            }
        }
    }

    /// The address a "group one" store (STA) writes to.
    fn write_address<B: Bus>(&mut self, bus: &mut B, op: u8) -> u16 {
        match op & 0x1F {
            0x05 => self.a_zp(bus),
            0x15 => self.a_zpi(bus, self.x),
            0x0D => self.a_abs(bus),
            0x1D => self.a_absi(bus, self.x, true).0,
            0x19 => self.a_absi(bus, self.y, true).0,
            0x01 => self.a_izx(bus),
            _ => self.a_izy(bus, true).0,
        }
    }

    /// The address of a read-modify-write on memory (documented and undocumented), by mode.
    fn rmw_address<B: Bus>(&mut self, bus: &mut B, op: u8) -> u16 {
        match op & 0x1F {
            0x06 | 0x07 => self.a_zp(bus),
            0x16 | 0x17 => self.a_zpi(bus, self.x),
            0x0E | 0x0F => self.a_abs(bus),
            0x1E | 0x1F => self.a_absi(bus, self.x, true).0,
            0x1B => self.a_absi(bus, self.y, true).0,
            0x03 => self.a_izx(bus),
            _ => self.a_izy(bus, true).0, // 0x13
        }
    }

    /// SHA, SHX, SHY and TAS: when the index carries into the high byte, the stored value also
    /// replaces the high byte of the address written.
    fn unstable_store<B: Bus>(&mut self, bus: &mut B, addr: u16, v: u8, crossed: bool) {
        let addr = if crossed { ((v as u16) << 8) | (addr & 0x00FF) } else { addr };
        self.wr(bus, addr, v);
    }

    /// ARR: AND, then rotate right, with its own flags; in decimal mode the NMOS chip also
    /// applies a decimal fix-up to each digit.
    fn arr(&mut self, v: u8) {
        let t = self.a & v;
        let c_in = self.p & FLAG_C;
        let r = (t >> 1) | (c_in << 7);
        if self.p & FLAG_D == 0 {
            self.a = r;
            self.set_nz(r);
            self.set_flag(FLAG_C, r & 0x40 != 0);
            self.set_flag(FLAG_V, ((r >> 6) ^ (r >> 5)) & 1 != 0);
            return;
        }
        self.set_flag(FLAG_N, c_in != 0);
        self.set_flag(FLAG_Z, r == 0);
        self.set_flag(FLAG_V, (t ^ r) & 0x40 != 0);
        let mut res = r;
        if (t & 0x0F) + (t & 0x01) > 0x05 {
            res = (res & 0xF0) | (res.wrapping_add(0x06) & 0x0F);
        }
        if (t as u16 & 0xF0) + (t as u16 & 0x10) > 0x50 {
            res = res.wrapping_add(0x60);
            self.p |= FLAG_C;
        } else {
            self.p &= !FLAG_C;
        }
        self.a = res;
    }

    /// Tells the processor an NMI edge arrived (for machines that signal it directly rather than
    /// through `Bus::take_nmi`).
    pub fn signal_nmi(&mut self) {
        self.nmi_cur = true;
    }
}
