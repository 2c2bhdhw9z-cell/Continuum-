//! The GameCube's CPU (IBM "Gekko", a PowerPC 750CXe with paired singles), and the Wii's
//! (IBM "Broadway", the same core at a higher clock). Written from IBM's published PowerPC
//! architecture books and the 750CL user's manual.
//!
//! This is the PLAIN interpreter: one instruction at a time, written to be obviously right. It
//! is the yardstick the fast interpreter and the JIT are proved against (gamecube.json, G7, G8),
//! so clarity wins over speed here.
//!
//! Layout:
//! - `mod.rs`: the registers, exceptions, special registers, address translation, memory access.
//! - `interp.rs`: decoding and the integer, branch, load/store and system instructions.
//! - `fpu.rs`: floating point and paired singles.

use crate::bus::Bus;
use serde::{Deserialize, Serialize};

mod fpu;
pub use fpu::{double_to_single, round_single, single_to_double};
pub mod fast;
mod fast_ops;
mod interp;
pub mod mmu;

// MSR bits (numbered from the least significant bit, 0 = LE).
pub const MSR_LE: u32 = 1 << 0;
pub const MSR_RI: u32 = 1 << 1;
pub const MSR_DR: u32 = 1 << 4;
pub const MSR_IR: u32 = 1 << 5;
pub const MSR_IP: u32 = 1 << 6;
pub const MSR_FE1: u32 = 1 << 8;
pub const MSR_SE: u32 = 1 << 10;
pub const MSR_FE0: u32 = 1 << 11;
pub const MSR_ME: u32 = 1 << 12;
pub const MSR_FP: u32 = 1 << 13;
pub const MSR_PR: u32 = 1 << 14;
pub const MSR_EE: u32 = 1 << 15;
pub const MSR_ILE: u32 = 1 << 16;
pub const MSR_POW: u32 = 1 << 18;

/// The MSR bits an exception saves into SRR1 and `rfi` restores (IBM bits 0, 5-9 and 16-31).
pub const MSR_SAVED: u32 = 0x87C0_FFFF;

// XER bits.
pub const XER_SO: u32 = 1 << 31;
pub const XER_OV: u32 = 1 << 30;
pub const XER_CA: u32 = 1 << 29;

// Special-purpose register numbers.
pub mod spr {
    pub const XER: u32 = 1;
    pub const LR: u32 = 8;
    pub const CTR: u32 = 9;
    pub const DSISR: u32 = 18;
    pub const DAR: u32 = 19;
    pub const DEC: u32 = 22;
    pub const SDR1: u32 = 25;
    pub const SRR0: u32 = 26;
    pub const SRR1: u32 = 27;
    pub const TBL_READ: u32 = 268;
    pub const TBU_READ: u32 = 269;
    pub const SPRG0: u32 = 272;
    pub const EAR: u32 = 282;
    pub const TBL: u32 = 284;
    pub const TBU: u32 = 285;
    pub const PVR: u32 = 287;
    pub const IBAT0U: u32 = 528;
    pub const DBAT0U: u32 = 536;
    /// Broadway's four extra BAT pairs (IBAT4U = 560 .. DBAT7L = 575).
    pub const IBAT4U: u32 = 560;
    pub const DBAT4U: u32 = 568;
    pub const GQR0: u32 = 912;
    pub const HID2: u32 = 920;
    pub const WPAR: u32 = 921;
    pub const DMAU: u32 = 922;
    pub const DMAL: u32 = 923;
    pub const HID0: u32 = 1008;
    pub const HID1: u32 = 1009;
    pub const HID4: u32 = 1011;
    pub const L2CR: u32 = 1017;
    pub const THRM1: u32 = 1020;
    pub const THRM2: u32 = 1021;
}

/// The temperature the thermal assist unit reports, in degrees Celsius.
pub const CHIP_TEMPERATURE: u32 = 45;

