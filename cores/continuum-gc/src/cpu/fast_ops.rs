//! The fast interpreter's decoded instructions and their handlers.
//!
//! Each instruction is decoded once into an `Op`: a handler function and its operand fields,
//! already pulled out of the instruction word. The handlers for the common integer, branch and
//! load/store instructions are written out here (and checked against the plain interpreter by
//! the fuzz tests at the bottom); everything else goes through `generic`, which runs the plain
//! interpreter's own `execute`, so every instruction is covered and behaves the same.
//!
//! Handlers set `cpu.exit` when the block must stop after them: a branch, an exception, a
//! hardware register access (the interrupt line may change) or a write into cached code.

use super::interp::rot_mask;
use super::{double_to_single, mmu, single_to_double, Access, Cpu, MSR_DR, MSR_FP, XER_CA, XER_OV, XER_SO};
use crate::hw::{Hw, MEM1_SIZE};

pub type Handler = fn(&mut Cpu, &mut Hw, &Op);

#[derive(Clone, Copy)]
pub struct Op {
    pub f: Handler,
    pub inst: u32,
    pub d: u8,
    pub a: u8,
    pub b: u8,
    pub c: u8,
    pub imm: u32,
}

#[inline(always)]
fn simm(i: u32) -> u32 {
    i as i16 as i32 as u32
}

// ---------------------------------------------------------------------- flags

#[inline(always)]
fn cr0(cpu: &mut Cpu, v: u32) {
    let r = v as i32;
    let f = if r < 0 {
        8
    } else if r > 0 {
        4
    } else {
        2
    };
    cpu.cr = (cpu.cr & 0x0FFF_FFFF) | ((f | (cpu.xer >> 31)) << 28);
}

#[inline(always)]
fn set_field(cpu: &mut Cpu, field: u32, v: u32) {
    let shift = 28 - 4 * field;
    cpu.cr = (cpu.cr & !(0xF << shift)) | ((v & 0xF) << shift);
}

#[inline(always)]
fn set_ca(cpu: &mut Cpu, c: bool) {
    if c {
        cpu.xer |= XER_CA;
    } else {
        cpu.xer &= !XER_CA;
    }
}

#[inline(always)]
fn set_ov(cpu: &mut Cpu, ov: bool) {
    if ov {
        cpu.xer |= XER_OV | XER_SO;
    } else {
        cpu.xer &= !XER_OV;
    }
}

// ---------------------------------------------------------------------- memory

/// Where an access of `N` bytes lands in main memory when it is inside the data window (see
/// `mmu::data_window`) and inside one 4 KB page: the common case, decided with two compares.
#[inline(always)]
fn in_window<const N: u32>(cpu: &mut Cpu, ea: u32) -> Option<usize> {
    let off = ea.wrapping_sub(0x8000_0000);
    if cpu.msr & MSR_DR != 0 && off < mmu::data_window(cpu) && (ea & 0xFFF) + N <= 0x1000 {
        Some(off as usize)
    } else {
        None
    }
}

/// A load of `N` bytes: straight from main memory inside the data window or when the
/// translation cache knows the page, otherwise the plain interpreter's path (faults, hardware
/// registers, page crossings).
#[inline(always)]
fn load<const N: u32>(cpu: &mut Cpu, hw: &mut Hw, ea: u32) -> Option<u32> {
    if let Some(p) = in_window::<N>(cpu, ea) {
        let m = &hw.mem1[p..p + N as usize];
        return Some(match N {
            1 => m[0] as u32,
            2 => u16::from_be_bytes([m[0], m[1]]) as u32,
            _ => u32::from_be_bytes([m[0], m[1], m[2], m[3]]),
        });
    }
    if (ea & 0xFFF) + N <= 0x1000 {
        let pa = if cpu.msr & MSR_DR != 0 { mmu::translate(cpu, ea, Access::Read, hw) } else { Some(ea) };
        if let Some(pa) = pa {
            let p = pa as usize;
            if p + N as usize <= MEM1_SIZE {
                let m = &hw.mem1;
                return Some(match N {
                    1 => m[p] as u32,
                    2 => u16::from_be_bytes([m[p], m[p + 1]]) as u32,
                    _ => u32::from_be_bytes([m[p], m[p + 1], m[p + 2], m[p + 3]]),
                });
            }
        }
    }
    load_slow(cpu, hw, ea, N)
}

#[inline(never)]
fn load_slow(cpu: &mut Cpu, hw: &mut Hw, ea: u32, n: u32) -> Option<u32> {
    cpu.pc = cpu.cur_pc.wrapping_add(4);
    cpu.faulted = false;
    let v = cpu.read(ea, n, hw).map(|v| v as u32);
    after_slow(cpu, hw);
    v
}

/// After a slow memory access: stop the block if it faulted, touched something that can
/// change the interrupt line, or changed cached code.
#[inline(always)]
fn after_slow(cpu: &mut Cpu, hw: &Hw) {
    if cpu.faulted || hw.irq_dirty || !hw.invalidated.is_empty() {
        cpu.exit = true;
    }
}

/// An 8-byte load (lfd, psq).
#[inline(always)]
fn load64(cpu: &mut Cpu, hw: &mut Hw, ea: u32) -> Option<u64> {
    if let Some(p) = in_window::<8>(cpu, ea) {
        return Some(u64::from_be_bytes(hw.mem1[p..p + 8].try_into().unwrap()));
    }
    if (ea & 0xFFF) + 8 <= 0x1000 {
        let pa = if cpu.msr & MSR_DR != 0 { mmu::translate(cpu, ea, Access::Read, hw) } else { Some(ea) };
        if let Some(pa) = pa {
            let p = pa as usize;
            if p + 8 <= MEM1_SIZE {
                return Some(u64::from_be_bytes(hw.mem1[p..p + 8].try_into().unwrap()));
            }
        }
    }
    cpu.pc = cpu.cur_pc.wrapping_add(4);
    cpu.faulted = false;
    let v = cpu.read(ea, 8, hw);
    after_slow(cpu, hw);
    v
}

#[inline(always)]
fn store64(cpu: &mut Cpu, hw: &mut Hw, ea: u32, v: u64) -> bool {
    if let Some(p) = in_window::<8>(cpu, ea) {
        hw.mem1[p..p + 8].copy_from_slice(&v.to_be_bytes());
        if hw.touch_page((p >> 12) as u32) {
            cpu.exit = true;
            cpu.pc = cpu.cur_pc.wrapping_add(4);
        }
        return true;
    }
    if (ea & 0xFFF) + 8 <= 0x1000 {
        let pa = if cpu.msr & MSR_DR != 0 { mmu::translate(cpu, ea, Access::Write, hw) } else { Some(ea) };
        if let Some(pa) = pa {
            let p = pa as usize;
            if p + 8 <= MEM1_SIZE {
                hw.mem1[p..p + 8].copy_from_slice(&v.to_be_bytes());
                if hw.touch_page(pa >> 12) {
                    cpu.exit = true;
                    cpu.pc = cpu.cur_pc.wrapping_add(4);
                }
                return true;
            }
        }
    }
    cpu.pc = cpu.cur_pc.wrapping_add(4);
    cpu.faulted = false;
    let ok = cpu.write(ea, 8, v, hw).is_some();
    after_slow(cpu, hw);
    ok
}

