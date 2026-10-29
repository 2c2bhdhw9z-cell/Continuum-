//! The MINX, the Pokémon Mini's processor (Nintendo's variant of Epson's S1C88): every opcode of
//! the main page and of the two extension pages (`CE xx`, `CF xx`), with its cycles, as PokeMini
//! models the chip (its timings were measured on the console by Team Pokeme). Written for cMini;
//! `tests/cpu_pokemini.rs` checks every opcode against PokeMini's own processor from thousands of
//! random starting states.
//!
//! The registers: A and B (BA), H and L (HL), X, Y and SP, each 16 bits; the bank registers I
//! (for [HL], [N+nn] and [nnnn]), XI and YI (for [X] and [Y]); N (the page of [N+nn]); V, the
//! code bank, and U, the bank the next jump, call or return moves to. A write to U only holds
//! for the instruction after it: two instructions later U is V again, unless a jump came between.
//! F holds Zero, Carry, Overflow and Sign, Decimal (the add and subtract work in BCD), Unpack
//! (they work on the low four bits only), and the two interrupt-level bits. E holds four input
//! lines the JX and JNX branches test.

use serde::{Deserialize, Serialize};

pub const FLAG_Z: u8 = 0x01;
pub const FLAG_C: u8 = 0x02;
pub const FLAG_O: u8 = 0x04;
pub const FLAG_S: u8 = 0x08;
/// Add and subtract work in binary-coded decimal.
pub const FLAG_D: u8 = 0x10;
/// Add and subtract work on the low four bits only.
pub const FLAG_U: u8 = 0x20;
/// The two interrupt-level bits: interrupts at or below this level wait.
pub const FLAG_I0: u8 = 0x40;
pub const FLAG_I1: u8 = 0x80;
/// Kept old name: the interrupt mask, both level bits.
pub const FLAG_I: u8 = FLAG_I0 | FLAG_I1;

/// What the processor is doing between instructions.
pub const RUNNING: u8 = 0;
pub const HALTED: u8 = 1;
pub const STOPPED: u8 = 2;
/// An interrupt has been accepted: its call comes before the next instruction.
pub const ENTERING: u8 = 3;

pub trait Bus {
    fn read(&mut self, address: u32) -> u8;
    fn write(&mut self, address: u32, value: u8);
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Minx {
    pub pc: u16,
    pub sp: u16,
    pub ba: u16,
    pub hl: u16,
    pub x: u16,
    pub y: u16,
    /// The bank the next jump, call or return takes.
    pub u: u8,
    /// U as the instruction after next will see it (U holds for one instruction after a write).
    pub u_next: u8,
    /// Instructions left before U falls back to V (2 just after a write to U changed it).
    pub u_hold: u8,
    /// The code bank now.
    pub v: u8,
    pub f: u8,
    pub e: u8,
    pub n: u8,
    pub i: u8,
    pub xi: u8,
    pub yi: u8,
    /// RUNNING, HALTED, STOPPED or ENTERING.
    pub status: u8,
    /// The vector address of an interrupt being entered.
    pub vector: u8,
    /// The last byte fetched (what an unmapped read returns).
    pub ir: u8,
    pub cycles: u64,
}

#[inline]
fn sext(v: u8) -> u16 {
    v as i8 as i16 as u16
}

impl Minx {
    /// As the console starts: from the reset vector (the start-up program's), or straight at
    /// the cartridge's entry, $2102, when the console's own program isn't there to run.
    pub fn reset(&mut self, bus: &mut impl Bus, direct: bool) {
        *self = Self::default();
        self.f = FLAG_I;
        self.e = 0x1F;
        if direct {
            self.pc = 0x2102;
            self.sp = 0x2000;
        } else {
            self.pc = u16::from_le_bytes([bus.read(0), bus.read(1)]);
        }
    }

    /// Whether an interrupt may be taken now: not while a write to U is still pending.
    pub fn interruptible(&self) -> bool {
        self.u_hold == 0
    }

    /// The interrupt level in F (0 lets every interrupt in, 3 only the non-maskable ones).
    pub fn level(&self) -> u8 {
        self.f >> 6
    }

    /// The machine accepts an interrupt: its call is made before the next instruction, and a
    /// halted or stopped processor wakes for it.
    pub fn interrupt(&mut self, vector: u8) {
        self.status = ENTERING;
        self.vector = vector;
    }

    /// Kept for the libretro layer and older callers.
    pub fn halted(&self) -> bool {
        self.status == HALTED || self.status == STOPPED
    }

    /// The address the program counter points at: banked (by V) in the upper half.
    pub fn address(&self) -> u32 {
        if self.pc & 0x8000 != 0 {
            (u32::from(self.v) << 15) | u32::from(self.pc & 0x7FFF)
        } else {
            u32::from(self.pc)
        }
    }

    fn fetch(&mut self, bus: &mut impl Bus) -> u8 {
        let v = bus.read(self.address());
        self.pc = self.pc.wrapping_add(1);
        self.ir = v;
        v
    }
    fn fetch16(&mut self, bus: &mut impl Bus) -> u16 {
        let lo = self.fetch(bus);
        let hi = self.fetch(bus);
        u16::from_le_bytes([lo, hi])
    }
    /// A word at an address, its second byte at the next address (no wrap inside a bank).
    fn read16(bus: &mut impl Bus, a: u32) -> u16 {
        u16::from_le_bytes([bus.read(a), bus.read(a.wrapping_add(1))])
    }
    /// A word in bank `bank` at `a`, the second byte wrapping inside the bank.
    fn read16_in(bus: &mut impl Bus, bank: u8, a: u16) -> u16 {
        let base = u32::from(bank) << 16;
        u16::from_le_bytes([bus.read(base | u32::from(a)), bus.read(base | u32::from(a.wrapping_add(1)))])
    }
    fn write16_in(bus: &mut impl Bus, bank: u8, a: u16, v: u16) {
        let base = u32::from(bank) << 16;
        let [lo, hi] = v.to_le_bytes();
        bus.write(base | u32::from(a), lo);
        bus.write(base | u32::from(a.wrapping_add(1)), hi);
    }

