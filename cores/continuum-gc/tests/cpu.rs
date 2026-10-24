//! The plain interpreter against hand-worked answers from the PowerPC architecture books and
//! the 750CL manual. Each test assembles a few instructions (by the encodings in the books),
//! runs them from address 0 with translation off, and checks registers and memory.

use continuum_gc::bus::{Bus, FlatRam};
use continuum_gc::cpu::{self, Cpu, HID2_PSE, MSR_DR, MSR_EE, MSR_FP, MSR_IR, XER_CA, XER_OV, XER_SO};

// ------------------------------------------------------------------ a tiny assembler

fn d(op: u32, t: u32, a: u32, imm: i32) -> u32 {
    (op << 26) | (t << 21) | (a << 16) | (imm as u32 & 0xFFFF)
}
fn x(t: u32, a: u32, b: u32, xo: u32, rc: u32) -> u32 {
    (31 << 26) | (t << 21) | (a << 16) | (b << 11) | (xo << 1) | rc
}
fn xo(t: u32, a: u32, b: u32, oe: u32, xo: u32, rc: u32) -> u32 {
    (31 << 26) | (t << 21) | (a << 16) | (b << 11) | (oe << 10) | (xo << 1) | rc
}
fn m(op: u32, s: u32, a: u32, sh: u32, mb: u32, me: u32, rc: u32) -> u32 {
    (op << 26) | (s << 21) | (a << 16) | (sh << 11) | (mb << 6) | (me << 1) | rc
}
fn fa(op: u32, t: u32, a: u32, b: u32, c: u32, xo: u32) -> u32 {
    (op << 26) | (t << 21) | (a << 16) | (b << 11) | (c << 6) | (xo << 1)
}
fn fx(t: u32, a: u32, b: u32, xo: u32) -> u32 {
    (63 << 26) | (t << 21) | (a << 16) | (b << 11) | (xo << 1)
}
fn li(t: u32, v: i32) -> u32 {
    d(14, t, 0, v)
}
fn lis(t: u32, v: i32) -> u32 {
    d(15, t, 0, v)
}
fn ori(a: u32, s: u32, v: u32) -> u32 {
    d(24, s, a, v as i32)
}
fn mtspr(spr: u32, s: u32) -> u32 {
    (31 << 26) | (s << 21) | ((spr & 31) << 16) | ((spr >> 5) << 11) | (467 << 1)
}
fn mfspr(t: u32, spr: u32) -> u32 {
    (31 << 26) | (t << 21) | ((spr & 31) << 16) | ((spr >> 5) << 11) | (339 << 1)
}
fn b(off: i32, lk: u32) -> u32 {
    (18 << 26) | (off as u32 & 0x03FF_FFFC) | lk
}
fn bc(bo: u32, bi: u32, off: i32) -> u32 {
    (16 << 26) | (bo << 21) | (bi << 16) | (off as u32 & 0xFFFC)
}
const BLR: u32 = 0x4E80_0020;
const SC: u32 = 0x4400_0002;
const RFI: u32 = 0x4C00_0064;
const NOP: u32 = 0x6000_0000;

/// Loads `prog` at 0, runs `steps` instructions, returns the CPU and memory.
fn run(prog: &[u32], steps: usize, setup: impl FnOnce(&mut Cpu, &mut FlatRam)) -> (Cpu, FlatRam) {
    let mut ram = FlatRam::new(0x10_0000);
    for (k, w) in prog.iter().enumerate() {
        ram.write32(k as u32 * 4, *w);
    }
    let mut cpu = Cpu::default();
    cpu.pc = 0;
    cpu.msr = MSR_FP;
    cpu.hid2 = HID2_PSE;
    setup(&mut cpu, &mut ram);
    for _ in 0..steps {
        cpu.step(&mut ram);
    }
    (cpu, ram)
}

fn regs(prog: &[u32]) -> Cpu {
    run(prog, prog.len(), |_, _| {}).0
}

// ------------------------------------------------------------------ integer

