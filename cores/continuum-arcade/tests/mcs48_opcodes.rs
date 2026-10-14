//! Every one of the MCS-48's 256 opcodes against Intel's MCS-48 Family User's Manual (1980):
//! each opcode's mnemonic, length and machine cycles from the manual's instruction table, and
//! what it does from the operation the manual gives for it ("(A) ← (A) + (Rr)", and so on).
//! `datasheet` below is that description written out on its own, keyed by the mnemonic's text
//! rather than by opcode bits, and every opcode is run on the chip and on it from thousands of
//! random machine states, on both RAM sizes. Every register, flag, byte of RAM, port latch and
//! timer bit must come out the same, and so must every access to the outside world, in order.
//!
//! Two things the manual leaves open follow MAME's MCS-48 core, as src/mcs48.rs does: an
//! instruction's cycles are counted by the timer before the instruction acts, and the
//! undefined opcodes are one-cycle no-ops. Bits of @R0 and @R1 above the RAM size are not
//! decoded.
//!
//! The table's encodings are checked against an independent assembler: tests/mcs48/opcodes.asm
//! (written from the table by `MCS48_WRITE_ASM=1 cargo test --test mcs48_opcodes`) has every
//! instruction at twice its opcode, and tests/mcs48/opcodes.bin is what The Macro Assembler AS
//! made of it (`asl -cpu 8048 opcodes.asm && p2bin opcodes.p opcodes.bin`).

use std::path::PathBuf;

use continuum_arcade::mcs48::{Bus, Count, Mcs48, Model};