    fn a(&self) -> u8 {
        self.ba as u8
    }
    fn b(&self) -> u8 {
        (self.ba >> 8) as u8
    }
    fn l(&self) -> u8 {
        self.hl as u8
    }
    fn h(&self) -> u8 {
        (self.hl >> 8) as u8
    }
    fn set_a(&mut self, v: u8) {
        self.ba = (self.ba & 0xFF00) | u16::from(v);
    }
    fn set_b(&mut self, v: u8) {
        self.ba = (self.ba & 0x00FF) | u16::from(v) << 8;
    }
    fn set_l(&mut self, v: u8) {
        self.hl = (self.hl & 0xFF00) | u16::from(v);
    }
    fn set_h(&mut self, v: u8) {
        self.hl = (self.hl & 0x00FF) | u16::from(v) << 8;
    }
    /// The 8-bit registers by their number in the encodings: A, B, L, H.
    fn r8(&self, k: u8) -> u8 {
        match k & 3 {
            0 => self.a(),
            1 => self.b(),
            2 => self.l(),
            _ => self.h(),
        }
    }
    fn set_r8(&mut self, k: u8, v: u8) {
        match k & 3 {
            0 => self.set_a(v),
            1 => self.set_b(v),
            2 => self.set_l(v),
            _ => self.set_h(v),
        }
    }
    /// The 16-bit registers by their number: BA, HL, X, Y.
    fn r16(&self, k: u8) -> u16 {
        match k & 3 {
            0 => self.ba,
            1 => self.hl,
            2 => self.x,
            _ => self.y,
        }
    }
    fn set_r16(&mut self, k: u8, v: u16) {
        match k & 3 {
            0 => self.ba = v,
            1 => self.hl = v,
            2 => self.x = v,
            _ => self.y = v,
        }
    }

    fn at_hl(&self) -> u32 {
        u32::from(self.i) << 16 | u32::from(self.hl)
    }
    fn at_x(&self) -> u32 {
        u32::from(self.xi) << 16 | u32::from(self.x)
    }
    fn at_y(&self) -> u32 {
        u32::from(self.yi) << 16 | u32::from(self.y)
    }
    fn at_n(&self, nn: u8) -> u32 {
        u32::from(self.i) << 16 | u32::from(self.n) << 8 | u32::from(nn)
    }
    fn at_abs(&self, nnnn: u16) -> u32 {
        u32::from(self.i) << 16 | u32::from(nnnn)
    }
    /// [X+ss], [Y+ss], [X+L] or [Y+L] (the offset added inside the bank).
    fn at_indexed(&mut self, bus: &mut impl Bus, k: u8) -> u32 {
        let (bank, base) = if k & 1 == 0 { (self.xi, self.x) } else { (self.yi, self.y) };
        let off = if k & 2 == 0 { sext(self.fetch(bus)) } else { sext(self.l()) };
        u32::from(bank) << 16 | u32::from(base.wrapping_add(off))
    }

    // ------------------------------------------------------------------ arithmetic

    fn add8(&mut self, a: u8, b: u8, carry_in: bool) -> u8 {
        let c = u8::from(carry_in && self.f & FLAG_C != 0);
        self.f &= 0xF0;
        let (lo_a, lo_b) = (a & 15, b & 15);
        match self.f & (FLAG_D | FLAG_U) {
            0 => {
                let r = a.wrapping_add(b).wrapping_add(c);
                self.zero(r == 0);
                self.carry(r < a);
                self.flag(FLAG_O, (a ^ r) & 0x80 != 0 && (a ^ b) & 0x80 == 0);
                self.flag(FLAG_S, r & 0x80 != 0);
                r
            }
            FLAG_D => {
                let mut r = a.wrapping_add(b).wrapping_add(c);
                if lo_a + lo_b + c >= 10 {
                    r = r.wrapping_add(6);
                }
                if r >= 0xA0 {
                    r = r.wrapping_add(0x60);
                }
                self.zero(r == 0);
                self.carry(r < a);
                r
            }
            FLAG_U => {
                let r = lo_a + lo_b + c;
                self.zero(r & 15 == 0);
                self.carry(r >= 16);
                self.flag(FLAG_O, (a ^ r) & 8 != 0 && (a ^ b) & 8 == 0);
                self.flag(FLAG_S, r & 8 != 0);
                r & 15
            }
            _ => {
                let mut r = lo_a + lo_b + c;
                if r >= 10 {
                    r += 6;
                }
                self.zero(r & 15 == 0);
                self.carry(r >= 16);
                r & 15
            }
        }
    }

    fn sub8(&mut self, a: u8, b: u8, carry_in: bool) -> u8 {
        let c = u8::from(carry_in && self.f & FLAG_C != 0);
        self.f &= 0xF0;
        let lo = (a & 15).wrapping_sub(b & 15).wrapping_sub(c);
        match self.f & (FLAG_D | FLAG_U) {
            0 => {
                let r = a.wrapping_sub(b).wrapping_sub(c);
                self.zero(r == 0);
                self.carry(a < b);
                self.flag(FLAG_O, (a ^ r) & 0x80 != 0 && (a ^ b) & 0x80 != 0);
                self.flag(FLAG_S, r & 0x80 != 0);
                r
            }
            FLAG_D => {
                let mut r = a.wrapping_sub(b).wrapping_sub(c);
                if lo >= 10 {
                    r = r.wrapping_sub(6);
                }
                if r >= 0xA0 {
                    r = r.wrapping_sub(0x60);
                }
                self.zero(r == 0);
                self.carry(a < b);
                r
            }
            FLAG_U => {
                let r = lo;
                self.zero(r & 15 == 0);
                self.carry(r >= 16);
                self.flag(FLAG_O, (a ^ r) & 8 != 0 && (a ^ b) & 8 != 0);
                self.flag(FLAG_S, r & 8 != 0);
                r & 15
            }
            _ => {
                let r = if lo >= 10 { lo.wrapping_sub(6) } else { lo };
                self.zero(r & 15 == 0);
                self.carry(r >= 16);
                r & 15
            }
        }
    }

