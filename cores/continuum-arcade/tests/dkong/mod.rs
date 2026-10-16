//! What the Donkey Kong-board tests share: cArcade's own test programs (test-programs/dkong) laid
//! out as MAME's dkong and dkongjr sets, the inputs each run presses, running a program on
//! cArcade's board and on MAME, and the reference files MAME's runs left in tests/references/dkong.
//!
//! Each run gives every frame's picture, which must match MAME's exactly, and, on the sound
//! programs, the 8035's every instruction (its registers, ports, timer and cycle count, which must
//! match MAME's trace line for line) and the sound, which goes through the analogue circuits
//! cArcade approximates and so is compared by level and make-up, segment by segment.
//!
//! A frame is VBLANK to VBLANK, as `Dkong::run_frame` runs it: our frame n is MAME's snapshot at
//! frame notifier n - 1.

#![allow(dead_code)]

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use continuum_arcade::dkong::{self, Dkong, Ports, Roms, Variant};
use continuum_arcade::mcs48::Mcs48;

use crate::measure::Measure;

pub mod sound;

pub fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

pub fn program_file(name: &str) -> Vec<u8> {
    let p = crate_dir().join("test-programs/dkong").join(name);
    fs::read(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Port {
    In0,
    In1,
    In2,
    Dsw0,
}

impl Port {
    fn tag(self) -> &'static str {
        match self {
            Port::In0 => ":IN0",
            Port::In1 => ":IN1",
            Port::In2 => ":IN2",
            Port::Dsw0 => ":DSW0",
        }
    }
}

/// Something the test does to the inputs at the start of a frame (all active high).
#[derive(Clone, Copy, Debug)]
pub enum Event {
    Press(Port, u8),
    Release(Port, u8),
    /// A DIP switch: `mask`'s bits set to `value`.
    Dip(Port, u8, u8),
}

pub struct Case {
    pub name: &'static str,
    /// test-programs/dkong/<main>.bin, the Z80's program.
    pub main: &'static str,
    /// test-programs/dkong/<sound>.bin, the 8035's (None: a blank ROM, all NOPs).
    pub sound: Option<&'static str>,
    pub set: &'static str,
    pub variant: Variant,
    pub frames: u32,
    pub events: Vec<(u32, Event)>,
    /// MAME's ports at rest (factory switches).
    pub ports: Ports,
}

fn dk_rest() -> Ports {
    Ports { in0: 0, in1: 0, in2: 0, dsw0: 0x80 }
}

fn jr_rest() -> Ports {
    Ports { in0: 0, in1: 0, in2: 0x40, dsw0: 0x80 }
}

pub fn cases() -> Vec<Case> {
    let dk = |name: &'static str, frames: u32| Case {
        name,
        main: name,
        sound: None,
        set: "dkong",
        variant: Variant::DonkeyKong,
        frames,
        events: vec![],
        ports: dk_rest(),
    };
    vec![
        dk("tiles", 200),
        dk("sprites", 260),
        dk("raster", 200),
        Case { events: input_script(), ..dk("inputs", 120) },
        dk("nmi", 200),
        dk("watchdog", 400),
        Case { set: "dkongjr", variant: Variant::DonkeyKongJr, ports: jr_rest(), ..dk("dkjr", 220) },
        Case { sound: Some("dksnd"), ..dk("dksound", 700) },
        Case { sound: Some("jrsnd"), set: "dkongjr", variant: Variant::DonkeyKongJr, ports: jr_rest(), ..dk("jrsound", 760) },
    ]
}

/// Every control pressed in turn (IN0, IN1, then IN2's service, starts and coin), then each DIP
/// switch through its settings.
fn input_script() -> Vec<(u32, Event)> {
    let mut ev = Vec::new();
    let mut f = 10;
    for (port, bits) in [
        (Port::In0, [0x01u8, 0x02, 0x04, 0x08, 0x10].as_slice()),
        (Port::In1, [0x01u8, 0x02, 0x04, 0x08, 0x10].as_slice()),
        (Port::In2, [0x01u8, 0x04, 0x08, 0x80].as_slice()),
    ] {
        for &b in bits {
            ev.push((f, Event::Press(port, b)));
            ev.push((f + 2, Event::Release(port, b)));
            f += 3;
        }
    }
    for (mask, values) in [
        (0x03u8, [0x01u8, 0x02, 0x03, 0x00].as_slice()),
        (0x0C, [0x04, 0x08, 0x0C, 0x00].as_slice()),
        (0x70, [0x10, 0x20, 0x40, 0x70, 0x00].as_slice()),
        (0x80, [0x00, 0x80].as_slice()),
    ] {
        for &v in values {
            ev.push((f, Event::Dip(Port::Dsw0, mask, v)));
            f += 2;
        }
    }
    assert!(f < 120, "the inputs script runs past its 120 frames");
    ev
}

