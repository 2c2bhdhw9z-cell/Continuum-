//! cAtari through its libretro entry points, as the app calls them: the settings it declares, the
//! picture and sound a frame gives, the controls, save states, reset, and the memory it publishes.
//! The game is our own test program (test-programs/zones.asm) in an .a78 header.

use continuum_atari::libretro::*;
use std::ffi::{c_char, c_void, CStr, CString};
use std::sync::Mutex;

struct Seen {
    keys: Vec<String>,
    values: Vec<(String, String)>,
    changed: bool,
    picture: Vec<u32>,
    size: (u32, u32),
    audio: usize,
    buttons: [[bool; 16]; 2],
    maps: usize,
}

static SEEN: Mutex<Seen> = Mutex::new(Seen {
    keys: Vec::new(),
    values: Vec::new(),
    changed: false,
    picture: Vec::new(),
    size: (0, 0),
    audio: 0,
    buttons: [[false; 16]; 2],
    maps: 0,
});
/// The tests share the core (it is one per process), so they run one at a time.
static ONE: Mutex<()> = Mutex::new(());

#[repr(C)]
struct OptValue {
    value: *const c_char,
    label: *const c_char,
}
#[repr(C)]
struct OptDefinition {
    key: *const c_char,
    desc: *const c_char,
    desc_categorized: *const c_char,
    info: *const c_char,
    info_categorized: *const c_char,
    category_key: *const c_char,
    values: [OptValue; 128],
    default_value: *const c_char,
}
#[repr(C)]
struct OptionsV2 {
    categories: *const c_void,
    definitions: *const OptDefinition,
}
#[repr(C)]
struct Variable {
    key: *const c_char,
    value: *const c_char,
}

static VALUE_STORE: Mutex<Vec<CString>> = Mutex::new(Vec::new());

unsafe extern "C" fn env(cmd: u32, data: *mut c_void) -> bool {
    match cmd {
        67 => {
            let o = &*(data as *const OptionsV2);
            let mut d = o.definitions;
            let mut seen = SEEN.lock().unwrap();
            while !(*d).key.is_null() {
                seen.keys.push(CStr::from_ptr((*d).key).to_string_lossy().into_owned());
                d = d.add(1);
            }
            true
        }
        15 => {
            let v = &mut *(data as *mut Variable);
            let key = CStr::from_ptr(v.key).to_string_lossy().into_owned();
            let seen = SEEN.lock().unwrap();
            if let Some((_, val)) = seen.values.iter().find(|(k, _)| *k == key) {
                let c = CString::new(val.as_str()).unwrap();
                v.value = c.as_ptr();
                VALUE_STORE.lock().unwrap().push(c);
                return true;
            }
            false
        }
        17 => {
            let mut seen = SEEN.lock().unwrap();
            *(data as *mut bool) = seen.changed;
            seen.changed = false;
            true
        }
        36 => {
            #[repr(C)]
            struct Map {
                d: *const c_void,
                n: u32,
            }
            SEEN.lock().unwrap().maps = (*(data as *const Map)).n as usize;
            true
        }
        10 | 11 => true,
        9 => {
            *(data as *mut *const c_char) = c"/nonexistent".as_ptr();
            true
        }
        _ => false,
    }
}

unsafe extern "C" fn video(data: *const c_void, w: u32, h: u32, pitch: usize) {
    let mut seen = SEEN.lock().unwrap();
    seen.size = (w, h);
    seen.picture.clear();
    for y in 0..h as usize {
        let row = (data as *const u8).add(y * pitch) as *const u32;
        for x in 0..w as usize {
            seen.picture.push(*row.add(x));
        }
    }
}

unsafe extern "C" fn audio(_data: *const i16, frames: usize) -> usize {
    SEEN.lock().unwrap().audio += frames;
    frames
}

unsafe extern "C" fn poll() {}

unsafe extern "C" fn input(port: u32, device: u32, _index: u32, id: u32) -> i16 {
    if device != 1 || port > 1 || id > 15 {
        return 0;
    }
    SEEN.lock().unwrap().buttons[port as usize][id as usize] as i16
}

