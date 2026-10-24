//! The block compiler: one block of PowerPC instructions to ARM64 code.
//!
//! Generated code is called as `extern "C" fn(cpu, hw, ctx, budget) -> budget left`, where the
//! budget is how many instructions it may run. It keeps x19 = &Cpu, x20 = &Hw, x21 = &JitCtx,
//! x22 = main memory, x23 = the code-page bitmap and x25 = the budget left.
//!
//! Every instruction is either compiled natively (integer arithmetic and logic, rotates,
//! compares, LR/CTR moves, loads and stores with an inline main-memory path, branches) or
//! runs the fast interpreter's own handler for it through one helper call. The native code
//! does exactly what that handler does; whenever its quick path does not apply (an address
//! outside the standard block mapping, a page holding cached code), it calls the handler too.
//! So the results are the fast interpreter's, which are the plain interpreter's.
//!
//! Blocks chain: an exit whose target is known can be patched (by the runtime) to jump
//! straight into the target block's second entry, which skips the register set-up and first
//! checks that the budget left covers the whole block (else it returns, with the PC at its
//! start, for the runtime to finish exactly as the interpreter would).

use super::a64::{Asm, Cond, Shift, R};
use crate::cpu::fast_ops::{ends_block, Op};
use crate::cpu::Cpu;
use std::mem::offset_of;

/// What generated code needs that is not in the CPU or the hardware.
#[repr(C)]
pub struct JitCtx {
    pub mem1: u64,
    pub code_pages: u64,
    /// `fallback(cpu, hw, op)`: runs one instruction's fast-interpreter handler.
    pub fallback: u64,
    /// Main-memory offsets (effective address - 0x80000000) below this take the inline path
    /// for any access of up to 8 bytes: the data BAT's window less 7.
    pub mem_limit: u64,
    /// Compiled blocks by effective address, for exits whose target is in a register (blr,
    /// bctr): `TABLE_SIZE` entries of (address, code address to jump to), indexed by
    /// address bits 2 up; an address that is not there returns to the runtime.
    pub table: u64,
}

/// Entries in the indirect-branch table.
pub const TABLE_BITS: u32 = 16;
pub const TABLE_SIZE: usize = 1 << TABLE_BITS;
/// An empty table entry's address (never a block's: blocks start on 4-byte boundaries).
pub const TABLE_EMPTY: u64 = 1;

/// The word a link site holds while it is not linked: a branch to the next instruction, which
/// is the stub that stores the target PC and returns.
pub const UNLINKED: u32 = 0x1400_0001;

const CPU: R = 19;
const HW: R = 20;
const CTX: R = 21;
const MEM: R = 22;
const PAGES: R = 23;
const LIMIT: R = 24;
const BUDGET: R = 25;

fn gpr(r: u8) -> u32 {
    (offset_of!(Cpu, gpr) + 4 * r as usize) as u32
}
fn off_cr() -> u32 {
    offset_of!(Cpu, cr) as u32
}
fn off_xer() -> u32 {
    offset_of!(Cpu, xer) as u32
}
fn off_lr() -> u32 {
    offset_of!(Cpu, lr) as u32
}
fn off_ctr() -> u32 {
    offset_of!(Cpu, ctr) as u32
}
fn off_pc() -> u32 {
    offset_of!(Cpu, pc) as u32
}
fn off_cur_pc() -> u32 {
    offset_of!(Cpu, cur_pc) as u32
}
fn off_exit() -> u32 {
    offset_of!(Cpu, exit) as u32
}
fn off_msr() -> u32 {
    offset_of!(Cpu, msr) as u32
}
fn off_fpscr() -> u32 {
    offset_of!(Cpu, fpscr) as u32
}
fn ps0(r: u8) -> u32 {
    (offset_of!(Cpu, ps0) + 8 * r as usize) as u32
}
fn ps1(r: u8) -> u32 {
    (offset_of!(Cpu, ps1) + 8 * r as usize) as u32
}
fn off_hid2() -> u32 {
    offset_of!(Cpu, hid2) as u32
}
fn gqr(n: u32) -> u32 {
    (offset_of!(Cpu, gqr) + 4 * n as usize) as u32
}

/// Everything the compiler needs to know about one instruction.
pub struct Inst<'a> {
    pub ea: u32,
    pub op: &'a Op,
}

/// The code for a block, and where its exits are.
pub struct Compiled {
    pub code: Vec<u32>,
    /// Where chained blocks enter (word index): after the set-up, at the budget check.
    pub inner: usize,
    /// Exits with a known target: (word index of the branch to patch, target effective address).
    pub links: Vec<(usize, u32)>,
    /// How many instructions were compiled natively (the rest run their handlers).
    pub native: usize,
}

struct Ctx {
    a: Asm,
    /// Branches to patch to the shared exit stub for a given count: (at, count, kind).
    exits: Vec<(usize, u32, ExitKind)>,
    /// Branches to the epilogue itself: (at, condition; Al for an unconditional `b`).
    to_epilogue: Vec<(usize, Cond)>,
    links: Vec<(usize, u32)>,
    fast_mem: bool,
}

#[derive(Clone, Copy)]
enum ExitKind {
    B,
    Cbnz(R),
}

