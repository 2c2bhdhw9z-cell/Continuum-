//! Decoding and the integer, branch, condition-register, load/store and system instructions.
//! Floating point and paired singles are in `fpu.rs`.

use super::*;

#[inline]
pub(crate) fn rd(i: u32) -> usize {
    ((i >> 21) & 31) as usize
}
#[inline]
pub(crate) fn ra(i: u32) -> usize {
    ((i >> 16) & 31) as usize
}
#[inline]
pub(crate) fn rb(i: u32) -> usize {
    ((i >> 11) & 31) as usize
}
#[inline]
pub(crate) fn rc(i: u32) -> usize {
    ((i >> 6) & 31) as usize
}
#[inline]
fn simm(i: u32) -> u32 {
    i as i16 as i32 as u32
}
#[inline]
fn uimm(i: u32) -> u32 {
    i & 0xFFFF
}

/// The rotate mask from bit `mb` to bit `me` (IBM numbering, wrapping when mb > me).
pub fn rot_mask(mb: u32, me: u32) -> u32 {
    let m1 = u32::MAX >> mb;
    let m2 = u32::MAX << (31 - me);
    if mb <= me {
        m1 & m2
    } else {
        m1 | m2
    }
}

/// Arithmetic instructions of primary opcode 31 that have an OE (overflow) form, by their
/// 9-bit extended opcode.
const ARITH_XO9: [u32; 16] = [8, 10, 11, 40, 75, 104, 136, 138, 200, 202, 232, 234, 235, 266, 459, 491];

impl Cpu {
    /// `ra|0`: register A, or zero when the field is 0.
    #[inline]
    fn ra0(&self, i: u32) -> u32 {
        let a = ra(i);
        if a == 0 {
            0
        } else {
            self.gpr[a]
        }
    }

    fn user(&self) -> bool {
        self.msr & MSR_PR != 0
    }

    /// Faults with a privileged-instruction program exception if in user mode.
    fn privileged(&mut self) -> bool {
        if self.user() {
            self.fault(Exception::Program, PROGRAM_PRIVILEGED);
            true
        } else {
            false
        }
    }

    pub(crate) fn illegal(&mut self) {
        self.fault(Exception::Program, PROGRAM_ILLEGAL);
    }