// HID2 bits.
pub const HID2_LSQE: u32 = 1 << 31;
pub const HID2_WPE: u32 = 1 << 30;
pub const HID2_PSE: u32 = 1 << 29;
pub const HID2_LCE: u32 = 1 << 28;
// HID4 (Broadway): SBE turns on BATs 4-7.
pub const HID4_SBE: u32 = 1 << 25;

/// Exception vectors (offsets; the base is 0 or 0xFFF00000 by MSR[IP]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Exception {
    SystemReset = 0x100,
    MachineCheck = 0x200,
    Dsi = 0x300,
    Isi = 0x400,
    External = 0x500,
    Alignment = 0x600,
    Program = 0x700,
    FpUnavailable = 0x800,
    Decrementer = 0x900,
    Syscall = 0xC00,
    Trace = 0xD00,
    PerfMonitor = 0xF00,
}

// SRR1 bits a program exception adds.
pub const PROGRAM_FP: u32 = 1 << 20;
pub const PROGRAM_ILLEGAL: u32 = 1 << 19;
pub const PROGRAM_PRIVILEGED: u32 = 1 << 18;
pub const PROGRAM_TRAP: u32 = 1 << 17;

/// Gekko's processor version register value (750CXe "Gekko").
pub const PVR_GEKKO: u32 = 0x0008_3214;
/// Broadway's.
pub const PVR_BROADWAY: u32 = 0x0008_7102;

/// The locked half of the data cache, usable as 16 KB of fast memory at 0xE0000000.
pub const LOCKED_CACHE_BASE: u32 = 0xE000_0000;
pub const LOCKED_CACHE_SIZE: usize = 16 * 1024;

/// One CPU cycle per instruction is close enough for now; the time base runs at a twelfth of
/// the CPU clock on both consoles (bus clock / 4).
pub const CYCLES_PER_TIMEBASE_TICK: u64 = 12;

#[derive(Clone, Serialize, Deserialize)]
pub struct Cpu {
    pub gpr: [u32; 32],
    /// Floating-point registers as two 64-bit doubles each: ps0 is the ordinary FPR, ps1 is the
    /// paired-single second half. Kept as raw bits so NaN payloads survive exactly.
    pub ps0: [u64; 32],
    pub ps1: [u64; 32],
    pub cr: u32,
    pub xer: u32,
    pub lr: u32,
    pub ctr: u32,
    pub msr: u32,
    pub fpscr: u32,
    pub pc: u32,
    pub sr: [u32; 16],
    pub srr0: u32,
    pub srr1: u32,
    pub dsisr: u32,
    pub dar: u32,
    pub sdr1: u32,
    pub sprg: [u32; 4],
    pub ear: u32,
    pub dec: u32,
    /// Time base, in time-base ticks.
    pub tb: u64,
    pub ibat: [[u32; 2]; 8],
    pub dbat: [[u32; 2]; 8],
    pub gqr: [u32; 8],
    pub hid0: u32,
    pub hid1: u32,
    pub hid2: u32,
    pub hid4: u32,
    pub wpar: u32,
    pub dmau: u32,
    pub dmal: u32,
    pub l2cr: u32,
    pub pvr: u32,
    /// Every other SPR (performance monitor, thermal, IABR ...): stored, no effect.
    #[serde(with = "serde_spr")]
    pub other_spr: Vec<(u32, u32)>,
    /// lwarx / stwcx. reservation.
    pub reserve: Option<u32>,
    /// The 16 KB locked cache.
    #[serde(with = "serde_bytes_vec")]
    pub locked_cache: Vec<u8>,
    /// The external interrupt line from the console's interrupt controller.
    pub external_line: bool,
    pub dec_pending: bool,
    /// Instructions retired, for timing.
    pub cycles: u64,
    /// Sub-tick remainder of `cycles` for the time base and decrementer.
    tb_rem: u64,
    /// The instruction being executed, for exceptions that point at it.
    #[serde(skip)]
    cur_pc: u32,
    /// Set when the current instruction raised an exception (so it does not complete).
    #[serde(skip)]
    faulted: bool,
    /// The fast interpreter's "stop this block after this instruction" flag.
    #[serde(skip)]
    pub(crate) exit: bool,
    /// How many times each exception vector was taken (development aid; not in states).
    #[serde(skip)]
    pub exceptions: [u64; 16],
    /// The first faults taken (vector, SRR0, DAR), for development.
    #[serde(skip)]
    pub fault_log: Vec<(u32, u32, u32)>,
    /// The page-table translation cache. Rebuilt on demand; never in a save state.
    #[serde(skip)]
    pub(crate) tlb: mmu::Tlb,
}

