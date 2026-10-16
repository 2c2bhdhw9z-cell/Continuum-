//! The Zilog Z80, as Zilog's NMOS part behaves, for every board cArcade plays: every documented and
//! undocumented instruction, all eight bits of F (the two undocumented ones, 3 and 5, included),
//! the hidden MEMPTR register (WZ) that leaks into those two bits, Q (which decides what SCF and
//! CCF put there), the refresh register, interrupt modes 0, 1 and 2, the NMI, HALT, and the
//! T-states of every instruction.
//!
//! `step` runs one whole instruction, taking a waiting interrupt first, and returns the T-states it
//! took; a board adds any wait states its own memory inserts. Interrupts are only looked at between
//! instructions, as on the chip.
//!
//! Written from Zilog's Z80 CPU User Manual and what has been measured on real chips since: Sean
//! Young's "The Undocumented Z80 Documented" (the undocumented instructions and flags), boo_boo
//! and Vladimir Kladov's "MEMPTR, esoteric register of the ZiLOG Z80 CPU", Patrik Rak's findings
//! on Q, and David Banks' on the flags a block instruction leaves when it repeats. Proven against
//! every case of SingleStepTests' Z80 suite (tests/z80_singlestep.rs) and by ZEXDOC and ZEXALL
//! (tests/z80_zex.rs); the interrupts, which neither suite reaches, by the tests at the end of
//! this file.

use serde::{Deserialize, Serialize};

/// Carry.
pub const CF: u8 = 0x01;
/// Add/subtract: set by a subtraction, read by DAA.
pub const NF: u8 = 0x02;
/// Parity or overflow.
pub const PF: u8 = 0x04;
/// Undocumented bit 3: a copy of bit 3 of a value the instruction worked on.
pub const XF: u8 = 0x08;
/// Half carry, out of bit 3 (bit 11 for 16-bit arithmetic).
pub const HF: u8 = 0x10;
/// Undocumented bit 5: a copy of bit 5 of a value the instruction worked on.
pub const YF: u8 = 0x20;
pub const ZF: u8 = 0x40;
pub const SF: u8 = 0x80;

const fn flag_table(parity: bool) -> [u8; 256] {
    let mut t = [0u8; 256];
    let mut i = 0;
    while i < 256 {
        let v = i as u8;
        let mut f = v & (SF | YF | XF);
        if v == 0 {
            f |= ZF;
        }
        if parity && v.count_ones() & 1 == 0 {
            f |= PF;
        }
        t[i] = f;
        i += 1;
    }
    t
}

/// S, Z and bits 5 and 3 of a result.
static SZXY: [u8; 256] = flag_table(false);
/// The same, with P/V set for even parity.
static SZXYP: [u8; 256] = flag_table(true);

/// T-states of each unprefixed instruction. A conditional jump, call or return is listed at its
/// not-taken time and the rest is added where it is taken. A DD or FD prefix adds 4, and
/// (IX+d) or (IY+d) in place of (HL) adds 8 more (5 for LD (IX+d),n, which reads n meanwhile).
/// CB, DD, ED and FD are 0: their instructions are timed where they are run.
#[rustfmt::skip]
static CYCLES: [u8; 256] = [
//  0   1   2   3   4   5   6   7   8   9   A   B   C   D   E   F
    4, 10,  7,  6,  4,  4,  7,  4,  4, 11,  7,  6,  4,  4,  7,  4, // 0
    8, 10,  7,  6,  4,  4,  7,  4, 12, 11,  7,  6,  4,  4,  7,  4, // 1
    7, 10, 16,  6,  4,  4,  7,  4,  7, 11, 16,  6,  4,  4,  7,  4, // 2
    7, 10, 13,  6, 11, 11, 10,  4,  7, 11, 13,  6,  4,  4,  7,  4, // 3
    4,  4,  4,  4,  4,  4,  7,  4,  4,  4,  4,  4,  4,  4,  7,  4, // 4
    4,  4,  4,  4,  4,  4,  7,  4,  4,  4,  4,  4,  4,  4,  7,  4, // 5
    4,  4,  4,  4,  4,  4,  7,  4,  4,  4,  4,  4,  4,  4,  7,  4, // 6
    7,  7,  7,  7,  7,  7,  4,  7,  4,  4,  4,  4,  4,  4,  7,  4, // 7
    4,  4,  4,  4,  4,  4,  7,  4,  4,  4,  4,  4,  4,  4,  7,  4, // 8
    4,  4,  4,  4,  4,  4,  7,  4,  4,  4,  4,  4,  4,  4,  7,  4, // 9
    4,  4,  4,  4,  4,  4,  7,  4,  4,  4,  4,  4,  4,  4,  7,  4, // A
    4,  4,  4,  4,  4,  4,  7,  4,  4,  4,  4,  4,  4,  4,  7,  4, // B
    5, 10, 10, 10, 10, 11,  7, 11,  5, 10, 10,  0, 10, 17,  7, 11, // C
    5, 10, 10, 11, 10, 11,  7, 11,  5,  4, 10, 11, 10,  0,  7, 11, // D
    5, 10, 10, 19, 10, 11,  7, 11,  5,  4, 10,  4, 10,  0,  7, 11, // E
    5, 10, 10,  4, 10, 11,  7, 11,  5,  6, 10,  4, 10,  0,  7, 11, // F
];

/// What a Z80 is wired to on a board: its memory, its I/O ports and the interrupting device.
pub trait Bus {
    /// A memory read.
    fn read(&mut self, addr: u16) -> u8;
    /// A memory write.
    fn write(&mut self, addr: u16, val: u8);
    /// An opcode fetch (an M1 cycle): a plain read, unless the board decrypts opcodes on the way.
    #[inline]
    fn fetch(&mut self, addr: u16) -> u8 {
        self.read(addr)
    }
    /// An I/O read. The Z80 puts out a 16-bit address for I/O: the port in the low byte, and A
    /// (for IN A,(n)) or B (for the rest) in the high one.
    fn input(&mut self, port: u16) -> u8;
    /// An I/O write.
    fn output(&mut self, port: u16, val: u8);
    /// An interrupt acknowledge: the byte the interrupting device puts on the data bus. In mode 2
    /// it is the low half of the vector-table address; in mode 0 it is the instruction to run
    /// (usually an RST), and this is asked again for each further byte of a longer one. Mode 1
    /// ignores it, but still asks, so a board can see the acknowledge. With nothing driving the
    /// bus the pull-ups read 0xFF, which is RST 38h in mode 0.
    #[inline]
    fn int_ack(&mut self) -> u8 {
        0xFF
    }
}

/// Which register an instruction's "HL" means: HL itself, or IX or IY after a DD or FD prefix
/// (which also turns H and L into the halves of IX or IY, and (HL) into (IX+d)).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Idx {
    Hl,
    Ix,
    Iy,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Z80 {
    pub a: u8,
    pub f: u8,
    pub b: u8,
    pub c: u8,
    pub d: u8,
    pub e: u8,
    pub h: u8,
    pub l: u8,
    /// The second register set (AF', BC', DE', HL'), swapped in by EX AF,AF' and EXX.
    pub af2: u16,
    pub bc2: u16,
    pub de2: u16,
    pub hl2: u16,
    pub ix: u16,
    pub iy: u16,
    pub sp: u16,
    pub pc: u16,
    /// The interrupt vector register: the high half of the mode-2 vector table's address.
    pub i: u8,
    /// The refresh register. Its low seven bits count opcode fetches (one per M1 cycle, so a
    /// prefixed instruction counts two); bit 7 only changes by LD R,A.
    pub r: u8,
    /// MEMPTR (WZ), the Z80's hidden address latch. No instruction reads it, but BIT n,(HL)
    /// copies its bits 13 and 11 into flag bits 5 and 3, so a program can see it.
    pub wz: u16,
    pub iff1: bool,
    pub iff2: bool,
    /// Interrupt mode, 0 to 2.
    pub im: u8,
    /// Stopped by HALT until an interrupt. PC already points past the HALT, so that is what the
    /// interrupt pushes.
    pub halted: bool,
    /// Q: the flags the last instruction wrote, or 0 if it wrote none. Zilog's NMOS Z80 sets bits 5
    /// and 3 after SCF and CCF from (Q ^ F) | A.
    pub q: u8,
    /// The last instruction was EI: a maskable interrupt waits for one more instruction.
    pub ei: bool,
    /// The last instruction was LD A,I or LD A,R: an interrupt accepted now clears the P/V flag
    /// they just set from IFF2 (an NMOS quirk).
    pub p: bool,
    /// The /INT line, true while the board holds it asserted.
    pub irq: bool,
    /// An NMI edge not yet taken.
    pub nmi_pending: bool,
    /// Running a mode-0 interrupt's instruction: its bytes come from the device, not from memory
    /// at PC. Only ever set inside `step`.
    #[serde(skip)]
    im0: bool,
    /// The wait states the mode-0 acknowledge cycles of that instruction added.
    #[serde(skip)]
    im0_waits: u32,
}

