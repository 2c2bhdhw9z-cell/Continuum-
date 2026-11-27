//! Headless driver for the mGBA suite's Video tests (scripts/mgba-suite-build/build/suite.gba).
//! The Video suite has no pass/fail counters: each test shows an ACTUAL picture, and
//! Right/A toggles to the EXPECTED picture drawn by hand from real-hardware behaviour.
//! This driver captures both framebuffers headlessly and diffs them pixel by pixel, so a
//! test "passes" when actual == expected. One fresh boot per test.
//!
//! Usage: cargo run --release --example p8video
//! Output: one line per test: differing pixels, differing lines, first/last diff line.

use continuum_gb::gba::Gba;

const KEY_A: u16 = 0x0001;
const KEY_B: u16 = 0x0002;
const KEY_RIGHT: u16 = 0x0010;
const KEY_DOWN: u16 = 0x0080;

const TESTS: [&str; 7] = [
    "Basic Mode 3",
    "Basic Mode 4",
    "Degenerate OBJ transforms",
    "Layer toggle",
    "Layer toggle 2",
    "OAM Update Delay",
    "Window offscreen reset",
];

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

fn settle(gba: &mut Gba, frames: usize) {
    for _ in 0..frames {
        gba.run_frame();
    }
}

/// Per-line diff counts between two framebuffers; returns (total pixels, lines touched,
// first diff line, last diff line, and the diff count per line for band analysis).
fn diff(a: &[u32], b: &[u32]) -> (usize, Vec<usize>) {
    let mut per_line = vec![0usize; 160];
    let mut total = 0;
    for y in 0..160 {
        for x in 0..240 {
            if a[y * 240 + x] != b[y * 240 + x] {
                per_line[y] += 1;
                total += 1;
            }
        }
    }
    (total, per_line)
}

/// Compress per-line diffs into (start, end, max-per-line) bands of consecutive differing lines.
fn bands(per_line: &[usize]) -> Vec<(usize, usize, usize)> {
    let mut out = Vec::new();
    let mut y = 0;
    while y < 160 {
        if per_line[y] == 0 {
            y += 1;
            continue;
        }
        let start = y;
        let mut max = 0;
        while y < 160 && per_line[y] > 0 {
            max = max.max(per_line[y]);
            y += 1;
        }
        out.push((start, y - 1, max));
    }
    out
}