    fn add16(&mut self, a: u16, b: u16, carry_in: bool) -> u16 {
        let c = u16::from(carry_in && self.f & FLAG_C != 0);
        let r = a.wrapping_add(b).wrapping_add(c);
        self.f &= 0xF0;
        self.zero(r == 0);
        self.carry(r < a);
        self.flag(FLAG_O, (a ^ r) & 0x8000 != 0 && (a ^ b) & 0x8000 == 0);
        self.flag(FLAG_S, r & 0x8000 != 0);
        r
    }

    fn sub16(&mut self, a: u16, b: u16, carry_in: bool) -> u16 {
        let c = u16::from(carry_in && self.f & FLAG_C != 0);
        let r = a.wrapping_sub(b).wrapping_sub(c);
        self.f &= 0xF0;
        self.zero(r == 0);
        self.carry(a < b);
        self.flag(FLAG_O, (a ^ r) & 0x8000 != 0 && (a ^ b) & 0x8000 != 0);
        self.flag(FLAG_S, r & 0x8000 != 0);
        r
    }

    /// AND, OR, XOR, NOT and TST: Zero and Sign; Carry and Overflow kept.
    fn logic(&mut self, r: u8) -> u8 {
        self.f &= 0xF6;
        self.zero(r == 0);
        self.flag(FLAG_S, r & 0x80 != 0);
        r
    }

    /// INC and DEC: only Zero changes.
    fn count8(&mut self, r: u8) -> u8 {
        self.f &= !FLAG_Z;
        self.zero(r == 0);
        r
    }
    fn count16(&mut self, r: u16) -> u16 {
        self.f &= !FLAG_Z;
        self.zero(r == 0);
        r
    }

    /// The shifts and rotates (CE 80-9F), NOT and NEG (CE A0-A7), by their row.
    fn shift(&mut self, row: u8, a: u8) -> u8 {
        let carry = self.f & FLAG_C != 0;
        let (keep, c, r, o) = match row {
            0 => (0xF0, a & 0x80 != 0, a << 1, (a & 0x40 != 0) != (a & 0x80 != 0)),
            1 => (0xF4, a & 0x80 != 0, a << 1, false),
            2 => (0xF0, a & 1 != 0, (a & 0x80) | (a >> 1), false),
            3 => (0xF4, a & 1 != 0, a >> 1, false),
            4 => (0xF4, a & 0x80 != 0, (a << 1) | u8::from(carry), false),
            5 => (0xF4, a & 0x80 != 0, a.rotate_left(1), false),
            6 => (0xF4, a & 1 != 0, (a >> 1) | if carry { 0x80 } else { 0 }, false),
            7 => (0xF4, a & 1 != 0, a.rotate_right(1), false),
            8 => return self.logic(!a),
            _ => {
                let r = a.wrapping_neg();
                self.f &= 0xF0;
                self.zero(r == 0);
                self.carry(r != 0);
                self.flag(FLAG_O, r == 0x80);
                self.flag(FLAG_S, r & 0x80 != 0);
                return r;
            }
        };
        self.f &= keep;
        self.carry(c);
        self.flag(FLAG_O, o);
        self.zero(r == 0);
        self.flag(FLAG_S, r & 0x80 != 0);
        r
    }

    fn flag(&mut self, bit: u8, on: bool) {
        if on {
            self.f |= bit;
        }
    }
    fn zero(&mut self, on: bool) {
        self.flag(FLAG_Z, on)
    }
    fn carry(&mut self, on: bool) {
        self.flag(FLAG_C, on)
    }

    /// The ALU groups of the main page (00-3F) and of CE 00-3F: ADD, ADC, SUB, SBC, AND, OR,
    /// CMP, XOR. Returns the result, or None for CMP (which only sets the flags).
    fn alu(&mut self, group: u8, a: u8, b: u8) -> Option<u8> {
        Some(match group {
            0 => self.add8(a, b, false),
            1 => self.add8(a, b, true),
            2 => self.sub8(a, b, false),
            3 => self.sub8(a, b, true),
            4 => self.logic(a & b),
            5 => self.logic(a | b),
            6 => {
                self.sub8(a, b, false);
                return None;
            }
            _ => self.logic(a ^ b),
        })
    }

    // ------------------------------------------------------------------ stack and jumps

    fn push(&mut self, bus: &mut impl Bus, v: u8) {
        self.sp = self.sp.wrapping_sub(1);
        bus.write(u32::from(self.sp), v);
    }
    fn pop(&mut self, bus: &mut impl Bus) -> u8 {
        let v = bus.read(u32::from(self.sp));
        self.sp = self.sp.wrapping_add(1);
        v
    }
    fn push_pc(&mut self, bus: &mut impl Bus) {
        self.push(bus, self.v);
        self.push(bus, (self.pc >> 8) as u8);
        self.push(bus, self.pc as u8);
    }
    /// Moves to bank U (the jump, call and return all do).
    fn take_u(&mut self) {
        self.v = self.u;
        self.u_next = self.u;
    }
    /// A relative jump: the offset counts from the instruction's last byte.
    fn jump_by(&mut self, offset: u16) {
        self.take_u();
        self.pc = self.pc.wrapping_add(offset).wrapping_sub(1);
    }
    fn call_by(&mut self, bus: &mut impl Bus, offset: u16) {
        self.push_pc(bus);
        self.jump_by(offset);
    }
    fn jump_to(&mut self, addr: u16) {
        self.take_u();
        self.pc = addr;
    }
    /// An interrupt's call (CINT, and every interrupt the machine raises), or JINT's jump: F
    /// saved, the level raised to 3, and on through the vector.
    fn call_vector(&mut self, bus: &mut impl Bus, vector: u16, with_return: bool) {
        if with_return {
            self.push_pc(bus);
        }
        self.push(bus, self.f);
        self.f |= FLAG_I;
        self.take_u();
        self.pc = Self::read16(bus, u32::from(vector));
    }
    fn set_u(&mut self, v: u8) {
        if v != self.u_next {
            self.u_hold = 2;
        }
        self.u = v;
        self.u_next = v;
    }
    fn ret(&mut self, bus: &mut impl Bus) {
        let lo = self.pop(bus);
        let hi = self.pop(bus);
        self.pc = u16::from_le_bytes([lo, hi]);
        self.v = self.pop(bus);
        self.set_u(self.v);
    }