mod serde_spr {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    pub fn serialize<S: Serializer>(v: &Vec<(u32, u32)>, s: S) -> Result<S::Ok, S::Error> {
        v.serialize(s)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<(u32, u32)>, D::Error> {
        Vec::deserialize(d)
    }
}

mod serde_bytes_vec {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    pub fn serialize<S: Serializer>(v: &Vec<u8>, s: S) -> Result<S::Ok, S::Error> {
        v.serialize(s)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        Vec::deserialize(d)
    }
}

/// What a memory access is for (translation and the exception it raises differ).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Access {
    Read,
    Write,
    Fetch,
}

impl Default for Cpu {
    fn default() -> Self {
        Self::new(PVR_GEKKO)
    }
}

impl Cpu {
    pub fn new(pvr: u32) -> Self {
        Cpu {
            gpr: [0; 32],
            ps0: [0; 32],
            ps1: [0; 32],
            cr: 0,
            xer: 0,
            lr: 0,
            ctr: 0,
            // Out of reset: exception prefix high, translation off, machine check on.
            msr: MSR_IP | MSR_ME,
            fpscr: 0,
            pc: 0xFFF0_0100,
            sr: [0; 16],
            srr0: 0,
            srr1: 0,
            dsisr: 0,
            dar: 0,
            sdr1: 0,
            sprg: [0; 4],
            ear: 0,
            dec: 0xFFFF_FFFF,
            tb: 0,
            ibat: [[0; 2]; 8],
            dbat: [[0; 2]; 8],
            gqr: [0; 8],
            hid0: 0,
            hid1: 0,
            hid2: 0,
            hid4: 0,
            wpar: 0,
            dmau: 0,
            dmal: 0,
            l2cr: 0,
            pvr,
            other_spr: Vec::new(),
            reserve: None,
            locked_cache: vec![0; LOCKED_CACHE_SIZE],
            external_line: false,
            dec_pending: false,
            cycles: 0,
            tb_rem: 0,
            cur_pc: 0,
            faulted: false,
            exit: false,
            exceptions: [0; 16],
            fault_log: Vec::new(),
            tlb: mmu::Tlb::default(),
        }
    }

    /// Broadway (Wii) has four extra BAT pairs, turned on by HID4[SBE].
    pub fn is_broadway(&self) -> bool {
        self.pvr == PVR_BROADWAY
    }

    // ------------------------------------------------------------------ condition register

    pub fn cr_bit(&self, bit: u32) -> bool {
        (self.cr >> (31 - bit)) & 1 != 0
    }

    pub fn set_cr_bit(&mut self, bit: u32, v: bool) {
        let m = 1u32 << (31 - bit);
        if v {
            self.cr |= m;
        } else {
            self.cr &= !m;
        }
    }

    pub fn cr_field(&self, field: u32) -> u32 {
        (self.cr >> (28 - 4 * field)) & 0xF
    }

    pub fn set_cr_field(&mut self, field: u32, v: u32) {
        let shift = 28 - 4 * field;
        self.cr = (self.cr & !(0xF << shift)) | ((v & 0xF) << shift);
    }

