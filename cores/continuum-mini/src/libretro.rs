//! cMini's libretro boundary, behaving as PokeMini's libretro core does (the emulator cMini
//! replaces): 96×64 video at 72 Hz (or 60, by running six frames to every five), 44100 Hz sound
//! in both channels, the same buttons and turbo, the screen shake and rumble the motor drives,
//! the console's clock set from the phone's local time at every start, reset and loaded state,
//! the save chip's 8 KiB as save RAM, and RAM with the I/O registers (8 KiB, laid out as
//! PokeMini lays it out) as system RAM for achievements.

use crate::{
    audio::{LowPass, RATE},
    lcd::{self, HEIGHT, WIDTH},
    machine::{Machine, BIOS_SIZE, MEMORY},
    options::Settings,
    ports::EEPROM_SIZE,
};
use serde::{Deserialize, Serialize};
use std::ffi::{c_char, c_void, CStr};
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
#[repr(C)]
struct InputDesc {
    port: u32,
    device: u32,
    index: u32,
    id: u32,
    description: *const c_char,
}
#[repr(C)]
struct Rumble {
    set: Option<unsafe extern "C" fn(u32, u32, u16) -> bool>,
}
type Env = unsafe extern "C" fn(u32, *mut c_void) -> bool;
type Video = unsafe extern "C" fn(*const c_void, u32, u32, usize);
type Audio = unsafe extern "C" fn(i16, i16);
type AudioBatch = unsafe extern "C" fn(*const i16, usize) -> usize;
type Poll = unsafe extern "C" fn();
type Input = unsafe extern "C" fn(u32, u32, u32, u32) -> i16;
const GET_SYSTEM: u32 = 9;
const SET_PIXEL: u32 = 10;
const SET_INPUT: u32 = 11;
const GET_RUMBLE: u32 = 23;
const PIXEL_XRGB: u32 = 1;
const JOY: u32 = 1;
const MEMORY_SAVE: u32 = 0;
const MEMORY_SYSTEM: u32 = 2;
/// A saved state's room: the machine without its cartridge comes to well under this.
const STATE_SIZE: usize = 256 * 1024;
const STATE_VERSION: u32 = 2;
struct Cb {
    env: Option<Env>,
    video: Option<Video>,
    audio: Option<AudioBatch>,
    poll: Option<Poll>,
    input: Option<Input>,
    rumble: Option<unsafe extern "C" fn(u32, u32, u16) -> bool>,
}
static CB: Mutex<Cb> = Mutex::new(Cb {
    env: None,
    video: None,
    audio: None,
    poll: None,
    input: None,
    rumble: None,
});
static CORE: Mutex<Option<Core>> = Mutex::new(None);
struct Core {
    m: Machine,
    s: Settings,
    frame: Vec<u32>,
    frame_no: u64,
    cart_tag: u64,
    /// The phone's time goes to the console's clock before the next frame.
    clock_due: bool,
    turbo: u16,
    /// 60 Hz: the frame of the six-to-five cycle, and the sound waiting to go out.
    hz60_count: u8,
    pending: Vec<i16>,
    low_pass: LowPass,
    stereo: Vec<i16>,
    /// The motor's strength last sent, and the shake's place in its pattern.
    rumble_sent: u16,
    shake_step: usize,
}
#[derive(Serialize, Deserialize)]
struct State {
    magic: [u8; 8],
    version: u32,
    cart_tag: u64,
    machine: Machine,
    frame_no: u64,
    turbo: u16,
    hz60_count: u8,
    low_pass: LowPass,
    shake_step: usize,
}
fn tag(b: &[u8]) -> u64 {
    let mut h = 0xcbf29ce484222325u64;
    for &x in b {
        h = (h ^ x as u64).wrapping_mul(0x100000001b3)
    }
    h
}
unsafe fn system_dir(env: Env) -> Option<std::path::PathBuf> {
    let mut p: *const c_char = std::ptr::null();
    if env(GET_SYSTEM, &mut p as *mut _ as *mut c_void) && !p.is_null() {
        Some(CStr::from_ptr(p).to_string_lossy().into_owned().into())
    } else {
        None
    }
}
/// The player's own start-up program, when there is a whole one.
unsafe fn players_bios(env: Env) -> Option<Vec<u8>> {
    let p = system_dir(env)?.join("bios.min");
    let b = std::fs::read(p).ok()?;
    (b.len() == BIOS_SIZE).then_some(b)
}