    /// The conditions of the CE page's branches and calls (CE E0-FF), by their low four bits.
    fn condition(&self, k: u8) -> bool {
        let (z, o, s) = (self.f & FLAG_Z != 0, self.f & FLAG_O != 0, self.f & FLAG_S != 0);
        match k & 15 {
            0 => o != s,
            1 => o != s || z,
            2 => o == s && !z,
            3 => o == s,
            4 => o,
            5 => !o,
            6 => !s,
            7 => s,
            8..=11 => self.e & (1 << (k & 3)) == 0,
            _ => self.e & (1 << (k & 3)) != 0,
        }
    }

    // ------------------------------------------------------------------ running

    /// One instruction (or an interrupt's entry, or a cycle of sleep). Returns its cycles.
    pub fn step(&mut self, bus: &mut impl Bus) -> u32 {
        if self.u_hold > 0 {
            self.u = self.u_next;
            self.u_next = self.v;
            self.u_hold -= 1;
        }
        let c = match self.status {
            ENTERING => {
                self.status = RUNNING;
                self.call_vector(bus, u16::from(self.vector), true);
                20
            }
            HALTED | STOPPED => 8,
            _ => {
                let op = self.fetch(bus);
                self.main(bus, op)
            }
        };
        self.cycles += u64::from(c);
        c
    }

