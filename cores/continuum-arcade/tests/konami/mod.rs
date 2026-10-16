//! What the Konami-board tests share: cArcade's own test programs (test-programs/konami) laid out
//! as MAME's sets (scramble, theend, scobra, frogger, amidar), the inputs each run presses,
//! running them on cArcade's board and on MAME, and the reference files MAME's runs left in
//! tests/references/konami.
//!
//! Each run gives three things: every frame's picture (which must match MAME's exactly), the sound
//! Z80's every instruction (its registers and cycle count, which must match MAME's trace line for
//! line), and the sound, which goes through the filter circuit cArcade approximates and so is
//! compared by level and make-up (galaxian/sound.rs's measures), segment by segment.
//!
//! A frame is VBLANK to VBLANK, as in tests/galaxian: our frame n is MAME's snapshot at frame
//! notifier n - 1.

#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use continuum_arcade::galaxian::{self, Galaxian, Konami, Ports, Roms, Rotation, Variant};
use continuum_arcade::z80::Z80;

pub fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

pub fn program_file(dir: &str, name: &str) -> Vec<u8> {
    let p = crate_dir().join("test-programs").join(dir).join(name);
    fs::read(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Port {
    In0,
    In1,
    In2,
}

impl Port {
    fn tag(self) -> &'static str {
        match self {
            Port::In0 => ":IN0",
            Port::In1 => ":IN1",
            Port::In2 => ":IN2",
        }
    }
}

/// Something the test does to the inputs at the start of a frame. The controls are active low.
#[derive(Clone, Copy, Debug)]
pub enum Event {
    Press(Port, u8),
    Release(Port, u8),
    /// A DIP switch: `mask`'s bits set to `value`.
    Dip(Port, u8, u8),
}

pub struct Case {
    pub name: &'static str,
    pub main: &'static str,
    pub sound: &'static str,
    pub set: &'static str,
    pub variant: Variant,
    pub frames: u32,
    pub events: Vec<(u32, Event)>,
    /// MAME's ports with nothing pressed and the switches at the factory settings.
    pub ports: Ports,
    pub protection_bits: u8,
}

pub fn cases() -> Vec<Case> {
    let k = |name, main, sound, set, konami, ports: [u8; 4], prot| Case {
        name,
        main,
        sound,
        set,
        variant: Variant::Konami(konami),
        frames: 260,
        events: input_script(name == "amidar"),
        ports: Ports { in0: ports[0], in1: ports[1], in2: ports[2], in3: ports[3] },
        protection_bits: prot,
    };
    vec![
        k("scramble", "kscramble", "ksound", "scramble", Konami::Scramble, [0xFF, 0xFC, 0xF1, 0x00], 0xA0),
        k("theend", "kscramble", "ksound", "theend", Konami::TheEnd, [0xFF, 0xFC, 0xF1, 0x00], 0xA0),
        k("scobra", "kscobra", "ksound", "scobra", Konami::SuperCobra, [0xFF, 0xFD, 0xF3, 0x00], 0),
        k("frogger", "kfrogger", "kfsound", "frogger", Konami::Frogger, [0xFF, 0xFC, 0xF1, 0x00], 0),
        k("amidar", "kturtles", "ksound", "amidar", Konami::Turtles, [0xFF, 0xFF, 0xF1, 0xFF], 0),
    ]
}

/// Every bit of IN0 and IN1 pressed in turn, then IN2's non-switch bits, then three DIP switches
/// (Amidar's IN2 has two one-bit switches where the others have the coinage pair and the cabinet).
fn input_script(amidar: bool) -> Vec<(u32, Event)> {
    let mut ev = Vec::new();
    let mut f = 10;
    for port in [Port::In0, Port::In1] {
        for b in 0..8 {
            ev.push((f, Event::Press(port, 1 << b)));
            ev.push((f + 2, Event::Release(port, 1 << b)));
            f += 3;
        }
    }
    for b in [0x01u8, 0x10, 0x40] {
        ev.push((f, Event::Press(Port::In2, b)));
        ev.push((f + 2, Event::Release(Port::In2, b)));
        f += 3;
    }
    ev.push((f, Event::Dip(Port::In1, 0x03, 0x02)));
    if amidar {
        ev.push((f + 3, Event::Dip(Port::In2, 0x04, 0x04)));
        ev.push((f + 6, Event::Dip(Port::In2, 0x02, 0x02)));
    } else {
        ev.push((f + 3, Event::Dip(Port::In2, 0x06, 0x04)));
        ev.push((f + 6, Event::Dip(Port::In2, 0x08, 0x08)));
    }
    assert!(f + 6 < 150, "the inputs run into the watchdog test");
    ev
}

fn apply(ports: &mut Ports, e: Event) {
    let p = match e {
        Event::Press(p, _) | Event::Release(p, _) | Event::Dip(p, _, _) => p,
    };
    let r = match p {
        Port::In0 => &mut ports.in0,
        Port::In1 => &mut ports.in1,
        Port::In2 => &mut ports.in2,
    };
    match e {
        Event::Press(_, b) => *r &= !b,
        Event::Release(_, b) => *r |= b,
        Event::Dip(_, m, v) => *r = (*r & !m) | (v & m),
    }
}

/// D0 and D1 swapped (Frogger's boards wire one graphics ROM and one sound ROM that way); its own
/// inverse, written here again apart from the core's.
fn swap01(d: &[u8]) -> Vec<u8> {
    d.iter().map(|&b| (b & 0xFC) | (b >> 1 & 1) | (b << 1 & 2)).collect()
}

/// The ROM files of `case`'s MAME set, under its names: (name, region, offset, bytes).
pub fn files(case: &Case) -> Vec<(String, &'static str, usize, Vec<u8>)> {
    let mut main = program_file("konami", &format!("{}.bin", case.main));
    main.resize(0x8000, 0);
    let mut sound = program_file("konami", &format!("{}.bin", case.sound));
    sound.resize(0x2000, 0);
    let gfx = program_file("galaxian", "gfx.bin");
    let (hi, lo) = (gfx[..0x800].to_vec(), gfx[0x1000..0x1800].to_vec());
    let palette = program_file("galaxian", "palette.bin");
    let mut f = Vec::new();
    let split = |region: &'static str, names: &[&str], size: usize, data: &[u8], f: &mut Vec<_>| {
        for (i, n) in names.iter().enumerate() {
            f.push((n.to_string(), region, i * size, data[i * size..(i + 1) * size].to_vec()));
        }
    };
    match case.set {
        "scramble" => {
            split("maincpu", &["s1.2d", "s2.2e", "s3.2f", "s4.2h", "s5.2j", "s6.2l", "s7.2m", "s8.2p"], 0x800, &main, &mut f);
            split("audiocpu", &["ot1.5c", "ot2.5d", "ot3.5e"], 0x800, &sound, &mut f);
            f.push(("c2.5f".into(), "gfx1", 0, hi));
            f.push(("c1.5h".into(), "gfx1", 0x800, lo));
            f.push(("c01s.6e".into(), "proms", 0, palette));
        }
        "theend" => {
            split(
                "maincpu",
                &["ic13_1t.bin", "ic14_2t.bin", "ic15_3t.bin", "ic16_4t.bin", "ic17_5t.bin", "ic18_6t.bin"],
                0x800,
                &main,
                &mut f,
            );
            split("audiocpu", &["ic56_1.bin", "ic55_2.bin"], 0x800, &sound, &mut f);
            f.push(("ic30_2c.bin".into(), "gfx1", 0, hi));
            f.push(("ic31_1c.bin".into(), "gfx1", 0x800, lo));
            f.push(("6331-1j.86".into(), "proms", 0, palette));
        }
        "scobra" => {
            split("maincpu", &["epr1265.2c", "2e", "epr1267.2f", "2h", "epr1269.2j", "2l"], 0x1000, &main, &mut f);
            split("audiocpu", &["5c", "5d", "5e"], 0x800, &sound, &mut f);
            f.push(("epr1274.5h".into(), "gfx1", 0, hi));
            f.push(("epr1273.5f".into(), "gfx1", 0x800, lo));
            f.push(("82s123.6e".into(), "proms", 0, palette));
        }
        "frogger" => {
            split("maincpu", &["frogger.26", "frogger.27", "frsm3.7"], 0x1000, &main, &mut f);
            // The board swaps D0 and D1 of the first sound ROM and the second graphics ROM.
            let mut s = swap01(&sound[..0x800]);
            s.extend_from_slice(&sound[0x800..]);
            split("audiocpu", &["frogger.608", "frogger.609", "frogger.610"], 0x800, &s, &mut f);
            f.push(("frogger.607".into(), "gfx1", 0, hi));
            f.push(("frogger.606".into(), "gfx1", 0x800, swap01(&lo)));
            f.push(("pr-91.6l".into(), "proms", 0, palette));
        }
        "amidar" => {
            split("maincpu", &["1.2c", "2.2e", "3.2f", "4.2h", "5.2j"], 0x1000, &main, &mut f);
            split("audiocpu", &["s1.5c", "s2.5d"], 0x1000, &sound, &mut f);
            f.push(("c2.5f".into(), "gfx1", 0, hi));
            f.push(("c2.5d".into(), "gfx1", 0x800, lo));
            f.push(("amidar.clr".into(), "proms", 0, palette));
        }
        other => panic!("no layout for {other}"),
    }
    f
}

