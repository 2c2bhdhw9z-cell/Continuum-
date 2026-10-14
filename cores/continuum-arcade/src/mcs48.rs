//! The Intel MCS-48 family as cArcade's boards use it: the 8035 that plays Donkey Kong's music
//! and the 8039 (the same chip with 128 bytes of internal RAM instead of 64; the 8048 and 8049
//! are these two with a ROM inside, and Fujitsu's MB8884 and Mitsubishi's M58715 are clones).
//! Every instruction with its machine cycles, both register banks, the eight-level stack in
//! internal RAM, the PSW and its flags, F1, the timer and event counter with their interrupt,
//! the external interrupt, ports 1 and 2, the BUS, the T0 and T1 test inputs, MOVX to external
//! data memory, the 8243 expander instructions, and the two 2 KB program memory banks that
//! SEL MB0 and SEL MB1 choose between.
//!
//! `step` runs one instruction, taking a waiting interrupt first, and returns the machine cycles
//! it took. A machine cycle is 15 oscillator periods (five states of three clocks), so Donkey
//! Kong's 6 MHz 8035 runs 400,000 cycles a second.
//!
//! Written from Intel's MCS-48 Family User's Manual and the 8035/8039/8048/8049 data sheets, and
//! from how MAME's MCS-48 core settles what the manuals leave open: when in an instruction the
//! timer counts, when an interrupt is looked at, and what an undefined opcode does. Proven by
//! tests derived from the data sheets for every opcode (tests/mcs48_opcodes.rs), by comparing
//! traces with MAME's core on programs of our own (tests/mcs48_mame.rs), and by the tests at
//! the end of this file for the interrupts, the timer, the counter and the memory banks.

use serde::{Deserialize, Serialize};

/// Oscillator periods in one machine cycle: the clock is divided by 3 into states, and five
/// states make a cycle.
pub const CLOCKS_PER_CYCLE: u32 = 15;

/// PSW bit 7: carry.
pub const CY: u8 = 0x80;
/// PSW bit 6: auxiliary carry, out of bit 3 (DA A reads it).
pub const AC: u8 = 0x40;
/// PSW bit 5: F0, a flag for the program (CLR F0, CPL F0, JF0).
pub const F0: u8 = 0x20;
/// PSW bit 4: register bank select (SEL RB0, SEL RB1).
pub const BS: u8 = 0x10;
/// PSW bit 3 has no use and always reads 1.
const PSW_UNUSED: u8 = 0x08;

/// Machine cycles of each opcode, from the data sheet's instruction table. Every two-byte
/// instruction takes two, and so do the one-byte ones that use the outside world or the stack:
/// the port and BUS instructions, MOVX, MOVD, ANLD and ORLD, MOVP, MOVP3, JMPP, RET and RETR.
/// The undefined opcodes are one-cycle no-ops, as MAME runs them.
#[rustfmt::skip]
pub static CYCLES: [u8; 256] = [
//  0  1  2  3  4  5  6  7  8  9  A  B  C  D  E  F
    1, 1, 2, 2, 2, 1, 1, 1, 2, 2, 2, 1, 2, 2, 2, 2, // 0
    1, 1, 2, 2, 2, 1, 2, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 1
    1, 1, 1, 2, 2, 1, 2, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 2
    1, 1, 2, 1, 2, 1, 2, 1, 1, 2, 2, 1, 2, 2, 2, 2, // 3
    1, 1, 1, 2, 2, 1, 2, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 4
    1, 1, 2, 2, 2, 1, 2, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 5
    1, 1, 1, 1, 2, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 6
    1, 1, 2, 1, 2, 1, 2, 1, 1, 1, 1, 1, 1, 1, 1, 1, // 7
    2, 2, 1, 2, 2, 1, 2, 1, 2, 2, 2, 1, 2, 2, 2, 2, // 8
    2, 2, 2, 2, 2, 1, 2, 1, 2, 2, 2, 1, 2, 2, 2, 2, // 9
    1, 1, 1, 2, 2, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // A
    2, 2, 2, 2, 2, 1, 2, 1, 2, 2, 2, 2, 2, 2, 2, 2, // B
    1, 1, 1, 1, 2, 1, 2, 1, 1, 1, 1, 1, 1, 1, 1, 1, // C
    1, 1, 2, 2, 2, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, // D
    1, 1, 1, 2, 2, 1, 2, 1, 2, 2, 2, 2, 2, 2, 2, 2, // E
    1, 1, 2, 1, 2, 1, 2, 1, 1, 1, 1, 1, 1, 1, 1, 1, // F
];

/// Which part: how much internal RAM it has. The instruction set is the same.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Model {
    /// 64 bytes: the 8035 and 8048 (and the MB8884). An @R0 or @R1 address above 3Fh reaches
    /// the same byte as its low six bits, since the RAM decodes no more.
    I8035,
    /// 128 bytes: the 8039 and 8049 (and the M58715), decoding seven address bits.
    I8039,
}

impl Model {
    fn ram_size(self) -> usize {
        match self {
            Model::I8035 => 64,
            Model::I8039 => 128,
        }
    }
}

/// What the timer/counter is doing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Count {
    /// STOP TCNT (and reset): the count holds.
    Stopped,
    /// STRT T: one count every 32 machine cycles, through a five-bit prescaler.
    Timer,
    /// STRT CNT: one count for each high-to-low transition of T1, sampled once a cycle.
    Counter,
}