#[test]
fn immediates() {
    let c = regs(&[li(3, -1), lis(4, 0x1234), ori(4, 4, 0x5678), d(14, 5, 4, -0x5678), d(15, 6, 3, 1)]);
    assert_eq!(c.gpr[3], 0xFFFF_FFFF);
    assert_eq!(c.gpr[4], 0x1234_5678);
    assert_eq!(c.gpr[5], 0x1234_0000);
    // addis r6, r3, 1: -1 + 0x10000.
    assert_eq!(c.gpr[6], 0x0000_FFFF);
}

#[test]
fn carries_and_overflow() {
    let c = regs(&[li(3, -1), li(4, 1), xo(5, 3, 4, 0, 10, 0)]); // addc
    assert_eq!(c.gpr[5], 0);
    assert!(c.xer & XER_CA != 0);
    // adde with CA set: 1 + 1 + 1.
    let c = regs(&[li(3, -1), li(4, 1), xo(5, 3, 4, 0, 10, 0), xo(6, 4, 4, 0, 138, 0)]);
    assert_eq!(c.gpr[6], 3);
    assert!(c.xer & XER_CA == 0);
    // subfc r5 = r4 - r3 = 5 - 7: borrow, so CA = 0.
    let c = regs(&[li(3, 7), li(4, 5), xo(5, 3, 4, 0, 8, 0)]);
    assert_eq!(c.gpr[5], (-2i32) as u32);
    assert!(c.xer & XER_CA == 0);
    // subfic r5 = 10 - 3, no borrow: CA = 1.
    let c = regs(&[li(3, 3), d(8, 5, 3, 10)]);
    assert_eq!(c.gpr[5], 7);
    assert!(c.xer & XER_CA != 0);
    // addo. 0x7FFFFFFF + 1 overflows: OV, SO, CR0 = LT|SO.
    let c = regs(&[lis(3, 0x7FFF), ori(3, 3, 0xFFFF), li(4, 1), xo(5, 3, 4, 1, 266, 1)]);
    assert_eq!(c.gpr[5], 0x8000_0000);
    assert!(c.xer & XER_OV != 0 && c.xer & XER_SO != 0);
    assert_eq!(c.cr >> 28, 0b1001);
}

#[test]
fn multiply_and_divide() {
    let c = regs(&[li(3, -3), li(4, 7), xo(5, 3, 4, 0, 235, 0), xo(6, 3, 4, 0, 75, 0), xo(7, 3, 4, 0, 11, 0)]);
    assert_eq!(c.gpr[5], (-21i32) as u32); // mullw
    assert_eq!(c.gpr[6], 0xFFFF_FFFF); // mulhw: high word of -21
    assert_eq!(c.gpr[7], 6); // mulhwu: 0xFFFFFFFD * 7 >> 32
    let c = regs(&[li(3, -7), li(4, 2), xo(5, 3, 4, 0, 491, 0), xo(6, 3, 4, 0, 459, 0)]);
    assert_eq!(c.gpr[5], (-3i32) as u32); // divw truncates toward zero
    assert_eq!(c.gpr[6], 0x7FFF_FFFC); // divwu
    // Division by zero: Gekko gives all ones for a negative dividend, zero otherwise.
    let c = regs(&[li(3, -7), li(4, 0), xo(5, 3, 4, 1, 491, 0), li(8, 9), xo(6, 8, 4, 0, 491, 0), xo(7, 3, 4, 0, 459, 0)]);
    assert_eq!(c.gpr[5], 0xFFFF_FFFF);
    assert!(c.xer & XER_OV != 0);
    assert_eq!(c.gpr[6], 0);
    assert_eq!(c.gpr[7], 0);
}