    pub fn execute<B: Bus>(&mut self, i: u32, bus: &mut B) {
        match i >> 26 {
            3 => self.trap(i, self.gpr[ra(i)], simm(i)),
            4 => self.op4(i, bus),
            7 => self.gpr[rd(i)] = (self.gpr[ra(i)] as i32).wrapping_mul(simm(i) as i32) as u32,
            8 => {
                let a = self.gpr[ra(i)];
                let imm = simm(i);
                self.gpr[rd(i)] = imm.wrapping_sub(a);
                // Carry out of ~a + imm + 1.
                self.set_ca((!a) as u64 + imm as u64 + 1 > u32::MAX as u64);
            }
            10 => {
                let (a, b) = (self.gpr[ra(i)], uimm(i));
                self.compare(rd(i) as u32 >> 2, a.cmp(&b));
            }
            11 => {
                let (a, b) = (self.gpr[ra(i)] as i32, simm(i) as i32);
                self.compare(rd(i) as u32 >> 2, a.cmp(&b));
            }
            12 | 13 => {
                let a = self.gpr[ra(i)];
                let (r, c) = a.overflowing_add(simm(i));
                self.gpr[rd(i)] = r;
                self.set_ca(c);
                if i >> 26 == 13 {
                    self.update_cr0(r);
                }
            }
            14 => self.gpr[rd(i)] = self.ra0(i).wrapping_add(simm(i)),
            15 => self.gpr[rd(i)] = self.ra0(i).wrapping_add(i << 16),
            16 => self.bc(i, bus),
            17 => {
                // sc: the return address is the next instruction.
                let next = self.pc;
                self.raise(Exception::Syscall, next, 0);
            }
            18 => {
                let li = (((i & 0x03FF_FFFC) << 6) as i32 >> 6) as u32;
                let target = if i & 2 != 0 { li } else { self.cur_pc.wrapping_add(li) };
                if i & 1 != 0 {
                    self.lr = self.pc;
                }
                self.pc = target;
            }
            19 => self.op19(i, bus),
            20 => {
                let r = self.gpr[rd(i)].rotate_left(rb(i) as u32);
                let m = rot_mask(rc(i) as u32, (i >> 1) & 31);
                let v = (r & m) | (self.gpr[ra(i)] & !m);
                self.gpr[ra(i)] = v;
                if i & 1 != 0 {
                    self.update_cr0(v);
                }
            }
            21 => {
                let v = self.gpr[rd(i)].rotate_left(rb(i) as u32) & rot_mask(rc(i) as u32, (i >> 1) & 31);
                self.gpr[ra(i)] = v;
                if i & 1 != 0 {
                    self.update_cr0(v);
                }
            }
            23 => {
                let n = self.gpr[rb(i)] & 31;
                let v = self.gpr[rd(i)].rotate_left(n) & rot_mask(rc(i) as u32, (i >> 1) & 31);
                self.gpr[ra(i)] = v;
                if i & 1 != 0 {
                    self.update_cr0(v);
                }
            }
            24 => self.gpr[ra(i)] = self.gpr[rd(i)] | uimm(i),
            25 => self.gpr[ra(i)] = self.gpr[rd(i)] | (uimm(i) << 16),
            26 => self.gpr[ra(i)] = self.gpr[rd(i)] ^ uimm(i),
            27 => self.gpr[ra(i)] = self.gpr[rd(i)] ^ (uimm(i) << 16),
            28 => {
                let v = self.gpr[rd(i)] & uimm(i);
                self.gpr[ra(i)] = v;
                self.update_cr0(v);
            }
            29 => {
                let v = self.gpr[rd(i)] & (uimm(i) << 16);
                self.gpr[ra(i)] = v;
                self.update_cr0(v);
            }
            31 => self.op31(i, bus),
            32..=55 => self.load_store_d(i, bus),
            56 | 57 | 60 | 61 => self.psq_d(i, bus),
            59 => self.op59(i),
            63 => self.op63(i),
            _ => self.illegal(),
        }
    }

    fn compare(&mut self, field: u32, ord: std::cmp::Ordering) {
        let v = match ord {
            std::cmp::Ordering::Less => 8,
            std::cmp::Ordering::Greater => 4,
            std::cmp::Ordering::Equal => 2,
        };
        self.set_cr_field(field, v | (self.xer >> 31));
    }

    fn trap(&mut self, i: u32, a: u32, b: u32) {
        let to = rd(i) as u32;
        let (sa, sb) = (a as i32, b as i32);
        let hit = (to & 16 != 0 && sa < sb)
            || (to & 8 != 0 && sa > sb)
            || (to & 4 != 0 && a == b)
            || (to & 2 != 0 && a < b)
            || (to & 1 != 0 && a > b);
        if hit {
            self.fault(Exception::Program, PROGRAM_TRAP);
        }
    }

    // ------------------------------------------------------------------ branches

    /// Whether a conditional branch is taken, decrementing CTR when BO says so.
    fn branch_ok(&mut self, bo: u32, bi: u32, use_ctr: bool) -> bool {
        let ctr_ok = if bo & 4 != 0 || !use_ctr {
            true
        } else {
            self.ctr = self.ctr.wrapping_sub(1);
            (self.ctr != 0) != (bo & 2 != 0)
        };
        let cond_ok = bo & 0x10 != 0 || self.cr_bit(bi) == (bo & 8 != 0);
        ctr_ok && cond_ok
    }

    fn bc<B: Bus>(&mut self, i: u32, _bus: &mut B) {
        let (bo, bi) = (rd(i) as u32, ra(i) as u32);
        let bd = (i & 0xFFFC) as i16 as i32 as u32;
        let taken = self.branch_ok(bo, bi, true);
        if i & 1 != 0 {
            self.lr = self.pc;
        }
        if taken {
            self.pc = if i & 2 != 0 { bd } else { self.cur_pc.wrapping_add(bd) };
        }
    }

