//! What the Pac-Man board tests share: cArcade's own test programs (test-programs/pacman) as
//! MAME's sets lay them out, the inputs each run presses frame by frame, running a program on
//! cArcade's board and on MAME, and the reference files MAME's runs left in tests/references.
//!
//! MAME runs ten frames past the last one compared: its sound reaches the WAV file a little
//! behind the picture.
//!
//! A frame here is VBLANK to VBLANK, as `Pacman::run_frame` runs it: frame n ends at the n-th
//! VBLANK after power-on, where MAME's frame notifier sees frame number n - 1 and its snapshot is
//! the picture we draw at the end of frame n.

#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use continuum_arcade::pacman::{self, Mods, Pacman, Ports, Roms, Variant};

pub fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// A file of test-programs/pacman (assembled by scripts/build-test-programs.sh, committed).
pub fn program_file(name: &str) -> Vec<u8> {
    let p = crate_dir().join("test-programs/pacman").join(name);
    fs::read(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Port {
    In0,
    In1,
    Dsw1,
}

impl Port {
    fn mame_tag(self) -> &'static str {
        match self {
            Port::In0 => ":IN0",
            Port::In1 => ":IN1",
            Port::Dsw1 => ":DSW1",
        }
    }
}

/// Something the test does to the inputs at the start of a frame.
#[derive(Clone, Copy, Debug)]
pub enum Event {
    /// A button or stick direction goes down (its active-low bit reads 0).
    Press(Port, u8),
    Release(Port, u8),
    /// A DIP switch (or MAME's cabinet setting): `mask`'s bits set to `value`.
    Dip(Port, u8, u8),
}

/// One test program run on one of MAME's sets.
pub struct Case {
    /// The program, and the name of its reference file.
    pub name: &'static str,
    /// MAME's set the files are laid out for.
    pub set: &'static str,
    pub variant: Variant,
    /// What the set's board adds to the plain one.
    pub mods: Mods,
    pub frames: u32,
    /// (frame, event): applied before that frame runs.
    pub events: Vec<(u32, Event)>,
}

pub fn cases() -> Vec<Case> {
    let mut cases = vec![
        Case {
            name: "video",
            set: "pacman",
            variant: Variant::Plain,
            mods: Mods::NONE,
            frames: 120,
            events: vec![],
        },
        Case {
            name: "sprites",
            set: "pacman",
            variant: Variant::Plain,
            mods: Mods::NONE,
            frames: 300,
            events: vec![],
        },
        Case {
            name: "sound",
            set: "pacman",
            variant: Variant::Plain,
            mods: Mods::NONE,
            frames: 240,
            events: vec![],
        },
        Case {
            name: "irq",
            set: "pacman",
            variant: Variant::Plain,
            mods: Mods::NONE,
            frames: 200,
            events: vec![],
        },
        Case {
            name: "watchdog",
            set: "pacman",
            variant: Variant::Plain,
            mods: Mods::NONE,
            frames: 260,
            events: vec![],
        },
        Case {
            name: "mspacman",
            set: "mspacman",
            variant: Variant::MsPacman,
            mods: Mods::NONE,
            frames: 30,
            events: vec![],
        },
        Case {
            name: "pacplus",
            set: "pacplus",
            variant: Variant::PacPlus,
            mods: Mods::NONE,
            frames: 80,
            events: vec![],
        },
        Case {
            name: "upper",
            set: "mspacmab",
            variant: Variant::UpperRom,
            mods: Mods::NONE,
            frames: 20,
            events: vec![],
        },
    ];
    cases.push(Case {
        name: "inputs",
        set: "pacman",
        variant: Variant::Plain,
        mods: Mods::NONE,
        frames: 150,
        events: input_script(),
    });
    cases
}

/// The inputs run: every button and direction pressed for a few frames in turn, then each DIP
/// switch through its settings, then the cabinet set to cocktail.
fn input_script() -> Vec<(u32, Event)> {
    let mut ev = Vec::new();
    let mut f = 10;
    for (port, bits) in [
        (
            Port::In0,
            [0x01u8, 0x02, 0x04, 0x08, 0x20, 0x40, 0x80].as_slice(),
        ),
        (Port::In1, [0x01u8, 0x02, 0x04, 0x08, 0x20, 0x40].as_slice()),
    ] {
        for &b in bits {
            ev.push((f, Event::Press(port, b)));
            ev.push((f + 4, Event::Release(port, b)));
            f += 6;
        }
    }
    // Two held at once: start 1 with coin 1.
    ev.push((f, Event::Press(Port::In1, 0x20)));
    ev.push((f, Event::Press(Port::In0, 0x20)));
    ev.push((f + 3, Event::Release(Port::In1, 0x20)));
    ev.push((f + 3, Event::Release(Port::In0, 0x20)));
    f += 5;
    // The DIP switches: coinage, lives, bonus life, difficulty, ghost names.
    for (mask, values) in [
        (0x03u8, [0x03u8, 0x02, 0x00, 0x01].as_slice()),
        (0x0C, [0x00, 0x04, 0x0C, 0x08].as_slice()),
        (0x30, [0x10, 0x20, 0x30, 0x00].as_slice()),
        (0x40, [0x00, 0x40].as_slice()),
        (0x80, [0x00, 0x80].as_slice()),
    ] {
        for &v in values {
            ev.push((f, Event::Dip(Port::Dsw1, mask, v)));
            f += 2;
        }
    }
    // The rack test switch, the test switch, and the cabinet.
    ev.push((f, Event::Dip(Port::In0, 0x10, 0x00)));
    ev.push((f + 3, Event::Dip(Port::In0, 0x10, 0x10)));
    f += 5;
    ev.push((f, Event::Dip(Port::In1, 0x10, 0x00)));
    ev.push((f + 3, Event::Dip(Port::In1, 0x10, 0x10)));
    f += 5;
    ev.push((f, Event::Dip(Port::In1, 0x80, 0x00)));
    assert!(f < 150, "the inputs script runs past its 150 frames");
    ev
}

fn apply(ports: &mut Ports, e: Event) {
    let port = match e {
        Event::Press(p, _) | Event::Release(p, _) | Event::Dip(p, _, _) => p,
    };
    let r = match port {
        Port::In0 => &mut ports.in0,
        Port::In1 => &mut ports.in1,
        Port::Dsw1 => &mut ports.dsw1,
    };
    match e {
        Event::Press(_, b) => *r &= !b,
        Event::Release(_, b) => *r |= b,
        Event::Dip(_, m, v) => *r = (*r & !m) | (v & m),
    }
}

// ---- The daughterboard's and Pac-Man Plus's scrambling, the other way round -----------------
// Written here again from MAME's description of the boards, independently of src/pacman.rs: if
// both were wrong the same way, cArcade would still unscramble what this scrambles, but MAME
// wouldn't, and the comparison with MAME would fail.

/// `from[0]` names the source bit of the result's top bit, and so on down to bit 0.
fn bitswap(v: u32, from: &[u32]) -> u32 {
    let n = from.len() as u32;
    let mut out = 0;
    for (i, &b) in from.iter().enumerate() {
        out |= ((v >> b) & 1) << (n - 1 - i as u32);
    }
    out
}

fn invert8(f: impl Fn(u8) -> u8) -> [u8; 256] {
    let mut inv = [0u8; 256];
    let mut seen = [false; 256];
    for x in 0..=255u8 {
        let y = f(x);
        assert!(!seen[usize::from(y)], "not one to one");
        seen[usize::from(y)] = true;
        inv[usize::from(y)] = x;
    }
    inv
}

/// Ms. Pac-Man's u5, u6 and u7 as they must be stored for the daughterboard to show `view`
/// (u7 as seen at 3000h-3FFFh, then u5/u6 as seen at 8000h-97FFh).
fn msp_scramble(u7_view: &[u8], u56_view: &[u8]) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    let data = invert8(|x| bitswap(u32::from(x), &[0, 4, 5, 7, 6, 3, 2, 1]) as u8);
    let mut u7 = vec![0u8; 0x1000];
    for i in 0..0x1000u32 {
        let a = bitswap(i, &[11, 3, 7, 9, 10, 8, 6, 5, 4, 2, 1, 0]) as usize;
        u7[a] = data[usize::from(u7_view[i as usize])];
    }
    let mut u5 = vec![0u8; 0x800];
    let mut u6 = vec![0u8; 0x1000];
    for i in 0..0x800u32 {
        let a5 = bitswap(i, &[8, 7, 5, 9, 10, 6, 3, 4, 2, 1, 0]) as usize;
        let a6 = bitswap(i, &[3, 7, 9, 10, 8, 6, 5, 4, 2, 1, 0]) as usize;
        u5[a5] = data[usize::from(u56_view[i as usize])];
        u6[0x800 + a6] = data[usize::from(u56_view[0x800 + i as usize])];
        u6[a6] = data[usize::from(u56_view[0x1000 + i as usize])];
    }
    (u5, u6, u7)
}