/// The same files in the board's regions.
pub fn roms(case: &Case) -> Roms {
    let mut program = vec![0u8; 0x8000];
    let mut sound = vec![0u8; 0x2000];
    let mut gfx = vec![0u8; 0x1000];
    let mut palette = vec![0u8; 0x20];
    for (_, region, at, data) in files(case) {
        let dst = match region {
            "maincpu" => &mut program,
            "audiocpu" => &mut sound,
            "gfx1" => &mut gfx,
            _ => &mut palette,
        };
        dst[at..at + data.len()].copy_from_slice(&data);
    }
    Roms { program, gfx, palette, sound, protection_bits: case.protection_bits }
}

/// One frame's picture and sound (48 kHz), and the sound board's state at its end.
pub struct FrameOut {
    pub video: Vec<u32>,
    pub audio: Vec<i16>,
    /// The two AYs' registers 0-13, the filter switches and whether the board is silenced: what
    /// the sound segments are told apart by.
    pub sound_state: Vec<u16>,
}

/// One line of the sound Z80's trace: the cycle count since its reset, then AF, BC, DE, HL, SP, IX
/// and PC before the instruction.
pub fn trace_line(cpu: &Z80, cycles: u64) -> String {
    format!(
        "{} {:04X} {:04X} {:04X} {:04X} {:04X} {:04X} {:04X}",
        cycles,
        cpu.af(),
        cpu.bc(),
        cpu.de(),
        cpu.hl(),
        cpu.sp,
        cpu.ix,
        cpu.pc
    )
}