impl Default for Z80 {
    fn default() -> Self {
        Z80::new()
    }
}

impl Z80 {
    /// A Z80 just powered on, as MAME's Z80 comes up (the reference the boards are compared
    /// with; on a real chip most of these hold no defined value): every register 0 but F, which
    /// has only the zero flag set, and IX and IY, which are FFFFh; the power-on reset clears PC,
    /// I, R and the interrupt state. (Real chips are said to come up with AF and SP at FFFFh; a
    /// program that relies on either before setting it shows the difference.)
    pub fn new() -> Z80 {
        let mut z = Z80 {
            a: 0x00,
            f: 0x40,
            b: 0,
            c: 0,
            d: 0,
            e: 0,
            h: 0,
            l: 0,
            af2: 0,
            bc2: 0,
            de2: 0,
            hl2: 0,
            // IX and IY come up as FFFF on the chip, as MAME's Z80 has them at power-on (a
            // reset leaves them alone). A game that pushes them before loading them shows it.
            ix: 0xFFFF,
            iy: 0xFFFF,
            sp: 0x0000,
            pc: 0,
            i: 0,
            r: 0,
            wz: 0,
            iff1: false,
            iff2: false,
            im: 0,
            halted: false,
            q: 0,
            ei: false,
            p: false,
            irq: false,
            nmi_pending: false,
            im0: false,
            im0_waits: 0,
        };
        z.reset();
        z
    }

    /// /RESET: PC, I and R go to zero, interrupts are disabled and mode 0 is selected. The other
    /// registers keep what they held.
    pub fn reset(&mut self) {
        self.pc = 0;
        self.i = 0;
        self.r = 0;
        self.iff1 = false;
        self.iff2 = false;
        self.im = 0;
        self.halted = false;
        self.ei = false;
        self.p = false;
        self.q = 0;
        self.nmi_pending = false;
    }

    /// Sets the level of the /INT line (true: asserted). The board holds it until its device is
    /// done; the Z80 takes it between instructions while IFF1 is set.
    pub fn set_irq(&mut self, asserted: bool) {
        self.irq = asserted;
    }

    /// A falling edge on /NMI. It is taken before the next instruction, whatever IFF1 says.
    pub fn nmi(&mut self) {
        self.nmi_pending = true;
    }

    pub fn af(&self) -> u16 {
        u16::from(self.a) << 8 | u16::from(self.f)
    }
    pub fn bc(&self) -> u16 {
        u16::from(self.b) << 8 | u16::from(self.c)
    }
    pub fn de(&self) -> u16 {
        u16::from(self.d) << 8 | u16::from(self.e)
    }
    pub fn hl(&self) -> u16 {
        u16::from(self.h) << 8 | u16::from(self.l)
    }
    pub fn set_af(&mut self, v: u16) {
        self.a = (v >> 8) as u8;
        self.f = v as u8;
    }
    pub fn set_bc(&mut self, v: u16) {
        self.b = (v >> 8) as u8;
        self.c = v as u8;
    }
    pub fn set_de(&mut self, v: u16) {
        self.d = (v >> 8) as u8;
        self.e = v as u8;
    }
    pub fn set_hl(&mut self, v: u16) {
        self.h = (v >> 8) as u8;
        self.l = v as u8;
    }

    /// Runs one instruction and returns the T-states it took. If an interrupt is waiting, it is
    /// taken first, and the first instruction of its handler runs in the same step: the Z80 only
    /// looks at /INT and /NMI at the end of an instruction, so a second NMI can't cut into the
    /// first one's response (MAME runs the two together the same way).
    pub fn step<B: Bus>(&mut self, bus: &mut B) -> u32 {
        if self.nmi_pending {
            let t = self.take_nmi(bus);
            return t + self.run_one(bus);
        }
        if self.irq && self.iff1 && !self.ei {
            let t = self.take_int(bus);
            return t + self.run_one(bus);
        }
        self.run_one(bus)
    }

    /// Takes a waiting interrupt, if there is one, without running the instruction after it, and
    /// returns its T-states (0 if none was waiting). A `step` right after it runs the handler's
    /// first instruction, so the two together are one `step`; a board that traces instructions,
    /// as MAME's debugger does after an interrupt is taken, uses them apart.
    pub fn take_interrupt<B: Bus>(&mut self, bus: &mut B) -> u32 {
        if self.nmi_pending {
            return self.take_nmi(bus);
        }
        if self.irq && self.iff1 && !self.ei {
            return self.take_int(bus);
        }
        0
    }

    /// One instruction, with no interrupt check before it.
    fn run_one<B: Bus>(&mut self, bus: &mut B) -> u32 {
        let q_in = self.q;
        self.q = 0;
        self.ei = false;
        self.p = false;
        if self.halted {
            // A halted Z80 keeps fetching the byte after the HALT and running NOPs in its place,
            // 4 T-states and one refresh each, until an interrupt.
            bus.fetch(self.pc);
            self.inc_r();
            return 4;
        }
        let op = self.fetch_op(bus);
        self.exec(bus, op, q_in)
    }

    /// The NMI: IFF1 is cleared (IFF2 keeps the old state for RETN to restore) and the Z80 calls
    /// 0066h, in 11 T-states.
    fn take_nmi<B: Bus>(&mut self, bus: &mut B) -> u32 {
        self.nmi_pending = false;
        self.halted = false;
        self.accept_quirk();
        self.iff1 = false;
        self.inc_r();
        let pc = self.pc;
        self.push(bus, pc);
        self.pc = 0x0066;
        self.wz = 0x0066;
        11
    }

    /// A maskable interrupt: both flip-flops are cleared, then the acknowledge cycle (an opcode
    /// fetch with two wait states) and what the mode says.
    fn take_int<B: Bus>(&mut self, bus: &mut B) -> u32 {
        self.halted = false;
        self.accept_quirk();
        self.iff1 = false;
        self.iff2 = false;
        self.inc_r();
        let t = match self.im {
            0 => {
                // Mode 0: the device supplies an instruction (on arcade boards an RST) and the Z80
                // runs it with PC left where it was, so an RST or CALL pushes the interrupted
                // address. Every byte comes from the device: prefixes in further acknowledge
                // cycles (two wait states each, like the first), operands in memory reads.
                let op = bus.int_ack();
                self.im0 = true;
                self.im0_waits = 2;
                let q_in = self.q;
                self.q = 0;
                let t = self.exec(bus, op, q_in);
                self.im0 = false;
                t + self.im0_waits
            }
            1 => {
                // Mode 1: an RST 38h, whatever is on the bus.
                bus.int_ack();
                let pc = self.pc;
                self.push(bus, pc);
                self.pc = 0x0038;
                13
            }
            _ => {
                // Mode 2: a call through the table entry at I:vector. All eight bits of the
                // vector are used (Zilog says bit 0 must be 0; real chips don't care).
                let v = bus.int_ack();
                let pc = self.pc;
                self.push(bus, pc);
                let entry = u16::from(self.i) << 8 | u16::from(v);
                self.pc = self.read16(bus, entry);
                19
            }
        };
        self.wz = self.pc;
        t
    }

    /// The NMOS quirk: LD A,I and LD A,R copy IFF2 into P/V, but an interrupt accepted straight
    /// after one of them leaves P/V clear.
    fn accept_quirk(&mut self) {
        if self.p {
            self.f &= !PF;
        }
        self.p = false;
        self.ei = false;
        self.q = 0;
    }

