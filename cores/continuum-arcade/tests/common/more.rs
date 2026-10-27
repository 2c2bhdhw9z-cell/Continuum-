//! The Pac-Man driver's other machines (the second round, `tasks/arcade-2.json`): cArcade's own test
//! programs laid out as each of MAME's sets lays out its chips, scrambled the way that set's board
//! is wired, run on cArcade (through the set list and the real loading path) and on MAME.
//!
//! The scrambling here is written again from MAME's description of each board (its inits and
//! graphics layouts), separately from src/pacman.rs's unscrambling: if both were wrong the same
//! way, cArcade would still undo what this does, but MAME wouldn't, and the comparison would fail.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use continuum_arcade::pacman::{self, GfxWiring, Pacman, Ports, Scramble};
use continuum_arcade::sets::{self, Archive, Board, Op, SetSpec};
use continuum_arcade::zip::{write_stored, Zip};

use super::{apply, audio_hash, crate_dir, program_file, run_mame_files, video_hash, Event, FrameOut};

/// Which program a case runs, and how its 64 KB of program space is filled.
#[derive(Clone, Copy, Debug)]
pub enum Program {
    /// A 16 KB program at 0000h, and on boards with ROM at 8000h its second 16 KB there.
    Plain(&'static str, Option<&'static str>),
    /// The Pengo bootlegs' 32 KB image (0000h-0FFFh and 4000h-7FFFh used).
    Pengo,
    /// Champion Edition's two games, each 16 KB at 0000h and 16 KB at 8000h.
    Champion,
}

pub struct MoreCase {
    /// The case, and its reference file's name.
    pub name: &'static str,
    /// MAME's set.
    pub set: &'static str,
    pub program: Program,
    pub frames: u32,
    pub events: Vec<(u32, Event)>,
}

pub fn cases() -> Vec<MoreCase> {
    use super::Port::*;
    let wiring = Program::Plain("wiring.bin", Some("wiring-hi.bin"));
    let c = |name, set, program, frames| MoreCase { name, set, program, frames, events: vec![] };
    let mut v = vec![
        // The plain board's wiring, for comparison, and every board that scrambles its ROMs.
        c("wiring-crush2", "crush2", wiring, 120),
        c("wiring-eyes", "eyes", wiring, 120),
        c("wiring-woodpeck", "woodpeck", Program::Plain("lowgfx.bin", Some("lowgfx-hi.bin")), 120),
        c("lowgfx-lizwiz", "lizwiz", Program::Plain("lowgfx.bin", Some("lowgfx-hi.bin")), 120),
        c("wiring-ponpoko", "ponpoko", wiring, 120),
        c("wiring-crush5", "crush5", wiring, 120),
        c("wiring-mspacmbe", "mspacmbe", wiring, 120),
        c("wiring-mspacmbn", "mspacmbn", wiring, 120),
        c("wiring-mspackpls", "mspackpls", wiring, 120),
        c("wiring-mspacmanbgf", "mspacmanbgf", wiring, 120),
        c("wiring-piranha", "piranha", wiring, 120),
        // The vector, through each board's sync bus controller.
        c("vectors-pacman", "gorkans", Program::Plain("vectors.bin", None), 140),
        c("vectors-piranha", "piranha", Program::Plain("vectors.bin", None), 140),
        c("vectors-nmouse", "nmouse", Program::Plain("vectors.bin", None), 140),
        c("vectors-mspacii", "mspacii", Program::Plain("vectors.bin", None), 140),
        // The protection chips, and the plain board's ports for comparison.
        c("ports-crush2", "crush2", Program::Plain("ports.bin", None), 100),
        c("ports-mbrush", "mbrush", Program::Plain("ports.bin", None), 100),
        c("ports-mspacii", "mspacii", Program::Plain("ports.bin", None), 100),
        c("pengo", "pengojpm", Program::Pengo, 130),
        c("champion", "mschamp", Program::Champion, 200),
    ];
    // Make Trax's chip, with its switches moved as it runs (its answers mix them in).
    let mut maketrax = c("ports-maketrax", "maketrax", Program::Plain("ports.bin", None), 100);
    maketrax.events = vec![
        (12, Event::Dip(Dsw1, 0x03, 0x02)),
        (20, Event::Dip(Dsw1, 0x0C, 0x08)),
        (30, Event::Dip(Dsw1, 0x10, 0x00)),
        (45, Event::Dip(Dsw1, 0x20, 0x00)),
        (55, Event::Dip(Dsw1, 0x03, 0x01)),
        (80, Event::Dip(Dsw1, 0x0C, 0x04)),
    ];
    v.push(maketrax);
    let mut korosuke = c("ports-korosuke", "korosuke", Program::Plain("ports.bin", None), 100);
    korosuke.events = vec![(50, Event::Dip(Dsw1, 0x03, 0x00))];
    v.push(korosuke);
    // Champion Edition: its switches through the latch, then the game switch moved before the
    // program lets the watchdog reset the board.
    let champ = v.iter_mut().find(|c| c.name == "champion").unwrap();
    champ.events = vec![
        (10, Event::Dip(Dsw2, 0x03, 0x02)),
        (20, Event::Dip(Dsw2, 0x04, 0x04)),
        (45, Event::Dip(Dsw2, 0x08, 0x00)),
        (150, Event::Dip(Dsw2, 0x08, 0x08)),
    ];
    v
}