#[inline(always)]
fn store<const N: u32>(cpu: &mut Cpu, hw: &mut Hw, ea: u32, v: u32) -> bool {
    if let Some(p) = in_window::<N>(cpu, ea) {
        let m = &mut hw.mem1[p..p + N as usize];
        match N {
            1 => m[0] = v as u8,
            2 => m.copy_from_slice(&(v as u16).to_be_bytes()),
            _ => m.copy_from_slice(&v.to_be_bytes()),
        }
        if hw.touch_page((p >> 12) as u32) {
            cpu.exit = true;
            cpu.pc = cpu.cur_pc.wrapping_add(4);
        }
        return true;
    }
    if (ea & 0xFFF) + N <= 0x1000 {
        let pa = if cpu.msr & MSR_DR != 0 { mmu::translate(cpu, ea, Access::Write, hw) } else { Some(ea) };
        if let Some(pa) = pa {
            let p = pa as usize;
            if p + N as usize <= MEM1_SIZE {
                let m = &mut hw.mem1;
                match N {
                    1 => m[p] = v as u8,
                    2 => m[p..p + 2].copy_from_slice(&(v as u16).to_be_bytes()),
                    _ => m[p..p + 4].copy_from_slice(&v.to_be_bytes()),
                }
                if hw.touch_page(pa >> 12) {
                    cpu.exit = true;
                    cpu.pc = cpu.cur_pc.wrapping_add(4);
                }
                return true;
            }
        }
    }
    store_slow(cpu, hw, ea, N, v)
}

#[inline(never)]
fn store_slow(cpu: &mut Cpu, hw: &mut Hw, ea: u32, n: u32, v: u32) -> bool {
    cpu.pc = cpu.cur_pc.wrapping_add(4);
    cpu.faulted = false;
    let ok = cpu.write(ea, n, v as u64, hw).is_some();
    after_slow(cpu, hw);
    ok
}

// ---------------------------------------------------------------------- floating point memory

#[inline(always)]
fn fp_off(cpu: &mut Cpu) -> bool {
    if cpu.msr & MSR_FP == 0 {
        cpu.pc = cpu.cur_pc.wrapping_add(4);
        cpu.faulted = false;
        cpu.fp_enabled();
        cpu.exit = true;
        return true;
    }
    false
}

macro_rules! fp_d {
    ($name:ident, $kind:literal, $update:literal) => {
        fn $name(cpu: &mut Cpu, hw: &mut Hw, op: &Op) {
            if fp_off(cpu) {
                return;
            }
            let base = if $update || op.a != 0 { cpu.gpr[op.a as usize] } else { 0 };
            let ea = base.wrapping_add(if $kind >= 4 { cpu.gpr[op.b as usize] } else { op.imm });
            let d = op.d as usize;
            let ok = match $kind & 3 {
                0 => match load::<4>(cpu, hw, ea) {
                    Some(w) => {
                        let v = single_to_double(w);
                        cpu.ps0[d] = v;
                        cpu.ps1[d] = v;
                        true
                    }
                    None => false,
                },
                1 => match load64(cpu, hw, ea) {
                    Some(v) => {
                        cpu.ps0[d] = v;
                        true
                    }
                    None => false,
                },
                2 => {
                    let w = double_to_single(cpu.ps0[d]);
                    store::<4>(cpu, hw, ea, w)
                }
                _ => {
                    let v = cpu.ps0[d];
                    store64(cpu, hw, ea, v)
                }
            };
            if ok && $update && !cpu.faulted {
                cpu.gpr[op.a as usize] = ea;
            }
        }
    };
}

fp_d!(lfs, 0, false);
fp_d!(lfsu, 0, true);
fp_d!(lfd, 1, false);
fp_d!(lfdu, 1, true);
fp_d!(stfs, 2, false);
fp_d!(stfsu, 2, true);
fp_d!(stfd, 3, false);
fp_d!(stfdu, 3, true);
fp_d!(lfsx, 4, false);
fp_d!(lfsux, 4, true);
fp_d!(lfdx, 5, false);
fp_d!(lfdux, 5, true);
fp_d!(stfsx, 6, false);
fp_d!(stfsux, 6, true);
fp_d!(stfdx, 7, false);
fp_d!(stfdux, 7, true);

// ---------------------------------------------------------------------- paired singles, quickly
//
// The most common floating-point and paired-single instructions have handlers of their own
// with the plain interpreter's quick case written in (ordinary numbers, floating point and
// paired singles on, plain main memory): the same helpers in the same order, so the results
// are bit for bit the plain interpreter's. Anything else (NaNs, infinities, a division by zero,
// an exception, an address outside the data window) goes to the plain interpreter's own group
// handler, which works it out from the start. 3D games spend much of their time here
// (Animal Crossing's N64 graphics emulation is all paired-single maths).

/// Floating point and paired singles both on.
#[inline(always)]
fn ps_on(cpu: &Cpu) -> bool {
    cpu.msr & MSR_FP != 0 && cpu.hid2 & super::HID2_PSE != 0
}

/// Bytes per element for a quantisation type (0 float; 4/6 eight-bit; 5/7 sixteen-bit).
#[inline(always)]
fn q_size(ty: u32) -> u32 {
    match ty {
        4 | 6 => 1,
        5 | 7 => 2,
        _ => 4,
    }
}

#[inline(always)]
fn be_read(m: &[u8], size: u32) -> u32 {
    match size {
        1 => m[0] as u32,
        2 => u16::from_be_bytes([m[0], m[1]]) as u32,
        _ => u32::from_be_bytes([m[0], m[1], m[2], m[3]]),
    }
}

/// A quantised load (psq_l, psq_lu, psq_lx, psq_lux) from the data window; false to take the
/// plain path. `ea` is the address, `w` one element only, `q` the GQR index.
#[inline(always)]
fn psq_load_quick(cpu: &mut Cpu, hw: &mut Hw, d: usize, ea: u32, w: bool, q: usize) -> bool {
    let gqr = cpu.gqr[q];
    let (ty, scale) = ((gqr >> 16) & 7, (gqr >> 24) & 0x3F);
    let size = q_size(ty);
    let n = if w { size } else { 2 * size };
    let off = ea.wrapping_sub(0x8000_0000);
    if cpu.msr & MSR_DR == 0 || off >= mmu::data_window(cpu) || (ea & 0xFFF) + n > 0x1000 {
        return false;
    }
    let p = off as usize;
    let m = &hw.mem1[p..p + n as usize];
    let a = be_read(m, size);
    let b = if w { None } else { Some(be_read(&m[size as usize..], size)) };
    cpu.ps0[d] = cpu.dequantize(a, ty, scale);
    cpu.ps1[d] = match b {
        Some(b) => cpu.dequantize(b, ty, scale),
        None => 1.0f64.to_bits(),
    };
    true
}

/// A quantised store from the data window; false to take the plain path.
#[inline(always)]
fn psq_store_quick(cpu: &mut Cpu, hw: &mut Hw, s: usize, ea: u32, w: bool, q: usize) -> bool {
    let gqr = cpu.gqr[q];
    let (ty, scale) = (gqr & 7, (gqr >> 8) & 0x3F);
    let size = q_size(ty);
    let n = if w { size } else { 2 * size };
    let off = ea.wrapping_sub(0x8000_0000);
    if cpu.msr & MSR_DR == 0 || off >= mmu::data_window(cpu) || (ea & 0xFFF) + n > 0x1000 {
        return false;
    }
    let a = cpu.quantize(cpu.ps0[s], ty, scale);
    let b = if w { 0 } else { cpu.quantize(cpu.ps1[s], ty, scale) };
    let p = off as usize;
    let put = |m: &mut [u8], v: u32| match size {
        1 => m[0] = v as u8,
        2 => m[..2].copy_from_slice(&(v as u16).to_be_bytes()),
        _ => m[..4].copy_from_slice(&v.to_be_bytes()),
    };
    put(&mut hw.mem1[p..], a);
    if !w {
        put(&mut hw.mem1[p + size as usize..], b);
    }
    if hw.touch_page((p >> 12) as u32) {
        cpu.exit = true;
        cpu.pc = cpu.cur_pc.wrapping_add(4);
    }
    true
}

/// psq_l, psq_lu, psq_st, psq_stu (opcodes 56, 57, 60, 61).
fn psq_quick(cpu: &mut Cpu, hw: &mut Hw, op: &Op) {
    if ps_on(cpu) {
        let i = op.inst;
        let opc = i >> 26;
        let update = opc == 57 || opc == 61;
        let a = op.a as usize;
        let base = if update || a != 0 { cpu.gpr[a] } else { 0 };
        let ea = base.wrapping_add(op.imm);
        let w = (i >> 15) & 1 != 0;
        let q = ((i >> 12) & 7) as usize;
        let done = if opc <= 57 { psq_load_quick(cpu, hw, op.d as usize, ea, w, q) } else { psq_store_quick(cpu, hw, op.d as usize, ea, w, q) };
        if done {
            if update {
                cpu.gpr[a] = ea;
            }
            return;
        }
    }
    psq(cpu, hw, op);
}