    fn main(&mut self, bus: &mut impl Bus, op: u8) -> u32 {
        match op {
            0x00..=0x3F => {
                let rhs = match op & 7 {
                    0 => self.a(),
                    1 => self.b(),
                    2 => self.fetch(bus),
                    3 => bus.read(self.at_hl()),
                    4 => {
                        let nn = self.fetch(bus);
                        bus.read(self.at_n(nn))
                    }
                    5 => {
                        let nnnn = self.fetch16(bus);
                        bus.read(self.at_abs(nnnn))
                    }
                    6 => bus.read(self.at_x()),
                    _ => bus.read(self.at_y()),
                };
                if let Some(r) = self.alu(op >> 3, self.a(), rhs) {
                    self.set_a(r);
                }
                match op & 7 {
                    4 => 12,
                    5 => 16,
                    _ => 8,
                }
            }
            0x40..=0x5F => {
                let v = match op & 7 {
                    k @ 0..=3 => self.r8(k),
                    4 => {
                        let nn = self.fetch(bus);
                        bus.read(self.at_n(nn))
                    }
                    5 => bus.read(self.at_hl()),
                    6 => bus.read(self.at_x()),
                    _ => bus.read(self.at_y()),
                };
                self.set_r8(op >> 3, v);
                match op & 7 {
                    0..=3 => 4,
                    4 => 12,
                    _ => 8,
                }
            }
            0x7C => {
                self.fetch(bus);
                64
            }
            0x60..=0x7F => {
                let to_n = op >= 0x78;
                let n_dst = if to_n { Some(self.fetch(bus)) } else { None };
                let v = match op & 7 {
                    k @ 0..=3 => self.r8(k),
                    4 => {
                        let nn = self.fetch(bus);
                        bus.read(self.at_n(nn))
                    }
                    5 => bus.read(self.at_hl()),
                    6 => bus.read(self.at_x()),
                    _ => bus.read(self.at_y()),
                };
                let dst = match (op >> 3) & 3 {
                    0 => self.at_x(),
                    1 => self.at_hl(),
                    2 => self.at_y(),
                    _ => self.at_n(n_dst.unwrap_or(0)),
                };
                bus.write(dst, v);
                match (to_n, op & 7) {
                    (_, 0..=3) => 8,
                    (true, _) => 16,
                    (false, 4) => 16,
                    _ => 12,
                }
            }
            0x80..=0x8F => {
                let dec = op & 8 != 0;
                let step = |v: u8| if dec { v.wrapping_sub(1) } else { v.wrapping_add(1) };
                match op & 7 {
                    k @ 0..=3 => {
                        let r = self.count8(step(self.r8(k)));
                        self.set_r8(k, r);
                        8
                    }
                    4 => {
                        self.n = self.count8(step(self.n));
                        8
                    }
                    5 => {
                        let nn = self.fetch(bus);
                        let a = self.at_n(nn);
                        let r = self.count8(step(bus.read(a)));
                        bus.write(a, r);
                        16
                    }
                    6 => {
                        let a = self.at_hl();
                        let r = self.count8(step(bus.read(a)));
                        bus.write(a, r);
                        12
                    }
                    _ => {
                        let r = if dec { self.sp.wrapping_sub(1) } else { self.sp.wrapping_add(1) };
                        self.sp = self.count16(r);
                        8
                    }
                }
            }
            0x90..=0x93 | 0x98..=0x9B => {
                let k = op & 3;
                let v = self.r16(k);
                let r = if op & 8 != 0 { v.wrapping_sub(1) } else { v.wrapping_add(1) };
                let r = self.count16(r);
                self.set_r16(k, r);
                8
            }
            0x94 => {
                self.logic(self.a() & self.b());
                8
            }
            0x95 => {
                let k = self.fetch(bus);
                let v = bus.read(self.at_hl());
                self.logic(v & k);
                12
            }
            0x96 | 0x97 => {
                let k = self.fetch(bus);
                self.logic(self.r8(op & 1) & k);
                8
            }
            0x9C..=0x9F => {
                let k = self.fetch(bus);
                self.f = match op {
                    0x9C => self.f & k,
                    0x9D => self.f | k,
                    0x9E => self.f ^ k,
                    _ => k,
                };
                12
            }
            0xA0..=0xA3 => {
                let v = self.r16(op & 3);
                self.push(bus, (v >> 8) as u8);
                self.push(bus, v as u8);
                16
            }
            0xA4 => {
                self.push(bus, self.n);
                12
            }
            0xA5 => {
                self.push(bus, self.i);
                12
            }
            0xA6 => {
                self.push(bus, self.xi);
                self.push(bus, self.yi);
                16
            }
            0xA7 => {
                self.push(bus, self.f);
                12
            }
            0xA8..=0xAB => {
                let lo = self.pop(bus);
                let hi = self.pop(bus);
                self.set_r16(op & 3, u16::from_le_bytes([lo, hi]));
                12
            }
            0xAC => {
                self.n = self.pop(bus);
                8
            }
            0xAD => {
                self.i = self.pop(bus);
                8
            }
            0xAE => {
                self.yi = self.pop(bus);
                self.xi = self.pop(bus);
                12
            }
            0xAF => {
                self.f = self.pop(bus);
                8
            }
            0xB0..=0xB3 => {
                let k = self.fetch(bus);
                self.set_r8(op & 3, k);
                8
            }
            0xB4 => {
                self.n = self.fetch(bus);
                8
            }
            0xB5..=0xB7 => {
                let k = self.fetch(bus);
                let a = match op {
                    0xB5 => self.at_hl(),
                    0xB6 => self.at_x(),
                    _ => self.at_y(),
                };
                bus.write(a, k);
                12
            }
            0xB8..=0xBB => {
                let a = self.fetch16(bus);
                let v = Self::read16_in(bus, self.i, a);
                self.set_r16(op & 3, v);
                20
            }
            0xBC..=0xBF => {
                let a = self.fetch16(bus);
                Self::write16_in(bus, self.i, a, self.r16(op & 3));
                20
            }
            0xC0..=0xC3 => {
                let k = self.fetch16(bus);
                let r = self.add16(self.r16(op & 3), k, false);
                self.set_r16(op & 3, r);
                12
            }
            0xC4..=0xC7 => {
                let k = self.fetch16(bus);
                self.set_r16(op & 3, k);
                12
            }
            0xC8..=0xCA => {
                let other = self.r16((op & 3) + 1);
                self.set_r16((op & 3) + 1, self.ba);
                self.ba = other;
                12
            }
            0xCB => {
                std::mem::swap(&mut self.ba, &mut self.sp);
                12
            }
            0xCC => {
                self.ba = self.ba.rotate_left(8);
                8
            }
            0xCD => {
                let a = self.at_hl();
                let v = bus.read(a);
                bus.write(a, self.a());
                self.set_a(v);
                12
            }
            0xCE => {
                let op = self.fetch(bus);
                self.page_ce(bus, op)
            }
            0xCF => {
                let op = self.fetch(bus);
                self.page_cf(bus, op)
            }
            0xD0..=0xD7 => {
                let k = self.fetch16(bus);
                let v = self.r16(op & 3);
                let r = self.sub16(v, k, false);
                if op < 0xD4 {
                    self.set_r16(op & 3, r);
                }
                12
            }
            0xD8..=0xDD => {
                let nn = self.fetch(bus);
                let k = self.fetch(bus);
                let a = self.at_n(nn);
                match op {
                    0xD8..=0xDA => {
                        let v = bus.read(a);
                        let r = self.logic(match op {
                            0xD8 => v & k,
                            0xD9 => v | k,
                            _ => v ^ k,
                        });
                        bus.write(a, r);
                        20
                    }
                    0xDB => {
                        let v = bus.read(a);
                        self.sub8(v, k, false);
                        16
                    }
                    0xDC => {
                        let v = bus.read(a);
                        self.logic(v & k);
                        16
                    }
                    _ => {
                        bus.write(a, k);
                        16
                    }
                }
            }
            0xDE => {
                self.set_a((self.a() & 0x0F) | (self.b() << 4));
                8
            }
            0xDF => {
                let a = self.a();
                self.set_b(a >> 4);
                self.set_a(a & 0x0F);
                8
            }
            0xE0..=0xEF => {
                let cond = match op & 3 {
                    0 => self.f & FLAG_C != 0,
                    1 => self.f & FLAG_C == 0,
                    2 => self.f & FLAG_Z != 0,
                    _ => self.f & FLAG_Z == 0,
                };
                let long = op & 8 != 0;
                let offset = if long { self.fetch16(bus) } else { sext(self.fetch(bus)) };
                let call = op & 4 == 0;
                match (call, cond, long) {
                    (true, true, false) => {
                        self.call_by(bus, offset);
                        20
                    }
                    (true, true, true) => {
                        self.call_by(bus, offset);
                        24
                    }
                    (true, false, false) => 8,
                    (true, false, true) => 12,
                    (false, true, _) => {
                        self.jump_by(offset);
                        if long { 12 } else { 8 }
                    }
                    (false, false, _) => {
                        if long {
                            12
                        } else {
                            8
                        }
                    }
                }
            }
            0xF0 => {
                let k = self.fetch(bus);
                self.call_by(bus, sext(k));
                20
            }
            0xF1 => {
                let k = self.fetch(bus);
                self.jump_by(sext(k));
                8
            }
            0xF2 => {
                let k = self.fetch16(bus);
                self.call_by(bus, k);
                24
            }
            0xF3 => {
                let k = self.fetch16(bus);
                self.jump_by(k);
                12
            }
            0xF4 => {
                self.jump_to(self.hl);
                8
            }
            0xF5 => {
                let k = self.fetch(bus);
                let b = self.count8(self.b().wrapping_sub(1));
                self.set_b(b);
                if b != 0 {
                    self.jump_by(sext(k));
                }
                16
            }
            0xF6 => {
                self.set_a(self.a().rotate_left(4));
                8
            }
            0xF7 => {
                let a = self.at_hl();
                let v = bus.read(a);
                bus.write(a, v.rotate_left(4));
                12
            }
            0xF8 => {
                self.ret(bus);
                16
            }
            0xF9 => {
                self.f = self.pop(bus);
                self.ret(bus);
                16
            }
            0xFA => {
                self.ret(bus);
                self.pc = self.pc.wrapping_add(2);
                16
            }
            0xFB => {
                let k = self.fetch16(bus);
                self.push_pc(bus);
                self.take_u();
                self.pc = Self::read16(bus, (u32::from(self.i) << 16).wrapping_add(u32::from(k)));
                20
            }
            0xFC => {
                let k = self.fetch(bus);
                self.call_vector(bus, u16::from(k), true);
                20
            }
            0xFD => {
                let k = self.fetch(bus);
                self.call_vector(bus, u16::from(k), false);
                8
            }
            // A "crash" opcode: the console's own behaviour is unknown; nothing happens here.
            0xFE => 4,
            _ => 8,
        }
    }