#[test]
fn rotates_and_shifts() {
    // rlwinm r4, r3, 8, 24, 31: the top byte to the bottom.
    let c = regs(&[lis(3, 0x1234), ori(3, 3, 0x5678), m(21, 3, 4, 8, 24, 31, 0)]);
    assert_eq!(c.gpr[4], 0x12);
    // rlwimi r4, r3, 16, 0, 15: insert the low half of r3 into the high half of r4.
    let c = regs(&[lis(3, 0x1234), ori(3, 3, 0x5678), li(4, 0x0FF), m(20, 3, 4, 16, 0, 15, 0)]);
    assert_eq!(c.gpr[4], 0x5678_00FF);
    // A wrapping mask: rlwinm r4, r3, 0, 28, 3 keeps the top and bottom nibbles.
    let c = regs(&[lis(3, -1), ori(3, 3, 0xFFFF), m(21, 3, 4, 0, 28, 3, 0)]);
    assert_eq!(c.gpr[4], 0xF000_000F);
    // srawi r4, r3, 2 on -5: -2, and CA because 1-bits were shifted out of a negative number.
    let c = regs(&[li(3, -5), x(3, 4, 2, 824, 0)]);
    assert_eq!(c.gpr[4], (-2i32) as u32);
    assert!(c.xer & XER_CA != 0);
    // slw by 32 or more gives 0; sraw by 40 gives the sign.
    let c = regs(&[li(3, -5), li(5, 40), x(3, 4, 5, 24, 0), x(3, 6, 5, 792, 0)]);
    assert_eq!(c.gpr[4], 0);
    assert_eq!(c.gpr[6], 0xFFFF_FFFF);
    // cntlzw, extsb, extsh.
    let c = regs(&[li(3, 0x80), x(3, 4, 0, 26, 0), x(3, 5, 0, 954, 0), li(6, -0x8000), x(6, 7, 0, 922, 0)]);
    assert_eq!(c.gpr[4], 24);
    assert_eq!(c.gpr[5], 0xFFFF_FF80);
    assert_eq!(c.gpr[7], 0xFFFF_8000);
}

#[test]
fn compares_and_cr_logic() {
    // cmpwi cr1, r3, 5 with r3 = -1: LT. cmplwi cr2, r3, 5: GT (unsigned).
    let c = regs(&[li(3, -1), d(11, 1 << 2, 3, 5), d(10, 2 << 2, 3, 5)]);
    assert_eq!(c.cr_field(1), 8);
    assert_eq!(c.cr_field(2), 4);
    // crxor 0,0,0 clears bit 0; creqv 1,1,1 sets bit 1; mcrf cr7, cr1.
    let c = regs(&[li(3, -1), d(11, 1 << 2, 3, 5), (19 << 26) | (6 << 21) | (6 << 16) | (6 << 11) | (289 << 1), (19 << 26) | (7 << 23) | (1 << 18)]);
    // creqv set bit 6, cr1's EQ, so cr1 (copied to cr7) is now LT|EQ.
    assert!(c.cr_bit(6));
    assert_eq!(c.cr_field(7), 0b1010);
}

#[test]
fn branches_loops_and_calls() {
    // r3 = 0; ctr = 10; loop: addi r3, r3, 3; bdnz loop.
    let prog = [li(3, 0), li(4, 10), mtspr(9, 4), d(14, 3, 3, 3), bc(16, 0, -4), NOP];
    let (c, _) = run(&prog, 3 + 20 + 1, |_, _| {});
    assert_eq!(c.gpr[3], 30);
    assert_eq!(c.ctr, 0);
    assert_eq!(c.pc, 0x18);
    // bl to a function at 0x10 that sets r3 and returns; then r4 = 1.
    let prog = [b(0x10, 1), li(4, 1), b(0, 0), NOP, li(3, 42), BLR];
    let (c, _) = run(&prog, 4, |_, _| {});
    assert_eq!(c.gpr[3], 42);
    assert_eq!(c.gpr[4], 1);
    assert_eq!(c.lr, 4);
    // beq taken / not taken.
    let prog = [li(3, 0), d(11, 0, 3, 0), bc(12, 2, 8), li(4, 1), li(5, 1)];
    let (c, _) = run(&prog, 4, |_, _| {});
    assert_eq!((c.gpr[4], c.gpr[5]), (0, 1));
}

