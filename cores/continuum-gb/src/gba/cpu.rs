//! The ARM7TDMI: ARM and Thumb instruction sets, banked registers for every mode, the barrel
//! shifter with its carry rules, and IRQ entry. Memory access and waitstates are the bus's;
//! SWIs are handed to the high-level BIOS (`bios.rs`).
//!
//! The pipeline is the real one: while an opcode runs, the one after it is already read and the
//! one after that is being read (that read is the first cycle of every instruction, at r15).
//! A jump throws both away and reads two new ones, the first non-sequentially. Every cycle an
//! instruction spends is a bus access or an internal cycle, as the ARM7TDMI's own timing tables
//! list them, so the bus can time each one and the prefetch buffer can use the free ones.

use serde::{Deserialize, Serialize};

use super::bios;
use super::bus::Bus;

const N: u32 = 1 << 31;
const Z: u32 = 1 << 30;
const C: u32 = 1 << 29;
const V: u32 = 1 << 28;
const T: u32 = 1 << 5;
const I: u32 = 1 << 7;

pub const MODE_USR: u32 = 0x10;
pub const MODE_FIQ: u32 = 0x11;
pub const MODE_IRQ: u32 = 0x12;
pub const MODE_SVC: u32 = 0x13;
pub const MODE_SYS: u32 = 0x1F;

