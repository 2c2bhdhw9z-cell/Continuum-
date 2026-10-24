//! The libretro entry points driven the way the app's engine drives them: load our own test
//! DOL from a path, ask about JIT, run frames, get pictures, save and load a state.

use continuum_gc::libretro::*;
use std::ffi::{c_void, CString};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

static FRAMES: AtomicU32 = AtomicU32::new(0);
static LAST_W: AtomicU32 = AtomicU32::new(0);
static JIT_ASKED: AtomicBool = AtomicBool::new(false);

unsafe extern "C" fn env(cmd: u32, data: *mut c_void) -> bool {
    match cmd {
        10 => true,
        74 => {
            JIT_ASKED.store(true, Ordering::SeqCst);
            *(data as *mut bool) = false;
            true
        }
        _ => false,
    }
}
unsafe extern "C" fn video(_d: *const c_void, w: u32, _h: u32, _p: usize) {
    FRAMES.fetch_add(1, Ordering::SeqCst);
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
    let size = retro_serialize_size();
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