/// The manual's instruction table, one row per opcode: the mnemonic, and its machine cycles.
/// "#data" is an immediate byte, "addr" a jump destination's low byte, "xx" the low byte of a
/// JMP or CALL destination whose bits 8-10 are the digit before it. "?" is an undefined opcode.
#[rustfmt::skip]
static TABLE: [(&str, u32); 256] = [
    ("NOP", 1), ("?", 1), ("OUTL BUS,A", 2), ("ADD A,#data", 2),
    ("JMP 0xx", 2), ("EN I", 1), ("?", 1), ("DEC A", 1),
    ("INS A,BUS", 2), ("IN A,P1", 2), ("IN A,P2", 2), ("?", 1),
    ("MOVD A,P4", 2), ("MOVD A,P5", 2), ("MOVD A,P6", 2), ("MOVD A,P7", 2),
    ("INC @R0", 1), ("INC @R1", 1), ("JB0 addr", 2), ("ADDC A,#data", 2),
    ("CALL 0xx", 2), ("DIS I", 1), ("JTF addr", 2), ("INC A", 1),
    ("INC R0", 1), ("INC R1", 1), ("INC R2", 1), ("INC R3", 1),
    ("INC R4", 1), ("INC R5", 1), ("INC R6", 1), ("INC R7", 1),
    ("XCH A,@R0", 1), ("XCH A,@R1", 1), ("?", 1), ("MOV A,#data", 2),
    ("JMP 1xx", 2), ("EN TCNTI", 1), ("JNT0 addr", 2), ("CLR A", 1),
    ("XCH A,R0", 1), ("XCH A,R1", 1), ("XCH A,R2", 1), ("XCH A,R3", 1),
    ("XCH A,R4", 1), ("XCH A,R5", 1), ("XCH A,R6", 1), ("XCH A,R7", 1),
    ("XCHD A,@R0", 1), ("XCHD A,@R1", 1), ("JB1 addr", 2), ("?", 1),
    ("CALL 1xx", 2), ("DIS TCNTI", 1), ("JT0 addr", 2), ("CPL A", 1),
    ("?", 1), ("OUTL P1,A", 2), ("OUTL P2,A", 2), ("?", 1),
    ("MOVD P4,A", 2), ("MOVD P5,A", 2), ("MOVD P6,A", 2), ("MOVD P7,A", 2),
    ("ORL A,@R0", 1), ("ORL A,@R1", 1), ("MOV A,T", 1), ("ORL A,#data", 2),
    ("JMP 2xx", 2), ("STRT CNT", 1), ("JNT1 addr", 2), ("SWAP A", 1),
    ("ORL A,R0", 1), ("ORL A,R1", 1), ("ORL A,R2", 1), ("ORL A,R3", 1),
    ("ORL A,R4", 1), ("ORL A,R5", 1), ("ORL A,R6", 1), ("ORL A,R7", 1),
    ("ANL A,@R0", 1), ("ANL A,@R1", 1), ("JB2 addr", 2), ("ANL A,#data", 2),
    ("CALL 2xx", 2), ("STRT T", 1), ("JT1 addr", 2), ("DA A", 1),
    ("ANL A,R0", 1), ("ANL A,R1", 1), ("ANL A,R2", 1), ("ANL A,R3", 1),
    ("ANL A,R4", 1), ("ANL A,R5", 1), ("ANL A,R6", 1), ("ANL A,R7", 1),
    ("ADD A,@R0", 1), ("ADD A,@R1", 1), ("MOV T,A", 1), ("?", 1),
    ("JMP 3xx", 2), ("STOP TCNT", 1), ("?", 1), ("RRC A", 1),
    ("ADD A,R0", 1), ("ADD A,R1", 1), ("ADD A,R2", 1), ("ADD A,R3", 1),
    ("ADD A,R4", 1), ("ADD A,R5", 1), ("ADD A,R6", 1), ("ADD A,R7", 1),
    ("ADDC A,@R0", 1), ("ADDC A,@R1", 1), ("JB3 addr", 2), ("?", 1),
    ("CALL 3xx", 2), ("ENT0 CLK", 1), ("JF1 addr", 2), ("RR A", 1),
    ("ADDC A,R0", 1), ("ADDC A,R1", 1), ("ADDC A,R2", 1), ("ADDC A,R3", 1),
    ("ADDC A,R4", 1), ("ADDC A,R5", 1), ("ADDC A,R6", 1), ("ADDC A,R7", 1),
    ("MOVX A,@R0", 2), ("MOVX A,@R1", 2), ("?", 1), ("RET", 2),
    ("JMP 4xx", 2), ("CLR F0", 1), ("JNI addr", 2), ("?", 1),
    ("ORL BUS,#data", 2), ("ORL P1,#data", 2), ("ORL P2,#data", 2), ("?", 1),
    ("ORLD P4,A", 2), ("ORLD P5,A", 2), ("ORLD P6,A", 2), ("ORLD P7,A", 2),
    ("MOVX @R0,A", 2), ("MOVX @R1,A", 2), ("JB4 addr", 2), ("RETR", 2),
    ("CALL 4xx", 2), ("CPL F0", 1), ("JNZ addr", 2), ("CLR C", 1),
    ("ANL BUS,#data", 2), ("ANL P1,#data", 2), ("ANL P2,#data", 2), ("?", 1),
    ("ANLD P4,A", 2), ("ANLD P5,A", 2), ("ANLD P6,A", 2), ("ANLD P7,A", 2),
    ("MOV @R0,A", 1), ("MOV @R1,A", 1), ("?", 1), ("MOVP A,@A", 2),
    ("JMP 5xx", 2), ("CLR F1", 1), ("?", 1), ("CPL C", 1),
    ("MOV R0,A", 1), ("MOV R1,A", 1), ("MOV R2,A", 1), ("MOV R3,A", 1),
    ("MOV R4,A", 1), ("MOV R5,A", 1), ("MOV R6,A", 1), ("MOV R7,A", 1),
    ("MOV @R0,#data", 2), ("MOV @R1,#data", 2), ("JB5 addr", 2), ("JMPP @A", 2),
    ("CALL 5xx", 2), ("CPL F1", 1), ("JF0 addr", 2), ("?", 1),
    ("MOV R0,#data", 2), ("MOV R1,#data", 2), ("MOV R2,#data", 2), ("MOV R3,#data", 2),
    ("MOV R4,#data", 2), ("MOV R5,#data", 2), ("MOV R6,#data", 2), ("MOV R7,#data", 2),
    ("?", 1), ("?", 1), ("?", 1), ("?", 1),
    ("JMP 6xx", 2), ("SEL RB0", 1), ("JZ addr", 2), ("MOV A,PSW", 1),
    ("DEC R0", 1), ("DEC R1", 1), ("DEC R2", 1), ("DEC R3", 1),
    ("DEC R4", 1), ("DEC R5", 1), ("DEC R6", 1), ("DEC R7", 1),
    ("XRL A,@R0", 1), ("XRL A,@R1", 1), ("JB6 addr", 2), ("XRL A,#data", 2),
    ("CALL 6xx", 2), ("SEL RB1", 1), ("?", 1), ("MOV PSW,A", 1),
    ("XRL A,R0", 1), ("XRL A,R1", 1), ("XRL A,R2", 1), ("XRL A,R3", 1),
    ("XRL A,R4", 1), ("XRL A,R5", 1), ("XRL A,R6", 1), ("XRL A,R7", 1),
    ("?", 1), ("?", 1), ("?", 1), ("MOVP3 A,@A", 2),
    ("JMP 7xx", 2), ("SEL MB0", 1), ("JNC addr", 2), ("RL A", 1),
    ("DJNZ R0,addr", 2), ("DJNZ R1,addr", 2), ("DJNZ R2,addr", 2), ("DJNZ R3,addr", 2),
    ("DJNZ R4,addr", 2), ("DJNZ R5,addr", 2), ("DJNZ R6,addr", 2), ("DJNZ R7,addr", 2),
    ("MOV A,@R0", 1), ("MOV A,@R1", 1), ("JB7 addr", 2), ("?", 1),
    ("CALL 7xx", 2), ("SEL MB1", 1), ("JC addr", 2), ("RLC A", 1),
    ("MOV A,R0", 1), ("MOV A,R1", 1), ("MOV A,R2", 1), ("MOV A,R3", 1),
    ("MOV A,R4", 1), ("MOV A,R5", 1), ("MOV A,R6", 1), ("MOV A,R7", 1),
];