impl Ctx {
    fn ldr(&mut self, rt: R, off: u32) {
        self.a.ldr_w(rt, CPU, off);
    }
    fn str(&mut self, rt: R, off: u32) {
        self.a.str_w(rt, CPU, off);
    }
    fn load_gpr(&mut self, rt: R, r: u8) {
        self.ldr(rt, gpr(r));
    }
    fn store_gpr(&mut self, rt: R, r: u8) {
        self.str(rt, gpr(r));
    }
    /// rt = rA, or 0 for rA = 0.
    fn load_ra0(&mut self, rt: R, r: u8) {
        if r == 0 {
            self.a.mov_imm(false, rt, 0);
        } else {
            self.load_gpr(rt, r);
        }
    }
    /// rd = rn + imm (any 32-bit immediate).
    fn add_imm32(&mut self, rd: R, rn: R, imm: u32, tmp: R) {
        if imm == 0 {
            if rd != rn {
                self.a.mov(false, rd, rn);
            }
        } else if imm < 4096 {
            self.a.add_imm(false, rd, rn, imm);
        } else if imm.wrapping_neg() < 4096 {
            self.a.sub_imm(false, rd, rn, imm.wrapping_neg());
        } else {
            self.a.mov_imm(false, tmp, imm as u64);
            self.a.add(false, rd, rn, tmp);
        }
    }
    /// Leaves the block after `count` instructions (pc already stored).
    fn exit(&mut self, count: u32) {
        let at = self.a.here();
        self.a.emit(0);
        self.exits.push((at, count, ExitKind::B));
    }
    /// Leaves the block after `count` instructions for `target`: through a branch the runtime
    /// may point straight at the target block, else storing the PC and returning.
    fn exit_to(&mut self, count: u32, target: u32) {
        self.a.sub_imm(true, BUDGET, BUDGET, count);
        self.links.push((self.a.here(), target));
        self.a.emit(UNLINKED);
        self.a.mov_imm(false, 0, target as u64);
        self.str(0, off_pc());
        self.b_epilogue();
    }
    fn b_epilogue(&mut self) {
        self.to_epilogue.push((self.a.here(), Cond::Al));
        self.a.emit(0);
    }
    /// Leaves the block after `count` instructions for the address in w6 (PC already
    /// stored): straight into its compiled block when the table has it, else returning.
    fn exit_indirect(&mut self, count: u32) {
        self.a.sub_imm(true, BUDGET, BUDGET, count);
        self.a.ldr_x(7, CTX, offset_of!(JitCtx, table) as u32);
        self.a.ubfx(true, 8, 6, 2, TABLE_BITS);
        self.a.addsub_reg(true, false, false, 7, 7, 8, Shift::Lsl, 4);
        self.a.ldp_off(9, 10, 7, 0);
        self.a.cmp(true, 9, 6);
        self.to_epilogue.push((self.a.here(), Cond::Ne));
        self.a.emit(0);
        self.a.br(10);
    }
    /// The CR field nibble from the flags of a compare (signed or not) plus XER[SO], into w1.
    fn cr_nibble(&mut self, signed: bool) {
        let (lt, gt) = if signed { (Cond::Lt, Cond::Gt) } else { (Cond::Lo, Cond::Hi) };
        self.a.cset(false, 1, lt);
        self.a.cset(false, 2, gt);
        self.a.cset(false, 3, Cond::Eq);
        self.a.addsub_reg(false, false, false, 1, 2, 1, Shift::Lsl, 1);
        self.a.addsub_reg(false, false, false, 1, 3, 1, Shift::Lsl, 1);
        self.a.lsl_imm(false, 1, 1, 1);
        self.ldr(4, off_xer());
        self.a.logic_reg(false, 0b01, false, 1, 1, 4, Shift::Lsr, 31);
    }
    /// Puts the nibble in w1 into CR field `field`.
    fn set_cr_field(&mut self, field: u32) {
        self.ldr(5, off_cr());
        self.a.bfi(false, 5, 1, 28 - 4 * field, 4);
        self.str(5, off_cr());
    }
    /// CR0 from the 32-bit result in `r`.
    fn cr0(&mut self, r: R) {
        self.a.cmp_imm(false, r, 0);
        self.cr_nibble(true);
        self.set_cr_field(0);
    }

    /// Runs the instruction's own fast-interpreter handler; leaves the block if it says so.
    fn fallback(&mut self, ins: &Inst, count: u32) {
        self.a.mov_imm(false, 0, ins.ea as u64);
        self.str(0, off_cur_pc());
        // The budget left at this instruction (the budget register holds what was left at
        // the block's start), so a hardware access in the handler sees the time (`Cpu::now`).
        self.a.sub_imm(true, 0, BUDGET, count - 1);
        self.a.str_x_any(0, CPU, offset_of!(Cpu, jit_left) as u32, 1);
        self.a.mov(true, 0, CPU);
        self.a.mov(true, 1, HW);
        self.a.mov_imm(true, 2, ins.op as *const Op as u64);
        self.a.ldr_x(16, CTX, offset_of!(JitCtx, fallback) as u32);
        self.a.blr(16);
        self.a.ldrb(0, CPU, off_exit());
        let at = self.a.here();
        self.a.emit(0);
        self.exits.push((at, count, ExitKind::Cbnz(0)));
    }
}

/// The ARM64 32-bit EOR with 0x80000000 (logical immediate N=0, immr=1, imms=0).
fn eor_top_bit(rd: R, rn: R) -> u32 {
    0x5201_0000 | (rn << 5) | rd
}

pub fn compile(insts: &[Inst], fast_mem: bool) -> Compiled {
    let mut c = Ctx { a: Asm::new(), exits: Vec::new(), to_epilogue: Vec::new(), links: Vec::new(), fast_mem };
    // Prologue (the entry the runtime calls).
    c.a.stp_pre(29, 30, 31, -16);
    c.a.stp_pre(19, 20, 31, -16);
    c.a.stp_pre(21, 22, 31, -16);
    c.a.stp_pre(23, 24, 31, -16);
    c.a.stp_pre(25, 26, 31, -16);
    c.a.mov(true, CPU, 0);
    c.a.mov(true, HW, 1);
    c.a.mov(true, CTX, 2);
    c.a.mov(true, BUDGET, 3);
    c.a.ldr_x(MEM, CTX, offset_of!(JitCtx, mem1) as u32);
    c.a.ldr_x(PAGES, CTX, offset_of!(JitCtx, code_pages) as u32);
    c.a.ldr_x(LIMIT, CTX, offset_of!(JitCtx, mem_limit) as u32);
    // The entry chained blocks use: is there budget for the whole block?
    let inner = c.a.here();
    let start = insts.first().map_or(0, |i| i.ea);
    c.a.cmp_imm(true, BUDGET, insts.len() as u32);
    let bail_at = c.a.here();
    c.a.emit(0);
    let mut native_count = 0;
    let mut ended = false;
    for (k, ins) in insts.iter().enumerate() {
        let count = k as u32 + 1;
        if native(&mut c, ins, count) {
            native_count += 1;
        } else {
            c.fallback(ins, count);
        }
        if is_branch(ins.op.inst) {
            ended = true;
            break;
        }
    }
    if !ended {
        let next = insts.last().map_or(0, |i| i.ea.wrapping_add(4));
        let last = insts.last().map_or(0, |i| i.op.inst);
        // After an instruction that can change translation, the MSR or the BATs (all of which
        // end blocks and run their handlers), the runtime must look again before going on.
        if insts.last().is_some_and(|i| ends_block(i.op.inst)) && !is_native_kind(last) {
            c.a.mov_imm(false, 0, next as u64);
            c.str(0, off_pc());
            c.exit(insts.len() as u32);
        } else {
            c.exit_to(insts.len() as u32, next);
        }
    }
    // Not enough budget: return with the PC at the block's start, nothing run.
    let bail = c.a.here();
    c.a.patch(bail_at, Asm::b_cond_to(bail_at, bail, Cond::Lo));
    c.a.mov_imm(false, 0, start as u64);
    c.str(0, off_pc());
    c.b_epilogue();
    // Exit stubs: "sub x25, x25, #count; b epilogue".
    let exits = std::mem::take(&mut c.exits);
    let mut counts: Vec<u32> = exits.iter().map(|e| e.1).collect();
    counts.sort_unstable();
    counts.dedup();
    let mut stubs: Vec<(u32, usize)> = Vec::new();
    for &n in &counts {
        stubs.push((n, c.a.here()));
        c.a.sub_imm(true, BUDGET, BUDGET, n);
        c.b_epilogue();
    }
    let epilogue = c.a.here();
    c.a.mov(true, 0, BUDGET);
    c.a.ldp_post(25, 26, 31, 16);
    c.a.ldp_post(23, 24, 31, 16);
    c.a.ldp_post(21, 22, 31, 16);
    c.a.ldp_post(19, 20, 31, 16);
    c.a.ldp_post(29, 30, 31, 16);
    c.a.ret();
    for (at, cond) in std::mem::take(&mut c.to_epilogue) {
        let w = if cond == Cond::Al { c.a.b_to(at, epilogue, false) } else { Asm::b_cond_to(at, epilogue, cond) };
        c.a.patch(at, w);
    }
    for (at, n, kind) in exits {
        let stub = stubs.iter().find(|s| s.0 == n).unwrap().1;
        let w = match kind {
            ExitKind::B => c.a.b_to(at, stub, false),
            ExitKind::Cbnz(r) => Asm::cbz_to(false, true, r, at, stub),
        };
        c.a.patch(at, w);
    }
    let _ = c.fast_mem;
    Compiled { code: c.a.code, inner, links: c.links, native: native_count }
}

