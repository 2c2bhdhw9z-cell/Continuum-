//! Headless driver for the mGBA test suite (scripts/mgba-suite-build/build/suite.gba).
//! Drives the suite menu with scripted key presses, polls each suite's
//! `passes`/`totalResults` statics in EWRAM (addresses from suite.map), and
//! dumps the new SRAM log lines (the suite's savprintf output) per suite.
//! Temporary probe for P7; delete before finishing.
//!
//! Usage: cargo run --release --example p7mgba
//! Output: one SUITE line per suite, then the FAIL lines from SRAM.

use continuum_gb::gba::Gba;

const KEY_A: u16 = 0x0001;
const KEY_B: u16 = 0x0002;
const KEY_UP: u16 = 0x0040;
const KEY_DOWN: u16 = 0x0080;

// (menu name, totalResults addr, passes addr) — from suite.map, in menu order.
// NOTE: these move when the build changes (removing --gc-sections shifted them
// +8); re-check suite.map after any rebuild.
const SUITES: [(&str, u32, u32); 13] = [
    ("Memory", 0x020035b0, 0x020035b4),
    ("I/O read", 0x020031a0, 0x020031a4),
    ("Timing", 0x020035f8, 0x020035fc),
    ("Timer count-up", 0x020035f0, 0x020035f4),
    ("Timer IRQ", 0x020035e4, 0x020035e8),
    ("Shifter", 0x020035c8, 0x020035cc),
    ("Carry", 0x02003190, 0x02003194),
    ("Multiply long", 0x020035c0, 0x020035c4),
    ("BIOS math", 0x02003188, 0x0200318c),
    ("DMA", 0x02003198, 0x0200319c),
    ("SIO register R/W", 0x020035d0, 0x020035d4),
    ("SIO timing", 0x020035dc, 0x020035e0),
    ("Misc edge cases", 0x020035b8, 0x020035bc),
];
// Video (index 13) is interactive visual judgement; not driven headless.

fn ewram_u32(gba: &Gba, addr: u32) -> u32 {
    let o = (addr - 0x0200_0000) as usize;
    u32::from_le_bytes(gba.bus.ewram[o..o + 4].try_into().unwrap())
}

fn press(gba: &mut Gba, key: u16) {
    gba.set_keys(key);
    for _ in 0..3 {
        gba.run_frame();
    }
    gba.set_keys(0);
    for _ in 0..3 {
        gba.run_frame();
    }
}

/// All printable log lines currently in SRAM. The suite's savprintf log is a
/// plain-text stream, but the Memory suite's own SRAM-mirror tests overwrite
/// the log's first bytes, so offset-based parsing stops early; scanning the
/// whole chip for printable lines recovers everything except the few bytes
/// the tests overwrote.
fn sram_log_lines(gba: &Gba) -> Vec<String> {
    let d = &gba.bus.save.data;
    let mut lines = Vec::new();
    let mut cur = Vec::new();
    for &b in d {
        if b == b'\n' || b == 0 {
            if !cur.is_empty() && cur.iter().all(|&c| (0x20..0x7f).contains(&c)) {
                lines.push(String::from_utf8_lossy(&cur).into_owned());
            }
            cur.clear();
        } else {
            cur.push(b);
            if cur.len() > 200 {
                cur.clear();
            }
        }
    }
    lines
}

fn main() {
    let rom = std::fs::read("scripts/mgba-suite-build/build/suite.gba")
        .expect("build suite.gba first: ./scripts/mgba-suite-build/build.sh");
    println!("save kind: {:?}", Gba::new(rom.clone()).bus.save.kind);
    // One fresh boot per suite, like gameboy-rust's scoreboard tool: after a
    // suite finishes, the suite leaves IE=0 (its runTest saves IE after
    // irqInit cleared it), so the results screen's VBlankIntrWait never
    // returns and B never registers — navigating back to the menu is not
    // possible, so reboot instead.
    for (i, (name, total_addr, passes_addr)) in SUITES.iter().enumerate() {
        let mut gba = Gba::new(rom.clone());
        // Let the menu come up.
        for _ in 0..60 {
            gba.run_frame();
        }
        for _ in 0..i {
            press(&mut gba, KEY_DOWN);
        }
        press(&mut gba, KEY_A);
        // Poll until the counters stop moving, or the frame budget runs out.
        let mut last = (0u32, 0u32);
        let mut stable = 0;
        let mut frames = 0;
        let mut timed_out = false;
        loop {
            for _ in 0..10 {
                gba.run_frame();
                frames += 1;
            }
            let cur = (ewram_u32(&gba, *total_addr), ewram_u32(&gba, *passes_addr));
            if cur.0 > 0 && cur == last {
                stable += 1;
                if stable >= 3 {
                    break;
                }
            } else {
                stable = 0;
            }
            last = cur;
            if frames >= 18000 {
                timed_out = true;
                break;
            }
        }
        let (total, passes) = last;
        let lines = sram_log_lines(&gba);
        let fails: Vec<&String> = lines.iter().filter(|l| l.contains("FAIL")).collect();
        let timed_out_flag = if *name == "SIO timing" {
            ewram_u32(&gba, 0x020035d8) != 0
        } else {
            false
        };
        println!(
            "SUITE {} {}/{} frames={} timeout={} sio_timed_out={} sram_lines={} fails={}",
            name, passes, total, frames, timed_out, timed_out_flag, lines.len(), fails.len()
        );
        for f in fails.iter().take(40) {
            println!("  FAIL {}", f);
        }
        if fails.len() > 40 {
            println!("  ... and {} more FAIL lines", fails.len() - 40);
        }
        if timed_out {
            println!("STOPPED: {} never settled", name);
        }
    }
    println!("DONE");
}