pub fn spec(case: &MoreCase) -> &'static SetSpec {
    sets::find(case.set).unwrap_or_else(|| panic!("{} isn't in cArcade's set list", case.set))
}

fn mods(spec: &SetSpec) -> (pacman::Variant, pacman::Mods) {
    match spec.board {
        Board::Pacman(v, m) => (v, m),
        _ => panic!("{} isn't on the Pac-Man board", spec.name),
    }
}

fn region_size(spec: &SetSpec, tag: &str) -> usize {
    spec.regions.iter().find(|r| r.tag == tag).map_or(0, |r| r.size)
}

/// The regions as the board must see them once unscrambled: the program, the graphics (5E then
/// 5F), the palette and lookup PROMs, the sound PROMs.
fn plain_regions(case: &MoreCase) -> HashMap<&'static str, Vec<u8>> {
    let spec = spec(case);
    let mut cpu = vec![0u8; region_size(spec, "maincpu")];
    match case.program {
        Program::Plain(lo, hi) => {
            let p = program_file(lo);
            cpu[..0x4000].copy_from_slice(&p);
            if let Some(hi) = hi {
                let h = program_file(hi);
                cpu[0x8000..0x8000 + h.len()].copy_from_slice(&h);
            }
        }
        Program::Pengo => {
            let p = program_file("pengo.bin");
            cpu[..0x1000].copy_from_slice(&p[..0x1000]);
            cpu[0x4000..0x8000].copy_from_slice(&p[0x4000..0x8000]);
        }
        Program::Champion => {
            for (b, n) in ["champion1", "champion2"].iter().enumerate() {
                let lo = program_file(&format!("{n}.bin"));
                let hi = program_file(&format!("{n}-hi.bin"));
                cpu[b * 0x8000..b * 0x8000 + 0x4000].copy_from_slice(&lo);
                cpu[b * 0x8000 + 0x4000..b * 0x8000 + 0x8000].copy_from_slice(&hi);
            }
        }
    }
    let mut proms = program_file("palette.bin");
    proms.extend(program_file("lookup.bin"));
    proms.resize(region_size(spec, "proms"), 0);
    let mut namco = program_file("wave.bin");
    namco.extend(program_file("timing.bin"));
    let mut m = HashMap::new();
    m.insert("maincpu", cpu);
    m.insert("gfx1", program_file("gfx.bin"));
    m.insert("proms", proms);
    m.insert("namco", namco);
    m
}

fn bitswap(v: u32, from: &[u32]) -> u32 {
    let n = from.len() as u32;
    from.iter().enumerate().fold(0, |acc, (i, &b)| acc | ((v >> b) & 1) << (n - 1 - i as u32))
}