    fn page_ce(&mut self, bus: &mut impl Bus, op: u8) -> u32 {
        match op {
            0x00..=0x3F => {
                let group = op >> 3;
                if op & 4 == 0 {
                    let a = self.at_indexed(bus, op & 3);
                    let v = bus.read(a);
                    if let Some(r) = self.alu(group, self.a(), v) {
                        self.set_a(r);
                    }
                    16
                } else {
                    let rhs = match op & 3 {
                        0 => self.a(),
                        1 => self.fetch(bus),
                        2 => bus.read(self.at_x()),
                        _ => bus.read(self.at_y()),
                    };
                    let at = self.at_hl();
                    let v = bus.read(at);
                    if let Some(r) = self.alu(group, v, rhs) {
                        bus.write(at, r);
                    }
                    if op & 3 == 0 {
                        16
                    } else {
                        20
                    }
                }
            }
            0x40..=0x5F => {
                let reg = (op - 0x40) >> 3;
                let a = self.at_indexed(bus, op & 3);
                if op & 4 == 0 {
                    let v = bus.read(a);
                    self.set_r8(reg, v);
                } else {
                    bus.write(a, self.r8(reg));
                }
                16
            }
            0x60..=0x63 | 0x68..=0x6B | 0x78..=0x7B => {
                let a = self.at_indexed(bus, op & 3);
                let v = bus.read(a);
                let dst = match op {
                    0x60..=0x63 => self.at_hl(),
                    0x68..=0x6B => self.at_x(),
                    _ => self.at_y(),
                };
                bus.write(dst, v);
                20
            }
            0x64 | 0x65 => {
                let k = self.fetch16(bus);
                let reg = op & 1;
                let r = self.add16(self.r16(reg), k, true);
                self.set_r16(reg, r);
                16
            }
            0x66 | 0x67 => {
                let k = u16::from(self.fetch(bus)) << 8 | u16::from(self.l());
                let reg = op & 1;
                let r = self.add16(self.r16(reg), k, true);
                self.set_r16(reg, r);
                24
            }
            0x6C | 0x6D => {
                if op == 0x6C {
                    // Adds a byte to SP, then carries on as 6D.
                    let k = u16::from(self.fetch(bus));
                    self.sp = self.add16(self.sp, k, false);
                }
                let k = u16::from(self.fetch(bus));
                self.hl = self.add16(self.x, ((k << 4) * 3) + ((k & 8) >> 3), false);
                self.f &= !FLAG_C;
                40
            }
            0x6E => {
                let k = u16::from(self.fetch(bus)) << 8 | u16::from(self.l());
                self.sp = self.add16(self.sp, k, false);
                16
            }
            0x6F => {
                let k = u16::from(self.l());
                self.hl = self.add16(self.x, ((k << 4) * 3) + ((k & 8) >> 3), false);
                self.f &= !FLAG_C;
                40
            }
            0x70 | 0x71 => {
                self.pc = self.pc.wrapping_add(1);
                64
            }
            0x72 | 0x73 => 64,
            0x74..=0x77 => {
                let reg = if op & 1 == 0 { 0 } else { 2 };
                let a = self.at_indexed(bus, op & 3);
                let v = bus.read(a);
                self.set_r8(reg, v);
                64
            }
            0x7C | 0x7D => {
                self.pc = self.pc.wrapping_add(1);
                if op == 0x7C {
                    20
                } else {
                    16
                }
            }
            0x7E => 20,
            0x7F => 16,
            0x80..=0xA7 => {
                let row = (op - 0x80) >> 2;
                match op & 3 {
                    k @ 0..=1 => {
                        let r = self.shift(row, self.r8(k));
                        self.set_r8(k, r);
                        12
                    }
                    2 => {
                        let nn = self.fetch(bus);
                        let a = self.at_n(nn);
                        let r = self.shift(row, bus.read(a));
                        bus.write(a, r);
                        20
                    }
                    _ => {
                        let a = self.at_hl();
                        let r = self.shift(row, bus.read(a));
                        bus.write(a, r);
                        16
                    }
                }
            }
            0xA8 => {
                self.ba = sext(self.a());
                12
            }
            0xA9 => 8,
            0xAA => 12,
            0xAB..=0xAD => 64,
            0xAE => {
                self.status = HALTED;
                8
            }
            0xAF => {
                self.status = STOPPED;
                8
            }
            0xB0..=0xBF => {
                let col = op & 3;
                let row = (op - 0xB0) >> 2;
                if col == 3 {
                    match row {
                        0 => self.set_h(self.v),
                        1 => self.x = (self.x & 0x00FF) | u16::from(self.v) << 8,
                        2 => self.y = (self.y & 0x00FF) | u16::from(self.v) << 8,
                        _ => {
                            let k = self.fetch(bus);
                            self.sub8(self.n, k, false);
                        }
                    }
                    return 12;
                }
                let k = self.fetch(bus);
                let reg = col + 1;
                let v = self.r8(reg);
                match row {
                    0 => {
                        let r = self.logic(v & k);
                        self.set_r8(reg, r)
                    }
                    1 => {
                        let r = self.logic(v | k);
                        self.set_r8(reg, r)
                    }
                    2 => {
                        let r = self.logic(v ^ k);
                        self.set_r8(reg, r)
                    }
                    _ => {
                        self.sub8(v, k, false);
                    }
                }
                12
            }
            0xC0 => {
                self.set_a(self.n);
                8
            }
            0xC1 => {
                self.set_a(self.f);
                8
            }
            0xC2 => {
                self.n = self.a();
                8
            }
            0xC3 => {
                self.f = self.a();
                8
            }
            0xC4 => {
                let k = self.fetch(bus);
                self.set_u(k);
                16
            }
            0xC5..=0xC7 => {
                let k = self.fetch(bus);
                match op {
                    0xC5 => self.i = k,
                    0xC6 => self.xi = k,
                    _ => self.yi = k,
                }
                12
            }
            0xC8..=0xCB => {
                let v = match op {
                    0xC8 => self.v,
                    0xC9 => self.i,
                    0xCA => self.xi,
                    _ => self.yi,
                };
                self.set_a(v);
                8
            }
            0xCC => {
                self.set_u(self.a());
                8
            }
            0xCD..=0xCF => {
                let a = self.a();
                match op {
                    0xCD => self.i = a,
                    0xCE => self.xi = a,
                    _ => self.yi = a,
                }
                8
            }
            0xD0..=0xD7 => {
                let a = u32::from(self.fetch16(bus));
                if op < 0xD4 {
                    let v = bus.read(a);
                    self.set_r8(op & 3, v);
                } else {
                    bus.write(a, self.r8(op & 3));
                }
                20
            }
            0xD8 => {
                self.f &= 0xF0;
                self.hl = u16::from(self.l()) * u16::from(self.a());
                self.zero(self.hl == 0);
                self.flag(FLAG_S, self.hl & 0x8000 != 0);
                48
            }
            0xD9 => {
                self.f &= 0xF0;
                let d = u16::from(self.a());
                if d != 0 {
                    let q = self.hl / d;
                    if q < 256 {
                        let rem = (self.hl % d) as u8;
                        self.hl = u16::from(rem) << 8 | q;
                        self.zero(q == 0);
                        self.flag(FLAG_S, q & 0x80 != 0);
                    } else {
                        self.f |= FLAG_O | FLAG_S;
                    }
                }
                52
            }
            0xDA | 0xDB | 0xDE | 0xDF => {
                self.fetch(bus);
                64
            }
            0xDC => 64,
            0xDD => 16,
            0xE0..=0xEF => {
                let k = self.fetch(bus);
                if self.condition(op) {
                    self.jump_by(sext(k));
                }
                12
            }
            _ => {
                let k = self.fetch(bus);
                if self.condition(op) {
                    self.call_by(bus, sext(k));
                }
                12
            }
        }
    }