    #[inline]
    fn inc_r(&mut self) {
        self.r = (self.r & 0x80) | (self.r.wrapping_add(1) & 0x7F);
    }

    /// An opcode fetch: an M1 cycle, which refreshes memory and so counts in R.
    #[inline]
    fn fetch_op<B: Bus>(&mut self, bus: &mut B) -> u8 {
        self.inc_r();
        if self.im0 {
            self.im0_waits += 2;
            return bus.int_ack();
        }
        let v = bus.fetch(self.pc);
        self.pc = self.pc.wrapping_add(1);
        v
    }

    /// An operand byte from the instruction stream (a plain memory read, not an M1).
    #[inline]
    fn imm8<B: Bus>(&mut self, bus: &mut B) -> u8 {
        if self.im0 {
            return bus.int_ack();
        }
        let v = bus.read(self.pc);
        self.pc = self.pc.wrapping_add(1);
        v
    }

    #[inline]
    fn imm16<B: Bus>(&mut self, bus: &mut B) -> u16 {
        let lo = self.imm8(bus);
        let hi = self.imm8(bus);
        u16::from(hi) << 8 | u16::from(lo)
    }

    #[inline]
    fn read16<B: Bus>(&mut self, bus: &mut B, addr: u16) -> u16 {
        let lo = bus.read(addr);
        let hi = bus.read(addr.wrapping_add(1));
        u16::from(hi) << 8 | u16::from(lo)
    }

    #[inline]
    fn write16<B: Bus>(&mut self, bus: &mut B, addr: u16, v: u16) {
        bus.write(addr, v as u8);
        bus.write(addr.wrapping_add(1), (v >> 8) as u8);
    }

    /// PUSH: the high byte goes in first, at SP-1.
    #[inline]
    fn push<B: Bus>(&mut self, bus: &mut B, v: u16) {
        self.sp = self.sp.wrapping_sub(1);
        bus.write(self.sp, (v >> 8) as u8);
        self.sp = self.sp.wrapping_sub(1);
        bus.write(self.sp, v as u8);
    }

    #[inline]
    fn pop<B: Bus>(&mut self, bus: &mut B) -> u16 {
        let lo = bus.read(self.sp);
        self.sp = self.sp.wrapping_add(1);
        let hi = bus.read(self.sp);
        self.sp = self.sp.wrapping_add(1);
        u16::from(hi) << 8 | u16::from(lo)
    }

    /// Writes F from an instruction that computes flags, which is what Q remembers.
    #[inline]
    fn setf(&mut self, f: u8) {
        self.f = f;
        self.q = f;
    }

    /// Register r of an opcode's 3-bit field (B C D E H L - A; 6 is memory, which the callers
    /// handle). Under a prefix H and L are the halves of IX or IY.
    #[inline]
    fn reg(&self, r: u8, x: Idx) -> u8 {
        match r & 7 {
            0 => self.b,
            1 => self.c,
            2 => self.d,
            3 => self.e,
            4 => match x {
                Idx::Hl => self.h,
                Idx::Ix => (self.ix >> 8) as u8,
                Idx::Iy => (self.iy >> 8) as u8,
            },
            5 => match x {
                Idx::Hl => self.l,
                Idx::Ix => self.ix as u8,
                Idx::Iy => self.iy as u8,
            },
            _ => self.a,
        }
    }

    #[inline]
    fn set_reg(&mut self, r: u8, x: Idx, v: u8) {
        match r & 7 {
            0 => self.b = v,
            1 => self.c = v,
            2 => self.d = v,
            3 => self.e = v,
            4 => match x {
                Idx::Hl => self.h = v,
                Idx::Ix => self.ix = (self.ix & 0x00FF) | u16::from(v) << 8,
                Idx::Iy => self.iy = (self.iy & 0x00FF) | u16::from(v) << 8,
            },
            5 => match x {
                Idx::Hl => self.l = v,
                Idx::Ix => self.ix = (self.ix & 0xFF00) | u16::from(v),
                Idx::Iy => self.iy = (self.iy & 0xFF00) | u16::from(v),
            },
            _ => self.a = v,
        }
    }

    #[inline]
    fn hlx(&self, x: Idx) -> u16 {
        match x {
            Idx::Hl => self.hl(),
            Idx::Ix => self.ix,
            Idx::Iy => self.iy,
        }
    }

    #[inline]
    fn set_hlx(&mut self, x: Idx, v: u16) {
        match x {
            Idx::Hl => self.set_hl(v),
            Idx::Ix => self.ix = v,
            Idx::Iy => self.iy = v,
        }
    }

    /// Register pair p of an opcode's 2-bit field: BC DE HL SP.
    #[inline]
    fn rp(&self, p: u8, x: Idx) -> u16 {
        match p & 3 {
            0 => self.bc(),
            1 => self.de(),
            2 => self.hlx(x),
            _ => self.sp,
        }
    }

    #[inline]
    fn set_rp(&mut self, p: u8, x: Idx, v: u16) {
        match p & 3 {
            0 => self.set_bc(v),
            1 => self.set_de(v),
            2 => self.set_hlx(x, v),
            _ => self.sp = v,
        }
    }

    /// PUSH and POP's pairs: BC DE HL AF.
    #[inline]
    fn rp2(&self, p: u8, x: Idx) -> u16 {
        match p & 3 {
            3 => self.af(),
            _ => self.rp(p, x),
        }
    }

    #[inline]
    fn set_rp2(&mut self, p: u8, x: Idx, v: u16) {
        match p & 3 {
            3 => self.set_af(v),
            _ => self.set_rp(p, x, v),
        }
    }

    /// Condition cc: NZ Z NC C PO PE P M.
    #[inline]
    fn cond(&self, cc: u8) -> bool {
        let f = self.f;
        match cc & 7 {
            0 => f & ZF == 0,
            1 => f & ZF != 0,
            2 => f & CF == 0,
            3 => f & CF != 0,
            4 => f & PF == 0,
            5 => f & PF != 0,
            6 => f & SF == 0,
            _ => f & SF != 0,
        }
    }

    /// The address of an instruction's memory operand and the T-states it adds: (HL), or (IX+d)
    /// with the displacement read now, which also loads MEMPTR and costs 8 (3 for d, 5 to add).
    #[inline]
    fn mem_addr<B: Bus>(&mut self, bus: &mut B, x: Idx) -> (u16, u32) {
        if x == Idx::Hl {
            return (self.hl(), 0);
        }
        let d = self.imm8(bus) as i8;
        let addr = self.hlx(x).wrapping_add(d as u16);
        self.wz = addr;
        (addr, 8)
    }

    #[inline]
    fn jr(&mut self, d: i8) {
        self.pc = self.pc.wrapping_add(d as u16);
        self.wz = self.pc;
    }

    // ---- Arithmetic and logic -------------------------------------------------------------

    /// ADD and ADC: half carry out of bit 3, overflow when both operands have the same sign and
    /// the result doesn't.
    #[inline]
    fn add_a(&mut self, v: u8, c: u8) {
        let a = self.a;
        let wide = u16::from(a) + u16::from(v) + u16::from(c);
        let r = wide as u8;
        let f = SZXY[r as usize]
            | ((a ^ v ^ r) & HF)
            | ((((a ^ !v) & (a ^ r)) >> 5) & PF)
            | (wide >> 8) as u8;
        self.a = r;
        self.setf(f);
    }

    /// SUB and SBC (and CP and NEG, which use it): borrow into bit 4 and out of bit 7.
    #[inline]
    fn sub_a(&mut self, v: u8, c: u8) -> u8 {
        let a = self.a;
        let wide = u16::from(a)
            .wrapping_sub(u16::from(v))
            .wrapping_sub(u16::from(c));
        let r = wide as u8;
        let f = SZXY[r as usize]
            | ((a ^ v ^ r) & HF)
            | ((((a ^ v) & (a ^ r)) >> 5) & PF)
            | NF
            | ((wide >> 8) as u8 & CF);
        self.setf(f);
        r
    }