fn cart() -> Vec<u8> {
    let rom = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/test-programs/zones.bin")).unwrap();
    let mut h = vec![0u8; 128];
    h[0] = 3;
    h[1..10].copy_from_slice(b"ATARI7800");
    h[49..53].copy_from_slice(&(rom.len() as u32).to_be_bytes());
    h[55] = 1;
    h[56] = 1;
    h[100..128].copy_from_slice(b"ACTUAL CART DATA STARTS HERE");
    h.extend(rom);
    h
}

fn start(values: &[(&str, &str)]) -> Vec<u8> {
    {
        let mut seen = SEEN.lock().unwrap();
        seen.keys.clear();
        seen.values = values.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        seen.audio = 0;
        seen.buttons = [[false; 16]; 2];
    }
    retro_set_environment(env);
    retro_set_video_refresh(video);
    retro_set_audio_sample_batch(audio);
    retro_set_input_poll(poll);
    retro_set_input_state(input);
    retro_init();
    let data = cart();
    let info = RetroGameInfo { path: c"zones.a78".as_ptr(), data: data.as_ptr() as *const c_void, size: data.len(), meta: std::ptr::null() };
    assert!(unsafe { retro_load_game(&info) });
    data
}

fn run(frames: usize) {
    for _ in 0..frames {
        unsafe { retro_run() };
    }
}

#[test]
fn a_game_plays_with_its_picture_and_sound() {
    let _one = ONE.lock().unwrap();
    let _data = start(&[]);
    run(60);
    let seen = SEEN.lock().unwrap();
    assert_eq!(seen.size, (320, 224));
    // 60 frames at 59.958 a second: 48,034 samples at 48 kHz, give or take one.
    assert!((48_033..=48_035).contains(&seen.audio), "{} samples", seen.audio);
    // The test program's objects are on the picture: more than the background's colour.
    let mut colours: Vec<u32> = seen.picture.clone();
    colours.sort_unstable();
    colours.dedup();
    assert!(colours.len() >= 5, "{} colours", colours.len());
    assert!(seen.keys.iter().all(|k| k.starts_with("catari_")));
    assert_eq!(seen.keys.len(), continuum_atari::options::keys().len());
    assert!(seen.maps >= 7, "the memory map was published");
    drop(seen);
    retro_unload_game();
}

#[test]
fn whole_picture_shows_every_line_maria_draws() {
    let _one = ONE.lock().unwrap();
    let _data = start(&[("catari_overscan", "on")]);
    run(3);
    assert_eq!(SEEN.lock().unwrap().size, (320, 242));
    retro_unload_game();
}

#[test]
fn a_save_state_comes_back_exactly() {
    let _one = ONE.lock().unwrap();
    let _data = start(&[]);
    run(30);
    let size = retro_serialize_size();
    let mut state = vec![0u8; size];
    assert!(unsafe { retro_serialize(state.as_mut_ptr() as *mut c_void, size) });
    run(10);
    let after = SEEN.lock().unwrap().picture.clone();
    run(25);
    assert!(unsafe { retro_unserialize(state.as_ptr() as *const c_void, size) });
    run(10);
    assert_eq!(SEEN.lock().unwrap().picture, after, "the same ten frames after loading the state");
    retro_unload_game();
}

#[test]
fn reset_starts_the_game_again() {
    let _one = ONE.lock().unwrap();
    let _data = start(&[]);
    run(20);
    let first = SEEN.lock().unwrap().picture.clone();
    run(20);
    retro_reset();
    run(20);
    assert_eq!(SEEN.lock().unwrap().picture, first, "frame 20 after a reset is frame 20 of a fresh start");
    retro_unload_game();
}

#[test]
fn colours_change_without_a_restart() {
    let _one = ONE.lock().unwrap();
    let _data = start(&[]);
    run(5);
    let before = SEEN.lock().unwrap().picture.clone();
    {
        let mut seen = SEEN.lock().unwrap();
        seen.values = vec![("catari_palette".into(), "grey".into())];
        seen.changed = true;
    }
    run(1);
    let seen = SEEN.lock().unwrap();
    assert_ne!(seen.picture, before);
    assert!(seen.picture.iter().all(|&p| (p & 0xFF) * 0x010101 == p & 0xFFFFFF), "black and white");
    drop(seen);
    retro_unload_game();
}
