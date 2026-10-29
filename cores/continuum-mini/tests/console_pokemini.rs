//! The whole console against PokeMini's (the emulator cMini replaces), on the same start-up
//! program and the same programs, both started as PokeMini's libretro core starts:
//!
//! - instruction by instruction, every part but the sound caught up after each one: the
//!   registers, the cycles and the processor's state must agree at every step;
//! - frame by frame as the libretro core runs them, with buttons pressed and the shake: RAM, the
//!   I/O registers, the LCD's own memory, the analog panel's shades, the frame's sound and every
//!   pixel must agree at every frame.
//!
//! PokeMini's side is `pmharness`, built by `.kiro/notes/wip/tools/pokemini/build.sh` (nothing of
//! PokeMini is committed). Set `POKEMINI_HARNESS` to it, `POKEMINI_BIOS` to a start-up program
//! (PokeMini's free one, `freebios/freebios.min` in its source) and `POKEMINI_PROGRAMS` to a
//! folder of `.min` programs (its `dev-misc/examples`, searched one level down). Without them
//! this test does nothing.

use std::path::{Path, PathBuf};
use std::process::Command;

use continuum_mini::machine::Machine;

fn fnv(bytes: &[u8], mut h: u64) -> u64 {
    for &b in bytes {
        h = (h ^ u64::from(b)).wrapping_mul(0x100000001b3);
    }
    h
}
const FNV: u64 = 0xcbf29ce484222325;

struct Setup {
    harness: PathBuf,
    bios: PathBuf,
    programs: Vec<PathBuf>,
}

fn setup() -> Option<Setup> {
    let harness = PathBuf::from(std::env::var("POKEMINI_HARNESS").ok()?);
    let bios = PathBuf::from(std::env::var("POKEMINI_BIOS").ok()?);
    let dir = PathBuf::from(std::env::var("POKEMINI_PROGRAMS").ok()?);
    let mut programs = Vec::new();
    for entry in std::fs::read_dir(&dir).ok()?.flatten() {
        let p = entry.path();
        let inner: Vec<PathBuf> = if p.is_dir() {
            std::fs::read_dir(&p).ok()?.flatten().map(|e| e.path()).collect()
        } else {
            vec![p]
        };
        programs.extend(inner.into_iter().filter(|p| p.extension().is_some_and(|e| e == "min")));
    }
    programs.sort();
    Some(Setup { harness, bios, programs })
}

fn pokemini(s: &Setup, program: &Path, mode: &str, n: usize, keys: Option<&Path>) -> Vec<String> {
    let mut cmd = Command::new(&s.harness);
    cmd.arg(&s.bios).arg(program).arg(mode).arg(n.to_string());
    if let Some(k) = keys {
        cmd.arg(k);
    }
    let out = cmd.output().expect("pmharness runs");
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| l.starts_with(|c: char| c.is_ascii_digit()))
        .map(str::to_owned)
        .collect()
}

fn machine(s: &Setup, program: &Path) -> Machine {
    let rom = std::fs::read(program).expect("program");
    let bios = std::fs::read(&s.bios).expect("start-up program");
    Machine::new(&rom, bios).expect("cMini starts")
}

fn trace_line(n: usize, m: &Machine, cycles: u32) -> String {
    let c = &m.cpu;
    format!(
        "{n} {:02X}:{:04X} {:04X} {:02X}:{:04X} {:02X}:{:04X} {:02X}:{:04X} {:04X} {:02X} {cycles} {}",
        c.v,
        c.pc,
        c.ba,
        c.i,
        c.hl,
        c.xi,
        c.x,
        c.yi,
        c.y,
        c.sp,
        c.f,
        m.cpu_status()
    )
}

/// The buttons a script holds from each frame on: (frame, mask).
fn keys_at(script: &[(usize, u32)], frame: usize, held: &mut u32) {
    for &(f, mask) in script {
        if f == frame {
            *held = mask;
        }
    }
}

