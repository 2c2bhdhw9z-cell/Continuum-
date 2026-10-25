//! Runs a program through the real libretro calls (as the app does) for some seconds of fields
//! and reports what the always-on telemetry says, plus how many pictures came as repeats.
//!
//!     cargo run --release --example pictures -- program.dol [seconds]
//!
//! Checks the new-picture path: a picture is converted only when the TV picture changed, and
//! the app is sent a repeat (null frame) otherwise.
use continuum_gc::libretro::*;
use std::ffi::{c_void, CString};
use std::sync::atomic::{AtomicU64, Ordering};

static NEW: AtomicU64 = AtomicU64::new(0);
static REPEATS: AtomicU64 = AtomicU64::new(0);
static SOUND: AtomicU64 = AtomicU64::new(0);

unsafe extern "C" fn env(cmd: u32, data: *mut c_void) -> bool {
    match cmd {
        // GET_LOG_INTERFACE: print the core's lines.
        27 => {
            *(data as *mut usize) = log as usize;
            true
        }
        // SET_PIXEL_FORMAT
        10 => true,
        _ => false,
    }
}
// The core always logs with "%s" and one string; on the x86-64 test machine a variadic call
// passes that string exactly as a third ordinary argument, so this stands in for printf.
unsafe extern "C" fn log(_level: u32, _fmt: *const std::ffi::c_char, s: *const std::ffi::c_char) {
    eprint!("{}", std::ffi::CStr::from_ptr(s).to_string_lossy());
}
unsafe extern "C" fn video(d: *const c_void, _w: u32, _h: u32, _p: usize) {
    if d.is_null() {
        REPEATS.fetch_add(1, Ordering::SeqCst);
    } else {
        NEW.fetch_add(1, Ordering::SeqCst);
    }
}
unsafe extern "C" fn audio(_d: *const i16, n: usize) -> usize {
    SOUND.fetch_add(n as u64, Ordering::SeqCst);
    n
}
unsafe extern "C" fn poll() {}
unsafe extern "C" fn input(_p: u32, _d: u32, _i: u32, _id: u32) -> i16 {
    0
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = CString::new(args[1].clone()).unwrap();
    let seconds: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(10);
    retro_set_environment(env);
    retro_set_video_refresh(video);
    retro_set_audio_sample_batch(audio);
    retro_set_input_poll(poll);
    retro_set_input_state(input);
    retro_init();
    let info = RetroGameInfo { path: path.as_ptr(), data: std::ptr::null(), size: 0, meta: std::ptr::null() };
    assert!(unsafe { retro_load_game(&info) }, "load");
    for _ in 0..seconds * 60 {
        retro_run();
    }
    println!(
        "{} frames sent as new pictures, {} as repeats, {} sound frames",
        NEW.load(Ordering::SeqCst),
        REPEATS.load(Ordering::SeqCst),
        SOUND.load(Ordering::SeqCst)
    );
}
