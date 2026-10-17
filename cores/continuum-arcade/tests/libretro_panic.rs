//! A panic inside cArcade never gets out of it. One that unwound out of a libretro entry point into
//! the frontend would end the whole app (Rust stops the process when a panic reaches an
//! `extern "C"` function), so every entry point catches it, logs it once and switches the core off,
//! as a poisoned lock leaves it: every frame after is the last picture again with silence, a game
//! won't load (with a sentence the player reads), and save states and Restart do nothing.
//!
//! The panics are planted (`libretro::test_hooks`, only in the tests' build), one where the board
//! runs (with the game's lock held, as a real one would be) and one at the start of each entry
//! point. This file is a process of its own, so a switched-off core can't upset the other tests.

mod common;

use std::ffi::{c_char, c_void, CStr};
use std::sync::{Mutex, Once};

use common::*;
use continuum_arcade::libretro::*;

/// The core is one global: tests take turns.
static SERIAL: Mutex<()> = Mutex::new(());

/// What the frontend was handed: a picture (its size and whether it was all black), or "the same
/// picture again"; and the sound.
#[derive(Clone, Debug, PartialEq)]
enum Frame {
    Picture(u32, u32, bool),
    Again(u32, u32),
}
static FRAMES: Mutex<Vec<Frame>> = Mutex::new(Vec::new());
static SOUND: Mutex<Vec<i16>> = Mutex::new(Vec::new());
/// The frontend's answer to GET_CAN_DUPE, and the options display callback the core gave it.
static CAN_DUPE: Mutex<bool> = Mutex::new(true);
static DISPLAY_CB: Mutex<Option<unsafe extern "C" fn() -> bool>> = Mutex::new(None);

fn take<T>(m: &Mutex<Vec<T>>) -> Vec<T> {
    std::mem::take(&mut *m.lock().unwrap_or_else(|e| e.into_inner()))
}

unsafe extern "C" fn env(cmd: u32, d: *mut c_void) -> bool {
    match cmd {
        3 => {
            *(d as *mut bool) = *CAN_DUPE.lock().unwrap();
            true
        }
        10 | 55 | 67 => true,
        69 => {
            *DISPLAY_CB.lock().unwrap() = *(d as *const Option<unsafe extern "C" fn() -> bool>);
            true
        }
        _ => false,
    }
}

unsafe extern "C" fn video(d: *const c_void, w: u32, h: u32, pitch: usize) {
    let frame = if d.is_null() {
        Frame::Again(w, h)
    } else {
        let px = std::slice::from_raw_parts(d as *const u8, pitch * h as usize);
        Frame::Picture(w, h, px.iter().all(|&b| b == 0))
    };
    FRAMES.lock().unwrap().push(frame);
}

unsafe extern "C" fn audio(d: *const i16, n: usize) -> usize {
    SOUND.lock().unwrap().extend_from_slice(std::slice::from_raw_parts(d, n * 2));
    n
}

unsafe extern "C" fn poll() {}

unsafe extern "C" fn input(_port: u32, _device: u32, _index: u32, _id: u32) -> i16 {
    0
}

/// The planted panics are expected: the default hook would print each one.
fn quiet_planted_panics() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let default = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let planted = info.payload().downcast_ref::<String>().is_some_and(|s| s.contains("planted by a test"));
            if !planted {
                default(info);
            }
        }));
    });
}

/// A fresh frontend, a core switched on, and Pac-Man's video test program loaded as pacman.zip.
fn start(can_dupe: bool) {
    quiet_planted_panics();
    test_hooks::revive();
    *CAN_DUPE.lock().unwrap() = can_dupe;
    retro_set_environment(env);
    retro_set_video_refresh(video);
    retro_set_audio_sample_batch(audio);
    retro_set_input_poll(poll);
    retro_set_input_state(input);
    retro_init();
    assert!(load(), "{}", last_log_line());
    take(&FRAMES);
    take(&SOUND);
}