/// Pac-Man Plus's program as it must be stored for its decoder to give `plain`.
fn pacplus_scramble(plain: &[u8]) -> Vec<u8> {
    const WAYS: [([u32; 8], u8); 6] = [
        ([7, 6, 5, 4, 3, 2, 1, 0], 0x00),
        ([7, 6, 5, 4, 3, 2, 1, 0], 0x28),
        ([6, 1, 3, 2, 5, 7, 0, 4], 0x96),
        ([6, 1, 5, 2, 3, 7, 0, 4], 0xBE),
        ([0, 3, 7, 6, 4, 2, 1, 5], 0xD5),
        ([0, 3, 4, 6, 7, 2, 1, 5], 0xDD),
    ];
    const PICK: [usize; 32] = [
        0, 2, 4, 2, 4, 0, 4, 2, 2, 0, 2, 2, 4, 0, 4, 2, 2, 2, 4, 0, 4, 2, 4, 0, 0, 4, 0, 4, 4, 2,
        4, 2,
    ];
    let inverses: Vec<[u8; 256]> = WAYS
        .iter()
        .map(|(bits, xor)| invert8(|x| bitswap(u32::from(x), bits) as u8 ^ xor))
        .collect();
    plain
        .iter()
        .enumerate()
        .map(|(a, &v)| {
            let sel = (a & 1)
                | (a >> 2 & 1) << 1
                | (a >> 5 & 1) << 2
                | (a >> 7 & 1) << 3
                | (a >> 9 & 1) << 4;
            let way = PICK[sel] ^ (a >> 11 & 1);
            inverses[way][usize::from(v)]
        })
        .collect()
}