#[test]
fn loads_and_stores() {
    let prog = [
        li(3, 0),
        ori(3, 3, 0x1000),
        lis(4, 0x1234),
        ori(4, 4, 0x5678),
        d(36, 4, 3, 0),     // stw r4, 0(r3)
        d(34, 5, 3, 1),     // lbz r5, 1(r3)
        d(42, 6, 3, 2),     // lha r6, 2(r3)
        d(40, 7, 3, 0),     // lhz r7, 0(r3)
        x(8, 0, 3, 534, 0), // lwbrx r8, 0, r3
        d(39, 4, 3, 8),     // stbu r4, 8(r3): r3 becomes 0x1008
    ];
    let (c, mut ram) = run(&prog, prog.len(), |_, _| {});
    assert_eq!(ram.read32(0x1000), 0x1234_5678);
    assert_eq!(c.gpr[5], 0x34);
    assert_eq!(c.gpr[6], 0x5678);
    assert_eq!(c.gpr[7], 0x1234);
    assert_eq!(c.gpr[8], 0x7856_3412);
    assert_eq!(c.gpr[3], 0x1008);
    assert_eq!(ram.read8(0x1008), 0x78);
}

#[test]
fn load_and_store_multiple_and_strings() {
    let prog = [li(29, 1), li(30, 2), li(31, 3), li(3, 0x2000), d(47, 29, 3, 0), d(46, 26, 3, 0)];
    let (c, mut ram) = run(&prog, prog.len(), |_, _| {});
    assert_eq!([ram.read32(0x2000), ram.read32(0x2004), ram.read32(0x2008)], [1, 2, 3]);
    assert_eq!(&c.gpr[26..29], &[1, 2, 3]);
    // lswi r5, r3, 6: six bytes into r5 and the top half of r6.
    let prog = [li(3, 0x3000), x(5, 3, 6, 597, 0)];
    let (c, _) = run(&prog, 2, |_, ram| ram.load(0x3000, b"ABCDEF"));
    assert_eq!(c.gpr[5], u32::from_be_bytes(*b"ABCD"));
    assert_eq!(c.gpr[6], u32::from_be_bytes([b'E', b'F', 0, 0]));
}

#[test]
fn reservations() {
    // lwarx; stwcx. succeeds; a second stwcx. fails (the reservation is gone).
    let prog = [li(3, 0x1000), li(4, 7), x(5, 0, 3, 20, 0), x(4, 0, 3, 150, 1), x(4, 0, 3, 150, 1)];
    let (c, _) = run(&prog, 4, |_, _| {});
    assert_eq!(c.cr_field(0) & 2, 2);
    let (c, mut ram) = run(&prog, 5, |_, _| {});
    assert_eq!(c.cr_field(0) & 2, 0);
    assert_eq!(ram.read32(0x1000), 7);
}

#[test]
fn system_call_and_return() {
    // sc at 0x8 goes to 0xC00 with SRR0 = 0xC; the handler at 0xC00 does rfi back.
    let mut prog = vec![NOP; 0x310];
    prog[2] = SC;
    prog[3] = li(9, 1);
    prog[0xC00 / 4] = li(8, 5);
    prog[0xC00 / 4 + 1] = RFI;
    let (c, _) = run(&prog, 3, |c, _| c.msr = MSR_FP | MSR_EE);
    assert_eq!(c.pc, 0xC00);
    assert_eq!(c.srr0, 0xC);
    assert_eq!(c.msr & MSR_EE, 0);
    let (c, _) = run(&prog, 6, |c, _| c.msr = MSR_FP | MSR_EE);
    assert_eq!((c.gpr[8], c.gpr[9]), (5, 1));
    assert!(c.msr & MSR_EE != 0);
}

#[test]
fn illegal_and_privileged_instructions_trap() {
    let mut prog = vec![NOP; 0x200];
    prog[0] = 0; // opcode 0 is illegal
    let (c, _) = run(&prog, 1, |_, _| {});
    assert_eq!(c.pc, 0x700);
    assert!(c.srr1 & cpu::PROGRAM_ILLEGAL != 0);
    assert_eq!(c.srr0, 0);
    // mfmsr in user mode.
    prog[0] = x(3, 0, 0, 83, 0);
    let (c, _) = run(&prog, 1, |c, _| c.msr |= cpu::MSR_PR);
    assert_eq!(c.pc, 0x700);
    assert!(c.srr1 & cpu::PROGRAM_PRIVILEGED != 0);
}

