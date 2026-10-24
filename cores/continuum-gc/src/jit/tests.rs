//! The JIT against the interpreter: random blocks of PowerPC instructions, compiled to ARM64
//! and run inside Unicorn (an ARM64 emulator, test machine only), must leave the CPU and
//! memory exactly as the fast interpreter's handlers do (which are proved equal to the plain
//! interpreter by their own tests).

use super::compile::{compile, Inst, JitCtx};
use crate::cpu::fast_ops::{decode, Op};
use crate::cpu::{Cpu, HID2_LSQE, HID2_PSE, MSR_DR, MSR_FP, MSR_IR};
use crate::hw::Hw;
use std::mem::offset_of;
use unicorn_engine::unicorn_const::{Arch, Mode, Prot};
use unicorn_engine::{RegisterARM64, Unicorn};

const CODE: u64 = 0x1000_0000;
const STACK: u64 = 0x2000_0000;
const CPUMEM: u64 = 0x3000_0000;
const CTXMEM: u64 = 0x3100_0000;
const PAGES: u64 = 0x3200_0000;
const MEM1: u64 = 0x4000_0000;
const FALLBACK: u64 = 0x5000_0000;
const WINDOW: usize = 0x10_0000;
const WINDOW_LEN: usize = 0x1_0000;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 16) as u32
    }
}

fn cpu_bytes(cpu: &Cpu) -> &[u8] {
    // SAFETY: plain bytes of a live value, for copying into the emulator and back.
    unsafe { std::slice::from_raw_parts(cpu as *const Cpu as *const u8, std::mem::size_of::<Cpu>()) }
}

/// Copies the emulator's view of the CPU back, field by field (only the fields generated code
/// and handlers touch; heap pointers inside `Cpu` are never taken from the emulator).
fn read_cpu(uc: &Unicorn<()>, cpu: &mut Cpu) {
    let mut b = vec![0u8; std::mem::size_of::<Cpu>()];
    uc.mem_read(CPUMEM, &mut b).unwrap();
    let w = |o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap());
    let q = |o: usize| u64::from_le_bytes(b[o..o + 8].try_into().unwrap());
    for r in 0..32 {
        cpu.gpr[r] = w(offset_of!(Cpu, gpr) + 4 * r);
        cpu.ps0[r] = q(offset_of!(Cpu, ps0) + 8 * r);
        cpu.ps1[r] = q(offset_of!(Cpu, ps1) + 8 * r);
    }
    cpu.cr = w(offset_of!(Cpu, cr));
    cpu.xer = w(offset_of!(Cpu, xer));
    cpu.lr = w(offset_of!(Cpu, lr));
    cpu.ctr = w(offset_of!(Cpu, ctr));
    cpu.pc = w(offset_of!(Cpu, pc));
    cpu.msr = w(offset_of!(Cpu, msr));
    cpu.fpscr = w(offset_of!(Cpu, fpscr));
    cpu.srr0 = w(offset_of!(Cpu, srr0));
    cpu.srr1 = w(offset_of!(Cpu, srr1));
    cpu.dar = w(offset_of!(Cpu, dar));
    cpu.dsisr = w(offset_of!(Cpu, dsisr));
    cpu.cur_pc = w(offset_of!(Cpu, cur_pc));
    cpu.exit = b[offset_of!(Cpu, exit)] != 0;
    cpu.faulted = b[offset_of!(Cpu, faulted)] != 0;
}

