//! cMini's libretro boundary: 96×64 video, stereo audio, controls, settings, EEPROM and states.

use crate::{
    machine::Machine,
    options::Settings,
    prc::{HEIGHT, WIDTH},
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
const STATE_SIZE: usize = 4 * 1024 * 1024;
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
    history: Vec<f32>,
    audio: Vec<i16>,
    frame_no: u64,
    cart_tag: u64,
}
#[derive(Serialize, Deserialize)]
struct State {
    magic: [u8; 8],
    version: u32,
    cart_tag: u64,
    machine: Machine,
    history: Vec<f32>,
    frame_no: u64,
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
unsafe fn bios(env: Env) -> Option<Vec<u8>> {
    let p = system_dir(env)?.join("bios.min");
    let b = std::fs::read(p).ok()?;
    (b.len() == 4096).then_some(b)
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
                sample_rate: s.sample_rate as f64,
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
    let cart_tag = tag(&rom);
    let Ok(m) = Machine::new(rom, bios(env), s.real_bios) else {
        return false;
    };
    let k = s.scale as usize;
    *CORE.lock().unwrap() = Some(Core {
        m,
        s,
        frame: vec![0; WIDTH * HEIGHT * k * k],
        history: vec![0.0; WIDTH * HEIGHT],
        audio: Vec::with_capacity(2048),
        frame_no: 0,
        cart_tag,
    });
    true
}
#[no_mangle]
pub extern "C" fn retro_unload_game() {
    if let Ok(mut c) = CORE.lock() {
        *c = None
    }
}
fn colour(p: u8, v: f32) -> u32 {
    let (v0, v1) = match p {
        1 => ([255., 255., 255.], [15., 15., 15.]),
        2 => ([210., 245., 190.], [20., 65., 25.]),
        3 => ([205., 235., 255.], [15., 30., 80.]),
        4 => ([255., 220., 210.], [85., 20., 15.]),
        5 => ([250., 235., 190.], [65., 45., 20.]),
        6 => ([255., 220., 240.], [80., 20., 60.]),
        _ => ([255., 251., 135.], [78., 78., 78.]),
    };
    let t = v.clamp(0., 1.);
    let r = (v0[0] + (v1[0] - v0[0]) * t) as u32;
    let g = (v0[1] + (v1[1] - v0[1]) * t) as u32;
    let b = (v0[2] + (v1[2] - v0[2]) * t) as u32;
    0xff000000 | r << 16 | g << 8 | b
}
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
            c.s = crate::options::read(env)
        }
    }
    if let Some(p) = poll {
        unsafe { p() }
    }
    let pressed = |id| input.map_or(false, |f| unsafe { f(0, JOY, 0, id) != 0 });
    let mut keys = 0xffu8;
    if pressed(8) {
        keys &= !1
    }
    if pressed(0) {
        keys &= !2
    }
    if pressed(11) {
        keys &= !4
    }
    if pressed(4) {
        keys &= !8
    }
    if pressed(5) {
        keys &= !16
    }
    if pressed(6) {
        keys &= !32
    }
    if pressed(7) {
        keys &= !64
    }
    if pressed(2) {
        keys &= !128
    }
    if pressed(9) && c.frame_no % (c.s.turbo.max(2) as u64) < (c.s.turbo.max(2) as u64) / 2 {
        keys &= !1
    }
    c.m.set_buttons(keys, pressed(10));
    c.m.infrared.enabled = c.s.infrared;
    c.audio.clear();
    c.m.run_frame(
        c.s.sample_rate,
        c.s.piezo,
        c.s.lowpass,
        c.s.volume,
        &mut c.audio,
    );
    if let Some(r) = rumble {
        let strength = if c.m.rumble {
            u16::MAX.saturating_mul(c.s.rumble as u16) / 10
        } else {
            0
        };
        unsafe {
            r(0, 0, strength);
            r(0, 1, strength);
        }
    }
    let k = c.s.scale as usize;
    if c.frame.len() != WIDTH * HEIGHT * k * k {
        c.frame.resize(WIDTH * HEIGHT * k * k, 0);
    }
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let now = c.m.prc.lcd[y * WIDTH + x] as f32;
            let old = c.history[y * WIDTH + x];
            let mut shade = match c.s.shades {
                2 => now,
                3 => (now * 2. + old).round() / 3.,
                _ => now * 0.72 + old * 0.28,
            };
            c.history[y * WIDTH + x] = shade;
            shade = ((shade * 128. + c.s.contrast as f32 - 64. + c.s.brightness as f32) / 128.)
                .clamp(0., 1.);
            for yy in 0..k {
                for xx in 0..k {
                    let cell = if c.s.filter == 1 && k > 1 && (xx == k - 1 || yy == k - 1) {
                        (shade * 0.72).min(1.)
                    } else if c.s.filter == 2 && yy == k - 1 {
                        (shade * 0.55).min(1.)
                    } else {
                        shade
                    };
                    c.frame[(y * k + yy) * WIDTH * k + x * k + xx] = colour(c.s.palette, cell)
                }
            }
        }
    }
    c.frame_no += 1;
    if c.m.rumble && c.s.shake > 0 {
        let shift = (c.s.shake as usize * k).min(c.frame.len().saturating_sub(1));
        c.frame.rotate_left(shift);
    }
    if let Some(a) = audio {
        unsafe {
            a(c.audio.as_ptr(), c.audio.len() / 2);
        }
    }
    if c.s.hz60 && c.frame_no % 6 == 0 {
        return;
    }
    if let Some(v) = video {
        unsafe {
            v(
                c.frame.as_ptr() as *const c_void,
                (WIDTH * k) as u32,
                (HEIGHT * k) as u32,
                WIDTH * k * 4,
            )
        }
    }
}
#[no_mangle]
pub extern "C" fn retro_reset() {
    if let Ok(mut c) = CORE.lock() {
        if let Some(c) = c.as_mut() {
            c.m.reset(c.s.real_bios);
            c.history.fill(0.0)
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
        version: 1,
        cart_tag: c.cart_tag,
        machine: c.m.clone(),
        history: c.history.clone(),
        frame_no: c.frame_no,
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
    if s.magic != *b"CMINISTA" || s.version != 1 {
        return false;
    }
    let mut c = match CORE.lock() {
        Ok(c) => c,
        Err(_) => return false,
    };
    let Some(c) = c.as_mut() else { return false };
    if s.cart_tag != c.cart_tag {
        return false;
    }
    c.m = s.machine;
    c.history = s.history;
    c.frame_no = s.frame_no;
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
        MEMORY_SAVE => c.m.eeprom.data.as_mut_ptr() as *mut c_void,
        MEMORY_SYSTEM => c.m.ram.as_mut_ptr() as *mut c_void,
        _ => std::ptr::null_mut(),
    }
}
#[no_mangle]
pub extern "C" fn retro_get_memory_size(id: u32) -> usize {
    match id {
        MEMORY_SAVE => 8192,
        MEMORY_SYSTEM => 4096,
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
