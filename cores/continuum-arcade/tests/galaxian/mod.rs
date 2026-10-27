//! What the Galaxian-family board tests share: cArcade's own test programs (test-programs/galaxian)
//! laid out as MAME's sets, the inputs each run presses frame by frame, running a program on
//! cArcade's board and on MAME, and the reference files MAME's runs left in
//! tests/references/galaxian.
//!
//! A frame here is VBLANK to VBLANK, as `Galaxian::run_frame` runs it: frame n ends at the n-th
//! VBLANK after power-on, where MAME's frame notifier sees frame number n - 1 and its snapshot is
//! the picture we draw at the end of frame n.

#![allow(dead_code)]

pub mod sound;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use continuum_arcade::galaxian::{self, Galaxian, Ports, Roms, Rotation, Variant};

pub fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// A file of test-programs/galaxian (assembled by scripts/build-test-programs.sh, committed).
pub fn program_file(name: &str) -> Vec<u8> {
    let p = crate_dir().join("test-programs/galaxian").join(name);
    fs::read(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Port {
    In0,
    In1,
    In2,
}

impl Port {
    fn mame_tag(self) -> &'static str {
        match self {
            Port::In0 => ":IN0",
            Port::In1 => ":IN1",
            Port::In2 => ":IN2",
        }
    }
}

/// Something the test does to the inputs at the start of a frame.
#[derive(Clone, Copy, Debug)]
pub enum Event {
    /// A button or stick direction goes down (its active-high bit reads 1).
    Press(Port, u8),
    Release(Port, u8),
    /// A DIP switch: `mask`'s bits set to `value`.
    Dip(Port, u8, u8),
}

/// One test program run on one of MAME's sets.
pub struct Case {
    /// The reference file's name.
    pub name: &'static str,
    /// The program (test-programs/galaxian/<program>.bin).
    pub program: &'static str,
    /// MAME's set the files are laid out for.
    pub set: &'static str,
    pub variant: Variant,
    pub frames: u32,
    /// (frame, event): applied before that frame runs.
    pub events: Vec<(u32, Event)>,
}

pub fn cases() -> Vec<Case> {
    let plain = |name: &'static str, frames: u32| Case {
        name,
        program: name,
        set: "galaxian",
        variant: Variant::Galaxian,
        frames,
        events: vec![],
    };
    vec![
        plain("tiles", 200),
        plain("sprites", 300),
        plain("shots", 200),
        plain("stars", 260),
        plain("raster", 150),
        plain("nmi", 110),
        plain("watchdog", 140),
        Case {
            events: input_script(),
            ..plain("inputs", 140)
        },
        Case {
            name: "mooncrst",
            program: "mooncrst",
            set: "mooncrstuu",
            variant: Variant::MoonCresta { encrypted: false },
            frames: 130,
            events: vec![],
        },
        Case {
            name: "pisces",
            program: "pisces",
            set: "pisces",
            variant: Variant::Pisces { batman2: false },
            frames: 130,
            events: vec![],
        },
        Case {
            name: "batman2",
            program: "pisces",
            set: "batman2",
            variant: Variant::Pisces { batman2: true },
            frames: 130,
            events: vec![],
        },
        Case {
            name: "mooncrst-encrypted",
            program: "mooncrst",
            set: "mooncrst",
            variant: Variant::MoonCresta { encrypted: true },
            frames: 130,
            events: vec![],
        },
    ]
}