/// psq_lx, psq_stx, psq_lux, psq_stux (opcode 4, extended 6, 7, 38, 39).
fn psqx_quick(cpu: &mut Cpu, hw: &mut Hw, op: &Op) {
    if ps_on(cpu) {
        let i = op.inst;
        let xo = (i >> 1) & 0x3F;
        let update = xo >= 38;
        let a = op.a as usize;
        let base = if update || a != 0 { cpu.gpr[a] } else { 0 };
        let ea = base.wrapping_add(cpu.gpr[op.b as usize]);
        let w = (i >> 10) & 1 != 0;
        let q = ((i >> 7) & 7) as usize;
        let done = if xo & 1 == 0 { psq_load_quick(cpu, hw, op.d as usize, ea, w, q) } else { psq_store_quick(cpu, hw, op.d as usize, ea, w, q) };
        if done {
            if update {
                cpu.gpr[a] = ea;
            }
            return;
        }
    }
    ps4(cpu, hw, op);
}

use super::fpu::Op as F;

/// The arithmetic the quick handlers do: which operation, and which halves of frA, frB and frC
/// each result half takes (0: ps0, 1: ps1) for opcode 4; `op.imm` holds the kind.
const PS_ADD: u32 = 0;
const PS_SUB: u32 = 1;
const PS_MUL: u32 = 2;
const PS_MADD: u32 = 3;
const PS_MSUB: u32 = 4;
const PS_NMADD: u32 = 5;
const PS_NMSUB: u32 = 6;
const PS_MULS0: u32 = 7;
const PS_MULS1: u32 = 8;
const PS_MADDS0: u32 = 9;
const PS_MADDS1: u32 = 10;
const PS_SUM0: u32 = 11;
const PS_SUM1: u32 = 12;

/// The plain interpreter's `farith` for one result, the quick case only.
#[inline(always)]
fn quick(cpu: &mut Cpu, f: F, a: u64, b: u64, c: u64, single: bool) -> Option<u64> {
    cpu.farith_quick(f, a, b, c, single)
}

/// Paired-single arithmetic (opcode 4): ps_add, ps_sub, ps_mul, ps_madd, ps_msub, ps_nmadd,
/// ps_nmsub, ps_muls0/1, ps_madds0/1, ps_sum0/1.
fn ps_arith(cpu: &mut Cpu, hw: &mut Hw, op: &Op) {
    if ps_on(cpu) {
        let (d, a, b, c) = (op.d as usize, op.a as usize, op.b as usize, op.c as usize);
        let (a0, a1, b0, b1, c0, c1) = (cpu.ps0[a], cpu.ps1[a], cpu.ps0[b], cpu.ps1[b], cpu.ps0[c], cpu.ps1[c]);
        let madd = |sub: bool, neg: bool| F::Madd { sub, neg };
        // (operation, half 0's a, b, c; half 1's a, b, c), as the plain interpreter pairs them.
        let pair = |f: F, x: (u64, u64, u64), y: (u64, u64, u64)| (f, x, y);
        let r = match op.imm {
            PS_ADD => Some(pair(F::Add, (a0, b0, 0), (a1, b1, 0))),
            PS_SUB => Some(pair(F::Sub, (a0, b0, 0), (a1, b1, 0))),
            PS_MUL => Some(pair(F::Mul, (a0, 0, c0), (a1, 0, c1))),
            PS_MADD => Some(pair(madd(false, false), (a0, b0, c0), (a1, b1, c1))),
            PS_MSUB => Some(pair(madd(true, false), (a0, b0, c0), (a1, b1, c1))),
            PS_NMADD => Some(pair(madd(false, true), (a0, b0, c0), (a1, b1, c1))),
            PS_NMSUB => Some(pair(madd(true, true), (a0, b0, c0), (a1, b1, c1))),
            PS_MULS0 => Some(pair(F::Mul, (a0, 0, c0), (a1, 0, c0))),
            PS_MULS1 => Some(pair(F::Mul, (a0, 0, c1), (a1, 0, c1))),
            PS_MADDS0 => Some(pair(madd(false, false), (a0, b0, c0), (a1, b1, c0))),
            PS_MADDS1 => Some(pair(madd(false, false), (a0, b0, c1), (a1, b1, c1))),
            _ => None,
        };
        let done = match r {
            Some((f, x, y)) => match quick(cpu, f, x.0, x.1, x.2, true) {
                // Half 1 is worked out before anything is stored, as the plain interpreter does.
                Some(r0) => match quick(cpu, f, y.0, y.1, y.2, true) {
                    Some(r1) => {
                        cpu.ps0[d] = r0;
                        cpu.ps1[d] = r1;
                        cpu.set_fprf(r0);
                        true
                    }
                    None => false,
                },
                None => false,
            },
            // ps_sum0: (a0 + b1, c1); ps_sum1: (c0, a0 + b1).
            None => match quick(cpu, F::Add, a0, b1, 0, true) {
                Some(s) => {
                    let (r0, r1) = if op.imm == PS_SUM0 { (s, super::fpu::round_single(c1)) } else { (super::fpu::round_single(c0), s) };
                    cpu.ps0[d] = r0;
                    cpu.ps1[d] = r1;
                    cpu.set_fprf(r0);
                    true
                }
                None => false,
            },
        };
        if done {
            cpu.rec_cr1(op.inst);
            return;
        }
    }
    ps4(cpu, hw, op);
}

/// Single-precision arithmetic (opcode 59): fadds, fsubs, fmuls, fdivs, fmadds, fmsubs,
/// fnmadds, fnmsubs.
fn fp59_arith(cpu: &mut Cpu, hw: &mut Hw, op: &Op) {
    if cpu.msr & MSR_FP != 0 {
        let (fa, fb, fc) = (cpu.ps0[op.a as usize], cpu.ps0[op.b as usize], cpu.ps0[op.c as usize]);
        if let Some(r) = fp_kind(op.imm).and_then(|(f, uses_c)| quick(cpu, f, fa, if uses_c && !matches!(f, F::Madd { .. }) { 0 } else { fb }, if uses_c { fc } else { 0 }, true)) {
            let d = op.d as usize;
            cpu.ps0[d] = r;
            cpu.ps1[d] = r;
            cpu.set_fprf(r);
            cpu.rec_cr1(op.inst);
            return;
        }
    }
    fp59(cpu, hw, op);
}

/// Double-precision arithmetic (opcode 63): fadd, fsub, fmul, fdiv, fmadd, fmsub, fnmadd,
/// fnmsub (frD's second half is left alone).
fn fp63_arith(cpu: &mut Cpu, hw: &mut Hw, op: &Op) {
    if cpu.msr & MSR_FP != 0 {
        let (fa, fb, fc) = (cpu.ps0[op.a as usize], cpu.ps0[op.b as usize], cpu.ps0[op.c as usize]);
        if let Some(r) = fp_kind(op.imm).and_then(|(f, uses_c)| quick(cpu, f, fa, if uses_c && !matches!(f, F::Madd { .. }) { 0 } else { fb }, if uses_c { fc } else { 0 }, false)) {
            cpu.ps0[op.d as usize] = r;
            cpu.set_fprf(r);
            cpu.rec_cr1(op.inst);
            return;
        }
    }
    fp63(cpu, hw, op);
}

/// The operation for opcodes 59 and 63's extended codes 18-31, and whether it takes frC.
#[inline(always)]
fn fp_kind(xo5: u32) -> Option<(F, bool)> {
    Some(match xo5 {
        18 => (F::Div, false),
        20 => (F::Sub, false),
        21 => (F::Add, false),
        25 => (F::Mul, true),
        28 => (F::Madd { sub: true, neg: false }, true),
        29 => (F::Madd { sub: false, neg: false }, true),
        30 => (F::Madd { sub: true, neg: true }, true),
        31 => (F::Madd { sub: false, neg: true }, true),
        _ => return None,
    })
}

