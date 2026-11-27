//! Headless driver for the mGBA test suite (scripts/mgba-suite-build/build/suite.gba).
//! Drives the suite menu with scripted key presses, reads each suite's own
//! `passes`/`totalResults` counters, and dumps the SRAM log lines (the suite's
//! savprintf output) per suite.
//!
//! The counters are found the way the suite itself finds them, not from addresses
//! written down here: `suite.map` gives the address of the suite's `suites[]` table
//! and of `nSuites`, and each entry of that table is a `struct TestSuite` (the
//! suite's include/suite.h) holding a pointer to its own `passes` and
//! `totalResults`. A relink moves the statics; following the pointers follows them.
//! (The written-down addresses did not: the first Ubuntu build printed 0/0.)
//!
//! Usage: cargo run --release --example p7mgba
//! Output: one SUITE line per suite, then the FAIL lines from SRAM.

use continuum_gb::gba::Gba;

const KEY_A: u16 = 0x0001;
const KEY_DOWN: u16 = 0x0080;

/// `struct TestSuite` as it lies in ROM on the ARM: name, run, list and show
/// pointers, then `nTests` (a size_t, four bytes here), `passes` and
/// `totalResults`. The last two are pointers into EWRAM, which is what makes
/// following them survive a relink.
const SUITE_PASSES: u32 = 0x14;
const SUITE_TOTAL: u32 = 0x18;

/// The addresses written down for the old P7 build, used only when suite.map
/// cannot be read. (menu name, totalResults, passes), in menu order.
const FALLBACK: [(&str, u32, u32); 13] = [
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

/// One suite's name and the two counters the suite keeps for it.
struct Suite {
    name: String,
    passes: u32,
    total: u32,
}

/// A symbol's address from a GNU ld map. A symbol is a line of its own — an
/// address column, then the name — while section and object lines carry more
/// fields, so two fields with the name second is the symbol.
fn map_symbol(map: &str, name: &str) -> Option<u32> {
    for line in map.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() == 2 && f[1] == name {
            if let Ok(v) = u32::from_str_radix(f[0].trim_start_matches("0x"), 16) {
                return Some(v);
            }
        }
    }
    None
}

fn c_string(gba: &Gba, addr: u32) -> String {
    let mut s = String::new();
    for i in 0..64u32 {
        let b = gba.bus.peek8(addr.wrapping_add(i)) as u8;
        if b == 0 {
            break;
        }
        s.push(char::from(b));
    }
    s
}

/// The suite's own list of suites, read out of the ROM through `suites[]`.
fn suites_from_rom(gba: &Gba, map: &str) -> Option<Vec<Suite>> {
    let table = map_symbol(map, "suites")?;
    let n = map_symbol(map, "nSuites").map(|a| gba.bus.peek32(a)).unwrap_or(14).min(32);
    let mut out = Vec::new();
    for i in 0..n {
        let suite = gba.bus.peek32(table + i * 4);
        if !(0x0800_0000..0x0E00_0000).contains(&suite) {
            continue;
        }
        let name = c_string(gba, gba.bus.peek32(suite));
        let passes = gba.bus.peek32(suite + SUITE_PASSES);
        let total = gba.bus.peek32(suite + SUITE_TOTAL);
        if passes == 0 || total == 0 {
            continue;
        }
        out.push(Suite { name, passes, total });
    }
    if out.is_empty() { None } else { Some(out) }
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
    let map_path = "scripts/mgba-suite-build/build/suite.map";
    let map = std::fs::read_to_string(map_path).unwrap_or_default();
    let fresh = Gba::new(rom.clone());
    println!("save kind: {:?}", fresh.bus.save.kind);
    // The counters, followed from the map through the suite's own table.
    let found = if map.is_empty() { None } else { suites_from_rom(&fresh, &map) };
    let from_map = found.is_some();
    let suites = match found {
        Some(s) => s,
        None => {
            if map.is_empty() {
                println!("MAP: {map_path} is missing; falling back to the written-down addresses, which a relink moves");
            } else {
                println!("MAP: no suites[] in {map_path}; falling back to the written-down addresses");
            }
            FALLBACK
                .iter()
                .map(|(n, t, p)| Suite { name: (*n).to_string(), passes: *p, total: *t })
                .collect()
        }
    };
    println!("SUITES: {} ({})", suites.len(), if from_map { "from suite.map" } else { "written down" });
    // SIO timing's own `timedOut` flag, also from the map: a suite that gives up
    // waiting for a transfer sets it, and its results are then not comparable.
    let sio_timed_out = map_symbol(&map, "timedOut");
    // One fresh boot per suite, like gameboy-rust's scoreboard tool: after a
    // suite finishes, the suite leaves IE=0 (its runTest saves IE after
    // irqInit cleared it), so the results screen's VBlankIntrWait never
    // returns and B never registers — navigating back to the menu is not
    // possible, so reboot instead.
    for (i, suite) in suites.iter().enumerate() {
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
            let cur = (gba.bus.peek32(suite.total), gba.bus.peek32(suite.passes));
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
        let timed_out_flag = sio_timed_out.map(|a| gba.bus.peek32(a) != 0).unwrap_or(false);
        println!(
            "SUITE {} {}/{} frames={} timeout={} sio_timed_out={} sram_lines={} fails={}",
            suite.name, passes, total, frames, timed_out, timed_out_flag, lines.len(), fails.len()
        );
        for f in fails.iter().take(40) {
            println!("  FAIL {}", f);
        }
        if fails.len() > 40 {
            println!("  ... and {} more FAIL lines", fails.len() - 40);
        }
        if timed_out {
            println!("STOPPED: {} never settled", suite.name);
        }
    }
    println!("DONE");
}