// ---- The sets' files ------------------------------------------------------------------------

/// The tune ROM: a pattern of our own (the same as tests/mcs48's).
pub fn tune_rom() -> Vec<u8> {
    (0..2048usize).map(|i| ((i * 37 + (i >> 8) * 101 + 11) & 0xFF) as u8).collect()
}

fn sound_program(case: &Case, len: usize) -> Vec<u8> {
    let mut p = case.sound.map(|n| program_file(&format!("{n}.bin"))).unwrap_or_default();
    assert!(p.len() <= len, "{}: the sound program is too long", case.name);
    p.resize(len, 0);
    p
}

/// The files of `case`'s MAME set, under its names.
pub fn files(case: &Case) -> Vec<(String, Vec<u8>)> {
    let main = program_file(&format!("{}.bin", case.main));
    let tiles = program_file("tiles-gfx.bin");
    let sprites = program_file("sprites-gfx.bin");
    let proms = program_file("proms.bin");
    let mut f: Vec<(String, Vec<u8>)> = Vec::new();
    let mut add = |n: &str, d: &[u8]| f.push((n.to_string(), d.to_vec()));
    match case.set {
        "dkong" => {
            for (i, n) in ["c_5et_g.bin", "c_5ct_g.bin", "c_5bt_g.bin", "c_5at_g.bin"].iter().enumerate() {
                add(n, &main[i * 0x1000..(i + 1) * 0x1000]);
            }
            add("s_3i_b.bin", &sound_program(case, 0x800));
            add("s_3j_b.bin", &tune_rom());
            add("v_5h_b.bin", &tiles[..0x800]);
            add("v_3pt.bin", &tiles[0x1000..0x1800]);
            for (i, n) in ["l_4m_b.bin", "l_4n_b.bin", "l_4r_b.bin", "l_4s_b.bin"].iter().enumerate() {
                add(n, &sprites[i * 0x800..(i + 1) * 0x800]);
            }
            add("c-2k.bpr", &proms[..0x100]);
            add("c-2j.bpr", &proms[0x100..0x200]);
            add("v-5e.bpr", &proms[0x200..]);
        }
        "dkongjr" => {
            // Each ROM's halves (or quarters) load at scattered places, as MAME's ROM_CONTINUEs
            // have them.
            let pieces = |at: &[usize], len: usize| at.iter().flat_map(|&a| main[a..a + len].to_vec()).collect::<Vec<u8>>();
            add("djr1-c_5b_f-2.5b", &pieces(&[0x0000, 0x3000], 0x1000));
            add("djr1-c_5c_f-2.5c", &pieces(&[0x2000, 0x4800, 0x1000, 0x5800], 0x800));
            add("djr1-c_5e_f-2.5e", &pieces(&[0x4000, 0x2800, 0x5000, 0x1800], 0x800));
            add("djr1-c_3h.3h", &sound_program(case, 0x1000));
            add("djr1-v.3n", &tiles[..0x1000]);
            add("djr1-v.3p", &tiles[0x1000..]);
            for (i, n) in ["djr1-v_7c.7c", "djr1-v_7d.7d", "djr1-v_7e.7e", "djr1-v_7f.7f"].iter().enumerate() {
                add(n, &sprites[i * 0x800..(i + 1) * 0x800]);
            }
            add("djr1-c-2e.2e", &proms[..0x100]);
            add("djr1-c-2f.2f", &proms[0x100..0x200]);
            add("djr1-v-2n.2n", &proms[0x200..]);
        }
        other => panic!("no layout for {other}"),
    }
    f
}