/// Whether an instruction that ends a block is one this compiler always does natively (the
/// branches), so the block's fall-through exit may chain.
fn is_native_kind(i: u32) -> bool {
    is_branch(i)
}

fn is_branch(i: u32) -> bool {
    match i >> 26 {
        16 | 18 => true,
        19 => matches!((i >> 1) & 0x3FF, 16 | 528),
        _ => false,
    }
}

/// Compiles one instruction natively. Returns false to use its handler instead.
fn native(c: &mut Ctx, ins: &Inst, count: u32) -> bool {
    let op = ins.op;
    let i = op.inst;
    let (d, a, b) = (op.d, op.a, op.b);
    let rec = i & 1 != 0;
    match i >> 26 {
        14 | 15 => {
            // addi, addis
            c.load_ra0(0, a);
            c.add_imm32(0, 0, op.imm, 1);
            c.store_gpr(0, d);
        }
        24 | 25 | 26 | 27 => {
            // ori, oris, xori, xoris
            c.load_gpr(0, d);
            c.a.mov_imm(false, 1, op.imm as u64);
            if i >> 26 < 26 {
                c.a.orr(false, 0, 0, 1);
            } else {
                c.a.eor(false, 0, 0, 1);
            }
            c.store_gpr(0, a);
        }
        28 | 29 => {
            // andi., andis.
            c.load_gpr(0, d);
            c.a.mov_imm(false, 1, op.imm as u64);
            c.a.and(false, 0, 0, 1);
            c.store_gpr(0, a);
            c.cr0(0);
        }
        10 | 11 => {
            // cmpli, cmpi (d holds the field)
            c.load_gpr(0, a);
            c.a.mov_imm(false, 1, op.imm as u64);
            c.a.cmp(false, 0, 1);
            c.cr_nibble(i >> 26 == 11);
            c.set_cr_field(d as u32);
        }
        21 => {
            // rlwinm
            c.load_gpr(0, d);
            if b != 0 {
                c.a.ror_imm(false, 0, 0, 32 - b as u32);
            }
            c.a.mov_imm(false, 1, op.imm as u64);
            c.a.and(false, 0, 0, 1);
            c.store_gpr(0, a);
            if rec {
                c.cr0(0);
            }
        }
        20 => {
            // rlwimi
            c.load_gpr(0, d);
            if b != 0 {
                c.a.ror_imm(false, 0, 0, 32 - b as u32);
            }
            c.a.mov_imm(false, 1, op.imm as u64);
            c.a.and(false, 0, 0, 1);
            c.load_gpr(2, a);
            c.a.bic(false, 2, 2, 1);
            c.a.orr(false, 0, 0, 2);
            c.store_gpr(0, a);
            if rec {
                c.cr0(0);
            }
        }
        31 => return native31(c, ins, count),
        59 | 63 if fp_kind(i).is_some() => fp_native(c, ins, count),
        48..=55 | 56 | 57 | 60 | 61 if c.fast_mem => fp_mem(c, ins, count),
        4 if ps_kind(i).is_some() => ps_native(c, ins, count),
        32 | 34 | 40 | 42 | 33 | 35 | 41 | 43 if c.fast_mem && !update_invalid(op) => {
            let n = match i >> 26 {
                32 | 33 => 4,
                40..=43 => 2,
                _ => 1,
            };
            let update = (i >> 26) & 1 == 1;
            if update {
                c.load_gpr(0, a);
            } else {
                c.load_ra0(0, a);
            }
            c.add_imm32(0, 0, op.imm, 1);
            mem_load(c, ins, count, n, i >> 26 >= 42, d, update.then_some(a));
        }
        36 | 38 | 44 | 37 | 39 | 45 if c.fast_mem && !update_invalid(op) => {
            let n = match i >> 26 {
                36 | 37 => 4,
                44 | 45 => 2,
                _ => 1,
            };
            let update = (i >> 26) & 1 == 1;
            if update {
                c.load_gpr(0, a);
            } else {
                c.load_ra0(0, a);
            }
            c.add_imm32(0, 0, op.imm, 1);
            mem_store(c, ins, count, n, d, update.then_some(a));
        }
        18 => {
            // b
            let target = if i & 2 != 0 { op.imm } else { ins.ea.wrapping_add(op.imm) };
            if i & 1 != 0 {
                c.a.mov_imm(false, 0, ins.ea.wrapping_add(4) as u64);
                c.str(0, off_lr());
            }
            c.exit_to(count, target);
        }
        16 => {
            let target = if i & 2 != 0 { op.imm } else { ins.ea.wrapping_add(op.imm) };
            branch_cond(c, ins, count, d as u32, a as u32, true, Some(target), i & 1 != 0);
        }
        19 => match (i >> 1) & 0x3FF {
            16 => branch_cond(c, ins, count, d as u32, a as u32, true, None, i & 1 != 0),
            528 => branch_cond(c, ins, count, d as u32, a as u32, false, None, i & 1 != 0),
            _ => return false,
        },
        _ => return false,
    }
    true
}

/// What a floating-point instruction does, when the JIT does it natively.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Fp {
    Add,
    Sub,
    Mul,
    Div,
    /// a * c + b, or a * c - b; then negated.
    Madd { sub: bool, neg: bool },
    /// fmr, fneg, fabs, fnabs: only the sign bit (op 63 only).
    Move(u8),
}

fn fp_kind(i: u32) -> Option<Fp> {
    let xo5 = (i >> 1) & 0x1F;
    let k = match xo5 {
        18 => Fp::Div,
        20 => Fp::Sub,
        21 => Fp::Add,
        25 => Fp::Mul,
        28 => Fp::Madd { sub: true, neg: false },
        29 => Fp::Madd { sub: false, neg: false },
        30 => Fp::Madd { sub: true, neg: true },
        31 => Fp::Madd { sub: false, neg: true },
        _ if i >> 26 == 63 => match (i >> 1) & 0x3FF {
            72 => Fp::Move(0),
            40 => Fp::Move(1),
            264 => Fp::Move(2),
            136 => Fp::Move(3),
            _ => return None,
        },
        _ => return None,
    };
    Some(k)
}

/// x9 = the 11-bit exponent of the double in `r`; branches (patched later) when it is all ones
/// (infinity or NaN). Returns where the branch is.
fn exp_all_ones(c: &mut Ctx, r: R) -> usize {
    c.a.ubfx(true, 9, r, 52, 11);
    c.a.cmp_imm(false, 9, 0x7FF);
    let at = c.a.here();
    c.a.emit(0);
    at
}

