//! cMini through its libretro interface, as the app drives it: a game starts on cMini's own
//! start-up program, the picture and the sound come every frame (the sound of each frame as the
//! console made it, 612 to 614 samples of 44100 Hz at 72 frames a second, 735 at 60), save RAM
//! and system RAM have PokeMini's sizes and layout, a saved state plays back exactly, and the
//! reset starts the game again.

use std::ffi::{c_char, c_void};
use std::sync::Mutex;

use continuum_mini::libretro::*;

/// Each picture: width, height, and a hash of it.
type Pictures = Mutex<Vec<(u32, u32, u64)>>;
static FRAMES: Pictures = Mutex::new(Vec::new());
static SOUND: Mutex<Vec<(usize, u64)>> = Mutex::new(Vec::new());
static HZ60: Mutex<bool> = Mutex::new(false);

fn fnv(bytes: &[u8], mut h: u64) -> u64 {
    for &b in bytes {
        h = (h ^ u64::from(b)).wrapping_mul(0x100000001b3);
    }
    h
}

unsafe extern "C" fn env(cmd: u32, data: *mut c_void) -> bool {
    match cmd {
        // The system folder: none, so cMini's own start-up program runs.
        9 => {
            *(data as *mut *const c_char) = std::ptr::null();
            false
        }
        10 | 11 | 67 => true,
        // A setting: only the 60 Hz one is given.
        15 => {
            #[repr(C)]
            struct Var {
                key: *const c_char,
                value: *const c_char,
            }
            let v = &mut *(data as *mut Var);
            if std::ffi::CStr::from_ptr(v.key).to_bytes() == b"cmini_60hz" && *HZ60.lock().unwrap() {
                v.value = c"on".as_ptr();
                return true;
            }
            false
        }
        17 => {
            *(data as *mut bool) = false;
            true
        }
        _ => false,
    }
}
unsafe extern "C" fn video(data: *const c_void, w: u32, h: u32, pitch: usize) {
    let bytes = std::slice::from_raw_parts(data as *const u8, pitch * h as usize);
    FRAMES.lock().unwrap().push((w, h, fnv(bytes, 0xcbf29ce484222325)));
}
unsafe extern "C" fn audio(data: *const i16, frames: usize) -> usize {
    let s = std::slice::from_raw_parts(data, frames * 2);
    let bytes: Vec<u8> = s.iter().flat_map(|v| v.to_le_bytes()).collect();
    SOUND.lock().unwrap().push((frames, fnv(&bytes, 0xcbf29ce484222325)));
    frames
}
unsafe extern "C" fn poll() {}
unsafe extern "C" fn input(_: u32, _: u32, _: u32, _: u32) -> i16 {
    0
}

/// A small game: the PRC drawing a sprite that moves, timer 3 playing a tone whose pulse width
/// changes, and a count at $1500, all driven by the PRC's frame interrupt (every second frame,
/// the PRC's fastest).
fn game() -> Vec<u8> {
    let mut b = vec![0xFFu8; 0x8000];
    b[0x2100..0x2102].copy_from_slice(b"MN");
    let jrl = |b: &mut Vec<u8>, at: usize, to: usize| {
        let off = (to as i64 - at as i64 - 2) as u16;
        b[at..at + 3].copy_from_slice(&[0xF3, off as u8, (off >> 8) as u8]);
    };
    jrl(&mut b, 0x2102, 0x2400);
    // The frame interrupt's slot ($210E): answer it and return.
    b[0x2300..0x2304].copy_from_slice(&[0xDD, 0x27, 0xFF, 0xF9]);
    jrl(&mut b, 0x210E, 0x2300);
    let mut code = vec![0xB4, 0x20];
    for (reg, v) in [
        (0x80, 0x0E), (0x81, 0x08), (0x87, 0x00), (0x88, 0x30), (0x89, 0x00),
        (0x19, 0x30), (0x1C, 0x0B), (0x1D, 0x00), (0x4A, 0x00), (0x4B, 0x02), (0x4C, 0x00), (0x4D, 0x01), (0x48, 0x86), (0x71, 0x03),
        (0x20, 0xC0), (0x23, 0x40),
    ] {
        code.extend_from_slice(&[0xDD, reg, v]);
    }
    // Sprite 0 on, tile 0, at (40, 30).
    code.extend_from_slice(&[0xC7, 0x00, 0x13, 0xB7, 40, 0xC7, 0x01, 0x13, 0xB7, 30, 0xC7, 0x02, 0x13, 0xB7, 0, 0xC7, 0x03, 0x13, 0xB7, 0x08]);
    code.extend_from_slice(&[0x9C, 0x3F]);
    let top = code.len();
    // HALT; INC [$1500]; INC [$1300] (the sprite moves); INC [N+$4C] (the pulse widens); again.
    code.extend_from_slice(&[0xCE, 0xAE, 0xC5, 0x00, 0x15, 0x86, 0xC5, 0x00, 0x13, 0x86, 0x85, 0x4C]);
    let at = 0x2400 + code.len();
    let back = (0x2400 + top) as i64 - (at as i64 + 2);
    code.extend_from_slice(&[0xF3, back as u16 as u8, (back as u16 >> 8) as u8]);
    b[0x2400..0x2400 + code.len()].copy_from_slice(&code);
    // The sprite's tiles at $3000: a mask of 0 (all drawn) and a pattern.
    for k in 0..64 {
        b[0x3000 + k] = if (16..32).contains(&k) || k >= 48 { 0x5A ^ k as u8 } else { 0x00 };
    }
    b
}