    /// The eight ALU operations of opcodes 80-BF and C6-FE: ADD ADC SUB SBC AND XOR OR CP.
    #[inline]
    fn alu(&mut self, op: u8, v: u8) {
        match op & 7 {
            0 => self.add_a(v, 0),
            1 => {
                let c = self.f & CF;
                self.add_a(v, c);
            }
            2 => self.a = self.sub_a(v, 0),
            3 => {
                let c = self.f & CF;
                self.a = self.sub_a(v, c);
            }
            4 => {
                self.a &= v;
                self.setf(SZXYP[self.a as usize] | HF);
            }
            5 => {
                self.a ^= v;
                self.setf(SZXYP[self.a as usize]);
            }
            6 => {
                self.a |= v;
                self.setf(SZXYP[self.a as usize]);
            }
            _ => {
                // CP: a subtraction that keeps A, with bits 5 and 3 from the operand instead.
                self.sub_a(v, 0);
                let f = (self.f & !(YF | XF)) | (v & (YF | XF));
                self.setf(f);
            }
        }
    }

    #[inline]
    fn inc8(&mut self, v: u8) -> u8 {
        let r = v.wrapping_add(1);
        let mut f = (self.f & CF) | SZXY[r as usize];
        if r & 0x0F == 0 {
            f |= HF;
        }
        if r == 0x80 {
            f |= PF;
        }
        self.setf(f);
        r
    }

    #[inline]
    fn dec8(&mut self, v: u8) -> u8 {
        let r = v.wrapping_sub(1);
        let mut f = (self.f & CF) | NF | SZXY[r as usize];
        if r & 0x0F == 0x0F {
            f |= HF;
        }
        if r == 0x7F {
            f |= PF;
        }
        self.setf(f);
        r
    }

    /// ADD HL,rr (and IX, IY): carry and half carry from the high byte, bits 5 and 3 from the
    /// result's high byte, S Z P/V kept. MEMPTR becomes the old HL plus one.
    #[inline]
    fn add16(&mut self, a: u16, b: u16) -> u16 {
        let wide = u32::from(a) + u32::from(b);
        let r = wide as u16;
        self.wz = a.wrapping_add(1);
        let f = (self.f & (SF | ZF | PF))
            | ((r >> 8) as u8 & (YF | XF))
            | (((a ^ b ^ r) >> 8) as u8 & HF)
            | (wide >> 16) as u8;
        self.setf(f);
        r
    }

    fn adc_hl(&mut self, b: u16) {
        let a = self.hl();
        let wide = u32::from(a) + u32::from(b) + u32::from(self.f & CF);
        let r = wide as u16;
        self.wz = a.wrapping_add(1);
        let mut f = ((r >> 8) as u8 & (SF | YF | XF))
            | (((a ^ b ^ r) >> 8) as u8 & HF)
            | ((((a ^ !b) & (a ^ r)) >> 13) as u8 & PF)
            | (wide >> 16) as u8;
        if r == 0 {
            f |= ZF;
        }
        self.set_hl(r);
        self.setf(f);
    }

    fn sbc_hl(&mut self, b: u16) {
        let a = self.hl();
        let wide = u32::from(a)
            .wrapping_sub(u32::from(b))
            .wrapping_sub(u32::from(self.f & CF));
        let r = wide as u16;
        self.wz = a.wrapping_add(1);
        let mut f = ((r >> 8) as u8 & (SF | YF | XF))
            | (((a ^ b ^ r) >> 8) as u8 & HF)
            | ((((a ^ b) & (a ^ r)) >> 13) as u8 & PF)
            | NF
            | ((wide >> 16) as u8 & CF);
        if r == 0 {
            f |= ZF;
        }
        self.set_hl(r);
        self.setf(f);
    }

    /// DAA: corrects A after a BCD addition or subtraction, by what N, H and C say happened.
    fn daa(&mut self) {
        let a = self.a;
        let f = self.f;
        let mut fix = 0u8;
        let mut carry = f & CF;
        if f & HF != 0 || a & 0x0F > 9 {
            fix |= 0x06;
        }
        if carry != 0 || a > 0x99 {
            fix |= 0x60;
            carry = CF;
        }
        let r = if f & NF != 0 {
            a.wrapping_sub(fix)
        } else {
            a.wrapping_add(fix)
        };
        self.a = r;
        self.setf(SZXYP[r as usize] | ((a ^ r) & HF) | (f & NF) | carry);
    }

    /// The rotates and shifts of CB 00-3F: RLC RRC RL RR SLA SRA SLL SRL. SLL (CB 30-37) is
    /// undocumented: a left shift that sets bit 0.
    #[inline]
    fn shift(&mut self, op: u8, v: u8) -> u8 {
        let (r, c) = match (op >> 3) & 7 {
            0 => (v.rotate_left(1), v >> 7),
            1 => (v.rotate_right(1), v & 1),
            2 => ((v << 1) | (self.f & CF), v >> 7),
            3 => ((v >> 1) | (self.f << 7), v & 1),
            4 => (v << 1, v >> 7),
            5 => ((v >> 1) | (v & 0x80), v & 1),
            6 => ((v << 1) | 1, v >> 7),
            _ => (v >> 1, v & 1),
        };
        self.setf(SZXYP[r as usize] | c);
        r
    }

    /// BIT n: Z and P/V say the bit is clear, S is bit 7 when n is 7, and bits 5 and 3 come from
    /// `xy`: the register itself, MEMPTR's high byte for (HL), the address's high byte for (IX+d).
    #[inline]
    fn bit(&mut self, n: u8, v: u8, xy: u8) {
        let m = v & (1 << n);
        let mut f = (self.f & CF) | HF | (xy & (YF | XF)) | (m & SF);
        if m == 0 {
            f |= ZF | PF;
        }
        self.setf(f);
    }

    // ---- Instructions -----------------------------------------------------------------------

    #[inline]
    fn exec<B: Bus>(&mut self, bus: &mut B, op: u8, q_in: u8) -> u32 {
        match op {
            0xCB => self.exec_cb(bus),
            0xED => self.exec_ed(bus),
            0xDD => self.exec_prefixed(bus, Idx::Ix),
            0xFD => self.exec_prefixed(bus, Idx::Iy),
            _ => self.exec_main(bus, op, Idx::Hl, q_in),
        }
    }

    /// After DD or FD. Each prefix is an opcode fetch of its own (4 T-states, one refresh) that
    /// writes no flags, so Q reads 0 after it; in a run of them only the last counts, and no
    /// interrupt is accepted between a prefix and its instruction.
    fn exec_prefixed<B: Bus>(&mut self, bus: &mut B, mut x: Idx) -> u32 {
        let mut t = 4;
        loop {
            let op = self.fetch_op(bus);
            match op {
                0xDD => x = Idx::Ix,
                0xFD => x = Idx::Iy,
                0xCB => return t + self.exec_index_cb(bus, x),
                // No ED instruction uses HL in a way the prefix would change, so it is dropped.
                0xED => return t + self.exec_ed(bus),
                _ => return t + self.exec_main(bus, op, x, 0),
            }
            t += 4;
        }
    }