/// The same chips in the board's regions, as MAME's sets load them.
pub fn roms(case: &Case) -> Roms {
    let main = program_file(&format!("{}.bin", case.main));
    let tiles = program_file("tiles-gfx.bin");
    let mut program = main.clone();
    program.resize(0x10000, 0);
    let (sound, tiles) = if case.variant == Variant::DonkeyKongJr {
        (sound_program(case, 0x1000), tiles)
    } else {
        let p = sound_program(case, 0x800);
        let mut s = p.clone();
        s.extend_from_slice(&p);
        s.extend_from_slice(&tune_rom());
        let mut t = tiles[..0x800].to_vec();
        t.extend_from_slice(&tiles[0x1000..0x1800]);
        (s, t)
    };
    Roms { program, sound, tiles, sprites: program_file("sprites-gfx.bin"), proms: program_file("proms.bin") }
}

fn apply(ports: &mut Ports, e: Event) {
    let p = match e {
        Event::Press(p, _) | Event::Release(p, _) | Event::Dip(p, _, _) => p,
    };
    let r = match p {
        Port::In0 => &mut ports.in0,
        Port::In1 => &mut ports.in1,
        Port::In2 => &mut ports.in2,
        Port::Dsw0 => &mut ports.dsw0,
    };
    match e {
        Event::Press(_, b) => *r |= b,
        Event::Release(_, b) => *r &= !b,
        Event::Dip(_, m, v) => *r = (*r & !m) | (v & m),
    }
}

// ---- Running it -----------------------------------------------------------------------------

/// One frame of a run.
pub struct FrameOut {
    pub video: Vec<u32>,
    pub audio: Vec<i16>,
    /// The sound test's script segment (SEG in test-programs/dkong/sound.inc, 0 elsewhere) and
    /// latch 6H at the frame's end: what the sound's stretches are told apart by.
    pub sound_state: (u8, u8),
}

/// One line of the 8035's trace, as tests/mcs48 writes them: bank and PC, A, PSW, R0-R7, the
/// timer and prescaler, P1, P2 and the cycles before the instruction.
pub fn trace_line(m: &Mcs48, cycles: u64) -> String {
    let mut s = format!("{}:{:03X} A={:02X} F={:02X} R=", m.pc >> 11, m.pc & 0x7FF, m.a, m.psw);
    for n in 0..8 {
        write!(s, "{:02X}", m.reg(n)).unwrap();
    }
    write!(s, " T={:02X} P={:02X} P1={:02X} P2={:02X} C={cycles}", m.timer, m.prescaler, m.p1, m.p2).unwrap();
    s
}

pub fn run_ours(case: &Case, mut trace: Option<&mut dyn FnMut(String)>) -> Vec<FrameOut> {
    let mut b = Dkong::new(case.variant, &roms(case)).expect("the test set loads");
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
            Some(t) => b.run_frame_traced(&mut |m: &Mcs48, c: u64| t(trace_line(m, c))),
            None => b.run_frame(),
        }
        let seg = if case.sound.is_some() { b.peek(SEG) } else { 0 };
        out.push(FrameOut { video: b.video().to_vec(), audio: b.audio().to_vec(), sound_state: (seg, b.sound_state().0) });
    }
    out
}

/// Where the sound programs keep their script's segment (USER in hw.inc).
pub const SEG: u16 = 0x6010;

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

pub struct MameRun {
    pub video: Vec<Vec<u32>>,
    pub audio: Vec<i16>,
    /// The 8035's trace, if the case has a sound program, reduced to our lines.
    pub trace: Vec<String>,
}

