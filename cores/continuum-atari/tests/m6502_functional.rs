//! Klaus Dormann's 6502 functional test (GPL-3.0, fetched by scripts/fetch-test-suites.sh, never
//! committed): a 64 KB program that checks every documented instruction, addressing mode and flag,
//! decimal mode included, and ends in a jump to itself. It ends at $3469 when everything passed;
//! anywhere else names the check that failed (see the test's listing).

use continuum_atari::m6502::{Bus, M6502};
use std::path::PathBuf;

struct Ram(Vec<u8>);

impl Bus for Ram {
    fn read(&mut self, addr: u16) -> u8 {
        self.0[addr as usize]
    }
    fn write(&mut self, addr: u16, value: u8) {
        self.0[addr as usize] = value;
    }
}

#[test]
fn klaus_dormanns_functional_test_passes() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test-suites/6502_functional_test.bin");
    let Ok(image) = std::fs::read(&path) else {
        eprintln!("SKIPPED: the functional test isn't here (scripts/fetch-test-suites.sh)");
        return;
    };
    assert_eq!(image.len(), 0x10000);
    let mut ram = Ram(image);
    let mut cpu = M6502::new();
    cpu.pc = 0x0400;
    let mut last = 0xFFFF;
    let mut steps = 0u64;
    loop {
        cpu.step(&mut ram);
        steps += 1;
        if cpu.pc == last {
            break;
        }
        last = cpu.pc;
        assert!(steps < 200_000_000, "the test never ended (pc {:04x})", cpu.pc);
    }
    assert_eq!(cpu.pc, 0x3469, "trapped at {:04x}, not at the success address", cpu.pc);
    eprintln!("passed after {steps} instructions, {} cycles", cpu.cycles);
}