    /// The unprefixed instructions, and the same under DD or FD (x says which).
    fn exec_main<B: Bus>(&mut self, bus: &mut B, op: u8, x: Idx, q_in: u8) -> u32 {
        let mut t = u32::from(CYCLES[op as usize]);
        match op {
            0x00 => {}
            0x01 | 0x11 | 0x21 | 0x31 => {
                let v = self.imm16(bus);
                self.set_rp(op >> 4, x, v);
            }
            0x02 | 0x12 => {
                // LD (BC),A and LD (DE),A: MEMPTR gets A on top and the address's low byte + 1.
                let addr = if op == 0x02 { self.bc() } else { self.de() };
                bus.write(addr, self.a);
                self.wz = u16::from(self.a) << 8 | (addr.wrapping_add(1) & 0xFF);
            }
            0x0A | 0x1A => {
                let addr = if op == 0x0A { self.bc() } else { self.de() };
                self.a = bus.read(addr);
                self.wz = addr.wrapping_add(1);
            }
            0x03 | 0x13 | 0x23 | 0x33 => {
                let v = self.rp(op >> 4, x).wrapping_add(1);
                self.set_rp(op >> 4, x, v);
            }
            0x0B | 0x1B | 0x2B | 0x3B => {
                let v = self.rp(op >> 4, x).wrapping_sub(1);
                self.set_rp(op >> 4, x, v);
            }
            0x04 | 0x0C | 0x14 | 0x1C | 0x24 | 0x2C | 0x3C => {
                let r = op >> 3;
                let v = self.reg(r, x);
                let v = self.inc8(v);
                self.set_reg(r, x, v);
            }
            0x05 | 0x0D | 0x15 | 0x1D | 0x25 | 0x2D | 0x3D => {
                let r = op >> 3;
                let v = self.reg(r, x);
                let v = self.dec8(v);
                self.set_reg(r, x, v);
            }
            0x34 | 0x35 => {
                let (addr, extra) = self.mem_addr(bus, x);
                t += extra;
                let v = bus.read(addr);
                let v = if op == 0x34 {
                    self.inc8(v)
                } else {
                    self.dec8(v)
                };
                bus.write(addr, v);
            }
            0x06 | 0x0E | 0x16 | 0x1E | 0x26 | 0x2E | 0x3E => {
                let v = self.imm8(bus);
                self.set_reg(op >> 3, x, v);
            }
            0x36 => {
                // LD (HL),n; under a prefix d comes before n and the add overlaps reading n.
                let addr = if x == Idx::Hl {
                    self.hl()
                } else {
                    let d = self.imm8(bus) as i8;
                    let addr = self.hlx(x).wrapping_add(d as u16);
                    self.wz = addr;
                    t += 5;
                    addr
                };
                let v = self.imm8(bus);
                bus.write(addr, v);
            }
            0x07 => {
                // RLCA RRCA RLA RRA: S, Z and P/V are kept, bits 5 and 3 come from the new A.
                self.a = self.a.rotate_left(1);
                let f = (self.f & (SF | ZF | PF)) | (self.a & (YF | XF | CF));
                self.setf(f);
            }
            0x0F => {
                let c = self.a & 1;
                self.a = self.a.rotate_right(1);
                let f = (self.f & (SF | ZF | PF)) | (self.a & (YF | XF)) | c;
                self.setf(f);
            }
            0x17 => {
                let c = self.a >> 7;
                self.a = (self.a << 1) | (self.f & CF);
                let f = (self.f & (SF | ZF | PF)) | (self.a & (YF | XF)) | c;
                self.setf(f);
            }
            0x1F => {
                let c = self.a & 1;
                self.a = (self.a >> 1) | (self.f << 7);
                let f = (self.f & (SF | ZF | PF)) | (self.a & (YF | XF)) | c;
                self.setf(f);
            }
            0x08 => {
                // EX AF,AF' moves F without computing it, so Q stays 0.
                let af = self.af();
                let other = self.af2;
                self.set_af(other);
                self.af2 = af;
            }
            0x09 | 0x19 | 0x29 | 0x39 => {
                let a = self.hlx(x);
                let b = self.rp(op >> 4, x);
                let r = self.add16(a, b);
                self.set_hlx(x, r);
            }
            0x10 => {
                // DJNZ: 8 T-states falling through, 13 jumping.
                let d = self.imm8(bus) as i8;
                self.b = self.b.wrapping_sub(1);
                if self.b != 0 {
                    self.jr(d);
                    t += 5;
                }
            }
            0x18 => {
                let d = self.imm8(bus) as i8;
                self.jr(d);
            }
            0x20 | 0x28 | 0x30 | 0x38 => {
                let d = self.imm8(bus) as i8;
                if self.cond((op >> 3) & 3) {
                    self.jr(d);
                    t += 5;
                }
            }
            0x22 => {
                let addr = self.imm16(bus);
                let v = self.hlx(x);
                self.write16(bus, addr, v);
                self.wz = addr.wrapping_add(1);
            }
            0x2A => {
                let addr = self.imm16(bus);
                let v = self.read16(bus, addr);
                self.set_hlx(x, v);
                self.wz = addr.wrapping_add(1);
            }
            0x32 => {
                let addr = self.imm16(bus);
                bus.write(addr, self.a);
                self.wz = u16::from(self.a) << 8 | (addr.wrapping_add(1) & 0xFF);
            }
            0x3A => {
                let addr = self.imm16(bus);
                self.a = bus.read(addr);
                self.wz = addr.wrapping_add(1);
            }
            0x27 => self.daa(),
            0x2F => {
                self.a = !self.a;
                let f = (self.f & (SF | ZF | PF | CF)) | HF | NF | (self.a & (YF | XF));
                self.setf(f);
            }
            0x37 => {
                // SCF and CCF: bits 5 and 3 from (Q ^ F) | A, Q being what the instruction
                // before wrote (Patrik Rak, on Zilog's NMOS parts).
                let f = (self.f & (SF | ZF | PF)) | CF | (((q_in ^ self.f) | self.a) & (YF | XF));
                self.setf(f);
            }
            0x3F => {
                // CCF: H gets the old carry.
                let c = self.f & CF;
                let f = (self.f & (SF | ZF | PF))
                    | (c << 4)
                    | (c ^ CF)
                    | (((q_in ^ self.f) | self.a) & (YF | XF));
                self.setf(f);
            }
            0x76 => self.halted = true,
            0x40..=0x7F => {
                let dst = (op >> 3) & 7;
                let src = op & 7;
                if src == 6 {
                    // LD r,(IX+d) loads the real H or L, not a half of IX.
                    let (addr, extra) = self.mem_addr(bus, x);
                    t += extra;
                    let v = bus.read(addr);
                    self.set_reg(dst, Idx::Hl, v);
                } else if dst == 6 {
                    let (addr, extra) = self.mem_addr(bus, x);
                    t += extra;
                    let v = self.reg(src, Idx::Hl);
                    bus.write(addr, v);
                } else {
                    let v = self.reg(src, x);
                    self.set_reg(dst, x, v);
                }
            }
            0x80..=0xBF => {
                let src = op & 7;
                let v = if src == 6 {
                    let (addr, extra) = self.mem_addr(bus, x);
                    t += extra;
                    bus.read(addr)
                } else {
                    self.reg(src, x)
                };
                self.alu(op >> 3, v);
            }
            0xC0 | 0xC8 | 0xD0 | 0xD8 | 0xE0 | 0xE8 | 0xF0 | 0xF8 => {
                if self.cond(op >> 3) {
                    self.pc = self.pop(bus);
                    self.wz = self.pc;
                    t += 6;
                }
            }
            0xC1 | 0xD1 | 0xE1 | 0xF1 => {
                // POP AF moves F without computing it, so Q stays 0.
                let v = self.pop(bus);
                self.set_rp2(op >> 4, x, v);
            }
            0xC5 | 0xD5 | 0xE5 | 0xF5 => {
                let v = self.rp2(op >> 4, x);
                self.push(bus, v);
            }
            0xC2 | 0xCA | 0xD2 | 0xDA | 0xE2 | 0xEA | 0xF2 | 0xFA => {
                // JP cc and CALL cc load MEMPTR with the target whether or not they go there.
                let nn = self.imm16(bus);
                self.wz = nn;
                if self.cond(op >> 3) {
                    self.pc = nn;
                }
            }
            0xC3 => {
                let nn = self.imm16(bus);
                self.pc = nn;
                self.wz = nn;
            }
            0xC4 | 0xCC | 0xD4 | 0xDC | 0xE4 | 0xEC | 0xF4 | 0xFC => {
                let nn = self.imm16(bus);
                self.wz = nn;
                if self.cond(op >> 3) {
                    let pc = self.pc;
                    self.push(bus, pc);
                    self.pc = nn;
                    t += 7;
                }
            }
            0xCD => {
                let nn = self.imm16(bus);
                self.wz = nn;
                let pc = self.pc;
                self.push(bus, pc);
                self.pc = nn;
            }
            0xC6 | 0xCE | 0xD6 | 0xDE | 0xE6 | 0xEE | 0xF6 | 0xFE => {
                let v = self.imm8(bus);
                self.alu(op >> 3, v);
            }
            0xC7 | 0xCF | 0xD7 | 0xDF | 0xE7 | 0xEF | 0xF7 | 0xFF => {
                let pc = self.pc;
                self.push(bus, pc);
                self.pc = u16::from(op & 0x38);
                self.wz = self.pc;
            }
            0xC9 => {
                self.pc = self.pop(bus);
                self.wz = self.pc;
            }
            0xD3 => {
                // OUT (n),A: A goes out on the top half of the address too. MEMPTR gets A on top
                // and n + 1 below, without a carry into A.
                let n = self.imm8(bus);
                let port = u16::from(self.a) << 8 | u16::from(n);
                bus.output(port, self.a);
                self.wz = u16::from(self.a) << 8 | u16::from(n.wrapping_add(1));
            }
            0xDB => {
                let n = self.imm8(bus);
                let port = u16::from(self.a) << 8 | u16::from(n);
                self.a = bus.input(port);
                self.wz = port.wrapping_add(1);
            }
            0xD9 => {
                let (bc, de, hl) = (self.bc(), self.de(), self.hl());
                let (bc2, de2, hl2) = (self.bc2, self.de2, self.hl2);
                self.set_bc(bc2);
                self.set_de(de2);
                self.set_hl(hl2);
                self.bc2 = bc;
                self.de2 = de;
                self.hl2 = hl;
            }
            0xE3 => {
                // EX (SP),HL reads both bytes, then writes the high one first. MEMPTR gets the
                // value that came off the stack.
                let sp = self.sp;
                let lo = bus.read(sp);
                let hi = bus.read(sp.wrapping_add(1));
                let v = self.hlx(x);
                bus.write(sp.wrapping_add(1), (v >> 8) as u8);
                bus.write(sp, v as u8);
                let new = u16::from(hi) << 8 | u16::from(lo);
                self.set_hlx(x, new);
                self.wz = new;
            }
            0xE9 => self.pc = self.hlx(x),
            0xEB => {
                // EX DE,HL always means HL: a DD or FD before it changes nothing.
                let de = self.de();
                let hl = self.hl();
                self.set_de(hl);
                self.set_hl(de);
            }
            0xF3 => {
                self.iff1 = false;
                self.iff2 = false;
            }
            0xFB => {
                self.iff1 = true;
                self.iff2 = true;
                self.ei = true;
            }
            0xF9 => self.sp = self.hlx(x),
            // CB, DD, ED and FD never get here: `exec` and `exec_prefixed` take them first.
            _ => {}
        }
        t
    }