pub fn run_mame(mame: &Path, case: &Case) -> MameRun {
    let work = crate_dir().join("target/mame-runs/dkong").join(case.name);
    let _ = fs::remove_dir_all(&work);
    let rom_dir = work.join("roms").join(case.set);
    fs::create_dir_all(&rom_dir).unwrap();
    for (name, data) in files(case) {
        fs::write(rom_dir.join(name), data).unwrap();
    }
    let traced = case.sound.is_some();
    // MAME runs the script again after a soft reset (the watchdog's): everything in it happens
    // once, behind a global.
    let mut lua = String::from(
        "if _G.started then return end\n_G.started = true\n\
         local scr = manager.machine.screens[':screen']\nlocal io = manager.machine.ioport.ports\n\
         local dbg = manager.machine.debugger\nlocal ev = {}\n",
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
            port.tag()
        );
    }
    if traced {
        lua += "dbg:command('focus soundcpu')\n\
                dbg.visible_cpu = manager.machine.devices[':soundcpu']\n\
                dbg:command('trace sound.tr,soundcpu,noloop,{tracelog \"A=%02X F=%02X R=%02X%02X%02X%02X%02X%02X%02X%02X T=%02X P=%02X P1=%02X P2=%02X C=%d \",a,curflags,r0,r1,r2,r3,r4,r5,r6,r7,tc,tpre,p1,p2,totalcycles}')\n";
    }
    lua += &format!(
        "_G.sub = emu.add_machine_frame_notifier(function()\n\
           local k = scr:frame_number()\n\
           if k < {n} then scr:snapshot(string.format('f%05d.png', k)) end\n\
           if ev[k] then for _, a in ipairs(ev[k]) do a() end end\n\
           if k == {n} + 1 and {t} then dbg:command('trace off,soundcpu') end\n\
           if k >= {n} + 4 then manager.machine:exit() end\n\
         end)\n",
        n = case.frames,
        t = traced
    );
    fs::write(work.join("run.lua"), lua).unwrap();
    let mut cmd = Command::new(mame);
    cmd.current_dir(&work)
        .env("SDL_VIDEODRIVER", "dummy")
        .env("SDL_AUDIODRIVER", "dummy")
        .args([case.set, "-rompath", "roms", "-video", "none", "-sound", "none", "-nothrottle"])
        .args(["-skip_gameinfo", "-autoboot_script", "run.lua", "-snapshot_directory", "snap"])
        .args(["-cfg_directory", "cfg", "-nvram_directory", "nvram"])
        .args(["-samplerate", "48000", "-wavwrite", "sound.wav", "-seconds_to_run"])
        .arg((case.frames / 60 + 6).to_string());
    if traced {
        cmd.args(["-debug", "-debugger", "none"]);
    }
    let out = cmd.output().expect("MAME runs");
    let log = String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr);
    fs::write(work.join("mame.log"), &log).unwrap();
    assert!(!log.contains("LUA ERROR"), "MAME's Lua script failed:\n{log}");
    let wav = fs::read(work.join("sound.wav")).unwrap_or_else(|_| panic!("MAME wrote no sound:\n{log}"));
    let data = wav.windows(4).position(|w| w == b"data").expect("a WAV data chunk") + 8;
    let audio: Vec<i16> = wav[data..].chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]])).collect();
    let video = (0..case.frames as usize)
        .map(|k| read_png(&work.join("snap").join(format!("f{k:05}.png"))))
        .collect();
    let trace = if traced { read_mame_trace(&work.join("sound.tr")) } else { Vec::new() };
    MameRun { video, audio, trace }
}

