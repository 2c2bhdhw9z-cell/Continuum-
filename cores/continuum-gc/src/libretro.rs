//! The libretro face of the core: what the app's engine calls.
//!
//! JIT (owner rule: used automatically wherever the phone allows it): at load the core asks
//! `RETRO_ENVIRONMENT_GET_JIT_CAPABLE`, which the engine answers from `crates/emulator-bridge/
//! src/jit.rs`. When the answer is yes, the machine turns our JIT on (step G8), taking its code
//! memory from the app (`continuum_jit_region`, two addresses for the same memory as iOS 26
//! needs); when the app gives none, or the answer is no, the fast interpreter runs. The core id
//! the engine's `core_may_use_jit` list must name for iOS 26 phones is `continuum_gc`.

use crate::disc::{Console, Disc};
use crate::dol::Executable;
use crate::hw::pad;
use crate::machine::Machine;
use std::ffi::{c_char, c_void, CStr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

#[repr(C)]
pub struct RetroSystemInfo {
    pub library_name: *const c_char,
    pub library_version: *const c_char,
    pub valid_extensions: *const c_char,
    pub need_fullpath: bool,
    pub block_extract: bool,
}

#[repr(C)]
pub struct RetroGameGeometry {
    pub base_width: u32,
    pub base_height: u32,
    pub max_width: u32,
    pub max_height: u32,
    pub aspect_ratio: f32,
}

#[repr(C)]
pub struct RetroSystemTiming {
    pub fps: f64,
    pub sample_rate: f64,
}

#[repr(C)]
pub struct RetroSystemAvInfo {
    pub geometry: RetroGameGeometry,
    pub timing: RetroSystemTiming,
}

#[repr(C)]
pub struct RetroGameInfo {
    pub path: *const c_char,
    pub data: *const c_void,
    pub size: usize,
    pub meta: *const c_char,
}

type EnvFn = unsafe extern "C" fn(u32, *mut c_void) -> bool;
type VideoFn = unsafe extern "C" fn(*const c_void, u32, u32, usize);
type AudioFn = unsafe extern "C" fn(i16, i16);
type AudioBatchFn = unsafe extern "C" fn(*const i16, usize) -> usize;
type PollFn = unsafe extern "C" fn();
type InputFn = unsafe extern "C" fn(u32, u32, u32, u32) -> i16;

const ENV_SET_PIXEL_FORMAT: u32 = 10;
const ENV_GET_JIT_CAPABLE: u32 = 74;
const PIXEL_FORMAT_XRGB8888: u32 = 1;
const DEVICE_JOYPAD: u32 = 1;
const DEVICE_ANALOG: u32 = 5;

const MAX_W: u32 = 720;
const MAX_H: u32 = 576;
const SAMPLE_RATE: f64 = 48_000.0;
const FPS: f64 = 59.94;
const FRAME_SAMPLES: usize = 801;
static SILENCE: [i16; 2 * FRAME_SAMPLES] = [0; 2 * FRAME_SAMPLES];

/// Whether the engine said JIT may be used, asked once at load.
pub static JIT_ALLOWED: AtomicBool = AtomicBool::new(false);

#[derive(Default)]
struct Callbacks {
    env: Option<EnvFn>,
    video: Option<VideoFn>,
    audio: Option<AudioBatchFn>,
    poll: Option<PollFn>,
    input: Option<InputFn>,
}

struct Core {
    machine: Machine,
    frame: Vec<u32>,
    size: (u32, u32),
}

static CORE: Mutex<Option<Core>> = Mutex::new(None);
static CB: Mutex<Callbacks> = Mutex::new(Callbacks { env: None, video: None, audio: None, poll: None, input: None });

fn cb() -> std::sync::MutexGuard<'static, Callbacks> {
    CB.lock().unwrap_or_else(|p| p.into_inner())
}
fn core() -> std::sync::MutexGuard<'static, Option<Core>> {
    CORE.lock().unwrap_or_else(|p| p.into_inner())
}

#[no_mangle]
pub extern "C" fn retro_api_version() -> u32 {
    1
}