    /// CB: rotates and shifts, BIT, RES and SET, on a register or (HL).
    fn exec_cb<B: Bus>(&mut self, bus: &mut B) -> u32 {
        let op = self.fetch_op(bus);
        let n = (op >> 3) & 7;
        let r = op & 7;
        if r == 6 {
            let hl = self.hl();
            let v = bus.read(hl);
            match op >> 6 {
                0 => {
                    let v = self.shift(op, v);
                    bus.write(hl, v);
                    15
                }
                1 => {
                    // BIT n,(HL): bits 5 and 3 come from MEMPTR, the only way a program can see it.
                    let xy = (self.wz >> 8) as u8;
                    self.bit(n, v, xy);
                    12
                }
                2 => {
                    bus.write(hl, v & !(1 << n));
                    15
                }
                _ => {
                    bus.write(hl, v | (1 << n));
                    15
                }
            }
        } else {
            let v = self.reg(r, Idx::Hl);
            match op >> 6 {
                0 => {
                    let v = self.shift(op, v);
                    self.set_reg(r, Idx::Hl, v);
                }
                1 => self.bit(n, v, v),
                2 => self.set_reg(r, Idx::Hl, v & !(1 << n)),
                _ => self.set_reg(r, Idx::Hl, v | (1 << n)),
            }
            8
        }
    }

    /// DD CB d op and FD CB d op. Only the prefix and CB are opcode fetches (two refreshes); d
    /// and op are plain reads. Every op works on (IX+d). Undocumented: the shifts, RES and SET
    /// also copy the result into register r unless r is 6, and all eight BIT encodings are BIT.
    fn exec_index_cb<B: Bus>(&mut self, bus: &mut B, x: Idx) -> u32 {
        let d = self.imm8(bus) as i8;
        let op = self.imm8(bus);
        let addr = self.hlx(x).wrapping_add(d as u16);
        self.wz = addr;
        let v = bus.read(addr);
        let n = (op >> 3) & 7;
        let res = match op >> 6 {
            1 => {
                // BIT n,(IX+d): bits 5 and 3 from the high byte of the address.
                self.bit(n, v, (addr >> 8) as u8);
                return 16;
            }
            0 => self.shift(op, v),
            2 => v & !(1 << n),
            _ => v | (1 << n),
        };
        bus.write(addr, res);
        if op & 7 != 6 {
            self.set_reg(op & 7, Idx::Hl, res);
        }
        19
    }

    /// ED: I/O through C, 16-bit ADC and SBC, the I and R registers, interrupt modes, RETN and
    /// RETI, RLD and RRD, and the block instructions. Every undefined ED opcode is an 8 T-state NOP.
    fn exec_ed<B: Bus>(&mut self, bus: &mut B) -> u32 {
        let op = self.fetch_op(bus);
        match op {
            0x40..=0x7F => match op & 7 {
                0 => {
                    // IN r,(C). r = 6 is IN (C) (also written IN F,(C)): it only sets the flags.
                    let bc = self.bc();
                    let v = bus.input(bc);
                    self.wz = bc.wrapping_add(1);
                    let f = (self.f & CF) | SZXYP[v as usize];
                    self.setf(f);
                    let r = (op >> 3) & 7;
                    if r != 6 {
                        self.set_reg(r, Idx::Hl, v);
                    }
                    12
                }
                1 => {
                    // OUT (C),r. r = 6 is OUT (C),0: an NMOS Z80 puts 0 on the bus.
                    let r = (op >> 3) & 7;
                    let v = if r == 6 { 0 } else { self.reg(r, Idx::Hl) };
                    let bc = self.bc();
                    bus.output(bc, v);
                    self.wz = bc.wrapping_add(1);
                    12
                }
                2 => {
                    let v = self.rp(op >> 4, Idx::Hl);
                    if op & 8 == 0 {
                        self.sbc_hl(v);
                    } else {
                        self.adc_hl(v);
                    }
                    15
                }
                3 => {
                    let addr = self.imm16(bus);
                    if op & 8 == 0 {
                        let v = self.rp(op >> 4, Idx::Hl);
                        self.write16(bus, addr, v);
                    } else {
                        let v = self.read16(bus, addr);
                        self.set_rp(op >> 4, Idx::Hl, v);
                    }
                    self.wz = addr.wrapping_add(1);
                    20
                }
                4 => {
                    // NEG, at all eight of its encodings.
                    let a = self.a;
                    self.a = 0;
                    self.a = self.sub_a(a, 0);
                    8
                }
                5 => {
                    // RETN, and RETI (ED 4D), which does the same: IFF1 gets IFF2 back.
                    self.iff1 = self.iff2;
                    self.pc = self.pop(bus);
                    self.wz = self.pc;
                    14
                }
                6 => {
                    // IM 0, 1, 2; the undocumented ED 4E and 6E are mode 0 too.
                    self.im = match (op >> 3) & 3 {
                        0 | 1 => 0,
                        2 => 1,
                        _ => 2,
                    };
                    8
                }
                _ => match op {
                    0x47 => {
                        self.i = self.a;
                        9
                    }
                    0x4F => {
                        self.r = self.a;
                        9
                    }
                    0x57 | 0x5F => {
                        // LD A,I and LD A,R: P/V gets IFF2. R is read after this instruction's
                        // own two refreshes.
                        self.a = if op == 0x57 { self.i } else { self.r };
                        let pv = if self.iff2 { PF } else { 0 };
                        let f = (self.f & CF) | SZXY[self.a as usize] | pv;
                        self.setf(f);
                        self.p = true;
                        9
                    }
                    0x67 => {
                        // RRD: A's low nibble and (HL)'s two rotate right as one 12-bit value.
                        let hl = self.hl();
                        let v = bus.read(hl);
                        bus.write(hl, (self.a << 4) | (v >> 4));
                        self.a = (self.a & 0xF0) | (v & 0x0F);
                        self.wz = hl.wrapping_add(1);
                        let f = (self.f & CF) | SZXYP[self.a as usize];
                        self.setf(f);
                        18
                    }
                    0x6F => {
                        // RLD: the same, rotating left.
                        let hl = self.hl();
                        let v = bus.read(hl);
                        bus.write(hl, (v << 4) | (self.a & 0x0F));
                        self.a = (self.a & 0xF0) | (v >> 4);
                        self.wz = hl.wrapping_add(1);
                        let f = (self.f & CF) | SZXYP[self.a as usize];
                        self.setf(f);
                        18
                    }
                    // ED 77 and ED 7F do nothing.
                    _ => 8,
                },
            },
            0xA0..=0xBF if op & 0x04 == 0 => self.block(bus, op),
            _ => 8,
        }
    }