/// What an MCS-48 is wired to on a board. Port 1 and port 2 are quasi-bidirectional: a pin
/// whose latch holds 1 is only pulled up weakly, so the outside world can pull it low, and a
/// read sees the pin. The BUS is a true bidirectional port; on the ROM-less 8035 and 8039 it
/// also carries the program memory's address and data, which the board decodes behind
/// `program`.
pub trait Bus {
    /// A program memory read: an opcode or operand fetch, or the table read of MOVP, MOVP3 or
    /// JMPP. The address is twelve bits; bit 11 is the memory bank.
    fn program(&mut self, addr: u16) -> u8;
    /// MOVX A,@Rr: a read of external data memory at the address in R0 or R1 (/RD strobe).
    fn movx_read(&mut self, _addr: u8) -> u8 {
        0xFF
    }
    /// MOVX @Rr,A: a write to external data memory (/WR strobe).
    fn movx_write(&mut self, _addr: u8, _val: u8) {}
    /// What the outside world does to port 1 or 2 (`port` is 1 or 2): 0 for each pin it pulls
    /// low. The chip reads the pins, which is this ANDed with its own output latch. With nothing
    /// attached the pull-ups read 0xFF.
    fn port_read(&mut self, _port: u8) -> u8 {
        0xFF
    }
    /// Port 1 or 2's output latch changed (or was rewritten with the same value).
    fn port_write(&mut self, _port: u8, _val: u8) {}
    /// INS A,BUS, and the read half of ANL BUS and ORL BUS: the BUS pins under /RD.
    fn bus_read(&mut self) -> u8 {
        0xFF
    }
    /// OUTL BUS,A, and the write half of ANL BUS and ORL BUS: the BUS latched under /WR.
    fn bus_write(&mut self, _val: u8) {}
    /// The T0 test input (JT0, JNT0).
    fn t0(&mut self) -> bool {
        false
    }
    /// The T1 test input (JT1, JNT1, and the event counter).
    fn t1(&mut self) -> bool {
        false
    }
    /// The PROG output, which strobes an 8243 port expander during MOVD, ANLD and ORLD.
    fn prog(&mut self, _level: bool) {}
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mcs48 {
    pub model: Model,
    /// The accumulator.
    pub a: u8,
    /// The 12-bit program counter. Counting only carries through bits 0-10, so a program runs
    /// round its own 2 KB bank; bit 11 changes only by a JMP or CALL (from the memory bank
    /// flip-flop), an interrupt, or a return.
    pub pc: u16,
    /// CY, AC, F0, BS, (bit 3, unused), and the stack pointer in bits 0-2. Bit 3 is kept 0 here
    /// and reads 1 through `psw()` and MOV A,PSW.
    pub psw: u8,
    /// F1, the second flag for the program (CLR F1, CPL F1, JF1). It isn't in the PSW, so the
    /// stack doesn't save it.
    pub f1: bool,
    /// The memory bank flip-flop (SEL MB1 sets it, SEL MB0 clears it): bit 11 of the address a
    /// JMP or CALL goes to, outside an interrupt routine.
    pub mbf: bool,
    /// Internal RAM: R0-R7 of bank 0 at 00h-07h, the stack at 08h-17h, R0-R7 of bank 1 at
    /// 18h-1Fh, and the rest for the program (through @R0 and @R1).
    pub ram: Vec<u8>,
    /// Port 1's output latch.
    pub p1: u8,
    /// Port 2's output latch.
    pub p2: u8,
    /// The 8-bit timer/counter (MOV A,T and MOV T,A).
    pub timer: u8,
    /// The timer's divide-by-32 prescaler. STRT T clears it; nothing else does.
    pub prescaler: u8,
    pub count: Count,
    /// T1 as the counter last sampled it, to find its high-to-low transitions.
    pub t1_last: bool,
    /// The timer flag: set when the timer/counter passes FFh, read and cleared by JTF.
    pub timer_flag: bool,
    /// A timer/counter overflow waiting to interrupt. It is only latched while the timer
    /// interrupt is enabled, and DIS TCNTI throws it away.
    pub timer_irq: bool,
    /// EN TCNTI / DIS TCNTI.
    pub timer_irq_enabled: bool,
    /// EN I / DIS I.
    pub int_enabled: bool,
    /// Inside an interrupt routine, from the call to 003h or 007h until RETR: no other interrupt
    /// is taken, and bit 11 of the program counter is held at 0.
    pub in_irq: bool,
    /// The /INT line, true while the board holds it asserted (low).
    pub irq: bool,
    /// ENT0 CLK has made T0 an output of the clock divided by 3. Only reset turns it off.
    pub t0_clock: bool,
    /// Set by a JNI that saw no interrupt: where it would have jumped. The interrupt logic
    /// samples /INT before JNI tests it, so an interrupt accepted right after such a JNI was
    /// already there when the JNI ran, and the JNI is taken (MAME settles it the same way).
    pub jni_target: Option<u16>,
}

impl Mcs48 {
    /// A chip just powered on: everything zero, then a reset. Internal RAM holds no defined
    /// value on real chips; it starts at 0.
    pub fn new(model: Model) -> Mcs48 {
        let mut m = Mcs48 {
            model,
            a: 0,
            pc: 0,
            psw: 0,
            f1: false,
            mbf: false,
            ram: vec![0; model.ram_size()],
            p1: 0,
            p2: 0,
            timer: 0,
            prescaler: 0,
            count: Count::Stopped,
            t1_last: false,
            timer_flag: false,
            timer_irq: false,
            timer_irq_enabled: false,
            int_enabled: false,
            in_irq: false,
            irq: false,
            t0_clock: false,
            jni_target: None,
        };
        m.reset();
        m
    }