/// The regions as the set's chips hold them: scrambled the way its board is wired.
fn scrambled_regions(case: &MoreCase) -> HashMap<&'static str, Vec<u8>> {
    let spec = spec(case);
    let (_, m) = mods(spec);
    let mut r = plain_regions(case);
    let cpu = r.get_mut("maincpu").unwrap();
    match m.program {
        Scramble::None => {}
        // init_eyes: "Data lines D3 and D5 swapped" on 0000h-BFFFh.
        Scramble::Eyes => {
            for b in &mut cpu[..0xC000] {
                *b = bitswap(u32::from(*b), &[7, 6, 3, 4, 5, 2, 1, 0]) as u8;
            }
        }
        // init_mspacmbe: A1 and A0 swapped where A2 (of the 4-byte group's base) leaves bit 3 low.
        Scramble::Mspacmbe => {
            let mut i = 0x1000;
            while i < 0x2000 {
                if i & 8 == 0 {
                    cpu.swap(i + 1, i + 2);
                }
                i += 4;
            }
        }
        // init_pengomc1: each 256 bytes' halves exchanged byte for byte, j with j ^ FFh.
        Scramble::PageReversed => {
            for page in (0..0xC000).step_by(0x100) {
                for j in 0..0x80 {
                    cpu.swap(page | j, page | (j ^ 0xFF));
                }
            }
        }
        // init_mspackpls: D3 and D4 swapped on 0000h-9FFFh.
        Scramble::Mspackpls => {
            for b in &mut cpu[..0xA000] {
                *b = bitswap(u32::from(*b), &[7, 6, 5, 3, 4, 2, 1, 0]) as u8;
            }
        }
    }
    let gfx_wiring = if m.program == Scramble::Eyes { GfxWiring::Eyes } else { m.gfx };
    let plain_gfx = r["gfx1"].clone();
    let gfx = r.get_mut("gfx1").unwrap();
    match gfx_wiring {
        GfxWiring::Plain => {}
        // eyes_decode: in each 8 bytes, the byte at j is the stored one at j with bits 0 and 2
        // exchanged, its data bits 4 and 6 exchanged.
        GfxWiring::Eyes => {
            for (g, chunk) in plain_gfx.chunks(8).enumerate() {
                for (j, &v) in chunk.iter().enumerate() {
                    let stored = bitswap(j as u32, &[0, 1, 2]) as usize;
                    gfx[g * 8 + stored] = bitswap(u32::from(v), &[7, 4, 5, 6, 3, 2, 1, 0]) as u8;
                }
            }
        }
        // init_ponpoko undoes this: tiles' halves swapped; each sprite's 32 bytes, 8 at a time,
        // moved one place back (what is at 00h was stored at 18h, 08h at 00h, and so on).
        GfxWiring::Ponpoko => {
            for i in (0..0x1000).step_by(16) {
                for j in 0..8 {
                    gfx.swap(i + j, i + j + 8);
                }
            }
            for i in (0x1000..0x2000).step_by(32) {
                for j in 0..8 {
                    let (a, b, c, d) = (plain_gfx[i + j], plain_gfx[i + j + 8], plain_gfx[i + j + 16], plain_gfx[i + j + 24]);
                    gfx[i + j + 0x18] = a;
                    gfx[i + j] = b;
                    gfx[i + j + 8] = c;
                    gfx[i + j + 0x10] = d;
                }
            }
        }
        // crush4_tilelayout/_spritelayout: the high plane in the second half, the low plane in the
        // low nibbles of the first; the rest of each byte is junk.
        GfxWiring::Crush4 => {
            let mut g = vec![0u8; 0x4000];
            for (i, &v) in plain_gfx.iter().enumerate() {
                g[i] = (v & 0x0F) | ((i as u8).wrapping_mul(7) & 0xF0);
                g[0x2000 + i] = (v & 0xF0) | ((i as u8).wrapping_mul(13) & 0x0F);
            }
            *gfx = g;
        }
    }
    r
}