    /// LDI LDD LDIR LDDR, CPI CPD CPIR CPDR, INI IND INIR INDR, OUTI OUTD OTIR OTDR: 16
    /// T-states, or 21 when a repeating one goes round again (by stepping PC back over itself).
    fn block<B: Bus>(&mut self, bus: &mut B, op: u8) -> u32 {
        let down = op & 0x08 != 0;
        let repeat = op & 0x10 != 0;
        let step = if down { 0xFFFF } else { 1 };
        match op & 3 {
            0 => {
                // LDI: bits 5 and 3 are bits 1 and 3 of the byte moved plus A.
                let hl = self.hl();
                let de = self.de();
                let v = bus.read(hl);
                bus.write(de, v);
                self.set_hl(hl.wrapping_add(step));
                self.set_de(de.wrapping_add(step));
                let bc = self.bc().wrapping_sub(1);
                self.set_bc(bc);
                let n = v.wrapping_add(self.a);
                let mut f = (self.f & (SF | ZF | CF)) | (n & XF) | ((n << 4) & YF);
                if bc != 0 {
                    f |= PF;
                }
                self.setf(f);
                if repeat && bc != 0 {
                    self.repeat_block();
                    return 21;
                }
            }
            1 => {
                // CPI: a compare with A; bits 5 and 3 are bits 1 and 3 of A - (HL) - H.
                let hl = self.hl();
                let v = bus.read(hl);
                let r = self.a.wrapping_sub(v);
                let h = (self.a ^ v ^ r) & HF;
                let n = r.wrapping_sub(h >> 4);
                self.set_hl(hl.wrapping_add(step));
                self.wz = self.wz.wrapping_add(step);
                let bc = self.bc().wrapping_sub(1);
                self.set_bc(bc);
                let mut f = (self.f & CF)
                    | NF
                    | (SZXY[r as usize] & (SF | ZF))
                    | h
                    | (n & XF)
                    | ((n << 4) & YF);
                if bc != 0 {
                    f |= PF;
                }
                self.setf(f);
                if repeat && bc != 0 && r != 0 {
                    self.repeat_block();
                    return 21;
                }
            }
            2 => {
                // INI: the port is BC before B counts down; MEMPTR is that plus or minus one.
                let bc = self.bc();
                let v = bus.input(bc);
                self.wz = bc.wrapping_add(step);
                self.b = self.b.wrapping_sub(1);
                let hl = self.hl();
                bus.write(hl, v);
                self.set_hl(hl.wrapping_add(step));
                let k = u16::from(v) + u16::from(self.c.wrapping_add(step as u8));
                self.block_io_flags(v, k);
                if repeat && self.b != 0 {
                    self.repeat_block_io(v);
                    return 21;
                }
            }
            _ => {
                // OUTI: B counts down first, so the port is the new BC.
                let hl = self.hl();
                let v = bus.read(hl);
                self.b = self.b.wrapping_sub(1);
                let bc = self.bc();
                bus.output(bc, v);
                self.wz = bc.wrapping_add(step);
                self.set_hl(hl.wrapping_add(step));
                let k = u16::from(v) + u16::from(self.l);
                self.block_io_flags(v, k);
                if repeat && self.b != 0 {
                    self.repeat_block_io(v);
                    return 21;
                }
            }
        }
        16
    }

    /// The flags of INI, IND, OUTI and OUTD: S Z 5 3 from the new B, N from bit 7 of the byte, H
    /// and C from the carry of k (the byte plus C+-1 for input, plus L for output), and P/V the
    /// parity of (k & 7) ^ B.
    fn block_io_flags(&mut self, v: u8, k: u16) {
        let b = self.b;
        let mut f = SZXY[b as usize] | ((v >> 6) & NF);
        if k > 0xFF {
            f |= HF | CF;
        }
        f |= SZXYP[(((k as u8) & 7) ^ b) as usize] & PF;
        self.setf(f);
    }

    /// A repeating block instruction going round again: PC steps back over it and MEMPTR becomes
    /// that address plus one, and bits 5 and 3 come from PC's bits 13 and 11 (David Banks).
    fn repeat_block(&mut self) {
        self.pc = self.pc.wrapping_sub(2);
        self.wz = self.pc.wrapping_add(1);
        let f = (self.f & !(YF | XF)) | ((self.pc >> 8) as u8 & (YF | XF));
        self.setf(f);
    }

    /// INIR, INDR, OTIR and OTDR going round again also change H and P/V, by B and the byte's bit
    /// 7: P/V flips when the three bits worked out below have odd parity.
    fn repeat_block_io(&mut self, v: u8) {
        self.repeat_block();
        let b = self.b;
        let mut f = self.f;
        let x = if f & CF != 0 {
            f &= !HF;
            if v & 0x80 != 0 {
                if b & 0x0F == 0x00 {
                    f |= HF;
                }
                b.wrapping_sub(1) & 7
            } else {
                if b & 0x0F == 0x0F {
                    f |= HF;
                }
                b.wrapping_add(1) & 7
            }
        } else {
            b & 7
        };
        f ^= (SZXYP[x as usize] & PF) ^ PF;
        self.setf(f);
    }
}