/// Paired-single moves and merges (opcode 4: ps_neg, ps_mr, ps_nabs, ps_abs, ps_merge00/01/
/// 10/11): no rounding but the merges' frA half, no flags.
fn ps_move(cpu: &mut Cpu, hw: &mut Hw, op: &Op) {
    if ps_on(cpu) {
        let (a, b) = (op.a as usize, op.b as usize);
        let (a0, a1, b0, b1) = (cpu.ps0[a], cpu.ps1[a], cpu.ps0[b], cpu.ps1[b]);
        let sign = 1u64 << 63;
        let (r0, r1) = match (op.inst >> 1) & 0x3FF {
            40 => (b0 ^ sign, b1 ^ sign),
            72 => (b0, b1),
            136 => (b0 | sign, b1 | sign),
            264 => (b0 & !sign, b1 & !sign),
            528 => (super::fpu::round_single(a0), b0),
            560 => (super::fpu::round_single(a0), b1),
            592 => (a1, b0),
            _ => (a1, b1),
        };
        let d = op.d as usize;
        cpu.ps0[d] = r0;
        cpu.ps1[d] = r1;
        cpu.rec_cr1(op.inst);
        return;
    }
    ps4(cpu, hw, op);
}

/// Paired singles (opcode 4) and the quantised loads and stores (56, 57, 60, 61): the plain
/// interpreter's own group handlers, decoded once.
fn ps4(cpu: &mut Cpu, hw: &mut Hw, op: &Op) {
    cpu.pc = cpu.cur_pc.wrapping_add(4);
    cpu.faulted = false;
    cpu.op4(op.inst, hw);
    after_slow(cpu, hw);
}

fn psq(cpu: &mut Cpu, hw: &mut Hw, op: &Op) {
    cpu.pc = cpu.cur_pc.wrapping_add(4);
    cpu.faulted = false;
    cpu.psq_d(op.inst, hw);
    after_slow(cpu, hw);
}

// ---------------------------------------------------------------------- the fallback

/// Any instruction: the plain interpreter's `execute`, with the block stopped whenever the
/// instruction did something the block runner must see.
fn generic(cpu: &mut Cpu, hw: &mut Hw, op: &Op) {
    let next = cpu.cur_pc.wrapping_add(4);
    cpu.pc = next;
    cpu.faulted = false;
    cpu.execute(op.inst, hw);
    if cpu.faulted || cpu.pc != next || hw.irq_dirty || !hw.invalidated.is_empty() || cpu.dec_pending {
        cpu.exit = true;
    }
}

/// Floating point and paired singles: straight to the plain interpreter's group handlers.
fn fp59(cpu: &mut Cpu, _hw: &mut Hw, op: &Op) {
    cpu.pc = cpu.cur_pc.wrapping_add(4);
    cpu.faulted = false;
    cpu.op59(op.inst);
    if cpu.faulted {
        cpu.exit = true;
    }
}

fn fp63(cpu: &mut Cpu, _hw: &mut Hw, op: &Op) {
    cpu.pc = cpu.cur_pc.wrapping_add(4);
    cpu.faulted = false;
    cpu.op63(op.inst);
    if cpu.faulted {
        cpu.exit = true;
    }
}

// ---------------------------------------------------------------------- integer, immediate

fn addi(cpu: &mut Cpu, _: &mut Hw, op: &Op) {
    let base = if op.a == 0 { 0 } else { cpu.gpr[op.a as usize] };
    cpu.gpr[op.d as usize] = base.wrapping_add(op.imm);
}

fn addic(cpu: &mut Cpu, _: &mut Hw, op: &Op) {
    let (r, c) = cpu.gpr[op.a as usize].overflowing_add(op.imm);
    cpu.gpr[op.d as usize] = r;
    set_ca(cpu, c);
}

fn addic_rc(cpu: &mut Cpu, _: &mut Hw, op: &Op) {
    let (r, c) = cpu.gpr[op.a as usize].overflowing_add(op.imm);
    cpu.gpr[op.d as usize] = r;
    set_ca(cpu, c);
    cr0(cpu, r);
}

fn subfic(cpu: &mut Cpu, _: &mut Hw, op: &Op) {
    let a = cpu.gpr[op.a as usize];
    cpu.gpr[op.d as usize] = op.imm.wrapping_sub(a);
    set_ca(cpu, (!a) as u64 + op.imm as u64 + 1 > u32::MAX as u64);
}

fn mulli(cpu: &mut Cpu, _: &mut Hw, op: &Op) {
    cpu.gpr[op.d as usize] = (cpu.gpr[op.a as usize] as i32).wrapping_mul(op.imm as i32) as u32;
}

fn cmpi(cpu: &mut Cpu, _: &mut Hw, op: &Op) {
    let (a, b) = (cpu.gpr[op.a as usize] as i32, op.imm as i32);
    let v = if a < b {
        8
    } else if a > b {
        4
    } else {
        2
    };
    set_field(cpu, op.d as u32, v | (cpu.xer >> 31));
}

fn cmpli(cpu: &mut Cpu, _: &mut Hw, op: &Op) {
    let (a, b) = (cpu.gpr[op.a as usize], op.imm);
    let v = if a < b {
        8
    } else if a > b {
        4
    } else {
        2
    };
    set_field(cpu, op.d as u32, v | (cpu.xer >> 31));
}

fn ori(cpu: &mut Cpu, _: &mut Hw, op: &Op) {
    cpu.gpr[op.a as usize] = cpu.gpr[op.d as usize] | op.imm;
}

fn xori(cpu: &mut Cpu, _: &mut Hw, op: &Op) {
    cpu.gpr[op.a as usize] = cpu.gpr[op.d as usize] ^ op.imm;
}

fn andi_rc(cpu: &mut Cpu, _: &mut Hw, op: &Op) {
    let v = cpu.gpr[op.d as usize] & op.imm;
    cpu.gpr[op.a as usize] = v;
    cr0(cpu, v);
}

// ---------------------------------------------------------------------- rotates

fn rlwinm(cpu: &mut Cpu, _: &mut Hw, op: &Op) {
    let v = cpu.gpr[op.d as usize].rotate_left(op.b as u32) & op.imm;
    cpu.gpr[op.a as usize] = v;
    if op.c != 0 {
        cr0(cpu, v);
    }
}

fn rlwimi(cpu: &mut Cpu, _: &mut Hw, op: &Op) {
    let r = cpu.gpr[op.d as usize].rotate_left(op.b as u32);
    let v = (r & op.imm) | (cpu.gpr[op.a as usize] & !op.imm);
    cpu.gpr[op.a as usize] = v;
    if op.c != 0 {
        cr0(cpu, v);
    }
}

fn rlwnm(cpu: &mut Cpu, _: &mut Hw, op: &Op) {
    let n = cpu.gpr[op.b as usize] & 31;
    let v = cpu.gpr[op.d as usize].rotate_left(n) & op.imm;
    cpu.gpr[op.a as usize] = v;
    if op.c != 0 {
        cr0(cpu, v);
    }
}

// ---------------------------------------------------------------------- opcode 31 integer