/// The inputs run: every button and direction pressed for a few frames in turn, then each DIP
/// switch through its settings.
fn input_script() -> Vec<(u32, Event)> {
    let mut ev = Vec::new();
    let mut f = 10;
    for (port, bits) in [
        (Port::In0, [0x01u8, 0x02, 0x04, 0x08, 0x10, 0x80].as_slice()),
        (Port::In1, [0x01u8, 0x02, 0x04, 0x08, 0x10].as_slice()),
    ] {
        for &b in bits {
            ev.push((f, Event::Press(port, b)));
            ev.push((f + 3, Event::Release(port, b)));
            f += 5;
        }
    }
    // Two held at once: start 1 with coin 1.
    ev.push((f, Event::Press(Port::In1, 0x01)));
    ev.push((f, Event::Press(Port::In0, 0x01)));
    ev.push((f + 3, Event::Release(Port::In1, 0x01)));
    ev.push((f + 3, Event::Release(Port::In0, 0x01)));
    f += 5;
    // The DIP switches: the cabinet and service mode (IN0), coinage (IN1), bonus, lives and the
    // unused one (IN2).
    for (port, mask, values) in [
        (Port::In0, 0x20u8, [0x20u8, 0x00].as_slice()),
        (Port::In0, 0x40, [0x40, 0x00].as_slice()),
        (Port::In1, 0xC0, [0x40, 0x80, 0xC0, 0x00].as_slice()),
        (Port::In2, 0x03, [0x01, 0x02, 0x03, 0x00].as_slice()),
        (Port::In2, 0x04, [0x00, 0x04].as_slice()),
        (Port::In2, 0x08, [0x08, 0x00].as_slice()),
    ] {
        for &v in values {
            ev.push((f, Event::Dip(port, mask, v)));
            f += 3;
        }
    }
    assert!(f < 140, "the inputs script runs past its 140 frames");
    ev
}

/// MAME's factory settings for the galaxian set's ports: everything off, 3 lives.
fn default_ports() -> Ports {
    Ports {
        in0: 0x00,
        in1: 0x00,
        in2: 0x04,
        in3: 0x00,
    }
}

fn apply(ports: &mut Ports, e: Event) {
    let port = match e {
        Event::Press(p, _) | Event::Release(p, _) | Event::Dip(p, _, _) => p,
    };
    let r = match port {
        Port::In0 => &mut ports.in0,
        Port::In1 => &mut ports.in1,
        Port::In2 => &mut ports.in2,
    };
    match e {
        Event::Press(_, b) => *r |= b,
        Event::Release(_, b) => *r &= !b,
        Event::Dip(_, m, v) => *r = (*r & !m) | (v & m),
    }
}

// ---- Moon Cresta's program scrambling, the other way round ------------------------------------
// Written here again from MAME's description of the board, independently of src/galaxian.rs: if
// both were wrong the same way, cArcade would still unscramble what this scrambles, but MAME
// wouldn't, and the comparison with MAME would fail.

/// What the board's decoder makes of a stored byte at an even or odd address.
fn mooncrst_decoder(even: bool, data: u8) -> u8 {
    let mut res = data;
    if data & 0x02 != 0 {
        res ^= 0x40;
    }
    if data & 0x20 != 0 {
        res ^= 0x04;
    }
    if even {
        // bitswap<8>(res, 7,2,5,4,3,6,1,0): bits 6 and 2 trade places.
        let b6 = res >> 6 & 1;
        let b2 = res >> 2 & 1;
        res = (res & !0x44) | b2 << 6 | b6 << 2;
    }
    res
}

fn mooncrst_scramble(plain: &[u8]) -> Vec<u8> {
    let mut inv = [[0u8; 256]; 2];
    for (parity, table) in inv.iter_mut().enumerate() {
        let mut seen = [false; 256];
        for x in 0..=255u8 {
            let y = mooncrst_decoder(parity == 0, x);
            assert!(!seen[usize::from(y)], "not one to one");
            seen[usize::from(y)] = true;
            table[usize::from(y)] = x;
        }
    }
    plain
        .iter()
        .enumerate()
        .map(|(a, &v)| inv[a & 1][usize::from(v)])
        .collect()
}

/// The files of `case`'s MAME set, named as MAME wants them.
pub fn files(case: &Case) -> Vec<(String, Vec<u8>)> {
    let gfx = program_file("gfx.bin");
    let palette = program_file("palette.bin");
    let program = program_file(&format!("{}.bin", case.program));
    let mut f: Vec<(String, Vec<u8>)> = Vec::new();
    let mut split = |names: &[&str], data: &[u8]| {
        for (i, n) in names.iter().enumerate() {
            f.push((n.to_string(), data[i * 0x800..(i + 1) * 0x800].to_vec()));
        }
    };
    match case.set {
        "galaxian" => {
            split(
                &["galmidw.u", "galmidw.v", "galmidw.w", "galmidw.y", "7l"],
                &program,
            );
            split(&["1h.bin"], &gfx[..0x800]);
            split(&["1k.bin"], &gfx[0x1000..0x1800]);
            f.push(("6l.bpr".into(), palette));
        }
        "mooncrstuu" => {
            split(
                &[
                    "smc1f", "smc2f", "smc3f", "smc4f", "e5", "bepr199", "e7", "smc8f",
                ],
                &program,
            );
            split(&["mcs_b", "mcs_d", "mcs_a", "mcs_c"], &gfx);
            f.push(("mmi6331.6l".into(), palette));
        }
        "mooncrst" => {
            let scrambled = mooncrst_scramble(&program);
            split(
                &[
                    "mc1", "mc2", "mc3", "mc4", "mc5.7r", "mc6.8d", "mc7.8e", "mc8",
                ],
                &scrambled,
            );
            split(&["mcs_b", "mcs_d", "mcs_a", "mcs_c"], &gfx);
            f.push(("mmi6331.6l".into(), palette));
        }
        _ => return generic_files(case),
    }
    f
}