/// An instruction's length: two bytes if it has an immediate or an address byte.
fn length(text: &str) -> u16 {
    if text.contains("#data") || text.contains("addr") || text.contains("xx") {
        2
    } else {
        1
    }
}

/// What the chip did to the outside world.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Event {
    Program(u16),
    MovxRead(u8),
    MovxWrite(u8, u8),
    PortRead(u8),
    PortWrite(u8, u8),
    BusRead,
    BusWrite(u8),
    Prog(bool),
}

/// The outside world for one case: random program memory and inputs that hold still through
/// the instruction, and a log of every access.
#[derive(Clone)]
struct World {
    rom: Vec<u8>,
    movx: Vec<u8>,
    /// What the outside world does to ports 1 and 2 (index 1 and 2).
    ports: [u8; 3],
    bus_in: u8,
    t0: bool,
    t1: bool,
    log: Vec<Event>,
}

impl Bus for World {
    fn program(&mut self, addr: u16) -> u8 {
        assert!(
            addr < 0x1000,
            "program address {addr:#x} has more than twelve bits"
        );
        self.log.push(Event::Program(addr));
        self.rom[addr as usize]
    }
    fn movx_read(&mut self, addr: u8) -> u8 {
        self.log.push(Event::MovxRead(addr));
        self.movx[addr as usize]
    }
    fn movx_write(&mut self, addr: u8, val: u8) {
        self.log.push(Event::MovxWrite(addr, val));
    }
    fn port_read(&mut self, port: u8) -> u8 {
        self.log.push(Event::PortRead(port));
        self.ports[port as usize]
    }
    fn port_write(&mut self, port: u8, val: u8) {
        self.log.push(Event::PortWrite(port, val));
    }
    fn bus_read(&mut self) -> u8 {
        self.log.push(Event::BusRead);
        self.bus_in
    }
    fn bus_write(&mut self, val: u8) {
        self.log.push(Event::BusWrite(val));
    }
    fn t0(&mut self) -> bool {
        self.t0
    }
    fn t1(&mut self) -> bool {
        self.t1
    }
    fn prog(&mut self, level: bool) {
        self.log.push(Event::Prog(level));
    }
}

/// xorshift64*, so every run tests the same states.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn byte(&mut self) -> u8 {
        // A quarter of the bytes are values where carries, digits and signs change.
        let special = [
            0x00, 0x01, 0x09, 0x0A, 0x0F, 0x10, 0x7F, 0x80, 0x90, 0x99, 0x9A, 0xF9, 0xFA, 0xFF,
        ];
        let r = self.next();
        if r & 3 == 0 {
            special[(r >> 8) as usize % special.len()]
        } else {
            (r >> 32) as u8
        }
    }
    fn bit(&mut self) -> bool {
        self.next() & 0x100 != 0
    }
}

fn random_case(rng: &mut Rng, model: Model, op: u8) -> (Mcs48, World) {
    let mut m = Mcs48::new(model);
    m.a = rng.byte();
    m.pc = (rng.next() >> 20) as u16 & 0xFFF;
    m.psw = rng.byte() & !0x08;
    m.f1 = rng.bit();
    m.mbf = rng.bit();
    for b in m.ram.iter_mut() {
        *b = rng.byte();
    }
    m.p1 = rng.byte();
    m.p2 = rng.byte();
    m.timer = rng.byte();
    m.prescaler = (rng.next() >> 40) as u8 & 0x1F;
    m.count = [Count::Stopped, Count::Timer, Count::Counter][(rng.next() >> 33) as usize % 3];
    m.t1_last = rng.bit();
    m.timer_flag = rng.bit();
    m.timer_irq = rng.bit();
    m.timer_irq_enabled = rng.bit();
    m.int_enabled = rng.bit();
    m.in_irq = rng.bit();
    m.irq = rng.bit();
    m.t0_clock = rng.bit();
    m.jni_target = if rng.bit() {
        Some((rng.next() >> 20) as u16 & 0xFFF)
    } else {
        None
    };
    // An interrupt taken first would run the vector's instruction instead: leave none waiting.
    if !m.in_irq && ((m.irq && m.int_enabled) || (m.timer_irq && m.timer_irq_enabled)) {
        m.irq = false;
        m.timer_irq = false;
    }
    let mut w = World {
        rom: (0..0x1000).map(|_| rng.byte()).collect(),
        movx: (0..256).map(|_| rng.byte()).collect(),
        ports: [0xFF, rng.byte(), rng.byte()],
        bus_in: rng.byte(),
        t0: rng.bit(),
        t1: rng.bit(),
        log: Vec::new(),
    };
    w.rom[m.pc as usize] = op;
    (m, w)
}