/// MAME's trace reduced to our lines: "A=.. F=.. R=.. T=.. P=.. P1=.. P2=.. C=n b:ppp: text"
/// becomes "b:ppp A=.. F=.. R=.. T=.. P=.. P1=.. P2=.. C=n".
pub fn read_mame_trace(p: &Path) -> Vec<String> {
    let text = fs::read_to_string(p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
    text.lines()
        .filter(|l| l.starts_with("A="))
        .map(|l| {
            let w: Vec<&str> = l.split_whitespace().collect();
            format!("{} {}", w[8].trim_end_matches(':'), w[..8].join(" "))
        })
        .collect()
}

pub fn trace_hashes(lines: &[String]) -> Vec<u64> {
    lines.chunks(1024).map(|c| fnv64(c.iter().flat_map(|l| l.bytes().chain(std::iter::once(b'\n'))))).collect()
}

pub fn read_png(p: &Path) -> Vec<u32> {
    let file = fs::File::open(p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
    let mut reader = png::Decoder::new(file).read_info().unwrap();
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).unwrap();
    assert_eq!((info.width, info.height), (dkong::WIDTH as u32, dkong::HEIGHT as u32), "{}", p.display());
    let bpp = info.color_type.samples();
    buf[..info.buffer_size()]
        .chunks_exact(bpp)
        .map(|c| u32::from(c[0]) << 16 | u32::from(c[1]) << 8 | u32::from(c[2]))
        .collect()
}

pub fn write_png(p: &Path, v: &[u32]) {
    let file = fs::File::create(p).unwrap();
    let mut enc = png::Encoder::new(file, dkong::WIDTH as u32, dkong::HEIGHT as u32);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    let mut w = enc.write_header().unwrap();
    let data: Vec<u8> = v.iter().flat_map(|&p| [(p >> 16) as u8, (p >> 8) as u8, p as u8]).collect();
    w.write_image_data(&data).unwrap();
}

/// Where two runs' pictures first differ, in words; the pair is written next to MAME's run.
pub fn first_difference(case: &Case, ours: &[FrameOut], theirs: &[Vec<u32>]) -> Option<String> {
    for (i, (o, t)) in ours.iter().map(|f| &f.video).zip(theirs).enumerate() {
        if o != t {
            let n = o.iter().zip(t).filter(|(a, b)| a != b).count();
            let p = o.iter().zip(t).position(|(a, b)| a != b).unwrap();
            let dir = crate_dir().join("target/mame-runs/dkong").join(case.name);
            if dir.is_dir() {
                write_png(&dir.join(format!("ours-{:05}.png", i + 1)), o);
                write_png(&dir.join(format!("mame-{:05}.png", i + 1)), t);
            }
            return Some(format!(
                "{}: frame {} picture: {n} pixels differ, the first at ({}, {}): ours {:06x}, MAME {:06x}",
                case.name,
                i + 1,
                p % dkong::WIDTH,
                p / dkong::WIDTH,
                o[p],
                t[p]
            ));
        }
    }
    None
}

pub fn reference_path(case: &Case) -> PathBuf {
    crate_dir().join("tests/references/dkong").join(format!("{}.txt", case.name))
}

/// The cycle count a trace line ends with.
fn line_cycles(l: &str) -> u64 {
    l.rsplit("C=").next().and_then(|c| c.trim().parse().ok()).expect("a trace line ends with its cycles")
}

/// Our trace from MAME's first line on (MAME's starts once the Lua script runs): the part both
/// cover. None if MAME's first line isn't in ours.
pub fn aligned<'a>(ours: &'a [String], first: &str) -> Option<&'a [String]> {
    let c = line_cycles(first);
    let at = ours.iter().position(|l| line_cycles(l) == c)?;
    Some(&ours[at..])
}

/// Where our 8035 trace first differs from MAME's, over the instructions both ran.
pub fn trace_difference(ours: &[String], theirs: &[String]) -> Option<String> {
    let Some(first) = theirs.first() else { return Some("MAME's trace is empty".into()) };
    let Some(ours) = aligned(ours, first) else {
        return Some(format!("MAME's first 8035 line isn't in ours: {first}"));
    };
    let n = ours.len().min(theirs.len());
    if n + 64 < theirs.len().min(ours.len() + 64) {
        return Some(format!("the traces are {} and {} lines", ours.len(), theirs.len()));
    }
    let i = (0..n).find(|&i| ours[i] != theirs[i])?;
    Some(format!(
        "8035 instruction {i} differs:\n  ours {}\n  MAME {}\n  (before it, both: {})",
        ours[i],
        theirs[i],
        if i > 0 { &theirs[i - 1] } else { "-" }
    ))
}

/// What a reference file keeps of MAME's run: each frame's picture hash and, on the sound
/// programs, MAME's first 8035 trace line, how many lines a run of the case's length covers, a
/// hash of each 1024 of them, each frame's sound level and the measures of each of the script's
/// segments (tests/dkong/sound.rs).
#[derive(Debug, Default)]
pub struct Reference {
    pub video: Vec<u64>,
    pub trace_first: String,
    pub trace_lines: usize,
    pub trace: Vec<u64>,
    pub levels: Vec<sound::Level>,
    pub segments: Vec<(sound::Segment, Measure)>,
}

