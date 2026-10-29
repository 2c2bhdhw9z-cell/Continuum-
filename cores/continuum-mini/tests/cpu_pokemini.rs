//! cMini's MINX processor against PokeMini's, one instruction at a time (the way SingleStepTests
//! check other processors): every opcode, primary and both extension pages, from many random
//! starting states over the same memory, must leave the same registers, flags, bank registers,
//! cycle count and writes. PokeMini is the Pokémon Mini emulator cMini replaces, the one whose
//! timings came from measuring the real console; its processor is built on its own by
//! `.kiro/notes/wip/tools/pokemini/build.sh` (`POKEMINI_CPU_HARNESS`; nothing of it is committed).
//! Without the variable this test does nothing.

use std::collections::BTreeMap;
use std::io::Write;
use std::process::{Command, Stdio};

use continuum_mini::minx::{Bus, Minx};

/// Memory that reads a pattern of the case's seed until written, as the harness's does.
struct Flat {
    seed: u32,
    code: Vec<(u32, u8)>,
    writes: Vec<(u32, u8)>,
}

impl Flat {
    fn pattern(&self, a: u32) -> u8 {
        let mut h = (a ^ self.seed).wrapping_mul(0x9E37_79B1);
        h ^= h >> 15;
        h = h.wrapping_mul(0x85EB_CA6B);
        (h >> 24) as u8
    }
}

impl Bus for Flat {
    fn read(&mut self, a: u32) -> u8 {
        let a = a & 0x1F_FFFF;
        if let Some(&(_, v)) = self.writes.iter().rev().find(|w| w.0 == a) {
            return v;
        }
        if let Some(&(_, v)) = self.code.iter().find(|c| c.0 == a) {
            return v;
        }
        self.pattern(a)
    }
    fn write(&mut self, a: u32, v: u8) {
        self.writes.push((a & 0x1F_FFFF, v));
    }
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 16) as u32
    }
}

#[derive(Clone)]
struct Case {
    seed: u32,
    regs: [u32; 15],
    code: [u8; 6],
}

impl Case {
    fn line(&self) -> String {
        let r = &self.regs;
        let mut s = format!("{:X}", self.seed);
        for v in r {
            s += &format!(" {v:X}");
        }
        for b in self.code {
            s += &format!(" {b:02X}");
        }
        s
    }
}

/// cMini's answer, in the harness's words.
fn ours(c: &Case) -> String {
    let r = &c.regs;
    let mut cpu = Minx {
        ba: r[0] as u16,
        hl: r[1] as u16,
        x: r[2] as u16,
        y: r[3] as u16,
        sp: r[4] as u16,
        pc: r[5] as u16,
        f: r[6] as u8,
        e: r[7] as u8,
        n: r[8] as u8,
        i: r[9] as u8,
        xi: r[10] as u8,
        yi: r[11] as u8,
        v: r[12] as u8,
        u: r[13] as u8,
        u_next: r[13] as u8,
        u_hold: r[14] as u8,
        ..Minx::default()
    };
    let at = if cpu.pc & 0x8000 != 0 { (u32::from(cpu.v) << 15) | u32::from(cpu.pc & 0x7FFF) } else { u32::from(cpu.pc) };
    let mut mem = Flat { seed: c.seed, code: (0..6).map(|k| ((at + k) & 0x1F_FFFF, c.code[k as usize])).collect(), writes: Vec::new() };
    let cycles = cpu.step(&mut mem);
    let status = cpu.status;
    let mut s = format!(
        "{:04X} {:04X} {:04X} {:04X} {:04X} {:04X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {} {} {} |",
        cpu.ba, cpu.hl, cpu.x, cpu.y, cpu.sp, cpu.pc, cpu.f, cpu.e, cpu.n, cpu.i, cpu.xi, cpu.yi, cpu.v, cpu.u, cpu.u_hold, cycles, status
    );
    for (a, v) in mem.writes {
        s += &format!(" {a:06X}:{v:02X}");
    }
    s
}