#[test]
fn decrementer_interrupt() {
    let mut prog = vec![NOP; 0x400];
    prog[0x900 / 4] = li(7, 1);
    // DEC = 2 ticks = 24 cycles of NOPs, then it passes zero and the interrupt is taken.
    let (c, _) = run(&prog, 40, |c, _| {
        c.msr = MSR_EE;
        c.dec = 1;
    });
    assert_eq!(c.gpr[7], 1);
    assert!(c.srr0 < 0x900);
}

// ------------------------------------------------------------------ address translation

#[test]
fn bat_translation() {
    // DBAT0: effective 0x80000000, 256 MB (BL = 0x7FF), supervisor valid, maps to physical 0, read/write.
    let prog = [
        lis(3, 0x8000u32 as i32),
        ori(3, 3, 0x1FFE),
        mtspr(536, 3),
        li(4, 2),
        mtspr(537, 4),
        lis(5, 0x8000u32 as i32),
        li(6, 77),
        d(36, 6, 5, 0x4000), // stw r6, 0x4000(r5) -> physical 0x4000
    ];
    let (_, mut ram) = run(&prog, prog.len(), |c, _| c.msr = MSR_FP | MSR_DR);
    assert_eq!(ram.read32(0x4000), 77);
}

#[test]
fn page_table_translation_and_dsi() {
    // Hand-built page table at 0x10000 (SDR1 = 0x10000, mask 0): map effective 0x30005000 to
    // physical 0x7000 through segment 3 (VSID 0x123).
    let mut ram = FlatRam::new(0x10_0000);
    let vsid = 0x123u32;
    let ea = 0x3000_5000u32;
    let page_index = (ea >> 12) & 0xFFFF;
    let hash = (vsid & 0x7FFFF) ^ page_index;
    let pteg = 0x10000 | ((hash & 0x3FF) << 6);
    ram.write32(pteg, 0x8000_0000 | (vsid << 7) | (page_index >> 10));
    ram.write32(pteg + 4, 0x7000 | 2);
    ram.write32(0x7010, 0xCAFE_F00D);
    let mut cpu = Cpu::default();
    cpu.msr = MSR_DR;
    cpu.sdr1 = 0x10000;
    cpu.sr[3] = vsid;
    let prog = [lis(3, 0x3000), d(32, 4, 3, 0x5010), d(32, 5, 3, 0x6000)];
    for (k, w) in prog.iter().enumerate() {
        ram.write32(k as u32 * 4, *w);
    }
    cpu.pc = 0;
    cpu.step(&mut ram);
    cpu.step(&mut ram);
    assert_eq!(cpu.gpr[4], 0xCAFE_F00D);
    // The referenced bit was set in the page table entry.
    assert!(ram.read32(pteg + 4) & 0x100 != 0);
    // An unmapped page: DSI with DAR the address.
    cpu.step(&mut ram);
    assert_eq!(cpu.pc, 0x300);
    assert_eq!(cpu.dar, 0x3000_6000);
    assert_eq!(cpu.srr0, 8);
    let _ = MSR_IR;
}

#[test]
fn special_registers() {
    let prog = [li(3, 0x55), mtspr(272, 3), mfspr(4, 272), mfspr(5, 287), mtspr(912, 3), mfspr(6, 912)];
    let c = regs(&prog);
    assert_eq!(c.gpr[4], 0x55);
    assert_eq!(c.gpr[5], cpu::PVR_GEKKO);
    assert_eq!(c.gpr[6], 0x55);
}

// ------------------------------------------------------------------ floating point

fn fpr(c: &Cpu, r: usize) -> f64 {
    f64::from_bits(c.ps0[r])
}

