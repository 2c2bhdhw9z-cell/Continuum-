//! cBoy's Game Boy Advance cheat codes (src/gba/cheats.rs) against mGBA's own, line for line.
//!
//! mGBA reads GameShark, Action Replay, CodeBreaker and raw codes, and it's the Advance emulator
//! the app shipped before cBoy, so a cheat list that works there has to work here. This test
//! makes thousands of lists (every kind of code, encrypted and plain, with reseeds, hooks, ROM
//! patches, fills, conditions and blocks, and plain random lines too), gives each to a small
//! program built from mGBA's own cheat code (`MGBA_CHEAT_HARNESS`, built in a scratch folder,
//! never committed; see .kiro/notes/docs/CBOY_CHEATS.md), and checks that both find the same
//! hook, patch the same ROM bytes and write the same bytes when the program runs twice over the
//! same memory. Lists that meet a case cBoy reads differently on purpose (the top of
//! src/gba/cheats.rs lists them) are counted and left out. Without the variable it does nothing.

use std::collections::BTreeMap;
use std::io::Write;
use std::process::{Command, Stdio};

use continuum_gb::gba::cheats::{tea_encrypt, Device, Memory};

/// Memory that reads a fixed pattern until written, the same pattern the harness uses.
#[derive(Default)]
struct Fake {
    written: BTreeMap<u32, u8>,
}

impl Fake {
    fn byte(&self, a: u32) -> u8 {
        self.written.get(&a).copied().unwrap_or((a.wrapping_mul(0x9E37_79B1) >> 24) as u8)
    }
}

