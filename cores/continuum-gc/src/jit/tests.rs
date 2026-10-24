//! The JIT against the interpreter: random blocks of PowerPC instructions, compiled to ARM64
//! and run through the same backend the runtime uses (the real processor on ARM64; on the x86
//! test machine Unicorn, an ARM64 emulator), must leave the CPU and memory exactly as the
//! fast interpreter's handlers do (which are proved equal to the plain interpreter by their
//! own tests).

#![cfg(any(target_arch = "aarch64", feature = "unicorn"))]

use super::compile::{compile, Inst, JitCtx};
use super::runtime::Backend;
use crate::cpu::fast_ops::{decode, Op};
use crate::cpu::{Cpu, HID2_LSQE, HID2_PSE, MSR_DR, MSR_FP, MSR_IR};
use crate::hw::Hw;

const WINDOW: usize = 0x10_0000;
const WINDOW_LEN: usize = 0x1_0000;
/// Where block B goes in the code area (instructions).
const B_AT: usize = 0x2000;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 16) as u32
    }
}

fn instruction(rng: &mut Rng) -> u32 {
    let ops = [14u32, 15, 24, 25, 26, 27, 28, 29, 10, 11, 20, 21, 32, 33, 34, 35, 40, 41, 42, 43, 36, 37, 38, 39, 44, 45, 31, 31, 31, 31, 31, 31, 7, 8, 12, 13, 23];
    let op = ops[(rng.next() as usize) % ops.len()];
    let f = rng.next() & 0x03FF_FFFF;
    if op == 31 {
        let xos = [266u32, 40, 28, 60, 444, 412, 316, 124, 476, 284, 0, 32, 26, 922, 954, 339, 467, 10, 104, 235, 23, 151, 824];
        let xo = xos[(rng.next() as usize) % xos.len()];
        let mut i = (31 << 26) | (f & 0x03FF_F801) | (xo << 1);
        if xo == 339 || xo == 467 {
            let n = [8u32, 9][(rng.next() % 2) as usize];
            i = (i & !0x001F_F800) | ((n & 31) << 16) | ((n >> 5) << 11);
        }
        return i;
    }
    (op << 26) | f
}

fn branch(rng: &mut Rng) -> u32 {
    match rng.next() % 4 {
        0 => (18 << 26) | ((rng.next() & 0x3FFC) | (rng.next() & 1)),
        1 => (16 << 26) | (rng.next() & 0x03FF_FFFD),
        2 => (19 << 26) | (rng.next() & 0x03FF_F800) | (16 << 1) | (rng.next() & 1),
        _ => (19 << 26) | ((rng.next() & 0x03FF_F800) | if rng.next() % 2 == 0 { 0x0280_0000 } else { 0 }) | (528 << 1) | (rng.next() & 1),
    }
}

fn setup(cpu: &mut Cpu, rng: &mut Rng) {
    cpu.ibat[0] = [0x8000_1FFF, 0x0000_0002];
    cpu.dbat[0] = [0x8000_1FFF, 0x0000_0002];
    cpu.msr = MSR_IR | MSR_DR | MSR_FP;
    cpu.hid2 = HID2_PSE | HID2_LSQE;
    for r in 0..32 {
        cpu.gpr[r] = match rng.next() % 6 {
            0 => 0x8010_0000 + (rng.next() & 0xFFFC),
            1 => rng.next() & 0xFF,
            2 => [0u32, 1, 0xFFFF_FFFF, 0x8000_0000, 0x7FFF_FFFF, 0x8000_FFFF][(rng.next() % 6) as usize],
            3 => 0x8010_0000 + (rng.next() & 0xFFFF),
            _ => rng.next(),
        };
    }
    cpu.cr = rng.next();
    cpu.xer = rng.next() & 0xE000_007F;
    cpu.lr = 0x8000_4000 + (rng.next() & 0xFFF);
    cpu.ctr = rng.next() % 3;
}