    /// CR0 from a result: LT, GT, EQ and a copy of XER[SO].
    pub fn update_cr0(&mut self, result: u32) {
        let r = result as i32;
        let v = if r < 0 {
            8
        } else if r > 0 {
            4
        } else {
            2
        };
        self.set_cr_field(0, v | (self.xer >> 31));
    }

    pub fn ca(&self) -> bool {
        self.xer & XER_CA != 0
    }

    pub fn set_ca(&mut self, v: bool) {
        if v {
            self.xer |= XER_CA;
        } else {
            self.xer &= !XER_CA;
        }
    }

    pub fn set_ov(&mut self, v: bool) {
        if v {
            self.xer |= XER_OV | XER_SO;
        } else {
            self.xer &= !XER_OV;
        }
    }

    // ------------------------------------------------------------------ exceptions

    /// Takes an exception: SRR0/SRR1 saved, MSR set for the handler, PC to the vector.
    /// `srr0` is the address the handler returns to.
    pub fn raise(&mut self, ex: Exception, srr0: u32, srr1_bits: u32) {
        self.exceptions[(ex as usize >> 8) & 15] += 1;
        self.srr0 = srr0;
        self.srr1 = (self.msr & MSR_SAVED) | srr1_bits;
        let keep = self.msr & (MSR_ME | MSR_IP | MSR_ILE);
        self.msr = keep | if keep & MSR_ILE != 0 { MSR_LE } else { 0 };
        let base = if self.msr & MSR_IP != 0 { 0xFFF0_0000 } else { 0 };
        self.pc = base | ex as u32;
        self.faulted = true;
        self.reserve = None;
        self.tlb.flush_mode();
    }

    /// The current instruction's fault (DSI, ISI, program ...): SRR0 points at it.
    pub(crate) fn fault(&mut self, ex: Exception, srr1_bits: u32) {
        let pc = self.cur_pc;
        if matches!(ex, Exception::Dsi | Exception::Isi | Exception::Program | Exception::Alignment) && self.fault_log.len() < 16 {
            self.fault_log.push((ex as u32, pc, self.dar));
        }
        self.raise(ex, pc, srr1_bits);
    }

    /// Takes a pending asynchronous interrupt if it is allowed now. Returns true if one was taken.
    #[inline]
    pub fn check_interrupts(&mut self) -> bool {
        if self.msr & MSR_EE == 0 {
            return false;
        }
        if self.external_line {
            let pc = self.pc;
            self.raise(Exception::External, pc, 0);
            self.faulted = false;
            return true;
        }
        if self.dec_pending {
            self.dec_pending = false;
            let pc = self.pc;
            self.raise(Exception::Decrementer, pc, 0);
            self.faulted = false;
            return true;
        }
        false
    }

    // ------------------------------------------------------------------ time

    /// Advances the time base and the decrementer by `cycles` CPU cycles.
    #[inline]
    pub fn advance_time(&mut self, cycles: u64) {
        self.cycles += cycles;
        self.tb_rem += cycles;
        if self.tb_rem < CYCLES_PER_TIMEBASE_TICK {
            return;
        }
        let ticks = self.tb_rem / CYCLES_PER_TIMEBASE_TICK;
        if ticks == 0 {
            return;
        }
        self.tb_rem %= CYCLES_PER_TIMEBASE_TICK;
        self.tb = self.tb.wrapping_add(ticks);
        let old = self.dec;
        self.dec = old.wrapping_sub(ticks as u32);
        // The decrementer interrupt fires when bit 0 goes from 0 to 1 (it passes zero).
        if (old as i32) >= 0 && (ticks > old as u64) {
            self.dec_pending = true;
        }
    }

    /// CPU cycles until the decrementer next passes zero (for the scheduler).
    pub fn cycles_to_decrementer(&self) -> u64 {
        if (self.dec as i32) < 0 {
            return u64::MAX;
        }
        (self.dec as u64 + 1) * CYCLES_PER_TIMEBASE_TICK - self.tb_rem
    }