/// Floating-point arithmetic natively, exactly as the interpreter's quick path does it: every
/// operand an ordinary number (and a divisor that is not zero), the host's IEEE double
/// arithmetic (fused for the multiply-adds; single precision rounds the multiplier first and
/// the result after), the result not infinite, denormals flushed when FPSCR[NI] is set, then
/// FPRF and CR1. Anything else (an infinity or NaN in or out, a zero divisor, floating point
/// switched off) runs the handler, which does the full PowerPC rules.
fn fp_native(c: &mut Ctx, ins: &Inst, count: u32) {
    let op = ins.op;
    let i = op.inst;
    let kind = fp_kind(i).expect("checked");
    let single = i >> 26 == 59;
    // frC is not among the decoded fields for these opcodes: from the word itself.
    let (d, a, b, cc) = (op.d, op.a, op.b, ((i >> 6) & 31) as u8);
    let rec = i & 1 != 0;
    let mut slow: Vec<(usize, Cond)> = Vec::new();
    // Floating point switched off: the handler raises the exception.
    c.ldr(0, off_msr());
    let at = c.a.here();
    c.a.emit(0);
    let fp_off = at;
    if let Fp::Move(m) = kind {
        c.a.ldr_x_any(1, CPU, ps0(b), 16);
        c.a.mov_imm(true, 11, 1 << 63);
        match m {
            0 => {}
            1 => c.a.eor(true, 1, 1, 11),
            2 => c.a.bic(true, 1, 1, 11),
            _ => c.a.orr(true, 1, 1, 11),
        }
        c.a.str_x_any(1, CPU, ps0(d), 16);
        if rec {
            fp_cr1(c);
        }
    } else {
        let uses_b = !matches!(kind, Fp::Mul);
        let uses_c = matches!(kind, Fp::Mul | Fp::Madd { .. });
        c.a.ldr_x_any(1, CPU, ps0(a), 16);
        slow.push((exp_all_ones(c, 1), Cond::Eq));
        if uses_b {
            c.a.ldr_x_any(2, CPU, ps0(b), 16);
            slow.push((exp_all_ones(c, 2), Cond::Eq));
        }
        if kind == Fp::Div {
            // A zero divisor (either sign).
            c.a.lsl_imm(true, 9, 2, 1);
            let at = c.a.here();
            c.a.emit(0);
            slow.push((at, Cond::Al)); // patched as CBZ below
        }
        if uses_c {
            c.a.ldr_x_any(3, CPU, ps0(cc), 16);
            slow.push((exp_all_ones(c, 3), Cond::Eq));
            if single {
                // The multiplier sees frC rounded to 25 significant bits.
                c.a.mov_imm(true, 11, 0x0800_0000);
                c.a.and(true, 12, 3, 11);
                c.a.mov_imm(true, 11, 0xFFFF_FFFF_F800_0000);
                c.a.and(true, 3, 3, 11);
                c.a.add(true, 3, 3, 12);
                slow.push((exp_all_ones(c, 3), Cond::Eq));
            }
            c.a.fmov_d_x(3, 3);
        }
        c.a.fmov_d_x(1, 1);
        if uses_b {
            c.a.fmov_d_x(2, 2);
        }
        match kind {
            Fp::Add => c.a.fadd_d(0, 1, 2),
            Fp::Sub => c.a.fsub_d(0, 1, 2),
            Fp::Mul => c.a.fmul_d(0, 1, 3),
            Fp::Div => c.a.fdiv_d(0, 1, 2),
            Fp::Madd { sub, neg } => {
                if sub {
                    c.a.fnmsub_d(0, 1, 3, 2);
                } else {
                    c.a.fmadd_d(0, 1, 3, 2);
                }
                if neg {
                    c.a.fneg_d(0, 0);
                }
            }
            Fp::Move(_) => unreachable!(),
        }
        if single {
            c.a.fcvt_s_d(0, 0);
            c.a.fcvt_d_s(0, 0);
        }
        c.a.fmov_x_d(0, 0);
        // An infinite result sets overflow flags: the handler's job.
        slow.push((exp_all_ones(c, 0), Cond::Eq));
        // FPSCR[NI]: denormal results become zero (x9 still holds the exponent).
        c.ldr(4, off_fpscr());
        let ni_at = c.a.here();
        c.a.emit(0);
        let exp_at = c.a.here();
        c.a.emit(0);
        c.a.mov_imm(true, 11, 1 << 63);
        c.a.and(true, 0, 0, 11);
        let after_ni = c.a.here();
        c.a.patch(ni_at, Asm::tbz_to(false, 4, 2, ni_at, after_ni));
        c.a.patch(exp_at, Asm::cbz_to(false, true, 9, exp_at, after_ni));
        c.a.str_x_any(0, CPU, ps0(d), 16);
        if single {
            c.a.str_x_any(0, CPU, ps1(d), 16);
        }
        // FPRF: the result's class. x9 = exponent (0 after a flush too, for a zero).
        c.a.ubfx(true, 9, 0, 52, 11);
        c.a.lsr_imm(true, 10, 0, 63);
        c.a.lsl_imm(true, 11, 0, 12);
        c.a.mov_imm(false, 12, 4);
        c.a.lslv(false, 12, 12, 10); // normal: 0x04 / 0x08
        c.a.mov_imm(false, 13, 2);
        c.a.logic_reg(false, 0b01, false, 13, 13, 10, Shift::Lsl, 4); // zero: 0x02 / 0x12
        c.a.mov_imm(false, 14, 0x14);
        c.a.addsub_reg(false, false, false, 14, 14, 10, Shift::Lsl, 2); // denormal: 0x14 / 0x18
        c.a.cmp_imm(true, 11, 0);
        c.a.csel(false, 13, 13, 14, Cond::Eq);
        c.a.cmp_imm(false, 9, 0);
        c.a.csel(false, 12, 13, 12, Cond::Eq);
        c.a.bfi(false, 4, 12, 12, 5);
        c.str(4, off_fpscr());
        if rec {
            fp_cr1(c);
        }
    }
    let done_at = c.a.here();
    c.a.emit(0);
    let slow_here = c.a.here();
    c.a.patch(fp_off, Asm::tbz_to(false, 0, 13, fp_off, slow_here));
    for (at, cond) in slow {
        let w = if cond == Cond::Al { Asm::cbz_to(true, false, 9, at, slow_here) } else { Asm::b_cond_to(at, slow_here, cond) };
        c.a.patch(at, w);
    }
    c.fallback(ins, count);
    let done = c.a.here();
    let w = c.a.b_to(done_at, done, false);
    c.a.patch(done_at, w);
}

/// A branch to the slow path, patched once the slow path's place is known.
#[derive(Clone, Copy)]
enum Slow {
    Cond(Cond),
    /// TBZ (false) or TBNZ (true) on a bit of a register.
    Bit(bool, R, u32),
    /// CBZ x9.
    Zero9,
}

struct SlowSites(Vec<(usize, Slow)>);