/// add, addc, adde, subf, subfc, subfe, addze, addme, subfze, subfme, neg, mullw, mulhw,
/// mulhwu, divw, divwu: the plain interpreter's arithmetic, with the sub-opcode decoded once.
fn arith(cpu: &mut Cpu, _: &mut Hw, op: &Op) {
    let i = op.inst;
    let (x, y) = (cpu.gpr[op.a as usize], cpu.gpr[op.b as usize]);
    let ca = (cpu.xer >> 29) & 1;
    let add3 = |p: u32, q: u32, c: u32| -> (u32, Option<bool>, bool) {
        let wide = p as u64 + q as u64 + c as u64;
        let r = wide as u32;
        let ov = ((p ^ r) & (q ^ r)) >> 31 != 0;
        (r, Some(wide > u32::MAX as u64), ov)
    };
    let (r, carry, ov) = match op.imm {
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
        104 => (x.wrapping_neg(), None, x == 0x8000_0000),
        235 => {
            let p = (x as i32 as i64) * (y as i32 as i64);
            (p as u32, None, p != p as i32 as i64)
        }
        75 => (((x as i32 as i64 * y as i32 as i64) >> 32) as u32, None, false),
        11 => (((x as u64 * y as u64) >> 32) as u32, None, false),
        491 => {
            let (sx, sy) = (x as i32, y as i32);
            if sy == 0 || (sx == i32::MIN && sy == -1) {
                (if sx < 0 { u32::MAX } else { 0 }, None, true)
            } else {
                ((sx / sy) as u32, None, false)
            }
        }
        _ => {
            if y == 0 {
                (0, None, true)
            } else {
                (x / y, None, false)
            }
        }
    };
    cpu.gpr[op.d as usize] = r;
    if let Some(c) = carry {
        set_ca(cpu, c);
    }
    if i & 0x400 != 0 {
        set_ov(cpu, ov);
    }
    if i & 1 != 0 {
        cr0(cpu, r);
    }
}

fn add(cpu: &mut Cpu, _: &mut Hw, op: &Op) {
    cpu.gpr[op.d as usize] = cpu.gpr[op.a as usize].wrapping_add(cpu.gpr[op.b as usize]);
}

fn subf(cpu: &mut Cpu, _: &mut Hw, op: &Op) {
    cpu.gpr[op.d as usize] = cpu.gpr[op.b as usize].wrapping_sub(cpu.gpr[op.a as usize]);
}

fn cmp(cpu: &mut Cpu, _: &mut Hw, op: &Op) {
    let (a, b) = (cpu.gpr[op.a as usize] as i32, cpu.gpr[op.b as usize] as i32);
    let v = if a < b {
        8
    } else if a > b {
        4
    } else {
        2
    };
    set_field(cpu, op.d as u32, v | (cpu.xer >> 31));
}

fn cmpl(cpu: &mut Cpu, _: &mut Hw, op: &Op) {
    let (a, b) = (cpu.gpr[op.a as usize], cpu.gpr[op.b as usize]);
    let v = if a < b {
        8
    } else if a > b {
        4
    } else {
        2
    };
    set_field(cpu, op.d as u32, v | (cpu.xer >> 31));
}

/// The two-register logical ops (and, andc, or, orc, xor, nor, nand, eqv) and the shifts,
/// with the sub-opcode decoded once.
fn logic(cpu: &mut Cpu, _: &mut Hw, op: &Op) {
    let (s, b) = (cpu.gpr[op.d as usize], cpu.gpr[op.b as usize]);
    let v = match op.imm {
        28 => s & b,
        60 => s & !b,
        444 => s | b,
        412 => s | !b,
        316 => s ^ b,
        124 => !(s | b),
        476 => !(s & b),
        284 => !(s ^ b),
        24 => {
            let n = b & 0x3F;
            if n > 31 {
                0
            } else {
                s << n
            }
        }
        536 => {
            let n = b & 0x3F;
            if n > 31 {
                0
            } else {
                s >> n
            }
        }
        26 => s.leading_zeros(),
        922 => s as i16 as i32 as u32,
        _ => s as i8 as i32 as u32,
    };
    cpu.gpr[op.a as usize] = v;
    if op.c != 0 {
        cr0(cpu, v);
    }
}

fn or(cpu: &mut Cpu, _: &mut Hw, op: &Op) {
    cpu.gpr[op.a as usize] = cpu.gpr[op.d as usize] | cpu.gpr[op.b as usize];
}

fn srawi(cpu: &mut Cpu, _: &mut Hw, op: &Op) {
    let n = op.b as u32;
    let s = cpu.gpr[op.d as usize] as i32;
    let v = (s >> n) as u32;
    set_ca(cpu, s < 0 && n > 0 && (s as u32) & ((1u32 << n) - 1) != 0);
    cpu.gpr[op.a as usize] = v;
    if op.c != 0 {
        cr0(cpu, v);
    }
}

fn sraw(cpu: &mut Cpu, _: &mut Hw, op: &Op) {
    let n = cpu.gpr[op.b as usize] & 0x3F;
    let s = cpu.gpr[op.d as usize] as i32;
    let (v, ca) = if n > 31 {
        ((s >> 31) as u32, s < 0)
    } else {
        ((s >> n) as u32, s < 0 && n > 0 && (s as u32) & ((1u32 << n) - 1) != 0)
    };
    set_ca(cpu, ca);
    cpu.gpr[op.a as usize] = v;
    if op.c != 0 {
        cr0(cpu, v);
    }
}

fn mfcr(cpu: &mut Cpu, _: &mut Hw, op: &Op) {
    cpu.gpr[op.d as usize] = cpu.cr;
}

fn mtcrf(cpu: &mut Cpu, _: &mut Hw, op: &Op) {
    cpu.cr = (cpu.cr & !op.imm) | (cpu.gpr[op.d as usize] & op.imm);
}

fn mflr(cpu: &mut Cpu, _: &mut Hw, op: &Op) {
    cpu.gpr[op.d as usize] = cpu.lr;
}

fn mfctr(cpu: &mut Cpu, _: &mut Hw, op: &Op) {
    cpu.gpr[op.d as usize] = cpu.ctr;
}

fn mfxer(cpu: &mut Cpu, _: &mut Hw, op: &Op) {
    cpu.gpr[op.d as usize] = cpu.xer;
}

fn mtlr(cpu: &mut Cpu, _: &mut Hw, op: &Op) {
    cpu.lr = cpu.gpr[op.d as usize];
}

fn mtctr(cpu: &mut Cpu, _: &mut Hw, op: &Op) {
    cpu.ctr = cpu.gpr[op.d as usize];
}

fn mtxer(cpu: &mut Cpu, _: &mut Hw, op: &Op) {
    cpu.xer = cpu.gpr[op.d as usize];
}

fn nop(_: &mut Cpu, _: &mut Hw, _: &Op) {}

// ---------------------------------------------------------------------- loads and stores

macro_rules! load_d {
    ($name:ident, $n:literal, $conv:expr, $update:literal) => {
        fn $name(cpu: &mut Cpu, hw: &mut Hw, op: &Op) {
            let base = if $update || op.a != 0 { cpu.gpr[op.a as usize] } else { 0 };
            let ea = base.wrapping_add(op.imm);
            if let Some(v) = load::<$n>(cpu, hw, ea) {
                cpu.gpr[op.d as usize] = $conv(v);
                if $update {
                    cpu.gpr[op.a as usize] = ea;
                }
            }
        }
    };
}

macro_rules! store_d {
    ($name:ident, $n:literal, $update:literal) => {
        fn $name(cpu: &mut Cpu, hw: &mut Hw, op: &Op) {
            let base = if $update || op.a != 0 { cpu.gpr[op.a as usize] } else { 0 };
            let ea = base.wrapping_add(op.imm);
            if store::<$n>(cpu, hw, ea, cpu.gpr[op.d as usize]) && $update && !cpu.faulted {
                cpu.gpr[op.a as usize] = ea;
            }
        }
    };
}

macro_rules! load_x {
    ($name:ident, $n:literal, $conv:expr, $update:literal) => {
        fn $name(cpu: &mut Cpu, hw: &mut Hw, op: &Op) {
            let base = if $update || op.a != 0 { cpu.gpr[op.a as usize] } else { 0 };
            let ea = base.wrapping_add(cpu.gpr[op.b as usize]);
            if let Some(v) = load::<$n>(cpu, hw, ea) {
                cpu.gpr[op.d as usize] = $conv(v);
                if $update {
                    cpu.gpr[op.a as usize] = ea;
                }
            }
        }
    };
}