    /// /RESET, as the data sheet lists it: PC to 0, the stack pointer to 0, register bank 0 and
    /// memory bank 0, ports 1 and 2 to input mode (their latches all ones), both interrupts off,
    /// the timer stopped and its flag clear, F0 and F1 clear, and T0's clock output off. The
    /// accumulator, CY, AC, the timer's count and RAM keep what they held. The BUS floats; a
    /// board that latches it sees 0xFF.
    pub fn reset(&mut self) {
        self.pc = 0;
        self.psw &= CY | AC;
        self.f1 = false;
        self.mbf = false;
        self.p1 = 0xFF;
        self.p2 = 0xFF;
        self.count = Count::Stopped;
        self.timer_flag = false;
        self.timer_irq = false;
        self.timer_irq_enabled = false;
        self.int_enabled = false;
        self.in_irq = false;
        self.t0_clock = false;
        self.jni_target = None;
    }

    /// Sets the level of /INT (true: asserted). It is level-triggered: the board holds it until
    /// its device is served, and if it is still held after RETR the interrupt is taken again.
    pub fn set_irq(&mut self, asserted: bool) {
        self.irq = asserted;
    }

    /// The PSW as MOV A,PSW reads it: bit 3 reads 1.
    pub fn psw(&self) -> u8 {
        self.psw | PSW_UNUSED
    }

    /// The stack pointer, PSW bits 0-2: the next free pair of the stack at 08h + 2 × SP.
    pub fn sp(&self) -> u8 {
        self.psw & 7
    }

    /// Register Rn of the selected bank.
    pub fn reg(&self, n: u8) -> u8 {
        self.ram[self.bank() + usize::from(n & 7)]
    }

    pub fn set_reg(&mut self, n: u8, v: u8) {
        let at = self.bank() + usize::from(n & 7);
        self.ram[at] = v;
    }

    fn bank(&self) -> usize {
        if self.psw & BS != 0 {
            0x18
        } else {
            0
        }
    }

    /// A byte of internal RAM through @R0 or @R1: the chip decodes six address bits (8035) or
    /// seven (8039), so higher addresses reach the same bytes again.
    fn ram_index(&self, addr: u8) -> usize {
        usize::from(addr) & (self.ram.len() - 1)
    }

    fn indirect(&self, i: u8) -> u8 {
        self.ram[self.ram_index(self.reg(i))]
    }

    fn set_indirect(&mut self, i: u8, v: u8) {
        let at = self.ram_index(self.reg(i));
        self.ram[at] = v;
    }

    /// Runs one instruction and returns the machine cycles it took. If an interrupt is waiting
    /// it is taken first (two cycles, like the CALL it is), and the first instruction of its
    /// routine runs in the same step: the chip looks at its interrupts only between
    /// instructions.
    pub fn step<B: Bus>(&mut self, bus: &mut B) -> u32 {
        let cycles = self.take_interrupt(bus);
        self.jni_target = None;
        let op = self.fetch(bus);
        cycles + self.exec(bus, op)
    }

    /// The interrupt logic, between instructions. Nothing interrupts an interrupt routine.
    /// The external interrupt comes first and calls 003h; a timer/counter overflow calls 007h,
    /// and taking it clears the overflow it was waiting on.
    fn take_interrupt<B: Bus>(&mut self, bus: &mut B) -> u32 {
        if self.in_irq {
            return 0;
        }
        let external = self.irq && self.int_enabled;
        let timer = self.timer_irq && self.timer_irq_enabled;
        if !(external || timer) {
            return 0;
        }
        self.tick(bus, 2);
        self.in_irq = true;
        if external {
            if let Some(target) = self.jni_target {
                self.pc = target;
            }
            self.push();
            self.pc = 0x003;
        } else {
            self.push();
            self.pc = 0x007;
            self.timer_irq = false;
        }
        2
    }

    /// The timer or counter, run for an instruction's cycles. An instruction's cycles are
    /// counted before it acts, so MOV A,T reads the count its own cycle made and JTF sees an
    /// overflow that happened during it.
    fn tick<B: Bus>(&mut self, bus: &mut B, cycles: u32) {
        let overflow = match self.count {
            Count::Stopped => false,
            Count::Timer => {
                self.prescaler += cycles as u8;
                let carry = self.prescaler >> 5;
                self.prescaler &= 0x1F;
                let (t, over) = self.timer.overflowing_add(carry);
                self.timer = t;
                over
            }
            Count::Counter => {
                let mut over = false;
                for _ in 0..cycles {
                    let t1 = bus.t1();
                    if self.t1_last && !t1 {
                        self.timer = self.timer.wrapping_add(1);
                        over |= self.timer == 0;
                    }
                    self.t1_last = t1;
                }
                over
            }
        };
        if overflow {
            self.timer_flag = true;
            // The overflow only waits as an interrupt if the timer interrupt is on now.
            if self.timer_irq_enabled {
                self.timer_irq = true;
            }
        }
    }

    /// An opcode or operand byte. The program counter's increment stops at bit 10.
    fn fetch<B: Bus>(&mut self, bus: &mut B) -> u8 {
        let v = bus.program(self.pc);
        self.pc = (self.pc & 0x800) | (self.pc.wrapping_add(1) & 0x7FF);
        v
    }

    /// Bit 11 of a JMP or CALL's destination: the memory bank flip-flop, or 0 in an interrupt
    /// routine.
    fn a11(&self) -> u16 {
        if self.mbf && !self.in_irq {
            0x800
        } else {
            0
        }
    }

    /// A CALL or interrupt: the return address and the top four bits of the PSW go to the stack
    /// pair at 08h + 2 × SP (PC bits 0-7 in the first byte; PSW bits 4-7 and PC bits 8-11 in
    /// the second), and SP counts up, round eight levels.
    fn push(&mut self) {
        let sp = self.psw & 7;
        let at = 8 + 2 * usize::from(sp);
        self.ram[at] = self.pc as u8;
        self.ram[at + 1] = (self.psw & 0xF0) | ((self.pc >> 8) as u8 & 0x0F);
        self.psw = (self.psw & 0xF0) | ((sp + 1) & 7);
    }