fn cases() -> Vec<Case> {
    let mut r = Rng(0x5EED_0F0F_1234_ABCD);
    let mut out = Vec::new();
    let mut add = |r: &mut Rng, code: [u8; 6]| {
        let mut regs = [0u32; 15];
        for (k, v) in regs.iter_mut().enumerate() {
            *v = match k {
                0..=5 => r.next() & 0xFFFF,
                14 => r.next() % 3,
                _ => r.next() & 0xFF,
            };
        }
        out.push(Case { seed: r.next(), regs, code });
    };
    for op in 0..=0xFFu8 {
        let pages: Vec<Option<u8>> = if op == 0xCE || op == 0xCF { (0..=0xFF).map(Some).collect() } else { vec![None] };
        for second in pages {
            let n = if second.is_some() { 24 } else { 120 };
            for _ in 0..n {
                let mut code = [0u8; 6];
                for b in code.iter_mut() {
                    *b = r.next() as u8;
                }
                code[0] = op;
                if let Some(s) = second {
                    code[1] = s;
                }
                add(&mut r, code);
            }
        }
    }
    out
}

#[test]
fn every_instruction_does_what_pokemini_s_does() {
    let Ok(harness) = std::env::var("POKEMINI_CPU_HARNESS") else {
        eprintln!("POKEMINI_CPU_HARNESS not set: skipped");
        return;
    };
    let cases = cases();
    let mut child = Command::new(harness).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().expect("harness runs");
    let mut input = child.stdin.take().unwrap();
    let text: String = cases.iter().map(|c| c.line() + "\n").collect();
    let writer = std::thread::spawn(move || input.write_all(text.as_bytes()));
    let out = child.wait_with_output().expect("harness answers");
    writer.join().unwrap().unwrap();
    let answers: Vec<String> = String::from_utf8_lossy(&out.stdout).lines().map(str::to_owned).collect();
    assert_eq!(answers.len(), cases.len());
    // Differences by opcode (the extension pages as CE xx / CF xx).
    let mut wrong: BTreeMap<String, (u32, String)> = BTreeMap::new();
    let mut same = 0;
    for (c, theirs) in cases.iter().zip(&answers) {
        let mine = ours(c);
        if &mine == theirs {
            same += 1;
            continue;
        }
        let key = if c.code[0] == 0xCE || c.code[0] == 0xCF { format!("{:02X} {:02X}", c.code[0], c.code[1]) } else { format!("{:02X}", c.code[0]) };
        let e = wrong.entry(key).or_insert((0, String::new()));
        e.0 += 1;
        if e.1.is_empty() {
            e.1 = format!("{}\n    PokeMini: {theirs}\n    cMini:    {mine}", c.line());
        }
    }
    eprintln!("{same} of {} instructions alike; {} opcodes differ", cases.len(), wrong.len());
    if std::env::var("CPU_FIELDS").is_ok() {
        // Which fields disagree, over every case that differs.
        let names = ["BA", "HL", "X", "Y", "SP", "PC", "F", "E", "N", "I", "XI", "YI", "V", "U", "shift", "cycles", "status"];
        let mut count: BTreeMap<&str, u32> = BTreeMap::new();
        for (c, theirs) in cases.iter().zip(&answers) {
            let mine = ours(c);
            let (a, b): (Vec<&str>, Vec<&str>) = (theirs.split_whitespace().collect(), mine.split_whitespace().collect());
            for (k, n) in names.iter().enumerate() {
                if a.get(k) != b.get(k) {
                    *count.entry(n).or_default() += 1;
                }
            }
            if theirs.split('|').nth(1) != mine.split('|').nth(1) {
                *count.entry("writes").or_default() += 1;
            }
        }
        eprintln!("fields differing: {count:?}");
    }
    if !wrong.is_empty() {
        let list: Vec<String> = wrong.iter().take(40).map(|(k, (n, ex))| format!("{k} ({n}x): {ex}")).collect();
        panic!("{} opcodes differ, the first:\n{}", wrong.len(), list.join("\n"));
    }
}