macro_rules! store_x {
    ($name:ident, $n:literal, $update:literal) => {
        fn $name(cpu: &mut Cpu, hw: &mut Hw, op: &Op) {
            let base = if $update || op.a != 0 { cpu.gpr[op.a as usize] } else { 0 };
            let ea = base.wrapping_add(cpu.gpr[op.b as usize]);
            if store::<$n>(cpu, hw, ea, cpu.gpr[op.d as usize]) && $update {
                cpu.gpr[op.a as usize] = ea;
            }
        }
    };
}

fn same(v: u32) -> u32 {
    v
}
fn sext16(v: u32) -> u32 {
    v as u16 as i16 as i32 as u32
}

load_d!(lwz, 4, same, false);
load_d!(lwzu, 4, same, true);
load_d!(lbz, 1, same, false);
load_d!(lbzu, 1, same, true);
load_d!(lhz, 2, same, false);
load_d!(lhzu, 2, same, true);
load_d!(lha, 2, sext16, false);
load_d!(lhau, 2, sext16, true);
store_d!(stw, 4, false);
store_d!(stwu, 4, true);
store_d!(stb, 1, false);
store_d!(stbu, 1, true);
store_d!(sth, 2, false);
store_d!(sthu, 2, true);
load_x!(lwzx, 4, same, false);
load_x!(lwzux, 4, same, true);
load_x!(lbzx, 1, same, false);
load_x!(lbzux, 1, same, true);
load_x!(lhzx, 2, same, false);
load_x!(lhzux, 2, same, true);
load_x!(lhax, 2, sext16, false);
load_x!(lhaux, 2, sext16, true);
store_x!(stwx, 4, false);
store_x!(stwux, 4, true);
store_x!(stbx, 1, false);
store_x!(stbux, 1, true);
store_x!(sthx, 2, false);
store_x!(sthux, 2, true);

// ---------------------------------------------------------------------- branches

fn branch(cpu: &mut Cpu, _: &mut Hw, op: &Op) {
    let target = if op.c & 2 != 0 { op.imm } else { cpu.cur_pc.wrapping_add(op.imm) };
    if op.c & 1 != 0 {
        cpu.lr = cpu.cur_pc.wrapping_add(4);
    }
    cpu.pc = target;
    cpu.exit = true;
}

#[inline(always)]
fn cond(cpu: &mut Cpu, bo: u32, bi: u32, use_ctr: bool) -> bool {
    let ctr_ok = if bo & 4 != 0 || !use_ctr {
        true
    } else {
        cpu.ctr = cpu.ctr.wrapping_sub(1);
        (cpu.ctr != 0) != (bo & 2 != 0)
    };
    let cond_ok = bo & 0x10 != 0 || ((cpu.cr >> (31 - bi)) & 1 != 0) == (bo & 8 != 0);
    ctr_ok && cond_ok
}

fn bc(cpu: &mut Cpu, _: &mut Hw, op: &Op) {
    let next = cpu.cur_pc.wrapping_add(4);
    let taken = cond(cpu, op.d as u32, op.a as u32, true);
    if op.c & 1 != 0 {
        cpu.lr = next;
    }
    cpu.pc = if taken {
        if op.c & 2 != 0 {
            op.imm
        } else {
            cpu.cur_pc.wrapping_add(op.imm)
        }
    } else {
        next
    };
    cpu.exit = true;
}

fn bclr(cpu: &mut Cpu, _: &mut Hw, op: &Op) {
    let next = cpu.cur_pc.wrapping_add(4);
    let target = cpu.lr & !3;
    let taken = cond(cpu, op.d as u32, op.a as u32, true);
    if op.c & 1 != 0 {
        cpu.lr = next;
    }
    cpu.pc = if taken { target } else { next };
    cpu.exit = true;
}

fn bcctr(cpu: &mut Cpu, _: &mut Hw, op: &Op) {
    let next = cpu.cur_pc.wrapping_add(4);
    let target = cpu.ctr & !3;
    let taken = cond(cpu, op.d as u32, op.a as u32, false);
    if op.c & 1 != 0 {
        cpu.lr = next;
    }
    cpu.pc = if taken { target } else { next };
    cpu.exit = true;
}

// ---------------------------------------------------------------------- decoding

/// Whether an instruction ends its block: control flow, and anything that can change address
/// translation, the MSR, special registers that matter to the block runner, or interrupts.
pub fn ends_block(i: u32) -> bool {
    match i >> 26 {
        16 | 17 | 18 => true,
        19 => !matches!((i >> 1) & 0x3FF, 0 | 33 | 129 | 193 | 225 | 257 | 289 | 417 | 449),
        31 => match (i >> 1) & 0x3FF {
            146 | 210 | 242 | 306 | 370 | 566 | 982 => true,
            467 => !matches!(spr(i), 1 | 8 | 9),
            339 | 371 => is_time_spr(spr(i)),
            _ => false,
        },
        _ => false,
    }
}

/// Whether an instruction reads or writes the time base or the decrementer: it must be the
/// first in its block, where the time is exact.
pub fn is_timed(i: u32) -> bool {
    i >> 26 == 31 && matches!((i >> 1) & 0x3FF, 339 | 371 | 467) && is_time_spr(spr(i))
}

fn is_time_spr(n: u32) -> bool {
    matches!(n, 22 | 268 | 269 | 284 | 285)
}

fn spr(i: u32) -> u32 {
    ((i >> 16) & 0x1F) | (((i >> 11) & 0x1F) << 5)
}

pub fn decode(i: u32) -> Op {
    let d = ((i >> 21) & 31) as u8;
    let a = ((i >> 16) & 31) as u8;
    let b = ((i >> 11) & 31) as u8;
    let mk = |f: Handler, imm: u32, c: u8| Op { f, inst: i, d, a, b, c, imm };
    let rec = (i & 1) as u8;
    match i >> 26 {
        7 => mk(mulli, simm(i), 0),
        8 => mk(subfic, simm(i), 0),
        10 => mk(cmpli, i & 0xFFFF, 0).with_d(d >> 2),
        11 => mk(cmpi, simm(i), 0).with_d(d >> 2),
        12 => mk(addic, simm(i), 0),
        13 => mk(addic_rc, simm(i), 0),
        14 => mk(addi, simm(i), 0),
        15 => mk(addi, i << 16, 0),
        16 => mk(bc, (i & 0xFFFC) as i16 as i32 as u32, (i & 3) as u8),
        18 => mk(branch, (((i & 0x03FF_FFFC) << 6) as i32 >> 6) as u32, (i & 3) as u8),
        19 => match (i >> 1) & 0x3FF {
            16 => mk(bclr, 0, rec),
            528 => mk(bcctr, 0, rec),
            _ => mk(generic, 0, 0),
        },
        20 => mk(rlwimi, rot_mask(((i >> 6) & 31) as u32, (i >> 1) & 31), rec),
        21 => mk(rlwinm, rot_mask(((i >> 6) & 31) as u32, (i >> 1) & 31), rec),
        23 => mk(rlwnm, rot_mask(((i >> 6) & 31) as u32, (i >> 1) & 31), rec),
        24 => mk(ori, i & 0xFFFF, 0),
        25 => mk(ori, (i & 0xFFFF) << 16, 0),
        26 => mk(xori, i & 0xFFFF, 0),
        27 => mk(xori, (i & 0xFFFF) << 16, 0),
        28 => mk(andi_rc, i & 0xFFFF, 0),
        29 => mk(andi_rc, (i & 0xFFFF) << 16, 0),
        31 => decode31(i, d, a, b, rec),
        32 => mk(lwz, simm(i), 0),
        33 => mk(lwzu, simm(i), 0),
        34 => mk(lbz, simm(i), 0),
        35 => mk(lbzu, simm(i), 0),
        36 => mk(stw, simm(i), 0),
        37 => mk(stwu, simm(i), 0),
        38 => mk(stb, simm(i), 0),
        39 => mk(stbu, simm(i), 0),
        40 => mk(lhz, simm(i), 0),
        41 => mk(lhzu, simm(i), 0),
        42 => mk(lha, simm(i), 0),
        43 => mk(lhau, simm(i), 0),
        44 => mk(sth, simm(i), 0),
        45 => mk(sthu, simm(i), 0),
        48 => mk(lfs, simm(i), 0),
        49 => mk(lfsu, simm(i), 0),
        50 => mk(lfd, simm(i), 0),
        51 => mk(lfdu, simm(i), 0),
        52 => mk(stfs, simm(i), 0),
        53 => mk(stfsu, simm(i), 0),
        54 => mk(stfd, simm(i), 0),
        55 => mk(stfdu, simm(i), 0),
        4 => decode4(i),
        56 | 57 | 60 | 61 => mk(psq_quick, (((i & 0xFFF) << 20) as i32 >> 20) as u32, ((i >> 6) & 31) as u8),
        59 => match (i >> 1) & 0x1F {
            x @ (18 | 20 | 21 | 25 | 28..=31) => mk(fp59_arith, x, ((i >> 6) & 31) as u8),
            _ => mk(fp59, 0, 0),
        },
        63 => match (i >> 1) & 0x1F {
            x @ (18 | 20 | 21 | 25 | 28..=31) => mk(fp63_arith, x, ((i >> 6) & 31) as u8),
            _ => mk(fp63, 0, 0),
        },
        _ => mk(generic, 0, 0),
    }
    .fix_updates()
}