    /// RET (the PSW stays) or RETR (CY, AC, F0 and BS come back too). Inside an interrupt
    /// routine bit 11 of the return address is held at 0.
    fn pull(&mut self, with_psw: bool) {
        let sp = self.psw.wrapping_sub(1) & 7;
        let at = 8 + 2 * usize::from(sp);
        let lo = self.ram[at];
        let hi = self.ram[at + 1];
        self.pc = u16::from(hi & 0x0F) << 8 | u16::from(lo);
        if self.in_irq {
            self.pc &= 0x7FF;
        }
        let top = if with_psw { hi & 0xF0 } else { self.psw & 0xF0 };
        self.psw = top | sp;
    }

    /// A conditional jump: the second byte replaces the low eight bits of the program counter,
    /// in the page that byte was fetched from. Returns where it goes, taken or not.
    fn branch<B: Bus>(&mut self, bus: &mut B, taken: bool) -> u16 {
        let page = self.pc & 0xF00;
        let target = page | u16::from(self.fetch(bus));
        if taken {
            self.pc = target;
        }
        target
    }

    /// ADD and ADDC: CY out of bit 7, AC out of bit 3. No other flag changes.
    fn add(&mut self, v: u8, carry_in: bool) {
        let c = u16::from(carry_in);
        let sum = u16::from(self.a) + u16::from(v) + c;
        let half = u16::from(self.a & 0x0F) + u16::from(v & 0x0F) + c;
        self.psw &= !(CY | AC);
        if sum > 0xFF {
            self.psw |= CY;
        }
        if half > 0x0F {
            self.psw |= AC;
        }
        self.a = sum as u8;
    }

    /// MOVD, ANLD and ORLD through an 8243: the operation (0 read, 1 write, 2 OR, 3 AND) and the
    /// expander port (4-7 as 0-3) go out on P20-P23, PROG falls to latch them, the four data bits
    /// cross on P20-P23, and PROG rises.
    fn expander<B: Bus>(&mut self, bus: &mut B, op: u8, port: u8) {
        self.p2 = (self.p2 & 0xF0) | (op << 2) | (port & 3);
        bus.port_write(2, self.p2);
        bus.prog(false);
        if op == 0 {
            // P20-P23 go to input mode, and the expander drives them.
            self.p2 |= 0x0F;
            bus.port_write(2, self.p2);
            self.a = bus.port_read(2) & 0x0F;
        } else {
            self.p2 = (self.p2 & 0xF0) | (self.a & 0x0F);
            bus.port_write(2, self.p2);
        }
        bus.prog(true);
    }

