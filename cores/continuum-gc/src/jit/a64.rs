//! A small ARM64 (AArch64) assembler: just the instructions the JIT emits, encoded from the
//! Arm Architecture Reference Manual. Checked against LLVM's assembler by the tests below.

/// A general-purpose register number (0-30; 31 is SP or the zero register by context).
pub type R = u32;
pub const ZR: R = 31;
pub const SP: R = 31;

/// Condition codes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cond {
    Eq = 0,
    Ne = 1,
    Hs = 2,
    Lo = 3,
    Mi = 4,
    Pl = 5,
    Vs = 6,
    Vc = 7,
    Hi = 8,
    Ls = 9,
    Ge = 10,
    Lt = 11,
    Gt = 12,
    Le = 13,
    Al = 14,
}

impl Cond {
    pub fn invert(self) -> Cond {
        use Cond::*;
        match self {
            Eq => Ne,
            Ne => Eq,
            Hs => Lo,
            Lo => Hs,
            Mi => Pl,
            Pl => Mi,
            Vs => Vc,
            Vc => Vs,
            Hi => Ls,
            Ls => Hi,
            Ge => Lt,
            Lt => Ge,
            Gt => Le,
            Le => Gt,
            Al => Al,
        }
    }
}

/// Shift kinds for shifted-register operands.
#[derive(Clone, Copy)]
pub enum Shift {
    Lsl = 0,
    Lsr = 1,
    Asr = 2,
    Ror = 3,
}

#[derive(Default)]
pub struct Asm {
    pub code: Vec<u32>,
}

/// sf bit: 64-bit (true) or 32-bit operation.
fn sf(x: bool) -> u32 {
    (x as u32) << 31
}

impl Asm {
    pub fn new() -> Self {
        Asm { code: Vec::with_capacity(1024) }
    }

    pub fn emit(&mut self, w: u32) {
        self.code.push(w);
    }

    /// The index of the next instruction (for branches).
    pub fn here(&self) -> usize {
        self.code.len()
    }

    pub fn bytes(&self) -> Vec<u8> {
        self.code.iter().flat_map(|w| w.to_le_bytes()).collect()
    }

    // ------------------------------------------------------------------ moves

    /// MOVZ/MOVK sequence for any 64-bit (x) or 32-bit constant.
    pub fn mov_imm(&mut self, x: bool, rd: R, v: u64) {
        let v = if x { v } else { v & 0xFFFF_FFFF };
        let parts = if x { 4 } else { 2 };
        let mut first = true;
        for k in 0..parts {
            let h = ((v >> (16 * k)) & 0xFFFF) as u32;
            if h == 0 {
                continue;
            }
            let opc = if first { 0b10 } else { 0b11 }; // MOVZ, MOVK
            self.emit(sf(x) | (opc << 29) | (0b100101 << 23) | ((k as u32) << 21) | (h << 5) | rd);
            first = false;
        }
        if first {
            // All zero.
            self.emit(sf(x) | (0b10 << 29) | (0b100101 << 23) | rd);
        }
    }

    /// MOV (register), as ORR rd, zr, rm.
    pub fn mov(&mut self, x: bool, rd: R, rm: R) {
        self.logic_reg(x, 0b01, false, rd, ZR, rm, Shift::Lsl, 0);
    }

    // ------------------------------------------------------------------ loads and stores

    /// LDR/STR with an unsigned scaled offset: size 0 = byte, 1 = half, 2 = word, 3 = double.
    fn ldst_uimm(&mut self, size: u32, load: bool, signed: bool, rt: R, rn: R, off: u32) {
        assert!(off % (1 << size) == 0 && off >> size < 4096, "offset {off} out of range");
        let opc = match (load, signed) {
            (false, _) => 0b00,
            (true, false) => 0b01,
            (true, true) => 0b10, // sign-extend to 64 bits
        };
        self.emit((size << 30) | (0b111001 << 24) | (opc << 22) | ((off >> size) << 10) | (rn << 5) | rt);
    }

    pub fn ldr_w(&mut self, rt: R, rn: R, off: u32) {
        self.ldst_uimm(2, true, false, rt, rn, off);
    }
    pub fn ldr_x(&mut self, rt: R, rn: R, off: u32) {
        self.ldst_uimm(3, true, false, rt, rn, off);
    }
    pub fn str_w(&mut self, rt: R, rn: R, off: u32) {
        self.ldst_uimm(2, false, false, rt, rn, off);
    }
    pub fn str_x(&mut self, rt: R, rn: R, off: u32) {
        self.ldst_uimm(3, false, false, rt, rn, off);
    }
    pub fn ldrb(&mut self, rt: R, rn: R, off: u32) {
        self.ldst_uimm(0, true, false, rt, rn, off);
    }
    pub fn strb(&mut self, rt: R, rn: R, off: u32) {
        self.ldst_uimm(0, false, false, rt, rn, off);
    }

