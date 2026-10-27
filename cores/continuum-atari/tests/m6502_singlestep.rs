//! SingleStepTests' 6502 suite: 256 opcodes, 10,000 cases each, every bus cycle of every case
//! compared (address, value, read or write), then the registers and memory afterwards. Fetch it
//! with scripts/fetch-test-suites.sh; without it this test says SKIPPED and passes.
//! CATARI_SST_OPS=a9,b1 runs only those opcodes; CATARI_SST_CASES=n only the first n of each.

use continuum_atari::m6502::{Bus, M6502};
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Deserialize)]
struct State {
    pc: u16,
    s: u8,
    a: u8,
    x: u8,
    y: u8,
    p: u8,
    ram: Vec<(u16, u8)>,
}

#[derive(Deserialize)]
struct Case {
    name: String,
    initial: State,
    #[serde(rename = "final")]
    end: State,
    cycles: Vec<(u16, u8, String)>,
}

/// 64 KB of RAM that records every cycle.
struct Ram {
    mem: Vec<u8>,
    log: Vec<(u16, u8, bool)>,
}

impl Bus for Ram {
    fn read(&mut self, addr: u16) -> u8 {
        let v = self.mem[addr as usize];
        self.log.push((addr, v, false));
        v
    }
    fn write(&mut self, addr: u16, value: u8) {
        self.mem[addr as usize] = value;
        self.log.push((addr, value, true));
    }
}

fn suite() -> Option<PathBuf> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test-suites/singlestep-6502/v1");
    p.is_dir().then_some(p)
}

fn is_jam(op: u8) -> bool {
    matches!(op, 0x02 | 0x12 | 0x22 | 0x32 | 0x42 | 0x52 | 0x62 | 0x72 | 0x92 | 0xB2 | 0xD2 | 0xF2)
}

#[test]
fn every_opcode_matches_singlesteptests_cycle_for_cycle() {
    let Some(dir) = suite() else {
        eprintln!("SKIPPED: SingleStepTests' 6502 suite isn't here (scripts/fetch-test-suites.sh)");
        return;
    };
    let only: Option<Vec<u8>> = std::env::var("CATARI_SST_OPS")
        .ok()
        .map(|s| s.split(',').map(|h| u8::from_str_radix(h.trim(), 16).unwrap()).collect());
    let limit: usize = std::env::var("CATARI_SST_CASES").ok().and_then(|s| s.parse().ok()).unwrap_or(usize::MAX);

    let mut ram = Ram { mem: vec![0; 0x10000], log: Vec::with_capacity(16) };
    let mut failures = Vec::new();
    let mut total = 0usize;
    for op in 0..=255u8 {
        if let Some(only) = &only {
            if !only.contains(&op) {
                continue;
            }
        }
        let path = dir.join(format!("{op:02x}.json"));
        let text = std::fs::read_to_string(&path).unwrap();
        let cases: Vec<Case> = serde_json::from_str(&text).unwrap();
        let mut op_fail = 0usize;
        for case in cases.iter().take(limit) {
            total += 1;
            for &(a, v) in &case.initial.ram {
                ram.mem[a as usize] = v;
            }
            ram.log.clear();
            let mut cpu = M6502::new();
            cpu.pc = case.initial.pc;
            cpu.s = case.initial.s;
            cpu.a = case.initial.a;
            cpu.x = case.initial.x;
            cpu.y = case.initial.y;
            cpu.p = case.initial.p;
            cpu.step(&mut ram);
            if is_jam(op) {
                while ram.log.len() < case.cycles.len() {
                    cpu.step(&mut ram);
                }
            }
            let mut why = Vec::new();
            let got: Vec<(u16, u8, bool)> = ram.log.clone();
            let want: Vec<(u16, u8, bool)> =
                case.cycles.iter().map(|(a, v, k)| (*a, *v, k == "write")).collect();
            if got != want {
                why.push(format!("cycles {:?}\n      want {:?}", fmt_cycles(&got), fmt_cycles(&want)));
            }
            let e = &case.end;
            let regs = (cpu.pc, cpu.s, cpu.a, cpu.x, cpu.y, cpu.p);
            let want_regs = (e.pc, e.s, e.a, e.x, e.y, e.p);
            if regs != want_regs {
                why.push(format!(
                    "regs pc={:04x} s={:02x} a={:02x} x={:02x} y={:02x} p={:02x}, want pc={:04x} s={:02x} a={:02x} x={:02x} y={:02x} p={:02x}",
                    regs.0, regs.1, regs.2, regs.3, regs.4, regs.5,
                    want_regs.0, want_regs.1, want_regs.2, want_regs.3, want_regs.4, want_regs.5
                ));
            }
            for &(a, v) in &e.ram {
                if ram.mem[a as usize] != v {
                    why.push(format!("ram[{a:04x}]={:02x}, want {v:02x}", ram.mem[a as usize]));
                }
            }
            if !why.is_empty() {
                op_fail += 1;
                if failures.len() < 40 {
                    failures.push(format!("{} (opcode {op:02x}): {}", case.name, why.join("; ")));
                }
            }
            // Leave memory clean for the next case.
            for &(a, _) in case.initial.ram.iter().chain(e.ram.iter()) {
                ram.mem[a as usize] = 0;
            }
            for &(a, _, _) in &got {
                ram.mem[a as usize] = 0;
            }
        }
        if op_fail > 0 {
            eprintln!("opcode {op:02x}: {op_fail} cases differ");
        }
    }
    assert!(failures.is_empty(), "{} of {total} cases differ, first ones:\n{}", failures.len(), failures.join("\n"));
    eprintln!("{total} cases, every cycle matches");
}

fn fmt_cycles(c: &[(u16, u8, bool)]) -> Vec<String> {
    c.iter().map(|(a, v, w)| format!("{}{:04x}={:02x}", if *w { "W" } else { "R" }, a, v)).collect()
}
