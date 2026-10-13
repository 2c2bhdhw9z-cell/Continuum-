//! Real Game Boy Advance homebrew games, run headless through the real libretro entry points the
//! way the app runs them, for a few hundred frames each with the buttons being pressed.
//!
//! Every game here is published free by its author under an open licence (MIT, GPL, AGPL, MPL);
//! none is a commercial cartridge. They are downloaded by scripts/fetch-test-roms.sh into
//! test-roms/homebrew, which is git-ignored: no ROM is ever committed. A missing game is skipped,
//! so a machine with no network still runs the rest of the suite.
//!
//! What this gates: the core does not panic or hang, the picture stays 240x160 in the format the
//! frontend was promised, the game keeps drawing new pictures, sound keeps coming out, and a save
//! state taken half way through brings the game back to exactly the same picture.

use std::collections::BTreeSet;
use std::ffi::c_void;
use std::path::PathBuf;
use std::sync::Mutex;

use continuum_gb::libretro::*;

/// The core is one global: tests take turns.
static SERIAL: Mutex<()> = Mutex::new(());

/// libretro's joypad buttons, by the numbers a frontend uses.
const PAD_START: u32 = 3;
const PAD_UP: u32 = 4;
const PAD_RIGHT: u32 = 7;
const PAD_A: u32 = 8;

struct Out {
    /// (width, height, pitch) of every picture sent, so one odd frame cannot hide.
    sizes: BTreeSet<(u32, u32, usize)>,
    drawn: u32,
    skipped: u32,
    last: Vec<u32>,
    /// Every different picture the run drew, and the most colours any one of them held.
    pictures: BTreeSet<u64>,
    best: usize,
    samples: usize,
    /// Runs that handed over nothing but silence, and how many samples were not silent.
    quiet_runs: u32,
    loud: usize,
    /// The pixel format the core asked the frontend for.
    fmt: Option<u32>,
    /// Which frame of the game the buttons are for. The harness sets it, so the replay after a
    /// loaded state presses exactly what the first run pressed.
    frame: u32,
}

const EMPTY: Out = Out {
    sizes: BTreeSet::new(),
    drawn: 0,
    skipped: 0,
    last: Vec::new(),
    pictures: BTreeSet::new(),
    best: 0,
    samples: 0,
    quiet_runs: 0,
    loud: 0,
    fmt: None,
    frame: 0,
};
static OUT: Mutex<Out> = Mutex::new(EMPTY);

unsafe extern "C" fn env(cmd: u32, d: *mut c_void) -> bool {
    match cmd {
        // SET_PIXEL_FORMAT: remember what the core asked for, the way a frontend takes it.
        10 => {
            OUT.lock().unwrap().fmt = Some(*(d as *const u32));
            true
        }
        // SET_CORE_OPTIONS_V2.
        67 => true,
        _ => false,
    }
}

unsafe extern "C" fn video(d: *const c_void, w: u32, h: u32, pitch: usize) {
    let mut o = OUT.lock().unwrap();
    if d.is_null() {
        o.skipped += 1;
        return;
    }
    o.sizes.insert((w, h, pitch));
    o.drawn += 1;
    let px = std::slice::from_raw_parts(d as *const u32, (w * h) as usize).to_vec();
    o.pictures.insert(hash(&px));
    o.best = o.best.max(colours(&px));
    o.last = px;
}

unsafe extern "C" fn audio(d: *const i16, n: usize) -> usize {
    let mut o = OUT.lock().unwrap();
    let s = std::slice::from_raw_parts(d, n * 2);
    o.samples += n;
    let loud = s.iter().filter(|v| **v != 0).count();
    o.loud += loud;
    if loud == 0 {
        o.quiet_runs += 1;
    }
    n
}

unsafe extern "C" fn poll() {}

/// A, Start and the pad, pressed on a timetable so a game gets past its title screen and into
/// play instead of sitting on one picture. Purely a function of the frame number, so a run can be
/// repeated exactly.
unsafe extern "C" fn input(_port: u32, dev: u32, _index: u32, id: u32) -> i16 {
    if dev != 1 {
        return 0;
    }
    let f = OUT.lock().unwrap().frame;
    let held = match id {
        PAD_A => f % 40 < 3,
        PAD_START => f % 97 < 3,
        PAD_RIGHT => f % 23 < 8,
        PAD_UP => f % 31 < 5,
        _ => false,
    };
    i16::from(held)
}

fn rom(name: &str) -> Option<Vec<u8>> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test-roms/homebrew").join(name);
    let r = std::fs::read(&p).ok();
    if r.is_none() {
        eprintln!("SKIPPED: {} not found (run scripts/fetch-test-roms.sh)", p.display());
    }
    r
}

fn hash(px: &[u32]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for p in px {
        h = (h ^ u64::from(*p)).wrapping_mul(0x100_0000_01b3);
    }
    h
}

fn colours(px: &[u32]) -> usize {
    let mut c: Vec<u32> = px.to_vec();
    c.sort_unstable();
    c.dedup();
    c.len()
}

/// What one game's run came to.
struct Run {
    sizes: BTreeSet<(u32, u32, usize)>,
    drawn: u32,
    skipped: u32,
    pictures: usize,
    best: usize,
    samples: usize,
    quiet_runs: u32,
    loud: usize,
    fmt: Option<u32>,
    last: Vec<u32>,
    hash: u64,
    /// True when a state taken half way through, loaded back, carried on identically.
    state_matched: bool,
    state_len: usize,
}