/// Any other set: the program and graphics laid out the way the set list says its chips load
/// (the program at 0000h, the graphics as the board's ROMs, the colour PROM).
fn generic_files(case: &Case) -> Vec<(String, Vec<u8>)> {
    use continuum_arcade::sets::{self, Op};
    let spec = sets::find(case.set).unwrap_or_else(|| panic!("{} isn't in the set list", case.set));
    let mut regions = std::collections::HashMap::new();
    let size = |tag: &str| spec.regions.iter().find(|r| r.tag == tag).map_or(0, |r| r.size);
    let mut cpu = program_file(&format!("{}.bin", case.program));
    cpu.resize(size("maincpu"), 0);
    let mut gfx = program_file("gfx.bin");
    if size("gfx1") == 0x1000 {
        // Galaxian's half-size ROMs: the first 2 KB of each plane.
        gfx = [&gfx[..0x800], &gfx[0x1000..0x1800]].concat();
    }
    regions.insert("maincpu", cpu);
    regions.insert("gfx1", gfx);
    regions.insert("proms", program_file("palette.bin"));
    let mut chips: Vec<Vec<u8>> = spec.roms.iter().map(|r| vec![0u8; r.size as usize]).collect();
    for region in spec.regions {
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

/// The same files in the board's regions (where MAME's set loads each one).
pub fn roms(case: &Case) -> Roms {
    let files = files(case);
    let get = |n: &str| {
        files
            .iter()
            .find(|(name, _)| name == n)
            .map(|(_, d)| d.clone())
            .unwrap()
    };
    let cat = |names: &[&str]| names.iter().flat_map(|n| get(n)).collect::<Vec<u8>>();
    let (mut program, gfx, palette) = match case.set {
        "galaxian" => (
            cat(&["galmidw.u", "galmidw.v", "galmidw.w", "galmidw.y", "7l"]),
            cat(&["1h.bin", "1k.bin"]),
            get("6l.bpr"),
        ),
        "mooncrstuu" => (
            cat(&[
                "smc1f", "smc2f", "smc3f", "smc4f", "e5", "bepr199", "e7", "smc8f",
            ]),
            cat(&["mcs_b", "mcs_d", "mcs_a", "mcs_c"]),
            get("mmi6331.6l"),
        ),
        "mooncrst" => (
            cat(&[
                "mc1", "mc2", "mc3", "mc4", "mc5.7r", "mc6.8d", "mc7.8e", "mc8",
            ]),
            cat(&["mcs_b", "mcs_d", "mcs_a", "mcs_c"]),
            get("mmi6331.6l"),
        ),
        _ => {
            // Through the set list, as the app loads it.
            use continuum_arcade::sets::{self, Archive};
            use continuum_arcade::zip::{write_stored, Zip};
            let refs: Vec<(&str, &[u8])> = files.iter().map(|(n, d)| (n.as_str(), d.as_slice())).collect();
            let zip = Zip::parse(write_stored(&refs)).unwrap();
            let found = sets::identify(Archive { zip, stem: case.set.into() }, &|_| None)
                .unwrap_or_else(|e| panic!("{}: {}", case.name, e.line()));
            let regions = found.regions().unwrap();
            return found.galaxian_roms(&regions, 0);
        }
    };
    program.resize(0x4000, 0);
    Roms {
        program,
        gfx,
        palette,
        sound: Vec::new(),
        protection_bits: 0,
    }
}

/// Runs `case` on cArcade's board: each frame's picture, upright, 0x00RRGGBB.
pub fn run_ours(case: &Case) -> Vec<Vec<u32>> {
    // The set's own monitor rotation (Batman Part 2's is the other way).
    let rotation = match continuum_arcade::sets::find(case.set).map(|s| s.board) {
        Some(continuum_arcade::sets::Board::Galaxian(_, r, _)) => r,
        _ => Rotation::Rot90,
    };
    let mut board = Galaxian::new(case.variant, rotation, &roms(case)).expect("the test set loads");
    let mut ports = default_ports();
    let mut out = Vec::new();
    for frame in 1..=case.frames {
        for &(f, e) in &case.events {
            if f == frame {
                apply(&mut ports, e);
            }
        }
        board.set_ports(ports);
        board.run_frame();
        out.push(board.video().to_vec());
    }
    out
}

pub fn fnv64(bytes: impl Iterator<Item = u8>) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

pub fn video_hash(v: &[u32]) -> u64 {
    fnv64(v.iter().flat_map(|p| p.to_le_bytes()))
}

pub fn reference_path(case: &Case) -> PathBuf {
    crate_dir()
        .join("tests/references/galaxian")
        .join(format!("{}.txt", case.name))
}

/// A reference file: one picture hash per frame.
pub fn read_reference(case: &Case) -> Vec<u64> {
    let p = reference_path(case);
    let text = fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
    text.lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .enumerate()
        .map(|(i, l)| {
            let w: Vec<&str> = l.split_whitespace().collect();
            assert_eq!(
                w[0].parse::<usize>().unwrap(),
                i + 1,
                "{}: frames out of order",
                p.display()
            );
            u64::from_str_radix(w[1], 16).unwrap()
        })
        .collect()
}

pub fn write_reference(case: &Case, frames: &[Vec<u32>], mame_version: &str) {
    let mut s = format!(
        "# {}: MAME {} running set {} with cArcade's test program {}, {} frames.\n\
         # Per frame: the frame, then the FNV-1a 64 hash of its picture (224 x 768 0x00RRGGBB,\n\
         # little-endian u32s).\n",
        case.name,
        mame_version,
        case.set,
        case.program,
        frames.len()
    );
    for (i, f) in frames.iter().enumerate() {
        s += &format!("{} {:016x}\n", i + 1, video_hash(f));
    }
    let p = reference_path(case);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(&p, s).unwrap();
}

// ---- MAME ---------------------------------------------------------------------------------

/// The local reference MAME, if this machine has one: $CARCADE_MAME, or the one built in this
/// project's scratch space (/projects/sandbox/.scratch/mame, see its READY file).
pub fn mame() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("CARCADE_MAME") {
        let p = PathBuf::from(p);
        return p.is_file().then_some(p);
    }
    let p = PathBuf::from("/projects/sandbox/.scratch/mame/mame-mame0289/arcaderef");
    p.is_file().then_some(p)
}

pub fn mame_version(mame: &Path) -> String {
    let out = Command::new(mame)
        .arg("-version")
        .env("SDL_VIDEODRIVER", "dummy")
        .env("SDL_AUDIODRIVER", "dummy")
        .output()
        .expect("MAME runs");
    String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .next()
        .unwrap_or("?")
        .to_string()
}

/// Runs `case` on MAME: the set's files in a fresh folder, and a Lua script that snapshots every
/// frame and applies the inputs.
pub fn run_mame(mame: &Path, case: &Case) -> Vec<Vec<u32>> {
    let work = crate_dir()
        .join("target/mame-runs/galaxian")
        .join(case.name);
    let _ = fs::remove_dir_all(&work);
    let roms = work.join("roms").join(case.set);
    fs::create_dir_all(&roms).unwrap();
    for (name, data) in files(case) {
        fs::write(roms.join(name), data).unwrap();
    }
    // MAME takes in an input change at its next VBLANK: inputs for frame n are set at frame
    // notifier n - 3, a frame before the VBLANK that starts frame n.
    let mut lua = String::from(
        "local scr = manager.machine.screens[':screen']\nlocal io = manager.machine.ioport.ports\nlocal ev = {}\n",
    );
    for &(f, e) in &case.events {
        assert!(f >= 3, "inputs can only change from frame 3 on");
        let k = f as i64 - 3;
        let (port, mask, act) = match e {
            Event::Press(p, b) => (p, b, "f:set_value(1)".to_string()),
            Event::Release(p, b) => (p, b, "f:set_value(0)".to_string()),
            Event::Dip(p, m, v) => (p, m, format!("f.user_value = {v}")),
        };
        lua += &format!(
            "ev[{k}] = ev[{k}] or {{}}\ntable.insert(ev[{k}], function() local f = io['{}']:field({mask}) {act} end)\n",
            port.mame_tag()
        );
    }
    lua += &format!(
        "if ev[-1] then for _, a in ipairs(ev[-1]) do a() end end\n\
         sub = emu.add_machine_frame_notifier(function()\n\
           local k = scr:frame_number()\n\
           if k < {n} then scr:snapshot(string.format('f%05d.png', k)) end\n\
           if ev[k] then for _, a in ipairs(ev[k]) do a() end end\n\
           if k >= {n} + 2 then manager.machine:exit() end\n\
         end)\n",
        n = case.frames
    );
    fs::write(work.join("run.lua"), lua).unwrap();
    let out = Command::new(mame)
        .current_dir(&work)
        .env("SDL_VIDEODRIVER", "dummy")
        .env("SDL_AUDIODRIVER", "dummy")
        .args([
            case.set,
            "-rompath",
            "roms",
            "-video",
            "none",
            "-sound",
            "none",
            "-nothrottle",
        ])
        .args([
            "-skip_gameinfo",
            "-autoboot_script",
            "run.lua",
            "-snapshot_directory",
            "snap",
        ])
        .args([
            "-cfg_directory",
            "cfg",
            "-nvram_directory",
            "nvram",
            "-seconds_to_run",
        ])
        .arg((case.frames / 60 + 5).to_string())
        .output()
        .expect("MAME runs");
    let log =
        String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr);
    fs::write(work.join("mame.log"), &log).unwrap();
    assert!(
        !log.contains("LUA ERROR"),
        "MAME's Lua script failed:\n{log}"
    );
    (0..case.frames as usize)
        .map(|k| read_png(&work.join("snap").join(format!("f{k:05}.png"))))
        .collect()
}