impl SlowSites {
    fn add(&mut self, c: &mut Ctx, kind: Slow) {
        self.0.push((c.a.here(), kind));
        c.a.emit(0);
    }
    /// Ends the native path: jumps over the slow path, which runs the handler.
    fn finish(self, c: &mut Ctx, ins: &Inst, count: u32) {
        let done_at = c.a.here();
        c.a.emit(0);
        let slow = c.a.here();
        for (at, k) in self.0 {
            let w = match k {
                Slow::Cond(cond) => Asm::b_cond_to(at, slow, cond),
                Slow::Bit(nz, r, bit) => Asm::tbz_to(nz, r, bit, at, slow),
                Slow::Zero9 => Asm::cbz_to(true, false, 9, at, slow),
            };
            c.a.patch(at, w);
        }
        c.fallback(ins, count);
        let done = c.a.here();
        let w = c.a.b_to(done_at, done, false);
        c.a.patch(done_at, w);
    }
}

/// The address in w0 is inside the data window and an `n`-byte access there stays inside one
/// 4 KB page: w1 = its offset in main memory. Otherwise the slow path.
fn window(c: &mut Ctx, n: u32, slow: &mut SlowSites) {
    c.a.ubfx(false, 4, 0, 0, 12);
    c.a.cmp_imm(false, 4, 0x1000 - n);
    slow.add(c, Slow::Cond(Cond::Hi));
    c.a.emit(eor_top_bit(1, 0));
    c.a.cmp(false, 1, LIMIT);
    slow.add(c, Slow::Cond(Cond::Hs));
}

/// A store to offset w1 must not hit a page holding compiled or cached code.
fn not_code_page(c: &mut Ctx, slow: &mut SlowSites) {
    c.a.lsr_imm(false, 4, 1, 18);
    c.a.ldr_x_reg_lsl3(5, PAGES, 4);
    c.a.lsr_imm(false, 6, 1, 12);
    c.a.lsrv(true, 5, 5, 6);
    slow.add(c, Slow::Bit(true, 5, 0));
}

/// The single in w`src` as a double in x`dst` (lfs): exact. An infinity or NaN (whose bits
/// PowerPC keeps and the host's conversion would not) takes the slow path.
fn single_to_double(c: &mut Ctx, dst: R, src: R, slow: &mut SlowSites) {
    c.a.ubfx(false, 9, src, 23, 8);
    c.a.cmp_imm(false, 9, 0xFF);
    slow.add(c, Slow::Cond(Cond::Eq));
    c.a.fmov_s_w(0, src);
    c.a.fcvt_d_s(0, 0);
    c.a.fmov_x_d(dst, 0);
}

/// The double in x`src` as a single in w`dst` by PowerPC's store-single rule (stfs): its bits
/// picked out when it is in single range (or zero, infinite or NaN), its sign alone when it is
/// far below; values that must be denormalised take the slow path.
fn double_to_single(c: &mut Ctx, dst: R, src: R, slow: &mut SlowSites) {
    c.a.ubfx(true, 9, src, 52, 11);
    c.a.lsl_imm(true, 10, src, 1);
    // Bits: exponent above 896, or a zero.
    c.a.cmp_imm(false, 9, 896);
    let bits_hi = c.a.here();
    c.a.emit(0);
    let bits_zero = c.a.here();
    c.a.emit(0);
    // 874 to 896: denormalised, the handler's job; below: the sign alone.
    c.a.cmp_imm(false, 9, 874);
    slow.add(c, Slow::Cond(Cond::Hs));
    c.a.lsr_imm(true, dst, src, 32);
    c.a.mov_imm(false, 11, 0x8000_0000);
    c.a.and(false, dst, dst, 11);
    let to_end = c.a.here();
    c.a.emit(0);
    let bits = c.a.here();
    c.a.patch(bits_hi, Asm::b_cond_to(bits_hi, bits, Cond::Hi));
    c.a.patch(bits_zero, Asm::cbz_to(true, false, 10, bits_zero, bits));
    c.a.lsr_imm(true, 12, src, 32);
    c.a.mov_imm(false, 11, 0xC000_0000);
    c.a.and(false, 12, 12, 11);
    c.a.lsr_imm(true, dst, src, 29);
    c.a.mov_imm(false, 11, 0x3FFF_FFFF);
    c.a.and(false, dst, dst, 11);
    c.a.orr(false, dst, dst, 12);
    let end = c.a.here();
    let w = c.a.b_to(to_end, end, false);
    c.a.patch(to_end, w);
}

/// lfs, lfd, stfs, stfd and their update forms; psq_l and psq_st (and update forms) when the
/// GQR says plain floats. Inline inside the data window, the handler otherwise, exactly as
/// the handlers do them.
fn fp_mem(c: &mut Ctx, ins: &Inst, count: u32) {
    let op = ins.op;
    let i = op.inst;
    let opc = i >> 26;
    let (d, a) = (op.d, op.a);
    let psq = matches!(opc, 56 | 57 | 60 | 61);
    let update = matches!(opc, 49 | 51 | 53 | 55 | 57 | 61);
    let store = matches!(opc, 52..=55 | 60 | 61);
    let mut slow = SlowSites(Vec::new());
    // Floating point on (and paired singles, for psq).
    c.ldr(0, off_msr());
    slow.add(c, Slow::Bit(false, 0, 13));
    let w_one = (i >> 15) & 1 != 0;
    if psq {
        c.ldr(0, off_hid2());
        slow.add(c, Slow::Bit(false, 0, 29));
        // Plain floats only: the load type is GQR bits 16-18, the store type bits 0-2.
        c.ldr(2, gqr((i >> 12) & 7));
        c.a.ubfx(false, 2, 2, if store { 0 } else { 16 }, 3);
        c.a.cmp_imm(false, 2, 4);
        slow.add(c, Slow::Cond(Cond::Hs));
    }
    // The address.
    let disp = if psq { (((i & 0xFFF) << 20) as i32 >> 20) as u32 } else { op.imm };
    if update || a != 0 {
        c.load_gpr(0, a);
    } else {
        c.a.mov_imm(false, 0, 0);
    }
    c.add_imm32(0, 0, disp, 1);
    let n = match opc {
        48 | 49 | 52 | 53 => 4,
        50 | 51 | 54 | 55 => 8,
        _ => {
            if w_one {
                4
            } else {
                8
            }
        }
    };
    window(c, n, &mut slow);
    if !store {
        if n == 4 {
            c.a.ldr_w_reg(3, MEM, 1);
            c.a.rev_w(3, 3);
        } else {
            c.a.ldr_x_reg(3, MEM, 1);
            c.a.rev_x(3, 3);
        }
        match opc {
            48 | 49 => {
                single_to_double(c, 7, 3, &mut slow);
                c.a.str_x_any(7, CPU, ps0(d), 16);
                c.a.str_x_any(7, CPU, ps1(d), 16);
            }
            50 | 51 => c.a.str_x_any(3, CPU, ps0(d), 16),
            _ => {
                if w_one {
                    single_to_double(c, 7, 3, &mut slow);
                    c.a.mov_imm(true, 8, 1.0f64.to_bits());
                } else {
                    // Two singles: the first is the high word.
                    c.a.lsr_imm(true, 6, 3, 32);
                    single_to_double(c, 7, 6, &mut slow);
                    c.a.mov(false, 6, 3);
                    single_to_double(c, 8, 6, &mut slow);
                }
                c.a.str_x_any(7, CPU, ps0(d), 16);
                c.a.str_x_any(8, CPU, ps1(d), 16);
            }
        }
    } else {
        not_code_page(c, &mut slow);
        match opc {
            52 | 53 => {
                c.a.ldr_x_any(2, CPU, ps0(d), 16);
                double_to_single(c, 3, 2, &mut slow);
                c.a.rev_w(3, 3);
                c.a.str_w_reg(3, MEM, 1);
            }
            54 | 55 => {
                c.a.ldr_x_any(3, CPU, ps0(d), 16);
                c.a.rev_x(3, 3);
                c.a.str_x_reg(3, MEM, 1);
            }
            _ => {
                c.a.ldr_x_any(2, CPU, ps0(d), 16);
                double_to_single(c, 3, 2, &mut slow);
                if w_one {
                    c.a.rev_w(3, 3);
                    c.a.str_w_reg(3, MEM, 1);
                } else {
                    c.a.ldr_x_any(2, CPU, ps1(d), 16);
                    double_to_single(c, 7, 2, &mut slow);
                    // First value in the high word.
                    c.a.lsl_imm(true, 3, 3, 32);
                    c.a.orr(true, 3, 3, 7);
                    c.a.rev_x(3, 3);
                    c.a.str_x_reg(3, MEM, 1);
                }
            }
        }
    }
    if update {
        c.store_gpr(0, a);
    }
    slow.finish(c, ins, count);
}

