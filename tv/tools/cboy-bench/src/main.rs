// cboy-bench <game.gba> [frames]: runs a game through the libretro entry points with a fixed
// button pattern. HASH=1 prints a fingerprint of every picture and sound sample, PROF=1 writes
// a sampling profile to /tmp/samples.txt, BENCH_OPTS="key=value,..." sets core options.
use std::ffi::{c_void, c_char};
use std::sync::atomic::{AtomicUsize, Ordering};
extern crate cboy_tv;
extern "C" {
    fn retro_set_environment(cb: unsafe extern "C" fn(u32, *mut c_void) -> bool);
    fn retro_set_video_refresh(cb: unsafe extern "C" fn(*const c_void, u32, u32, usize));
    fn retro_set_audio_sample_batch(cb: unsafe extern "C" fn(*const i16, usize) -> usize);
    fn retro_set_input_poll(cb: unsafe extern "C" fn());
    fn retro_set_input_state(cb: unsafe extern "C" fn(u32, u32, u32, u32) -> i16);
    fn retro_init();
    fn retro_load_game(g: *const GameInfo) -> bool;
    fn retro_run();
    fn retro_get_memory_data(id: u32) -> *mut c_void;
    fn retro_get_memory_size(id: u32) -> usize;
}
#[repr(C)] struct GameInfo { path: *const c_char, data: *const c_void, size: usize, meta: *const c_char }
static mut FRAME: u32 = 0;
static OPTS: std::sync::OnceLock<Vec<(std::ffi::CString, std::ffi::CString)>> = std::sync::OnceLock::new();
#[repr(C)] struct Var { key: *const c_char, value: *const c_char }
unsafe extern "C" fn env(cmd: u32, data: *mut c_void) -> bool {
    match cmd {
        10 => true,
        67 => true,
        15 => { let v = &mut *(data as *mut Var); let k = std::ffi::CStr::from_ptr(v.key);
                if let Some((_, val)) = OPTS.get().unwrap().iter().find(|(kk, _)| kk.as_c_str() == k) { v.value = val.as_ptr(); true } else { false } }
        9 | 31 => { *(data as *mut *const c_char) = c"/tmp".as_ptr(); true }
        _ => false,
    }
}
static mut HASH: u64 = 0xcbf29ce484222325;
// FRAMES=1: a picture and a sound fingerprint of every frame on its own line (to find the first
// frame where two builds part ways).
static mut VH: u64 = 0xcbf29ce484222325;
static mut AH: u64 = 0xcbf29ce484222325;
fn mix(b: &[u8]) { unsafe { for x in b { HASH ^= *x as u64; HASH = HASH.wrapping_mul(0x100000001b3); } } }
fn mix_to(h: *mut u64, b: &[u8]) { unsafe { for x in b { *h ^= *x as u64; *h = (*h).wrapping_mul(0x100000001b3); } } }
// SHOT=n SHOT_FILE=path: frame n's picture as raw 32-bit pixels (w, h as two u32 first).
unsafe extern "C" fn video(d: *const c_void, w: u32, h: u32, pitch: usize) {
    if !d.is_null() && std::env::var("SHOT").ok().and_then(|v| v.parse::<u32>().ok()) == Some(FRAME) {
        let mut out = Vec::new(); out.extend(w.to_le_bytes()); out.extend(h.to_le_bytes());
        for y in 0..h as usize { out.extend_from_slice(std::slice::from_raw_parts((d as *const u8).add(y*pitch), w as usize*4)); }
        std::fs::write(std::env::var("SHOT_FILE").unwrap_or("/tmp/shot.raw".into()), out).ok();
    }
    video_hash(d, w, h, pitch)
}
unsafe fn video_hash(d: *const c_void, w: u32, h: u32, pitch: usize) { if !d.is_null() && std::env::var_os("HASH").is_some() { for y in 0..h as usize { let row = std::slice::from_raw_parts((d as *const u8).add(y*pitch), w as usize*4); mix(row); mix_to(&raw mut VH, row); } } }
unsafe extern "C" fn audio(d: *const i16, n: usize) -> usize { if std::env::var_os("HASH").is_some() { let b = std::slice::from_raw_parts(d as *const u8, n*4); mix(b); mix_to(&raw mut AH, b); if let Some(p) = std::env::var_os("DUMPA") { use std::io::Write; let mut f = std::fs::OpenOptions::new().create(true).append(true).open(p).unwrap(); f.write_all(b).unwrap(); } } n }
unsafe extern "C" fn poll() {}
unsafe extern "C" fn input(_: u32, _: u32, _: u32, id: u32) -> i16 {
    // NOKEYS=1: press nothing (a game left alone on its title screen plays its intro).
    if std::env::var_os("NOKEYS").is_some() { return 0; }
    let p = FRAME % 120;
    let on = if p < 6 { id == 3 } else if (40..46).contains(&p) { id == 8 } else if p > 60 { id == 7 } else { false };
    on as i16
}
const N: usize = 1 << 20;
static mut SAMPLES: [usize; N] = [0; N];
static COUNT: AtomicUsize = AtomicUsize::new(0);
extern "C" fn on_prof(_: i32, _: *mut libc::siginfo_t, ctx: *mut c_void) {
    unsafe {
        let uc = ctx as *mut libc::ucontext_t;
        #[cfg(target_arch = "x86_64")]
        let pc = (*uc).uc_mcontext.gregs[libc::REG_RIP as usize] as usize;
        // Built for the TV (tv/tools/profile-cboy-tv.sh): the ARM program counter.
        #[cfg(target_arch = "arm")]
        let pc = (*uc).uc_mcontext.arm_pc as usize;
        let i = COUNT.fetch_add(1, Ordering::Relaxed);
        if i < N { SAMPLES[i] = pc; }
    }
}
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let rom = std::fs::read(&args[1]).unwrap();
    let frames: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(1200);
    let opts: Vec<(std::ffi::CString, std::ffi::CString)> = std::env::var("BENCH_OPTS").unwrap_or_default().split(',').filter_map(|kv| kv.split_once('=')).map(|(k, v)| (std::ffi::CString::new(k).unwrap(), std::ffi::CString::new(v).unwrap())).collect();
    OPTS.set(opts).ok();
    unsafe {
        retro_set_environment(env); retro_set_video_refresh(video); retro_set_audio_sample_batch(audio);
        retro_set_input_poll(poll); retro_set_input_state(input); retro_init();
        let path = std::ffi::CString::new(args[1].clone()).unwrap();
        let gi = GameInfo { path: path.as_ptr(), data: rom.as_ptr() as *const c_void, size: rom.len(), meta: std::ptr::null() };
        assert!(retro_load_game(&gi));
        // SRM=path: the game's battery save, put in as a frontend does after loading.
        if let Some(p) = std::env::var_os("SRM") {
            let d = std::fs::read(p).unwrap();
            let (m, n) = (retro_get_memory_data(0) as *mut u8, retro_get_memory_size(0));
            if !m.is_null() { std::ptr::copy_nonoverlapping(d.as_ptr(), m, d.len().min(n)); }
            eprintln!("save {} bytes into {} bytes", d.len(), n);
        }
        let prof = std::env::var_os("PROF").is_some();
        // PROF_FROM=n: run n frames first, then start sampling (to look at one part of a game).
        let from: u32 = std::env::var("PROF_FROM").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
        for i in 0..from { FRAME = i; retro_run(); }
        if prof {
            let mut sa: libc::sigaction = std::mem::zeroed();
            sa.sa_sigaction = on_prof as usize; sa.sa_flags = libc::SA_SIGINFO | libc::SA_RESTART;
            libc::sigaction(libc::SIGPROF, &sa, std::ptr::null_mut());
            let it = libc::itimerval { it_interval: libc::timeval { tv_sec: 0, tv_usec: 500 }, it_value: libc::timeval { tv_sec: 0, tv_usec: 500 } };
            libc::setitimer(libc::ITIMER_PROF, &it, std::ptr::null_mut());
        }
        let t = std::time::Instant::now();
        let per_frame = std::env::var_os("FRAMES").is_some();
        // TIMES=1: the speed of every second (60 frames) on its own line, to find the slow parts.
        let times = std::env::var_os("TIMES").is_some();
        let mut w = std::time::Instant::now();
        for i in from..frames {
            FRAME = i; retro_run();
            if per_frame { println!("{i} v {:016x} a {:016x}", VH, AH); }
            if times && i % 60 == 59 { println!("frames {}-{} {:.1} fps", i - 59, i, 60.0 / w.elapsed().as_secs_f64()); w = std::time::Instant::now(); }
        }
        let s = t.elapsed().as_secs_f64();
        eprintln!("{frames} frames {:.2}s = {:.1} fps hash {:016x}", s, frames as f64 / s, HASH);
        if prof {
            let off = { let maps = std::fs::read_to_string("/proc/self/maps").unwrap(); let exe = std::fs::read_link("/proc/self/exe").unwrap();
                let line = maps.lines().find(|l| l.ends_with(exe.to_str().unwrap())).unwrap();
                usize::from_str_radix(line.split('-').next().unwrap(), 16).unwrap() };
            let n = COUNT.load(Ordering::Relaxed).min(N);
            let mut out = String::new();
            for i in 0..n { out.push_str(&format!("{:x}\n", SAMPLES[i] - off)); }
            std::fs::write("/tmp/samples.txt", out).unwrap();
            eprintln!("{n} samples");
        }
    }
}