fn write_cpu(uc: &mut Unicorn<()>, cpu: &Cpu) {
    uc.mem_write(CPUMEM, cpu_bytes(cpu)).unwrap();
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
        _ => (19 << 26) | ((rng.next() & 0x03FF_F800) | 0x0280_0000) | (528 << 1) | (rng.next() & 1),
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
    let mut uc = Unicorn::new(Arch::ARM64, Mode::LITTLE_ENDIAN).unwrap();
    uc.mem_map(CODE, 1 << 20, Prot::ALL).unwrap();
    uc.mem_map(STACK, 1 << 16, Prot::ALL).unwrap();
    let cpu_size = (std::mem::size_of::<Cpu>() as u64 + 0xFFF) & !0xFFF;
    uc.mem_map(CPUMEM, cpu_size, Prot::ALL).unwrap();
    uc.mem_map(CTXMEM, 0x1000, Prot::ALL).unwrap();
    uc.mem_map(PAGES, 0x1000, Prot::ALL).unwrap();
    uc.mem_map(FALLBACK, 0x1000, Prot::ALL).unwrap();
    // `ret` at the fallback address; the hook below does the work first.
    uc.mem_write(FALLBACK, &0xD65F_03C0u32.to_le_bytes()).unwrap();
    let ctx = JitCtx { mem1: MEM1, code_pages: PAGES, fallback: FALLBACK };
    // SAFETY: plain bytes of a repr(C) struct of u64s.
    let ctx_bytes = unsafe { std::slice::from_raw_parts(&ctx as *const JitCtx as *const u8, std::mem::size_of::<JitCtx>()) };
    uc.mem_write(CTXMEM, ctx_bytes).unwrap();

    let mut rng = Rng(0x0BAD_C0DE_1234_5678);
    let mut hw_ref = Hw::new(false);
    let mut hw_jit = Hw::new(false);
    for k in (0..WINDOW_LEN).step_by(4) {
        let w = rng.next();
        hw_ref.mem1[WINDOW + k..WINDOW + k + 4].copy_from_slice(&w.to_be_bytes());
    }
    hw_jit.mem1.copy_from_slice(&hw_ref.mem1);
    // The emulator sees the JIT side's main memory itself, so generated code and handlers
    // always agree on every byte of it.
    // SAFETY: `hw_jit.mem1` is never resized and outlives `uc`'s use of it in this test.
    unsafe { uc.mem_map_ptr(MEM1, crate::hw::MEM1_SIZE as u64, Prot::ALL, hw_jit.mem1.as_mut_ptr().cast()).unwrap() };
    // One page of "cached code" in the window: stores there must take the slow path.
    hw_jit.code_pages[(WINDOW + 0x8000) >> 18] |= 1 << (((WINDOW + 0x8000) >> 12) & 63);
    hw_ref.code_pages.copy_from_slice(&hw_jit.code_pages.clone());
    uc.mem_write(PAGES, &hw_jit.code_pages.iter().take(512).flat_map(|w| w.to_le_bytes()).collect::<Vec<u8>>()).unwrap();

    let blocks = 20_000;
    let mut native_hits = 0usize;
    for case in 0..blocks {
        let len = 1 + (rng.next() % 10) as usize;
        let mut words: Vec<u32> = (0..len).map(|_| instruction(&mut rng)).collect();
        if rng.next() % 2 == 0 {
            words.push(branch(&mut rng));
        }
        let ops: Vec<Op> = words.iter().map(|&w| decode(w)).collect();
        let base = 0x8000_3100u32;
        let insts: Vec<Inst> = ops.iter().enumerate().map(|(k, op)| Inst { ea: base + 4 * k as u32, op }).collect();
        let compiled = compile(&insts, true);
        native_hits += compiled.code.len();

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
        // Reference: the handlers, as the fast interpreter runs a block.
        cpu_ref.exit = false;
        cpu_ref.faulted = false;
        let mut ea = base;
        let mut n_ref = 0u64;
        for op in &ops {
            cpu_ref.cur_pc = ea;
            (op.f)(&mut cpu_ref, &mut hw_ref, op);
            n_ref += 1;
            if cpu_ref.exit {
                break;
            }
            ea = ea.wrapping_add(4);
        }
        if !cpu_ref.exit {
            cpu_ref.pc = ea;
        }

        // The JIT, in the emulator.
        let code: Vec<u8> = compiled.code.iter().flat_map(|w| w.to_le_bytes()).collect();
        uc.mem_write(CODE, &code).unwrap();
        // Unicorn keeps translated code; drop it so the new block is what runs.
        uc.ctl_remove_cache(CODE, CODE + (1 << 20)).unwrap();
        write_cpu(&mut uc, &cpu_jit);
        uc.reg_write(RegisterARM64::X0, CPUMEM).unwrap();
        uc.reg_write(RegisterARM64::X1, 0).unwrap();
        uc.reg_write(RegisterARM64::X2, CTXMEM).unwrap();
        uc.reg_write(RegisterARM64::SP, STACK + 0x8000).unwrap();
        // Returning from the block lands on an address we stop at.
        let end = CODE + 0xF_FF00;
        uc.reg_write(RegisterARM64::X30, end).unwrap();
        // The fallback: run the handler on the host with the emulator's state.
        let hw_ptr = &mut hw_jit as *mut Hw;
        let cpu_ptr = &mut cpu_jit as *mut Cpu;
        let hook = uc
            .add_code_hook(FALLBACK, FALLBACK + 4, move |uc, _addr, _size| {
                // SAFETY: the pointers outlive the emulation call below; nothing else uses them
                // while it runs.
                let (hw, cpu) = unsafe { (&mut *hw_ptr, &mut *cpu_ptr) };
                read_cpu(uc, cpu);
                let op_ptr = uc.reg_read(RegisterARM64::X2).unwrap() as *const Op;
                // SAFETY: the address the compiler embedded, of an Op that is still alive.
                let op = unsafe { &*op_ptr };
                (op.f)(cpu, hw, op);
                if std::env::var("JIT_TRACE").is_ok() {
                    eprintln!("  fallback {:08X} at {:08X}: exit {} faulted {} pc {:08X}", op.inst, cpu.cur_pc, cpu.exit, cpu.faulted, cpu.pc);
                }
                write_cpu(uc, cpu);
            })
            .unwrap();
        uc.emu_start(CODE, end, 0, 1_000_000).unwrap();
        uc.remove_hook(hook).unwrap();
        read_cpu(&uc, &mut cpu_jit);
        let n_jit = uc.reg_read(RegisterARM64::X0).unwrap();

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
    assert!(native_hits > 0);
}