/// Runs `data` for `frames` frames through the real entry points. Half way through it takes a
/// save state the way the app does, carries on to the end, then loads that state back and runs
/// the second half again: a game whose state is whole comes out on exactly the same picture.
fn drive(data: &[u8], frames: u32) -> Run {
    *OUT.lock().unwrap() = EMPTY;
    let half = frames / 2;
    let mut state = Vec::new();
    let (end_hash, replay_hash);
    unsafe {
        retro_set_environment(env);
        retro_set_video_refresh(video);
        retro_set_audio_sample_batch(audio);
        retro_set_input_poll(poll);
        retro_set_input_state(input);
        retro_init();
        let info = RetroGameInfo { path: std::ptr::null(), data: data.as_ptr() as *const c_void, size: data.len(), meta: std::ptr::null() };
        assert!(retro_load_game(&info), "the core refused the game");
        for f in 0..frames {
            OUT.lock().unwrap().frame = f;
            retro_run();
            if f + 1 == half {
                state = vec![0u8; retro_serialize_size()];
                assert!(retro_serialize(state.as_mut_ptr() as *mut c_void, state.len()), "retro_serialize said no");
            }
        }
        end_hash = hash(&OUT.lock().unwrap().last);
        assert!(retro_unserialize(state.as_ptr() as *const c_void, state.len()), "retro_unserialize said no");
        for f in half..frames {
            OUT.lock().unwrap().frame = f;
            retro_run();
        }
        replay_hash = hash(&OUT.lock().unwrap().last);
        retro_unload_game();
        retro_deinit();
    }
    let mut o = OUT.lock().unwrap();
    Run {
        sizes: std::mem::take(&mut o.sizes),
        drawn: o.drawn,
        skipped: o.skipped,
        pictures: o.pictures.len(),
        best: o.best,
        samples: o.samples,
        quiet_runs: o.quiet_runs,
        loud: o.loud,
        fmt: o.fmt,
        last: std::mem::take(&mut o.last),
        hash: end_hash,
        state_matched: end_hash == replay_hash,
        state_len: state.len(),
    }
}

/// The games, in the order they are run. Frames: enough for each one to get through its own
/// start-up and draw its game (Skyland spends its first 500 frames on the Game Boy Player
/// splash screen its author built in). Colours: the fewest the richest picture of the run may
/// hold, about half of what each game really draws today, so a game going blank fails but a
/// change of a few pixels does not. Music: whether the game plays any sound at all in that time
/// (Pong and Voxelburg are silent; their menus make no sound).
const GAMES: &[(&str, &str, u32, usize, bool)] = &[
    ("pong.gba", "Pong, by ZeroDayArcade (MIT)", 300, 4, false),
    ("wigly-hunt.gba", "Wigly Hunt, by rawbits2010 (GPL-3.0)", 300, 8, true),
    ("everything-counts.gba", "Everything Counts, by Godless-1 (AGPL-3.0)", 300, 32, true),
    ("voxelburg.gba", "Attack on Voxelburg, by Mutant Stargoat (GPL-3.0)", 300, 64, false),
    ("sym-merged.gba", "Symbol Merged, by copyrat90 (MIT)", 300, 6, true),
    ("skyland.gba", "Skyland, by Evan Bowman (MPL-2.0)", 900, 12, true),
];

/// One sound sample pair per 1/48000 s of a 59.73 Hz frame, give or take rounding.
const SAMPLES_PER_RUN: f64 = 48_000.0 / (16_777_216.0 / 280_896.0);

#[test]
fn homebrew_games_run() {
    let _turn = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let only = std::env::var("HOMEBREW_ONLY").unwrap_or_default();
    let mut ran = 0;
    for &(name, what, frames, least_colours, music) in GAMES {
        if !only.is_empty() && name != only {
            continue;
        }
        let Some(data) = rom(name) else { continue };
        ran += 1;
        let r = drive(&data, frames);
        eprintln!(
            "{what}: {} runs, {} pictures, best {} colours, {} samples ({} not silence), state {} bytes, last picture {:#018x}",
            r.drawn, r.pictures, r.best, r.samples, r.loud, r.state_len, r.hash
        );
        if let Ok(dir) = std::env::var("HOMEBREW_PNG") {
            std::fs::create_dir_all(&dir).unwrap();
            let f = std::fs::File::create(format!("{dir}/{name}.png")).unwrap();
            let mut enc = png::Encoder::new(f, 240, 160);
            enc.set_color(png::ColorType::Rgb);
            let mut w = enc.write_header().unwrap();
            let px: Vec<u8> = r.last.iter().flat_map(|p| [(p >> 16) as u8, (p >> 8) as u8, *p as u8]).collect();
            w.write_image_data(&px).unwrap();
        }
        // The picture: every frame an Advance one, with the pitch the frontend is handed.
        let runs = frames + frames / 2;
        assert_eq!(r.sizes, BTreeSet::from([(240, 160, 960)]), "{name}: picture size or pitch");
        assert_eq!(r.fmt, Some(1), "{name}: the core must ask for XRGB8888");
        assert_eq!((r.drawn, r.skipped), (runs, 0), "{name}: one picture per run");
        // It is still playing: a hung game draws the same picture for ever.
        assert!(r.pictures >= 20, "{name}: only {} different pictures in {runs} frames", r.pictures);
        assert!(r.best >= least_colours, "{name}: richest picture had {} colours", r.best);
        // Sound: the frontend is handed a full frame's worth every run, and a game with music
        // really makes some.
        let want = SAMPLES_PER_RUN * f64::from(runs);
        assert!((r.samples as f64 - want).abs() < want * 0.01, "{name}: {} samples, wanted about {want:.0}", r.samples);
        assert_eq!(r.quiet_runs < runs, music, "{name}: music expected {music}, {} of {runs} runs silent", r.quiet_runs);
        // A save state taken half way through carries on to exactly the same picture.
        assert!(r.state_len > 0 && r.state_matched, "{name}: the save state did not carry on the same");
    }
    eprintln!("{ran} homebrew games run");
}