/// The interrupts, HALT and the refresh register: neither test suite reaches them, since each
/// runs instructions with nothing interrupting.
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    struct TestBus {
        mem: Vec<u8>,
        /// What the interrupting device puts on the bus, a byte per acknowledge or operand read.
        ack: VecDeque<u8>,
        acks: usize,
    }

    impl TestBus {
        fn new(program: &[(u16, &[u8])]) -> TestBus {
            let mut mem = vec![0; 0x10000];
            for &(at, bytes) in program {
                mem[at as usize..at as usize + bytes.len()].copy_from_slice(bytes);
            }
            TestBus {
                mem,
                ack: VecDeque::new(),
                acks: 0,
            }
        }
        fn word(&self, addr: u16) -> u16 {
            u16::from(self.mem[addr as usize]) | u16::from(self.mem[addr as usize + 1]) << 8
        }
    }

    impl Bus for TestBus {
        fn read(&mut self, addr: u16) -> u8 {
            self.mem[addr as usize]
        }
        fn write(&mut self, addr: u16, val: u8) {
            self.mem[addr as usize] = val;
        }
        fn input(&mut self, _port: u16) -> u8 {
            0xFF
        }
        fn output(&mut self, _port: u16, _val: u8) {}
        fn int_ack(&mut self) -> u8 {
            self.acks += 1;
            self.ack.pop_front().unwrap_or(0xFF)
        }
    }

    fn cpu(im: u8, enabled: bool) -> Z80 {
        let mut z = Z80::new();
        z.sp = 0x8000;
        z.im = im;
        z.iff1 = enabled;
        z.iff2 = enabled;
        z
    }

    #[test]
    fn reset_state() {
        let z = Z80::new();
        assert_eq!(
            (z.pc, z.sp, z.af(), z.i, z.r, z.im),
            (0, 0x0000, 0x0040, 0, 0, 0)
        );
        assert!(!z.iff1 && !z.iff2 && !z.halted);
    }

    #[test]
    fn im1_waits_one_instruction_after_ei_then_calls_0038h() {
        let mut bus = TestBus::new(&[(0, &[0xFB, 0x00, 0x00])]);
        let mut z = cpu(1, false);
        assert_eq!(z.step(&mut bus), 4); // EI
        z.set_irq(true);
        // The instruction after EI runs before the interrupt is accepted.
        assert_eq!(z.step(&mut bus), 4);
        assert_eq!(z.pc, 2);
        // 13 T-states to take it, then the NOP at 0038h.
        assert_eq!(z.step(&mut bus), 13 + 4);
        assert_eq!(z.pc, 0x39);
        assert_eq!((z.sp, bus.word(0x7FFE)), (0x7FFE, 2));
        assert!(!z.iff1 && !z.iff2);
        assert_eq!(z.wz, 0x38);
        assert_eq!(bus.acks, 1);
        // Both flip-flops are clear now, so a held /INT is not taken again.
        assert_eq!(z.step(&mut bus), 4);
        assert_eq!(z.pc, 0x3A);
    }

    #[test]
    fn di_holds_off_the_interrupt() {
        let mut bus = TestBus::new(&[(0, &[0xF3, 0x00])]);
        let mut z = cpu(1, true);
        z.step(&mut bus); // DI
        z.set_irq(true);
        z.step(&mut bus);
        assert_eq!(z.pc, 2);
        assert_eq!(bus.acks, 0);
    }

    #[test]
    fn im2_calls_through_the_table_with_all_eight_vector_bits() {
        // Pac-Man's way: the game writes the vector's low byte to the board, I holds the top.
        let mut bus = TestBus::new(&[(0x4031, &[0x34, 0x12]), (0x1234, &[0x00])]);
        bus.ack.push_back(0x31);
        let mut z = cpu(2, true);
        z.i = 0x40;
        z.pc = 0x0200;
        z.set_irq(true);
        assert_eq!(z.step(&mut bus), 19 + 4);
        assert_eq!(z.pc, 0x1235);
        assert_eq!(bus.word(0x7FFE), 0x0200);
        assert_eq!(z.wz, 0x1234);
    }

    #[test]
    fn im0_runs_the_rst_the_device_supplies() {
        let mut bus = TestBus::new(&[]);
        bus.ack.push_back(0xD7); // RST 10h
        let mut z = cpu(0, true);
        z.pc = 0x0300;
        z.set_irq(true);
        assert_eq!(z.step(&mut bus), 13 + 4);
        assert_eq!(z.pc, 0x11);
        assert_eq!(bus.word(0x7FFE), 0x0300);
    }

    #[test]
    fn im0_takes_every_byte_of_a_call_from_the_device() {
        let mut bus = TestBus::new(&[(0x1234, &[0x00])]);
        bus.ack.extend([0xCD, 0x34, 0x12]); // CALL 1234h
        let mut z = cpu(0, true);
        z.pc = 0x0300;
        z.set_irq(true);
        assert_eq!(z.step(&mut bus), 19 + 4);
        assert_eq!(bus.acks, 3);
        assert_eq!(z.pc, 0x1235);
        assert_eq!(bus.word(0x7FFE), 0x0300);
    }

    #[test]
    fn im0_with_nothing_on_the_bus_is_rst_38h() {
        let mut bus = TestBus::new(&[]);
        let mut z = cpu(0, true);
        z.pc = 0x0300;
        z.set_irq(true);
        z.step(&mut bus);
        assert_eq!(z.pc, 0x39);
    }

    #[test]
    fn nmi_calls_0066h_and_retn_restores_iff1() {
        // RETN at 0066h.
        let mut bus = TestBus::new(&[(0x66, &[0xED, 0x45])]);
        let mut z = cpu(1, true);
        z.pc = 0x0400;
        z.nmi();
        z.set_irq(true);
        // The NMI wins over /INT, then the handler's first instruction (RETN, 14) runs.
        assert_eq!(z.step(&mut bus), 11 + 14);
        assert_eq!(z.pc, 0x0400);
        assert!(z.iff1 && z.iff2, "RETN gives IFF1 back from IFF2");
        assert_eq!(bus.acks, 0);
    }

    #[test]
    fn nmi_is_taken_with_interrupts_disabled() {
        let mut bus = TestBus::new(&[]);
        let mut z = cpu(1, false);
        z.pc = 0x0400;
        z.nmi();
        z.step(&mut bus);
        assert_eq!(z.pc, 0x67);
        assert!(!z.iff1 && !z.iff2);
        assert_eq!(bus.word(0x7FFE), 0x0400);
    }

    #[test]
    fn halt_runs_nops_until_an_interrupt_and_returns_past_itself() {
        let mut bus = TestBus::new(&[(0x100, &[0x76])]);
        let mut z = cpu(1, true);
        z.pc = 0x100;
        assert_eq!(z.step(&mut bus), 4);
        assert!(z.halted);
        assert_eq!(z.pc, 0x101);
        let r = z.r;
        for _ in 0..10 {
            assert_eq!(z.step(&mut bus), 4);
        }
        assert_eq!(z.pc, 0x101);
        assert_eq!(z.r, r + 10, "each NOP of a halt is a refresh");
        z.set_irq(true);
        z.step(&mut bus);
        assert!(!z.halted);
        assert_eq!(bus.word(0x7FFE), 0x101);
    }

    #[test]
    fn nmi_wakes_a_halt() {
        let mut bus = TestBus::new(&[(0x100, &[0x76])]);
        let mut z = cpu(1, false);
        z.pc = 0x100;
        z.step(&mut bus);
        z.nmi();
        z.step(&mut bus);
        assert!(!z.halted);
        assert_eq!(z.pc, 0x67);
        assert_eq!(bus.word(0x7FFE), 0x101);
    }

    #[test]
    fn an_interrupt_right_after_ld_a_i_clears_p_v() {
        let mut bus = TestBus::new(&[(0, &[0xED, 0x57, 0xED, 0x57])]);
        let mut z = cpu(1, true);
        z.step(&mut bus);
        assert_ne!(z.f & PF, 0, "LD A,I copies IFF2 into P/V");
        z.set_irq(true);
        z.step(&mut bus);
        assert_eq!(
            z.f & PF,
            0,
            "taken straight after, the interrupt leaves P/V clear"
        );
        assert_eq!(z.pc, 0x39);
    }

    #[test]
    fn a_run_of_prefixes_is_one_step() {
        // DD DD DD 21 34 12: only the last prefix counts (LD IX,1234h). The Z80 accepts no
        // interrupt between a prefix and its instruction, and a step never stops between them.
        let mut bus = TestBus::new(&[(0, &[0xDD, 0xDD, 0xDD, 0x21, 0x34, 0x12])]);
        let mut z = cpu(1, true);
        assert_eq!(z.step(&mut bus), 4 + 4 + 14);
        assert_eq!((z.ix, z.pc, z.r), (0x1234, 6, 4));
    }

    #[test]
    fn refresh_counts_seven_bits_and_keeps_the_eighth() {
        // LD A,80h; LD R,A; then 200 NOPs.
        let mut bus = TestBus::new(&[(0, &[0x3E, 0x80, 0xED, 0x4F])]);
        let mut z = cpu(0, false);
        z.step(&mut bus);
        z.step(&mut bus);
        assert_eq!(z.r, 0x80);
        for _ in 0..200 {
            z.step(&mut bus);
        }
        assert_eq!(z.r, 0x80 | (200 & 0x7F));
    }

    #[test]
    fn a_save_state_round_trips() {
        let mut bus = TestBus::new(&[(
            0,
            &[0x31, 0x00, 0x90, 0x3E, 0x12, 0xF5, 0xED, 0x5E, 0xFB, 0x76],
        )]);
        let mut z = cpu(0, false);
        for _ in 0..6 {
            z.step(&mut bus);
        }
        z.set_irq(true);
        let bytes = bincode::serialize(&z).unwrap();
        let mut back: Z80 = bincode::deserialize(&bytes).unwrap();
        assert_eq!(back, z);
        let mut bus2 = TestBus {
            mem: bus.mem.clone(),
            ack: VecDeque::new(),
            acks: 0,
        };
        for _ in 0..4 {
            assert_eq!(back.step(&mut bus2), z.step(&mut bus));
        }
        assert_eq!(back, z);
        assert_eq!(bus2.mem, bus.mem);
    }
}
