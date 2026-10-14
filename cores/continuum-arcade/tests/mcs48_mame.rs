//! The 8035 against MAME's MCS-48 core, instruction by instruction. Each program in tests/mcs48
//! (alu, flow, timer, io: our own, assembled with The Macro Assembler AS) was run on MAME 0.289's
//! Donkey Kong sound board, whose sound CPU MAME runs as an MB8884, its 8035, with MAME's
//! debugger tracing every instruction from the first: the program counter with its bank, A, the
//! PSW, R0-R7 of the selected bank, the timer and its prescaler, both port latches, and the
//! cycles run so far. tests/mcs48/<program>.mame holds how many instructions were compared and a
//! hash of each 1024 lines of that trace (made by tests/mcs48/mame_trace.py). Here the same
//! program runs on our 8035 wired as that board wires it, and every hash must match.
//!
//! The board as MAME has it: the 2 KB program ROM answers in both banks; MOVX and INS A,BUS read
//! the tune ROM page that P2 bits 0-2 pick, or the sound command latch when P2 bit 6 is set; P1
//! (the DAC) reads nothing back from outside; P2 goes through a latch that MAME updates at the
//! end of each instruction; and tests/mcs48/dkmain.asm, the Z80's program, holds /INT
//! asserted, T1 low (T0 stays high), P2 bit 5 pulled low and the command latch at 5, from a few
//! microseconds in, before any program looks at them. `MCS48_TRACE=<dir>` writes our traces
//! there, to compare line by line with MAME's (`MCS48_KEEP=1 mame_trace.py`) when one differs.

use std::fmt::Write as _;
use std::path::PathBuf;

use continuum_arcade::mcs48::{Bus, Mcs48, Model};

/// Donkey Kong's sound board, as the programs see it once the Z80 has set it up.
struct DkSound {
    rom: Vec<u8>,
    tune: Vec<u8>,
    /// What P2 last wrote, which the board holds in a latch and reads back through.
    p2_latch: u8,
    /// MAME's board takes a P2 write into its latch when the instruction ends (the latch is
    /// synchronised with the scheduler), so within one instruction P2 reads the latch from
    /// before. Only MOVD A,Pp, which writes P2 and reads it back, can tell.
    p2_written: Option<u8>,
}

impl DkSound {
    fn end_of_instruction(&mut self) {
        if let Some(v) = self.p2_written.take() {
            self.p2_latch = v;
        }
    }

    /// Port 2's pins: latch 6H bit 3 (set by the Z80) pulls bit 5 low.
    fn p2_pins(&self) -> u8 {
        self.p2_latch & !0x20
    }

    /// What MOVX and INS A,BUS read: the command latch (5, read through inverters) when P2 bit
    /// 6 is set, otherwise the tune ROM at the page in P2 bits 0-2.
    fn tune_read(&self, addr: u8) -> u8 {
        let page = self.p2_pins() & 0x47;
        if page & 0x40 != 0 {
            (5 & 0x0F) ^ 0x0F
        } else {
            self.tune[usize::from(page & 7) << 8 | usize::from(addr)]
        }
    }
}

impl Bus for DkSound {
    fn program(&mut self, addr: u16) -> u8 {
        self.rom[usize::from(addr) & 0x7FF]
    }
    fn movx_read(&mut self, addr: u8) -> u8 {
        self.tune_read(addr)
    }
    fn bus_read(&mut self) -> u8 {
        self.tune_read(0)
    }
    fn port_read(&mut self, port: u8) -> u8 {
        if port == 2 {
            self.p2_pins()
        } else {
            0xFF
        }
    }
    fn port_write(&mut self, port: u8, val: u8) {
        if port == 2 {
            self.p2_written = Some(val);
        }
    }
    fn t0(&mut self) -> bool {
        true
    }
    fn t1(&mut self) -> bool {
        false
    }
}

/// The tune ROM: a pattern of our own, the same as mame_trace.py's.
fn tune_rom() -> Vec<u8> {
    (0..2048usize)
        .map(|i| ((i * 37 + (i >> 8) * 101 + 11) & 0xFF) as u8)
        .collect()
}

/// One trace line, as mame_trace.py writes MAME's.
fn line(m: &Mcs48, cycles: u64) -> String {
    let mut s = format!(
        "{}:{:03X} A={:02X} F={:02X} R=",
        m.pc >> 11,
        m.pc & 0x7FF,
        m.a,
        m.psw
    );
    for n in 0..8 {
        write!(s, "{:02X}", m.reg(n)).unwrap();
    }
    write!(
        s,
        " T={:02X} P={:02X} P1={:02X} P2={:02X} C={cycles}",
        m.timer, m.prescaler, m.p1, m.p2
    )
    .unwrap();
    s
}

fn fnv1a(lines: &[String]) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for l in lines {
        for &b in l.as_bytes().iter().chain(b"\n") {
            h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01B3);
        }
    }
    h
}

fn compare(name: &str) {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/mcs48");
    let rom = std::fs::read(dir.join(format!("{name}.bin"))).unwrap();
    let reference = std::fs::read_to_string(dir.join(format!("{name}.mame"))).unwrap();
    let mut body = reference.lines().filter(|l| !l.starts_with('#'));
    let count: usize = body
        .next()
        .and_then(|l| l.strip_prefix("instructions "))
        .expect("an instruction count")
        .parse()
        .unwrap();
    let hashes: Vec<u64> = body.map(|l| u64::from_str_radix(l, 16).unwrap()).collect();

    let mut bus = DkSound {
        rom,
        tune: tune_rom(),
        p2_latch: 0xFF,
        p2_written: None,
    };
    let mut cpu = Mcs48::new(Model::I8035);
    cpu.set_irq(true);
    let mut cycles = 0u64;
    let mut lines = Vec::with_capacity(count);
    for _ in 0..count {
        cycles += u64::from(cpu.interrupt(&mut bus));
        lines.push(line(&cpu, cycles));
        cycles += u64::from(cpu.instruction(&mut bus));
        bus.end_of_instruction();
    }
    if let Ok(out) = std::env::var("MCS48_TRACE") {
        std::fs::write(
            PathBuf::from(out).join(format!("{name}.trace")),
            lines.join("\n"),
        )
        .unwrap();
    }
    let ours: Vec<u64> = lines.chunks(1024).map(fnv1a).collect();
    assert_eq!(ours.len(), hashes.len(), "{name}: block count");
    if let Some(block) = (0..ours.len()).find(|&b| ours[b] != hashes[b]) {
        panic!(
            "{name}: our trace differs from MAME's within instructions {} to {} (set \
             MCS48_TRACE to see ours); ours there starts:\n{}",
            block * 1024,
            (block * 1024 + 1023).min(count - 1),
            lines[block * 1024..(block * 1024 + 8).min(count)].join("\n")
        );
    }
    eprintln!("{name}: all {count} instructions match MAME's trace ({cycles} cycles)");
}

#[test]
fn arithmetic_and_logic_match_mame() {
    compare("alu");
}

#[test]
fn jumps_calls_banks_and_the_stack_match_mame() {
    compare("flow");
}

#[test]
fn the_timer_counter_and_interrupts_match_mame() {
    compare("timer");
}

#[test]
fn the_ports_movx_and_the_external_interrupt_match_mame() {
    compare("io");
}
