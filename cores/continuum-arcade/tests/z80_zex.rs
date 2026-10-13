//! ZEXDOC and ZEXALL, Frank D. Cringle's Z80 instruction exercisers (GPL-2.0-or-later, fetched by
//! scripts/fetch-test-suites.sh, never committed). Each runs every instruction group over
//! thousands of machine states and compares a CRC of the results with one taken on a real Z80:
//! ZEXDOC on the documented flags, ZEXALL on all eight. Both must report no errors.
//!
//! They are CP/M programs, so the harness is a tiny CP/M stand-in: the program at 0100h, the
//! BDOS entry at 0005h answering the two calls they use (2: print the character in E; 9: print
//! the string at DE up to '$'), and a jump to 0000h (CP/M's warm boot) ending the run. Without
//! the files the tests say SKIPPED.

use std::path::PathBuf;
use std::time::Instant;

use continuum_arcade::z80::{Bus, Z80};

struct Cpm {
    mem: Vec<u8>,
}

impl Bus for Cpm {
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
}

/// Runs a CP/M program to its warm boot and returns what it printed.
fn run_cpm(program: &[u8]) -> String {
    let mut bus = Cpm {
        mem: vec![0; 0x10000],
    };
    bus.mem[0x100..0x100 + program.len()].copy_from_slice(program);
    // The BDOS entry is a RET once the call is answered. The word at 0006h, which a real BDOS
    // jump's operand would be, is the top of memory: the programs put their stack there.
    bus.mem[5] = 0xC9;
    bus.mem[6] = 0x00;
    bus.mem[7] = 0xF0;
    let mut z = Z80::new();
    z.pc = 0x100;
    let mut out = String::new();
    let mut t: u64 = 0;
    let mut instructions: u64 = 0;
    let start = Instant::now();
    loop {
        match z.pc {
            0x0000 => break,
            0x0005 => match z.c {
                2 => out.push(z.e as char),
                9 => {
                    let mut a = z.de();
                    while bus.mem[a as usize] != b'$' {
                        out.push(bus.mem[a as usize] as char);
                        a = a.wrapping_add(1);
                    }
                }
                other => panic!("BDOS call {other} is not one these programs make"),
            },
            _ => {}
        }
        t += u64::from(z.step(&mut bus));
        instructions += 1;
    }
    eprintln!(
        "{instructions} instructions, {t} T-states ({:.1} s at the Z80's 3.072 MHz on Pac-Man), \
         run in {:.1} s",
        t as f64 / 3_072_000.0,
        start.elapsed().as_secs_f64()
    );
    out
}

fn exerciser(name: &str) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("test-suites")
        .join(name);
    let Ok(program) = std::fs::read(&path) else {
        eprintln!(
            "SKIPPED: {} not found (run scripts/fetch-test-suites.sh)",
            path.display()
        );
        return;
    };
    let out = run_cpm(&program);
    eprintln!("{name}:\n{out}");
    assert!(
        out.contains("Tests complete"),
        "{name} did not finish:\n{out}"
    );
    assert!(!out.contains("ERROR"), "{name} found errors:\n{out}");
    // Every group reports OK: a test that printed nothing would not count as passing.
    assert_eq!(
        out.matches("OK").count(),
        67,
        "{name} should pass 67 groups:\n{out}"
    );
}

#[test]
fn zexdoc() {
    exerciser("zexdoc.com");
}

#[test]
fn zexall() {
    exerciser("zexall.com");
}