impl Reference {
    /// From MAME's run; the script's segments are where our run of the same program has them
    /// (the program shows SEG on the screen, so the pictures matching says they're MAME's too).
    pub fn from_run(run: &MameRun, ours: &[FrameOut], sound_program: bool) -> Reference {
        // MAME traces on a frame past ours: keep the lines up to the end of our last frame.
        let n = run.video.len() as u64;
        let end = n * 6600;
        let lines: Vec<String> = run.trace.iter().take_while(|l| line_cycles(l) < end).cloned().collect();
        let mut r = Reference {
            video: run.video.iter().map(|f| video_hash(f)).collect(),
            trace_first: lines.first().cloned().unwrap_or_default(),
            trace_lines: lines.len(),
            trace: trace_hashes(&lines),
            ..Reference::default()
        };
        if sound_program {
            r.levels = (1..=run.video.len()).map(|f| sound::level(sound::frame(&run.audio, f))).collect();
            r.segments = sound::segments(ours)
                .into_iter()
                .filter_map(|s| sound::measured_frames(&s).map(|(a, b)| (s, sound::measure_frames(&run.audio, a, b))))
                .collect();
        }
        r
    }

    pub fn render(&self, case: &Case, version: &str) -> String {
        let mut s = format!(
            "# {}: MAME {version} running set {} with cArcade's test programs {} (Z80) and {} (8035), {} frames.\n\
             # video: each frame's picture, FNV-1a 64 of 224 x 256 0x00RRGGBB little-endian u32s.\n\
             # trace: MAME's 8035 trace (as tests/dkong writes ours), its first line and how many lines\n\
             # fall in the run, then a hash of each 1024 lines.\n\
             # level: each frame's sound (792 samples at 48 kHz), its RMS less the mean and its mean, as\n\
             # fractions of full scale.\n\
             # segment: the script's segment, its first and last frame, then over its frames but the first\n\
             # two and the last: the RMS, the strongest frequency in Hz, the energy in twelfth-octave bands\n\
             # from 40 Hz and in octave bands from 40 Hz (tests/measure).\n",
            case.name,
            case.set,
            case.main,
            case.sound.unwrap_or("none"),
            case.frames
        );
        for (i, h) in self.video.iter().enumerate() {
            s += &format!("video {} {h:016x}\n", i + 1);
        }
        if self.trace_lines > 0 {
            s += &format!("trace {} {}\n", self.trace_lines, self.trace_first);
            for h in &self.trace {
                s += &format!("tracehash {h:016x}\n");
            }
        }
        for (i, l) in self.levels.iter().enumerate() {
            s += &format!("level {} {:.6} {:.6}\n", i + 1, l.rms, l.mean);
        }
        let list = |v: &[f64]| v.iter().map(|e| format!("{e:.4e}")).collect::<Vec<_>>().join(",");
        for (g, m) in &self.segments {
            s += &format!("segment {} {} {} {:.6} {:.3} {} {}\n", g.seg, g.first, g.last, m.rms, m.freq, list(&m.fine), list(&m.octaves));
        }
        s
    }

    pub fn write(&self, case: &Case, version: &str) {
        let p = reference_path(case);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, self.render(case, version)).unwrap();
    }

    pub fn read_text(case: &Case) -> String {
        let p = reference_path(case);
        fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
    }

    pub fn read(case: &Case) -> Reference {
        let p = reference_path(case);
        let text = Reference::read_text(case);
        let mut r = Reference::default();
        let list = |s: &str| s.split(',').map(|v| v.parse().unwrap()).collect::<Vec<f64>>();
        for l in text.lines().filter(|l| !l.starts_with('#')) {
            let (key, rest) = l.split_once(' ').unwrap();
            let w: Vec<&str> = rest.split_whitespace().collect();
            match key {
                "video" => r.video.push(u64::from_str_radix(w[1], 16).unwrap()),
                "trace" => {
                    r.trace_lines = w[0].parse().unwrap();
                    r.trace_first = rest.split_once(' ').unwrap().1.to_string();
                }
                "tracehash" => r.trace.push(u64::from_str_radix(w[0], 16).unwrap()),
                "level" => r.levels.push(sound::Level { rms: w[1].parse().unwrap(), mean: w[2].parse().unwrap() }),
                "segment" => r.segments.push((
                    sound::Segment { seg: w[0].parse().unwrap(), first: w[1].parse().unwrap(), last: w[2].parse().unwrap() },
                    Measure { rms: w[3].parse().unwrap(), freq: w[4].parse().unwrap(), fine: list(w[5]), octaves: list(w[6]) },
                )),
                other => panic!("{}: unknown line {other}", p.display()),
            }
        }
        r
    }
}