// ---------------------------------------------------------------------------------------------
// The manual's description of each instruction, written out on its own.
// ---------------------------------------------------------------------------------------------

/// "Bit 11 is not altered by normal incrementing of the program counter."
fn next_pc(pc: u16) -> u16 {
    (pc & 0x800) | ((pc + 1) & 0x7FF)
}

fn operand(m: &mut Mcs48, w: &mut World) -> u8 {
    let v = w.program(m.pc);
    m.pc = next_pc(m.pc);
    v
}

/// "Bank 1 ... registers R0-R7 at locations 24-31" when PSW bit 4 is set.
fn reg_at(m: &Mcs48, n: u8) -> usize {
    let base = if m.psw & 0x10 != 0 { 24 } else { 0 };
    base + usize::from(n)
}

fn ram_at(m: &Mcs48, addr: u8) -> usize {
    usize::from(addr) % m.ram.len()
}

fn reg_number(arg: &str) -> u8 {
    arg.trim_start_matches('@')
        .trim_start_matches('R')
        .parse()
        .unwrap()
}

/// The value an operand names.
fn source(m: &mut Mcs48, w: &mut World, arg: &str) -> u8 {
    match arg {
        "#data" => operand(m, w),
        "A" => m.a,
        "T" => m.timer,
        // "Bit 3 ... unused" reads as one.
        "PSW" => m.psw | 0x08,
        _ if arg.starts_with("@R") => {
            let at = ram_at(m, m.ram[reg_at(m, reg_number(arg))]);
            m.ram[at]
        }
        _ if arg.starts_with('R') => m.ram[reg_at(m, reg_number(arg))],
        _ => panic!("no source {arg}"),
    }
}

/// Stores into the register or data memory location an operand names.
fn store(m: &mut Mcs48, arg: &str, v: u8) {
    if arg.starts_with("@R") {
        let at = ram_at(m, m.ram[reg_at(m, reg_number(arg))]);
        m.ram[at] = v;
    } else {
        let at = reg_at(m, reg_number(arg));
        m.ram[at] = v;
    }
}

/// "(A) ← (A) + data [+ (C)]": C from bit 7, AC from bit 3.
fn add(m: &mut Mcs48, v: u8, with_carry: bool) {
    let c = u32::from(with_carry && m.psw & 0x80 != 0);
    let full = u32::from(m.a) + u32::from(v) + c;
    let low = u32::from(m.a & 15) + u32::from(v & 15) + c;
    m.psw = (m.psw & 0x3F) | if full > 255 { 0x80 } else { 0 } | if low > 15 { 0x40 } else { 0 };
    m.a = full as u8;
}

/// "((SP)) ← (PC), (PSW 4-7); (SP) ← (SP) + 1": two bytes per level at 8 + 2 × SP.
fn push(m: &mut Mcs48) {
    let sp = usize::from(m.psw & 7);
    m.ram[8 + 2 * sp] = (m.pc & 0xFF) as u8;
    m.ram[9 + 2 * sp] = (m.psw & 0xF0) | ((m.pc >> 8) & 15) as u8;
    m.psw = (m.psw & 0xF0) | ((sp as u8 + 1) % 8);
}

/// "(SP) ← (SP) − 1; (PC) ← ((SP))", and for RETR "(PSW 4-7) ← ((SP))".
fn pop(m: &mut Mcs48, psw_too: bool) {
    let sp = ((m.psw & 7) + 7) & 7;
    let lo = m.ram[8 + 2 * usize::from(sp)];
    let hi = m.ram[9 + 2 * usize::from(sp)];
    m.pc = u16::from(hi & 15) * 256 + u16::from(lo);
    // "Bit 11 of the program counter is held at 0 during the interrupt service routine."
    if m.in_irq {
        m.pc &= 0x7FF;
    }
    m.psw = if psw_too { hi & 0xF0 } else { m.psw & 0xF0 } | sp;
}