/// Runs `case` on cArcade; with `trace`, the sound Z80's every instruction goes to it.
pub fn run_ours(case: &Case, mut trace: Option<&mut dyn FnMut(String)>) -> Vec<FrameOut> {
    let mut b = Galaxian::new(case.variant, Rotation::Rot90, &roms(case)).expect("the test set loads");
    let mut ports = case.ports;
    let mut out = Vec::new();
    for frame in 1..=case.frames {
        for &(f, e) in &case.events {
            if f == frame {
                apply(&mut ports, e);
            }
        }
        b.set_ports(ports);
        match trace.as_mut() {
            Some(t) => {
                let mut tr = |cpu: &Z80, c: u64| t(trace_line(cpu, c));
                b.run_frame_traced(&mut tr);
            }
            None => b.run_frame(),
        }
        let snd = b.konami_sound().unwrap();
        let mut state: Vec<u16> = Vec::new();
        for n in 0..2 {
            state.extend(snd.ay(n).regs[..14].iter().map(|&r| u16::from(r)));
        }
        state.push(snd.filter_switches());
        state.push(u16::from(snd.muted()));
        out.push(FrameOut { video: b.video().to_vec(), audio: b.audio().to_vec(), sound_state: state });
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
    String::from_utf8_lossy(&out.stdout).split_whitespace().next().unwrap_or("?").to_string()
}

/// MAME's run of `case`: each frame's picture, its sound at 48 kHz, the ports it started with,
/// and the sound CPU's trace (written to a file; its path).
pub struct MameRun {
    pub video: Vec<Vec<u32>>,
    pub audio: Vec<i16>,
    pub ports: [u8; 4],
    pub trace: PathBuf,
}

pub fn run_mame(mame: &Path, case: &Case) -> MameRun {
    let work = crate_dir().join("target/mame-runs/konami").join(case.name);
    let _ = fs::remove_dir_all(&work);
    let rom_dir = work.join("roms").join(case.set);
    fs::create_dir_all(&rom_dir).unwrap();
    for (name, _, _, data) in files(case) {
        fs::write(rom_dir.join(name), data).unwrap();
    }
    // MAME runs the script again after a soft reset (the watchdog's): everything in it happens
    // once, behind a global.
    let mut lua = String::from(
        "if _G.started then return end\n_G.started = true\n\
         local scr = manager.machine.screens[':screen']\nlocal io = manager.machine.ioport.ports\n\
         local dbg = manager.machine.debugger\nlocal ev = {}\n",
    );
    for &(f, e) in &case.events {
        let k = f as i64 - 3;
        let (port, mask, act) = match e {
            Event::Press(p, b) => (p, b, "f:set_value(1)".to_string()),
            Event::Release(p, b) => (p, b, "f:set_value(0)".to_string()),
            Event::Dip(p, m, v) => (p, m, format!("f.user_value = {v}")),
        };
        lua += &format!(
            "ev[{k}] = ev[{k}] or {{}}\ntable.insert(ev[{k}], function() local f = io['{}']:field({mask}) {act} end)\n",
            port.tag()
        );
    }
    lua += &format!(
        "dbg:command('focus audiocpu')\n\
         dbg.visible_cpu = manager.machine.devices[':audiocpu']\n\
         dbg:command('trace sound.tr,audiocpu,noloop,{{tracelog \"C=%d AF=%04X BC=%04X DE=%04X HL=%04X SP=%04X IX=%04X \",totalcycles,af,bc,de,hl,sp,ix}}')\n\
         local pf = assert(_G.io.open('ports.txt', 'w'))\n\
         pf:write(string.format('%d %d %d %d\\n', io[':IN0']:read(), io[':IN1']:read(), io[':IN2']:read(), io[':IN3']:read()))\n\
         pf:close()\n\
         _G.sub = emu.add_machine_frame_notifier(function()\n\
           local k = scr:frame_number()\n\
           if k < {n} then scr:snapshot(string.format('f%05d.png', k)) end\n\
           if ev[k] then for _, a in ipairs(ev[k]) do a() end end\n\
           if k == {n} + 1 then dbg:command('trace off,audiocpu') end\n\
           if k >= {n} + 4 then manager.machine:exit() end\n\
         end)\n",
        n = case.frames
    );
    fs::write(work.join("run.lua"), lua).unwrap();
    let out = Command::new(mame)
        .current_dir(&work)
        .env("SDL_VIDEODRIVER", "dummy")
        .env("SDL_AUDIODRIVER", "dummy")
        .args([case.set, "-rompath", "roms", "-video", "none", "-sound", "none", "-nothrottle"])
        .args(["-skip_gameinfo", "-autoboot_script", "run.lua", "-snapshot_directory", "snap"])
        .args(["-cfg_directory", "cfg", "-nvram_directory", "nvram", "-debug", "-debugger", "none"])
        .args(["-samplerate", "48000", "-wavwrite", "sound.wav", "-seconds_to_run"])
        .arg((case.frames / 60 + 6).to_string())
        .output()
        .expect("MAME runs");
    let log = String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr);
    fs::write(work.join("mame.log"), &log).unwrap();
    assert!(!log.contains("LUA ERROR"), "MAME's Lua script failed:\n{log}");
    let wav = fs::read(work.join("sound.wav")).unwrap_or_else(|_| panic!("MAME wrote no sound:\n{log}"));
    let data = wav.windows(4).position(|w| w == b"data").expect("a WAV data chunk") + 8;
    let audio: Vec<i16> = wav[data..].chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]])).collect();
    let ports_text = fs::read_to_string(work.join("ports.txt")).unwrap();
    let p: Vec<u8> = ports_text.split_whitespace().map(|v| v.parse::<u32>().unwrap() as u8).collect();
    let video = (0..case.frames as usize)
        .map(|k| read_png(&work.join("snap").join(format!("f{k:05}.png"))))
        .collect();
    MameRun { video, audio, ports: [p[0], p[1], p[2], p[3]], trace: work.join("sound.tr") }
}