    /// LDR/STR with a register offset: [rn, rm] (no shift). size as above.
    fn ldst_reg(&mut self, size: u32, load: bool, rt: R, rn: R, rm: R) {
        let opc = if load { 0b01 } else { 0b00 };
        // option 011 (LSL/UXTX), S = 0.
        self.emit((size << 30) | (0b111000 << 24) | (opc << 22) | (1 << 21) | (rm << 16) | (0b011 << 13) | (0b10 << 10) | (rn << 5) | rt);
    }
    pub fn ldr_w_reg(&mut self, rt: R, rn: R, rm: R) {
        self.ldst_reg(2, true, rt, rn, rm);
    }
    pub fn ldrh_reg(&mut self, rt: R, rn: R, rm: R) {
        self.ldst_reg(1, true, rt, rn, rm);
    }
    pub fn ldrb_reg(&mut self, rt: R, rn: R, rm: R) {
        self.ldst_reg(0, true, rt, rn, rm);
    }
    pub fn ldr_x_reg(&mut self, rt: R, rn: R, rm: R) {
        self.ldst_reg(3, true, rt, rn, rm);
    }
    pub fn str_w_reg(&mut self, rt: R, rn: R, rm: R) {
        self.ldst_reg(2, false, rt, rn, rm);
    }
    pub fn strh_reg(&mut self, rt: R, rn: R, rm: R) {
        self.ldst_reg(1, false, rt, rn, rm);
    }
    pub fn strb_reg(&mut self, rt: R, rn: R, rm: R) {
        self.ldst_reg(0, false, rt, rn, rm);
    }
    pub fn str_x_reg(&mut self, rt: R, rn: R, rm: R) {
        self.ldst_reg(3, false, rt, rn, rm);
    }
    /// LDR Xt, [Xn, Xm, LSL #3].
    pub fn ldr_x_reg_lsl3(&mut self, rt: R, rn: R, rm: R) {
        self.emit((3 << 30) | (0b111000 << 24) | (0b01 << 22) | (1 << 21) | (rm << 16) | (0b011 << 13) | (1 << 12) | (0b10 << 10) | (rn << 5) | rt);
    }

    /// STP/LDP x registers, pre-indexed (push) or post-indexed (pop), offset in bytes.
    pub fn stp_pre(&mut self, rt: R, rt2: R, rn: R, off: i32) {
        let imm7 = ((off / 8) as u32) & 0x7F;
        self.emit((0b10 << 30) | (0b101 << 27) | (0b011 << 23) | (imm7 << 15) | (rt2 << 10) | (rn << 5) | rt);
    }
    /// LDP x registers with a signed offset (no write-back).
    pub fn ldp_off(&mut self, rt: R, rt2: R, rn: R, off: i32) {
        let imm7 = ((off / 8) as u32) & 0x7F;
        self.emit((0b10 << 30) | (0b101 << 27) | (0b010 << 23) | (1 << 22) | (imm7 << 15) | (rt2 << 10) | (rn << 5) | rt);
    }
    pub fn ldp_post(&mut self, rt: R, rt2: R, rn: R, off: i32) {
        let imm7 = ((off / 8) as u32) & 0x7F;
        self.emit((0b10 << 30) | (0b101 << 27) | (0b001 << 23) | (1 << 22) | (imm7 << 15) | (rt2 << 10) | (rn << 5) | rt);
    }

    // ------------------------------------------------------------------ arithmetic

    /// ADD/SUB (immediate, 12 bits, optionally shifted by 12); s sets flags.
    pub fn addsub_imm(&mut self, x: bool, sub: bool, s: bool, rd: R, rn: R, imm: u32) {
        let (imm, sh) = if imm < 4096 { (imm, 0) } else { (imm >> 12, 1) };
        assert!(imm < 4096);
        self.emit(sf(x) | ((sub as u32) << 30) | ((s as u32) << 29) | (0b100010 << 23) | (sh << 22) | (imm << 10) | (rn << 5) | rd);
    }
    pub fn add_imm(&mut self, x: bool, rd: R, rn: R, imm: u32) {
        self.addsub_imm(x, false, false, rd, rn, imm);
    }
    pub fn sub_imm(&mut self, x: bool, rd: R, rn: R, imm: u32) {
        self.addsub_imm(x, true, false, rd, rn, imm);
    }