pub fn read_png(p: &Path) -> Vec<u32> {
    let file = fs::File::open(p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
    let mut reader = png::Decoder::new(file).read_info().unwrap();
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).unwrap();
    assert_eq!(
        (info.width, info.height),
        (galaxian::WIDTH as u32, galaxian::HEIGHT as u32),
        "{}",
        p.display()
    );
    let bpp = info.color_type.samples();
    buf[..info.buffer_size()]
        .chunks_exact(bpp)
        .map(|c| u32::from(c[0]) << 16 | u32::from(c[1]) << 8 | u32::from(c[2]))
        .collect()
}

/// Writes a frame as a PNG (for looking at a difference).
pub fn write_png(p: &Path, v: &[u32]) {
    let file = fs::File::create(p).unwrap();
    let mut enc = png::Encoder::new(file, galaxian::WIDTH as u32, galaxian::HEIGHT as u32);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    let mut w = enc.write_header().unwrap();
    let data: Vec<u8> = v
        .iter()
        .flat_map(|&p| [(p >> 16) as u8, (p >> 8) as u8, p as u8])
        .collect();
    w.write_image_data(&data).unwrap();
}

/// Where two runs first differ, in words, or None if they don't. The first differing pair of
/// frames is written next to MAME's run for a look.
pub fn first_difference(case: &Case, ours: &[Vec<u32>], theirs: &[Vec<u32>]) -> Option<String> {
    for (i, (o, t)) in ours.iter().zip(theirs).enumerate() {
        if o != t {
            let n = o.iter().zip(t).filter(|(a, b)| a != b).count();
            let p = o.iter().zip(t).position(|(a, b)| a != b).unwrap();
            let dir = crate_dir()
                .join("target/mame-runs/galaxian")
                .join(case.name);
            if dir.is_dir() {
                write_png(&dir.join(format!("ours-{:05}.png", i + 1)), o);
                write_png(&dir.join(format!("mame-{:05}.png", i + 1)), t);
            }
            return Some(format!(
                "{}: frame {} picture: {n} pixels differ, the first at ({}, {}): ours {:06x}, MAME {:06x}",
                case.name,
                i + 1,
                p % galaxian::WIDTH,
                p / galaxian::WIDTH,
                o[p],
                t[p]
            ));
        }
    }
    None
}
