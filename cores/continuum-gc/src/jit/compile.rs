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
use crate::hw::MEM1_SIZE;
use std::mem::offset_of;

/// What generated code needs that is not in the CPU or the hardware.
#[repr(C)]
pub struct JitCtx {
    pub mem1: u64,
    pub code_pages: u64,
    /// `fallback(cpu, hw, op)`: runs one instruction's fast-interpreter handler.
    pub fallback: u64,
}

/// The word a link site holds while it is not linked: a branch to the next instruction, which
/// is the stub that stores the target PC and returns.
pub const UNLINKED: u32 = 0x1400_0001;

const CPU: R = 19;
const HW: R = 20;
const CTX: R = 21;
const MEM: R = 22;
const PAGES: R = 23;
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
    /// Branches to the epilogue itself: (at).
    to_epilogue: Vec<usize>,
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
        self.to_epilogue.push(self.a.here());
        self.a.emit(0);
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
    for at in std::mem::take(&mut c.to_epilogue) {
        let w = c.a.b_to(at, epilogue, false);
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
            c.exit(count);
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
/// physical 0, translation on), or the handler when that does not apply.
#[allow(clippy::too_many_arguments)]
fn mem_load(c: &mut Ctx, ins: &Inst, count: u32, n: u32, algebraic: bool, d: u8, update: Option<u8>) {
    let page_at = page_check(c, n);
    c.a.emit(eor_top_bit(1, 0));
    c.a.mov_imm(false, 2, (MEM1_SIZE as u32 - n + 1) as u64);
    c.a.cmp(false, 1, 2);
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
    c.a.mov_imm(false, 2, (MEM1_SIZE as u32 - n + 1) as u64);
    c.a.cmp(false, 1, 2);
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