    /// ADD/SUB (shifted register).
    #[allow(clippy::too_many_arguments)]
    pub fn addsub_reg(&mut self, x: bool, sub: bool, s: bool, rd: R, rn: R, rm: R, shift: Shift, amount: u32) {
        self.emit(sf(x) | ((sub as u32) << 30) | ((s as u32) << 29) | (0b01011 << 24) | ((shift as u32) << 22) | (rm << 16) | (amount << 10) | (rn << 5) | rd);
    }
    pub fn add(&mut self, x: bool, rd: R, rn: R, rm: R) {
        self.addsub_reg(x, false, false, rd, rn, rm, Shift::Lsl, 0);
    }
    pub fn adds(&mut self, x: bool, rd: R, rn: R, rm: R) {
        self.addsub_reg(x, false, true, rd, rn, rm, Shift::Lsl, 0);
    }
    pub fn sub(&mut self, x: bool, rd: R, rn: R, rm: R) {
        self.addsub_reg(x, true, false, rd, rn, rm, Shift::Lsl, 0);
    }
    pub fn subs(&mut self, x: bool, rd: R, rn: R, rm: R) {
        self.addsub_reg(x, true, true, rd, rn, rm, Shift::Lsl, 0);
    }
    pub fn cmp(&mut self, x: bool, rn: R, rm: R) {
        self.subs(x, ZR, rn, rm);
    }
    pub fn cmp_imm(&mut self, x: bool, rn: R, imm: u32) {
        self.addsub_imm(x, true, true, ZR, rn, imm);
    }

    /// ADC/ADCS and SBC/SBCS.
    pub fn adc(&mut self, x: bool, sub: bool, s: bool, rd: R, rn: R, rm: R) {
        self.emit(sf(x) | ((sub as u32) << 30) | ((s as u32) << 29) | (0b11010000 << 21) | (rm << 16) | (rn << 5) | rd);
    }

    /// Logical (shifted register): opc 00 AND, 01 ORR, 10 EOR, 11 ANDS; n inverts rm (BIC, ORN, EON).
    #[allow(clippy::too_many_arguments)]
    pub fn logic_reg(&mut self, x: bool, opc: u32, n: bool, rd: R, rn: R, rm: R, shift: Shift, amount: u32) {
        self.emit(sf(x) | (opc << 29) | (0b01010 << 24) | ((shift as u32) << 22) | ((n as u32) << 21) | (rm << 16) | (amount << 10) | (rn << 5) | rd);
    }
    pub fn and(&mut self, x: bool, rd: R, rn: R, rm: R) {
        self.logic_reg(x, 0b00, false, rd, rn, rm, Shift::Lsl, 0);
    }
    pub fn orr(&mut self, x: bool, rd: R, rn: R, rm: R) {
        self.logic_reg(x, 0b01, false, rd, rn, rm, Shift::Lsl, 0);
    }
    pub fn eor(&mut self, x: bool, rd: R, rn: R, rm: R) {
        self.logic_reg(x, 0b10, false, rd, rn, rm, Shift::Lsl, 0);
    }
    pub fn ands(&mut self, x: bool, rd: R, rn: R, rm: R) {
        self.logic_reg(x, 0b11, false, rd, rn, rm, Shift::Lsl, 0);
    }
    pub fn bic(&mut self, x: bool, rd: R, rn: R, rm: R) {
        self.logic_reg(x, 0b00, true, rd, rn, rm, Shift::Lsl, 0);
    }
    pub fn orn(&mut self, x: bool, rd: R, rn: R, rm: R) {
        self.logic_reg(x, 0b01, true, rd, rn, rm, Shift::Lsl, 0);
    }
    pub fn eon(&mut self, x: bool, rd: R, rn: R, rm: R) {
        self.logic_reg(x, 0b10, true, rd, rn, rm, Shift::Lsl, 0);
    }
    pub fn mvn(&mut self, x: bool, rd: R, rm: R) {
        self.orn(x, rd, ZR, rm);
    }
    /// TST rn, rm (ANDS zr).
    pub fn tst(&mut self, x: bool, rn: R, rm: R) {
        self.ands(x, ZR, rn, rm);
    }

