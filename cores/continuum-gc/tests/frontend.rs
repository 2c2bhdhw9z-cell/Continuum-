//! The libretro entry points driven the way the app's engine drives them: load our own test
//! DOL from a path, ask about JIT, run frames, get pictures, save and load a state.

use continuum_gc::libretro::*;
use std::ffi::{c_void, CString};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

static FRAMES: AtomicU32 = AtomicU32::new(0);
static LAST_W: AtomicU32 = AtomicU32::new(0);
static JIT_ASKED: AtomicBool = AtomicBool::new(false);
static READOUT_DECLARED: AtomicBool = AtomicBool::new(false);
static READOUT_ON: AtomicBool = AtomicBool::new(false);
static READOUT_CHANGED: AtomicBool = AtomicBool::new(false);
/// The picture's pixel at (6, 4): white where the read-out's first letter is.
static CORNER: AtomicU32 = AtomicU32::new(0);

#[repr(C)]
struct Definition {
    key: *const std::ffi::c_char,
    rest: [*const std::ffi::c_char; 5],
    values: [[*const std::ffi::c_char; 2]; 128],
    default: *const std::ffi::c_char,
}

unsafe extern "C" fn env(cmd: u32, data: *mut c_void) -> bool {
    match cmd {
        10 => true,
        // SET_CORE_OPTIONS_V2: look for the speed read-out among the definitions.
        67 => {
            let mut d = *(data as *const *const Definition).add(1);
            while !(*d).key.is_null() {
                if std::ffi::CStr::from_ptr((*d).key).to_str() == Ok("cgc_speed_readout") {
                    READOUT_DECLARED.store(true, Ordering::SeqCst);
                }
                d = d.add(1);
            }
            true
        }
        // GET_VARIABLE.
        15 => {
            let var = data as *mut [*const std::ffi::c_char; 2];
            if std::ffi::CStr::from_ptr((*var)[0]).to_str() == Ok("cgc_speed_readout") {
                (*var)[1] = if READOUT_ON.load(Ordering::SeqCst) { c"on".as_ptr() } else { c"off".as_ptr() };
                return true;
            }
            false
        }
        // GET_VARIABLE_UPDATE.
        17 => {
            *(data as *mut bool) = READOUT_CHANGED.swap(false, Ordering::SeqCst);
            true
        }
        74 => {
            JIT_ASKED.store(true, Ordering::SeqCst);
            *(data as *mut bool) = false;
            true
        }
        _ => false,
    }
}
unsafe extern "C" fn video(d: *const c_void, w: u32, _h: u32, _p: usize) {
    FRAMES.fetch_add(1, Ordering::SeqCst);
    CORNER.store(*(d as *const u32).add(4 * w as usize + 6), Ordering::SeqCst);
    LAST_W.store(w, Ordering::SeqCst);
}
unsafe extern "C" fn audio(_d: *const i16, n: usize) -> usize {
    n
}
unsafe extern "C" fn poll() {}
unsafe extern "C" fn input(_p: u32, _d: u32, _i: u32, _id: u32) -> i16 {
    0
}

#[test]
fn loads_runs_and_saves_through_the_libretro_calls() {
    // The smallest program: turn the display on (640 wide) and hang.
    let words: Vec<u32> = vec![
        0x3C60_CC00, // lis r3, 0xCC00
        0x6063_2000, // ori r3, r3, 0x2000
        0x3880_0F06, // li r4, (240 << 4) | 6
        0xB083_0000, // sth r4, 0(r3)
        0x3880_2850, // li r4, (40 << 8) | 80
        0xB083_0048, // sth r4, 0x48(r3)
        0x3880_0005, // li r4, 5
        0xB083_0002, // sth r4, 2(r3)
        0x4800_0000, // b .
    ];
    let mut dol = vec![0u8; 0x100];
    dol[0..4].copy_from_slice(&0x100u32.to_be_bytes());
    dol[0x48..0x4C].copy_from_slice(&0x8000_3100u32.to_be_bytes());
    dol[0x90..0x94].copy_from_slice(&((words.len() * 4) as u32).to_be_bytes());
    dol[0xE0..0xE4].copy_from_slice(&0x8000_3100u32.to_be_bytes());
    dol.extend(words.iter().flat_map(|w| w.to_be_bytes()));
    let path = std::env::temp_dir().join("continuum_gc_frontend_test.dol");
    std::fs::write(&path, dol).unwrap();
    let cpath = CString::new(path.to_str().unwrap()).unwrap();

    retro_set_environment(env);
    retro_set_video_refresh(video);
    retro_set_audio_sample_batch(audio);
    retro_set_input_poll(poll);
    retro_set_input_state(input);
    retro_init();
    let info = RetroGameInfo { path: cpath.as_ptr(), data: std::ptr::null(), size: 0, meta: std::ptr::null() };
    assert!(unsafe { retro_load_game(&info) });
    assert!(JIT_ASKED.load(Ordering::SeqCst), "the core asks the engine about JIT");
    assert!(!JIT_ALLOWED.load(Ordering::SeqCst));
    for _ in 0..3 {
        retro_run();
    }
    assert_eq!(FRAMES.load(Ordering::SeqCst), 3);
    assert_eq!(LAST_W.load(Ordering::SeqCst), 640);
    // The speed read-out: declared, off by default, drawn in the corner once switched on.
    assert!(READOUT_DECLARED.load(Ordering::SeqCst), "the speed read-out option is declared");
    assert_ne!(CORNER.load(Ordering::SeqCst), 0x00FF_FFFF);
    READOUT_ON.store(true, Ordering::SeqCst);
    READOUT_CHANGED.store(true, Ordering::SeqCst);
    retro_run();
    assert_eq!(CORNER.load(Ordering::SeqCst), 0x00FF_FFFF, "the read-out's first letter");
    READOUT_ON.store(false, Ordering::SeqCst);
    READOUT_CHANGED.store(true, Ordering::SeqCst);
    retro_run();
    assert_ne!(CORNER.load(Ordering::SeqCst), 0x00FF_FFFF, "gone again");
    let size = retro_serialize_size();
    // A GameCube's states leave out the Wii's 64 MB of MEM2.
    assert!(size < 64 << 20, "GameCube state room {size}");
    let mut buf = vec![0u8; size];
    assert!(unsafe { retro_serialize(buf.as_mut_ptr() as *mut c_void, size) });
    retro_run();
    assert!(unsafe { retro_unserialize(buf.as_ptr() as *const c_void, size) });
    retro_unload_game();
    retro_deinit();
    let missing = CString::new("/nonexistent/game.iso").unwrap();
    let bad = RetroGameInfo { path: missing.as_ptr(), data: std::ptr::null(), size: 0, meta: std::ptr::null() };
    assert!(!unsafe { retro_load_game(&bad) });
}