    fn exec<B: Bus>(&mut self, bus: &mut B, op: u8) -> u32 {
        let cycles = u32::from(CYCLES[usize::from(op)]);
        self.tick(bus, cycles);
        let r = op & 7;
        let i = op & 1;
        match op {
            0x00 => {}                     // NOP
            0x02 => bus.bus_write(self.a), // OUTL BUS,A
            0x03 => {
                // ADD A,#data
                let v = self.fetch(bus);
                self.add(v, false);
            }
            // JMP addr: bits 5-7 of the opcode are address bits 8-10.
            0x04 | 0x24 | 0x44 | 0x64 | 0x84 | 0xA4 | 0xC4 | 0xE4 => {
                let lo = self.fetch(bus);
                self.pc = self.a11() | u16::from(op >> 5) << 8 | u16::from(lo);
            }
            0x05 => self.int_enabled = true,             // EN I
            0x07 => self.a = self.a.wrapping_sub(1),     // DEC A
            0x08 => self.a = bus.bus_read(),             // INS A,BUS
            0x09 => self.a = bus.port_read(1) & self.p1, // IN A,P1
            0x0A => self.a = bus.port_read(2) & self.p2, // IN A,P2
            0x0C..=0x0F => self.expander(bus, 0, op),    // MOVD A,Pp
            0x10 | 0x11 => {
                // INC @Ri
                let v = self.indirect(i).wrapping_add(1);
                self.set_indirect(i, v);
            }
            // JBb: jump if bit b (opcode bits 5-7) of A is set.
            0x12 | 0x32 | 0x52 | 0x72 | 0x92 | 0xB2 | 0xD2 | 0xF2 => {
                let bit = self.a >> (op >> 5) & 1 != 0;
                self.branch(bus, bit);
            }
            0x13 => {
                // ADDC A,#data
                let v = self.fetch(bus);
                self.add(v, self.psw & CY != 0);
            }
            // CALL addr: the return address and PSW go to the stack first.
            0x14 | 0x34 | 0x54 | 0x74 | 0x94 | 0xB4 | 0xD4 | 0xF4 => {
                let lo = self.fetch(bus);
                self.push();
                self.pc = self.a11() | u16::from(op >> 5) << 8 | u16::from(lo);
            }
            0x15 => self.int_enabled = false, // DIS I
            0x16 => {
                // JTF: jump if the timer flag is set, and clear it.
                let tf = self.timer_flag;
                self.timer_flag = false;
                self.branch(bus, tf);
            }
            0x17 => self.a = self.a.wrapping_add(1), // INC A
            0x18..=0x1F => {
                // INC Rr
                let v = self.reg(r).wrapping_add(1);
                self.set_reg(r, v);
            }
            0x20 | 0x21 => {
                // XCH A,@Ri
                let v = self.indirect(i);
                self.set_indirect(i, self.a);
                self.a = v;
            }
            0x23 => self.a = self.fetch(bus),      // MOV A,#data
            0x25 => self.timer_irq_enabled = true, // EN TCNTI
            0x26 => {
                // JNT0
                let t0 = bus.t0();
                self.branch(bus, !t0);
            }
            0x27 => self.a = 0, // CLR A
            0x28..=0x2F => {
                // XCH A,Rr
                let v = self.reg(r);
                self.set_reg(r, self.a);
                self.a = v;
            }
            0x30 | 0x31 => {
                // XCHD A,@Ri: the low four bits change places.
                let v = self.indirect(i);
                self.set_indirect(i, (v & 0xF0) | (self.a & 0x0F));
                self.a = (self.a & 0xF0) | (v & 0x0F);
            }
            0x35 => {
                // DIS TCNTI: also drops an overflow that was waiting.
                self.timer_irq_enabled = false;
                self.timer_irq = false;
            }
            0x36 => {
                // JT0
                let t0 = bus.t0();
                self.branch(bus, t0);
            }
            0x37 => self.a = !self.a, // CPL A
            0x39 => {
                // OUTL P1,A
                self.p1 = self.a;
                bus.port_write(1, self.p1);
            }
            0x3A => {
                // OUTL P2,A
                self.p2 = self.a;
                bus.port_write(2, self.p2);
            }
            0x3C..=0x3F => self.expander(bus, 1, op), // MOVD Pp,A
            0x40 | 0x41 => self.a |= self.indirect(i), // ORL A,@Ri
            0x42 => self.a = self.timer,              // MOV A,T
            0x43 => self.a |= self.fetch(bus),        // ORL A,#data
            0x45 => {
                // STRT CNT: T1 is sampled now, so a level already low isn't counted.
                if self.count != Count::Counter {
                    self.t1_last = bus.t1();
                }
                self.count = Count::Counter;
            }
            0x46 => {
                // JNT1
                let t1 = bus.t1();
                self.branch(bus, !t1);
            }
            0x47 => self.a = self.a.rotate_left(4), // SWAP A
            0x48..=0x4F => self.a |= self.reg(r),   // ORL A,Rr
            0x50 | 0x51 => self.a &= self.indirect(i), // ANL A,@Ri
            0x53 => self.a &= self.fetch(bus),      // ANL A,#data
            0x55 => {
                // STRT T: the prescaler starts from 0.
                self.count = Count::Timer;
                self.prescaler = 0;
            }
            0x56 => {
                // JT1
                let t1 = bus.t1();
                self.branch(bus, t1);
            }
            0x57 => {
                // DA A: 6 is added to a low digit above 9 (or after AC), then 60h to a high
                // digit above 9 (or after CY). CY is set by either carry and never cleared.
                if self.a & 0x0F > 9 || self.psw & AC != 0 {
                    if self.a > 0xF9 {
                        self.psw |= CY;
                    }
                    self.a = self.a.wrapping_add(0x06);
                }
                if self.a & 0xF0 > 0x90 || self.psw & CY != 0 {
                    self.a = self.a.wrapping_add(0x60);
                    self.psw |= CY;
                }
            }
            0x58..=0x5F => self.a &= self.reg(r), // ANL A,Rr
            0x60 | 0x61 => {
                // ADD A,@Ri
                let v = self.indirect(i);
                self.add(v, false);
            }
            0x62 => self.timer = self.a,         // MOV T,A
            0x65 => self.count = Count::Stopped, // STOP TCNT
            0x67 => {
                // RRC A: through the carry.
                let c = self.a & 1 != 0;
                self.a = self.a >> 1 | (self.psw & CY);
                self.psw = (self.psw & !CY) | if c { CY } else { 0 };
            }
            0x68..=0x6F => {
                // ADD A,Rr
                let v = self.reg(r);
                self.add(v, false);
            }
            0x70 | 0x71 => {
                // ADDC A,@Ri
                let v = self.indirect(i);
                self.add(v, self.psw & CY != 0);
            }
            0x75 => self.t0_clock = true, // ENT0 CLK
            0x76 => {
                // JF1
                let f1 = self.f1;
                self.branch(bus, f1);
            }
            0x77 => self.a = self.a.rotate_right(1), // RR A
            0x78..=0x7F => {
                // ADDC A,Rr
                let v = self.reg(r);
                self.add(v, self.psw & CY != 0);
            }
            0x80 | 0x81 => {
                // MOVX A,@Ri
                let addr = self.reg(i);
                self.a = bus.movx_read(addr);
            }
            0x83 => self.pull(false), // RET
            0x85 => self.psw &= !F0,  // CLR F0
            0x86 => {
                // JNI: jump if /INT is asserted.
                let int = self.irq;
                let target = self.branch(bus, int);
                if !int {
                    self.jni_target = Some(target);
                }
            }
            0x88 => {
                // ORL BUS,#data
                let v = self.fetch(bus);
                let b = bus.bus_read() | v;
                bus.bus_write(b);
            }
            0x89 => {
                // ORL P1,#data
                self.p1 |= self.fetch(bus);
                bus.port_write(1, self.p1);
            }
            0x8A => {
                // ORL P2,#data
                self.p2 |= self.fetch(bus);
                bus.port_write(2, self.p2);
            }
            0x8C..=0x8F => self.expander(bus, 2, op), // ORLD Pp,A
            0x90 | 0x91 => {
                // MOVX @Ri,A
                let addr = self.reg(i);
                bus.movx_write(addr, self.a);
            }
            0x93 => {
                // RETR: the interrupt routine ends, so the return address keeps its bit 11.
                self.in_irq = false;
                self.pull(true);
            }
            0x95 => self.psw ^= F0, // CPL F0
            0x96 => {
                // JNZ
                let nz = self.a != 0;
                self.branch(bus, nz);
            }
            0x97 => self.psw &= !CY, // CLR C
            0x98 => {
                // ANL BUS,#data
                let v = self.fetch(bus);
                let b = bus.bus_read() & v;
                bus.bus_write(b);
            }
            0x99 => {
                // ANL P1,#data
                self.p1 &= self.fetch(bus);
                bus.port_write(1, self.p1);
            }
            0x9A => {
                // ANL P2,#data
                self.p2 &= self.fetch(bus);
                bus.port_write(2, self.p2);
            }
            0x9C..=0x9F => self.expander(bus, 3, op), // ANLD Pp,A
            0xA0 | 0xA1 => self.set_indirect(i, self.a), // MOV @Ri,A
            0xA3 => {
                // MOVP A,@A: from the page the next instruction is in.
                let addr = (self.pc & 0xF00) | u16::from(self.a);
                self.a = bus.program(addr);
            }
            0xA5 => self.f1 = false,                // CLR F1
            0xA7 => self.psw ^= CY,                 // CPL C
            0xA8..=0xAF => self.set_reg(r, self.a), // MOV Rr,A
            0xB0 | 0xB1 => {
                // MOV @Ri,#data
                let v = self.fetch(bus);
                self.set_indirect(i, v);
            }
            0xB3 => {
                // JMPP @A: the low eight bits of the program counter come from the byte at A in
                // the current page.
                let page = self.pc & 0xF00;
                self.pc = page | u16::from(bus.program(page | u16::from(self.a)));
            }
            0xB5 => self.f1 = !self.f1, // CPL F1
            0xB6 => {
                // JF0
                let f0 = self.psw & F0 != 0;
                self.branch(bus, f0);
            }
            0xB8..=0xBF => {
                // MOV Rr,#data
                let v = self.fetch(bus);
                self.set_reg(r, v);
            }
            0xC5 => self.psw &= !BS, // SEL RB0
            0xC6 => {
                // JZ
                let z = self.a == 0;
                self.branch(bus, z);
            }
            0xC7 => self.a = self.psw(), // MOV A,PSW
            0xC8..=0xCF => {
                // DEC Rr
                let v = self.reg(r).wrapping_sub(1);
                self.set_reg(r, v);
            }
            0xD0 | 0xD1 => self.a ^= self.indirect(i), // XRL A,@Ri
            0xD3 => self.a ^= self.fetch(bus),         // XRL A,#data
            0xD5 => self.psw |= BS,                    // SEL RB1
            0xD7 => self.psw = self.a & !PSW_UNUSED,   // MOV PSW,A (the stack pointer too)
            0xD8..=0xDF => self.a ^= self.reg(r),      // XRL A,Rr
            0xE3 => {
                // MOVP3 A,@A: from page 3 of bank 0, whatever the bank.
                self.a = bus.program(0x300 | u16::from(self.a));
            }
            0xE5 => self.mbf = false, // SEL MB0
            0xE6 => {
                // JNC
                let nc = self.psw & CY == 0;
                self.branch(bus, nc);
            }
            0xE7 => self.a = self.a.rotate_left(1), // RL A
            0xE8..=0xEF => {
                // DJNZ Rr,addr
                let v = self.reg(r).wrapping_sub(1);
                self.set_reg(r, v);
                self.branch(bus, v != 0);
            }
            0xF0 | 0xF1 => self.a = self.indirect(i), // MOV A,@Ri
            0xF5 => self.mbf = true,                  // SEL MB1
            0xF6 => {
                // JC
                let c = self.psw & CY != 0;
                self.branch(bus, c);
            }
            0xF7 => {
                // RLC A: through the carry.
                let c = self.a & 0x80 != 0;
                self.a = self.a << 1 | (self.psw & CY != 0) as u8;
                self.psw = (self.psw & !CY) | if c { CY } else { 0 };
            }
            0xF8..=0xFF => self.a = self.reg(r), // MOV A,Rr
            // The 26 undefined opcodes (01, 06, 0B, 22, 33, 38, 3B, 63, 66, 73, 82, 87, 8B,
            // 9B, A2, A6, B7, C0-C3, D6, E0-E2, F3): no operation, one cycle.
            _ => {}
        }
        cycles
    }
}

/// The interrupts, the timer and counter, the memory banks and the RAM sizes: the parts the
/// per-opcode tests can't reach in one instruction.
#[cfg(test)]
mod tests {
    use super::*;