    /// Data processing (2 source): UDIV 0b000010, SDIV 0b000011, LSLV 0b001000, LSRV 0b001001,
    /// ASRV 0b001010, RORV 0b001011.
    fn dp2(&mut self, x: bool, op: u32, rd: R, rn: R, rm: R) {
        self.emit(sf(x) | (0b0011010110 << 21) | (rm << 16) | (op << 10) | (rn << 5) | rd);
    }
    pub fn udiv(&mut self, x: bool, rd: R, rn: R, rm: R) {
        self.dp2(x, 0b000010, rd, rn, rm);
    }
    pub fn sdiv(&mut self, x: bool, rd: R, rn: R, rm: R) {
        self.dp2(x, 0b000011, rd, rn, rm);
    }
    pub fn lslv(&mut self, x: bool, rd: R, rn: R, rm: R) {
        self.dp2(x, 0b001000, rd, rn, rm);
    }
    pub fn lsrv(&mut self, x: bool, rd: R, rn: R, rm: R) {
        self.dp2(x, 0b001001, rd, rn, rm);
    }
    pub fn asrv(&mut self, x: bool, rd: R, rn: R, rm: R) {
        self.dp2(x, 0b001010, rd, rn, rm);
    }
    pub fn rorv(&mut self, x: bool, rd: R, rn: R, rm: R) {
        self.dp2(x, 0b001011, rd, rn, rm);
    }

    /// Data processing (1 source): CLZ 0b000100, REV (W) 0b000010, REV (X) 0b000011, REV16 0b000001.
    fn dp1(&mut self, x: bool, op: u32, rd: R, rn: R) {
        self.emit(sf(x) | (0b1011010110 << 21) | (op << 10) | (rn << 5) | rd);
    }
    pub fn clz(&mut self, x: bool, rd: R, rn: R) {
        self.dp1(x, 0b000100, rd, rn);
    }
    pub fn rev_w(&mut self, rd: R, rn: R) {
        self.dp1(false, 0b000010, rd, rn);
    }
    pub fn rev_x(&mut self, rd: R, rn: R) {
        self.dp1(true, 0b000011, rd, rn);
    }
    pub fn rev16_w(&mut self, rd: R, rn: R) {
        self.dp1(false, 0b000001, rd, rn);
    }

    /// MADD/MSUB (rd = ra +/- rn * rm), SMULL/UMULL (64 = 32 x 32), SMULH/UMULH.
    pub fn madd(&mut self, x: bool, rd: R, rn: R, rm: R, ra: R) {
        self.emit(sf(x) | (0b0011011000 << 21) | (rm << 16) | (ra << 10) | (rn << 5) | rd);
    }
    pub fn mul(&mut self, x: bool, rd: R, rn: R, rm: R) {
        self.madd(x, rd, rn, rm, ZR);
    }
    pub fn smull(&mut self, rd: R, rn: R, rm: R) {
        self.emit((0b10011011001 << 21) | (rm << 16) | (ZR << 10) | (rn << 5) | rd);
    }
    pub fn umull(&mut self, rd: R, rn: R, rm: R) {
        self.emit((0b10011011101 << 21) | (rm << 16) | (ZR << 10) | (rn << 5) | rd);
    }