    // ------------------------------------------------------------------ special registers

    pub fn read_spr(&mut self, n: u32) -> u32 {
        use spr::*;
        match n {
            XER => self.xer,
            LR => self.lr,
            CTR => self.ctr,
            DSISR => self.dsisr,
            DAR => self.dar,
            DEC => self.dec,
            SDR1 => self.sdr1,
            SRR0 => self.srr0,
            SRR1 => self.srr1,
            TBL_READ | TBL => self.tb as u32,
            TBU_READ | TBU => (self.tb >> 32) as u32,
            272..=275 => self.sprg[(n - SPRG0) as usize],
            EAR => self.ear,
            PVR => self.pvr,
            528..=535 => self.ibat[((n - IBAT0U) / 2) as usize][(n & 1) as usize],
            536..=543 => self.dbat[((n - DBAT0U) / 2) as usize][(n & 1) as usize],
            560..=567 => self.ibat[(4 + (n - IBAT4U) / 2) as usize][(n & 1) as usize],
            568..=575 => self.dbat[(4 + (n - DBAT4U) / 2) as usize][(n & 1) as usize],
            912..=919 => self.gqr[(n - GQR0) as usize],
            HID0 => self.hid0,
            HID1 => self.hid1,
            HID2 => self.hid2,
            HID4 => self.hid4,
            // WPAR reads back with the "buffer not empty" bit clear: the pipe drains at once.
            WPAR => self.wpar & !1,
            DMAU => self.dmau,
            DMAL => self.dmal,
            L2CR => self.l2cr,
            // The thermal assist unit (THRM1, THRM2): with V set, the compare has finished at
            // once (TIV) and TIN says whether the chip is past the threshold, for a steady 45 C.
            THRM1 | THRM2 => {
                let v = self.other_spr.iter().find(|(k, _)| *k == n).map_or(0, |(_, v)| *v);
                if v & 1 == 0 {
                    return v & !0xC000_0000;
                }
                let threshold = (v >> 23) & 0x7F;
                let below = v & 4 != 0;
                let crossed = if below { CHIP_TEMPERATURE < threshold } else { CHIP_TEMPERATURE > threshold };
                (v & !0xC000_0000) | 0x4000_0000 | if crossed { 0x8000_0000 } else { 0 }
            }
            _ => self.other_spr.iter().find(|(k, _)| *k == n).map_or(0, |(_, v)| *v),
        }
    }

    pub fn write_spr<B: Bus>(&mut self, n: u32, v: u32, bus: &mut B) {
        use spr::*;
        match n {
            XER => self.xer = v,
            LR => self.lr = v,
            CTR => self.ctr = v,
            DSISR => self.dsisr = v,
            DAR => self.dar = v,
            DEC => {
                // Writing a value with bit 0 set from one without it fires the interrupt.
                if (self.dec as i32) >= 0 && (v as i32) < 0 {
                    self.dec_pending = true;
                }
                self.dec = v;
            }
            SDR1 => {
                self.sdr1 = v;
                self.tlb.flush_all();
            }
            SRR0 => self.srr0 = v,
            SRR1 => self.srr1 = v,
            272..=275 => self.sprg[(n - SPRG0) as usize] = v,
            EAR => self.ear = v,
            TBL => self.tb = (self.tb & !0xFFFF_FFFF) | v as u64,
            TBU => self.tb = (self.tb & 0xFFFF_FFFF) | ((v as u64) << 32),
            528..=535 => {
                self.ibat[((n - IBAT0U) / 2) as usize][(n & 1) as usize] = v;
                self.tlb.flush_all();
            }
            536..=543 => {
                self.dbat[((n - DBAT0U) / 2) as usize][(n & 1) as usize] = v;
                self.tlb.flush_all();
            }
            560..=567 => {
                self.ibat[(4 + (n - IBAT4U) / 2) as usize][(n & 1) as usize] = v;
                self.tlb.flush_all();
            }
            568..=575 => {
                self.dbat[(4 + (n - DBAT4U) / 2) as usize][(n & 1) as usize] = v;
                self.tlb.flush_all();
            }
            912..=919 => self.gqr[(n - GQR0) as usize] = v,
            HID0 => {
                // ICFI (bit 11): flash-invalidate the instruction cache.
                if v & (1 << 11) != 0 {
                    bus.invalidate_code(0, u32::MAX);
                }
                self.hid0 = v & !(1 << 11);
            }
            HID1 => self.hid1 = v,
            HID2 => self.hid2 = v,
            HID4 => {
                self.hid4 = v;
                self.tlb.flush_all();
            }
            WPAR => self.wpar = v & !0x1F,
            DMAU => self.dmau = v,
            DMAL => {
                self.dmal = v;
                if v & 2 != 0 {
                    self.locked_cache_dma(bus);
                }
            }
            L2CR => self.l2cr = v,
            PVR => {}
            _ => {
                if let Some(e) = self.other_spr.iter_mut().find(|(k, _)| *k == n) {
                    e.1 = v;
                } else {
                    self.other_spr.push((n, v));
                }
            }
        }
    }