/// What one lane of a paired-single instruction computes, and from which halves (0 = ps0,
/// 1 = ps1) of frA, frB and frC.
#[derive(Clone, Copy)]
enum Lane {
    Arith(Fp, u8, u8, u8),
    /// round_single of frC's half (ps_sum's other lane) or frA's (ps_merge00/01).
    RoundC(u8),
    RoundA(u8),
    /// A half of frA or frB as it is (ps_merge).
    A(u8),
    B(u8),
    /// frB's half with its sign changed: 0 as is, 1 negated, 2 cleared, 3 set.
    Sign(u8, u8),
}

/// Paired-single instructions the JIT does natively: their two lanes, and whether FPRF is set.
fn ps_kind(i: u32) -> Option<(Lane, Lane, bool)> {
    use Fp::*;
    let madd = |sub, neg| Madd { sub, neg };
    let k = match (i >> 1) & 0x1F {
        10 => (Lane::Arith(Add, 0, 1, 0), Lane::RoundC(1), true),
        11 => (Lane::RoundC(0), Lane::Arith(Add, 0, 1, 0), true),
        12 => (Lane::Arith(Mul, 0, 0, 0), Lane::Arith(Mul, 1, 0, 0), true),
        13 => (Lane::Arith(Mul, 0, 0, 1), Lane::Arith(Mul, 1, 0, 1), true),
        14 => (Lane::Arith(madd(false, false), 0, 0, 0), Lane::Arith(madd(false, false), 1, 1, 0), true),
        15 => (Lane::Arith(madd(false, false), 0, 0, 1), Lane::Arith(madd(false, false), 1, 1, 1), true),
        18 => (Lane::Arith(Div, 0, 0, 0), Lane::Arith(Div, 1, 1, 0), true),
        20 => (Lane::Arith(Sub, 0, 0, 0), Lane::Arith(Sub, 1, 1, 0), true),
        21 => (Lane::Arith(Add, 0, 0, 0), Lane::Arith(Add, 1, 1, 0), true),
        25 => (Lane::Arith(Mul, 0, 0, 0), Lane::Arith(Mul, 1, 0, 1), true),
        28 => (Lane::Arith(madd(true, false), 0, 0, 0), Lane::Arith(madd(true, false), 1, 1, 1), true),
        29 => (Lane::Arith(madd(false, false), 0, 0, 0), Lane::Arith(madd(false, false), 1, 1, 1), true),
        30 => (Lane::Arith(madd(true, true), 0, 0, 0), Lane::Arith(madd(true, true), 1, 1, 1), true),
        31 => (Lane::Arith(madd(false, true), 0, 0, 0), Lane::Arith(madd(false, true), 1, 1, 1), true),
        _ => match (i >> 1) & 0x3FF {
            72 => (Lane::Sign(0, 0), Lane::Sign(1, 0), false),
            40 => (Lane::Sign(0, 1), Lane::Sign(1, 1), false),
            264 => (Lane::Sign(0, 2), Lane::Sign(1, 2), false),
            136 => (Lane::Sign(0, 3), Lane::Sign(1, 3), false),
            528 => (Lane::RoundA(0), Lane::B(0), false),
            560 => (Lane::RoundA(0), Lane::B(1), false),
            592 => (Lane::A(1), Lane::B(0), false),
            624 => (Lane::A(1), Lane::B(1), false),
            _ => return None,
        },
    };
    Some(k)
}

/// One lane of single-precision arithmetic on halves of frA/frB/frC into x0, the
/// interpreter's quick path exactly (as `fp_native` for op 59); w4 must hold FPSCR.
#[allow(clippy::too_many_arguments)]
fn fp_lane(c: &mut Ctx, kind: Fp, a: u32, b: u32, cc: u32, slow: &mut SlowSites) {
    let uses_b = !matches!(kind, Fp::Mul);
    let uses_c = matches!(kind, Fp::Mul | Fp::Madd { .. });
    c.a.ldr_x_any(1, CPU, a, 16);
    fin_check(c, 1, slow);
    if uses_b {
        c.a.ldr_x_any(2, CPU, b, 16);
        fin_check(c, 2, slow);
    }
    if kind == Fp::Div {
        c.a.lsl_imm(true, 9, 2, 1);
        slow.add(c, Slow::Zero9);
    }
    if uses_c {
        c.a.ldr_x_any(3, CPU, cc, 16);
        fin_check(c, 3, slow);
        c.a.mov_imm(true, 11, 0x0800_0000);
        c.a.and(true, 12, 3, 11);
        c.a.mov_imm(true, 11, 0xFFFF_FFFF_F800_0000);
        c.a.and(true, 3, 3, 11);
        c.a.add(true, 3, 3, 12);
        fin_check(c, 3, slow);
        c.a.fmov_d_x(3, 3);
    }
    c.a.fmov_d_x(1, 1);
    if uses_b {
        c.a.fmov_d_x(2, 2);
    }
    match kind {
        Fp::Add => c.a.fadd_d(0, 1, 2),
        Fp::Sub => c.a.fsub_d(0, 1, 2),
        Fp::Mul => c.a.fmul_d(0, 1, 3),
        Fp::Div => c.a.fdiv_d(0, 1, 2),
        Fp::Madd { sub, neg } => {
            if sub {
                c.a.fnmsub_d(0, 1, 3, 2);
            } else {
                c.a.fmadd_d(0, 1, 3, 2);
            }
            if neg {
                c.a.fneg_d(0, 0);
            }
        }
        Fp::Move(_) => unreachable!(),
    }
    c.a.fcvt_s_d(0, 0);
    c.a.fcvt_d_s(0, 0);
    c.a.fmov_x_d(0, 0);
    fin_check(c, 0, slow);
    // NI: a denormal result becomes a signed zero (x9 holds the exponent).
    let ni_at = c.a.here();
    c.a.emit(0);
    let exp_at = c.a.here();
    c.a.emit(0);
    c.a.mov_imm(true, 11, 1 << 63);
    c.a.and(true, 0, 0, 11);
    let after = c.a.here();
    c.a.patch(ni_at, Asm::tbz_to(false, 4, 2, ni_at, after));
    c.a.patch(exp_at, Asm::cbz_to(false, true, 9, exp_at, after));
}