    /// Bitfield moves: SBFM (opc 00), BFM (01), UBFM (10).
    #[allow(clippy::too_many_arguments)]
    pub fn bfm(&mut self, x: bool, opc: u32, rd: R, rn: R, immr: u32, imms: u32) {
        let n = x as u32;
        self.emit(sf(x) | (opc << 29) | (0b100110 << 23) | (n << 22) | (immr << 16) | (imms << 10) | (rn << 5) | rd);
    }
    pub fn lsl_imm(&mut self, x: bool, rd: R, rn: R, sh: u32) {
        let w = if x { 64 } else { 32 };
        self.bfm(x, 0b10, rd, rn, (w - sh) % w, w - 1 - sh);
    }
    pub fn lsr_imm(&mut self, x: bool, rd: R, rn: R, sh: u32) {
        let w = if x { 63 } else { 31 };
        self.bfm(x, 0b10, rd, rn, sh, w);
    }
    pub fn asr_imm(&mut self, x: bool, rd: R, rn: R, sh: u32) {
        let w = if x { 63 } else { 31 };
        self.bfm(x, 0b00, rd, rn, sh, w);
    }
    pub fn sxtb(&mut self, x: bool, rd: R, rn: R) {
        self.bfm(x, 0b00, rd, rn, 0, 7);
    }
    pub fn sxth(&mut self, x: bool, rd: R, rn: R) {
        self.bfm(x, 0b00, rd, rn, 0, 15);
    }
    pub fn sxtw(&mut self, rd: R, rn: R) {
        self.bfm(true, 0b00, rd, rn, 0, 31);
    }
    /// UBFX rd, rn, #lsb, #width.
    pub fn ubfx(&mut self, x: bool, rd: R, rn: R, lsb: u32, width: u32) {
        self.bfm(x, 0b10, rd, rn, lsb, lsb + width - 1);
    }
    /// BFI rd, rn, #lsb, #width.
    pub fn bfi(&mut self, x: bool, rd: R, rn: R, lsb: u32, width: u32) {
        let w = if x { 64 } else { 32 };
        self.bfm(x, 0b01, rd, rn, (w - lsb) % w, width - 1);
    }
    /// ROR (immediate) as EXTR rd, rn, rn, #sh.
    pub fn ror_imm(&mut self, x: bool, rd: R, rn: R, sh: u32) {
        let n = x as u32;
        self.emit(sf(x) | (0b00100111 << 23) | (n << 22) | (rn << 16) | (sh << 10) | (rn << 5) | rd);
    }

    /// CSEL/CSINC/CSINV/CSNEG: op2 picks (op, o2): (0,0) CSEL, (0,1) CSINC, (1,0) CSINV, (1,1) CSNEG.
    #[allow(clippy::too_many_arguments)]
    pub fn csel_kind(&mut self, x: bool, op: u32, o2: u32, rd: R, rn: R, rm: R, c: Cond) {
        self.emit(sf(x) | (op << 30) | (0b11010100 << 21) | (rm << 16) | ((c as u32) << 12) | (o2 << 10) | (rn << 5) | rd);
    }
    pub fn csel(&mut self, x: bool, rd: R, rn: R, rm: R, c: Cond) {
        self.csel_kind(x, 0, 0, rd, rn, rm, c);
    }
    /// CSET rd, c (CSINC rd, zr, zr, !c).
    pub fn cset(&mut self, x: bool, rd: R, c: Cond) {
        self.csel_kind(x, 0, 1, rd, ZR, ZR, c.invert());
    }

    // ------------------------------------------------------------------ floating point (doubles)

    /// FMOV Dd, Xn (bits from a general register).
    pub fn fmov_d_x(&mut self, dd: R, xn: R) {
        self.emit(0x9E67_0000 | (xn << 5) | dd);
    }
    /// FMOV Xd, Dn.
    pub fn fmov_x_d(&mut self, xd: R, dn: R) {
        self.emit(0x9E66_0000 | (dn << 5) | xd);
    }
    /// FADD/FSUB/FMUL/FDIV Dd, Dn, Dm (op: 0 mul, 1 div, 2 add, 3 sub).
    fn fp2(&mut self, op: u32, dd: R, dn: R, dm: R) {
        let opcode = match op {
            0 => 0b0000,
            1 => 0b0001,
            2 => 0b0010,
            _ => 0b0011,
        };
        self.emit(0x1E60_0800 | (dm << 16) | (opcode << 12) | (dn << 5) | dd);
    }
    pub fn fmul_d(&mut self, dd: R, dn: R, dm: R) {
        self.fp2(0, dd, dn, dm);
    }
    pub fn fdiv_d(&mut self, dd: R, dn: R, dm: R) {
        self.fp2(1, dd, dn, dm);
    }
    pub fn fadd_d(&mut self, dd: R, dn: R, dm: R) {
        self.fp2(2, dd, dn, dm);
    }
    pub fn fsub_d(&mut self, dd: R, dn: R, dm: R) {
        self.fp2(3, dd, dn, dm);
    }
    /// FMADD Dd, Dn, Dm, Da = Da + Dn * Dm (fused); FMSUB = Da - Dn * Dm;
    /// FNMSUB = Dn * Dm - Da.
    pub fn fmadd_d(&mut self, dd: R, dn: R, dm: R, da: R) {
        self.emit(0x1F40_0000 | (dm << 16) | (da << 10) | (dn << 5) | dd);
    }
    pub fn fmsub_d(&mut self, dd: R, dn: R, dm: R, da: R) {
        self.emit(0x1F40_8000 | (dm << 16) | (da << 10) | (dn << 5) | dd);
    }
    pub fn fnmsub_d(&mut self, dd: R, dn: R, dm: R, da: R) {
        self.emit(0x1F60_8000 | (dm << 16) | (da << 10) | (dn << 5) | dd);
    }
    pub fn fneg_d(&mut self, dd: R, dn: R) {
        self.emit(0x1E61_4000 | (dn << 5) | dd);
    }
    /// FMOV Sd, Wn.
    pub fn fmov_s_w(&mut self, sd: R, wn: R) {
        self.emit(0x1E27_0000 | (wn << 5) | sd);
    }
    /// FCVT Sd, Dn (round to single) and FCVT Dd, Sn (exact).
    pub fn fcvt_s_d(&mut self, sd: R, dn: R) {
        self.emit(0x1E62_4000 | (dn << 5) | sd);
    }
    pub fn fcvt_d_s(&mut self, dd: R, sn: R) {
        self.emit(0x1E22_C000 | (sn << 5) | dd);
    }
    /// LDR/STR Xt, [Xn, #off] for 64-bit fields with offsets beyond the scaled range: via a
    /// register offset.
    pub fn ldr_x_any(&mut self, rt: R, rn: R, off: u32, tmp: R) {
        if off % 8 == 0 && off / 8 < 4096 {
            self.ldr_x(rt, rn, off);
        } else {
            self.mov_imm(true, tmp, off as u64);
            self.ldr_x_reg(rt, rn, tmp);
        }
    }
    pub fn str_x_any(&mut self, rt: R, rn: R, off: u32, tmp: R) {
        if off % 8 == 0 && off / 8 < 4096 {
            self.str_x(rt, rn, off);
        } else {
            self.mov_imm(true, tmp, off as u64);
            self.str_x_reg(rt, rn, tmp);
        }
    }