#[test]
fn jit_blocks_match_the_interpreter() {
    let mut backend: Box<dyn Backend> = super::backend(1 << 20).expect("code memory");
    let mut rng = Rng(0x0BAD_C0DE_1234_5678);
    let mut hw_ref = Hw::new(false);
    let mut hw_jit = Hw::new(false);
    for k in (0..WINDOW_LEN).step_by(4) {
        let w = rng.next();
        hw_ref.mem1[WINDOW + k..WINDOW + k + 4].copy_from_slice(&w.to_be_bytes());
    }
    hw_jit.mem1.copy_from_slice(&hw_ref.mem1);
    // One page of "cached code" in the window: stores there must take the slow path.
    hw_jit.code_pages[(WINDOW + 0x8000) >> 18] |= 1 << (((WINDOW + 0x8000) >> 12) & 63);
    hw_ref.code_pages.copy_from_slice(&hw_jit.code_pages.clone());
    let ctx = JitCtx { mem1: hw_jit.mem1.as_mut_ptr() as u64, code_pages: hw_jit.code_pages.as_ptr() as u64, fallback: backend.fallback(), mem_limit: (crate::hw::MEM1_SIZE - 7) as u64 };

    let blocks = 20_000;
    let mut native_hits = 0usize;
    let mut chained = 0usize;
    for case in 0..blocks {
        // Block A, and sometimes a block B straight after it that A's fall-through exit is
        // linked to, as the runtime links blocks.
        let with_b = rng.next() % 3 == 0;
        let len = 1 + (rng.next() % 10) as usize;
        let mut words: Vec<u32> = (0..len).map(|_| instruction(&mut rng)).collect();
        if !with_b && rng.next() % 2 == 0 {
            words.push(branch(&mut rng));
        }
        let base = 0x8000_3100u32;
        let b_base = base + 4 * words.len() as u32;
        let mut b_words: Vec<u32> = Vec::new();
        if with_b {
            let n = 1 + (rng.next() % 6) as usize;
            b_words = (0..n).map(|_| instruction(&mut rng)).collect();
            if rng.next() % 2 == 0 {
                b_words.push(branch(&mut rng));
            }
        }
        let ops: Vec<Op> = words.iter().map(|&w| decode(w)).collect();
        let b_ops: Vec<Op> = b_words.iter().map(|&w| decode(w)).collect();
        let insts: Vec<Inst> = ops.iter().enumerate().map(|(k, op)| Inst { ea: base + 4 * k as u32, op }).collect();
        let b_insts: Vec<Inst> = b_ops.iter().enumerate().map(|(k, op)| Inst { ea: b_base + 4 * k as u32, op }).collect();
        let compiled = compile(&insts, true);
        native_hits += compiled.native;
        let b_compiled = with_b.then(|| compile(&b_insts, true));
        // Usually enough budget for everything; sometimes not, to check the budget tests.
        let total = ops.len() + b_ops.len();
        let budget = if rng.next() % 4 == 0 { (rng.next() as usize % (total + 2)) as u64 } else { 1000 };

        let mut cpu_ref = Cpu::default();
        setup(&mut cpu_ref, &mut rng);
        cpu_ref.pc = base;
        let mut cpu_jit = cpu_ref.clone();
        let start = cpu_ref.clone();

        // As the machine does before each block.
        hw_ref.irq_dirty = false;
        hw_jit.irq_dirty = false;
        hw_ref.invalidated.clear();
        hw_jit.invalidated.clear();
        // Reference: the handlers, as the fast interpreter runs blocks, block by block, each
        // only when the budget left covers all of it (the runtime finishes the rest).
        let mut left = budget;
        let mut n_ref = 0u64;
        for (blk, blk_base) in [(&ops, base), (&b_ops, b_base)] {
            if blk.is_empty() || cpu_ref.pc != blk_base || (left as usize) < blk.len() {
                break;
            }
            cpu_ref.exit = false;
            cpu_ref.faulted = false;
            let mut ea = blk_base;
            let mut n = 0u64;
            for op in blk.iter() {
                cpu_ref.cur_pc = ea;
                (op.f)(&mut cpu_ref, &mut hw_ref, op);
                n += 1;
                if cpu_ref.exit {
                    break;
                }
                ea = ea.wrapping_add(4);
            }
            if !cpu_ref.exit {
                cpu_ref.pc = ea;
            }
            n_ref += n;
            left -= n;
            if cpu_ref.exit {
                break;
            }
        }

        // The JIT: A at the start of the code area, B further on, A's link to B patched in.
        let mut code_a = compiled.code.clone();
        if let Some(bc) = &b_compiled {
            let b_inner = B_AT + bc.inner;
            for &(site, target) in &compiled.links {
                if target == b_base {
                    code_a[site] = 0x1400_0000 | ((b_inner as u32).wrapping_sub(site as u32) & 0x03FF_FFFF);
                    chained += 1;
                }
            }
            backend.write(B_AT, &bc.code);
        }
        backend.write(0, &code_a);
        cpu_jit.exit = false;
        cpu_jit.faulted = false;
        // SAFETY: freshly compiled code; the CPU, hardware and context are live locals.
        let n_jit = budget - unsafe { backend.call(0, &mut cpu_jit, &mut hw_jit, &ctx, budget) };
        if with_b {
            words.extend_from_slice(&b_words);
        }

        let same = cpu_ref.gpr == cpu_jit.gpr && cpu_ref.cr == cpu_jit.cr && cpu_ref.xer == cpu_jit.xer && cpu_ref.lr == cpu_jit.lr && cpu_ref.ctr == cpu_jit.ctr && cpu_ref.pc == cpu_jit.pc && cpu_ref.msr == cpu_jit.msr && cpu_ref.srr0 == cpu_jit.srr0 && cpu_ref.dar == cpu_jit.dar;
        let whole = case % 500 == 499 || case == blocks - 1;
        let mem_same = if whole { hw_ref.mem1 == hw_jit.mem1 } else { hw_ref.mem1[WINDOW..WINDOW + WINDOW_LEN] == hw_jit.mem1[WINDOW..WINDOW + WINDOW_LEN] };
        if !same || n_ref != n_jit || !mem_same {
            let dis: Vec<String> = words.iter().map(|w| format!("{w:08X}")).collect();
            let mut diff = String::new();
            for r in 0..32 {
                if cpu_ref.gpr[r] != cpu_jit.gpr[r] {
                    diff.push_str(&format!(" r{r} {:08X}/{:08X}", cpu_ref.gpr[r], cpu_jit.gpr[r]));
                }
            }
            let regs: Vec<String> = start.gpr.iter().map(|g| format!("{g:08X}")).collect();
            eprintln!("start registers {regs:?} cr {:08X} xer {:08X}", start.cr, start.xer);
            eprintln!("ref dar {:08X} dsisr {:08X} srr0 {:08X} msr {:08X}; jit dar {:08X} msr {:08X}", cpu_ref.dar, cpu_ref.dsisr, cpu_ref.srr0, cpu_ref.msr, cpu_jit.dar, cpu_jit.msr);
            panic!(
                "case {case}: block {dis:?}\n count {n_ref}/{n_jit} pc {:08X}/{:08X} cr {:08X}/{:08X} xer {:08X}/{:08X} lr {:08X}/{:08X} ctr {:08X}/{:08X} memory same {}\n{diff}",
                cpu_ref.pc,
                cpu_jit.pc,
                cpu_ref.cr,
                cpu_jit.cr,
                cpu_ref.xer,
                cpu_jit.xer,
                cpu_ref.lr,
                cpu_jit.lr,
                cpu_ref.ctr,
                cpu_jit.ctr,
                mem_same
            );
        }
    }
    assert!(native_hits > 0 && chained > 0);
}