    /// 4 KB of program memory, and a log of what the chip did to the outside world.
    struct TestBus {
        rom: Vec<u8>,
        t1: bool,
        t1_reads: usize,
        p1_writes: Vec<u8>,
    }

    impl TestBus {
        fn new(program: &[(u16, &[u8])]) -> TestBus {
            let mut rom = vec![0; 0x1000];
            for &(at, bytes) in program {
                rom[at as usize..at as usize + bytes.len()].copy_from_slice(bytes);
            }
            TestBus {
                rom,
                t1: false,
                t1_reads: 0,
                p1_writes: Vec::new(),
            }
        }
    }

    impl Bus for TestBus {
        fn program(&mut self, addr: u16) -> u8 {
            self.rom[addr as usize]
        }
        fn t1(&mut self) -> bool {
            self.t1_reads += 1;
            self.t1
        }
        fn port_write(&mut self, port: u8, val: u8) {
            if port == 1 {
                self.p1_writes.push(val);
            }
        }
    }

    fn run(m: &mut Mcs48, bus: &mut TestBus, steps: usize) -> u32 {
        (0..steps).map(|_| m.step(bus)).sum()
    }

    #[test]
    fn reset_state() {
        let mut m = Mcs48::new(Model::I8035);
        assert_eq!(
            (m.pc, m.psw(), m.p1, m.p2, m.ram.len()),
            (0, 0x08, 0xFF, 0xFF, 64)
        );
        assert_eq!(Mcs48::new(Model::I8039).ram.len(), 128);
        // Reset keeps A, CY, AC, the timer's count and RAM; it clears F0, BS, SP, F1, MBF.
        m.a = 0x5A;
        m.psw = !PSW_UNUSED;
        m.timer = 0x33;
        m.f1 = true;
        m.mbf = true;
        m.count = Count::Timer;
        m.int_enabled = true;
        m.ram[0x20] = 0x77;
        m.reset();
        assert_eq!(
            (m.a, m.psw, m.timer, m.ram[0x20]),
            (0x5A, CY | AC, 0x33, 0x77)
        );
        assert!(!m.f1 && !m.mbf && !m.int_enabled && m.count == Count::Stopped);
    }

