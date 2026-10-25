//! Floating point (primary opcodes 59 and 63, the FP loads and stores) and Gekko's paired
//! singles (opcode 4 and the quantized loads and stores, opcodes 56-61).
//!
//! Registers hold raw IEEE double bits, and every NaN is handled explicitly instead of by the
//! host, because host CPUs pick and quiet NaNs differently from PowerPC.
//!
//! Gekko facts this follows (750CL user's manual, and measured behaviour widely documented):
//! - Double-precision instructions (opcode 63) and `lfd` write only ps0; single-precision
//!   instructions (opcode 59) and `lfs` write the same value to ps0 and ps1.
//! - Single-precision multiplies use frC rounded to 25 significant bits (the multiplier is
//!   narrower than a double's).
//! - fctiw/fctiwz leave 0xFFF80000 in the upper word.
//!
//! Known gaps, to close against the yardstick: `fres`/`frsqrte`/`ps_res`/`ps_rsqrte` return the
//! exact value, where hardware returns a table estimate; rounding modes other than
//! round-to-nearest are used by fctiw only; the inexact/overflow/underflow flags are approximate;
//! FP-enabled exceptions (MSR FE0/FE1) are not raised.

use super::interp::{ra, rb, rc, rd};
use super::*;

const QNAN: u64 = 0x7FF8_0000_0000_0000;
const QUIET_BIT: u64 = 0x0008_0000_0000_0000;

// FPSCR bits (IBM bit n is 1 << (31 - n)).
const FX: u32 = 1 << 31;
const FEX: u32 = 1 << 30;
const VX: u32 = 1 << 29;
const OX: u32 = 1 << 28;
const UX: u32 = 1 << 27;
const ZX: u32 = 1 << 26;
const XX: u32 = 1 << 25;
const VXSNAN: u32 = 1 << 24;
const VXISI: u32 = 1 << 23;
const VXIDI: u32 = 1 << 22;
const VXZDZ: u32 = 1 << 21;
const VXIMZ: u32 = 1 << 20;
const VXVC: u32 = 1 << 19;
const FR: u32 = 1 << 18;
const FI: u32 = 1 << 17;
const FPRF_SHIFT: u32 = 12;
const FPRF_MASK: u32 = 0x1F << FPRF_SHIFT;
const VXSOFT: u32 = 1 << 10;
const VXSQRT: u32 = 1 << 9;
const VXCVI: u32 = 1 << 8;
const VE: u32 = 1 << 7;
const NI: u32 = 1 << 2;
const VX_ALL: u32 = VXSNAN | VXISI | VXIDI | VXZDZ | VXIMZ | VXVC | VXSOFT | VXSQRT | VXCVI;

fn is_nan(b: u64) -> bool {
    b & 0x7FFF_FFFF_FFFF_FFFF > 0x7FF0_0000_0000_0000
}
fn is_snan(b: u64) -> bool {
    is_nan(b) && b & QUIET_BIT == 0
}
fn is_inf(b: u64) -> bool {
    b & 0x7FFF_FFFF_FFFF_FFFF == 0x7FF0_0000_0000_0000
}
fn is_zero(b: u64) -> bool {
    b & 0x7FFF_FFFF_FFFF_FFFF == 0
}

/// The FPRF class of a value.
pub fn fprf(b: u64) -> u32 {
    let neg = b >> 63 != 0;
    let exp = (b >> 52) & 0x7FF;
    let frac = b & 0x000F_FFFF_FFFF_FFFF;
    match (exp, frac == 0, neg) {
        (0x7FF, false, _) => 0x11,
        (0x7FF, true, true) => 0x09,
        (0x7FF, true, false) => 0x05,
        (0, true, true) => 0x12,
        (0, true, false) => 0x02,
        (0, false, true) => 0x18,
        (0, false, false) => 0x14,
        (_, _, true) => 0x08,
        (_, _, false) => 0x04,
    }
}

/// lfs: a single's bits as a double, exactly (signalling NaNs stay signalling).
pub fn single_to_double(w: u32) -> u64 {
    let sign = ((w >> 31) as u64) << 63;
    let exp = (w >> 23) & 0xFF;
    let frac = (w & 0x7F_FFFF) as u64;
    match exp {
        0xFF => sign | 0x7FF0_0000_0000_0000 | (frac << 29),
        0 if frac == 0 => sign,
        0 => {
            // Denormal single: normalise it into a double.
            let shift = frac.leading_zeros() - 40; // places to bring the top bit to bit 23
            let f = (frac << shift) & 0x7F_FFFF;
            let e = 1023 - 126 - shift as u64;
            sign | (e << 52) | (f << 29)
        }
        _ => sign | (((exp as u64) + 1023 - 127) << 52) | (frac << 29),
    }
}