impl Op {
    fn with_d(mut self, d: u8) -> Op {
        self.d = d;
        self
    }

    /// Update forms with rA = 0 or rA = rD are invalid; leave them to the plain interpreter.
    fn fix_updates(self) -> Op {
        let i = self.inst;
        let op = i >> 26;
        let xo = (i >> 1) & 0x3FF;
        let update_load = matches!(op, 33 | 35 | 41 | 43) || (op == 31 && matches!(xo, 55 | 119 | 311 | 375));
        let update_store = matches!(op, 37 | 39 | 45) || (op == 31 && matches!(xo, 183 | 247 | 439));
        if (update_load && (self.a == 0 || self.a == self.d)) || (update_store && self.a == 0) {
            return Op { f: generic, ..self };
        }
        self
    }
}

/// Opcode 4: the quantised indexed loads and stores, the common paired-single arithmetic and
/// moves get their own handlers; the rest the plain interpreter's group handler.
fn decode4(i: u32) -> Op {
    let (d, a, b, c) = (((i >> 21) & 31) as u8, ((i >> 16) & 31) as u8, ((i >> 11) & 31) as u8, ((i >> 6) & 31) as u8);
    let mk = |f: Handler, imm: u32| Op { f, inst: i, d, a, b, c, imm };
    let (xo5, xo6, xo10) = ((i >> 1) & 0x1F, (i >> 1) & 0x3F, (i >> 1) & 0x3FF);
    if xo10 == 1014 {
        return mk(ps4, 0);
    }
    if matches!(xo6, 6 | 7 | 38 | 39) {
        return mk(psqx_quick, 0);
    }
    let kind = match xo5 {
        10 => Some(PS_SUM0),
        11 => Some(PS_SUM1),
        12 => Some(PS_MULS0),
        13 => Some(PS_MULS1),
        14 => Some(PS_MADDS0),
        15 => Some(PS_MADDS1),
        20 => Some(PS_SUB),
        21 => Some(PS_ADD),
        25 => Some(PS_MUL),
        28 => Some(PS_MSUB),
        29 => Some(PS_MADD),
        30 => Some(PS_NMSUB),
        31 => Some(PS_NMADD),
        _ => None,
    };
    if let Some(k) = kind {
        return mk(ps_arith, k);
    }
    if matches!(xo10, 40 | 72 | 136 | 264 | 528 | 560 | 592 | 624) {
        return mk(ps_move, 0);
    }
    mk(ps4, 0)
}

fn decode31(i: u32, d: u8, a: u8, b: u8, rec: u8) -> Op {
    let mk = |f: Handler, imm: u32, c: u8| Op { f, inst: i, d, a, b, c, imm };
    let xo = (i >> 1) & 0x3FF;
    let xo9 = xo & 0x1FF;
    if matches!(xo9, 8 | 10 | 11 | 40 | 75 | 104 | 136 | 138 | 200 | 202 | 232 | 234 | 235 | 266 | 459 | 491) {
        // Plain add and subf without OE or Rc are the most common: their own handlers.
        if i & 0x401 == 0 && xo9 == 266 {
            return mk(add, 0, 0);
        }
        if i & 0x401 == 0 && xo9 == 40 {
            return mk(subf, 0, 0);
        }
        return mk(arith, xo9, 0);
    }
    match xo {
        0 => mk(cmp, 0, 0).with_d(d >> 2),
        32 => mk(cmpl, 0, 0).with_d(d >> 2),
        444 if rec == 0 => mk(or, 0, 0),
        28 | 60 | 444 | 412 | 316 | 124 | 476 | 284 | 24 | 536 | 26 | 922 | 954 => mk(logic, xo, rec),
        824 => mk(srawi, 0, rec),
        792 => mk(sraw, 0, rec),
        19 => mk(mfcr, 0, 0),
        144 => {
            let crm = (i >> 12) & 0xFF;
            let mut mask = 0u32;
            for f in 0..8 {
                if crm & (0x80 >> f) != 0 {
                    mask |= 0xF000_0000 >> (4 * f);
                }
            }
            mk(mtcrf, mask, 0)
        }
        339 => match spr(i) {
            8 => mk(mflr, 0, 0),
            9 => mk(mfctr, 0, 0),
            1 => mk(mfxer, 0, 0),
            _ => mk(generic, 0, 0),
        },
        467 => match spr(i) {
            8 => mk(mtlr, 0, 0),
            9 => mk(mtctr, 0, 0),
            1 => mk(mtxer, 0, 0),
            _ => mk(generic, 0, 0),
        },
        // dcbst, dcbf, dcbt, dcbtst, sync, eieio: no visible effect.
        54 | 86 | 246 | 278 | 598 | 854 => mk(nop, 0, 0),
        23 => mk(lwzx, 0, 0),
        55 => mk(lwzux, 0, 0),
        87 => mk(lbzx, 0, 0),
        119 => mk(lbzux, 0, 0),
        279 => mk(lhzx, 0, 0),
        311 => mk(lhzux, 0, 0),
        343 => mk(lhax, 0, 0),
        375 => mk(lhaux, 0, 0),
        151 => mk(stwx, 0, 0),
        183 => mk(stwux, 0, 0),
        215 => mk(stbx, 0, 0),
        247 => mk(stbux, 0, 0),
        407 => mk(sthx, 0, 0),
        439 => mk(sthux, 0, 0),
        535 => mk(lfsx, 0, 0),
        567 => mk(lfsux, 0, 0),
        599 => mk(lfdx, 0, 0),
        631 => mk(lfdux, 0, 0),
        663 => mk(stfsx, 0, 0),
        695 => mk(stfsux, 0, 0),
        727 => mk(stfdx, 0, 0),
        759 => mk(stfdux, 0, 0),
        _ => mk(generic, 0, 0),
    }
}