fn frame_line(n: usize, m: &Machine) -> String {
    let mut s = format!(
        "{n} {:016x} {:016x} {:016x} {:016x} ",
        fnv(&m.mem[..0x1000], FNV),
        fnv(m.io(), FNV),
        fnv(&m.lcd.memory, FNV),
        fnv(&m.lcd.shade, FNV)
    );
    let mut h = FNV;
    for &v in &m.audio.samples {
        h = fnv(&v.to_le_bytes(), h);
    }
    s += &format!("{h:016x} {} ", m.audio.samples.len());
    for i in (0..96 * 64).step_by(8) {
        let mut b = 0u8;
        for j in 0..8 {
            b |= u8::from(m.lcd.pixels[i + j] != 0) << j;
        }
        s += &format!("{b:02X}");
    }
    s
}

/// The first place two runs part, with the lines before it.
fn compare(what: &str, theirs: &[String], ours: &[String]) -> Result<(), String> {
    for (k, (a, b)) in theirs.iter().zip(ours).enumerate() {
        if a != b {
            let from = k.saturating_sub(4);
            let mut msg = format!("{what}: they part at line {k}\n");
            for j in from..k {
                msg += &format!("    both:     {}\n", &theirs[j][..theirs[j].len().min(140)]);
            }
            msg += &format!("    PokeMini: {}\n    cMini:    {}", &a[..a.len().min(140)], &b[..b.len().min(140)]);
            return Err(msg);
        }
    }
    if theirs.len() != ours.len() {
        return Err(format!("{what}: {} lines from PokeMini, {} from cMini", theirs.len(), ours.len()));
    }
    Ok(())
}