/// stfs: a double's bits as a single, by the PowerPC "store single" rule (truncation, no
/// rounding; out-of-range doubles give the hardware's bit selection).
pub fn double_to_single(d: u64) -> u32 {
    let exp = ((d >> 52) & 0x7FF) as i64;
    if exp > 896 || exp == 0 && d & 0x000F_FFFF_FFFF_FFFF == 0 || exp == 0x7FF {
        (((d >> 32) & 0xC000_0000) | ((d >> 29) & 0x3FFF_FFFF)) as u32
    } else if exp >= 874 {
        // Denormalise: shift the significand (with its hidden bit) right until the exponent is -126.
        let sign = ((d >> 63) as u32) << 31;
        let mut frac = (d & 0x000F_FFFF_FFFF_FFFF) | 0x0010_0000_0000_0000;
        let mut e = exp - 1023;
        while e < -126 {
            frac >>= 1;
            e += 1;
        }
        sign | ((frac >> 29) as u32 & 0x7F_FFFF)
    } else {
        ((d >> 32) & 0x8000_0000) as u32
    }
}

/// Rounds a double to the nearest single (frsp and every single-precision result).
pub fn round_single(b: u64) -> u64 {
    if is_nan(b) {
        return (b | QUIET_BIT) & 0xFFFF_FFFF_E000_0000;
    }
    ((f64::from_bits(b) as f32) as f64).to_bits()
}