/// The files of `case`'s MAME set, named as MAME wants them.
pub fn files(case: &Case) -> Vec<(String, Vec<u8>)> {
    let gfx = program_file("gfx.bin");
    let mut f: Vec<(String, Vec<u8>)> = Vec::new();
    let split4 = |prefix: &[&str], program: &[u8], f: &mut Vec<(String, Vec<u8>)>| {
        for (i, n) in prefix.iter().enumerate() {
            f.push((
                n.to_string(),
                program[i * 0x1000..(i + 1) * 0x1000].to_vec(),
            ));
        }
    };
    let (tiles, sprites) = (gfx[..0x1000].to_vec(), gfx[0x1000..].to_vec());
    match case.set {
        "pacman" => {
            let p = program_file(&format!("{}.bin", case.name));
            split4(
                &["pacman.6e", "pacman.6f", "pacman.6h", "pacman.6j"],
                &p,
                &mut f,
            );
            f.push(("pacman.5e".into(), tiles));
            f.push(("pacman.5f".into(), sprites));
            f.push(("82s123.7f".into(), program_file("palette.bin")));
            f.push(("82s126.4a".into(), program_file("lookup.bin")));
        }
        "pacplus" => {
            let p = pacplus_scramble(&program_file("pacplus.bin"));
            split4(
                &["pacplus.6e", "pacplus.6f", "pacplus.6h", "pacplus.6j"],
                &p,
                &mut f,
            );
            f.push(("pacplus.5e".into(), tiles));
            f.push(("pacplus.5f".into(), sprites));
            f.push(("pacplus.7f".into(), program_file("palette.bin")));
            f.push(("pacplus.4a".into(), program_file("lookup.bin")));
        }
        "mspacman" => {
            let p = program_file("mspacman.bin");
            split4(
                &["pacman.6e", "pacman.6f", "pacman.6h", "pacman.6j"],
                &p,
                &mut f,
            );
            let (u5, u6, u7) = msp_scramble(
                &program_file("mspacman-u7.bin"),
                &program_file("mspacman-u56.bin"),
            );
            f.push(("u5".into(), u5));
            f.push(("u6".into(), u6));
            f.push(("u7".into(), u7));
            f.push(("5e".into(), tiles));
            f.push(("5f".into(), sprites));
            f.push(("82s123.7f".into(), program_file("palette.bin")));
            f.push(("82s126.4a".into(), program_file("lookup.bin")));
        }
        "mspacmab" => {
            let p = program_file("upper.bin");
            split4(&["boot1", "boot2", "boot3", "boot4"], &p, &mut f);
            let hi = program_file("upper-hi.bin");
            f.push(("boot5".into(), hi[..0x1000].to_vec()));
            f.push(("boot6".into(), hi[0x1000..0x2000].to_vec()));
            f.push(("5e".into(), tiles));
            f.push(("5f".into(), sprites));
            f.push(("82s123.7f".into(), program_file("palette.bin")));
            f.push(("82s126.4a".into(), program_file("lookup.bin")));
        }
        other => panic!("no layout for set {other}"),
    }
    f.push(("82s126.1m".into(), program_file("wave.bin")));
    f.push(("82s126.3m".into(), program_file("timing.bin")));
    f
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
    let mut program = vec![0u8; 0x10000];
    let mut put = |at: usize, d: &[u8]| program[at..at + d.len()].copy_from_slice(d);
    let pre = if case.set == "pacplus" {
        "pacplus"
    } else {
        "pacman"
    };
    match case.set {
        "mspacmab" => {
            for (i, n) in ["boot1", "boot2", "boot3", "boot4", "boot5", "boot6"]
                .iter()
                .enumerate()
            {
                put(
                    if i < 4 {
                        i * 0x1000
                    } else {
                        0x8000 + (i - 4) * 0x1000
                    },
                    &get(n),
                );
            }
        }
        _ => {
            for (i, s) in ["6e", "6f", "6h", "6j"].iter().enumerate() {
                put(i * 0x1000, &get(&format!("{pre}.{s}")));
            }
            if case.set == "mspacman" {
                put(0x8000, &get("u5"));
                put(0x9000, &get("u6"));
                put(0xB000, &get("u7"));
            }
        }
    }
    let (g5e, g5f, pal, look) = match case.set {
        "pacman" => ("pacman.5e", "pacman.5f", "82s123.7f", "82s126.4a"),
        "pacplus" => ("pacplus.5e", "pacplus.5f", "pacplus.7f", "pacplus.4a"),
        _ => ("5e", "5f", "82s123.7f", "82s126.4a"),
    };
    let mut gfx = get(g5e);
    gfx.extend(get(g5f));
    Roms {
        program,
        gfx,
        palette: get(pal),
        lookup: get(look),
        wave: get("82s126.1m"),
        mods: case.mods,
    }
}