/// The slow path when the double in `r` is infinite or NaN (x9 = its exponent after).
fn fin_check(c: &mut Ctx, r: R, slow: &mut SlowSites) {
    c.a.ubfx(true, 9, r, 52, 11);
    c.a.cmp_imm(false, 9, 0x7FF);
    slow.add(c, Slow::Cond(Cond::Eq));
}

/// round_single of the double at `off` into x0: the host's rounding, the slow path for a NaN.
fn round_single(c: &mut Ctx, off: u32, slow: &mut SlowSites) {
    c.a.ldr_x_any(0, CPU, off, 16);
    // NaN: exponent all ones and a fraction.
    c.a.lsl_imm(true, 9, 0, 1);
    c.a.mov_imm(true, 11, 0xFFE0_0000_0000_0000);
    c.a.cmp(true, 9, 11);
    slow.add(c, Slow::Cond(Cond::Hi));
    c.a.fmov_d_x(0, 0);
    c.a.fcvt_s_d(0, 0);
    c.a.fcvt_d_s(0, 0);
    c.a.fmov_x_d(0, 0);
}

/// Paired-single arithmetic, ps_sum, ps_mr/neg/abs/nabs and ps_merge natively: each lane as
/// the interpreter computes it (the arithmetic through the single-precision quick path), both
/// written only when both took it, FPRF from the first lane, CR1.
fn ps_native(c: &mut Ctx, ins: &Inst, count: u32) {
    let op = ins.op;
    let i = op.inst;
    let (l0, l1, fprf) = ps_kind(i).expect("checked");
    let (d, a, b, cc) = (op.d, op.a, op.b, ((i >> 6) & 31) as u8);
    let rec = i & 1 != 0;
    let mut slow = SlowSites(Vec::new());
    c.ldr(0, off_msr());
    slow.add(c, Slow::Bit(false, 0, 13));
    c.ldr(0, off_hid2());
    slow.add(c, Slow::Bit(false, 0, 29));
    c.ldr(4, off_fpscr());
    let half = |r: u8, h: u8| if h == 0 { ps0(r) } else { ps1(r) };
    for (k, lane) in [l0, l1].into_iter().enumerate() {
        match lane {
            Lane::Arith(kind, ha, hb, hc) => fp_lane(c, kind, half(a, ha), half(b, hb), half(cc, hc), &mut slow),
            Lane::RoundC(h) => {
                // ps_sum's rounded lane can reach infinity, whose FPRF class the native
                // classifier does not do: the handler's job.
                round_single(c, half(cc, h), &mut slow);
                fin_check(c, 0, &mut slow);
            }
            Lane::RoundA(h) => round_single(c, half(a, h), &mut slow),
            Lane::A(h) => c.a.ldr_x_any(0, CPU, half(a, h), 16),
            Lane::B(h) => c.a.ldr_x_any(0, CPU, half(b, h), 16),
            Lane::Sign(h, m) => {
                c.a.ldr_x_any(0, CPU, half(b, h), 16);
                c.a.mov_imm(true, 11, 1 << 63);
                match m {
                    0 => {}
                    1 => c.a.eor(true, 0, 0, 11),
                    2 => c.a.bic(true, 0, 0, 11),
                    _ => c.a.orr(true, 0, 0, 11),
                }
            }
        }
        c.a.mov(true, if k == 0 { 7 } else { 8 }, 0);
    }
    c.a.str_x_any(7, CPU, ps0(d), 16);
    c.a.str_x_any(8, CPU, ps1(d), 16);
    if fprf {
        fprf_of(c, 7);
        c.str(4, off_fpscr());
    }
    if rec {
        fp_cr1(c);
    }
    slow.finish(c, ins, count);
}

/// FPSCR[FPRF] in w4 = the class of the finite double in x`r`.
fn fprf_of(c: &mut Ctx, r: R) {
    c.a.ubfx(true, 9, r, 52, 11);
    c.a.lsr_imm(true, 10, r, 63);
    c.a.lsl_imm(true, 11, r, 12);
    c.a.mov_imm(false, 12, 4);
    c.a.lslv(false, 12, 12, 10);
    c.a.mov_imm(false, 13, 2);
    c.a.logic_reg(false, 0b01, false, 13, 13, 10, Shift::Lsl, 4);
    c.a.mov_imm(false, 14, 0x14);
    c.a.addsub_reg(false, false, false, 14, 14, 10, Shift::Lsl, 2);
    c.a.cmp_imm(true, 11, 0);
    c.a.csel(false, 13, 13, 14, Cond::Eq);
    c.a.cmp_imm(false, 9, 0);
    c.a.csel(false, 12, 13, 12, Cond::Eq);
    c.a.bfi(false, 4, 12, 12, 5);
}

/// CR1 = FPSCR[FX, FEX, VX, OX].
fn fp_cr1(c: &mut Ctx) {
    c.ldr(4, off_fpscr());
    c.a.lsr_imm(false, 1, 4, 28);
    c.set_cr_field(1);
}

fn update_invalid(op: &Op) -> bool {
    let i = op.inst;
    let update = matches!(i >> 26, 33 | 35 | 41 | 43 | 37 | 39 | 45);
    let load = matches!(i >> 26, 33 | 35 | 41 | 43);
    update && (op.a == 0 || (load && op.a == op.d))
}

/// A conditional branch: bc (target known), bclr (LR) or bcctr (CTR).
#[allow(clippy::too_many_arguments)]
fn branch_cond(c: &mut Ctx, ins: &Inst, count: u32, bo: u32, bi: u32, use_ctr: bool, target: Option<u32>, link: bool) {
    let next = ins.ea.wrapping_add(4);
    // The target register is read before CTR is decremented or LR is written.
    if target.is_none() {
        c.ldr(6, if use_ctr { off_lr() } else { off_ctr() });
        c.a.lsr_imm(false, 6, 6, 2);
        c.a.lsl_imm(false, 6, 6, 2);
    }
    let mut skips: Vec<(usize, bool, u32)> = Vec::new(); // (at, is_tbz, bit)
    if bo & 4 == 0 && use_ctr {
        c.ldr(1, off_ctr());
        c.a.sub_imm(false, 1, 1, 1);
        c.str(1, off_ctr());
        // Branch only if (ctr != 0) != (bo & 2): skip when that fails.
        let at = c.a.here();
        c.a.emit(0);
        skips.push((at, false, if bo & 2 != 0 { 1 } else { 0 }));
    }
    if bo & 0x10 == 0 {
        c.ldr(2, off_cr());
        let at = c.a.here();
        c.a.emit(0);
        skips.push((at, true, bo & 8));
    }
    // Taken.
    if link {
        c.a.mov_imm(false, 0, next as u64);
        c.str(0, off_lr());
    }
    match target {
        Some(t) => c.exit_to(count, t),
        None => {
            c.str(6, off_pc());
            c.exit_indirect(count);
        }
    }
    // Not taken.
    let not_taken = c.a.here();
    if link {
        c.a.mov_imm(false, 0, next as u64);
        c.str(0, off_lr());
    }
    c.exit_to(count, next);
    for (at, is_cr, v) in skips {
        let w = if is_cr {
            // Condition bit set must equal (bo & 8 != 0): skip with TBZ when we needed it set.
            Asm::tbz_to(v == 0, 2, 31 - bi, at, not_taken)
        } else {
            // v = 1: branch when ctr == 0, so skip when ctr != 0 (CBNZ); else skip on zero.
            Asm::cbz_to(false, v == 1, 1, at, not_taken)
        };
        c.a.patch(at, w);
    }
}