/// frC as the single-precision multiplier sees it: rounded to 25 significant bits.
pub fn round_c25(b: u64) -> u64 {
    if is_nan(b) || is_inf(b) {
        return b;
    }
    
    (b & 0xFFFF_FFFF_F800_0000).wrapping_add(b & 0x0800_0000)
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Op {
    Add,
    Sub,
    Mul,
    Div,
    /// a * c + b, with the sign of the result negated afterwards when `neg`.
    Madd { sub: bool, neg: bool },
}

impl Cpu {
    pub(crate) fn fp_enabled(&mut self) -> bool {
        if self.msr & MSR_FP == 0 {
            self.fault(Exception::FpUnavailable, 0);
            return false;
        }
        true
    }

    fn fpscr_exc(&mut self, bits: u32) {
        if bits & !self.fpscr != 0 {
            self.fpscr |= FX;
        }
        self.fpscr |= bits;
        self.fpscr_summary();
    }

    fn fpscr_summary(&mut self) {
        if self.fpscr & VX_ALL != 0 {
            self.fpscr |= VX;
        } else {
            self.fpscr &= !VX;
        }
        // FEX: an exception summary bit (VX, OX, UX, ZX, XX: IBM bits 2-6) whose enable bit
        // (VE, OE, UE, ZE, XE: IBM bits 24-28) is set. Each enable sits 22 places below its bit.
        let exc = self.fpscr & (VX | OX | UX | ZX | XX);
        let enabled = (self.fpscr << 22) & (VX | OX | UX | ZX | XX);
        if exc & enabled != 0 {
            self.fpscr |= FEX;
        } else {
            self.fpscr &= !FEX;
        }
    }

    #[inline]
    pub(crate) fn set_fprf(&mut self, b: u64) {
        self.fpscr = (self.fpscr & !FPRF_MASK) | (fprf(b) << FPRF_SHIFT);
    }

    #[inline]
    pub(crate) fn flush_denormal(&self, b: u64) -> u64 {
        if self.fpscr & NI != 0 && (b >> 52) & 0x7FF == 0 {
            b & 0x8000_0000_0000_0000
        } else {
            b
        }
    }

    /// One arithmetic operation on raw doubles, with PowerPC's NaN rules and invalid flags.
    /// None: an invalid operation with VE set (the target is left alone).
    fn farith(&mut self, op: Op, a: u64, b: u64, c: u64, single: bool) -> Option<u64> {
        self.farith_impl(op, a, b, c, single, true)
    }

    /// The common case of `farith` on its own: every operand that takes part is an ordinary
    /// number (not infinite or NaN), and a division's divisor is not zero. Then no NaN rule,
    /// invalid case or zero-divide can apply, and only the overflow flag and the flush of
    /// denormals remain: exactly what the full path does for these inputs. None (and nothing
    /// changed) for any other inputs. The fast interpreter's own handlers call it directly.
    #[inline(always)]
    pub(crate) fn farith_quick(&mut self, op: Op, a: u64, b: u64, c: u64, single: bool) -> Option<u64> {
        let fin = |x: u64| (x >> 52) & 0x7FF != 0x7FF;
        // frC as the multiplier sees it (single precision rounds it first, which can carry it
        // up to infinity).
        let c = if single && matches!(op, Op::Mul | Op::Madd { .. }) { round_c25(c) } else { c };
        let quick = match op {
            Op::Add | Op::Sub => fin(a) && fin(b),
            Op::Mul => fin(a) && fin(c),
            Op::Div => fin(a) && fin(b) && !is_zero(b),
            Op::Madd { .. } => fin(a) && fin(b) && fin(c),
        };
        if !quick {
            return None;
        }
        let (fa, fb, fc) = (f64::from_bits(a), f64::from_bits(b), f64::from_bits(c));
        let r = match op {
            Op::Add => fa + fb,
            Op::Sub => fa - fb,
            Op::Mul => fa * fc,
            Op::Div => fa / fb,
            Op::Madd { sub, .. } => fa.mul_add(fc, if sub { -fb } else { fb }),
        };
        let mut bits = r.to_bits();
        if let Op::Madd { neg: true, .. } = op {
            bits ^= 1 << 63;
        }
        if single {
            bits = round_single(bits);
        }
        if is_inf(bits) {
            self.fpscr_exc(OX | XX);
        }
        Some(self.flush_denormal(bits))
    }

    fn farith_impl(&mut self, op: Op, a: u64, b: u64, c: u64, single: bool, allow_quick: bool) -> Option<u64> {
        if allow_quick {
            if let Some(r) = self.farith_quick(op, a, b, c, single) {
                return Some(r);
            }
        }
        let c = if single && matches!(op, Op::Mul | Op::Madd { .. }) { round_c25(c) } else { c };
        let used: &[u64] = match op {
            Op::Mul => &[a, c],
            Op::Madd { .. } => &[a, b, c],
            _ => &[a, b],
        };
        if used.iter().any(|&x| is_snan(x)) {
            self.fpscr_exc(VXSNAN);
        }
        // PowerPC NaN precedence: frA, then frB, then frC.
        let nan = [a, b, c]
            .into_iter()
            .zip([true, !matches!(op, Op::Mul), matches!(op, Op::Mul | Op::Madd { .. })])
            .find(|&(x, used)| used && is_nan(x))
            .map(|(x, _)| x | QUIET_BIT);
        if let Some(n) = nan {
            if self.fpscr & VE != 0 && used.iter().any(|&x| is_snan(x)) {
                return None;
            }
            let n = if single { round_single(n) } else { n };
            return Some(n);
        }
        let (fa, fb, fc) = (f64::from_bits(a), f64::from_bits(b), f64::from_bits(c));
        let mut invalid = 0;
        let r = match op {
            Op::Add => {
                if is_inf(a) && is_inf(b) && (a ^ b) >> 63 != 0 {
                    invalid = VXISI;
                }
                fa + fb
            }
            Op::Sub => {
                if is_inf(a) && is_inf(b) && (a ^ b) >> 63 == 0 {
                    invalid = VXISI;
                }
                fa - fb
            }
            Op::Mul => {
                if (is_inf(a) && is_zero(c)) || (is_zero(a) && is_inf(c)) {
                    invalid = VXIMZ;
                }
                fa * fc
            }
            Op::Div => {
                if is_inf(a) && is_inf(b) {
                    invalid = VXIDI;
                } else if is_zero(a) && is_zero(b) {
                    invalid = VXZDZ;
                } else if is_zero(b) {
                    self.fpscr_exc(ZX);
                }
                fa / fb
            }
            Op::Madd { sub, .. } => {
                if (is_inf(a) && is_zero(c)) || (is_zero(a) && is_inf(c)) {
                    invalid = VXIMZ;
                } else {
                    let prod = fa * fc;
                    if prod.is_infinite() && is_inf(b) && ((prod.to_bits() ^ b) >> 63 != 0) != sub {
                        invalid = VXISI;
                    }
                }
                fa.mul_add(fc, if sub { -fb } else { fb })
            }
        };
        if invalid != 0 {
            self.fpscr_exc(invalid);
            if self.fpscr & VE != 0 {
                return None;
            }
            return Some(QNAN);
        }
        let mut bits = r.to_bits();
        if let Op::Madd { neg: true, .. } = op {
            if !r.is_nan() {
                bits ^= 1 << 63;
            }
        }
        if single {
            bits = round_single(bits);
        }
        let finite_in = used.iter().all(|&x| !is_inf(x));
        if is_inf(bits) && finite_in {
            self.fpscr_exc(OX | XX);
        }
        Some(self.flush_denormal(bits))
    }

    #[inline]
    pub(crate) fn rec_cr1(&mut self, i: u32) {
        if i & 1 != 0 {
            let v = self.fpscr >> 28;
            self.set_cr_field(1, v);
        }
    }

    // ------------------------------------------------------------------ loads and stores

    pub(crate) fn fp_load_store<B: Bus>(&mut self, op: u32, d: usize, ea: u32, bus: &mut B) -> Option<()> {
        match op {
            48 | 49 => {
                let w = self.read(ea, 4, bus)? as u32;
                let v = single_to_double(w);
                self.ps0[d] = v;
                self.ps1[d] = v;
            }
            50 | 51 => self.ps0[d] = self.read(ea, 8, bus)?,
            52 | 53 => self.write(ea, 4, double_to_single(self.ps0[d]) as u64, bus)?,
            54 | 55 => self.write(ea, 8, self.ps0[d], bus)?,
            _ => unreachable!(),
        }
        Some(())
    }

    pub(crate) fn fp_load_store_x<B: Bus>(&mut self, i: u32, xo: u32, bus: &mut B) {
        if !self.fp_enabled() {
            return;
        }
        let (d, a, b) = (rd(i), ra(i), rb(i));
        let update = matches!(xo, 567 | 631 | 695 | 759);
        let base = if update { self.gpr[a] } else if a == 0 { 0 } else { self.gpr[a] };
        let ea = base.wrapping_add(self.gpr[b]);
        let ok = match xo {
            535 | 567 => self.fp_load_store(48, d, ea, bus),
            599 | 631 => self.fp_load_store(50, d, ea, bus),
            663 | 695 => self.fp_load_store(52, d, ea, bus),
            727 | 759 => self.fp_load_store(54, d, ea, bus),
            // stfiwx: the low word of the register, as is.
            983 => self.write(ea, 4, self.ps0[d] & 0xFFFF_FFFF, bus),
            _ => unreachable!(),
        };
        if ok.is_some() && update {
            self.gpr[a] = ea;
        }
    }

    // ------------------------------------------------------------------ quantized loads and stores

    #[inline]
    pub(crate) fn dequantize(&self, raw: u32, ty: u32, scale: u32) -> u64 {
        let s = ((scale as i32) << 26) >> 26;
        let v = match ty {
            4 => raw as u8 as f32,
            5 => raw as u16 as f32,
            6 => raw as u8 as i8 as f32,
            7 => raw as u16 as i16 as f32,
            _ => return single_to_double(raw),
        };
        ((v * 2f32.powi(-s)) as f64).to_bits()
    }

    #[inline]
    pub(crate) fn quantize(&self, d: u64, ty: u32, scale: u32) -> u32 {
        let s = ((scale as i32) << 26) >> 26;
        if !matches!(ty, 4..=7) {
            return double_to_single(d);
        }
        let v = f64::from_bits(d) as f32 * 2f32.powi(s);
        let (lo, hi) = match ty {
            4 => (0.0, 255.0),
            5 => (0.0, 65535.0),
            6 => (-128.0, 127.0),
            _ => (-32768.0, 32767.0),
        };
        let v = if v.is_nan() { 0.0 } else { v.clamp(lo, hi) };
        let n = v as i32;
        match ty {
            4 | 6 => n as u32 & 0xFF,
            _ => n as u32 & 0xFFFF,
        }
    }

    fn psq_load<B: Bus>(&mut self, d: usize, ea: u32, w: bool, gqr: u32, bus: &mut B) -> Option<()> {
        let ty = (gqr >> 16) & 7;
        let scale = (gqr >> 24) & 0x3F;
        let size = match ty {
            4 | 6 => 1,
            5 | 7 => 2,
            _ => 4,
        };
        let a = self.read(ea, size, bus)? as u32;
        let b = if w { None } else { Some(self.read(ea.wrapping_add(size), size, bus)? as u32) };
        self.ps0[d] = self.dequantize(a, ty, scale);
        self.ps1[d] = match b {
            Some(b) => self.dequantize(b, ty, scale),
            None => 1.0f64.to_bits(),
        };
        Some(())
    }

    fn psq_store<B: Bus>(&mut self, s: usize, ea: u32, w: bool, gqr: u32, bus: &mut B) -> Option<()> {
        let ty = gqr & 7;
        let scale = (gqr >> 8) & 0x3F;
        let size = match ty {
            4 | 6 => 1,
            5 | 7 => 2,
            _ => 4,
        };
        let a = self.quantize(self.ps0[s], ty, scale);
        if w {
            self.write(ea, size, a as u64, bus)
        } else {
            let b = self.quantize(self.ps1[s], ty, scale);
            // Both halves in one access, so a fault leaves memory untouched.
            let both = ((a as u64) << (8 * size)) | b as u64;
            self.write(ea, size * 2, both, bus)
        }
    }

    fn ps_enabled(&mut self) -> bool {
        if !self.fp_enabled() {
            return false;
        }
        if self.hid2 & HID2_PSE == 0 {
            self.illegal();
            return false;
        }
        true
    }

    pub(crate) fn psq_d<B: Bus>(&mut self, i: u32, bus: &mut B) {
        if !self.ps_enabled() {
            return;
        }
        let op = i >> 26;
        let (d, a) = (rd(i), ra(i));
        let update = op == 57 || op == 61;
        let disp = ((i & 0xFFF) << 20) as i32 >> 20;
        let w = (i >> 15) & 1 != 0;
        let gqr = self.gqr[((i >> 12) & 7) as usize];
        let base = if update || a != 0 { self.gpr[a] } else { 0 };
        let ea = base.wrapping_add(disp as u32);
        let ok = if op == 56 || op == 57 { self.psq_load(d, ea, w, gqr, bus) } else { self.psq_store(d, ea, w, gqr, bus) };
        if ok.is_some() && update {
            self.gpr[a] = ea;
        }
    }

    // ------------------------------------------------------------------ opcode 4: paired singles

    pub(crate) fn op4<B: Bus>(&mut self, i: u32, bus: &mut B) {
        let (d, a, b, c) = (rd(i), ra(i), rb(i), rc(i));
        let xo10 = (i >> 1) & 0x3FF;
        if xo10 == 1014 {
            // dcbz_l: zero a line of the locked cache.
            let ea = (if a == 0 { 0 } else { self.gpr[a] }).wrapping_add(self.gpr[b]) & !31;
            let off = (ea.wrapping_sub(LOCKED_CACHE_BASE)) as usize & (LOCKED_CACHE_SIZE - 1);
            self.locked_cache[off..off + 32].fill(0);
            return;
        }
        if !self.ps_enabled() {
            return;
        }
        let xo6 = (i >> 1) & 0x3F;
        if matches!(xo6, 6 | 7 | 38 | 39) {
            let update = xo6 >= 38;
            let base = if update || a != 0 { self.gpr[a] } else { 0 };
            let ea = base.wrapping_add(self.gpr[b]);
            let w = (i >> 10) & 1 != 0;
            let gqr = self.gqr[((i >> 7) & 7) as usize];
            let ok = if xo6 & 1 == 0 { self.psq_load(d, ea, w, gqr, bus) } else { self.psq_store(d, ea, w, gqr, bus) };
            if ok.is_some() && update {
                self.gpr[a] = ea;
            }
            return;
        }
        let (a0, a1, b0, b1, c0, c1) = (self.ps0[a], self.ps1[a], self.ps0[b], self.ps1[b], self.ps0[c], self.ps1[c]);
        let two = |cpu: &mut Cpu, op: Op, x: (u64, u64, u64), y: (u64, u64, u64)| -> Option<(u64, u64)> {
            let r0 = cpu.farith(op, x.0, x.1, x.2, true)?;
            let r1 = cpu.farith(op, y.0, y.1, y.2, true)?;
            Some((r0, r1))
        };
        let result: Option<(u64, u64)> = match (i >> 1) & 0x1F {
            10 => self.farith(Op::Add, a0, b1, 0, true).map(|r| (r, round_single(c1))),
            11 => self.farith(Op::Add, a0, b1, 0, true).map(|r| (round_single(c0), r)),
            12 => two(self, Op::Mul, (a0, 0, c0), (a1, 0, c0)),
            13 => two(self, Op::Mul, (a0, 0, c1), (a1, 0, c1)),
            14 => two(self, Op::Madd { sub: false, neg: false }, (a0, b0, c0), (a1, b1, c0)),
            15 => two(self, Op::Madd { sub: false, neg: false }, (a0, b0, c1), (a1, b1, c1)),
            18 => two(self, Op::Div, (a0, b0, 0), (a1, b1, 0)),
            20 => two(self, Op::Sub, (a0, b0, 0), (a1, b1, 0)),
            21 => two(self, Op::Add, (a0, b0, 0), (a1, b1, 0)),
            23 => {
                let sel = |x: u64, yes: u64, no: u64| if !is_nan(x) && (f64::from_bits(x) >= 0.0) { yes } else { no };
                Some((sel(a0, c0, b0), sel(a1, c1, b1)))
            }
            24 => Some((self.estimate_recip(b0, true), self.estimate_recip(b1, true))),
            25 => two(self, Op::Mul, (a0, 0, c0), (a1, 0, c1)),
            26 => Some((self.estimate_rsqrt(b0, true), self.estimate_rsqrt(b1, true))),
            28 => two(self, Op::Madd { sub: true, neg: false }, (a0, b0, c0), (a1, b1, c1)),
            29 => two(self, Op::Madd { sub: false, neg: false }, (a0, b0, c0), (a1, b1, c1)),
            30 => two(self, Op::Madd { sub: true, neg: true }, (a0, b0, c0), (a1, b1, c1)),
            31 => two(self, Op::Madd { sub: false, neg: true }, (a0, b0, c0), (a1, b1, c1)),
            _ => {
                let sign = 1u64 << 63;
                match xo10 {
                    0 | 32 => return self.fcmp(i, a0, b0, xo10 == 32),
                    64 | 96 => return self.fcmp(i, a1, b1, xo10 == 96),
                    40 => Some((b0 ^ sign, b1 ^ sign)),
                    72 => Some((b0, b1)),
                    136 => Some((b0 | sign, b1 | sign)),
                    264 => Some((b0 & !sign, b1 & !sign)),
                    528 => Some((round_single(a0), b0)),
                    560 => Some((round_single(a0), b1)),
                    592 => Some((a1, b0)),
                    624 => Some((a1, b1)),
                    _ => return self.illegal(),
                }
            }
        };
        if let Some((r0, r1)) = result {
            self.ps0[d] = r0;
            self.ps1[d] = r1;
            if !matches!(xo10, 40 | 72 | 136 | 264 | 528 | 560 | 592 | 624) || (i >> 1) & 0x1F == 23 {
                self.set_fprf(r0);
            }
        }
        self.rec_cr1(i);
    }

    fn estimate_recip(&mut self, b: u64, single: bool) -> u64 {
        if is_snan(b) {
            self.fpscr_exc(VXSNAN);
        }
        if is_nan(b) {
            return b | QUIET_BIT;
        }
        if is_zero(b) {
            self.fpscr_exc(ZX);
        }
        let r = (1.0 / f64::from_bits(b)).to_bits();
        if single {
            round_single(r)
        } else {
            r
        }
    }

    fn estimate_rsqrt(&mut self, b: u64, single: bool) -> u64 {
        if is_snan(b) {
            self.fpscr_exc(VXSNAN);
        }
        if is_nan(b) {
            return b | QUIET_BIT;
        }
        if is_zero(b) {
            self.fpscr_exc(ZX);
            return (b & (1 << 63)) | 0x7FF0_0000_0000_0000;
        }
        if b >> 63 != 0 {
            self.fpscr_exc(VXSQRT);
            return QNAN;
        }
        let r = (1.0 / f64::from_bits(b).sqrt()).to_bits();
        if single {
            round_single(r)
        } else {
            r
        }
    }

    pub(crate) fn fcmp(&mut self, i: u32, a: u64, b: u64, ordered: bool) {
        let (fa, fb) = (f64::from_bits(a), f64::from_bits(b));
        let v = if is_nan(a) || is_nan(b) {
            1
        } else if fa < fb {
            8
        } else if fa > fb {
            4
        } else {
            2
        };
        let crf = (rd(i) >> 2) as u32;
        self.set_cr_field(crf, v);
        self.fpscr = (self.fpscr & !(0xF << FPRF_SHIFT)) | (v << FPRF_SHIFT);
        if is_snan(a) || is_snan(b) {
            self.fpscr_exc(VXSNAN);
            if ordered && self.fpscr & VE == 0 {
                self.fpscr_exc(VXVC);
            }
        } else if ordered && (is_nan(a) || is_nan(b)) {
            self.fpscr_exc(VXVC);
        }
    }

    // ------------------------------------------------------------------ opcodes 59 and 63

    pub(crate) fn op59(&mut self, i: u32) {
        if !self.fp_enabled() {
            return;
        }
        let (d, a, b, c) = (rd(i), ra(i), rb(i), rc(i));
        let (fa, fb, fc) = (self.ps0[a], self.ps0[b], self.ps0[c]);
        let r = match (i >> 1) & 0x1F {
            18 => self.farith(Op::Div, fa, fb, 0, true),
            20 => self.farith(Op::Sub, fa, fb, 0, true),
            21 => self.farith(Op::Add, fa, fb, 0, true),
            24 => Some(self.estimate_recip(fb, true)),
            25 => self.farith(Op::Mul, fa, 0, fc, true),
            28 => self.farith(Op::Madd { sub: true, neg: false }, fa, fb, fc, true),
            29 => self.farith(Op::Madd { sub: false, neg: false }, fa, fb, fc, true),
            30 => self.farith(Op::Madd { sub: true, neg: true }, fa, fb, fc, true),
            31 => self.farith(Op::Madd { sub: false, neg: true }, fa, fb, fc, true),
            _ => return self.illegal(),
        };
        if let Some(r) = r {
            self.ps0[d] = r;
            self.ps1[d] = r;
            self.set_fprf(r);
        }
        self.rec_cr1(i);
    }

    pub(crate) fn op63(&mut self, i: u32) {
        if !self.fp_enabled() {
            return;
        }
        let (d, a, b, c) = (rd(i), ra(i), rb(i), rc(i));
        let (fa, fb, fc) = (self.ps0[a], self.ps0[b], self.ps0[c]);
        let sign = 1u64 << 63;
        let xo5 = (i >> 1) & 0x1F;
        let r = match xo5 {
            18 => self.farith(Op::Div, fa, fb, 0, false),
            20 => self.farith(Op::Sub, fa, fb, 0, false),
            21 => self.farith(Op::Add, fa, fb, 0, false),
            23 => Some(if !is_nan(fa) && f64::from_bits(fa) >= 0.0 { fc } else { fb }),
            25 => self.farith(Op::Mul, fa, 0, fc, false),
            26 => Some(self.estimate_rsqrt(fb, false)),
            28 => self.farith(Op::Madd { sub: true, neg: false }, fa, fb, fc, false),
            29 => self.farith(Op::Madd { sub: false, neg: false }, fa, fb, fc, false),
            30 => self.farith(Op::Madd { sub: true, neg: true }, fa, fb, fc, false),
            31 => self.farith(Op::Madd { sub: false, neg: true }, fa, fb, fc, false),
            _ => match (i >> 1) & 0x3FF {
                0 | 32 => return self.fcmp(i, fa, fb, (i >> 1) & 0x3FF == 32),
                12 => {
                    if is_snan(fb) {
                        self.fpscr_exc(VXSNAN);
                    }
                    let r = round_single(fb);
                    Some(self.flush_denormal(r))
                }
                14 | 15 => Some(self.fctiw(fb, (i >> 1) & 0x3FF == 15)),
                40 => {
                    self.ps0[d] = fb ^ sign;
                    return self.rec_cr1(i);
                }
                72 => {
                    self.ps0[d] = fb;
                    return self.rec_cr1(i);
                }
                136 => {
                    self.ps0[d] = fb | sign;
                    return self.rec_cr1(i);
                }
                264 => {
                    self.ps0[d] = fb & !sign;
                    return self.rec_cr1(i);
                }
                38 => {
                    let bit = 31 - ((i >> 21) & 31);
                    self.fpscr_exc(1 << bit);
                    return self.rec_cr1(i);
                }
                70 => {
                    let bit = 31 - ((i >> 21) & 31);
                    self.fpscr &= !(1 << bit);
                    self.fpscr_summary();
                    return self.rec_cr1(i);
                }
                64 => {
                    // mcrfs: copy a field, then clear its exception bits.
                    let src = (a >> 2) as u32;
                    let shift = 28 - 4 * src;
                    let v = (self.fpscr >> shift) & 0xF;
                    self.set_cr_field((d >> 2) as u32, v);
                    let clearable = 0x9FF8_0700u32;
                    self.fpscr &= !((0xF << shift) & clearable);
                    self.fpscr_summary();
                    return;
                }
                134 => {
                    let field = (d >> 2) as u32;
                    let imm = (i >> 12) & 0xF;
                    let shift = 28 - 4 * field;
                    self.fpscr = (self.fpscr & !(0xF << shift)) | (imm << shift);
                    self.fpscr_summary();
                    return self.rec_cr1(i);
                }
                583 => {
                    self.ps0[d] = 0xFFF8_0000_0000_0000 | self.fpscr as u64;
                    return self.rec_cr1(i);
                }
                711 => {
                    let fm = (i >> 17) & 0xFF;
                    let mut mask = 0u32;
                    for f in 0..8 {
                        if fm & (0x80 >> f) != 0 {
                            mask |= 0xF000_0000 >> (4 * f);
                        }
                    }
                    // FEX and VX are computed, never written.
                    mask &= !(FEX | VX);
                    self.fpscr = (self.fpscr & !mask) | (fb as u32 & mask);
                    self.fpscr_summary();
                    return self.rec_cr1(i);
                }
                _ => return self.illegal(),
            },
        };
        if let Some(r) = r {
            self.ps0[d] = r;
            if xo5 != 23 && !matches!((i >> 1) & 0x3FF, 14 | 15) {
                self.set_fprf(r);
            }
        }
        self.rec_cr1(i);
    }

    fn fctiw(&mut self, b: u64, toward_zero: bool) -> u64 {
        const UPPER: u64 = 0xFFF8_0000_0000_0000;
        let fb = f64::from_bits(b);
        if is_nan(b) {
            let mut bits = VXCVI;
            if is_snan(b) {
                bits |= VXSNAN;
            }
            self.fpscr_exc(bits);
            return UPPER | 0x8000_0000;
        }
        let rounded = if toward_zero {
            fb.trunc()
        } else {
            match self.fpscr & 3 {
                0 => {
                    // Round to nearest, ties to even.
                    let r = fb.round();
                    if (fb - fb.trunc()).abs() == 0.5 && r % 2.0 != 0.0 {
                        r - fb.signum()
                    } else {
                        r
                    }
                }
                1 => fb.trunc(),
                2 => fb.ceil(),
                _ => fb.floor(),
            }
        };
        let v = if rounded > i32::MAX as f64 {
            self.fpscr_exc(VXCVI);
            i32::MAX
        } else if rounded < i32::MIN as f64 {
            self.fpscr_exc(VXCVI);
            i32::MIN
        } else {
            if rounded != fb {
                self.fpscr |= XX | FI;
            }
            rounded as i32
        };
        let _ = (FR, UX);
        UPPER | v as u32 as u64
    }
}

#[cfg(test)]
mod quick_path_tests {
    use super::*;

    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
    }

    /// Doubles of every kind the quick path must treat exactly like the full path: ordinary,
    /// tiny, denormal, huge (near overflow, and whose 25-bit rounding carries), zero, infinite,
    /// NaN.
    fn value(r: &mut Rng) -> u64 {
        let sign = (r.next() & 1) << 63;
        let frac = r.next() & 0x000F_FFFF_FFFF_FFFF;
        match r.next() % 10 {
            0 => sign,
            1 => sign | 0x7FF0_0000_0000_0000,
            2 => sign | 0x7FF0_0000_0000_0000 | (frac | 1),
            3 => sign | frac,
            4 => sign | (0x7FE << 52) | 0x000F_FFFF_F800_0000 | (frac & 0x7FF_FFFF),
            5 => sign | ((0x47E + (r.next() % 4)) << 52) | frac,
            6 => sign | ((0x36A + (r.next() % 8)) << 52) | frac,
            7 => (((r.next() as u32 as f32) as f64) * if sign != 0 { -1e-3 } else { 1e3 }).to_bits(),
            _ => sign | ((0x380 + r.next() % 0x100) << 52) | frac,
        }
    }

    #[test]
    fn quick_path_matches_the_full_path() {
        let mut r = Rng(0xF00D_1234_5678_9ABC);
        let ops = [Op::Add, Op::Sub, Op::Mul, Op::Div, Op::Madd { sub: false, neg: false }, Op::Madd { sub: true, neg: false }, Op::Madd { sub: false, neg: true }, Op::Madd { sub: true, neg: true }];
        for n in 0..4_000_000u32 {
            let op = ops[(r.next() % ops.len() as u64) as usize];
            let (a, b, c) = (value(&mut r), value(&mut r), value(&mut r));
            let single = r.next() & 1 != 0;
            let mut p = Cpu::default();
            p.fpscr = (r.next() as u32) & !(VE | 0x8000_0000);
            if r.next() % 8 == 0 {
                p.fpscr |= VE;
            }
            let mut q = p.clone();
            let x = p.farith_impl(op, a, b, c, single, false);
            let y = q.farith_impl(op, a, b, c, single, true);
            assert!(x == y && p.fpscr == q.fpscr, "case {n}: {a:016X} {b:016X} {c:016X} single {single}: full {x:016X?} fpscr {:08X}, quick {y:016X?} fpscr {:08X}", p.fpscr, q.fpscr);
        }
    }
}