/// One frame's output: the picture (upright, 0x00RRGGBB) and the sound (192 kHz voice sums).
pub struct FrameOut {
    pub video: Vec<u32>,
    pub audio: Vec<i32>,
}

/// Runs `case` on cArcade's board.
pub fn run_ours(case: &Case) -> Vec<FrameOut> {
    let mut board = Pacman::new(case.variant, &roms(case)).expect("the test set loads");
    let mut ports = Ports::default();
    let mut out = Vec::new();
    for frame in 1..=case.frames {
        for &(f, e) in &case.events {
            if f == frame {
                apply(&mut ports, e);
            }
        }
        board.set_ports(ports);
        board.run_frame();
        out.push(FrameOut {
            video: board.video().to_vec(),
            audio: board.raw_audio().to_vec(),
        });
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

pub fn audio_hash(a: &[i32]) -> u64 {
    fnv64(a.iter().flat_map(|&s| (s as i16).to_le_bytes()))
}

pub fn reference_path(case: &Case) -> PathBuf {
    crate_dir()
        .join("tests/references/pacman")
        .join(format!("{}.txt", case.name))
}

/// A reference file: one (picture hash, sound hash) per frame.
pub fn read_reference(case: &Case) -> Vec<(u64, u64)> {
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
            (
                u64::from_str_radix(w[1], 16).unwrap(),
                u64::from_str_radix(w[2], 16).unwrap(),
            )
        })
        .collect()
}