/// Puts f64 values in FPRs 1.. through memory (lfd).
fn with_fprs(values: &[f64], body: &[u32]) -> Cpu {
    let mut prog = vec![li(3, 0x2000)];
    for (k, _) in values.iter().enumerate() {
        prog.push(d(50, k as u32 + 1, 3, k as i32 * 8));
    }
    prog.extend_from_slice(body);
    let n = prog.len();
    run(&prog, n, |_, ram| {
        for (k, v) in values.iter().enumerate() {
            ram.write64(0x2000 + k as u32 * 8, v.to_bits());
        }
    })
    .0
}

#[test]
fn double_arithmetic() {
    let c = with_fprs(
        &[1.5, 2.25, 3.0],
        &[fa(63, 4, 1, 2, 0, 21), fa(63, 5, 1, 2, 0, 20), fa(63, 6, 1, 0, 2, 25), fa(63, 7, 1, 2, 0, 18), fa(63, 8, 1, 2, 3, 29)],
    );
    assert_eq!(fpr(&c, 4), 3.75);
    assert_eq!(fpr(&c, 5), -0.75);
    assert_eq!(fpr(&c, 6), 3.375);
    assert_eq!(fpr(&c, 7), 1.5 / 2.25);
    // fmadd f8 = f1 * f3 + f2 = 4.5 + 2.25.
    assert_eq!(fpr(&c, 8), 6.75);
    // fnmsub: -(a*c - b).
    let c = with_fprs(&[2.0, 1.0, 3.0], &[fa(63, 4, 1, 2, 3, 30)]);
    assert_eq!(fpr(&c, 4), -5.0);
}

#[test]
fn single_results_round_and_fill_both_halves() {
    // fadds of 1 + 2^-30 rounds to 1.0 in single precision, and lands in ps0 and ps1.
    let c = with_fprs(&[1.0, 2f64.powi(-30)], &[fa(59, 4, 1, 2, 0, 21)]);
    assert_eq!(fpr(&c, 4), 1.0);
    assert_eq!(c.ps1[4], 1.0f64.to_bits());
    // frsp.
    let c = with_fprs(&[0.1], &[fx(4, 0, 1, 12)]);
    assert_eq!(fpr(&c, 4), 0.1f32 as f64);
    // A double instruction leaves ps1 alone.
    let c = with_fprs(&[1.0, 2.0], &[fa(59, 4, 1, 1, 0, 21), fa(63, 4, 1, 2, 0, 21)]);
    assert_eq!(fpr(&c, 4), 3.0);
    assert_eq!(f64::from_bits(c.ps1[4]), 2.0);
}

#[test]
fn nan_and_invalid_rules() {
    let inf = f64::INFINITY;
    // inf - inf is invalid: the default quiet NaN and VXISI.
    let c = with_fprs(&[inf, inf], &[fa(63, 4, 1, 2, 0, 20)]);
    assert_eq!(c.ps0[4], 0x7FF8_0000_0000_0000);
    assert!(c.fpscr & (1 << 23) != 0 && c.fpscr & (1 << 29) != 0 && c.fpscr & (1 << 31) != 0);
    // A NaN operand passes through (frA first), quieted.
    let snan = f64::from_bits(0x7FF0_0000_0000_0001);
    let qnan_b = f64::from_bits(0x7FF8_0000_0000_0002);
    let c = with_fprs(&[snan, qnan_b], &[fa(63, 4, 1, 2, 0, 21), fa(63, 5, 2, 1, 0, 21)]);
    assert_eq!(c.ps0[4], 0x7FF8_0000_0000_0001);
    assert_eq!(c.ps0[5], 0x7FF8_0000_0000_0002);
    assert!(c.fpscr & (1 << 24) != 0, "VXSNAN");
    // Divide by zero: infinity and ZX.
    let c = with_fprs(&[1.0, 0.0], &[fa(63, 4, 1, 2, 0, 18)]);
    assert_eq!(fpr(&c, 4), inf);
    assert!(c.fpscr & (1 << 26) != 0);
}