    /// The locked-cache DMA (DMAU/DMAL): copies between main memory and the locked cache.
    fn locked_cache_dma<B: Bus>(&mut self, bus: &mut B) {
        let mem = self.dmau & !0x1F;
        let cache = self.dmal & !0x1F;
        // Length in 32-byte lines: DMA_LEN_U (DMAU bits 0-4) and DMA_LEN_L (DMAL bits 2-3); 0 means 128.
        let mut lines = ((self.dmau & 0x1F) << 2) | ((self.dmal >> 2) & 3);
        if lines == 0 {
            lines = 128;
        }
        let load = self.dmal & 0x10 != 0;
        for i in 0..lines * 32 {
            let c = (cache.wrapping_sub(LOCKED_CACHE_BASE).wrapping_add(i)) as usize & (LOCKED_CACHE_SIZE - 1);
            let m = mem.wrapping_add(i);
            if load {
                self.locked_cache[c] = bus.read8(m);
            } else {
                bus.write8(m, self.locked_cache[c]);
            }
        }
        // Done at once: the trigger bit clears.
        self.dmal &= !2;
    }

    // ------------------------------------------------------------------ memory access

    /// Translates `ea` for a data access, raising a DSI and returning None if it does not map.
    fn data_pa<B: Bus>(&mut self, ea: u32, access: Access, bus: &mut B) -> Option<u32> {
        if self.msr & MSR_DR == 0 {
            return Some(ea);
        }
        match mmu::translate(self, ea, access, bus) {
            Some(pa) => Some(pa),
            None => {
                self.dar = ea;
                self.dsisr = 0x4000_0000 | if access == Access::Write { 0x0200_0000 } else { 0 };
                self.fault(Exception::Dsi, 0);
                None
            }
        }
    }

    fn in_locked_cache(&self, pa: u32) -> bool {
        self.hid2 & HID2_LCE != 0 && pa.wrapping_sub(LOCKED_CACHE_BASE) < LOCKED_CACHE_SIZE as u32
    }

    /// Reads `n` (1, 2, 4, 8) big-endian bytes at `ea`. Accesses that cross a 4 KB page are
    /// translated byte by byte, so a split access sees both pages correctly.
    pub fn read<B: Bus>(&mut self, ea: u32, n: u32, bus: &mut B) -> Option<u64> {
        if (ea & 0xFFF) + n > 0x1000 {
            let mut v = 0u64;
            for i in 0..n {
                let pa = self.data_pa(ea.wrapping_add(i), Access::Read, bus)?;
                v = (v << 8) | self.phys_read8(pa, bus) as u64;
            }
            return Some(v);
        }
        let pa = self.data_pa(ea, Access::Read, bus)?;
        if self.in_locked_cache(pa) {
            let mut v = 0u64;
            for i in 0..n {
                v = (v << 8) | self.phys_read8(pa + i, bus) as u64;
            }
            return Some(v);
        }
        Some(match n {
            1 => bus.read8(pa) as u64,
            2 => bus.read16(pa) as u64,
            4 => bus.read32(pa) as u64,
            _ => bus.read64(pa),
        })
    }