fn load() -> bool {
    let case = cases().into_iter().find(|c| c.name == "video").unwrap();
    let dir = crate_dir().join("target/libretro-panic-test");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("pacman.zip");
    let files = files(&case);
    let refs: Vec<(&str, &[u8])> = files.iter().map(|(n, d)| (n.as_str(), d.as_slice())).collect();
    let bytes = continuum_arcade::zip::write_stored(&refs);
    std::fs::write(&path, &bytes).unwrap();
    let cpath = std::ffi::CString::new(path.to_str().unwrap()).unwrap();
    let info = RetroGameInfo { path: cpath.as_ptr(), data: bytes.as_ptr() as *const c_void, size: bytes.len(), meta: std::ptr::null() };
    unsafe { retro_load_game(&info) }
}

/// One `retro_run`: what it handed over.
fn frame() -> (Vec<Frame>, Vec<i16>) {
    unsafe { retro_run() };
    (take(&FRAMES), take(&SOUND))
}

fn is_still(f: &(Vec<Frame>, Vec<i16>)) -> bool {
    f.0 == [Frame::Again(224, 288)] && f.1.len() == 792 * 2 && f.1.iter().all(|&s| s == 0)
}

fn state() -> Vec<u8> {
    let size = retro_serialize_size();
    let mut s = vec![0u8; size];
    assert!(size > 0 && unsafe { retro_serialize(s.as_mut_ptr() as *mut c_void, size) });
    s
}

#[test]
fn a_panic_mid_frame_switches_the_core_off_and_the_app_goes_on() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    start(true);
    for _ in 0..5 {
        let (pictures, sound) = frame();
        assert_eq!(pictures.len(), 1);
        assert!(matches!(pictures[0], Frame::Picture(224, 288, false)));
        assert_eq!(sound.len(), 792 * 2);
    }
    let saved = state();
    // A panic while the board runs, with the game's lock held.
    test_hooks::panic_in("retro_run: board");
    let logged = lines_logged();
    let cut = frame();
    assert!(is_still(&cut), "the frame the panic cut short: {:?}, {} samples", cut.0, cut.1.len());
    assert!(test_hooks::switched_off());
    assert_eq!(lines_logged(), logged + 1, "one line");
    assert_eq!(
        last_log_line(),
        "switched off after an internal error in retro_run: a panic planted by a test in retro_run: board"
    );
    // From then on every frame is the last picture again and silence, and nothing more is logged.
    for _ in 0..3 {
        assert!(is_still(&frame()));
    }
    assert_eq!(lines_logged(), logged + 1, "logged once");
    // Save states and Restart do nothing.
    assert_eq!(retro_serialize_size(), 0);
    let mut buf = vec![0u8; saved.len()];
    assert!(!unsafe { retro_serialize(buf.as_mut_ptr() as *mut c_void, buf.len()) });
    assert!(!unsafe { retro_unserialize(saved.as_ptr() as *const c_void, saved.len()) });
    retro_reset();
    assert!(is_still(&frame()));
    // A game won't load, and the player reads why.
    assert!(!load());
    assert_eq!(last_log_line(), BROKEN_LINE);
    assert!(is_still(&frame()));
    // What describes the core still answers.
    assert_eq!(retro_api_version(), 1);
    let mut sys = std::mem::MaybeUninit::<RetroSystemInfo>::uninit();
    let sys = unsafe {
        retro_get_system_info(sys.as_mut_ptr());
        sys.assume_init()
    };
    assert_eq!(unsafe { CStr::from_ptr(sys.library_name) }.to_str().unwrap(), "cArcade");
    let mut av = std::mem::MaybeUninit::<RetroSystemAvInfo>::uninit();
    let av = unsafe {
        retro_get_system_av_info(av.as_mut_ptr());
        av.assume_init()
    };
    assert_eq!((av.geometry.base_width, av.geometry.base_height, av.geometry.aspect_ratio), (224, 288, 0.75));
    assert_eq!(av.timing.sample_rate, 48_000.0);
    retro_unload_game();
    retro_deinit();
    // Opened again (here: switched back on), it plays.
    start(true);
    assert!(matches!(frame().0[..], [Frame::Picture(224, 288, false)]));
    retro_unload_game();
}