#[test]
fn compare_convert_and_move() {
    // fcmpu cr3, f1, f2.
    let c = with_fprs(&[1.0, 2.0, f64::NAN], &[fx(3 << 2, 1, 2, 0), fx(4 << 2, 1, 3, 0)]);
    assert_eq!(c.cr_field(3), 8);
    assert_eq!(c.cr_field(4), 1);
    // fctiwz of -2.7 is -2; fctiw (round to nearest even) of 2.5 is 2; of 3.5 is 4; huge saturates.
    let c = with_fprs(&[-2.7, 2.5, 3.5, 1e20], &[fx(5, 0, 1, 15), fx(6, 0, 2, 14), fx(7, 0, 3, 14), fx(8, 0, 4, 15)]);
    assert_eq!(c.ps0[5] as u32 as i32, -2);
    assert_eq!(c.ps0[6] as u32, 2);
    assert_eq!(c.ps0[7] as u32, 4);
    assert_eq!(c.ps0[8] as u32, 0x7FFF_FFFF);
    assert_eq!(c.ps0[5] >> 32, 0xFFF8_0000);
    // fneg, fabs, fnabs, fmr are bit operations.
    let c = with_fprs(&[-3.0], &[fx(4, 0, 1, 40), fx(5, 0, 1, 264), fx(6, 0, 5, 136), fx(7, 0, 1, 72)]);
    assert_eq!([fpr(&c, 4), fpr(&c, 5), fpr(&c, 6), fpr(&c, 7)], [3.0, 3.0, -3.0, -3.0]);
    // fsel f4 = f1 >= 0 ? f2 : f3.
    let c = with_fprs(&[-0.0, 10.0, 20.0], &[fa(63, 4, 1, 3, 2, 23)]);
    assert_eq!(fpr(&c, 4), 10.0);
}

#[test]
fn single_loads_and_stores_are_exact() {
    use continuum_gc::cpu::{double_to_single, single_to_double};
    // A signalling NaN survives lfs -> stfs bit for bit, which a host float conversion would break.
    for w in [0x7F80_0001u32, 0xFF80_1234, 0x0000_0001, 0x8040_0000, 0x3F80_0000, 0x7F7F_FFFF, 0] {
        assert_eq!(double_to_single(single_to_double(w)), w, "{w:08x}");
    }
    assert_eq!(f64::from_bits(single_to_double(0x0000_0001)), f32::from_bits(1) as f64);
    // stfs of a double too small for a normal single gives the truncated denormal.
    // 2^-127 = 2^22 * 2^-149: fraction 0x400000.
    assert_eq!(double_to_single(2f64.powi(-127).to_bits()), 0x0040_0000);
}

#[test]
fn fpscr_moves() {
    // mtfsfi 7, 1 sets RN = 1 (toward zero); mffs reads it back; mtfsb1 0... sets bit 31-0 (FX).
    let c = with_fprs(&[], &[(63 << 26) | (7 << 23) | (1 << 12) | (134 << 1), fx(4, 0, 0, 583)]);
    assert_eq!(c.fpscr & 3, 1);
    assert_eq!(c.ps0[4] as u32 & 3, 1);
}

// ------------------------------------------------------------------ paired singles

