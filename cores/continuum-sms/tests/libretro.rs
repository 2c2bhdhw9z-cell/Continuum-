//! cGear through its libretro entry points, the way the app drives it: a cartridge that switches
//! the screen on with a coloured backdrop, frames out at the picture's size, a frame's worth of
//! sound, the pad read through the ports, and a save state that comes back.

use std::ffi::c_void;
use std::sync::Mutex;

use continuum_sms::libretro::*;

struct Out {
    frames: Vec<(u32, u32, Vec<u32>)>,
    samples: usize,
}
static OUT: Mutex<Out> = Mutex::new(Out { frames: Vec::new(), samples: 0 });
static HELD: Mutex<Option<u32>> = Mutex::new(None);

/// The settings the app answers with: (key, value).
static OPTS: Mutex<Vec<(&'static str, std::ffi::CString)>> = Mutex::new(Vec::new());

#[repr(C)]
struct Var {
    key: *const std::ffi::c_char,
    value: *const std::ffi::c_char,
}

unsafe extern "C" fn env(cmd: u32, d: *mut c_void) -> bool {
    match cmd {
        10 => true,
        15 => {
            let v = &mut *(d as *mut Var);
            let key = std::ffi::CStr::from_ptr(v.key).to_string_lossy().into_owned();
            let opts = OPTS.lock().unwrap();
            match opts.iter().find(|(k, _)| *k == key) {
                Some((_, val)) => {
                    v.value = val.as_ptr();
                    true
                }
                None => false,
            }
        }
        _ => false,
    }
}
unsafe extern "C" fn video(d: *const c_void, w: u32, h: u32, pitch: usize) {
    assert_eq!(pitch, w as usize * 4);
    let px = std::slice::from_raw_parts(d as *const u32, (w * h) as usize).to_vec();
    OUT.lock().unwrap().frames.push((w, h, px));
}
unsafe extern "C" fn audio(_d: *const i16, n: usize) -> usize {
    OUT.lock().unwrap().samples += n;
    n
}
unsafe extern "C" fn poll() {}
unsafe extern "C" fn input(port: u32, dev: u32, _i: u32, id: u32) -> i16 {
    i16::from(port == 0 && dev == 1 && *HELD.lock().unwrap() == Some(id))
}

/// Sets register 7 (backdrop = sprite colour 0), colours 0 and 16 to blue, the screen on, then copies
/// port $DC to work RAM forever.
fn cartridge() -> Vec<u8> {
    let code: &[u8] = &[
        0xF3, // DI
        0x3E, 0x00, 0xD3, 0xBF, 0x3E, 0x87, 0xD3, 0xBF, // register 7 = 0
        0x3E, 0x00, 0xD3, 0xBF, 0x3E, 0xC0, 0xD3, 0xBF, // colour memory address 0
        0x3E, 0x30, 0xD3, 0xBE, // blue: colour 0, what a blank tile shows
        0x3E, 0x10, 0xD3, 0xBF, 0x3E, 0xC0, 0xD3, 0xBF, // colour memory address 16
        0x3E, 0x30, 0xD3, 0xBE, // blue: the backdrop, in the masked left column
        0x3E, 0x40, 0xD3, 0xBF, 0x3E, 0x81, 0xD3, 0xBF, // register 1 = $40: screen on
        0xDB, 0xDC, 0x32, 0x00, 0xC0, 0x18, 0xF9, // IN A,($DC) / LD ($C000),A / JR -7
    ];
    let mut rom = vec![0u8; 0x8000];
    rom[..code.len()].copy_from_slice(code);
    rom
}

/// Loads the cartridge with `opts`, runs `n` frames; returns the last picture's size and pixels,
/// and the samples made.
fn run_with(opts: &[(&'static str, &str)], n: usize) -> (u32, u32, Vec<u32>, usize) {
    *OPTS.lock().unwrap() = opts.iter().map(|(k, v)| (*k, std::ffi::CString::new(*v).unwrap())).collect();
    {
        let mut o = OUT.lock().unwrap();
        o.frames.clear();
        o.samples = 0;
    }
    let rom = cartridge();
    unsafe {
        retro_set_environment(env);
        retro_set_video_refresh(video);
        retro_set_audio_sample_batch(audio);
        retro_set_input_poll(poll);
        retro_set_input_state(input);
        retro_init();
        let info = RetroGameInfo { path: std::ptr::null(), data: rom.as_ptr() as *const c_void, size: rom.len(), meta: std::ptr::null() };
        assert!(retro_load_game(&info));
        for _ in 0..n {
            retro_run();
        }
        retro_unload_game();
        retro_deinit();
    }
    let mut o = OUT.lock().unwrap();
    let (w, h, px) = o.frames.pop().unwrap();
    *OPTS.lock().unwrap() = Vec::new();
    (w, h, px, o.samples)
}

static TURN: Mutex<()> = Mutex::new(());

#[test]
fn settings_reach_the_game() {
    let _t = TURN.lock().unwrap_or_else(|e| e.into_inner());
    let (w, h, px, _) = run_with(&[("cgear_hide_left", "on")], 5);
    assert_eq!((w, h, px.len()), (248, 192, 248 * 192), "the left column cut off");
    let (_, _, _, fifty) = run_with(&[("cgear_refresh", "50")], 50);
    let (_, _, _, sixty) = run_with(&[], 50);
    assert!(fifty > sixty * 115 / 100, "a 50 Hz frame is longer, so it makes more sound: {fifty} {sixty}");
    let (w, h, _, _) = run_with(&[("cgear_console", "gg")], 5);
    assert_eq!((w, h), (160, 144), "a Game Gear's own screen");
    let (w, h, _, _) = run_with(&[("cgear_console", "gg"), ("cgear_gg_full", "on")], 5);
    assert_eq!((w, h), (256, 192), "the whole picture it draws");
}

#[test]
fn a_cartridge_through_the_entry_points() {
    let _t = TURN.lock().unwrap_or_else(|e| e.into_inner());
    *OPTS.lock().unwrap() = Vec::new();
    {
        let mut o = OUT.lock().unwrap();
        o.frames.clear();
        o.samples = 0;
    }
    let rom = cartridge();
    unsafe {
        retro_set_environment(env);
        retro_set_video_refresh(video);
        retro_set_audio_sample_batch(audio);
        retro_set_input_poll(poll);
        retro_set_input_state(input);
        retro_init();
        let info = RetroGameInfo { path: std::ptr::null(), data: rom.as_ptr() as *const c_void, size: rom.len(), meta: std::ptr::null() };
        assert!(retro_load_game(&info));
        *HELD.lock().unwrap() = Some(8); // A: button 2
        for _ in 0..60 {
            retro_run();
        }
        let ram = retro_get_memory_data(2) as *const u8;
        assert_eq!(*ram & 0x3F, 0x3F & !0x20, "button 2 held reads 0 in its bit");
        let size = retro_serialize_size();
        let mut state = vec![0u8; size];
        assert!(retro_serialize(state.as_mut_ptr() as *mut c_void, size));
        *HELD.lock().unwrap() = None;
        retro_run();
        assert_eq!(*ram & 0x3F, 0x3F);
        assert!(retro_unserialize(state.as_ptr() as *const c_void, size));
        assert_eq!(*ram & 0x3F, 0x1F, "the state brought the old value back");
        retro_unload_game();
        retro_deinit();
    }
    let o = OUT.lock().unwrap();
    let (w, h, px) = o.frames.last().unwrap();
    assert_eq!((*w, *h), (256, 192));
    assert!(px.iter().all(|p| *p == 0x0000FF), "the blue backdrop everywhere");
    let per_frame = o.samples as f64 / o.frames.len() as f64;
    assert!((per_frame - 48_000.0 / 59.92).abs() < 3.0, "{per_frame}");
}