/// A made-up cartridge that works every part of the console at random: register writes and
/// reads all over the I/O block (timers, interrupts, the PRC, the LCD, the save chip's lines,
/// the sound, the colour add-on, unused addresses), RAM writes into the PRC's sprite table, tile
/// map and frame buffer, waits, changes of the interrupt level, and the odd HALT, looping
/// forever; every interrupt clears what is active and returns.
fn random_cartridge(seed: u64) -> Vec<u8> {
    let mut r = seed | 1;
    let mut next = move || {
        r ^= r << 13;
        r ^= r >> 7;
        r ^= r << 17;
        r
    };
    let mut cart = vec![0xFFu8; 0x10000];
    cart[0x2100] = b'M';
    cart[0x2101] = b'N';
    // A relative long jump at `at` to `to`: F3 and the offset from its last byte.
    let jrl = |cart: &mut Vec<u8>, at: usize, to: usize| {
        let off = (to as i64 - at as i64 - 2) as u16;
        cart[at..at + 3].copy_from_slice(&[0xF3, off as u8, (off >> 8) as u8]);
    };
    let handler = 0x21D0;
    let main = 0x2200;
    jrl(&mut cart, 0x2102, main);
    for k in 0..0x1D {
        jrl(&mut cart, 0x2108 + 6 * k, handler);
    }
    cart[handler..handler + 13].copy_from_slice(&[0xDD, 0x27, 0xFF, 0xDD, 0x28, 0xFF, 0xDD, 0x29, 0xFF, 0xDD, 0x2A, 0xFF, 0xF9]);
    // Everything running from the start: both oscillators, the three timers (timer 3 as one
    // sixteen-bit counter with a pivot), the 256 Hz timer, the PRC drawing and copying, the sound,
    // and every interrupt let in.
    let mut code: Vec<u8> = vec![0xB4, 0x20];
    for (reg, v) in [
        (0x19, 0x30), (0x18, 0x9A), (0x1A, 0xBC), (0x1C, 0xD9), (0x32, 0x40), (0x33, 0x10),
        (0x30, 0x06), (0x31, 0x06), (0x3A, 0x22), (0x3B, 0x30), (0x38, 0x06), (0x39, 0x06),
        (0x4A, 0x80), (0x4B, 0x01), (0x4C, 0x40), (0x4D, 0x00), (0x48, 0x86), (0x40, 0x01),
        (0x80, 0x0E), (0x81, 0x01), (0x71, 0x03), (0x20, 0xFF), (0x21, 0xFF), (0x22, 0xFF),
        (0x23, 0xFF), (0x24, 0x3F), (0x25, 0xFF), (0x26, 0xFF),
    ] {
        code.extend_from_slice(&[0xDD, reg, v]);
    }
    code.extend_from_slice(&[0x9C, 0x3F]);
    // The registers written at random (not the interrupt controller's, which come rarer).
    let regs: Vec<u8> = [0x03, 0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x10]
        .into_iter()
        .chain(0x18..=0x1F)
        .chain(0x30..=0x4F)
        .chain(0x50..=0x55)
        .chain([0x5F, 0x60, 0x61, 0x62, 0x70, 0x71, 0x72, 0x90, 0xC0])
        .chain(0x80..=0x8F)
        .chain(0xF0..=0xF7)
        .chain([0xFE, 0xFF])
        .collect();
    let body = main + code.len();
    // The save chip through its two lines, as a program talks to it: start, a byte out (eight
    // clocks, then one for the chip's acknowledge, read), a byte in (eight clocks read into RAM
    // through HL, then the acknowledge given), stop.
    let line = |code: &mut Vec<u8>, v: u8| code.extend_from_slice(&[0xDD, 0x61, v]);
    let start = |code: &mut Vec<u8>| {
        code.extend_from_slice(&[0xDD, 0x60, 0x0C]);
        for v in [0x0C, 0x08, 0x00] {
            line(code, v);
        }
    };
    let stop = |code: &mut Vec<u8>| {
        for v in [0x00, 0x08, 0x0C] {
            line(code, v);
        }
    };
    let out = |code: &mut Vec<u8>, byte: u8| {
        for k in (0..8).rev() {
            let d = if byte >> k & 1 != 0 { 0x04 } else { 0 };
            for v in [d, d | 0x08, d] {
                line(code, v);
            }
        }
        code.extend_from_slice(&[0xDD, 0x60, 0x08, 0xDD, 0x61, 0x08, 0x44, 0x61, 0x68, 0x91, 0xDD, 0x61, 0x00, 0xDD, 0x60, 0x0C]);
    };
    let input = |code: &mut Vec<u8>, last: bool| {
        code.extend_from_slice(&[0xDD, 0x60, 0x08]);
        for _ in 0..8 {
            code.extend_from_slice(&[0xDD, 0x61, 0x08, 0x44, 0x61, 0x68, 0x91, 0xDD, 0x61, 0x00]);
        }
        code.extend_from_slice(&[0xDD, 0x60, 0x0C]);
        let ack = if last { 0x04 } else { 0x00 };
        for v in [ack, ack | 0x08, ack] {
            line(code, v);
        }
    };
    while code.len() < 0xC000 {
        if next() % 40 == 0 {
            // A few bytes written somewhere, then read back from near there.
            let at = (next() % 0x2000) as u16;
            let ram = 0x1A00 + (next() % 0x400) as u16;
            code.extend_from_slice(&[0xC5, ram as u8, (ram >> 8) as u8]);
            start(&mut code);
            for b in [0xA0, (at >> 8) as u8, at as u8] {
                out(&mut code, b);
            }
            for _ in 0..1 + next() % 4 {
                out(&mut code, next() as u8);
            }
            stop(&mut code);
            let back = at.wrapping_sub((next() % 3) as u16);
            start(&mut code);
            for b in [0xA0, (back >> 8) as u8, back as u8] {
                out(&mut code, b);
            }
            start(&mut code);
            out(&mut code, 0xA1);
            let n = 1 + next() % 5;
            for k in 0..n {
                input(&mut code, k == n - 1);
            }
            stop(&mut code);
            continue;
        }
        let pick = next() % 100;
        let reg = regs[(next() % regs.len() as u64) as usize];
        let v = next() as u8;
        match pick {
            0..=34 => code.extend_from_slice(&[0xDD, reg, v]),
            35..=37 => code.extend_from_slice(&[0xDD, 0x20 + (next() % 7) as u8, v | 0x11]),
            38..=41 => code.extend_from_slice(&[0xDD, 0x27 + (next() % 4) as u8, v]),
            42..=56 => code.extend_from_slice(&[0x44, if next() % 3 == 0 { 0x20 + (next() % 11) as u8 } else { reg }]),
            57..=71 => {
                // RAM: mostly the PRC's tables and buffer.
                let a = if next() % 4 == 0 { 0x1000 + (next() % 0xE00) as u16 } else { 0x1000 + (next() % 0x3A0) as u16 };
                code.extend_from_slice(&[0xC5, a as u8, (a >> 8) as u8, 0xB5, v]);
            }
            72..=81 => code.extend_from_slice(&[0xB1, v | 1, 0xF5, 0xFF]),
            82..=86 => code.extend_from_slice(&[0xDD, 0xFE, v]),
            87..=90 => code.extend_from_slice(&[0x9C, 0x3F]),
            91..=93 => code.extend_from_slice(&[0x9D, 0xC0]),
            94..=96 => code.extend_from_slice(&[0x9F, v & 0xCF]),
            // Now and then a HALT, with the PRC's frame interrupt sure to wake it.
            _ if next() % 8 == 0 => code.extend_from_slice(&[0xDD, 0x80, 0x0E, 0xDD, 0x20, 0xFF, 0xDD, 0x23, 0xFF, 0x9C, 0x3F, 0xCE, 0xAE]),
            _ => code.extend_from_slice(&[0x4C, reg]),
        }
    }
    let end = main + code.len();
    cart[main..end].copy_from_slice(&code);
    jrl(&mut cart, end, body);
    cart
}