#[test]
fn paired_single_arithmetic() {
    // psq_l f1, 0(r3), w=0, gqr0 (floats) loads two singles.
    let prog = [
        li(3, 0x2000),
        (56 << 26) | (1 << 21) | (3 << 16),
        (56 << 26) | (2 << 21) | (3 << 16) | 8,
        fa(4, 4, 1, 2, 0, 21),             // ps_add f4 = f1 + f2
        fa(4, 5, 1, 0, 2, 25),             // ps_mul f5 = f1 * f2
        fa(4, 6, 1, 2, 2, 10),             // ps_sum0 f6: ps0 = f1.0 + f2.1, ps1 = f2.1
        (4 << 26) | (7 << 21) | (1 << 16) | (2 << 11) | (560 << 1), // ps_merge01 f7 = f1.0, f2.1
        fa(4, 8, 1, 2, 0, 20),             // ps_sub
        fa(4, 9, 1, 2, 2, 29),             // ps_madd f9 = f1*f2 + f2
    ];
    let (c, _) = run(&prog, prog.len(), |_, ram| {
        for (k, v) in [1.0f32, 2.0, 10.0, 20.0].iter().enumerate() {
            ram.write32(0x2000 + 4 * k as u32, v.to_bits());
        }
    });
    let pair = |r: usize| (f64::from_bits(c.ps0[r]), f64::from_bits(c.ps1[r]));
    assert_eq!(pair(1), (1.0, 2.0));
    assert_eq!(pair(4), (11.0, 22.0));
    assert_eq!(pair(5), (10.0, 40.0));
    assert_eq!(pair(6), (21.0, 20.0));
    assert_eq!(pair(7), (1.0, 20.0));
    assert_eq!(pair(8), (-9.0, -18.0));
    assert_eq!(pair(9), (20.0, 60.0));
}

#[test]
fn quantized_loads_and_stores() {
    // GQR1: load type u8 with scale 4 (divide by 16); store type s16 with scale 2 (times 4).
    let gqr = (4 << 24) | (4 << 16) | (2 << 8) | 7;
    let prog = [
        lis(4, gqr >> 16),
        ori(4, 4, gqr as u32 & 0xFFFF),
        mtspr(913, 4),
        li(3, 0x2000),
        (56 << 26) | (1 << 21) | (3 << 16) | (1 << 12), // psq_l f1, 0(r3), 0, qr1
        (60 << 26) | (1 << 21) | (3 << 16) | (1 << 12) | 0x10, // psq_st f1, 16(r3), 0, qr1
        (56 << 26) | (2 << 21) | (3 << 16) | (1 << 15) | (1 << 12), // psq_l f2 with w=1
    ];
    let (c, mut ram) = run(&prog, prog.len(), |_, ram| {
        ram.write8(0x2000, 32);
        ram.write8(0x2001, 255);
    });
    assert_eq!((f64::from_bits(c.ps0[1]), f64::from_bits(c.ps1[1])), (2.0, 255.0 / 16.0));
    // Stored as s16, times 4: 8 and 63 (63.75 truncated).
    assert_eq!(ram.read16(0x2010), 8);
    assert_eq!(ram.read16(0x2012), 63);
    // w=1 loads one value and 1.0.
    assert_eq!((f64::from_bits(c.ps0[2]), f64::from_bits(c.ps1[2])), (2.0, 1.0));
}

#[test]
fn paired_singles_need_hid2_pse() {
    let mut prog = vec![NOP; 0x200];
    prog[0] = fa(4, 4, 1, 2, 0, 21);
    let (c, _) = run(&prog, 1, |c, _| c.hid2 = 0);
    assert_eq!(c.pc, 0x700);
    // And FP instructions need MSR[FP].
    prog[0] = fa(63, 4, 1, 2, 0, 21);
    let (c, _) = run(&prog, 1, |c, _| c.msr = 0);
    assert_eq!(c.pc, 0x800);
}

#[test]
fn locked_cache() {
    // HID2[LCE] on: 0xE0000000 is 16 KB of scratch memory; dcbz_l zeroes a line of it.
    let prog = [lis(3, 0xE000u32 as i32), li(4, 99), d(36, 4, 3, 0x40), d(32, 5, 3, 0x40), ((4 << 26) | (3 << 11) | (1014 << 1)), ];
    let (c, mut ram) = run(&prog, 4, |c, _| c.hid2 |= cpu::HID2_LCE);
    assert_eq!(c.gpr[5], 99);
    assert_eq!(ram.read32(0x40), 0, "main memory untouched");
}

#[test]
fn save_state_round_trip() {
    let c = regs(&[li(3, 5), lis(4, 7)]);
    let bytes = bincode::serialize(&c).unwrap();
    let back: Cpu = bincode::deserialize(&bytes).unwrap();
    assert_eq!(back.gpr, c.gpr);
    assert_eq!(back.pc, c.pc);
}