fn bank(mode: u32) -> usize {
    match mode {
        MODE_FIQ => 1,
        MODE_IRQ => 2,
        MODE_SVC => 3,
        0x17 => 4,
        0x1B => 5,
        _ => 0,
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Cpu {
    pub r: [u32; 16],
    pub cpsr: u32,
    usr8: [u32; 5],
    fiq8: [u32; 5],
    r13: [u32; 6],
    r14: [u32; 6],
    spsr: [u32; 6],
    /// Address of the next instruction to run.
    pub pc: u32,
    branched: bool,
    /// Saved on its own after the rest (see `CpuPipe`).
    #[serde(skip)]
    pub pipe: CpuPipe,
}

/// The two opcodes already read: the one at `at` (the next to run) and the one after it. When
/// `at` is not the next instruction (the CPU was moved from outside: a save state from before
/// this was saved, the built-in BIOS, a test), they are read again without spending any time.
#[derive(Clone, Copy, Default, Serialize, Deserialize)]
pub struct CpuPipe {
    pub op: [u32; 2],
    pub at: Option<u32>,
}

impl Default for Cpu {
    /// The state the BIOS leaves when it jumps to the cartridge (no BIOS file needed).
    fn default() -> Self {
        let mut c = Cpu {
            r: [0; 16],
            cpsr: MODE_SYS,
            usr8: [0; 5],
            fiq8: [0; 5],
            r13: [0x0300_7F00, 0, 0x0300_7FA0, 0x0300_7FE0, 0, 0],
            r14: [0; 6],
            spsr: [0; 6],
            pc: 0x0800_0000,
            branched: false,
            pipe: CpuPipe::default(),
        };
        c.r[13] = 0x0300_7F00;
        c
    }
}

#[inline]
fn ror(v: u32, n: u32) -> u32 {
    v.rotate_right(n & 31)
}

impl Cpu {
    /// Power-on state, for running a real BIOS from address 0 (supervisor mode, IRQ/FIQ off).
    pub fn power_on() -> Cpu {
        let mut c = Cpu::default();
        c.r = [0; 16];
        c.r13 = [0; 6];
        c.cpsr = MODE_SVC | 0xC0;
        c.pc = 0;
        c
    }

    pub fn thumb(&self) -> bool {
        self.cpsr & T != 0
    }

    fn flag(&self, f: u32) -> bool {
        self.cpsr & f != 0
    }

    fn set_flag(&mut self, f: u32, on: bool) {
        if on {
            self.cpsr |= f;
        } else {
            self.cpsr &= !f;
        }
    }

    fn set_nz(&mut self, v: u32) {
        self.set_flag(N, v & N != 0);
        self.set_flag(Z, v == 0);
    }

    pub fn set_mode(&mut self, new: u32) {
        let old = self.cpsr & 0x1F;
        let (ob, nb) = (bank(old), bank(new));
        if ob != nb {
            self.r13[ob] = self.r[13];
            self.r14[ob] = self.r[14];
            if old == MODE_FIQ {
                self.fiq8.copy_from_slice(&self.r[8..13]);
                self.r[8..13].copy_from_slice(&self.usr8);
            } else if new == MODE_FIQ {
                self.usr8.copy_from_slice(&self.r[8..13]);
                self.r[8..13].copy_from_slice(&self.fiq8);
            }
            self.r[13] = self.r13[nb];
            self.r[14] = self.r14[nb];
        }
        self.cpsr = (self.cpsr & !0x1F) | new;
    }

    pub fn set_cpsr(&mut self, v: u32) {
        self.set_mode(v & 0x1F);
        self.cpsr = v;
    }

    fn spsr(&self) -> u32 {
        let b = bank(self.cpsr & 0x1F);
        if b == 0 { self.cpsr } else { self.spsr[b] }
    }

    fn set_spsr(&mut self, v: u32) {
        let b = bank(self.cpsr & 0x1F);
        if b != 0 {
            self.spsr[b] = v;
        }
    }

    /// Writes a register; r15 is a jump.
    pub fn set_reg(&mut self, i: usize, v: u32) {
        if i == 15 {
            self.pc = if self.thumb() { v & !1 } else { v & !3 };
            self.branched = true;
        } else {
            self.r[i] = v;
        }
    }

    fn user_reg(&self, i: usize) -> u32 {
        let mode = self.cpsr & 0x1F;
        match i {
            8..=12 if mode == MODE_FIQ => self.usr8[i - 8],
            13 if bank(mode) != 0 => self.r13[0],
            14 if bank(mode) != 0 => self.r14[0],
            _ => self.r[i],
        }
    }

    fn set_user_reg(&mut self, i: usize, v: u32) {
        let mode = self.cpsr & 0x1F;
        match i {
            8..=12 if mode == MODE_FIQ => self.usr8[i - 8] = v,
            13 if bank(mode) != 0 => self.r13[0] = v,
            14 if bank(mode) != 0 => self.r14[0] = v,
            15 => self.set_reg(15, v),
            _ => self.r[i] = v,
        }
    }

    /// Enters an exception: `lr` is the return address as that exception's handler expects it.
    pub fn exception(&mut self, mode: u32, vector: u32, lr: u32) {
        let old = self.cpsr;
        self.set_mode(mode);
        self.set_spsr(old);
        self.r[14] = lr;
        self.cpsr = (self.cpsr & !T) | I;
        self.pc = vector;
        self.branched = true;
    }

    fn cond(&self, c: u32) -> bool {
        let (n, z, cf, v) = (self.flag(N), self.flag(Z), self.flag(C), self.flag(V));
        match c {
            0 => z,
            1 => !z,
            2 => cf,
            3 => !cf,
            4 => n,
            5 => !n,
            6 => v,
            7 => !v,
            8 => cf && !z,
            9 => !cf || z,
            10 => n == v,
            11 => n != v,
            12 => !z && n == v,
            13 => z || n != v,
            14 => true,
            _ => false,
        }
    }

    /// The barrel shifter. `imm` marks an immediate amount (0 then means LSR/ASR #32, RRX).
    fn shift(&self, kind: u32, v: u32, amt: u32, imm: bool) -> (u32, bool) {
        let c = self.flag(C);
        match kind {
            0 => match amt {
                0 => (v, c),
                1..=31 => (v << amt, v >> (32 - amt) & 1 != 0),
                32 => (0, v & 1 != 0),
                _ => (0, false),
            },
            1 => {
                let amt = if imm && amt == 0 { 32 } else { amt };
                match amt {
                    0 => (v, c),
                    1..=31 => (v >> amt, v >> (amt - 1) & 1 != 0),
                    32 => (0, v >> 31 != 0),
                    _ => (0, false),
                }
            }
            2 => {
                let amt = if imm && amt == 0 { 32 } else { amt };
                match amt {
                    0 => (v, c),
                    1..=31 => (((v as i32) >> amt) as u32, v >> (amt - 1) & 1 != 0),
                    _ => (((v as i32) >> 31) as u32, v >> 31 != 0),
                }
            }
            _ => {
                if imm && amt == 0 {
                    (u32::from(c) << 31 | v >> 1, v & 1 != 0)
                } else if amt == 0 {
                    (v, c)
                } else if amt & 31 == 0 {
                    (v, v >> 31 != 0)
                } else {
                    (ror(v, amt), v >> ((amt & 31) - 1) & 1 != 0)
                }
            }
        }
    }

    fn add(&mut self, a: u32, b: u32, carry: bool, s: bool) -> u32 {
        let r64 = u64::from(a) + u64::from(b) + u64::from(carry);
        let r = r64 as u32;
        if s {
            self.set_nz(r);
            self.set_flag(C, r64 >> 32 != 0);
            self.set_flag(V, (!(a ^ b) & (a ^ r)) >> 31 != 0);
        }
        r
    }

    fn sub(&mut self, a: u32, b: u32, carry: bool, s: bool) -> u32 {
        self.add(a, !b, carry, s)
    }

    /// One instruction, or IRQ entry.
    pub fn step(&mut self, bus: &mut Bus) {
        if bus.irq_line() && self.cpsr & I == 0 {
            self.irq(bus);
        }
        let thumb = self.thumb();
        let len = if thumb { 2 } else { 4 };
        if self.pipe.at != Some(self.pc) {
            self.pipe_quietly(bus);
        }
        let addr = self.pc;
        let op = self.pipe.op[0];
        self.pipe.op[0] = self.pipe.op[1];
        let next = addr.wrapping_add(2 * len);
        self.pipe.op[1] = bus.code(next, thumb, true);
        self.r[15] = next;
        self.pc = addr.wrapping_add(len);
        self.pipe.at = Some(self.pc);
        self.branched = false;
        if thumb {
            self.thumb_op(bus, op as u16, addr);
        } else if self.cond(op >> 28) {
            self.arm_op(bus, op, addr);
        }
        if self.branched {
            self.refill(bus);
        }
    }

    /// IRQ entry: the opcode read that was due still happens, then the pipeline is refilled at
    /// the vector.
    fn irq(&mut self, bus: &mut Bus) {
        bus.bios_latch = 0xE25E_F004;
        let thumb = self.thumb();
        let len = if thumb { 2 } else { 4 };
        if self.pipe.at != Some(self.pc) {
            self.pipe_quietly(bus);
        }
        bus.code(self.pc.wrapping_add(2 * len), thumb, true);
        self.exception(MODE_IRQ, 0x18, self.pc.wrapping_add(4));
        self.refill(bus);
    }

    /// After a jump: the first opcode non-sequentially, the second sequentially.
    fn refill(&mut self, bus: &mut Bus) {
        let thumb = self.thumb();
        let len = if thumb { 2 } else { 4 };
        let pc = self.pc;
        self.pipe.op[0] = bus.code(pc, thumb, false);
        self.pipe.op[1] = bus.code(pc.wrapping_add(len), thumb, true);
        self.pipe.at = Some(pc);
    }

    fn pipe_quietly(&mut self, bus: &mut Bus) {
        let pc = self.pc;
        bus.exec_pc = pc;
        self.pipe.op = if self.thumb() {
            [bus.peek16(pc), bus.peek16(pc.wrapping_add(2))]
        } else {
            [bus.peek32(pc), bus.peek32(pc.wrapping_add(4))]
        };
        self.pipe.at = Some(pc);
    }

    // ---------------------------------------------------------------- ARM

    fn arm_op(&mut self, bus: &mut Bus, op: u32, addr: u32) {
        if op & 0x0FFF_FFF0 == 0x012F_FF10 {
            // BX
            let v = self.r[(op & 15) as usize];
            self.set_flag(T, v & 1 != 0);
            self.set_reg(15, v);
        } else if op & 0x0FC0_00F0 == 0x0000_0090 {
            self.arm_mul(bus, op);
        } else if op & 0x0F80_00F0 == 0x0080_0090 {
            self.arm_mull(bus, op);
        } else if op & 0x0FB0_0FF0 == 0x0100_0090 {
            self.arm_swap(bus, op);
        } else if op & 0x0E00_0090 == 0x0000_0090 && op & 0x60 != 0 {
            self.arm_half(bus, op);
        } else if op & 0x0C00_0000 == 0 {
            let opc = (op >> 21) & 15;
            if (8..=11).contains(&opc) && op & (1 << 20) == 0 {
                self.arm_psr(op);
            } else {
                self.arm_dp(bus, op);
            }
        } else if op & 0x0C00_0000 == 0x0400_0000 {
            if op & 0x0200_0010 == 0x0200_0010 {
                self.undefined(addr);
            } else {
                self.arm_single(bus, op);
            }
        } else if op & 0x0E00_0000 == 0x0800_0000 {
            self.arm_block(bus, op);
        } else if op & 0x0E00_0000 == 0x0A00_0000 {
            let off = ((op << 8) as i32 >> 6) as u32;
            if op & (1 << 24) != 0 {
                self.r[14] = addr.wrapping_add(4);
            }
            self.set_reg(15, self.r[15].wrapping_add(off));
        } else if op & 0x0F00_0000 == 0x0F00_0000 {
            if bus.real_bios {
                self.exception(MODE_SVC, 0x08, addr.wrapping_add(4));
            } else {
                bios::swi(self, bus, (op >> 16) & 0xFF);
                // Coming back from the BIOS is a jump.
                self.branched = true;
            }
        } else {
            self.undefined(addr);
        }
    }

    fn undefined(&mut self, addr: u32) {
        let lr = addr.wrapping_add(if self.thumb() { 2 } else { 4 });
        self.exception(0x1B, 0x04, lr);
    }

    fn arm_dp(&mut self, bus: &mut Bus, op: u32) {
        let opc = (op >> 21) & 15;
        let s = op & (1 << 20) != 0;
        let rn_i = ((op >> 16) & 15) as usize;
        let rd = ((op >> 12) & 15) as usize;
        let mut rn = self.r[rn_i];
        let (op2, sc) = if op & (1 << 25) != 0 {
            let rot = ((op >> 8) & 15) * 2;
            let v = ror(op & 0xFF, rot);
            (v, if rot == 0 { self.flag(C) } else { v >> 31 != 0 })
        } else {
            let rm_i = (op & 15) as usize;
            let kind = (op >> 5) & 3;
            if op & 0x10 != 0 {
                // Register-specified shift: one extra cycle, and PC reads 4 further ahead.
                bus.idle(1);
                let mut rm = self.r[rm_i];
                if rm_i == 15 {
                    rm = rm.wrapping_add(4);
                }
                if rn_i == 15 {
                    rn = rn.wrapping_add(4);
                }
                let amt = self.r[((op >> 8) & 15) as usize] & 0xFF;
                self.shift(kind, rm, amt, false)
            } else {
                self.shift(kind, self.r[rm_i], (op >> 7) & 31, true)
            }
        };
        let c = self.flag(C);
        let logical = |cpu: &mut Cpu, r: u32| {
            if s {
                cpu.set_nz(r);
                cpu.set_flag(C, sc);
            }
            r
        };
        let (res, write) = match opc {
            0 => (logical(self, rn & op2), true),
            1 => (logical(self, rn ^ op2), true),
            2 => (self.sub(rn, op2, true, s), true),
            3 => (self.sub(op2, rn, true, s), true),
            4 => (self.add(rn, op2, false, s), true),
            5 => (self.add(rn, op2, c, s), true),
            6 => (self.sub(rn, op2, c, s), true),
            7 => (self.sub(op2, rn, c, s), true),
            8 => (logical(self, rn & op2), false),
            9 => (logical(self, rn ^ op2), false),
            10 => (self.sub(rn, op2, true, true), false),
            11 => (self.add(rn, op2, false, true), false),
            12 => (logical(self, rn | op2), true),
            13 => (logical(self, op2), true),
            14 => (logical(self, rn & !op2), true),
            _ => (logical(self, !op2), true),
        };
        if rd == 15 && s {
            let spsr = self.spsr();
            self.set_cpsr(spsr);
        }
        if write {
            self.set_reg(rd, res);
        }
    }

    fn arm_psr(&mut self, op: u32) {
        let spsr = op & (1 << 22) != 0;
        if op & (1 << 21) == 0 {
            // MRS
            let v = if spsr { self.spsr() } else { self.cpsr };
            self.r[((op >> 12) & 15) as usize] = v;
            return;
        }
        let v = if op & (1 << 25) != 0 {
            ror(op & 0xFF, ((op >> 8) & 15) * 2)
        } else {
            self.r[(op & 15) as usize]
        };
        let mut mask = 0u32;
        if op & (1 << 19) != 0 { mask |= 0xFF00_0000 }
        if op & (1 << 18) != 0 { mask |= 0x00FF_0000 }
        if op & (1 << 17) != 0 { mask |= 0x0000_FF00 }
        if op & (1 << 16) != 0 { mask |= 0x0000_00FF }
        if spsr {
            let cur = self.spsr();
            self.set_spsr((cur & !mask) | (v & mask));
        } else {
            if self.cpsr & 0x1F == MODE_USR {
                mask &= 0xFF00_0000;
            }
            let nv = (self.cpsr & !mask) | (v & mask);
            self.set_cpsr(nv | 0x10);
        }
    }

    fn mul_cycles(bus: &mut Bus, rs: u32, signed: bool) {
        let m = if signed {
            let x = if rs as i32 >= 0 { rs } else { !rs };
            if x >> 8 == 0 { 1 } else if x >> 16 == 0 { 2 } else if x >> 24 == 0 { 3 } else { 4 }
        } else if rs >> 8 == 0 { 1 } else if rs >> 16 == 0 { 2 } else if rs >> 24 == 0 { 3 } else { 4 };
        bus.idle(m);
    }

    fn arm_mul(&mut self, bus: &mut Bus, op: u32) {
        let rd = ((op >> 16) & 15) as usize;
        let rs = self.r[((op >> 8) & 15) as usize];
        let mut v = self.r[(op & 15) as usize].wrapping_mul(rs);
        Self::mul_cycles(bus, rs, true);
        if op & (1 << 21) != 0 {
            v = v.wrapping_add(self.r[((op >> 12) & 15) as usize]);
            bus.idle(1);
        }
        if op & (1 << 20) != 0 {
            self.set_nz(v);
        }
        self.set_reg(rd, v);
    }

    fn arm_mull(&mut self, bus: &mut Bus, op: u32) {
        let hi = ((op >> 16) & 15) as usize;
        let lo = ((op >> 12) & 15) as usize;
        let rs = self.r[((op >> 8) & 15) as usize];
        let rm = self.r[(op & 15) as usize];
        let signed = op & (1 << 22) != 0;
        let mut v = if signed {
            (i64::from(rm as i32) * i64::from(rs as i32)) as u64
        } else {
            u64::from(rm) * u64::from(rs)
        };
        Self::mul_cycles(bus, rs, signed);
        bus.idle(1);
        if op & (1 << 21) != 0 {
            v = v.wrapping_add(u64::from(self.r[hi]) << 32 | u64::from(self.r[lo]));
            bus.idle(1);
        }
        if op & (1 << 20) != 0 {
            self.set_flag(N, v >> 63 != 0);
            self.set_flag(Z, v == 0);
        }
        self.set_reg(lo, v as u32);
        self.set_reg(hi, (v >> 32) as u32);
    }

    fn arm_swap(&mut self, bus: &mut Bus, op: u32) {
        let rn = self.r[((op >> 16) & 15) as usize];
        let rd = ((op >> 12) & 15) as usize;
        let rm = self.r[(op & 15) as usize];
        if op & (1 << 22) != 0 {
            let v = bus.read8(rn);
            bus.write8(rn, rm as u8);
            self.set_reg(rd, v);
        } else {
            let v = ror(bus.read32(rn & !3), (rn & 3) * 8);
            bus.write32(rn & !3, rm);
            self.set_reg(rd, v);
        }
        bus.idle(1);
    }

    fn arm_half(&mut self, bus: &mut Bus, op: u32) {
        let p = op & (1 << 24) != 0;
        let u = op & (1 << 23) != 0;
        let w = op & (1 << 21) != 0;
        let l = op & (1 << 20) != 0;
        let rn_i = ((op >> 16) & 15) as usize;
        let rd = ((op >> 12) & 15) as usize;
        let off = if op & (1 << 22) != 0 {
            (op >> 4) & 0xF0 | (op & 15)
        } else {
            self.r[(op & 15) as usize]
        };
        let base = self.r[rn_i];
        let moved = if u { base.wrapping_add(off) } else { base.wrapping_sub(off) };
        let addr = if p { moved } else { base };
        let kind = (op >> 5) & 3;
        if l {
            let v = match kind {
                1 => ror(bus.read16(addr & !1), (addr & 1) * 8),
                2 => bus.read8(addr) as i8 as i32 as u32,
                _ => {
                    if addr & 1 != 0 {
                        bus.read8(addr) as i8 as i32 as u32
                    } else {
                        bus.read16(addr) as i16 as i32 as u32
                    }
                }
            };
            bus.idle(1);
            if !p || w {
                self.set_reg(rn_i, moved);
            }
            self.set_reg(rd, v);
        } else {
            let mut v = self.r[rd];
            if rd == 15 {
                v = v.wrapping_add(4);
            }
            bus.write16(addr, v as u16);
            if !p || w {
                self.set_reg(rn_i, moved);
            }
        }
    }

    fn arm_single(&mut self, bus: &mut Bus, op: u32) {
        let p = op & (1 << 24) != 0;
        let u = op & (1 << 23) != 0;
        let b = op & (1 << 22) != 0;
        let w = op & (1 << 21) != 0;
        let l = op & (1 << 20) != 0;
        let rn_i = ((op >> 16) & 15) as usize;
        let rd = ((op >> 12) & 15) as usize;
        let off = if op & (1 << 25) != 0 {
            self.shift((op >> 5) & 3, self.r[(op & 15) as usize], (op >> 7) & 31, true).0
        } else {
            op & 0xFFF
        };
        let base = self.r[rn_i];
        let moved = if u { base.wrapping_add(off) } else { base.wrapping_sub(off) };
        let addr = if p { moved } else { base };
        if l {
            let v = if b { bus.read8(addr) } else { ror(bus.read32(addr & !3), (addr & 3) * 8) };
            bus.idle(1);
            if !p || w {
                self.set_reg(rn_i, moved);
            }
            self.set_reg(rd, v);
        } else {
            let mut v = self.r[rd];
            if rd == 15 {
                v = v.wrapping_add(4);
            }
            if b {
                bus.write8(addr, v as u8);
            } else {
                bus.write32(addr, v);
            }
            if !p || w {
                self.set_reg(rn_i, moved);
            }
        }
    }

    fn arm_block(&mut self, bus: &mut Bus, op: u32) {
        let p = op & (1 << 24) != 0;
        let u = op & (1 << 23) != 0;
        let s = op & (1 << 22) != 0;
        let w = op & (1 << 21) != 0;
        let l = op & (1 << 20) != 0;
        let rn_i = ((op >> 16) & 15) as usize;
        let list = op & 0xFFFF;
        let base = self.r[rn_i];
        let (regs, bytes): (Vec<usize>, u32) = if list == 0 {
            (vec![15], 0x40)
        } else {
            let v: Vec<usize> = (0..16).filter(|i| list & (1 << i) != 0).collect();
            let n = v.len() as u32 * 4;
            (v, n)
        };
        let (mut addr, new_base) = if u {
            (if p { base + 4 } else { base }, base.wrapping_add(bytes))
        } else {
            let start = base.wrapping_sub(bytes);
            (if p { start } else { start + 4 }, start)
        };
        let user = s && !(l && list & 0x8000 != 0);
        if l {
            if w {
                self.set_reg(rn_i, new_base);
            }
            for (k, &r) in regs.iter().enumerate() {
                let v = bus.read32_seq(addr & !3, k > 0);
                if user {
                    self.set_user_reg(r, v);
                } else {
                    self.set_reg(r, v);
                }
                addr = addr.wrapping_add(4);
            }
            bus.idle(1);
            if s && list & 0x8000 != 0 {
                let spsr = self.spsr();
                self.set_cpsr(spsr);
            }
        } else {
            for (k, &r) in regs.iter().enumerate() {
                let mut v = if user { self.user_reg(r) } else { self.r[r] };
                if r == 15 {
                    v = v.wrapping_add(4);
                }
                if r == rn_i && k != 0 {
                    v = new_base;
                }
                bus.write32_seq(addr & !3, v, k > 0);
                addr = addr.wrapping_add(4);
            }
            if w {
                self.set_reg(rn_i, new_base);
            }
        }
    }

    // ---------------------------------------------------------------- Thumb

    fn thumb_op(&mut self, bus: &mut Bus, op: u16, addr: u32) {
        let op = u32::from(op);
        match op >> 11 {
            0..=2 => {
                // Move shifted register
                let (v, c) = self.shift(op >> 11, self.r[((op >> 3) & 7) as usize], (op >> 6) & 31, true);
                self.set_nz(v);
                self.set_flag(C, c);
                self.r[(op & 7) as usize] = v;
            }
            3 => {
                let rs = self.r[((op >> 3) & 7) as usize];
                let n = (op >> 6) & 7;
                let v = if op & (1 << 10) != 0 { n } else { self.r[n as usize] };
                let r = if op & (1 << 9) != 0 { self.sub(rs, v, true, true) } else { self.add(rs, v, false, true) };
                self.r[(op & 7) as usize] = r;
            }
            4..=7 => {
                let rd = ((op >> 8) & 7) as usize;
                let imm = op & 0xFF;
                match (op >> 11) & 3 {
                    0 => {
                        self.r[rd] = imm;
                        self.set_nz(imm);
                    }
                    1 => {
                        self.sub(self.r[rd], imm, true, true);
                    }
                    2 => self.r[rd] = self.add(self.r[rd], imm, false, true),
                    _ => self.r[rd] = self.sub(self.r[rd], imm, true, true),
                }
            }
            8 => {
                if op & (1 << 10) == 0 {
                    self.thumb_alu(bus, op);
                } else {
                    self.thumb_hi(op);
                }
            }
            9 => {
                let a = (self.r[15] & !3).wrapping_add((op & 0xFF) * 4);
                let v = bus.read32(a);
                bus.idle(1);
                self.r[((op >> 8) & 7) as usize] = v;
            }
            10 | 11 => {
                let ro = self.r[((op >> 6) & 7) as usize];
                let a = self.r[((op >> 3) & 7) as usize].wrapping_add(ro);
                let rd = (op & 7) as usize;
                if op & (1 << 9) == 0 {
                    match (op >> 10) & 3 {
                        0 => bus.write32(a, self.r[rd]),
                        1 => bus.write8(a, self.r[rd] as u8),
                        2 => {
                            self.r[rd] = ror(bus.read32(a & !3), (a & 3) * 8);
                            bus.idle(1);
                        }
                        _ => {
                            self.r[rd] = bus.read8(a);
                            bus.idle(1);
                        }
                    }
                } else {
                    match (op >> 10) & 3 {
                        0 => bus.write16(a, self.r[rd] as u16),
                        1 => {
                            self.r[rd] = bus.read8(a) as i8 as i32 as u32;
                            bus.idle(1);
                        }
                        2 => {
                            self.r[rd] = ror(bus.read16(a & !1), (a & 1) * 8);
                            bus.idle(1);
                        }
                        _ => {
                            self.r[rd] = if a & 1 != 0 {
                                bus.read8(a) as i8 as i32 as u32
                            } else {
                                bus.read16(a) as i16 as i32 as u32
                            };
                            bus.idle(1);
                        }
                    }
                }
            }
            12..=15 => {
                let byte = op & (1 << 12) != 0;
                let off = (op >> 6) & 31;
                let rb = self.r[((op >> 3) & 7) as usize];
                let rd = (op & 7) as usize;
                let load = op & (1 << 11) != 0;
                if byte {
                    let a = rb.wrapping_add(off);
                    if load {
                        self.r[rd] = bus.read8(a);
                        bus.idle(1);
                    } else {
                        bus.write8(a, self.r[rd] as u8);
                    }
                } else {
                    let a = rb.wrapping_add(off * 4);
                    if load {
                        self.r[rd] = ror(bus.read32(a & !3), (a & 3) * 8);
                        bus.idle(1);
                    } else {
                        bus.write32(a, self.r[rd]);
                    }
                }
            }
            16 | 17 => {
                let a = self.r[((op >> 3) & 7) as usize].wrapping_add(((op >> 6) & 31) * 2);
                let rd = (op & 7) as usize;
                if op & (1 << 11) != 0 {
                    self.r[rd] = ror(bus.read16(a & !1), (a & 1) * 8);
                    bus.idle(1);
                } else {
                    bus.write16(a, self.r[rd] as u16);
                }
            }
            18 | 19 => {
                let a = self.r[13].wrapping_add((op & 0xFF) * 4);
                let rd = ((op >> 8) & 7) as usize;
                if op & (1 << 11) != 0 {
                    self.r[rd] = ror(bus.read32(a & !3), (a & 3) * 8);
                    bus.idle(1);
                } else {
                    bus.write32(a, self.r[rd]);
                }
            }
            20 | 21 => {
                let base = if op & (1 << 11) != 0 { self.r[13] } else { self.r[15] & !3 };
                self.r[((op >> 8) & 7) as usize] = base.wrapping_add((op & 0xFF) * 4);
            }
            22 | 23 => {
                if op & 0x0F00 == 0 {
                    let off = (op & 0x7F) * 4;
                    self.r[13] = if op & 0x80 != 0 { self.r[13].wrapping_sub(off) } else { self.r[13].wrapping_add(off) };
                } else if op & 0x0600 == 0x0400 {
                    self.thumb_push_pop(bus, op);
                } else {
                    self.undefined(addr);
                }
            }
            24 | 25 => self.thumb_ldm_stm(bus, op),
            26 | 27 => {
                let c = (op >> 8) & 15;
                if c == 15 {
                    if bus.real_bios {
                        self.exception(MODE_SVC, 0x08, addr.wrapping_add(2));
                    } else {
                        bios::swi(self, bus, op & 0xFF);
                        self.branched = true;
                    }
                } else if c == 14 {
                    self.undefined(addr);
                } else if self.cond(c) {
                    let off = ((op & 0xFF) as i8 as i32 * 2) as u32;
                    self.set_reg(15, self.r[15].wrapping_add(off));
                }
            }
            28 => {
                let off = (((op & 0x7FF) << 21) as i32 >> 20) as u32;
                self.set_reg(15, self.r[15].wrapping_add(off));
            }
            29 => self.undefined(addr),
            30 => {
                let off = (((op & 0x7FF) << 21) as i32 >> 9) as u32;
                self.r[14] = self.r[15].wrapping_add(off);
            }
            _ => {
                let target = self.r[14].wrapping_add((op & 0x7FF) << 1);
                self.r[14] = addr.wrapping_add(2) | 1;
                self.set_reg(15, target);
            }
        }
    }

    fn thumb_alu(&mut self, bus: &mut Bus, op: u32) {
        let rd = (op & 7) as usize;
        let rs = self.r[((op >> 3) & 7) as usize];
        let d = self.r[rd];
        let c = self.flag(C);
        let logic = |cpu: &mut Cpu, v: u32| {
            cpu.set_nz(v);
            v
        };
        let shift = |cpu: &mut Cpu, bus: &mut Bus, kind: u32| {
            bus.idle(1);
            let (v, c) = cpu.shift(kind, d, rs & 0xFF, false);
            cpu.set_nz(v);
            cpu.set_flag(C, c);
            v
        };
        let r = match (op >> 6) & 15 {
            0 => Some(logic(self, d & rs)),
            1 => Some(logic(self, d ^ rs)),
            2 => Some(shift(self, bus, 0)),
            3 => Some(shift(self, bus, 1)),
            4 => Some(shift(self, bus, 2)),
            5 => Some(self.add(d, rs, c, true)),
            6 => Some(self.sub(d, rs, c, true)),
            7 => Some(shift(self, bus, 3)),
            8 => {
                logic(self, d & rs);
                None
            }
            9 => Some(self.sub(0, rs, true, true)),
            10 => {
                self.sub(d, rs, true, true);
                None
            }
            11 => {
                self.add(d, rs, false, true);
                None
            }
            12 => Some(logic(self, d | rs)),
            13 => {
                Self::mul_cycles(bus, d, true);
                Some(logic(self, d.wrapping_mul(rs)))
            }
            14 => Some(logic(self, d & !rs)),
            _ => Some(logic(self, !rs)),
        };
        if let Some(r) = r {
            self.r[rd] = r;
        }
    }

    fn thumb_hi(&mut self, op: u32) {
        let rd = ((op & 7) | ((op >> 4) & 8)) as usize;
        let rs = self.r[((op >> 3) & 15) as usize];
        match (op >> 8) & 3 {
            0 => {
                let v = self.r[rd].wrapping_add(rs);
                self.set_reg(rd, v);
            }
            1 => {
                self.sub(self.r[rd], rs, true, true);
            }
            2 => self.set_reg(rd, rs),
            _ => {
                self.set_flag(T, rs & 1 != 0);
                self.set_reg(15, rs);
            }
        }
    }

    fn thumb_push_pop(&mut self, bus: &mut Bus, op: u32) {
        let pop = op & (1 << 11) != 0;
        let extra = op & (1 << 8) != 0;
        let list = op & 0xFF;
        if pop {
            let mut a = self.r[13];
            let mut seq = false;
            for i in 0..8 {
                if list & (1 << i) != 0 {
                    self.r[i] = bus.read32_seq(a & !3, seq);
                    seq = true;
                    a = a.wrapping_add(4);
                }
            }
            if extra {
                let v = bus.read32_seq(a & !3, seq);
                a = a.wrapping_add(4);
                self.set_reg(15, v);
            }
            bus.idle(1);
            self.r[13] = a;
        } else {
            let n = list.count_ones() + u32::from(extra);
            let mut a = self.r[13].wrapping_sub(n * 4);
            self.r[13] = a;
            let mut seq = false;
            for i in 0..8 {
                if list & (1 << i) != 0 {
                    bus.write32_seq(a, self.r[i], seq);
                    seq = true;
                    a = a.wrapping_add(4);
                }
            }
            if extra {
                bus.write32_seq(a, self.r[14], seq);
            }
        }
    }

    fn thumb_ldm_stm(&mut self, bus: &mut Bus, op: u32) {
        let load = op & (1 << 11) != 0;
        let rb = ((op >> 8) & 7) as usize;
        let list = op & 0xFF;
        let mut a = self.r[rb];
        if list == 0 {
            // Empty list: r15 is transferred and the base moves by 0x40.
            if load {
                let v = bus.read32(a & !3);
                self.set_reg(15, v);
            } else {
                bus.write32(a, self.r[15].wrapping_add(2));
            }
            self.r[rb] = a.wrapping_add(0x40);
            return;
        }
        let end = a.wrapping_add(list.count_ones() * 4);
        if load {
            let mut seq = false;
            for i in 0..8 {
                if list & (1 << i) != 0 {
                    self.r[i] = bus.read32_seq(a & !3, seq);
                    seq = true;
                    a = a.wrapping_add(4);
                }
            }
            bus.idle(1);
            if list & (1 << rb) == 0 {
                self.r[rb] = end;
            }
        } else {
            let first = list.trailing_zeros() as usize;
            for i in 0..8 {
                if list & (1 << i) != 0 {
                    let v = if i == rb && i != first { end } else { self.r[i] };
                    bus.write32_seq(a, v, i != first);
                    a = a.wrapping_add(4);
                }
            }
            self.r[rb] = end;
        }
    }
}