#[test]
fn random_programs_as_pokemini() {
    let Some(s) = setup() else {
        eprintln!("POKEMINI_HARNESS, POKEMINI_BIOS, POKEMINI_PROGRAMS not set: skipped");
        return;
    };
    let seeds: u64 = std::env::var("RANDOM_PROGRAMS").ok().and_then(|v| v.parse().ok()).unwrap_or(12);
    let steps: usize = std::env::var("TRACE_STEPS").ok().and_then(|v| v.parse().ok()).unwrap_or(300_000);
    let frames: usize = std::env::var("FRAMES").ok().and_then(|v| v.parse().ok()).unwrap_or(1200);
    let dir = std::env::temp_dir().join(format!("cmini-random-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let keys = dir.join("keys.txt");
    std::fs::write(&keys, "40 1\n55 0\n80 140\n95 0\n130 22\n150 0\n").unwrap();
    let script = [(40, 0x01), (55, 0), (80, 0x140), (95, 0), (130, 0x22), (150, 0)];
    let mut failures = Vec::new();
    for seed in 1..=seeds {
        let p = dir.join(format!("random{seed}.min"));
        std::fs::write(&p, random_cartridge(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15))).unwrap();
        let theirs = pokemini(&s, &p, "trace", steps, None);
        let mut m = machine(&s, &p);
        let ours: Vec<String> = (0..steps).map(|k| {
            let c = m.trace_step();
            trace_line(k, &m, c)
        }).collect();
        let trace = compare(&format!("random program {seed}, instructions"), &theirs, &ours);
        let theirs = pokemini(&s, &p, "frames", frames, Some(&keys));
        let mut m = machine(&s, &p);
        let mut held = 0u32;
        let ours: Vec<String> = (0..frames).map(|k| {
            keys_at(&script, k, &mut held);
            m.set_buttons(held as u8, held & 0x100 != 0);
            m.run_frame();
            frame_line(k, &m)
        }).collect();
        let framed = compare(&format!("random program {seed}, frames"), &theirs, &ours);
        match (trace, framed) {
            (Ok(()), Ok(())) => eprintln!("random program {seed}: {steps} instructions and {frames} frames alike"),
            (a, b) => failures.extend([a.err(), b.err()].into_iter().flatten()),
        }
    }
    std::fs::remove_dir_all(&dir).ok();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// cMini's own start-up program hands each game the console as PokeMini's free one does: the
/// processor's registers, every I/O register (but the PRC's frame divider, which counts the
/// frames the start took), RAM and the LCD's memory, at the moment the game's entry is reached.
#[test]
fn cmini_s_start_up_hands_over_what_pokemini_s_does() {
    let Some(s) = setup() else {
        eprintln!("POKEMINI_HARNESS, POKEMINI_BIOS, POKEMINI_PROGRAMS not set: skipped");
        return;
    };
    let mut failures = Vec::new();
    for p in &s.programs {
        let out = Command::new(&s.harness).arg(&s.bios).arg(p).arg("start").arg("1000000").output().expect("pmharness runs");
        let theirs: Vec<String> = String::from_utf8_lossy(&out.stdout).lines().filter(|l| !l.contains("chdir")).map(str::to_owned).collect();
        let rom = std::fs::read(p).unwrap();
        let mut m = Machine::new(&rom, continuum_mini::bios::image()).unwrap();
        let mut steps = 0;
        while m.cpu.pc != 0x2102 && steps < 1_000_000 {
            m.trace_step();
            steps += 1;
        }
        let c = &m.cpu;
        let mut io = m.io().to_vec();
        let mut their_io: Vec<u8> = (0..256).map(|k| u8::from_str_radix(&theirs[1][k * 2..k * 2 + 2], 16).unwrap()).collect();
        io[0x81] &= 0x0F;
        their_io[0x81] &= 0x0F;
        let ours = [
            format!("{:02X}:{:04X} {:04X} {:02X}:{:04X} {:02X}:{:04X} {:02X}:{:04X} {:04X} {:02X} {:02X}", c.v, c.pc, c.ba, c.i, c.hl, c.xi, c.x, c.yi, c.y, c.sp, c.f, c.n),
            io.iter().map(|b| format!("{b:02X}")).collect(),
            format!("{:016x} {:016x}", fnv(&m.mem[..0x1000], FNV), fnv(&m.lcd.memory, FNV)),
        ];
        let theirs = [theirs[0].clone(), their_io.iter().map(|b| format!("{b:02X}")).collect(), theirs[2].clone()];
        if ours != theirs {
            let io_diff: Vec<String> = (0..256).filter(|&k| io[k] != their_io[k]).map(|k| format!("${k:02X}: {:02X} for {:02X}", io[k], their_io[k])).collect();
            failures.push(format!("{}:\n    PokeMini: {}\n    cMini:    {}\n    I/O: {}\n    RAM, LCD: {} / {}", p.display(), theirs[0], ours[0], io_diff.join(", "), theirs[2], ours[2]));
        } else {
            eprintln!("{}: handed over alike, after {steps} instructions", p.display());
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A small assembler for the services cartridge: bytes, labels, long relative jumps.
struct Cart {
    b: Vec<u8>,
    at: usize,
    labels: std::collections::BTreeMap<&'static str, usize>,
    fixes: Vec<(usize, &'static str)>,
}

impl Cart {
    fn op(&mut self, bytes: &[u8]) -> &mut Self {
        self.b[self.at..self.at + bytes.len()].copy_from_slice(bytes);
        self.at += bytes.len();
        self
    }
    fn label(&mut self, name: &'static str) -> &mut Self {
        self.labels.insert(name, self.at);
        self
    }
    /// A long relative jump, call or branch (opcode `op`) to `to`.
    fn to(&mut self, op: u8, to: &'static str) -> &mut Self {
        self.fixes.push((self.at + 1, to));
        self.op(&[op, 0, 0])
    }
    /// A result: A stored at HL, HL moved on.
    fn keep(&mut self) -> &mut Self {
        self.op(&[0x68, 0x91])
    }
    /// A register's value kept.
    fn keep_reg(&mut self, reg: u8) -> &mut Self {
        self.op(&[0x44, reg]).keep()
    }
    fn finish(mut self) -> Vec<u8> {
        for (at, name) in std::mem::take(&mut self.fixes) {
            let to = self.labels[name];
            let off = (to as i64 - (at as i64 + 1)) as u16;
            self.b[at..at + 2].copy_from_slice(&off.to_le_bytes());
        }
        self.b
    }
}

/// A cartridge that calls the start-up program's services and keeps what comes back at $1400,
/// and has interrupts go both to a handler it puts in RAM and to its own table. With
/// `structure`, it also calls the structure test (CINT $3E) three ways.
fn services_cartridge(structure: bool) -> Vec<u8> {
    let mut c = Cart { b: vec![0xFF; 0x8000], at: 0x2100, labels: Default::default(), fixes: Vec::new() };
    c.op(b"MN").to(0xF3, "main");
    // Every slot of the cartridge's table to one handler.
    for k in 0..0x1D {
        c.at = 0x2108 + 6 * k;
        c.to(0xF3, "cart_irq");
    }
    // Each handler answers every interrupt and counts itself: PUSH HL; MOV HL,#count; INC [HL].
    c.at = 0x2200;
    c.label("cart_irq").op(&[0xDD, 0x27, 0xFF, 0xA1, 0xC5, 0x31, 0x15, 0x86, 0xA9, 0xF9]); // count at $1531
    c.at = 0x2220;
    c.label("ram_irq").op(&[0xDD, 0x27, 0xFF, 0xA1, 0xC5, 0x30, 0x15, 0x86, 0xA9, 0xF9]); // count at $1530
    // The structure test's subroutine: a mark at $1520.
    c.at = 0x2350;
    c.op(&[0xC7, 0x20, 0x15, 0xB7, 0x77, 0xF8]);
    // The structures: type, written address, compared address, value, subroutine.
    c.at = 0x2400;
    c.op(&[0x00, 0x10, 0x15, 0x00, 0x11, 0x15, 0x00, 0x5A, 0, 0, 0]);
    c.at = 0x2410;
    c.op(&[0x00, 0x12, 0x15, 0x00, 0x11, 0x15, 0x00, 0x5B, 0, 0, 0]);
    c.at = 0x2420;
    c.op(&[0x01, 0x13, 0x15, 0x00, 0x11, 0x15, 0x00, 0x5A, 0x50, 0x23, 0x23]);
    c.at = 0x2500;
    // N = $20, HL = $1400, and the stack below $1FFD, where the RAM handler goes.
    c.label("main").op(&[0xB4, 0x20, 0xC5, 0x00, 0x14, 0xCF, 0x6E, 0xF0, 0x1F]);
    // The contrast: read, set, stepped up, down, at its end, applied, shown.
    c.op(&[0xFC, 0x52]).keep();
    c.op(&[0xB0, 0x25, 0xFC, 0x4C]).keep().keep_reg(0x00);
    c.op(&[0x9C, 0xFE, 0xFC, 0x4E]).keep().keep_reg(0x00);
    c.op(&[0x9D, 0x01, 0xFC, 0x4E]).keep().keep_reg(0x00);
    c.op(&[0xB0, 0x3F, 0xFC, 0x4C, 0x9C, 0xFE, 0xFC, 0x4E]).keep().keep_reg(0x00);
    c.op(&[0xFC, 0x50]).keep().op(&[0x41]).keep();
    c.op(&[0xB0, 0x10, 0xFC, 0x54]).keep().op(&[0x41]).keep().keep_reg(0x80);
    // The LCD off, on, set up; the cartridge off and on.
    c.op(&[0xFC, 0x5A]).keep_reg(0x80).keep_reg(0x81);
    c.op(&[0xFC, 0x56]).keep_reg(0x80).keep_reg(0x81);
    c.op(&[0xFC, 0x58, 0xFC, 0x72]).keep_reg(0x00);
    c.op(&[0xFC, 0x74]).keep_reg(0x00);
    // The tests of $2053 and $2001, and the PRC's rate.
    c.op(&[0xFC, 0x7A, 0xCE, 0xC1]).keep();
    c.op(&[0xFC, 0x82, 0xCE, 0xC1]).keep();
    c.op(&[0xB0, 0x05, 0xFC, 0x7E]).keep().keep_reg(0x81);
    c.op(&[0xFC, 0x80]).keep();
    // A pulse on a RAM byte.
    c.op(&[0xC7, 0x00, 0x15, 0xB1, 0x03, 0xFC, 0x98, 0x47]).keep().op(&[0x41]).keep();
    // The structure test: equal, unequal, and with its subroutine.
    if structure {
        c.op(&[0xC7, 0x11, 0x15, 0xB7, 0x5A]);
        for at in [0x2400u16, 0x2410, 0x2420] {
            c.op(&[0xC6, at as u8, (at >> 8) as u8, 0xFC, 0x7C]).keep();
        }
        for at in [0x1510u16, 0x1512, 0x1513, 0x1520] {
            c.op(&[0xC7, at as u8, (at >> 8) as u8, 0x47]).keep();
        }
    }
    // A RAM handler: a jump at $1FFD to ram_irq, then the service that hands it the interrupts.
    let off = (0x2220i32 - (0x1FFD + 2)) as u16;
    c.op(&[0xC7, 0xFD, 0x1F, 0xB7, 0xF3, 0xC7, 0xFE, 0x1F, 0xB7, off as u8, 0xC7, 0xFF, 0x1F, 0xB7, (off >> 8) as u8]);
    c.op(&[0xFC, 0x5C]).keep_reg(0x01);
    // Timer 1's lower half interrupting, to the RAM handler three times...
    for (reg, v) in [(0x19, 0x20), (0x18, 0x0F), (0x32, 0x10), (0x30, 0x06), (0x20, 0x0C), (0x23, 0x04)] {
        c.op(&[0xDD, reg, v]);
    }
    c.op(&[0x9C, 0x3F]);
    c.label("wait_ram").op(&[0xC7, 0x30, 0x15, 0x47, 0x32, 0x03]).to(0xEC, "wait_ram");
    c.op(&[0x9D, 0xC0]).keep_reg(0x01);
    // ...then, with the RAM handler given up, to the cartridge's table twice.
    c.op(&[0xD8, 0x01, 0x3F, 0x9C, 0x3F]);
    c.label("wait_cart").op(&[0xC7, 0x31, 0x15, 0x47, 0x32, 0x02]).to(0xEC, "wait_cart");
    c.op(&[0x9D, 0xC0, 0xDD, 0x23, 0x00]).keep_reg(0x01);
    for at in [0x1530u16, 0x1531] {
        c.op(&[0xC7, at as u8, (at >> 8) as u8, 0x47]).keep();
    }
    // Suspend, until the power button.
    c.op(&[0xDD, 0x21, 0x0F, 0xDD, 0x23, 0x5A, 0xDD, 0x24, 0x21, 0xDD, 0x25, 0x18, 0xDD, 0x26, 0x81, 0xDD, 0x80, 0x0A]);
    c.op(&[0xFC, 0x42]);
    for reg in [0x23, 0x24, 0x25, 0x26, 0x80, 0x00, 0x01, 0x02] {
        c.keep_reg(reg);
    }
    c.op(&[0xB0, 0xAA]).keep();
    c.label("done").to(0xF3, "done");
    c.finish()
}

/// The start-up program's services give a game what PokeMini's free one gives it: the cartridge
/// above ends with the same results on cMini's start-up program as on PokeMini's. (Those on
/// PokeMini's are taken from cMini running PokeMini's program, checked here to end exactly as
/// PokeMini itself does, so a difference shows result by result.) The structure test is left
/// out: PokeMini's leaves one byte too many on the stack, so its RETI lands in the start-up
/// program and the game starts over; `the_structure_test_does_what_it_says` checks cMini's.
#[test]
fn cmini_s_start_up_services_do_what_pokemini_s_do() {
    let Some(s) = setup() else {
        eprintln!("POKEMINI_HARNESS, POKEMINI_BIOS, POKEMINI_PROGRAMS not set: skipped");
        return;
    };
    let dir = std::env::temp_dir().join(format!("cmini-services-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join("services.min");
    std::fs::write(&p, services_cartridge(false)).unwrap();
    // The power button pressed once the suspend has begun.
    let keys = dir.join("keys.txt");
    std::fs::write(&keys, "60 80\n70 0\n").unwrap();
    let theirs = pokemini(&s, &p, "frames", 120, Some(&keys));
    let rom = std::fs::read(&p).unwrap();
    let mut m = Machine::new(&rom, continuum_mini::bios::image()).unwrap();
    for f in 0..120 {
        m.set_buttons(if (60..70).contains(&f) { 0x80 } else { 0 }, false);
        m.run_frame();
    }
    std::fs::remove_dir_all(&dir).ok();
    let line = theirs.last().expect("PokeMini ran");
    let mut pm = Machine::new(&rom, std::fs::read(&s.bios).unwrap()).unwrap();
    for f in 0..120 {
        pm.set_buttons(if (60..70).contains(&f) { 0x80 } else { 0 }, false);
        pm.run_frame();
    }
    assert_eq!(
        line.split(' ').nth(1).unwrap(),
        format!("{:016x}", fnv(&pm.mem[..0x1000], FNV)),
        "cMini with PokeMini's start-up program doesn't end as PokeMini does"
    );
    let (mut ours, mut theirs) = (m.mem[0x400..0x480].to_vec(), pm.mem[0x400..0x480].to_vec());
    // The reads of $2081 (results 15, 17 and 23): its top four bits count the PRC's frames since
    // the start, which the two start-up programs take different times over.
    for k in [15, 17, 23] {
        ours[k] &= 0x0F;
        theirs[k] &= 0x0F;
    }
    let after_suspend = theirs.iter().position(|&b| b == 0xAA).expect("PokeMini's suspend came back");
    let differ: Vec<String> = (0..ours.len()).filter(|&k| ours[k] != theirs[k]).map(|k| format!("result {k}: {:02X} for {:02X}", ours[k], theirs[k])).collect();
    assert!(differ.is_empty(), "{}", differ.join(", "));
    eprintln!("{after_suspend} results alike: {:02X?}", &ours[..after_suspend + 1]);
}

/// The structure test (CINT $3E), on cMini's start-up program: $FF written where the structure
/// says, A 1 when the compared byte holds its value and 0 when not, and with bit 0 of its type
/// its subroutine called; everything else as it was.
#[test]
fn the_structure_test_does_what_it_says() {
    let rom = services_cartridge(true);
    let mut m = Machine::new(&rom, continuum_mini::bios::image()).unwrap();
    for f in 0..120 {
        m.set_buttons(if (60..70).contains(&f) { 0x80 } else { 0 }, false);
        m.run_frame();
    }
    // Results 25 on: the pulse (2), then the three structure tests, then $1510, $1512, $1513
    // and the subroutine's mark.
    assert_eq!(&m.mem[0x400 + 27..0x400 + 34], &[0x01, 0x00, 0x01, 0xFF, 0xFF, 0xFF, 0x77]);
    assert_eq!(m.mem[0x400 + 47], 0xAA, "the cartridge carried on to its end");
}

#[test]
fn instruction_by_instruction_as_pokemini() {
    let Some(s) = setup() else {
        eprintln!("POKEMINI_HARNESS, POKEMINI_BIOS, POKEMINI_PROGRAMS not set: skipped");
        return;
    };
    let n: usize = std::env::var("TRACE_STEPS").ok().and_then(|v| v.parse().ok()).unwrap_or(300_000);
    let mut failures = Vec::new();
    for p in &s.programs {
        let theirs = pokemini(&s, p, "trace", n, None);
        let mut m = machine(&s, p);
        let ours: Vec<String> = (0..n).map(|k| {
            let c = m.trace_step();
            trace_line(k, &m, c)
        }).collect();
        match compare(&p.display().to_string(), &theirs, &ours) {
            Ok(()) => eprintln!("{}: {n} instructions alike", p.display()),
            Err(e) => failures.push(e),
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn frame_by_frame_as_pokemini() {
    let Some(s) = setup() else {
        eprintln!("POKEMINI_HARNESS, POKEMINI_BIOS, POKEMINI_PROGRAMS not set: skipped");
        return;
    };
    let frames: usize = std::env::var("FRAMES").ok().and_then(|v| v.parse().ok()).unwrap_or(1200);
    // Every button in turn, two at once, held and let go, and the shake.
    let mut script: Vec<(usize, u32)> = Vec::new();
    let mut f = 90;
    for mask in [0x01, 0x02, 0x04, 0x08, 0x10, 0x20, 0x40, 0x100, 0x09, 0x41, 0x06, 0x80] {
        script.push((f, mask));
        script.push((f + 12, 0));
        f += 37;
    }
    let dir = std::env::temp_dir().join(format!("cmini-keys-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let keys = dir.join("keys.txt");
    std::fs::write(&keys, script.iter().map(|(f, m)| format!("{f} {m:x}\n")).collect::<String>()).unwrap();
    let mut failures = Vec::new();
    for p in &s.programs {
        let theirs = pokemini(&s, p, "frames", frames, Some(&keys));
        let mut m = machine(&s, p);
        let mut held = 0u32;
        let ours: Vec<String> = (0..frames).map(|k| {
            keys_at(&script, k, &mut held);
            m.set_buttons(held as u8, held & 0x100 != 0);
            m.run_frame();
            frame_line(k, &m)
        }).collect();
        match compare(&p.display().to_string(), &theirs, &ours) {
            Ok(()) => eprintln!("{}: {frames} frames alike", p.display()),
            Err(e) => failures.push(e),
        }
    }
    std::fs::remove_dir_all(&dir).ok();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