/// A frontend that can't be handed "the same picture again" gets a black one the same size.
#[test]
fn a_frontend_that_cant_repeat_a_picture_gets_a_black_one() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    start(false);
    frame();
    test_hooks::panic_in("retro_run: board");
    for _ in 0..2 {
        let (pictures, sound) = frame();
        assert_eq!(pictures, [Frame::Picture(224, 288, true)]);
        assert!(sound.len() == 792 * 2 && sound.iter().all(|&s| s == 0));
    }
    retro_unload_game();
}

/// The entry points, by name, each called the way a frontend calls it: what it answered, so a
/// panic in it shows as its "that didn't work".
fn call(name: &str) -> String {
    let mut sys = std::mem::MaybeUninit::<RetroSystemInfo>::zeroed();
    let mut av = std::mem::MaybeUninit::<RetroSystemAvInfo>::zeroed();
    let mut buf = vec![0u8; 1 << 20];
    unsafe {
        match name {
            "retro_api_version" => retro_api_version().to_string(),
            "retro_get_system_info" => {
                retro_get_system_info(sys.as_mut_ptr());
                CStr::from_ptr(sys.assume_init().library_name).to_string_lossy().into_owned()
            }
            "retro_get_system_av_info" => {
                retro_get_system_av_info(av.as_mut_ptr());
                let av = av.assume_init();
                format!("{}x{}", av.geometry.base_width, av.geometry.base_height)
            }
            "retro_set_environment" => {
                retro_set_environment(env);
                String::new()
            }
            "retro_set_video_refresh" => {
                retro_set_video_refresh(video);
                String::new()
            }
            "retro_set_audio_sample" => {
                unsafe extern "C" fn sample(_l: i16, _r: i16) {}
                retro_set_audio_sample(sample);
                String::new()
            }
            "retro_set_audio_sample_batch" => {
                retro_set_audio_sample_batch(audio);
                String::new()
            }
            "retro_set_input_poll" => {
                retro_set_input_poll(poll);
                String::new()
            }
            "retro_set_input_state" => {
                retro_set_input_state(input);
                String::new()
            }
            "retro_init" => {
                retro_init();
                String::new()
            }
            "retro_deinit" => {
                retro_deinit();
                String::new()
            }
            "retro_set_controller_port_device" => {
                retro_set_controller_port_device(0, 1);
                String::new()
            }
            "retro_load_game" => load().to_string(),
            "retro_load_game_special" => retro_load_game_special(0, std::ptr::null(), 0).to_string(),
            "retro_unload_game" => {
                retro_unload_game();
                String::new()
            }
            "retro_get_region" => retro_get_region().to_string(),
            "retro_reset" => {
                retro_reset();
                String::new()
            }
            "retro_run" => {
                let f = frame();
                if is_still(&f) { "still".into() } else { format!("{:?}", f.0) }
            }
            "retro_serialize_size" => retro_serialize_size().to_string(),
            "retro_serialize" => retro_serialize(buf.as_mut_ptr() as *mut c_void, buf.len()).to_string(),
            "retro_unserialize" => {
                let s = state();
                buf[..s.len()].copy_from_slice(&s);
                retro_unserialize(buf.as_ptr() as *const c_void, s.len()).to_string()
            }
            "retro_cheat_reset" => {
                retro_cheat_reset();
                String::new()
            }
            "retro_cheat_set" => {
                retro_cheat_set(0, true, c"".as_ptr() as *const c_char);
                String::new()
            }
            "retro_get_memory_data" => retro_get_memory_data(0).is_null().to_string(),
            "retro_get_memory_size" => retro_get_memory_size(0).to_string(),
            "refresh_display" => {
                let cb = DISPLAY_CB.lock().unwrap().expect("the core gave its display callback");
                cb().to_string()
            }
            other => panic!("no such entry point {other}"),
        }
    }
}