/// "The setting of PC bit 11 is determined by the most recent SELECT MB instruction", except in
/// an interrupt routine.
fn bank_bit(m: &Mcs48) -> u16 {
    if m.mbf && !m.in_irq {
        0x800
    } else {
        0
    }
}

/// The 8243 port expander: P20-P23 carry the operation and the port as PROG falls, then four
/// bits of data, until PROG rises.
fn expander(m: &mut Mcs48, w: &mut World, code: u8, port: &str) {
    let p = port.trim_start_matches('P').parse::<u8>().unwrap() - 4;
    m.p2 = (m.p2 & 0xF0) | (code << 2) | p;
    w.port_write(2, m.p2);
    w.prog(false);
    if code == 0 {
        // "Accumulator bits 4-7 are zeroed."
        m.p2 |= 0x0F;
        w.port_write(2, m.p2);
        m.a = w.port_read(2) & 0x0F;
    } else {
        m.p2 = (m.p2 & 0xF0) | (m.a & 0x0F);
        w.port_write(2, m.p2);
    }
    w.prog(true);
}

/// The timer/counter over the instruction's cycles. "The register is incremented every 32
/// instruction cycles" through the prescaler; as a counter, on "high to low transitions on T1".
/// An overflow sets the timer flag, and "while timer interrupts are disabled the timer
/// overflow f/f will not store any overflow".
fn count(m: &mut Mcs48, w: &World, cycles: u32) {
    let mut overflow = false;
    match m.count {
        Count::Stopped => {}
        Count::Timer => {
            let total = u32::from(m.prescaler) + cycles;
            m.prescaler = (total % 32) as u8;
            for _ in 0..total / 32 {
                m.timer = m.timer.wrapping_add(1);
                overflow |= m.timer == 0;
            }
        }
        Count::Counter => {
            for _ in 0..cycles {
                if m.t1_last && !w.t1 {
                    m.timer = m.timer.wrapping_add(1);
                    overflow |= m.timer == 0;
                }
                m.t1_last = w.t1;
            }
        }
    }
    if overflow {
        m.timer_flag = true;
        if m.timer_irq_enabled {
            m.timer_irq = true;
        }
    }
}