    fn op19<B: Bus>(&mut self, i: u32, bus: &mut B) {
        let (d, a, b) = (rd(i) as u32, ra(i) as u32, rb(i) as u32);
        match (i >> 1) & 0x3FF {
            0 => {
                let v = self.cr_field(a >> 2);
                self.set_cr_field(d >> 2, v);
            }
            16 => {
                let target = self.lr & !3;
                let taken = self.branch_ok(d, a, true);
                if i & 1 != 0 {
                    self.lr = self.pc;
                }
                if taken {
                    self.pc = target;
                }
            }
            528 => {
                let target = self.ctr & !3;
                // bcctr never decrements CTR (BO[2] = 0 is an invalid form).
                let taken = self.branch_ok(d, a, false);
                if i & 1 != 0 {
                    self.lr = self.pc;
                }
                if taken {
                    self.pc = target;
                }
            }
            50 => {
                if self.privileged() {
                    return;
                }
                self.msr = (self.msr & !MSR_SAVED) | (self.srr1 & MSR_SAVED);
                self.msr &= !MSR_POW;
                self.pc = self.srr0 & !3;
                self.tlb.flush_mode();
                self.reserve = None;
            }
            150 => {
                // isync: code may have changed under us.
                let _ = bus;
            }
            xo => {
                let (x, y) = (self.cr_bit(a), self.cr_bit(b));
                let v = match xo {
                    33 => !(x | y),
                    129 => x & !y,
                    193 => x ^ y,
                    225 => !(x & y),
                    257 => x & y,
                    289 => !(x ^ y),
                    417 => x | !y,
                    449 => x | y,
                    _ => return self.illegal(),
                };
                self.set_cr_bit(d, v);
            }
        }
    }

    // ------------------------------------------------------------------ opcode 31