/// The set's chip files: each one's bytes taken back out of the regions it loads into.
pub fn files(case: &MoreCase) -> Vec<(String, Vec<u8>)> {
    let spec = spec(case);
    let regions = scrambled_regions(case);
    let mut chips: Vec<Vec<u8>> = spec.roms.iter().map(|r| vec![0u8; r.size as usize]).collect();
    for region in spec.regions {
        // A region the board doesn't read (a bootleg board's own PROM): zeros.
        let zeros = vec![0u8; region.size];
        let bytes = regions.get(region.tag).unwrap_or(&zeros);
        for op in region.ops {
            if let Op::Load { rom, at, from, len } = *op {
                chips[rom][from..from + len].copy_from_slice(&bytes[at..at + len]);
            }
        }
    }
    spec.roms.iter().zip(chips).map(|(r, d)| (r.name.to_string(), d)).collect()
}

/// The set loaded the way the app loads it: its files in a zip, recognised by name, the chips put
/// in their regions by the set list, the board built from them.
pub fn board(case: &MoreCase) -> Pacman {
    let files = files(case);
    let refs: Vec<(&str, &[u8])> = files.iter().map(|(n, d)| (n.as_str(), d.as_slice())).collect();
    let zip = Zip::parse(write_stored(&refs)).unwrap();
    let found = sets::identify(Archive { zip, stem: case.set.into() }, &|_| None)
        .unwrap_or_else(|e| panic!("{}: {}", case.name, e.line()));
    assert_eq!(found.spec.name, case.set);
    let regions = found.regions().unwrap();
    let (variant, _) = mods(found.spec);
    Pacman::new(variant, &found.pacman_roms(&regions)).unwrap()
}

/// The set's ports at rest, as MAME's port lists have them.
pub fn rest(case: &MoreCase) -> Ports {
    let r = spec(case).rest;
    Ports { in0: r[0], in1: r[1], dsw1: r[2], dsw2: r[3] }
}

pub fn run_ours(case: &MoreCase) -> Vec<FrameOut> {
    let mut b = board(case);
    let mut ports = rest(case);
    let mut out = Vec::new();
    for frame in 1..=case.frames {
        for &(f, e) in &case.events {
            if f == frame {
                apply(&mut ports, e);
            }
        }
        b.set_ports(ports);
        b.run_frame();
        out.push(FrameOut { video: b.video().to_vec(), audio: b.raw_audio().to_vec() });
    }
    out
}

pub fn run_mame(mame: &Path, case: &MoreCase) -> Vec<FrameOut> {
    run_mame_files(mame, case.name, case.set, case.frames, &case.events, files(case))
}

pub fn reference_path(case: &MoreCase) -> PathBuf {
    crate_dir().join("tests/references/pacman-more").join(format!("{}.txt", case.name))
}

pub fn read_reference(case: &MoreCase) -> Vec<(u64, u64)> {
    let p = reference_path(case);
    let text = fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
    text.lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .map(|l| {
            let w: Vec<&str> = l.split_whitespace().collect();
            (u64::from_str_radix(w[1], 16).unwrap(), u64::from_str_radix(w[2], 16).unwrap())
        })
        .collect()
}

pub fn write_reference(case: &MoreCase, frames: &[FrameOut], mame_version: &str) {
    let mut s = format!(
        "# {}: MAME {} running set {} with cArcade's test program, {} frames.\n\
         # Per frame: the frame, then FNV-1a 64 hashes of its picture (224 x 288 0x00RRGGBB,\n\
         # little-endian u32s) and of its sound (3168 voice sums at 192 kHz, little-endian i16s).\n",
        case.name,
        mame_version,
        case.set,
        frames.len()
    );
    for (i, f) in frames.iter().enumerate() {
        s += &format!("{} {:016x} {:016x}\n", i + 1, video_hash(&f.video), audio_hash(&f.audio));
    }
    let p = reference_path(case);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(&p, s).unwrap();
}