/// One instruction from the manual's description. Returns its cycles from the table. The
/// rotates are written as the manual draws them, bits moving round, on purpose not the way
/// src/mcs48.rs does them.
#[allow(clippy::manual_rotate)]
fn datasheet(m: &mut Mcs48, w: &mut World) -> u32 {
    m.jni_target = None;
    let op = operand(m, w);
    let (text, cycles) = TABLE[usize::from(op)];
    count(m, w, cycles);
    let (name, rest) = text.split_once(' ').unwrap_or((text, ""));
    let args: Vec<&str> = if rest.is_empty() {
        Vec::new()
    } else {
        rest.split(',').collect()
    };
    // A conditional jump: "(PC 0-7) ← addr" if the condition holds, in the page of the address
    // byte (an instruction at location 255 of a page jumps into the next page).
    let jump_if = |m: &mut Mcs48, w: &mut World, cond: bool| -> u16 {
        let page = m.pc & 0xF00;
        let target = page | u16::from(operand(m, w));
        if cond {
            m.pc = target;
        }
        target
    };
    match (name, args.as_slice()) {
        ("?", []) | ("NOP", []) => {}
        ("ADD", ["A", src]) => {
            let v = source(m, w, src);
            add(m, v, false);
        }
        ("ADDC", ["A", src]) => {
            let v = source(m, w, src);
            add(m, v, true);
        }
        ("ANL" | "ORL" | "XRL", ["A", src]) => {
            let v = source(m, w, src);
            m.a = match name {
                "ANL" => m.a & v,
                "ORL" => m.a | v,
                _ => m.a ^ v,
            };
        }
        ("ANL" | "ORL", ["BUS", "#data"]) => {
            let mask = operand(m, w);
            let bus = w.bus_read();
            w.bus_write(if name == "ANL" {
                bus & mask
            } else {
                bus | mask
            });
        }
        ("ANL" | "ORL", [port, "#data"]) => {
            let mask = operand(m, w);
            let p = if *port == "P1" { 1 } else { 2 };
            let latch = if p == 1 { &mut m.p1 } else { &mut m.p2 };
            *latch = if name == "ANL" {
                *latch & mask
            } else {
                *latch | mask
            };
            let v = *latch;
            w.port_write(p, v);
        }
        ("MOVD", ["A", port]) => expander(m, w, 0, port),
        ("MOVD", [port, "A"]) => expander(m, w, 1, port),
        ("ORLD", [port, "A"]) => expander(m, w, 2, port),
        ("ANLD", [port, "A"]) => expander(m, w, 3, port),
        ("JMP" | "CALL", [dest]) => {
            let high = u16::from(dest.as_bytes()[0] - b'0') << 8;
            let low = u16::from(operand(m, w));
            if name == "CALL" {
                push(m);
            }
            m.pc = bank_bit(m) | high | low;
        }
        ("JMPP", ["@A"]) => {
            let page = m.pc & 0xF00;
            m.pc = page | u16::from(w.program(page | u16::from(m.a)));
        }
        ("MOVP", ["A", "@A"]) => m.a = w.program((m.pc & 0xF00) | u16::from(m.a)),
        ("MOVP3", ["A", "@A"]) => m.a = w.program(0x300 | u16::from(m.a)),
        ("DJNZ", [r, "addr"]) => {
            let v = source(m, w, r).wrapping_sub(1);
            store(m, r, v);
            jump_if(m, w, v != 0);
        }
        ("JNI", ["addr"]) => {
            // /INT low is the interrupt asserted. A JNI that saw none leaves its destination
            // for the interrupt logic (see src/mcs48.rs).
            let int = m.irq;
            let target = jump_if(m, w, int);
            if !int {
                m.jni_target = Some(target);
            }
        }
        ("JTF", ["addr"]) => {
            // "The flag is reset by executing a JTF."
            let tf = m.timer_flag;
            m.timer_flag = false;
            jump_if(m, w, tf);
        }
        (j, ["addr"]) => {
            let cond = match j {
                "JC" => m.psw & 0x80 != 0,
                "JNC" => m.psw & 0x80 == 0,
                "JZ" => m.a == 0,
                "JNZ" => m.a != 0,
                "JF0" => m.psw & 0x20 != 0,
                "JF1" => m.f1,
                "JT0" => w.t0,
                "JNT0" => !w.t0,
                "JT1" => w.t1,
                "JNT1" => !w.t1,
                _ => {
                    // JBb: "if accumulator bit 'b' is set".
                    let b = j.strip_prefix("JB").expect(j).parse::<u8>().unwrap();
                    m.a & (1 << b) != 0
                }
            };
            jump_if(m, w, cond);
        }
        ("MOVX", ["A", r]) => {
            let addr = m.ram[reg_at(m, reg_number(r))];
            m.a = w.movx_read(addr);
        }
        ("MOVX", [r, "A"]) => {
            let addr = m.ram[reg_at(m, reg_number(r))];
            w.movx_write(addr, m.a);
        }
        ("MOV", ["A", src]) => m.a = source(m, w, src),
        ("MOV", ["PSW", "A"]) => m.psw = m.a & !0x08,
        ("MOV", ["T", "A"]) => m.timer = m.a,
        ("MOV", [dst, src]) => {
            let v = source(m, w, src);
            store(m, dst, v);
        }
        ("XCH", ["A", r]) => {
            let v = source(m, w, r);
            store(m, r, m.a);
            m.a = v;
        }
        ("XCHD", ["A", r]) => {
            // "The lower 4 bits of the accumulator are exchanged with the lower 4 bits of the
            // data memory location"; the upper four of each stay.
            let v = source(m, w, r);
            store(m, r, (v & 0xF0) | (m.a & 0x0F));
            m.a = (m.a & 0xF0) | (v & 0x0F);
        }
        ("INC" | "DEC", ["A"]) => {
            m.a = if name == "INC" {
                m.a.wrapping_add(1)
            } else {
                m.a.wrapping_sub(1)
            };
        }
        ("INC" | "DEC", [r]) => {
            let v = source(m, w, r);
            store(
                m,
                r,
                if name == "INC" {
                    v.wrapping_add(1)
                } else {
                    v.wrapping_sub(1)
                },
            );
        }
        ("CLR", ["A"]) => m.a = 0,
        ("CPL", ["A"]) => m.a = !m.a,
        ("CLR", ["C"]) => m.psw &= !0x80,
        ("CPL", ["C"]) => m.psw ^= 0x80,
        ("CLR", ["F0"]) => m.psw &= !0x20,
        ("CPL", ["F0"]) => m.psw ^= 0x20,
        ("CLR", ["F1"]) => m.f1 = false,
        ("CPL", ["F1"]) => m.f1 = !m.f1,
        ("DA", ["A"]) => {
            // "If the contents of bits 0-3 are greater than nine, or if AC is one, the
            // accumulator is incremented by six. ... If bits 4-7 exceed nine, or if C is one,
            // these bits are increased by six. If an overflow occurs, C is set to one."
            if m.a & 15 > 9 || m.psw & 0x40 != 0 {
                let (v, over) = m.a.overflowing_add(6);
                m.a = v;
                if over {
                    m.psw |= 0x80;
                }
            }
            if m.a >> 4 > 9 || m.psw & 0x80 != 0 {
                m.a = m.a.wrapping_add(0x60);
                m.psw |= 0x80;
            }
        }
        ("RL", ["A"]) => m.a = m.a << 1 | m.a >> 7,
        ("RR", ["A"]) => m.a = m.a >> 1 | m.a << 7,
        ("RLC", ["A"]) => {
            let c = m.psw >> 7;
            m.psw = (m.psw & 0x7F) | (m.a & 0x80);
            m.a = m.a << 1 | c;
        }
        ("RRC", ["A"]) => {
            let c = m.psw & 0x80;
            m.psw = (m.psw & 0x7F) | (m.a & 1) << 7;
            m.a = m.a >> 1 | c;
        }
        ("SWAP", ["A"]) => m.a = m.a << 4 | m.a >> 4,
        ("EN", ["I"]) => m.int_enabled = true,
        ("DIS", ["I"]) => m.int_enabled = false,
        ("EN", ["TCNTI"]) => m.timer_irq_enabled = true,
        ("DIS", ["TCNTI"]) => {
            // "The pending timer interrupt ... may be removed by executing a DIS TCNTI."
            m.timer_irq_enabled = false;
            m.timer_irq = false;
        }
        ("ENT0", ["CLK"]) => m.t0_clock = true,
        ("IN", ["A", port]) => {
            // The pins: the outside world's level where the latch holds one, low where it holds
            // zero.
            let p = if *port == "P1" { 1 } else { 2 };
            let latch = if p == 1 { m.p1 } else { m.p2 };
            m.a = w.port_read(p) & latch;
        }
        ("INS", ["A", "BUS"]) => m.a = w.bus_read(),
        ("OUTL", ["BUS", "A"]) => w.bus_write(m.a),
        ("OUTL", [port, "A"]) => {
            let p = if *port == "P1" { 1 } else { 2 };
            if p == 1 {
                m.p1 = m.a;
            } else {
                m.p2 = m.a;
            }
            w.port_write(p, m.a);
        }
        ("RET", []) => pop(m, false),
        ("RETR", []) => {
            // "Re-enables the interrupt input logic", so the return keeps its bit 11.
            m.in_irq = false;
            pop(m, true);
        }
        ("SEL", ["RB0"]) => m.psw &= !0x10,
        ("SEL", ["RB1"]) => m.psw |= 0x10,
        ("SEL", ["MB0"]) => m.mbf = false,
        ("SEL", ["MB1"]) => m.mbf = true,
        ("STOP", ["TCNT"]) => m.count = Count::Stopped,
        ("STRT", ["T"]) => {
            // "The prescaler which counts the 32 cycles is cleared but the timer register is not."
            m.count = Count::Timer;
            m.prescaler = 0;
        }
        ("STRT", ["CNT"]) => {
            if m.count != Count::Counter {
                m.t1_last = w.t1;
            }
            m.count = Count::Counter;
        }
        _ => panic!("no description for {text}"),
    }
    cycles
}