    fn op31<B: Bus>(&mut self, i: u32, bus: &mut B) {
        let xo = (i >> 1) & 0x3FF;
        if ARITH_XO9.contains(&(xo & 0x1FF)) {
            return self.arith(i);
        }
        let (d, a, b) = (rd(i), ra(i), rb(i));
        let rec = i & 1 != 0;
        let logic = |cpu: &mut Cpu, v: u32| {
            cpu.gpr[a] = v;
            if rec {
                cpu.update_cr0(v);
            }
        };
        match xo {
            0 => {
                let (x, y) = (self.gpr[a] as i32, self.gpr[b] as i32);
                self.compare(d as u32 >> 2, x.cmp(&y));
            }
            32 => {
                let (x, y) = (self.gpr[a], self.gpr[b]);
                self.compare(d as u32 >> 2, x.cmp(&y));
            }
            4 => self.trap(i, self.gpr[a], self.gpr[b]),
            19 => self.gpr[d] = self.cr,
            144 => {
                let crm = (i >> 12) & 0xFF;
                let mut mask = 0u32;
                for f in 0..8 {
                    if crm & (0x80 >> f) != 0 {
                        mask |= 0xF000_0000 >> (4 * f);
                    }
                }
                self.cr = (self.cr & !mask) | (self.gpr[d] & mask);
            }
            512 => {
                let v = self.xer >> 28;
                self.set_cr_field(d as u32 >> 2, v);
                self.xer &= 0x0FFF_FFFF;
            }
            83 => {
                if !self.privileged() {
                    self.gpr[d] = self.msr;
                }
            }
            146 => {
                if !self.privileged() {
                    self.msr = self.gpr[d];
                    self.tlb.flush_mode();
                }
            }
            210 => {
                if !self.privileged() {
                    self.sr[a & 15] = self.gpr[d];
                    self.tlb.flush_all();
                }
            }
            242 => {
                if !self.privileged() {
                    self.sr[(self.gpr[b] >> 28) as usize] = self.gpr[d];
                    self.tlb.flush_all();
                }
            }
            595 => {
                if !self.privileged() {
                    self.gpr[d] = self.sr[a & 15];
                }
            }
            659 => {
                if !self.privileged() {
                    self.gpr[d] = self.sr[(self.gpr[b] >> 28) as usize];
                }
            }
            339 | 371 => {
                let n = ((i >> 16) & 0x1F) | (((i >> 11) & 0x1F) << 5);
                // SPRs with bit 4 of the number set are supervisor-only (the time base reads are not).
                if n & 0x10 != 0 && xo == 339 && self.privileged() {
                    return;
                }
                self.gpr[d] = self.read_spr(n);
            }
            467 => {
                let n = ((i >> 16) & 0x1F) | (((i >> 11) & 0x1F) << 5);
                if n & 0x10 != 0 && self.privileged() {
                    return;
                }
                let v = self.gpr[d];
                self.write_spr(n, v, bus);
            }
            24 => {
                let n = self.gpr[b] & 0x3F;
                logic(self, if n > 31 { 0 } else { self.gpr[d] << n });
            }
            536 => {
                let n = self.gpr[b] & 0x3F;
                logic(self, if n > 31 { 0 } else { self.gpr[d] >> n });
            }
            792 => {
                let n = self.gpr[b] & 0x3F;
                let s = self.gpr[d] as i32;
                let (v, ca) = if n > 31 {
                    let v = (s >> 31) as u32;
                    (v, s < 0)
                } else {
                    let v = (s >> n) as u32;
                    (v, s < 0 && n > 0 && (s as u32) & ((1u32 << n) - 1) != 0)
                };
                self.set_ca(ca);
                logic(self, v);
            }
            824 => {
                let n = b as u32;
                let s = self.gpr[d] as i32;
                let v = (s >> n) as u32;
                self.set_ca(s < 0 && n > 0 && (s as u32) & ((1u32 << n) - 1) != 0);
                logic(self, v);
            }
            26 => logic(self, self.gpr[d].leading_zeros()),
            28 => logic(self, self.gpr[d] & self.gpr[b]),
            60 => logic(self, self.gpr[d] & !self.gpr[b]),
            124 => logic(self, !(self.gpr[d] | self.gpr[b])),
            284 => logic(self, !(self.gpr[d] ^ self.gpr[b])),
            316 => logic(self, self.gpr[d] ^ self.gpr[b]),
            412 => logic(self, self.gpr[d] | !self.gpr[b]),
            444 => logic(self, self.gpr[d] | self.gpr[b]),
            476 => logic(self, !(self.gpr[d] & self.gpr[b])),
            922 => logic(self, self.gpr[d] as i16 as i32 as u32),
            954 => logic(self, self.gpr[d] as i8 as i32 as u32),
            // Cache hints and barriers: no visible effect without a modelled cache.
            54 | 86 | 246 | 278 | 598 | 854 | 566 => {}
            470 => {
                // dcbi: privileged.
                let _ = self.privileged();
            }
            306 => {
                if !self.privileged() {
                    self.tlb.flush_page(self.gpr[b]);
                }
            }
            370 => {
                if !self.privileged() {
                    self.tlb.flush_all();
                }
            }
            982 => {
                let ea = self.ra0(i).wrapping_add(self.gpr[b]) & !31;
                bus.invalidate_code(ea, 32);
            }
            1014 => {
                let ea = self.ra0(i).wrapping_add(self.gpr[b]) & !31;
                for k in 0..4 {
                    if self.write(ea + k * 8, 8, 0, bus).is_none() {
                        return;
                    }
                }
            }
            20 => {
                let ea = self.ra0(i).wrapping_add(self.gpr[b]);
                if let Some(v) = self.read(ea, 4, bus) {
                    self.gpr[d] = v as u32;
                    self.reserve = Some(ea);
                }
            }
            150 => {
                let ea = self.ra0(i).wrapping_add(self.gpr[b]);
                let so = self.xer >> 31;
                if self.reserve.is_some() {
                    if self.write(ea, 4, self.gpr[d] as u64, bus).is_none() {
                        return;
                    }
                    self.set_cr_field(0, 2 | so);
                } else {
                    self.set_cr_field(0, so);
                }
                self.reserve = None;
            }
            533 | 597 | 661 | 725 => self.string(i, xo, bus),
            534 => self.load_x(i, 4, false, false, true, bus),
            790 => self.load_x(i, 2, false, false, true, bus),
            662 => self.store_x(i, 4, false, true, bus),
            918 => self.store_x(i, 2, false, true, bus),
            310 => self.load_x(i, 4, false, false, false, bus),
            438 => self.store_x(i, 4, false, false, bus),
            23 => self.load_x(i, 4, false, false, false, bus),
            55 => self.load_x(i, 4, true, false, false, bus),
            87 => self.load_x(i, 1, false, false, false, bus),
            119 => self.load_x(i, 1, true, false, false, bus),
            279 => self.load_x(i, 2, false, false, false, bus),
            311 => self.load_x(i, 2, true, false, false, bus),
            343 => self.load_x(i, 2, false, true, false, bus),
            375 => self.load_x(i, 2, true, true, false, bus),
            151 => self.store_x(i, 4, false, false, bus),
            183 => self.store_x(i, 4, true, false, bus),
            215 => self.store_x(i, 1, false, false, bus),
            247 => self.store_x(i, 1, true, false, bus),
            407 => self.store_x(i, 2, false, false, bus),
            439 => self.store_x(i, 2, true, false, bus),
            535 | 567 | 599 | 631 | 663 | 695 | 727 | 759 | 983 => self.fp_load_store_x(i, xo, bus),
            _ => self.illegal(),
        }
    }