impl Memory for Fake {
    fn read(&self, addr: u32, width: u8) -> u32 {
        (0..u32::from(width)).fold(0, |v, i| v | u32::from(self.byte(addr.wrapping_add(i))) << (8 * i))
    }
    fn write(&mut self, addr: u32, width: u8, v: u32) {
        for i in 0..u32::from(width) {
            self.written.insert(addr.wrapping_add(i), (v >> (8 * i)) as u8);
        }
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
    fn below(&mut self, n: u32) -> u32 {
        self.next() % n
    }
    fn pick<T: Copy>(&mut self, xs: &[T]) -> T {
        xs[self.below(xs.len() as u32) as usize]
    }
    /// An address a code would name: mostly the console's RAM, sometimes anywhere.
    fn addr(&mut self) -> u32 {
        match self.below(8) {
            0..=3 => 0x0200_0000 | self.below(0x4_0000),
            4 | 5 => 0x0300_0000 | self.below(0x8000),
            6 => 0x0400_0000 | self.below(0x400),
            _ => self.next(),
        }
    }
}

const GS_SEEDS: [u32; 4] = [0x09F4_FBBD, 0x9681_884A, 0x3520_27E9, 0xF3DE_E5A7];
const AR_SEEDS: [u32; 4] = [0x7AA9_648F, 0x7FAE_6994, 0xC0EF_AAD5, 0x4271_2C57];

/// An address in Action Replay's own spelling.
fn ar(a: u32) -> u32 {
    (a & 0xF_FFFF) | ((a & 0x0F00_0000) >> 4)
}

fn gameshark_line(r: &mut Rng) -> (u32, u32) {
    let a = r.addr() & 0x0FFF_FFFF;
    match r.below(12) {
        0 => (a, r.below(0x100)),
        1 => (0x1000_0000 | a, r.below(0x1_0000)),
        2 => (0x2000_0000 | a, r.next()),
        3 => (0x6000_0000 | r.below(0x100_0000), r.below(0x1_0000)),
        4 => (0x8000_0000 | r.pick(&[0x0010_0000, 0x0020_0000, 0x00F0_0000]) | (a & 0x0F0F_FFFF), r.below(0x1_0000)),
        5 => (0xD000_0000 | a, r.below(4) << 20 | r.below(0x1_0000)),
        6 => (0xE000_0000 | r.below(4) << 16 | r.below(0x1_0000), r.addr()),
        7 => (0xF000_0000 | r.below(0x0200_0000), r.pick(&[0x0001_0000, 0x0002_0000, 0])),
        8 => (0xDEAD_FACE, r.below(0x1_0000)),
        9 => (r.next(), 0x001D_C0DE),
        10 => (0x3000_0000 | r.below(5), r.next()),
        _ => (r.next(), r.next()),
    }
}

fn action_replay_line(r: &mut Rng) -> (u32, u32) {
    let a = ar(r.addr());
    let width = r.below(4) << 25;
    match r.below(14) {
        0 => (width & 0x0400_0000 | a, r.next()),
        1 => (0x4000_0000 | width & 0x0400_0000 | a, r.next()),
        2 => (0x8000_0000 | width & 0x0400_0000 | a, r.below(0x1_0000)),
        3 => (r.pick(&[0xC600_0000, 0xC700_0000]) | r.below(0x400), r.below(0x1_0000)),
        4 | 5 => (r.pick(&[0x0000_0000, 0x4000_0000, 0x8000_0000]) | (1 + r.below(7)) << 27 | width | a, r.next()),
        6 => (0, r.pick(&[0x4000_0000, 0x6000_0000, 0, 0x0800_0000])),
        7 => (0, r.pick(&[0x8000_0000, 0x8200_0000, 0x8400_0000]) | ar(r.addr())),
        8 => (r.next(), r.next()),
        9 => (0, r.pick(&[0x1800_0000, 0x1A00_0000, 0x1C00_0000, 0x1E00_0000]) | r.below(0x100_0000)),
        10 => (0, r.pick(&[0x1000_0000, 0x1200_0000, 0x1400_0000]) | ar(r.addr())),
        11 => (0xC400_0000 | r.below(0x100_0000), 0),
        12 => (0xDEAD_FACE, r.below(0x1_0000)),
        _ => (r.next(), 0x001D_C0DE),
    }
}

fn codebreaker_line(r: &mut Rng) -> (u32, u16) {
    let a = r.addr() & 0x0FFF_FFFF;
    let v = r.below(0x1_0000) as u16;
    match r.below(10) {
        0 => (r.below(16) << 28 | a, v),
        1 => (0x4000_0000 | a, v),
        2 => (0x5000_0000 | a, r.below(7) as u16),
        3 => (0x9000_0000 | r.below(0x1000_0000), v),
        4 => (0xD000_0020, v),
        5 => (0x1000_0000 | r.below(0x0200_0000), v),
        _ => (r.next(), v),
    }
}

/// An Action Replay "if" block, closed as lists close them: the test, a few writes, maybe an
/// ELSE and a few more, then the ENDIF. All encrypted or all plain.
fn action_replay_block(r: &mut Rng) -> Vec<String> {
    let encrypt = r.below(2) == 0;
    let mut lines = Vec::new();
    // Real lists start with the game's ID line, which is also how the device is recognised.
    lines.push((r.next(), 0x001D_C0DE));
    let test = 0x8000_0000 | (1 + r.below(7)) << 27 | r.below(3) << 25 | ar(r.addr());
    lines.push((test, r.next() & 0xFFFF));
    let writes = |r: &mut Rng, lines: &mut Vec<(u32, u32)>| {
        for _ in 0..1 + r.below(3) {
            lines.push((r.below(2) << 25 | ar(r.addr()), r.below(0x100)));
        }
    };
    writes(r, &mut lines);
    if r.below(2) == 0 {
        lines.push((0, 0x6000_0000));
        writes(r, &mut lines);
    }
    lines.push((0, 0x4000_0000));
    if r.below(2) == 0 {
        writes(r, &mut lines);
    }
    lines
        .into_iter()
        .map(|(a, b)| if encrypt { tea_encrypt(a, b, &AR_SEEDS) } else { (a, b) })
        .map(|(a, b)| format!("{a:08X} {b:08X}"))
        .collect()
}

fn case(r: &mut Rng) -> Vec<String> {
    if r.below(5) == 0 {
        return action_replay_block(r);
    }
    let n = 1 + r.below(6);
    let style = r.below(6);
    let mut out = Vec::new();
    for _ in 0..n {
        let kind = if style == 5 { r.below(5) } else { style };
        out.push(match kind {
            0 => {
                let (a, b) = gameshark_line(r);
                let (a, b) = if r.below(2) == 0 { tea_encrypt(a, b, &GS_SEEDS) } else { (a, b) };
                format!("{a:08X} {b:08X}")
            }
            1 => {
                let (a, b) = action_replay_line(r);
                let (a, b) = if r.below(2) == 0 { tea_encrypt(a, b, &AR_SEEDS) } else { (a, b) };
                format!("{a:08X} {b:08X}")
            }
            2 => {
                let (a, b) = codebreaker_line(r);
                format!("{a:08X} {b:04X}")
            }
            3 => {
                let width = r.pick(&[2usize, 4, 8]);
                let addr = if r.below(3) == 0 { 0x0800_0000 | r.below(0x100_0000) } else { r.addr() };
                format!("{addr:08X}:{:0w$X}", r.next() as u64 & ((1u64 << (width * 4)) - 1), w = width)
            }
            _ => format!("{:08X} {:08X}", r.next(), r.next()),
        });
    }
    out
}

/// What cBoy makes of a list, in the harness's own words.
fn ours(lines: &[String]) -> (String, Option<&'static str>) {
    let d = Device::from_codes([lines.join("+").as_str()]);
    let mut mem = Fake::default();
    d.run(&mut mem);
    d.run(&mut mem);
    let hook = d.hook.map_or("none".to_owned(), |h| format!("{h:08X}"));
    let patches: Vec<String> = d.rom_patches.iter().map(|(o, v)| format!("{o:07X}:{v:02X}")).collect();
    let writes: Vec<String> = mem.written.iter().map(|(a, v)| format!("{a:08X}:{v:02X}")).collect();
    (format!("hook {hook} | patches {} | writes {}", patches.join(" "), writes.join(" ")), d.unlike_mgba)
}

/// The harness's line, with its patches in the order made folded the way cBoy keeps them (a
/// later patch of the same byte wins, sorted by address).
fn theirs(line: &str) -> String {
    let mut parts = line.split(" | ");
    let hook = parts.next().unwrap_or("");
    let patches = parts.next().unwrap_or("");
    let writes = parts.next().unwrap_or("");
    let mut folded = BTreeMap::new();
    for p in patches.trim_start_matches("patches").split_whitespace() {
        if let Some((o, v)) = p.split_once(':') {
            folded.insert(u32::from_str_radix(o, 16).unwrap(), v.to_owned());
        }
    }
    let patches: Vec<String> = folded.iter().map(|(o, v)| format!("{o:07X}:{v}")).collect();
    let writes = writes.trim_start_matches("writes").trim();
    format!("{hook} | patches {} | writes {writes}", patches.join(" "))
}

#[test]
fn every_kind_of_code_reads_and_runs_as_on_mgba() {
    let Ok(harness) = std::env::var("MGBA_CHEAT_HARNESS") else {
        eprintln!("MGBA_CHEAT_HARNESS not set: skipped");
        return;
    };
    let mut r = Rng(0x00C0_FFEE_1234_5678);
    // A random line can ask for millions of writes in a run (a fill's count is up to 24 bits).
    // Lists like that are left out: real ones fill tens of bytes, and they'd take minutes each.
    let small = |c: &Vec<String>| Device::from_codes([c.join("+").as_str()]).ops.iter().all(|o| o.repeat <= 1000);
    let cases: Vec<Vec<String>> = std::iter::repeat_with(|| case(&mut r)).filter(small).take(36_000).collect();
    let mut child = Command::new(harness).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().expect("harness runs");
    // Written from its own thread, so neither side waits on a full pipe for the other.
    let mut input = child.stdin.take().unwrap();
    let text_in: String = cases.iter().map(|c| c.join("|") + "\n").collect();
    let writer = std::thread::spawn(move || input.write_all(text_in.as_bytes()));
    let out = child.wait_with_output().expect("harness answers");
    writer.join().unwrap().unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    let answers: Vec<&str> = text.lines().collect();
    assert_eq!(answers.len(), cases.len(), "one answer a list");
    let (mut same, mut writes) = (0, 0);
    let mut apart: BTreeMap<&str, u32> = BTreeMap::new();
    let mut wrong = Vec::new();
    for (c, answer) in cases.iter().zip(&answers) {
        let (mine, unlike) = ours(c);
        if let Some(why) = unlike {
            *apart.entry(why).or_default() += 1;
            continue;
        }
        let expected = theirs(answer);
        if mine == expected {
            same += 1;
            writes += mine.matches(':').count();
        } else if wrong.len() < 5 {
            wrong.push(format!("{}\n  mGBA: {expected}\n  cBoy: {mine}", c.join(" + ")));
        }
    }
    eprintln!("{same} lists alike ({writes} bytes written or patched between them); left out on purpose: {apart:?}");
    assert!(wrong.is_empty(), "lists read differently:\n{}", wrong.join("\n"));
    assert!(same > 20_000, "too few lists compared: {same}");
}