    fn page_cf(&mut self, bus: &mut impl Bus, op: u8) -> u32 {
        match op {
            0x00..=0x3F => {
                // BA (00-1F) or HL (20-3F) with BA, HL, X or Y: ADD, ADC, SUB, SBC, then the
                // same again (10-17 / 30-37) except CMP in 18-1B / 38-3B, and SBC on BA in
                // 1C-1F and 3C-3F.
                let dst = if op < 0x20 { 0 } else { 1 };
                let src = self.r16(op & 3);
                let row = (op >> 2) & 7;
                let (kind, dst) = match row {
                    6 => (4, dst),
                    7 => (3, 0),
                    _ => (row & 3, dst),
                };
                let a = self.r16(dst);
                let r = match kind {
                    0 => self.add16(a, src, false),
                    1 => self.add16(a, src, true),
                    2 => self.sub16(a, src, false),
                    3 => self.sub16(a, src, true),
                    _ => {
                        self.sub16(a, src, false);
                        return 16;
                    }
                };
                self.set_r16(dst, r);
                16
            }
            0x40..=0x5F => {
                let target = (op >> 1) & 3;
                let src = if op & 1 == 0 { self.ba } else { self.hl };
                if target == 3 {
                    return 16;
                }
                let sub = op & 8 != 0;
                let a = match target {
                    0 => self.x,
                    1 => self.y,
                    _ => self.sp,
                };
                let cmp = sub && target == 2 && op >= 0x5C;
                let r = if sub { self.sub16(a, src, false) } else { self.add16(a, src, false) };
                if !cmp {
                    match target {
                        0 => self.x = r,
                        1 => self.y = r,
                        _ => self.sp = r,
                    }
                }
                16
            }
            0x60..=0x63 => {
                let k = self.fetch16(bus);
                let reg = op & 1;
                let r = if op < 0x62 { self.add16(self.r16(reg), k, true) } else { self.sub16(self.r16(reg), k, true) };
                self.set_r16(reg, r);
                16
            }
            0x68 | 0x6A | 0x6C | 0x6E => {
                let k = self.fetch16(bus);
                match op {
                    0x68 => self.sp = self.add16(self.sp, k, false),
                    0x6A => self.sp = self.sub16(self.sp, k, false),
                    0x6C => {
                        self.sub16(self.sp, k, false);
                    }
                    _ => self.sp = k,
                }
                16
            }
            0x64..=0x6F => 16,
            0x70..=0x77 => {
                let k = self.fetch(bus);
                let a = self.sp.wrapping_add(sext(k));
                if op < 0x74 {
                    let v = Self::read16_in(bus, 0, a);
                    self.set_r16(op & 3, v);
                } else {
                    Self::write16_in(bus, 0, a, self.r16(op & 3));
                }
                24
            }
            0x78 => {
                let a = self.fetch16(bus);
                self.sp = Self::read16_in(bus, self.i, a);
                24
            }
            0x7C => {
                let a = self.fetch16(bus);
                Self::write16_in(bus, self.i, a, self.sp);
                24
            }
            0x79..=0x7B | 0x7D..=0xAF | 0xBA | 0xBB | 0xBE | 0xBF => {
                let k = self.fetch(bus);
                self.special(k)
            }
            0xB0..=0xB3 => {
                let v = self.r8(op & 3);
                self.push(bus, v);
                12
            }
            0xB4..=0xB7 => {
                let v = self.pop(bus);
                self.set_r8(op & 3, v);
                12
            }
            0xB8 | 0xB9 => {
                for v in [self.ba, self.hl, self.x, self.y] {
                    self.push(bus, (v >> 8) as u8);
                    self.push(bus, v as u8);
                }
                self.push(bus, self.n);
                if op == 0xB9 {
                    self.push(bus, self.i);
                    self.push(bus, self.xi);
                    self.push(bus, self.yi);
                    60
                } else {
                    48
                }
            }
            0xBC | 0xBD => {
                if op == 0xBD {
                    self.yi = self.pop(bus);
                    self.xi = self.pop(bus);
                    self.i = self.pop(bus);
                }
                self.n = self.pop(bus);
                for k in [3, 2, 1, 0] {
                    let lo = self.pop(bus);
                    let hi = self.pop(bus);
                    self.set_r16(k, u16::from_le_bytes([lo, hi]));
                }
                if op == 0xBD {
                    56
                } else {
                    44
                }
            }
            0xC0..=0xC7 | 0xD0..=0xDF => {
                let (bank, at) = match op {
                    0xC0..=0xC7 => (self.i, self.hl),
                    0xD0..=0xD7 => (self.xi, self.x),
                    _ => (self.yi, self.y),
                };
                if op & 4 == 0 {
                    let v = Self::read16_in(bus, bank, at);
                    self.set_r16(op & 3, v);
                } else {
                    Self::write16_in(bus, bank, at, self.r16(op & 3));
                }
                20
            }
            0xC8..=0xCF => {
                self.set_b(self.v);
                12
            }
            0xE0..=0xEF => {
                let v = self.r16(op & 3);
                self.set_r16((op >> 2) & 3, v);
                8
            }
            0xF0..=0xF3 => {
                self.sp = self.r16(op & 3);
                8
            }
            0xF4 => {
                self.hl = self.sp;
                8
            }
            0xF5 => {
                self.hl = self.pc;
                8
            }
            0xF6 => {
                self.x = (self.x & 0x00FF) | u16::from(self.v) << 8;
                12
            }
            0xF7 => {
                self.y = (self.y & 0x00FF) | u16::from(self.v) << 8;
                12
            }
            0xF8 => {
                self.ba = self.sp;
                8
            }
            0xF9 => {
                self.ba = self.pc;
                8
            }
            0xFA => {
                self.x = self.sp;
                8
            }
            0xFB | 0xFC => 12,
            0xFD => {
                self.set_a(self.e);
                12
            }
            0xFE => {
                self.y = self.sp;
                8
            }
            _ => 64,
        }
    }