    // ------------------------------------------------------------------ branches

    /// B / BL to a target instruction index (patched later if needed).
    pub fn b_to(&mut self, from: usize, to: usize, link: bool) -> u32 {
        let off = (to as i64 - from as i64) as i32;
        (if link { 0x9400_0000 } else { 0x1400_0000 }) | ((off as u32) & 0x03FF_FFFF)
    }
    pub fn b(&mut self, to: usize) {
        let w = self.b_to(self.here(), to, false);
        self.emit(w);
    }
    /// B.cond to an instruction index.
    pub fn b_cond_to(from: usize, to: usize, c: Cond) -> u32 {
        let off = (to as i64 - from as i64) as i32;
        0x5400_0000 | (((off as u32) & 0x7FFFF) << 5) | c as u32
    }
    /// CBZ/CBNZ to an instruction index.
    pub fn cbz_to(x: bool, nz: bool, rt: R, from: usize, to: usize) -> u32 {
        let off = (to as i64 - from as i64) as i32;
        sf(x) | (0b011010 << 25) | ((nz as u32) << 24) | (((off as u32) & 0x7FFFF) << 5) | rt
    }
    /// TBZ/TBNZ rt, #bit to an instruction index.
    pub fn tbz_to(nz: bool, rt: R, bit: u32, from: usize, to: usize) -> u32 {
        let off = (to as i64 - from as i64) as i32;
        ((bit >> 5) << 31) | (0b011011 << 25) | ((nz as u32) << 24) | ((bit & 31) << 19) | (((off as u32) & 0x3FFF) << 5) | rt
    }
    pub fn blr(&mut self, rn: R) {
        self.emit(0xD63F_0000 | (rn << 5));
    }
    pub fn br(&mut self, rn: R) {
        self.emit(0xD61F_0000 | (rn << 5));
    }
    pub fn ret(&mut self) {
        self.emit(0xD65F_03C0);
    }