/// # Safety
/// `info` must point to writable memory for one `RetroSystemInfo`.
#[no_mangle]
pub unsafe extern "C" fn retro_get_system_info(info: *mut RetroSystemInfo) {
    *info = RetroSystemInfo {
        library_name: c"Continuum GC".as_ptr(),
        library_version: c"0.1".as_ptr(),
        valid_extensions: c"dol|elf|iso|gcm|rvz|wia|gcz|ciso|tgc".as_ptr(),
        need_fullpath: true,
        block_extract: false,
    };
}

/// # Safety
/// `info` must point to writable memory for one `RetroSystemAvInfo`.
#[no_mangle]
pub unsafe extern "C" fn retro_get_system_av_info(info: *mut RetroSystemAvInfo) {
    *info = RetroSystemAvInfo {
        geometry: RetroGameGeometry { base_width: 640, base_height: 480, max_width: MAX_W, max_height: MAX_H, aspect_ratio: 4.0 / 3.0 },
        timing: RetroSystemTiming { fps: FPS, sample_rate: SAMPLE_RATE },
    };
}

#[no_mangle]
pub extern "C" fn retro_set_environment(f: EnvFn) {
    cb().env = Some(f);
}
#[no_mangle]
pub extern "C" fn retro_set_video_refresh(f: VideoFn) {
    cb().video = Some(f);
}
#[no_mangle]
pub extern "C" fn retro_set_audio_sample(_f: AudioFn) {}
#[no_mangle]
pub extern "C" fn retro_set_audio_sample_batch(f: AudioBatchFn) {
    cb().audio = Some(f);
}
#[no_mangle]
pub extern "C" fn retro_set_input_poll(f: PollFn) {
    cb().poll = Some(f);
}
#[no_mangle]
pub extern "C" fn retro_set_input_state(f: InputFn) {
    cb().input = Some(f);
}
#[no_mangle]
pub extern "C" fn retro_init() {}
#[no_mangle]
pub extern "C" fn retro_deinit() {
    *core() = None;
}
#[no_mangle]
pub extern "C" fn retro_set_controller_port_device(_port: u32, _device: u32) {}

fn env(cmd: u32, data: *mut c_void) -> bool {
    let f = cb().env;
    // SAFETY: the frontend's callback, with a pointer of the type the command defines.
    f.is_some_and(|f| unsafe { f(cmd, data) })
}

/// How many helper threads draw pixels (gx/draw.rs; exactly the same picture as drawing on the
/// CPU's thread): all the processor cores but two (the emulated CPU's and the app's), at most
/// four, none on a phone with fewer than four cores.
fn draw_helpers() -> usize {
    let cores = std::thread::available_parallelism().map_or(1, |n| n.get());
    let n = cores.saturating_sub(2).min(4);
    if n < 2 {
        0
    } else {
        n
    }
}

/// Opens a game by its path: a DOL/ELF program, or a disc image.
pub fn open(path: &str) -> Result<Machine, String> {
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".dol") || lower.ends_with(".elf") {
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        let exe = Executable::parse(&bytes)?;
        let mut m = Machine::new(Console::GameCube);
        m.boot_executable(&exe);
        return Ok(m);
    }
    let disc: Box<dyn Disc> = crate::disc::open_image(path)?;
    let mut m = Machine::new(Console::GameCube);
    m.boot_disc(disc)?;
    Ok(m)
}

/// # Safety
/// `game` must be null or point to a valid `RetroGameInfo` with a NUL-terminated path.
#[no_mangle]
pub unsafe extern "C" fn retro_load_game(game: *const RetroGameInfo) -> bool {
    if game.is_null() || (*game).path.is_null() {
        return false;
    }
    let path = CStr::from_ptr((*game).path).to_string_lossy().into_owned();
    let mut fmt = PIXEL_FORMAT_XRGB8888;
    if !env(ENV_SET_PIXEL_FORMAT, &mut fmt as *mut u32 as *mut c_void) {
        return false;
    }
    let mut jit = false;
    let asked = env(ENV_GET_JIT_CAPABLE, &mut jit as *mut bool as *mut c_void);
    JIT_ALLOWED.store(asked && jit, Ordering::SeqCst);
    match open(&path) {
        Ok(mut machine) => {
            machine.set_clock(local_seconds_since_2000());
            if JIT_ALLOWED.load(Ordering::SeqCst) {
                machine.enable_jit();
            }
            machine.hw.gx.set_draw_threads(draw_helpers());
            *core() = Some(Core { machine, frame: vec![0; 640 * 480], size: (640, 480) });
            true
        }
        Err(_) => false,
    }
}