/// Every entry point (and the options display callback, which the frontend calls like one), with
/// what it answers when a panic stops it: the same as a core that is switched off.
const ENTRY_POINTS: &[(&str, &str)] = &[
    ("retro_api_version", "1"),
    ("retro_get_system_info", "cArcade"),
    ("retro_get_system_av_info", "224x288"),
    ("retro_set_environment", ""),
    ("retro_set_video_refresh", ""),
    ("retro_set_audio_sample", ""),
    ("retro_set_audio_sample_batch", ""),
    ("retro_set_input_poll", ""),
    ("retro_set_input_state", ""),
    ("retro_init", ""),
    ("retro_deinit", ""),
    ("retro_set_controller_port_device", ""),
    ("retro_load_game", "false"),
    ("retro_load_game_special", "false"),
    ("retro_unload_game", ""),
    ("retro_get_region", "0"),
    ("retro_reset", ""),
    ("retro_run", "still"),
    ("retro_serialize_size", "0"),
    ("retro_serialize", "false"),
    ("retro_unserialize", "false"),
    ("retro_cheat_reset", ""),
    ("retro_cheat_set", ""),
    ("retro_get_memory_data", "true"),
    ("retro_get_memory_size", "0"),
    ("refresh_display", "false"),
];

#[test]
fn every_entry_point_catches_a_panic_in_it() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    for &(name, failed) in ENTRY_POINTS {
        start(true);
        frame();
        let logged = lines_logged();
        // (retro_unserialize's call takes a good state first, through the other entry points.)
        test_hooks::panic_in(name);
        let answer = call(name);
        assert!(test_hooks::switched_off(), "{name}: the core is still on");
        assert_eq!(answer, failed, "{name}'s answer after a panic");
        let line = format!("switched off after an internal error in {name}: a panic planted by a test in {name}");
        if name == "retro_load_game" {
            // Then the sentence the player reads, last.
            assert_eq!(lines_logged(), logged + 2, "{name}");
            assert_eq!(last_log_line(), BROKEN_LINE);
        } else {
            assert_eq!(lines_logged(), logged + 1, "{name}: logged once");
            assert_eq!(last_log_line(), line, "{name}");
        }
        // Switched off, the game's entry points say no, and the frames are still.
        assert!(is_still(&frame()), "{name}");
        assert_eq!(retro_serialize_size(), 0, "{name}");
    }
    test_hooks::revive();
}

/// Every `#[no_mangle]` function in the core runs its body through `guarded`, under its own name,
/// and the list above has them all: none can be added later without the guard and this test.
#[test]
fn every_entry_point_is_guarded() {
    let src = std::fs::read_to_string(crate_dir().join("src/libretro.rs")).unwrap();
    let mut found = Vec::new();
    for (i, _) in src.match_indices("#[no_mangle]") {
        let rest = &src[i..];
        let name = rest.split("fn ").nth(1).and_then(|s| s.split('(').next()).unwrap().trim().to_string();
        let body: String = rest[..rest.find("\n}\n").unwrap()].chars().filter(|c| !c.is_whitespace()).collect();
        assert!(body.contains(&format!("guarded(\"{name}\"")), "{name} isn't guarded under its own name");
        found.push(name);
    }
    let options = std::fs::read_to_string(crate_dir().join("src/options.rs")).unwrap();
    assert!(options.contains("guarded(\"refresh_display\""), "the options display callback isn't guarded");
    found.push("refresh_display".to_string());
    let listed: Vec<String> = ENTRY_POINTS.iter().map(|e| e.0.to_string()).collect();
    assert_eq!(found, listed, "every entry point, in the file's order");
}