// ---------------------------------------------------------------------------------------------

#[test]
fn every_opcode_does_what_the_manual_says() {
    let mut rng = Rng(0x8035_8039_8048_8049);
    let mut failures = Vec::new();
    for model in [Model::I8035, Model::I8039] {
        for op in 0..=255u8 {
            let mut failed = 0;
            for case in 0..1500 {
                let (start, world) = random_case(&mut rng, model, op);
                let (mut chip, mut chip_world) = (start.clone(), world.clone());
                let (mut want, mut want_world) = (start.clone(), world);
                let got_cycles = chip.step(&mut chip_world);
                let want_cycles = datasheet(&mut want, &mut want_world);
                if got_cycles != want_cycles || chip != want || chip_world.log != want_world.log {
                    failed += 1;
                    if failed <= 2 {
                        failures.push(format!(
                            "{model:?} {op:02X} {} (case {case}): cycles {want_cycles} got \
                             {got_cycles}\n  start {start:?}\n  want  {want:?}\n  got   {chip:?}\n  \
                             want accesses {:?}\n  got  accesses {:?}",
                            TABLE[usize::from(op)].0,
                            want_world.log,
                            chip_world.log
                        ));
                    }
                }
            }
            if failed > 0 {
                failures.push(format!("{model:?} {op:02X}: {failed} of 1500 failed"));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn every_opcode_takes_its_cycles_and_length() {
    let mut rng = Rng(48);
    for op in 0..=255u8 {
        let (text, cycles) = TABLE[usize::from(op)];
        let (mut m, mut w) = random_case(&mut rng, Model::I8035, op);
        // Nothing jumps or returns from here: no condition holds and the stack is clean.
        m.a = 0x01;
        m.psw = 0x00;
        m.f1 = false;
        m.timer_flag = false;
        m.count = Count::Stopped;
        m.irq = false;
        w.t0 = false;
        w.t1 = true;
        if text.starts_with("JNT0") {
            w.t0 = true;
        }
        if text.starts_with("JNT1") {
            w.t1 = true;
        } else if text.starts_with("JT1") {
            w.t1 = false;
        }
        if text.starts_with("JNC") {
            m.psw = 0x80;
        }
        if text.starts_with("JNZ") || text.starts_with("DJNZ") {
            m.a = 0;
            m.ram[usize::from(op & 7)] = 1;
        }
        if text.starts_with("JZ") || text.starts_with("JB") {
            m.a = 0x01 ^ if text.starts_with("JB0") { 0x01 } else { 0 };
        }
        m.pc = 0x100;
        w.rom[0x100] = op;
        assert_eq!(m.step(&mut w), cycles, "{op:02X} {text}");
        let moves = text.starts_with("JMP")
            || text.starts_with("CALL")
            || text.starts_with("RET")
            || text == "JMPP @A";
        if !moves {
            assert_eq!(m.pc, 0x100 + length(text), "{op:02X} {text}");
        }
    }
}

#[test]
fn the_manuals_decimal_adjust_examples() {
    // "Assume accumulator contains 10011011. DA A: ACC adjusted to 00000001 with C set."
    // Then sums of BCD digits: 15+27, 09+09 (AC set by the addition), 99+01, 50+50.
    for (a, b, want, carry) in [
        (0x9Bu8, None, 0x01u8, true),
        (0x15, Some(0x27u8), 0x42, false),
        (0x09, Some(0x09), 0x18, false),
        (0x99, Some(0x01), 0x00, true),
        (0x50, Some(0x50), 0x00, true),
    ] {
        let (mut m, mut w) = random_case(&mut Rng(1), Model::I8035, 0);
        m.pc = 0;
        m.psw = 0;
        m.count = Count::Stopped;
        m.in_irq = false;
        m.int_enabled = false;
        m.timer_irq = false;
        m.a = a;
        let program: &[u8] = match b {
            // ADD A,#b; DA A
            Some(b) => &[0x03, b, 0x57],
            None => &[0x57],
        };
        w.rom[..program.len()].copy_from_slice(program);
        while (m.pc as usize) < program.len() {
            m.step(&mut w);
        }
        assert_eq!(
            (m.a, m.psw & 0x80 != 0),
            (want, carry),
            "{a:02X} + {b:02X?}"
        );
    }
}

/// The table, written as assembler source: every defined opcode's instruction at twice the
/// opcode, with 5Ah as the immediate, the next byte's own address as an in-page destination,
/// and 45h as the low byte of a JMP or CALL destination.
fn assembler_source() -> String {
    let mut s = String::from(
        "; Every MCS-48 instruction, one per opcode, each at twice its opcode. Written from the\n\
         ; table in tests/mcs48_opcodes.rs by that test; assembled with The Macro Assembler AS\n\
         ; (asl -cpu 8048) into opcodes.bin, which the test checks byte for byte.\n\
         \tcpu\t8048\n",
    );
    for (op, &(text, _)) in TABLE.iter().enumerate() {
        if text == "?" {
            continue;
        }
        let line = text
            .replace("#data", "#5AH")
            .replace("addr", "$+1")
            .replace("xx", "45H")
            .to_lowercase();
        s.push_str(&format!("\torg\t{:03X}h\n\t{line}\n", op * 2));
    }
    s
}

#[test]
fn an_independent_assembler_agrees_with_the_table() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/mcs48");
    let source = assembler_source();
    if std::env::var("MCS48_WRITE_ASM").is_ok() {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("opcodes.asm"), &source).unwrap();
    }
    let committed = std::fs::read_to_string(dir.join("opcodes.asm")).unwrap();
    assert_eq!(committed, source, "opcodes.asm no longer matches the table");
    let bin = std::fs::read(dir.join("opcodes.bin")).unwrap();
    for (op, &(text, _)) in TABLE.iter().enumerate() {
        if text == "?" {
            continue;
        }
        let at = op * 2;
        assert_eq!(
            usize::from(bin[at]),
            op,
            "AS put {:02X} for {text}",
            bin[at]
        );
        let want = if text.contains("#data") {
            Some(0x5A)
        } else if text.contains("addr") {
            Some((at + 1) as u8)
        } else if text.contains("xx") {
            Some(0x45)
        } else {
            None
        };
        if let Some(want) = want {
            assert_eq!(bin[at + 1], want, "the second byte of {text}");
        }
    }
}