fn load(rom: &[u8], hz60: bool) {
    *HZ60.lock().unwrap() = hz60;
    retro_set_environment(env);
    retro_set_video_refresh(video);
    retro_set_audio_sample_batch(audio);
    retro_set_input_poll(poll);
    retro_set_input_state(input);
    retro_init();
    let info = RetroGameInfo { path: std::ptr::null(), data: rom.as_ptr() as *const c_void, size: rom.len(), meta: std::ptr::null() };
    assert!(unsafe { retro_load_game(&info) }, "the game loads");
}

/// What `n` frames gave: the pictures, and the sound's batches (samples, hash).
type Output = (Vec<(u32, u32, u64)>, Vec<(usize, u64)>);

fn run(n: usize) -> Output {
    FRAMES.lock().unwrap().clear();
    SOUND.lock().unwrap().clear();
    for _ in 0..n {
        retro_run();
    }
    (FRAMES.lock().unwrap().clone(), SOUND.lock().unwrap().clone())
}

fn ram() -> Vec<u8> {
    let p = retro_get_memory_data(2) as *const u8;
    unsafe { std::slice::from_raw_parts(p, retro_get_memory_size(2)) }.to_vec()
}

#[test]
fn a_game_runs_through_the_libretro_interface_as_on_pokemini_s_core() {
    let rom = game();
    load(&rom, false);
    assert_eq!((retro_get_memory_size(0), retro_get_memory_size(2)), (8192, 8192));
    let (frames, sound) = run(240);
    assert_eq!(frames.len(), 240);
    assert!(frames.iter().all(|f| (f.0, f.1) == (96, 64)));
    let mut counts = std::collections::BTreeMap::new();
    for s in sound.iter().skip(1) {
        *counts.entry(s.0).or_insert(0) += 1;
    }
    assert!(counts.keys().all(|&n| (611..=614).contains(&n)), "{counts:?}");
    let total: usize = sound.iter().skip(1).map(|s| s.0).sum();
    assert!((total as f64 / 239.0 - 44_100.0 / 72.0).abs() < 1.0);
    // The game runs: its count goes up, the picture changes, the sound isn't silent.
    let mem = ram();
    let count = mem[0x500];
    // The start takes about a dozen frames; then one count every second frame.
    assert!((105..=121).contains(&count), "count {count}");
    assert!(frames.iter().map(|f| f.2).collect::<std::collections::BTreeSet<_>>().len() > 20);
    assert!(sound.iter().map(|s| s.1).collect::<std::collections::BTreeSet<_>>().len() > 20);
    // System RAM is laid out as on PokeMini: RAM, then the I/O registers at $1000.
    assert_eq!(mem[0x1080], 0x0E, "the PRC's mode register");
    // A state plays back exactly.
    let size = retro_serialize_size();
    let mut state = vec![0u8; size];
    assert!(unsafe { retro_serialize(state.as_mut_ptr() as *mut c_void, size) });
    let first = run(60);
    assert!(unsafe { retro_unserialize(state.as_ptr() as *const c_void, size) });
    let again = run(60);
    assert_eq!(first, again, "the state plays back the same pictures and sound");
    // A damaged state is refused and leaves the game as it was.
    let mut bad = state.clone();
    bad[8] ^= 0xFF;
    let before = ram();
    assert!(!unsafe { retro_unserialize(bad.as_ptr() as *const c_void, size) });
    assert_eq!(ram(), before);
    // The reset starts the game again: the count from 0.
    retro_reset();
    run(30);
    assert!(ram()[0x500] < 30);
    retro_unload_game();
    retro_deinit();

    // At 60 Hz: six of the console's frames to every five pictures, 735 samples a picture.
    load(&rom, true);
    // Past the start-up program's dozen frames.
    run(20);
    let before = ram()[0x500];
    let (frames, sound) = run(300);
    assert_eq!(frames.len(), 300);
    let total: usize = sound.iter().map(|s| s.0).sum();
    assert!((total as f64 / 300.0 - 735.0).abs() < 2.0, "{total}");
    // 300 pictures, 360 of the console's frames, the game's count one every second frame.
    let ran = ram()[0x500].wrapping_sub(before);
    assert!((179..=181).contains(&ran), "the game ran at 72 frames a second: {ran}");
    retro_unload_game();
    retro_deinit();
}