    fn arith(&mut self, i: u32) {
        let (d, a, b) = (rd(i), ra(i), rb(i));
        let oe = i & 0x400 != 0;
        let (x, y) = (self.gpr[a], self.gpr[b]);
        let ca = self.ca() as u32;
        // (result, carry or None when the op leaves CA alone, overflow)
        let add3 = |p: u32, q: u32, c: u32| -> (u32, Option<bool>, bool) {
            let wide = p as u64 + q as u64 + c as u64;
            let r = wide as u32;
            let ov = ((p ^ r) & (q ^ r)) >> 31 != 0;
            (r, Some(wide > u32::MAX as u64), ov)
        };
        let (r, carry, ov) = match (i >> 1) & 0x1FF {
            266 => {
                let (r, _, ov) = add3(x, y, 0);
                (r, None, ov)
            }
            10 => add3(x, y, 0),
            138 => add3(x, y, ca),
            40 => {
                let (r, _, ov) = add3(!x, y, 1);
                (r, None, ov)
            }
            8 => add3(!x, y, 1),
            136 => add3(!x, y, ca),
            202 => add3(x, 0, ca),
            234 => add3(x, u32::MAX, ca),
            200 => add3(!x, 0, ca),
            232 => add3(!x, u32::MAX, ca),
            104 => {
                let r = x.wrapping_neg();
                (r, None, x == 0x8000_0000)
            }
            235 => {
                let p = (x as i32 as i64) * (y as i32 as i64);
                (p as u32, None, p != p as i32 as i64)
            }
            75 => (((x as i32 as i64 * y as i32 as i64) >> 32) as u32, None, false),
            11 => (((x as u64 * y as u64) >> 32) as u32, None, false),
            491 => {
                let (sx, sy) = (x as i32, y as i32);
                if sy == 0 || (sx == i32::MIN && sy == -1) {
                    // Gekko leaves all ones for a negative dividend, else zero.
                    (if sx < 0 { u32::MAX } else { 0 }, None, true)
                } else {
                    ((sx / sy) as u32, None, false)
                }
            }
            459 => {
                if y == 0 {
                    (0, None, true)
                } else {
                    (x / y, None, false)
                }
            }
            _ => unreachable!(),
        };
        self.gpr[d] = r;
        if let Some(c) = carry {
            self.set_ca(c);
        }
        if oe {
            self.set_ov(ov);
        }
        if i & 1 != 0 {
            self.update_cr0(r);
        }
    }

    // ------------------------------------------------------------------ loads and stores