pub fn read_png(p: &Path) -> Vec<u32> {
    let file = fs::File::open(p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
    let mut reader = png::Decoder::new(file).read_info().unwrap();
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).unwrap();
    assert_eq!((info.width, info.height), (galaxian::WIDTH as u32, galaxian::HEIGHT as u32), "{}", p.display());
    let bpp = info.color_type.samples();
    buf[..info.buffer_size()]
        .chunks_exact(bpp)
        .map(|c| u32::from(c[0]) << 16 | u32::from(c[1]) << 8 | u32::from(c[2]))
        .collect()
}

pub fn write_png(p: &Path, v: &[u32]) {
    let file = fs::File::create(p).unwrap();
    let mut enc = png::Encoder::new(file, galaxian::WIDTH as u32, galaxian::HEIGHT as u32);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    let mut w = enc.write_header().unwrap();
    let data: Vec<u8> = v.iter().flat_map(|&p| [(p >> 16) as u8, (p >> 8) as u8, p as u8]).collect();
    w.write_image_data(&data).unwrap();
}

/// MAME's trace lines, reduced to ours: "C=n AF=.. ... IX=.. PPPP: ..." becomes
/// "n AF BC DE HL SP IX PC".
pub fn read_mame_trace(p: &Path) -> Vec<String> {
    let text = fs::read_to_string(p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
    text.lines()
        .filter(|l| l.starts_with("C="))
        .map(|l| {
            let w: Vec<&str> = l.split_whitespace().collect();
            let v = |i: usize| w[i].split('=').nth(1).unwrap().to_string();
            let pc = w[7].trim_end_matches(':');
            format!("{} {} {} {} {} {} {} {}", v(0), v(1), v(2), v(3), v(4), v(5), v(6), pc)
        })
        .collect()
}

/// Hashes of each 1024 trace lines.
pub fn trace_hashes(lines: &[String]) -> Vec<u64> {
    lines.chunks(1024).map(|c| fnv64(c.iter().flat_map(|l| l.bytes().chain(std::iter::once(b'\n'))))).collect()
}

/// The sound's segments: runs of frames with the sound board in one state, skipping their first
/// two frames and their last, those at least 4 frames long: (first frame, last frame + 1), 0-based.
pub fn sound_segments(frames: &[FrameOut]) -> Vec<(usize, usize)> {
    let mut segs = Vec::new();
    let mut start = 0;
    for i in 1..=frames.len() {
        if i == frames.len() || frames[i].sound_state != frames[start].sound_state {
            if i - start >= 7 {
                segs.push((start + 2, i - 1));
            }
            start = i;
        }
    }
    segs
}

pub fn reference_path(case: &Case) -> PathBuf {
    crate_dir().join("tests/references/konami").join(format!("{}.txt", case.name))
}

// ---- The sound, compared --------------------------------------------------------------------

use crate::measure::{self, Measure};

/// The segments compared: those at least 8 frames long (after the margins) while the board isn't
/// silenced. MAME 0.289's recording doesn't silence the sound when the main board sets the mute
/// bit (its system mute only reaches the speakers), so those frames aren't compared.
pub fn compared_segments(ours: &[FrameOut]) -> Vec<(usize, usize)> {
    sound_segments(ours)
        .into_iter()
        .filter(|&(a, b)| b - a >= 8 && ours[a..b].iter().all(|f| f.sound_state.last() == Some(&0)))
        .collect()
}

pub fn our_measure(ours: &[FrameOut], (a, b): (usize, usize)) -> Measure {
    let s: Vec<i16> = ours[a..b].iter().flat_map(|f| f.audio.iter().copied()).collect();
    measure::measure(&s, 48_000.0)
}

/// The tolerances. The AYs are exact, so a tone's strongest frequency must agree closely; its
/// level, which goes through the filters and the op-amp cArcade approximates, within 8% (they
/// come out within 7%), and its spectrum's shape in twelfth-octave bands at least 0.99 alike.
/// Quiet is under 1.5% of full scale in both (a segment with no tone still carries the coupling
/// capacitor settling after the last one, about 1%).
pub const LEVEL: f64 = 0.08;
pub const FREQ: f64 = 0.002;
pub const LIKENESS: f64 = 0.99;
pub const QUIET: f64 = 0.015;

pub fn compare_sound(name: &str, (a, b): (usize, usize), ours: &Measure, mame: &Measure) -> Result<(), String> {
    let at = format!("{name}: frames {}-{}", a + 1, b);
    if mame.rms < QUIET {
        return if ours.rms < QUIET {
            Ok(())
        } else {
            Err(format!("{at}: should be quiet: ours {:.4}, MAME {:.4}", ours.rms, mame.rms))
        };
    }
    let rel = |x: f64, y: f64| (x - y).abs() / y.abs().max(1e-9);
    if rel(ours.rms, mame.rms) > LEVEL {
        return Err(format!("{at}: level {:.4}, MAME {:.4} ({:.1}% apart)", ours.rms, mame.rms, 100.0 * rel(ours.rms, mame.rms)));
    }
    if rel(ours.freq, mame.freq) > FREQ {
        return Err(format!("{at}: strongest at {:.1} Hz, MAME {:.1} Hz", ours.freq, mame.freq));
    }
    let l = measure::likeness(&ours.fine, &mame.fine);
    if l < LIKENESS {
        return Err(format!("{at}: the spectrum is {l:.3} like MAME's"));
    }
    Ok(())
}

// ---- Reference files ------------------------------------------------------------------------

pub struct Reference {
    pub ports: [u8; 4],
    pub video: Vec<u64>,
    /// MAME's first trace line (its trace starts once the Lua script is running), how many lines
    /// it had, and a hash of each 1024 of them.
    pub trace_first: String,
    pub trace_lines: usize,
    pub trace: Vec<u64>,
    /// Each compared sound segment with MAME's measures of it.
    pub sound: Vec<((usize, usize), Measure)>,
}

pub fn write_reference(case: &Case, r: &Reference, version: &str) {
    let mut s = format!(
        "# {}: MAME {version} running set {} with cArcade's test programs {} (main) and {} (sound), {} frames.\n\
         # ports: the four input ports as MAME started them. video: each frame's picture, FNV-1a 64 of\n\
         # 224 x 768 0x00RRGGBB little-endian u32s. trace: MAME's sound CPU trace, its first line and\n\
         # length, then a hash of each 1024 lines (\"cycles AF BC DE HL SP IX PC\"). sound: a segment's\n\
         # frames, MAME's RMS (of full scale) and strongest frequency, and its twelfth-octave bands.\n",
        case.name, case.set, case.main, case.sound, case.frames
    );
    s += &format!("ports {:02X} {:02X} {:02X} {:02X}\n", r.ports[0], r.ports[1], r.ports[2], r.ports[3]);
    for (i, h) in r.video.iter().enumerate() {
        s += &format!("video {} {h:016x}\n", i + 1);
    }
    s += &format!("trace {} {}\n", r.trace_lines, r.trace_first);
    for h in &r.trace {
        s += &format!("tracehash {h:016x}\n");
    }
    for ((a, b), m) in &r.sound {
        let fine: Vec<String> = m.fine.iter().map(|e| format!("{e:.4e}")).collect();
        s += &format!("sound {a} {b} {:.6} {:.3} {}\n", m.rms, m.freq, fine.join(" "));
    }
    let p = reference_path(case);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, s).unwrap();
}