/// Inline load of `n` bytes at the address in w0 (the standard block mapping: 0x80000000 is
/// physical 0, translation on, for `mem_limit` + 7 bytes), or the handler when that does not
/// apply.
#[allow(clippy::too_many_arguments)]
fn mem_load(c: &mut Ctx, ins: &Inst, count: u32, n: u32, algebraic: bool, d: u8, update: Option<u8>) {
    let page_at = page_check(c, n);
    c.a.emit(eor_top_bit(1, 0));
    c.a.cmp(false, 1, LIMIT);
    let slow_at = c.a.here();
    c.a.emit(0);
    match n {
        4 => {
            c.a.ldr_w_reg(3, MEM, 1);
            c.a.rev_w(3, 3);
        }
        2 => {
            c.a.ldrh_reg(3, MEM, 1);
            c.a.rev16_w(3, 3);
            if algebraic {
                c.a.sxth(false, 3, 3);
            }
        }
        _ => c.a.ldrb_reg(3, MEM, 1),
    }
    c.store_gpr(3, d);
    if let Some(a) = update {
        c.store_gpr(0, a);
    }
    let done_at = c.a.here();
    c.a.emit(0);
    let slow = c.a.here();
    c.a.patch(slow_at, Asm::b_cond_to(slow_at, slow, Cond::Hs));
    c.a.patch(page_at, Asm::b_cond_to(page_at, slow, Cond::Hi));
    c.fallback(ins, count);
    let done = c.a.here();
    let w = c.a.b_to(done_at, done, false);
    c.a.patch(done_at, w);
}

/// Accesses that straddle two 4 KB pages take the handler (as the interpreter does): emits the
/// test and returns where its branch goes.
fn page_check(c: &mut Ctx, n: u32) -> usize {
    c.a.ubfx(false, 4, 0, 0, 12);
    c.a.cmp_imm(false, 4, 0x1000 - n);
    let at = c.a.here();
    c.a.emit(0);
    at
}

/// Inline store of `n` bytes of rS to the address in w0, skipping pages that hold cached code.
fn mem_store(c: &mut Ctx, ins: &Inst, count: u32, n: u32, s: u8, update: Option<u8>) {
    let page_at = page_check(c, n);
    c.a.emit(eor_top_bit(1, 0));
    c.a.cmp(false, 1, LIMIT);
    let slow_at = c.a.here();
    c.a.emit(0);
    // The code-page bit for physical page w1 >> 12.
    c.a.lsr_imm(false, 4, 1, 18);
    c.a.ldr_x_reg_lsl3(5, PAGES, 4);
    c.a.lsr_imm(false, 6, 1, 12);
    c.a.lsrv(true, 5, 5, 6);
    let code_at = c.a.here();
    c.a.emit(0);
    c.load_gpr(3, s);
    match n {
        4 => {
            c.a.rev_w(3, 3);
            c.a.str_w_reg(3, MEM, 1);
        }
        2 => {
            c.a.rev16_w(3, 3);
            c.a.strh_reg(3, MEM, 1);
        }
        _ => c.a.strb_reg(3, MEM, 1),
    }
    if let Some(a) = update {
        c.store_gpr(0, a);
    }
    let done_at = c.a.here();
    c.a.emit(0);
    let slow = c.a.here();
    c.a.patch(slow_at, Asm::b_cond_to(slow_at, slow, Cond::Hs));
    c.a.patch(code_at, Asm::tbz_to(true, 5, 0, code_at, slow));
    c.a.patch(page_at, Asm::b_cond_to(page_at, slow, Cond::Hi));
    c.fallback(ins, count);
    let done = c.a.here();
    let w = c.a.b_to(done_at, done, false);
    c.a.patch(done_at, w);
}

fn native31(c: &mut Ctx, ins: &Inst, _count: u32) -> bool {
    let op = ins.op;
    let i = op.inst;
    let (d, a, b) = (op.d, op.a, op.b);
    let rec = i & 1 != 0;
    let xo = (i >> 1) & 0x3FF;
    match xo {
        266 | 40 if i & 0x400 == 0 => {
            // add, subf (no OE)
            c.load_gpr(0, a);
            c.load_gpr(1, b);
            if xo == 266 {
                c.a.add(false, 0, 0, 1);
            } else {
                c.a.sub(false, 0, 1, 0);
            }
            c.store_gpr(0, d);
            if rec {
                c.cr0(0);
            }
        }
        28 | 60 | 444 | 412 | 316 | 124 | 476 | 284 => {
            c.load_gpr(0, d);
            c.load_gpr(1, b);
            match xo {
                28 => c.a.and(false, 0, 0, 1),
                60 => c.a.bic(false, 0, 0, 1),
                444 => c.a.orr(false, 0, 0, 1),
                412 => c.a.orn(false, 0, 0, 1),
                316 => c.a.eor(false, 0, 0, 1),
                124 => {
                    c.a.orr(false, 0, 0, 1);
                    c.a.mvn(false, 0, 0);
                }
                476 => {
                    c.a.and(false, 0, 0, 1);
                    c.a.mvn(false, 0, 0);
                }
                _ => c.a.eon(false, 0, 0, 1),
            }
            c.store_gpr(0, a);
            if rec {
                c.cr0(0);
            }
        }
        0 | 32 => {
            // cmp, cmpl (d holds the field)
            c.load_gpr(0, a);
            c.load_gpr(1, b);
            c.a.cmp(false, 0, 1);
            c.cr_nibble(xo == 0);
            c.set_cr_field(d as u32);
        }
        26 => {
            c.load_gpr(0, d);
            c.a.clz(false, 0, 0);
            c.store_gpr(0, a);
            if rec {
                c.cr0(0);
            }
        }
        922 | 954 => {
            c.load_gpr(0, d);
            if xo == 922 {
                c.a.sxth(false, 0, 0);
            } else {
                c.a.sxtb(false, 0, 0);
            }
            c.store_gpr(0, a);
            if rec {
                c.cr0(0);
            }
        }
        339 if op.imm == 0 && matches!(spr(i), 8 | 9) => {
            c.ldr(0, if spr(i) == 8 { off_lr() } else { off_ctr() });
            c.store_gpr(0, d);
        }
        467 if matches!(spr(i), 8 | 9) => {
            c.load_gpr(0, d);
            c.str(0, if spr(i) == 8 { off_lr() } else { off_ctr() });
        }
        _ => return false,
    }
    true
}

fn spr(i: u32) -> u32 {
    ((i >> 16) & 0x1F) | (((i >> 11) & 0x1F) << 5)
}
