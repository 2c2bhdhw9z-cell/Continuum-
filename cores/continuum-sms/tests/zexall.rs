//! ZEXALL and ZEXDOC on the Master System's own memory map and ports (Maxim's SMS port, fetched
//! by scripts/fetch-test-roms.sh, never committed): the Z80 exercised the way a cartridge runs
//! it, banking, work RAM, interrupts and the memory control port included. Each test case prints
//! "OK" or "ERROR" with the CRCs to the SDSC debug console; every one must be OK. Without the
//! files the tests say SKIPPED.

use std::path::PathBuf;

use continuum_sms::sms::Sms;

fn run(name: &str) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test-roms").join(name);
    let Ok(rom) = std::fs::read(&path) else {
        eprintln!("SKIPPED: {} not found (run scripts/fetch-test-roms.sh)", path.display());
        return;
    };
    let mut m = Sms::new(rom);
    // Long enough for the whole suite at the console's speed: about 4 hours of console time.
    for _ in 0..60 * 60 * 60 * 5 {
        m.run_frame();
        if m.bus.console.contains("Tests complete") || m.bus.console_stop {
            break;
        }
    }
    let out = m.bus.console.clone();
    eprintln!("{name} after {} frames:\n{out}", m.frames);
    assert!(out.contains("Tests complete"), "did not finish");
    let cases = out.matches("OK").count();
    assert!(!out.contains("ERROR"), "a case failed");
    assert!(cases >= 60, "only {cases} cases reported OK");
}

#[test]
fn zexdoc() {
    run("zexdoc.sms");
}

#[test]
fn zexall() {
    run("zexall.sms");
}