pub fn read_reference(case: &Case) -> Reference {
    let p = reference_path(case);
    let text = fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
    let mut r = Reference { ports: [0; 4], video: Vec::new(), trace_first: String::new(), trace_lines: 0, trace: Vec::new(), sound: Vec::new() };
    for l in text.lines().filter(|l| !l.starts_with('#')) {
        let (key, rest) = l.split_once(' ').unwrap();
        let w: Vec<&str> = rest.split_whitespace().collect();
        match key {
            "ports" => {
                for (i, v) in w.iter().enumerate() {
                    r.ports[i] = u8::from_str_radix(v, 16).unwrap();
                }
            }
            "video" => r.video.push(u64::from_str_radix(w[1], 16).unwrap()),
            "trace" => {
                r.trace_lines = w[0].parse().unwrap();
                r.trace_first = rest.split_once(' ').unwrap().1.to_string();
            }
            "tracehash" => r.trace.push(u64::from_str_radix(w[0], 16).unwrap()),
            "sound" => {
                let m = Measure {
                    rms: w[2].parse().unwrap(),
                    freq: w[3].parse().unwrap(),
                    fine: w[4..].iter().map(|v| v.parse().unwrap()).collect(),
                    octaves: Vec::new(),
                };
                r.sound.push(((w[0].parse().unwrap(), w[1].parse().unwrap()), m));
            }
            other => panic!("{}: unknown line {other}", p.display()),
        }
    }
    r
}