/// The phone's local time as the console's clock keeps it: year within the century, month,
/// day, hour, minute, second.
fn local_time() -> Option<[u8; 6]> {
    #[repr(C)]
    struct Tm {
        sec: i32,
        min: i32,
        hour: i32,
        mday: i32,
        mon: i32,
        year: i32,
        wday: i32,
        yday: i32,
        isdst: i32,
        gmtoff: i64,
        zone: *const c_char,
    }
    extern "C" {
        fn time(t: *mut i64) -> i64;
        fn localtime_r(t: *const i64, out: *mut Tm) -> *mut Tm;
    }
    let mut tm = Tm { sec: 0, min: 0, hour: 0, mday: 0, mon: 0, year: 0, wday: 0, yday: 0, isdst: 0, gmtoff: 0, zone: std::ptr::null() };
    // SAFETY: both are the C library's, called with valid pointers.
    let ok = unsafe {
        let now = time(std::ptr::null_mut());
        !localtime_r(&now, &mut tm).is_null()
    };
    ok.then(|| [(tm.year % 100) as u8, (tm.mon + 1) as u8, tm.mday as u8, tm.hour as u8, tm.min as u8, tm.sec as u8])
}
#[no_mangle]
pub extern "C" fn retro_api_version() -> u32 {
    1
}
#[no_mangle]
pub unsafe extern "C" fn retro_get_system_info(p: *mut RetroSystemInfo) {
    if let Some(p) = p.as_mut() {
        *p = RetroSystemInfo {
            library_name: c"cMini".as_ptr(),
            library_version: c"0.1.0 (state format 1)".as_ptr(),
            valid_extensions: c"min".as_ptr(),
            need_fullpath: false,
            block_extract: false,
        }
    }
}
#[no_mangle]
pub unsafe extern "C" fn retro_get_system_av_info(p: *mut RetroSystemAvInfo) {
    if let Some(p) = p.as_mut() {
        let s = CORE
            .lock()
            .ok()
            .and_then(|c| c.as_ref().map(|c| c.s.clone()))
            .unwrap_or_default();
        let k = s.scale as u32;
        *p = RetroSystemAvInfo {
            geometry: RetroGameGeometry {
                base_width: 96 * k,
                base_height: 64 * k,
                max_width: 576,
                max_height: 384,
                aspect_ratio: 1.5,
            },
            timing: RetroSystemTiming {
                fps: if s.hz60 { 60.0 } else { 72.0 },
                sample_rate: f64::from(RATE),
            },
        }
    }
}
#[no_mangle]
pub extern "C" fn retro_set_environment(e: Env) {
    if let Ok(mut c) = CB.lock() {
        c.env = Some(e)
    }
    unsafe {
        crate::options::declare(e);
        let d = [
            InputDesc {
                port: 0,
                device: JOY,
                index: 0,
                id: 0,
                description: c"B".as_ptr(),
            },
            InputDesc {
                port: 0,
                device: JOY,
                index: 0,
                id: 8,
                description: c"A".as_ptr(),
            },
            InputDesc {
                port: 0,
                device: JOY,
                index: 0,
                id: 9,
                description: c"Turbo A".as_ptr(),
            },
            InputDesc {
                port: 0,
                device: JOY,
                index: 0,
                id: 10,
                description: c"Shake".as_ptr(),
            },
            InputDesc {
                port: 0,
                device: JOY,
                index: 0,
                id: 11,
                description: c"C".as_ptr(),
            },
            InputDesc {
                port: 0,
                device: JOY,
                index: 0,
                id: 2,
                description: c"Power".as_ptr(),
            },
            InputDesc {
                port: 0,
                device: 0,
                index: 0,
                id: 0,
                description: std::ptr::null(),
            },
        ];
        e(SET_INPUT, d.as_ptr() as *mut c_void);
        let mut r = Rumble { set: None };
        if e(GET_RUMBLE, &mut r as *mut _ as *mut c_void) {
            if let Ok(mut c) = CB.lock() {
                c.rumble = r.set
            }
        }
    }
}
#[no_mangle]
pub extern "C" fn retro_set_video_refresh(f: Video) {
    if let Ok(mut c) = CB.lock() {
        c.video = Some(f)
    }
}
#[no_mangle]
pub extern "C" fn retro_set_audio_sample(_: Audio) {}
#[no_mangle]
pub extern "C" fn retro_set_audio_sample_batch(f: AudioBatch) {
    if let Ok(mut c) = CB.lock() {
        c.audio = Some(f)
    }
}
#[no_mangle]
pub extern "C" fn retro_set_input_poll(f: Poll) {
    if let Ok(mut c) = CB.lock() {
        c.poll = Some(f)
    }
}
#[no_mangle]
pub extern "C" fn retro_set_input_state(f: Input) {
    if let Ok(mut c) = CB.lock() {
        c.input = Some(f)
    }
}
#[no_mangle]
pub extern "C" fn retro_init() {}
#[no_mangle]
pub extern "C" fn retro_deinit() {
    if let Ok(mut c) = CORE.lock() {
        *c = None
    }
}
#[no_mangle]
pub unsafe extern "C" fn retro_load_game(g: *const RetroGameInfo) -> bool {
    let Some(g) = g.as_ref() else { return false };
    let cb = match CB.lock() {
        Ok(c) => c,
        Err(_) => return false,
    };
    let Some(env) = cb.env else { return false };
    let mut fmt = PIXEL_XRGB;
    if !env(SET_PIXEL, &mut fmt as *mut _ as *mut c_void) {
        return false;
    }
    let rom = if !g.data.is_null() && g.size > 0 {
        std::slice::from_raw_parts(g.data as *const u8, g.size).to_vec()
    } else if !g.path.is_null() {
        match std::fs::read(CStr::from_ptr(g.path).to_string_lossy().as_ref()) {
            Ok(b) => b,
            Err(_) => return false,
        }
    } else {
        return false;
    };
    let s = crate::options::read(env);
    let bios = if s.real_bios { players_bios(env) } else { None }.unwrap_or_else(crate::bios::image);
    let Ok(mut m) = Machine::new(&rom, bios) else {
        return false;
    };
    apply(&mut m, &s);
    let k = s.scale as usize;
    *CORE.lock().unwrap() = Some(Core {
        m,
        s,
        frame: vec![0; WIDTH * HEIGHT * k * k],
        frame_no: 0,
        cart_tag: tag(&rom),
        clock_due: true,
        turbo: 0,
        hz60_count: 0,
        pending: Vec::with_capacity(4096),
        low_pass: LowPass::default(),
        stereo: Vec::with_capacity(4096),
        rumble_sent: 0,
        shake_step: 0,
    });
    true
}
#[no_mangle]
pub extern "C" fn retro_unload_game() {
    if let Ok(mut c) = CORE.lock() {
        *c = None
    }
}
/// The settings the machine itself follows.
fn apply(m: &mut Machine, s: &Settings) {
    m.lcd_mode = match s.shades {
        2 => lcd::TWO_SHADES,
        3 => lcd::THREE_SHADES,
        _ => lcd::ANALOG,
    };
    m.audio.piezo = s.piezo;
    m.ports.infrared.connected = false;
}
/// The panel's colours: unlit and lit.
fn palette(p: u8) -> ([i32; 3], [i32; 3]) {
    match p {
        1 => ([255, 255, 255], [15, 15, 15]),
        2 => ([210, 245, 190], [20, 65, 25]),
        3 => ([205, 235, 255], [15, 30, 80]),
        4 => ([255, 220, 210], [85, 20, 15]),
        5 => ([250, 235, 190], [65, 45, 20]),
        6 => ([255, 220, 240], [80, 20, 60]),
        _ => ([255, 251, 135], [78, 78, 78]),
    }
}
/// A pixel's colour from its darkness (0-255), the contrast setting deepening the middle shades
/// and the brightness one moving them all, as PokeMini's do.
fn colour(light: [i32; 3], dark: [i32; 3], darkness: i32, contrast: i32, brightness: i32) -> u32 {
    let v = (darkness - brightness * 255 / 100).clamp(0, 255);
    let strength = contrast * 255 / 100;
    let curve = 255 - (255 - v) * (255 - v) * (255 - v) / 65_536;
    let t = ((255 - strength) * v + strength * curve) >> 8;
    let mix = |k: usize| (((255 - t) * light[k] + t * dark[k]) >> 8) as u32;
    0xff00_0000 | mix(0) << 16 | mix(1) << 8 | mix(2)
}
/// PokeMini's screen shake: the rows the picture moves by, frame after frame, at each level.
const SHAKE: [[i32; 4]; 4] = [[0, 0, 0, 0], [-1, 0, 1, 0], [-1, 1, -1, 1], [-2, 1, -1, 2]];
/// The 72 Hz frames' share of a 60 Hz frame's sound.
const HZ60_SAMPLES: usize = (RATE as f64 / 60.0) as usize;
#[no_mangle]
pub extern "C" fn retro_run() {
    let (cb_env, video, audio, poll, input, rumble) = match CB.lock() {
        Ok(c) => (c.env, c.video, c.audio, c.poll, c.input, c.rumble),
        Err(_) => return,
    };
    let Some(env) = cb_env else { return };
    let mut lock = match CORE.lock() {
        Ok(c) => c,
        Err(_) => return,
    };
    let Some(c) = lock.as_mut() else { return };
    unsafe {
        if crate::options::changed(env) {
            let was_60 = c.s.hz60;
            c.s = crate::options::read(env);
            apply(&mut c.m, &c.s);
            if was_60 != c.s.hz60 {
                c.hz60_count = 0;
                c.pending.clear();
            }
        }
    }
    if c.clock_due {
        if let Some(t) = local_time() {
            c.m.set_clock(t);
        }
        c.clock_due = false;
    }
    if let Some(p) = poll {
        unsafe { p() }
    }
    let pressed = |id| input.map_or(false, |f| unsafe { f(0, JOY, 0, id) != 0 });
    let mut held = 0u8;
    for (id, bit) in [(8, 0x01), (0, 0x02), (11, 0x04), (4, 0x08), (5, 0x10), (6, 0x20), (7, 0x40), (2, 0x80)] {
        if pressed(id) {
            held |= bit;
        }
    }
    // Turbo A: held for half the period (2 to 18 frames), let go for the rest.
    if pressed(9) {
        let period = u16::from(c.s.turbo).clamp(4, 120);
        let width = (period >> 1).clamp(2, 18);
        if c.turbo < width {
            held |= 0x01;
        }
        c.turbo += 1;
        if c.turbo >= period {
            c.turbo = 0;
        }
    } else {
        c.turbo = 0;
    }
    c.m.set_buttons(held, pressed(10));
    // The frames: one, or at 60 Hz an extra one every fifth call.
    let runs = if c.s.hz60 && c.hz60_count == 0 { 2 } else { 1 };
    for _ in 0..runs {
        c.m.run_frame();
        c.pending.extend_from_slice(&c.m.audio.samples);
    }
    let out = if c.s.hz60 {
        c.hz60_count += 1;
        if c.hz60_count >= 5 {
            c.hz60_count = 0;
            c.pending.len()
        } else {
            HZ60_SAMPLES.min(c.pending.len())
        }
    } else {
        c.pending.len()
    };
    let mut mono: Vec<i16> = c.pending.drain(..out).collect();
    if c.s.lowpass != 0 {
        c.low_pass.run(&mut mono, c.s.lowpass);
    }
    c.stereo.clear();
    for &s in &mono {
        c.stereo.extend_from_slice(&[s, s]);
    }
    // The motor: the controller's rumble and the screen shake follow it.
    let rumbling = c.m.rumbling();
    if let Some(r) = rumble {
        let strength = if rumbling && c.s.rumble > 0 { 0x1999 * u16::from(c.s.rumble.min(10)) + 5 } else { 0 };
        if strength != c.rumble_sent {
            unsafe {
                r(0, 0, strength);
                r(0, 1, strength);
            }
            c.rumble_sent = strength;
        }
    }
    let k = c.s.scale as usize;
    let (w, h) = (WIDTH * k, HEIGHT * k);
    if c.frame.len() != w * h {
        c.frame.resize(w * h, 0);
    }
    let (light, dark) = palette(c.s.palette);
    let (contrast, brightness) = (i32::from(c.s.contrast), i32::from(c.s.brightness));
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let d = i32::from(c.m.lcd.darkness(c.m.lcd_mode, y * WIDTH + x));
            for yy in 0..k {
                for xx in 0..k {
                    // The cells' edges (dot matrix) or the lines' (scanlines) show lighter.
                    let edge = (c.s.filter == 1 && k > 1 && (xx == k - 1 || yy == k - 1)) || (c.s.filter == 2 && k > 1 && yy == k - 1);
                    let level = if edge { (d * if c.s.filter == 1 { 184 } else { 141 }) >> 8 } else { d };
                    c.frame[(y * k + yy) * w + x * k + xx] = colour(light, dark, level, contrast, brightness);
                }
            }
        }
    }
    if rumbling && c.s.shake > 0 {
        let rows = SHAKE[usize::from(c.s.shake.min(3))][c.shake_step & 3] * k as i32;
        c.shake_step = c.shake_step.wrapping_add(1);
        let by = rows.unsigned_abs() as usize * w;
        if by > 0 && by < c.frame.len() {
            let n = c.frame.len();
            if rows > 0 {
                c.frame.copy_within(..n - by, by);
                c.frame[..by].fill(0xff00_0000);
            } else {
                c.frame.copy_within(by.., 0);
                c.frame[n - by..].fill(0xff00_0000);
            }
        }
    }
    c.frame_no += 1;
    if let Some(v) = video {
        unsafe { v(c.frame.as_ptr() as *const c_void, w as u32, h as u32, w * 4) }
    }
    if let Some(a) = audio {
        unsafe {
            a(c.stereo.as_ptr(), c.stereo.len() / 2);
        }
    }
}
#[no_mangle]
pub extern "C" fn retro_reset() {
    if let Ok(mut c) = CORE.lock() {
        if let Some(c) = c.as_mut() {
            c.m.reset();
            c.low_pass.reset();
            c.clock_due = true;
        }
    }
}
#[no_mangle]
pub extern "C" fn retro_serialize_size() -> usize {
    STATE_SIZE
}
#[no_mangle]
pub unsafe extern "C" fn retro_serialize(data: *mut c_void, size: usize) -> bool {
    if data.is_null() || size < STATE_SIZE {
        return false;
    }
    let c = match CORE.lock() {
        Ok(c) => c,
        Err(_) => return false,
    };
    let Some(c) = c.as_ref() else { return false };
    let s = State {
        magic: *b"CMINISTA",
        version: STATE_VERSION,
        cart_tag: c.cart_tag,
        machine: c.m.clone(),
        frame_no: c.frame_no,
        turbo: c.turbo,
        hz60_count: c.hz60_count,
        low_pass: c.low_pass.clone(),
        shake_step: c.shake_step,
    };
    let Ok(bytes) = bincode::serialize(&s) else {
        return false;
    };
    if bytes.len() + 4 > size {
        return false;
    }
    let out = std::slice::from_raw_parts_mut(data as *mut u8, size);
    out.fill(0);
    out[..4].copy_from_slice(&(bytes.len() as u32).to_le_bytes());
    out[4..4 + bytes.len()].copy_from_slice(&bytes);
    true
}
#[no_mangle]
pub unsafe extern "C" fn retro_unserialize(data: *const c_void, size: usize) -> bool {
    if data.is_null() || size < 4 {
        return false;
    }
    let input = std::slice::from_raw_parts(data as *const u8, size);
    let n = u32::from_le_bytes(input[..4].try_into().unwrap()) as usize;
    if n == 0 || n > size - 4 {
        return false;
    }
    let Ok(s): Result<State, _> = bincode::deserialize(&input[4..4 + n]) else {
        return false;
    };
    if s.magic != *b"CMINISTA" || s.version != STATE_VERSION {
        return false;
    }
    let mut c = match CORE.lock() {
        Ok(c) => c,
        Err(_) => return false,
    };
    let Some(c) = c.as_mut() else { return false };
    if s.cart_tag != c.cart_tag || !s.machine.consistent() {
        return false;
    }
    c.m.take_state(s.machine);
    apply(&mut c.m, &c.s);
    c.frame_no = s.frame_no;
    c.turbo = s.turbo;
    c.hz60_count = s.hz60_count;
    c.low_pass = s.low_pass;
    c.shake_step = s.shake_step;
    c.pending.clear();
    // As PokeMini does after a state: the console's clock is the phone's again.
    c.clock_due = true;
    true
}
#[no_mangle]
pub extern "C" fn retro_get_memory_data(id: u32) -> *mut c_void {
    let mut c = match CORE.lock() {
        Ok(c) => c,
        Err(_) => return std::ptr::null_mut(),
    };
    let Some(c) = c.as_mut() else {
        return std::ptr::null_mut();
    };
    match id {
        MEMORY_SAVE => c.m.ports.eeprom.as_mut_ptr() as *mut c_void,
        MEMORY_SYSTEM => c.m.mem.as_mut_ptr() as *mut c_void,
        _ => std::ptr::null_mut(),
    }
}
#[no_mangle]
pub extern "C" fn retro_get_memory_size(id: u32) -> usize {
    match id {
        MEMORY_SAVE => EEPROM_SIZE,
        MEMORY_SYSTEM => MEMORY,
        _ => 0,
    }
}
#[no_mangle]
pub extern "C" fn retro_cheat_reset() {}
#[no_mangle]
pub unsafe extern "C" fn retro_cheat_set(_: u32, _: bool, _: *const c_char) {}
#[no_mangle]
pub extern "C" fn retro_set_controller_port_device(_: u32, _: u32) {}
#[no_mangle]
pub extern "C" fn retro_load_game_special(_: u32, _: *const RetroGameInfo, _: usize) -> bool {
    false
}
#[no_mangle]
pub extern "C" fn retro_get_region() -> u32 {
    0
}