fn main() {
    let rom = std::fs::read("scripts/mgba-suite-build/build/suite.gba")
        .expect("build suite.gba first: ./scripts/mgba-suite-build/build.sh");
    let mut npass = 0;
    for (i, name) in TESTS.iter().enumerate() {
        let mut gba = Gba::new(rom.clone());
        settle(&mut gba, 60);
        for _ in 0..13 {
            press(&mut gba, KEY_DOWN);
        }
        press(&mut gba, KEY_A); // enter Video suite list
        for _ in 0..i {
            press(&mut gba, KEY_DOWN);
        }
        press(&mut gba, KEY_A); // show test i (ACTUAL first)
        settle(&mut gba, 10);
        let actual = gba.bus.ppu.frame.clone();
        press(&mut gba, KEY_RIGHT); // toggle to EXPECTED
        settle(&mut gba, 10);
        let expected = gba.bus.ppu.frame.clone();
        let (total, per_line) = diff(&actual, &expected);
        let lines = per_line.iter().filter(|&&c| c > 0).count();
        let first = per_line.iter().position(|&c| c > 0);
        let last = per_line.iter().rposition(|&c| c > 0);
        let b = bands(&per_line);
        if total == 0 {
            npass += 1;
        }
        println!(
            "[{}] {}: {} px differ, {} lines, first={:?} last={:?} bands={:?}",
            if total == 0 { "PASS" } else { "FAIL" },
            name,
            total,
            lines,
            first,
            last,
            b
        );
        // Per-line diff x-ranges, for the transition-line tests.
        if total > 0 {
            for y in 0..160 {
                if per_line[y] > 0 {
                    let xs: Vec<usize> = (0..240)
                        .filter(|&x| actual[y * 240 + x] != expected[y * 240 + x])
                        .collect();
                    println!("    line {y}: {} px, x {}..{}", xs.len(), xs[0], xs[xs.len() - 1]);
                }
            }
        }
        // Per-line non-backdrop x-ranges, for transition structure (mid-line vs whole-line).
        if std::env::var("P8VIDEO_BANDS").ok().as_deref() == Some(i.to_string().as_str()) {
            for (label, f) in [("ACTUAL", &actual), ("EXPECTED", &expected)] {
                println!("--- {label} {name} non-white runs ---");
                for y in 0..160 {
                    let xs: Vec<usize> = (0..240).filter(|&x| f[y * 240 + x] != 0x00FFFFFF).collect();
                    if xs.is_empty() {
                        println!("    y={y:3} OFF");
                    } else {
                        println!("    y={y:3} n={} x {}..{}", xs.len(), xs[0], xs[xs.len() - 1]);
                    }
                }
            }
        }
        // ASCII downsample (every 4th pixel) of actual vs expected, for shape debugging.
        if std::env::var("P8VIDEO_DUMP").ok().as_deref() == Some(i.to_string().as_str()) {
            for (label, f) in [("ACTUAL", &actual), ("EXPECTED", &expected)] {
                println!("--- {label} {name} ---");
                let mut palette: Vec<u32> = Vec::new();
                for y in (0..160).step_by(4) {
                    let mut row = String::new();
                    for x in (0..240).step_by(4) {
                        let p = f[y * 240 + x];
                        let idx = match palette.iter().position(|&c| c == p) {
                            Some(k) => k,
                            None => {
                                palette.push(p);
                                palette.len() - 1
                            }
                        };
                        row.push(char::from_digit(idx as u32, 36).unwrap_or('?'));
                    }
                    println!("{row}");
                }
                println!("palette: {palette:08X?}");
            }
        }
        // Pixel-exact probe of a few lines, for stripe/transition analysis.
        if std::env::var("P8VIDEO_PROBE").ok().as_deref() == Some(i.to_string().as_str()) {
            for (label, f) in [("ACTUAL", &actual), ("EXPECTED", &expected)] {
                for y in [0usize, 68, 69, 101, 102, 103] {
                    let row: Vec<String> =
                        (0..32).map(|x| format!("{:06X}", f[y * 240 + x] & 0xFFFFFF)).collect();
                    println!("    {label} y={y}: {}", row.join(" "));
                }
                // Distinct colours over the whole frame.
                let mut cols: Vec<u32> = f.to_vec();
                cols.sort_unstable();
                cols.dedup();
                println!("    {label} distinct colours: {}", cols.len());
            }
            // The colour at x=0 and x=4 of every line, one character a line, from one palette
            // shared by both pictures: the on/off bands and the stripe phase, line by line.
            let mut pal: Vec<u32> = Vec::new();
            for f in [&actual, &expected] {
                for y in 0..160 {
                    pal.push(f[y * 240] & 0xFFFFFF);
                    pal.push(f[y * 240 + 4] & 0xFFFFFF);
                }
            }
            pal.sort_unstable();
            pal.dedup();
            let ch = |c: u32| {
                let c = c & 0xFFFFFF;
                match pal.iter().position(|&p| p == c) {
                    Some(i) if i < 36 => char::from_digit(i as u32, 36).unwrap_or('?'),
                    _ => '?',
                }
            };
            for (label, f) in [("ACTUAL", &actual), ("EXPECTED", &expected)] {
                let zero: String = (0..160).map(|y| ch(f[y * 240])).collect();
                let four: String = (0..160).map(|y| ch(f[y * 240 + 4])).collect();
                println!("    {label} x=0 per line: {zero}");
                println!("    {label} x=4 per line: {four}");
            }
            println!("    columns palette: {pal:06X?}");
            // 8x8 nibble grids (colour -> palette nibble) for the top-left block.
            let nib = |c: u32| -> char {
                match c & 0xFFFFFF {
                    0xE7E7E7 => '8',
                    0xC6C6C6 => '7',
                    0xA5A5A5 => '6',
                    0x848484 => '5',
                    0x636363 => '4',
                    0x424242 => '3',
                    0x212121 => '2',
                    0x000000 => '1',
                    0xFFFFFF => '.',
                    _ => '?',
                }
            };
            // BG0 on/off per line (a line is ON when any pixel differs from white backdrop).
            for (label, f) in [("ACTUAL", &actual), ("EXPECTED", &expected)] {
                let mut s = String::new();
                for y in 0..160 {
                    let on = (0..240).any(|x| f[y * 240 + x] & 0xFFFFFF != 0xFFFFFF);
                    s.push(if on { '1' } else { '0' });
                }
                println!("    {label} bg-on per line: {s}");
            }
            // The lines where the two pictures differ, with both colours at x=0 and x=4. A few
            // are enough to read the shape of it, and the log tail keeps a scorecard that this
            // does not push out.
            let mut differing = 0;
            for y in 0..160 {
                if actual[y * 240..(y + 1) * 240] == expected[y * 240..(y + 1) * 240] {
                    continue;
                }
                differing += 1;
                if differing > 12 {
                    continue;
                }
                let (a0, e0) = (actual[y * 240] & 0xFFFFFF, expected[y * 240] & 0xFFFFFF);
                let (a4, e4) = (actual[y * 240 + 4] & 0xFFFFFF, expected[y * 240 + 4] & 0xFFFFFF);
                println!("    differ {y}: x0 {a0:06X} vs {e0:06X}, x4 {a4:06X} vs {e4:06X}");
            }
            println!("    differing lines: {differing}");
            for (label, f) in [("ACTUAL", &actual), ("EXPECTED", &expected)] {
                println!("    {label} 8x8 block at (0,0):");
                for y in 0..8 {
                    let row: String = (0..8).map(|x| nib(f[y * 240 + x])).collect();
                    println!("      {row}");
                }
                println!("    {label} 8x8 block at (0,64):");
                for y in 64..72 {
                    let row: String = (0..8).map(|x| nib(f[y * 240 + x])).collect();
                    println!("      {row}");
                }
            }
        }
        let hash = |f: &[u32]| -> u64 {
            let mut h: u64 = 0xcbf29ce484222325;
            for &p in f {
                h ^= p as u64;
                h = h.wrapping_mul(0x100000001b3);
            }
            h
        };
        println!("    actual_hash={:016x} expected_hash={:016x}", hash(&actual), hash(&expected));
        let _ = KEY_B;
    }
    println!("VIDEO: {npass}/7 framebuffer-identical");
}