#[cfg(test)]
mod tests {
    //! Every decoded handler against the plain interpreter: random instructions, random
    //! registers, the same memory; after one instruction all of the CPU and the touched
    //! memory must match exactly.
    use super::*;
    use crate::cpu::{HID2_LSQE, HID2_PSE, MSR_FP, MSR_IR};

    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u32 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            (self.0 >> 16) as u32
        }
    }

    fn cpu_setup(cpu: &mut Cpu, rng: &mut Rng) {
        cpu.ibat[0] = [0x8000_1FFF, 0x0000_0002];
        cpu.dbat[0] = [0x8000_1FFF, 0x0000_0002];
        cpu.msr = MSR_IR | MSR_DR | if rng.next() % 16 == 0 { 0 } else { MSR_FP };
        // Floating-point values: plain numbers of every size, numbers a single holds, and the
        // special ones (zeros, infinities, NaNs, single denormals), so both the quick and the
        // full paths are taken.
        let fp = |rng: &mut Rng| -> u64 {
            match rng.next() % 8 {
                0 => ((rng.next() as u64) << 32) | rng.next() as u64,
                1 => [0u64, 1 << 63, 0x7FF0_0000_0000_0000, 0xFFF0_0000_0000_0000, 0x7FF8_0000_0000_0000, 0x7FF4_0000_0000_0001, 0x3810_0000_0000_0000, 0x3690_0000_0000_0000, 0x47EF_FFFF_E000_0000][(rng.next() % 9) as usize],
                2 => super::super::single_to_double(rng.next() & 0x807F_FFFF),
                _ => super::super::single_to_double(rng.next()),
            }
        };
        for r in 0..32 {
            cpu.ps0[r] = fp(rng);
            cpu.ps1[r] = fp(rng);
        }
        for q in 0..8 {
            cpu.gqr[q] = if rng.next() & 1 == 0 { 0 } else { rng.next() & 0x3F07_3F07 };
        }
        cpu.fpscr = if rng.next() & 1 == 0 { 0 } else { rng.next() & !0x0000_0800 };
        cpu.hid2 = if rng.next() & 15 == 0 { HID2_LSQE } else { HID2_PSE | HID2_LSQE };
        for r in 0..32 {
            // Mostly addresses inside the test window, some small numbers, some anything.
            cpu.gpr[r] = match rng.next() % 8 {
                0 => 0x8010_0000 + (rng.next() & 0xFFFC),
                1 => rng.next() & 0xFF,
                2 => (rng.next() as i32 >> (rng.next() % 31)) as u32,
                3 => [0u32, 1, 0xFFFF_FFFF, 0x8000_0000, 0x7FFF_FFFF, 0xFFFF, 0x8000, 0xFFFF_8000, 31, 32, 33, 63][(rng.next() % 12) as usize],
                4 => 0x8010_0000 + (rng.next() & 0xFFFF),
                _ => rng.next(),
            };
        }
        cpu.gpr[0] = rng.next() & 0x7;
        cpu.cr = rng.next();
        cpu.xer = rng.next() & 0xE000_007F;
        cpu.lr = 0x8000_4000 + (rng.next() & 0xFFC);
        cpu.ctr = rng.next() % 4;
        cpu.tlb.flush_all();
    }

    /// Instruction templates: primary opcode (and extended opcode for 19/31), with random fields.
    fn random_inst(rng: &mut Rng) -> u32 {
        let pick = [7u32, 8, 10, 11, 12, 13, 14, 15, 16, 18, 19, 20, 21, 23, 24, 25, 26, 27, 28, 29, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 48, 49, 50, 51, 52, 53, 54, 55, 48, 50, 52, 54, 31, 31, 31, 31, 31, 31, 31, 31, 31, 31, 31, 31, 31, 31, 31, 31, 31, 31, 31, 31, 31, 31, 31, 31, 31, 31, 31, 31, 31, 31, 31, 31, 31, 31, 4, 4, 4, 4, 4, 4, 4, 4, 56, 57, 60, 61, 59, 59, 59, 59, 63, 63, 63, 63];
        let op = pick[(rng.next() as usize) % pick.len()];
        let fields = rng.next() & 0x03FF_FFFF;
        match op {
            19 => {
                let xo = [16u32, 528, 257, 193, 0][(rng.next() % 5) as usize];
                (19 << 26) | (fields & 0x03FF_F801) | (xo << 1)
            }
            31 => {
                let xos = [266u32, 10, 138, 40, 8, 136, 202, 234, 200, 232, 104, 235, 75, 11, 491, 459, 0, 32, 28, 60, 444, 412, 316, 124, 476, 284, 24, 536, 26, 922, 954, 824, 792, 19, 144, 23, 55, 87, 119, 279, 311, 343, 375, 151, 183, 215, 247, 407, 439, 339, 467, 535, 567, 599, 631, 663, 695, 727, 759];
                let xo = xos[(rng.next() as usize) % xos.len()];
                let mut i = (31 << 26) | (fields & 0x03FF_F801) | (xo << 1);
                if xo == 339 || xo == 467 {
                    // LR, CTR or XER.
                    let n = [8u32, 9, 1][(rng.next() % 3) as usize];
                    i = (i & !0x001F_F800) | ((n & 31) << 16) | ((n >> 5) << 11);
                }
                if matches!(xo, 266 | 10 | 138 | 40 | 8 | 136 | 202 | 234 | 200 | 232 | 104 | 235 | 75 | 11 | 491 | 459) {
                    i |= rng.next() & 0x400;
                }
                i
            }
            4 => {
                // Paired singles: mostly the arithmetic, moves and indexed quantised accesses.
                let xo = [10u32, 11, 12, 13, 14, 15, 18, 20, 21, 23, 24, 25, 26, 28, 29, 30, 31][(rng.next() % 17) as usize];
                match rng.next() % 4 {
                    0 => (4 << 26) | (fields & 0x03FF_FFC1) | (xo << 1),
                    1 => (4 << 26) | (fields & 0x03FF_F801) | ([40u32, 72, 136, 264, 528, 560, 592, 624, 0, 32, 64, 96][(rng.next() % 12) as usize] << 1),
                    2 => (4 << 26) | (fields & 0x03FF_FF81) | ([6u32, 7, 38, 39][(rng.next() % 4) as usize] << 1),
                    _ => (4 << 26) | fields,
                }
            }
            59 | 63 => {
                let xo = [18u32, 20, 21, 24, 25, 26, 28, 29, 30, 31, 23][(rng.next() % 11) as usize];
                (op << 26) | (fields & 0x03FF_FFC1) | (xo << 1)
            }
            _ => (op << 26) | fields,
        }
    }

    #[test]
    fn handlers_match_the_plain_interpreter() {
        let mut rng = Rng(0x1234_5678_9ABC_DEF1);
        let mut hw_a = Hw::new(false);
        let mut hw_b = Hw::new(false);
        let pc = 0x8000_3100u32;
        // The same memory window in both (it stays the same as long as every case matches).
        for k in (0..0x1_0000).step_by(4) {
            let w = rng.next();
            hw_a.mem1[0x10_0000 + k..0x10_0000 + k + 4].copy_from_slice(&w.to_be_bytes());
            hw_b.mem1[0x10_0000 + k..0x10_0000 + k + 4].copy_from_slice(&w.to_be_bytes());
        }
        let mut a = Cpu::default();
        for n in 0..3_000_000 {
            let inst = random_inst(&mut rng);
            cpu_setup(&mut a, &mut rng);
            a.pc = pc;
            let mut b = a.clone();
            hw_a.mem1[0x3100..0x3104].copy_from_slice(&inst.to_be_bytes());
            hw_b.mem1[0x3100..0x3104].copy_from_slice(&inst.to_be_bytes());
            a.step(&mut hw_a);
            let op = decode(inst);
            b.exit = false;
            b.cur_pc = pc;
            b.pc = pc.wrapping_add(4);
            b.faulted = false;
            (op.f)(&mut b, &mut hw_b, &op);
            b.advance_time(1);
            let same = a.ps0 == b.ps0 && a.ps1 == b.ps1 && a.fpscr == b.fpscr && a.gpr == b.gpr && a.cr == b.cr && a.xer == b.xer && a.lr == b.lr && a.ctr == b.ctr && a.pc == b.pc && a.msr == b.msr && a.srr0 == b.srr0 && a.srr1 == b.srr1 && a.dar == b.dar && a.dsisr == b.dsisr && a.cycles == b.cycles;
            assert!(same, "instruction {inst:08X} (case {n}) differs:\n plain gpr {:08X?} cr {:08X} xer {:08X} lr {:08X} ctr {:08X} pc {:08X}\n fast  gpr {:08X?} cr {:08X} xer {:08X} lr {:08X} ctr {:08X} pc {:08X}", a.gpr, a.cr, a.xer, a.lr, a.ctr, a.pc, b.gpr, b.cr, b.xer, b.lr, b.ctr, b.pc);
            assert!(hw_a.mem1[0x10_0000..0x11_0000] == hw_b.mem1[0x10_0000..0x11_0000], "instruction {inst:08X} wrote memory differently");
        }
    }
}