#[no_mangle]
pub extern "C" fn retro_load_game_special(_t: u32, _i: *const RetroGameInfo, _n: usize) -> bool {
    false
}

#[no_mangle]
pub extern "C" fn retro_unload_game() {
    *core() = None;
}

#[no_mangle]
pub extern "C" fn retro_reset() {
    // A restart re-opens the game through the frontend; nothing to do in place yet.
}

fn read_pad(input: InputFn, port: u32) -> crate::hw::PadState {
    // SAFETY: the frontend's input callback.
    let b = |id: u32| unsafe { input(port, DEVICE_JOYPAD, 0, id) } != 0;
    let axis = |index: u32, id: u32| unsafe { input(port, DEVICE_ANALOG, index, id) };
    let mut buttons = 0u16;
    // RetroPad -> GameCube: B/A/Y/X by position (GameCube A is the big right button).
    for (id, bit) in [
        (8, pad::A), (0, pad::B), (9, pad::X), (1, pad::Y), (3, pad::START),
        (4, pad::UP), (5, pad::DOWN), (6, pad::LEFT), (7, pad::RIGHT),
        (10, pad::L), (11, pad::R), (12, pad::Z), (13, pad::Z),
    ] {
        if b(id) {
            buttons |= bit;
        }
    }
    let to_u8 = |v: i16, invert: bool| {
        let v = if invert { -(v as i32) } else { v as i32 };
        ((v + 32768) >> 8).clamp(0, 255) as u8
    };
    crate::hw::PadState {
        connected: true,
        buttons,
        stick: (to_u8(axis(0, 0), false), to_u8(axis(0, 1), true)),
        cstick: (to_u8(axis(1, 0), false), to_u8(axis(1, 1), true)),
        triggers: (if buttons & pad::L != 0 { 255 } else { 0 }, if buttons & pad::R != 0 { 255 } else { 0 }),
        rumble: false,
    }
}

#[no_mangle]
pub extern "C" fn retro_run() {
    let (video, audio, poll, input) = {
        let c = cb();
        (c.video, c.audio, c.poll, c.input)
    };
    let mut guard = core();
    let Some(core) = guard.as_mut() else { return };
    if let Some(poll) = poll {
        // SAFETY: the frontend's poll callback.
        unsafe { poll() };
    }
    if let Some(input) = input {
        let p = read_pad(input, 0);
        let rumble = core.machine.hw.si.pads[0].rumble;
        core.machine.hw.si.pads[0] = crate::hw::PadState { rumble, ..p };
    }
    let per_field = core.machine.cpu_hz / 60;
    core.machine.run_field(per_field);
    if let Some((w, h, px)) = core.machine.hw.vi_picture() {
        core.frame = px;
        core.size = (w, h);
    }
    if let Some(video) = video {
        let (w, h) = core.size;
        // SAFETY: `frame` holds w*h pixels of 4 bytes, and lives until the call returns.
        unsafe { video(core.frame.as_ptr() as *const c_void, w, h, w as usize * 4) };
    }
    if let Some(audio) = audio {
        let out = std::mem::take(&mut core.machine.hw.audio_out);
        if out.is_empty() {
            // SAFETY: a static buffer of FRAME_SAMPLES stereo frames.
            unsafe { audio(SILENCE.as_ptr(), FRAME_SAMPLES) };
        } else {
            // SAFETY: `out` holds out.len() / 2 interleaved stereo frames for the call.
            unsafe { audio(out.as_ptr(), out.len() / 2) };
        }
        core.machine.hw.audio_out = out;
        core.machine.hw.audio_out.clear();
    }
}