    /// The three-byte CF instructions, by their third byte.
    fn special(&mut self, k: u8) -> u32 {
        match k {
            0x00..=0x3F => {
                let hl = k >= 0x20;
                let reg = u8::from(hl);
                let up = k & 8 != 0;
                let only_no_carry = k & 4 != 0;
                let keep = k & 0x1C != 0x18;
                if only_no_carry && self.f & FLAG_C != 0 {
                    return 16;
                }
                let v = self.r16(reg);
                let r = self.count16(if up { v.wrapping_add(1) } else { v.wrapping_sub(1) });
                if keep {
                    self.set_r16(reg, r);
                }
                16
            }
            0x40..=0x5F => {
                let what = (k >> 1) & 3;
                let up = k & 8 != 0;
                match what {
                    0 | 1 => {
                        let r = self.count16(if up { self.x.wrapping_add(1) } else { self.x.wrapping_sub(1) });
                        self.x = r;
                    }
                    2 => {
                        let r = self.count16(if up { self.sp.wrapping_add(1) } else { self.sp.wrapping_sub(1) });
                        self.sp = r;
                    }
                    _ => {}
                }
                16
            }
            0x60..=0x6F => 16,
            0x70..=0x73 => {
                self.set_r16(k & 3, 0x4D00 | u16::from(self.v));
                24
            }
            0x74..=0xAF => 24,
            0xB0..=0xBF => 12,
            0xC0..=0xC3 | 0xD0..=0xD3 | 0xD8..=0xDB => {
                let high = if k == 0xD3 { 0 } else { 0x2000 };
                self.set_r16(k & 3, high | u16::from(self.v));
                20
            }
            0xC4..=0xDF => 20,
            _ => 24,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Ram(Vec<u8>);
    impl Bus for Ram {
        fn read(&mut self, a: u32) -> u8 {
            self.0[a as usize & 0xFFFF]
        }
        fn write(&mut self, a: u32, v: u8) {
            self.0[a as usize & 0xFFFF] = v
        }
    }

    fn run(code: &[u8], n: usize) -> (Minx, Ram) {
        let mut ram = Ram(vec![0; 0x10000]);
        ram.0[0x1000..0x1000 + code.len()].copy_from_slice(code);
        let mut cpu = Minx { pc: 0x1000, sp: 0x2000, ..Minx::default() };
        for _ in 0..n {
            cpu.step(&mut ram);
        }
        (cpu, ram)
    }

    #[test]
    fn adding_in_decimal_and_on_the_low_four_bits() {
        // MOV A,#50 / MOV B,#50 / OR F,#10 (decimal) / ADD A,B: 50 + 50 = 100, A is 00, carry.
        let (cpu, _) = run(&[0xB0, 0x50, 0xB1, 0x50, 0x9D, 0x10, 0x01], 4);
        assert_eq!(cpu.a(), 0x00);
        assert_ne!(cpu.f & FLAG_C, 0);
        assert_ne!(cpu.f & FLAG_Z, 0);
        // Unpack: the low four bits only, A's high four cleared.
        let (cpu, _) = run(&[0xB0, 0x5C, 0xB1, 0x07, 0x9D, 0x20, 0x01], 4);
        assert_eq!(cpu.a(), 0x03);
        assert_ne!(cpu.f & FLAG_C, 0, "C + 7 carries out of the low four bits");
    }

    #[test]
    fn a_jump_takes_the_bank_written_to_u_only_right_after_it() {
        // MOV U,#02 / JMP $8010 (from $1006: $700B, less one): the jump lands in bank 2.
        let (cpu, _) = run(&[0xCE, 0xC4, 0x02, 0xF3, 0x0B, 0x70], 2);
        assert_eq!((cpu.v, cpu.pc), (2, 0x8010));
        // MOV U,#02 / NOP / NOP / JMP: by then U is V again.
        let (cpu, _) = run(&[0xCE, 0xC4, 0x02, 0xFF, 0xFF, 0xF3, 0x0B, 0x70], 4);
        assert_eq!(cpu.v, 0);
    }

    #[test]
    fn calls_and_returns_and_their_cycles() {
        // CALL $1012 (from $1003: $0010, less one), then the RET there.
        let mut code = vec![0xF2, 0x10, 0x00];
        code.resize(0x13, 0xFF);
        code[0x12] = 0xF8;
        let (cpu, _) = run(&code, 1);
        assert_eq!((cpu.pc, cpu.sp, cpu.cycles), (0x1012, 0x1FFD, 24));
        let (cpu, _) = run(&code, 2);
        assert_eq!((cpu.pc, cpu.sp, cpu.cycles), (0x1003, 0x2000, 40));
    }
}