    fn load_store_d<B: Bus>(&mut self, i: u32, bus: &mut B) {
        let op = i >> 26;
        let (d, a) = (rd(i), ra(i));
        let update = matches!(op, 33 | 35 | 37 | 39 | 41 | 43 | 45 | 49 | 51 | 53 | 55);
        let base = if update { self.gpr[a] } else { self.ra0(i) };
        let ea = base.wrapping_add(simm(i));
        let ok = match op {
            32 | 33 => self.read(ea, 4, bus).map(|v| self.gpr[d] = v as u32),
            34 | 35 => self.read(ea, 1, bus).map(|v| self.gpr[d] = v as u32),
            40 | 41 => self.read(ea, 2, bus).map(|v| self.gpr[d] = v as u32),
            42 | 43 => self.read(ea, 2, bus).map(|v| self.gpr[d] = v as u16 as i16 as i32 as u32),
            36 | 37 => self.write(ea, 4, self.gpr[d] as u64, bus),
            38 | 39 => self.write(ea, 1, self.gpr[d] as u64, bus),
            44 | 45 => self.write(ea, 2, self.gpr[d] as u64, bus),
            46 => {
                let mut ea = ea;
                for r in d..32 {
                    match self.read(ea, 4, bus) {
                        Some(v) => self.gpr[r] = v as u32,
                        None => return,
                    }
                    ea = ea.wrapping_add(4);
                }
                Some(())
            }
            47 => {
                let mut ea = ea;
                for r in d..32 {
                    if self.write(ea, 4, self.gpr[r] as u64, bus).is_none() {
                        return;
                    }
                    ea = ea.wrapping_add(4);
                }
                Some(())
            }
            48..=55 => {
                if !self.fp_enabled() {
                    return;
                }
                self.fp_load_store(op, d, ea, bus)
            }
            _ => unreachable!(),
        };
        if ok.is_some() && update && !self.faulted {
            self.gpr[a] = ea;
        }
    }

    fn load_x<B: Bus>(&mut self, i: u32, n: u32, update: bool, algebraic: bool, reversed: bool, bus: &mut B) {
        let (d, a, b) = (rd(i), ra(i), rb(i));
        let base = if update { self.gpr[a] } else { self.ra0(i) };
        let ea = base.wrapping_add(self.gpr[b]);
        let Some(v) = self.read(ea, n, bus) else { return };
        let mut v = v as u32;
        if reversed {
            v = if n == 4 { v.swap_bytes() } else { (v as u16).swap_bytes() as u32 };
        }
        if algebraic {
            v = v as u16 as i16 as i32 as u32;
        }
        self.gpr[d] = v;
        if update {
            self.gpr[a] = ea;
        }
    }

    fn store_x<B: Bus>(&mut self, i: u32, n: u32, update: bool, reversed: bool, bus: &mut B) {
        let (s, a, b) = (rd(i), ra(i), rb(i));
        let base = if update { self.gpr[a] } else { self.ra0(i) };
        let ea = base.wrapping_add(self.gpr[b]);
        let mut v = self.gpr[s];
        if reversed {
            v = if n == 4 { v.swap_bytes() } else { (v as u16).swap_bytes() as u32 };
        }
        if self.write(ea, n, v as u64, bus).is_none() {
            return;
        }
        if update {
            self.gpr[a] = ea;
        }
    }

    /// lswi, lswx, stswi, stswx: strings of bytes through consecutive registers.
    fn string<B: Bus>(&mut self, i: u32, xo: u32, bus: &mut B) {
        let (d, b) = (rd(i), rb(i));
        let immediate = xo == 597 || xo == 725;
        let (ea, n) = if immediate {
            let nb = rb(i) as u32;
            (self.ra0(i), if nb == 0 { 32 } else { nb })
        } else {
            (self.ra0(i).wrapping_add(self.gpr[b]), self.xer & 0x7F)
        };
        let load = xo == 533 || xo == 597;
        let mut r = d;
        let mut shift = 24i32;
        if load && n > 0 {
            self.gpr[r] = 0;
        }
        for k in 0..n {
            let addr = ea.wrapping_add(k);
            if load {
                let Some(byte) = self.read(addr, 1, bus) else { return };
                self.gpr[r] |= (byte as u32) << shift;
            } else {
                let byte = (self.gpr[r] >> shift) as u8;
                if self.write(addr, 1, byte as u64, bus).is_none() {
                    return;
                }
            }
            shift -= 8;
            if shift < 0 && k + 1 < n {
                shift = 24;
                r = (r + 1) % 32;
                if load {
                    self.gpr[r] = 0;
                }
            }
        }
    }
}