    /// Replaces the instruction at `at` (for forward branches).
    pub fn patch(&mut self, at: usize, w: u32) {
        self.code[at] = w;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Assembles `text` with LLVM's assembler (when it is installed) and compares.
    fn check(asm: &Asm, text: &str) {
        let Ok(out) = std::process::Command::new("llvm-mc")
            .args(["-triple=aarch64", "-show-encoding"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .and_then(|mut c| {
                use std::io::Write;
                c.stdin.take().unwrap().write_all(text.as_bytes())?;
                c.wait_with_output()
            })
        else {
            return;
        };
        let s = String::from_utf8_lossy(&out.stdout);
        let mut words = Vec::new();
        for line in s.lines() {
            if let Some(p) = line.find("encoding: [") {
                let bytes: Vec<u8> = line[p + 11..].trim_end_matches(']').split(',').map(|b| u8::from_str_radix(b.trim().trim_start_matches("0x"), 16).unwrap()).collect();
                words.push(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]));
            }
        }
        assert_eq!(asm.code.len(), words.len(), "{text}\n{s}");
        for (k, (a, b)) in asm.code.iter().zip(words.iter()).enumerate() {
            assert_eq!(a, b, "instruction {k} of\n{text}: ours {a:08X}, LLVM {b:08X}");
        }
    }

    #[test]
    fn encodings_match_llvm() {
        let mut a = Asm::new();
        let mut t = String::new();
        let mut put = |a: &mut Asm, f: &dyn Fn(&mut Asm), s: &str| {
            f(a);
            t.push_str(s);
            t.push('\n');
        };
        put(&mut a, &|a| a.mov_imm(false, 3, 0x1234_5678), "movz w3, #0x5678\nmovk w3, #0x1234, lsl #16");
        put(&mut a, &|a| a.mov_imm(true, 9, 0xDEAD_0000_BEEF), "movz x9, #0xbeef\nmovk x9, #0xdead, lsl #32");
        put(&mut a, &|a| a.mov_imm(false, 4, 0), "movz w4, #0");
        put(&mut a, &|a| a.mov(true, 1, 2), "mov x1, x2");
        put(&mut a, &|a| a.ldr_w(5, 19, 0x40), "ldr w5, [x19, #0x40]");
        put(&mut a, &|a| a.str_x(5, 19, 0x48), "str x5, [x19, #0x48]");
        put(&mut a, &|a| a.ldrb(6, 7, 3), "ldrb w6, [x7, #3]");
        put(&mut a, &|a| a.ldr_w_reg(1, 20, 2), "ldr w1, [x20, x2]");
        put(&mut a, &|a| a.ldrh_reg(1, 20, 2), "ldrh w1, [x20, x2]");
        put(&mut a, &|a| a.strb_reg(1, 20, 2), "strb w1, [x20, x2]");
        put(&mut a, &|a| a.str_x_reg(1, 20, 2), "str x1, [x20, x2]");
        put(&mut a, &|a| a.ldr_x_reg_lsl3(5, 23, 4), "ldr x5, [x23, x4, lsl #3]");
        put(&mut a, &|a| a.stp_pre(29, 30, SP, -16), "stp x29, x30, [sp, #-16]!");
        put(&mut a, &|a| a.ldp_post(29, 30, SP, 16), "ldp x29, x30, [sp], #16");
        put(&mut a, &|a| a.ldp_off(9, 10, 7, 0), "ldp x9, x10, [x7]");
        put(&mut a, &|a| a.ldp_off(9, 10, 7, 16), "ldp x9, x10, [x7, #16]");
        put(&mut a, &|a| a.addsub_reg(true, false, false, 7, 7, 8, Shift::Lsl, 4), "add x7, x7, x8, lsl #4");
        put(&mut a, &|a| a.ubfx(true, 8, 6, 2, 16), "ubfx x8, x6, #2, #16");
        put(&mut a, &|a| a.add_imm(false, 1, 2, 4095), "add w1, w2, #4095");
        put(&mut a, &|a| a.sub_imm(true, 1, 2, 0x5000), "sub x1, x2, #5, lsl #12");
        put(&mut a, &|a| a.add(false, 1, 2, 3), "add w1, w2, w3");
        put(&mut a, &|a| a.adds(false, 1, 2, 3), "adds w1, w2, w3");
        put(&mut a, &|a| a.subs(false, 1, 2, 3), "subs w1, w2, w3");
        put(&mut a, &|a| a.cmp(false, 2, 3), "cmp w2, w3");
        put(&mut a, &|a| a.cmp_imm(false, 2, 7), "cmp w2, #7");
        put(&mut a, &|a| a.adc(false, false, true, 1, 2, 3), "adcs w1, w2, w3");
        put(&mut a, &|a| a.and(false, 1, 2, 3), "and w1, w2, w3");
        put(&mut a, &|a| a.orr(false, 1, 2, 3), "orr w1, w2, w3");
        put(&mut a, &|a| a.eor(false, 1, 2, 3), "eor w1, w2, w3");
        put(&mut a, &|a| a.bic(false, 1, 2, 3), "bic w1, w2, w3");
        put(&mut a, &|a| a.orn(false, 1, 2, 3), "orn w1, w2, w3");
        put(&mut a, &|a| a.eon(false, 1, 2, 3), "eon w1, w2, w3");
        put(&mut a, &|a| a.mvn(false, 1, 3), "mvn w1, w3");
        put(&mut a, &|a| a.tst(false, 2, 3), "tst w2, w3");
        put(&mut a, &|a| a.udiv(false, 1, 2, 3), "udiv w1, w2, w3");
        put(&mut a, &|a| a.sdiv(false, 1, 2, 3), "sdiv w1, w2, w3");
        put(&mut a, &|a| a.lslv(false, 1, 2, 3), "lsl w1, w2, w3");
        put(&mut a, &|a| a.lsrv(true, 1, 2, 3), "lsr x1, x2, x3");
        put(&mut a, &|a| a.asrv(false, 1, 2, 3), "asr w1, w2, w3");
        put(&mut a, &|a| a.rorv(false, 1, 2, 3), "ror w1, w2, w3");
        put(&mut a, &|a| a.clz(false, 1, 2), "clz w1, w2");
        put(&mut a, &|a| a.rev_w(1, 2), "rev w1, w2");
        put(&mut a, &|a| a.rev_x(1, 2), "rev x1, x2");
        put(&mut a, &|a| a.rev16_w(1, 2), "rev16 w1, w2");
        put(&mut a, &|a| a.mul(false, 1, 2, 3), "mul w1, w2, w3");
        put(&mut a, &|a| a.smull(1, 2, 3), "smull x1, w2, w3");
        put(&mut a, &|a| a.umull(1, 2, 3), "umull x1, w2, w3");
        put(&mut a, &|a| a.lsl_imm(false, 1, 2, 4), "lsl w1, w2, #4");
        put(&mut a, &|a| a.lsr_imm(true, 1, 2, 32), "lsr x1, x2, #32");
        put(&mut a, &|a| a.asr_imm(false, 1, 2, 31), "asr w1, w2, #31");
        put(&mut a, &|a| a.sxtb(false, 1, 2), "sxtb w1, w2");
        put(&mut a, &|a| a.sxth(false, 1, 2), "sxth w1, w2");
        put(&mut a, &|a| a.sxtw(1, 2), "sxtw x1, w2");
        put(&mut a, &|a| a.ubfx(false, 1, 2, 3, 4), "ubfx w1, w2, #3, #4");
        put(&mut a, &|a| a.bfi(false, 1, 2, 28, 4), "bfi w1, w2, #28, #4");
        put(&mut a, &|a| a.ror_imm(false, 1, 2, 5), "ror w1, w2, #5");
        put(&mut a, &|a| a.csel(false, 1, 2, 3, Cond::Lt), "csel w1, w2, w3, lt");
        put(&mut a, &|a| a.cset(false, 1, Cond::Gt), "cset w1, gt");
        put(&mut a, &|a| a.fmov_d_x(1, 9), "fmov d1, x9");
        put(&mut a, &|a| a.fmov_x_d(9, 1), "fmov x9, d1");
        put(&mut a, &|a| a.fadd_d(0, 1, 2), "fadd d0, d1, d2");
        put(&mut a, &|a| a.fsub_d(0, 1, 2), "fsub d0, d1, d2");
        put(&mut a, &|a| a.fmul_d(0, 1, 2), "fmul d0, d1, d2");
        put(&mut a, &|a| a.fdiv_d(0, 1, 2), "fdiv d0, d1, d2");
        put(&mut a, &|a| a.fmadd_d(0, 1, 2, 3), "fmadd d0, d1, d2, d3");
        put(&mut a, &|a| a.fmsub_d(0, 1, 2, 3), "fmsub d0, d1, d2, d3");
        put(&mut a, &|a| a.fnmsub_d(0, 1, 2, 3), "fnmsub d0, d1, d2, d3");
        put(&mut a, &|a| a.fneg_d(0, 1), "fneg d0, d1");
        put(&mut a, &|a| a.fcvt_s_d(0, 1), "fcvt s0, d1");
        put(&mut a, &|a| a.fmov_s_w(2, 7), "fmov s2, w7");
        put(&mut a, &|a| a.fcvt_d_s(0, 1), "fcvt d0, s1");
        put(&mut a, &|a| a.blr(16), "blr x16");
        put(&mut a, &|a| a.br(16), "br x16");
        put(&mut a, &|a| a.ret(), "ret");
        check(&a, &t);
    }
}