pub fn write_reference(case: &Case, frames: &[FrameOut], mame_version: &str) {
    let mut s = format!(
        "# {}: MAME {} running set {} with cArcade's test program, {} frames.\n\
         # Per frame: the frame, then FNV-1a 64 hashes of its picture (224 x 288 0x00RRGGBB,\n\
         # little-endian u32s) and of its sound (3168 voice sums at 192 kHz, little-endian i16s).\n",
        case.name, mame_version, case.set, frames.len()
    );
    for (i, f) in frames.iter().enumerate() {
        s += &format!(
            "{} {:016x} {:016x}\n",
            i + 1,
            video_hash(&f.video),
            audio_hash(&f.audio)
        );
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

/// Runs `case` on MAME: the set's files in a fresh folder, a Lua script that snapshots every frame
/// and applies the inputs, and the sound written at 192 kHz.
pub fn run_mame(mame: &Path, case: &Case) -> Vec<FrameOut> {
    let work = crate_dir().join("target/mame-runs").join(case.name);
    let _ = fs::remove_dir_all(&work);
    let roms = work.join("roms").join(case.set);
    fs::create_dir_all(&roms).unwrap();
    for (name, data) in files(case) {
        fs::write(roms.join(name), data).unwrap();
    }
    // MAME takes in an input change at its next VBLANK: inputs for frame n are set at frame
    // notifier n - 3, a frame before the VBLANK that starts frame n.
    let mut lua = String::from("local scr = manager.machine.screens[':screen']\nlocal io = manager.machine.ioport.ports\nlocal ev = {}\n");
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
           if k >= {n} + 9 then manager.machine:exit() end\n\
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
            "-samplerate",
            "192000",
        ])
        .args(["-wavwrite", "sound.wav", "-seconds_to_run"])
        .arg((case.frames / 60 + 10).to_string())
        .output()
        .expect("MAME runs");
    let log =
        String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr);
    fs::write(work.join("mame.log"), &log).unwrap();
    assert!(
        !log.contains("LUA ERROR"),
        "MAME's Lua script failed:\n{log}"
    );
    let wav =
        fs::read(work.join("sound.wav")).unwrap_or_else(|_| panic!("MAME wrote no sound:\n{log}"));
    let data = wav
        .windows(4)
        .position(|w| w == b"data")
        .expect("a WAV data chunk")
        + 8;
    let samples: Vec<i32> = wav[data..]
        .chunks_exact(2)
        .map(|c| {
            let s = f64::from(i16::from_le_bytes([c[0], c[1]]));
            // MAME's sample is the voice sum / 384 × 32768, cut toward zero: back to the sum.
            (s * 384.0 / 32768.0).round() as i32
        })
        .collect();
    let per = pacman::RAW_SAMPLES_PER_FRAME;
    assert!(
        samples.len() >= per * case.frames as usize,
        "MAME's sound is short:\n{log}"
    );
    (0..case.frames as usize)
        .map(|k| {
            let png = work.join("snap").join(format!("f{k:05}.png"));
            FrameOut {
                video: read_png(&png),
                audio: samples[k * per..(k + 1) * per].to_vec(),
            }
        })
        .collect()
}

fn read_png(p: &Path) -> Vec<u32> {
    let file = fs::File::open(p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
    let mut reader = png::Decoder::new(file).read_info().unwrap();
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).unwrap();
    assert_eq!(
        (info.width, info.height),
        (pacman::WIDTH as u32, pacman::HEIGHT as u32),
        "{}",
        p.display()
    );
    let bpp = info.color_type.samples();
    buf[..info.buffer_size()]
        .chunks_exact(bpp)
        .map(|c| u32::from(c[0]) << 16 | u32::from(c[1]) << 8 | u32::from(c[2]))
        .collect()
}

/// Where two runs first differ, in words, or None if they don't.
pub fn first_difference(name: &str, ours: &[FrameOut], theirs: &[FrameOut]) -> Option<String> {
    for (i, (o, t)) in ours.iter().zip(theirs).enumerate() {
        if o.video != t.video {
            let n = o.video.iter().zip(&t.video).filter(|(a, b)| a != b).count();
            let p = o
                .video
                .iter()
                .zip(&t.video)
                .position(|(a, b)| a != b)
                .unwrap();
            return Some(format!(
                "{name}: frame {} picture: {n} pixels differ, the first at ({}, {}): ours {:06x}, MAME {:06x}",
                i + 1, p % pacman::WIDTH, p / pacman::WIDTH, o.video[p], t.video[p]
            ));
        }
        if o.audio != t.audio {
            let n = o.audio.iter().zip(&t.audio).filter(|(a, b)| a != b).count();
            let p = o
                .audio
                .iter()
                .zip(&t.audio)
                .position(|(a, b)| a != b)
                .unwrap();
            return Some(format!(
                "{name}: frame {} sound: {n} of {} samples differ, the first at {p}: ours {}, MAME {}",
                i + 1, o.audio.len(), o.audio[p], t.audio[p]
            ));
        }
    }
    None
}