/// Room for the largest state: Wii memory plus ARAM plus registers.
const STATE_ROOM: usize = crate::hw::MEM1_SIZE + crate::hw::MEM2_SIZE + crate::hw::ARAM_SIZE + (8 << 20);
/// A GameCube has no MEM2: its states need less than half the room (and the app's save files and
/// rewind snapshots are this size).
const STATE_ROOM_GAMECUBE: usize = crate::hw::MEM1_SIZE + crate::hw::ARAM_SIZE + (8 << 20);

#[no_mangle]
pub extern "C" fn retro_serialize_size() -> usize {
    match core().as_ref() {
        Some(c) if !c.machine.hw.wii => STATE_ROOM_GAMECUBE,
        _ => STATE_ROOM,
    }
}

/// # Safety
/// `data` must point to `size` writable bytes.
#[no_mangle]
pub unsafe extern "C" fn retro_serialize(data: *mut c_void, size: usize) -> bool {
    let guard = core();
    let Some(core) = guard.as_ref() else { return false };
    let state = core.machine.save_state();
    if state.len() + 8 > size {
        return false;
    }
    let out = std::slice::from_raw_parts_mut(data as *mut u8, size);
    out[..8].copy_from_slice(&(state.len() as u64).to_le_bytes());
    out[8..8 + state.len()].copy_from_slice(&state);
    true
}

/// # Safety
/// `data` must point to `size` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn retro_unserialize(data: *const c_void, size: usize) -> bool {
    let mut guard = core();
    let Some(core) = guard.as_mut() else { return false };
    if size < 8 {
        return false;
    }
    let bytes = std::slice::from_raw_parts(data as *const u8, size);
    let len = u64::from_le_bytes(bytes[..8].try_into().unwrap()) as usize;
    if len + 8 > size {
        return false;
    }
    core.machine.load_state(&bytes[8..8 + len]).is_ok()
}

#[no_mangle]
pub extern "C" fn retro_cheat_reset() {}
#[no_mangle]
pub extern "C" fn retro_cheat_set(_index: u32, _enabled: bool, _code: *const c_char) {}
#[no_mangle]
pub extern "C" fn retro_get_region() -> u32 {
    0
}
/// `RETRO_MEMORY_SAVE_RAM`: the memory card in slot A (memcard.rs), which the frontend keeps as
/// the game's .srm file, one card per game.
const MEMORY_SAVE_RAM: u32 = 0;

/// The memory card's bytes. Valid until the game is unloaded: the card is never reallocated
/// while a game runs.
#[no_mangle]
pub extern "C" fn retro_get_memory_data(id: u32) -> *mut c_void {
    match core().as_mut() {
        Some(c) if id == MEMORY_SAVE_RAM && !c.machine.hw.card.is_empty() => c.machine.hw.card.as_mut_ptr() as *mut c_void,
        _ => std::ptr::null_mut(),
    }
}
#[no_mangle]
pub extern "C" fn retro_get_memory_size(id: u32) -> usize {
    match core().as_ref() {
        Some(c) if id == MEMORY_SAVE_RAM => c.machine.hw.card.len(),
        _ => 0,
    }
}

/// Now, as seconds since 1 January 2000 on the phone's clock in its own time zone (what a
/// console's clock reads when it was set by hand).
fn local_seconds_since_2000() -> u32 {
    const UNIX_2000: i64 = 946_684_800;
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
    (now + utc_offset_seconds(now) - UNIX_2000).clamp(0, u32::MAX as i64) as u32
}

/// The phone's time zone offset from UTC at `unix` seconds (daylight saving included).
// tm_gmtoff is a C long: 64 bits on the phone, 32 on some other systems.
#[allow(clippy::unnecessary_cast)]
fn utc_offset_seconds(unix: i64) -> i64 {
    let t = unix as libc::time_t;
    // SAFETY: localtime_r only writes the tm it is given.
    unsafe {
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&t, &mut tm).is_null() {
            return 0;
        }
        tm.tm_gmtoff as i64
    }
}