    #[test]
    fn the_program_counter_wraps_inside_its_2k_bank() {
        // A NOP at 7FFh is followed by 000h, and one at FFFh by 800h.
        let mut bus = TestBus::new(&[]);
        let mut m = Mcs48::new(Model::I8035);
        m.pc = 0x7FF;
        m.step(&mut bus);
        assert_eq!(m.pc, 0x000);
        m.pc = 0xFFF;
        m.step(&mut bus);
        assert_eq!(m.pc, 0x800);
    }

    #[test]
    fn sel_mb1_takes_effect_at_the_next_jmp_or_call() {
        // SEL MB1; NOP; JMP 123h; then at 923h: SEL MB0; CALL 045h, and RET at 045h.
        let mut bus = TestBus::new(&[
            (0x000, &[0xF5, 0x00, 0x24, 0x23]),
            (0x923, &[0xE5, 0x14, 0x45]),
            (0x045, &[0x83]),
        ]);
        let mut m = Mcs48::new(Model::I8035);
        m.step(&mut bus);
        m.step(&mut bus);
        assert_eq!(m.pc, 0x002, "the bank flip-flop alone moves nothing");
        m.step(&mut bus);
        assert_eq!(m.pc, 0x923);
        m.step(&mut bus); // SEL MB0
        m.step(&mut bus); // CALL 045h, now in bank 0
        assert_eq!(m.pc, 0x045);
        m.step(&mut bus); // RET: back to bank 1, all twelve bits from the stack
        assert_eq!(m.pc, 0x926);
    }

    #[test]
    fn conditional_jumps_stay_in_the_page_of_their_second_byte() {
        // JZ at 0FFh: its second byte is at 100h, so it jumps within page 1.
        let mut bus = TestBus::new(&[(0x0FF, &[0xC6, 0x40])]);
        let mut m = Mcs48::new(Model::I8035);
        m.pc = 0x0FF;
        m.step(&mut bus);
        assert_eq!(m.pc, 0x140);
        // JZ at 0FEh: the second byte is at 0FFh, so the jump stays in page 0.
        let mut bus = TestBus::new(&[(0x0FE, &[0xC6, 0x40])]);
        m.pc = 0x0FE;
        m.step(&mut bus);
        assert_eq!(m.pc, 0x040);
    }

    #[test]
    fn the_stack_has_eight_levels_and_wraps() {
        // Nine CALLs to 010h, where each CALL calls 010h again.
        let mut bus = TestBus::new(&[(0x010, &[0x14, 0x10])]);
        let mut m = Mcs48::new(Model::I8035);
        m.pc = 0x010;
        for level in 1..=9 {
            m.step(&mut bus);
            assert_eq!(m.sp(), level & 7);
        }
        // The ninth overwrote the first pair (08h-09h).
        assert_eq!((m.ram[8], m.ram[9]), (0x12, 0x00));
    }

    #[test]
    fn the_external_interrupt_calls_003h_and_retr_restores_the_psw_and_bank() {
        // EN I at 800h in bank 1 (pc 800h), then NOPs; RETR at 003h.
        let mut bus = TestBus::new(&[(0x800, &[0x05, 0x00, 0x00]), (0x003, &[0x93])]);
        let mut m = Mcs48::new(Model::I8035);
        m.pc = 0x800;
        m.mbf = true;
        m.psw = CY | F0 | BS;
        m.set_irq(true);
        assert_eq!(m.step(&mut bus), 1, "EN I itself");
        // The interrupt (2 cycles), then RETR (2), which returns at once.
        assert_eq!(m.step(&mut bus), 2 + 2);
        assert_eq!(m.pc, 0x801);
        assert_eq!(m.psw, CY | F0 | BS);
        assert!(!m.in_irq);
        // /INT is level-triggered: still held, it is taken again.
        m.step(&mut bus);
        assert_eq!(m.pc, 0x801);
        assert_eq!(m.sp(), 0);
        m.set_irq(false);
        m.step(&mut bus);
        assert_eq!(m.pc, 0x802);
    }

    #[test]
    fn an_interrupt_routine_runs_in_bank_0_and_is_not_interrupted() {
        // At 003h: JMP 100h; at 100h: NOP, RETR.
        let mut bus = TestBus::new(&[(0x003, &[0x24, 0x00]), (0x100, &[0x00, 0x93])]);
        let mut m = Mcs48::new(Model::I8035);
        m.pc = 0x900;
        m.mbf = true;
        m.int_enabled = true;
        m.timer_irq_enabled = true;
        m.timer_irq = true;
        m.set_irq(true);
        // The external interrupt wins over the waiting timer one.
        m.step(&mut bus);
        assert_eq!(
            m.pc, 0x100,
            "MB1 is set, but JMP stays in bank 0 in a routine"
        );
        m.step(&mut bus); // NOP: nothing else interrupts the routine
        assert_eq!(m.pc, 0x101);
        assert!(m.timer_irq);
        m.set_irq(false);
        m.step(&mut bus); // RETR, back to bank 1
        assert_eq!(m.pc, 0x900);
        // Now the timer interrupt is taken, and clears its request.
        m.step(&mut bus);
        assert!(!m.timer_irq);
        assert_eq!(m.pc, 0x008);
    }