    pub fn write<B: Bus>(&mut self, ea: u32, n: u32, v: u64, bus: &mut B) -> Option<()> {
        if (ea & 0xFFF) + n > 0x1000 {
            // Translate every byte first so a fault leaves memory untouched.
            let mut pas = [0u32; 8];
            for i in 0..n {
                pas[i as usize] = self.data_pa(ea.wrapping_add(i), Access::Write, bus)?;
            }
            for i in 0..n {
                let byte = (v >> (8 * (n - 1 - i))) as u8;
                self.phys_write8(pas[i as usize], byte, bus);
            }
            return Some(());
        }
        let pa = self.data_pa(ea, Access::Write, bus)?;
        if self.in_locked_cache(pa) {
            for i in 0..n {
                self.phys_write8(pa + i, (v >> (8 * (n - 1 - i))) as u8, bus);
            }
            return Some(());
        }
        match n {
            1 => bus.write8(pa, v as u8),
            2 => bus.write16(pa, v as u16),
            4 => bus.write32(pa, v as u32),
            _ => bus.write64(pa, v),
        }
        Some(())
    }

    fn phys_read8<B: Bus>(&mut self, pa: u32, bus: &mut B) -> u8 {
        if self.in_locked_cache(pa) {
            self.locked_cache[(pa - LOCKED_CACHE_BASE) as usize]
        } else {
            bus.read8(pa)
        }
    }

    fn phys_write8<B: Bus>(&mut self, pa: u32, v: u8, bus: &mut B) {
        if self.in_locked_cache(pa) {
            self.locked_cache[(pa - LOCKED_CACHE_BASE) as usize] = v;
        } else {
            bus.write8(pa, v);
        }
    }

    /// Fetches the instruction at the PC, raising an ISI if it does not map.
    fn fetch<B: Bus>(&mut self, bus: &mut B) -> Option<u32> {
        let pc = self.pc;
        let pa = if self.msr & MSR_IR == 0 {
            pc
        } else {
            match mmu::translate(self, pc, Access::Fetch, bus) {
                Some(pa) => pa,
                None => {
                    self.fault(Exception::Isi, 0x4000_0000);
                    return None;
                }
            }
        };
        Some(bus.read32(pa))
    }

    // ------------------------------------------------------------------ running

    /// Runs one instruction (or takes one interrupt). Returns the cycles it took.
    pub fn step<B: Bus>(&mut self, bus: &mut B) -> u64 {
        if self.check_interrupts() {
            return 1;
        }
        self.faulted = false;
        self.cur_pc = self.pc;
        let Some(inst) = self.fetch(bus) else {
            return 1;
        };
        self.pc = self.pc.wrapping_add(4);
        self.execute(inst, bus);
        self.advance_time(1);
        1
    }

    /// Runs until at least `cycles` cycles have passed.
    pub fn run<B: Bus>(&mut self, bus: &mut B, cycles: u64) -> u64 {
        let mut done = 0;
        while done < cycles {
            done += self.step(bus);
        }
        done
    }

    // ------------------------------------------------------------------ FP registers as doubles

    pub fn f0(&self, r: usize) -> f64 {
        f64::from_bits(self.ps0[r])
    }
    pub fn f1(&self, r: usize) -> f64 {
        f64::from_bits(self.ps1[r])
    }
    pub fn set_f0(&mut self, r: usize, v: f64) {
        self.ps0[r] = v.to_bits();
    }
    pub fn set_f1(&mut self, r: usize, v: f64) {
        self.ps1[r] = v.to_bits();
    }
}