    #[test]
    fn a_jni_that_missed_the_interrupt_is_taken_when_it_arrives() {
        // JNI 020h at 010h, with /INT clear, then /INT arrives before the next instruction.
        let mut bus = TestBus::new(&[(0x010, &[0x86, 0x20])]);
        let mut m = Mcs48::new(Model::I8035);
        m.pc = 0x010;
        m.int_enabled = true;
        m.step(&mut bus);
        assert_eq!(m.pc, 0x012);
        m.set_irq(true);
        m.step(&mut bus);
        // The routine returns to the JNI's destination.
        assert_eq!((m.ram[8], m.ram[9] & 0x0F), (0x20, 0x00));
    }

    #[test]
    fn the_timer_counts_every_32_cycles_and_interrupts_on_overflow() {
        // STRT T, EN TCNTI, then NOPs, at 100h; RETR at 007h.
        let mut bus = TestBus::new(&[(0x100, &[0x55, 0x25]), (0x007, &[0x93])]);
        let mut m = Mcs48::new(Model::I8035);
        m.pc = 0x100;
        m.timer = 0xFE;
        m.prescaler = 0x1F;
        m.step(&mut bus); // STRT T: the prescaler restarts from 0
        assert_eq!((m.timer, m.prescaler), (0xFE, 0));
        m.step(&mut bus); // EN TCNTI
                          // NOPs from 102h: cycle 32 since STRT T counts FEh to FFh, cycle 64 overflows.
        let mut cycles = 1; // EN TCNTI's own cycle
        while m.timer == 0xFE {
            cycles += m.step(&mut bus);
        }
        assert_eq!(cycles, 32);
        while m.timer == 0xFF {
            cycles += m.step(&mut bus);
        }
        assert_eq!(cycles, 64);
        assert!(m.timer_flag && m.timer_irq);
        let pc = m.pc;
        m.step(&mut bus); // the interrupt, and RETR
        assert_eq!(m.pc, pc);
        assert!(!m.timer_irq && m.timer_flag, "JTF still sees the flag");
    }

    #[test]
    fn an_overflow_with_the_timer_interrupt_off_is_not_kept() {
        let mut bus = TestBus::new(&[(0x000, &[0x55])]);
        let mut m = Mcs48::new(Model::I8035);
        m.timer = 0xFF;
        m.step(&mut bus);
        run(&mut m, &mut bus, 40);
        assert!(m.timer_flag);
        assert!(!m.timer_irq);
        // Turning the interrupt on afterwards doesn't bring it back.
        m.timer_irq_enabled = true;
        let pc = m.pc;
        m.step(&mut bus);
        assert_eq!(m.pc, pc + 1);
    }

    #[test]
    fn the_counter_counts_t1_falling_edges() {
        // STRT CNT, then NOPs.
        let mut bus = TestBus::new(&[(0x000, &[0x45])]);
        let mut m = Mcs48::new(Model::I8035);
        bus.t1 = false;
        m.step(&mut bus);
        for _ in 0..5 {
            bus.t1 = true;
            m.step(&mut bus);
            bus.t1 = false;
            m.step(&mut bus);
        }
        assert_eq!(m.timer, 5);
        // A level that stays low counts nothing.
        run(&mut m, &mut bus, 10);
        assert_eq!(m.timer, 5);
        // STOP TCNT stops it; T1 is no longer sampled.
        bus.rom[m.pc as usize] = 0x65;
        m.step(&mut bus);
        let reads = bus.t1_reads;
        run(&mut m, &mut bus, 10);
        assert_eq!(bus.t1_reads, reads);
    }

    #[test]
    fn strt_cnt_does_not_count_a_level_that_was_already_low() {
        // T1 high, STRT CNT samples it; T1 goes low during the NOP: one count. Starting again
        // with T1 low counts nothing.
        let mut bus = TestBus::new(&[(0x000, &[0x45, 0x00, 0x65, 0x45, 0x00])]);
        let mut m = Mcs48::new(Model::I8035);
        bus.t1 = true;
        m.step(&mut bus);
        bus.t1 = false;
        m.step(&mut bus);
        assert_eq!(m.timer, 1);
        m.step(&mut bus); // STOP TCNT
        m.step(&mut bus); // STRT CNT with T1 low
        m.step(&mut bus);
        assert_eq!(m.timer, 1);
    }

    #[test]
    fn the_8035_mirrors_its_64_bytes_and_the_8039_does_not() {
        // MOV R0,#45h; MOV @R0,#99h
        let program: &[(u16, &[u8])] = &[(0x000, &[0xB8, 0x45, 0xB0, 0x99])];
        let mut m = Mcs48::new(Model::I8035);
        run(&mut m, &mut TestBus::new(program), 2);
        assert_eq!(m.ram[0x05], 0x99);
        let mut m = Mcs48::new(Model::I8039);
        run(&mut m, &mut TestBus::new(program), 2);
        assert_eq!((m.ram[0x45], m.ram[0x05]), (0x99, 0));
    }

    #[test]
    fn a_save_state_round_trips() {
        // EN TCNTI; STRT T; MOV A,#3; OUTL P1,A; INC A; JMP 004h
        let mut bus = TestBus::new(&[(0x000, &[0x25, 0x55, 0x23, 0x03, 0x39, 0x17, 0x04, 0x04])]);
        let mut m = Mcs48::new(Model::I8039);
        run(&mut m, &mut bus, 50);
        let bytes = bincode::serialize(&m).unwrap();
        let mut back: Mcs48 = bincode::deserialize(&bytes).unwrap();
        assert_eq!(back, m);
        let mut bus2 = TestBus::new(&[]);
        bus2.rom = bus.rom.clone();
        for _ in 0..500 {
            assert_eq!(back.step(&mut bus2), m.step(&mut bus));
        }
        assert_eq!(back, m);
        assert_eq!(
            bus2.p1_writes,
            bus.p1_writes[bus.p1_writes.len() - bus2.p1_writes.len()..]
        );
    }
}
