//! The libretro entry points: what the app calls. The name shown is cGear (the crate's working
//! name is continuum-sms; the owner asked not to be asked about names, so this one was picked and
//! noted in .kiro/notes/tasks/cboy-finish.json, and can be changed there).
//!
//! For now: Master System cartridges, two pads (the console's Pause button on Start), the
//! picture at the mode's own size, sound at 48 kHz, battery saves through the frontend, and save
//! states.

use std::ffi::{c_char, c_void};
use std::sync::Mutex;

use crate::sms::Sms;

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
const PIXEL_FORMAT_XRGB8888: u32 = 1;
const DEVICE_JOYPAD: u32 = 1;
const MEMORY_SAVE_RAM: u32 = 0;
const MEMORY_SYSTEM_RAM: u32 = 2;
const MEMORY_VIDEO_RAM: u32 = 3;
const RATE: u32 = 48_000;

struct Callbacks {
    env: Option<EnvFn>,
    video: Option<VideoFn>,
    audio_batch: Option<AudioBatchFn>,
    poll: Option<PollFn>,
    input: Option<InputFn>,
}

static CB: Mutex<Callbacks> = Mutex::new(Callbacks { env: None, video: None, audio_batch: None, poll: None, input: None });
static CORE: Mutex<Option<Box<Sms>>> = Mutex::new(None);

#[no_mangle]
pub extern "C" fn retro_api_version() -> u32 {
    1
}

#[no_mangle]
pub unsafe extern "C" fn retro_get_system_info(info: *mut RetroSystemInfo) {
    if info.is_null() {
        return;
    }
    *info = RetroSystemInfo {
        library_name: c"cGear".as_ptr(),
        library_version: c"0.1.0 (state format 1)".as_ptr(),
        valid_extensions: c"sms|gg|bin".as_ptr(),
        need_fullpath: false,
        block_extract: false,
    };
}

#[no_mangle]
pub unsafe extern "C" fn retro_get_system_av_info(info: *mut RetroSystemAvInfo) {
    if info.is_null() {
        return;
    }
    let (pal, gg) = CORE.lock().ok().and_then(|c| c.as_ref().map(|m| (m.bus.vdp.pal, m.bus.gg))).unwrap_or((false, false));
    // A Master System on a TV: 256 x 192 shown 4:3. A Game Gear's own screen: 160 x 144, 10:9.
    let (w, h, aspect) = if gg { (160, 144, 10.0 / 9.0) } else { (256, 192, 4.0 / 3.0) };
    *info = RetroSystemAvInfo {
        geometry: RetroGameGeometry { base_width: w, base_height: h, max_width: 256, max_height: 240, aspect_ratio: aspect },
        timing: RetroSystemTiming {
            fps: if pal { 3_546_895.0 / (228.0 * 313.0) } else { 3_579_545.0 / (228.0 * 262.0) },
            sample_rate: f64::from(RATE),
        },
    };
}

#[no_mangle]
pub extern "C" fn retro_set_environment(cb: EnvFn) {
    if let Ok(mut c) = CB.lock() {
        c.env = Some(cb);
    }
}
#[no_mangle]
pub extern "C" fn retro_set_video_refresh(cb: VideoFn) {
    if let Ok(mut c) = CB.lock() {
        c.video = Some(cb);
    }
}
#[no_mangle]
pub extern "C" fn retro_set_audio_sample(_cb: AudioFn) {}
#[no_mangle]
pub extern "C" fn retro_set_audio_sample_batch(cb: AudioBatchFn) {
    if let Ok(mut c) = CB.lock() {
        c.audio_batch = Some(cb);
    }
}
#[no_mangle]
pub extern "C" fn retro_set_input_poll(cb: PollFn) {
    if let Ok(mut c) = CB.lock() {
        c.poll = Some(cb);
    }
}
#[no_mangle]
pub extern "C" fn retro_set_input_state(cb: InputFn) {
    if let Ok(mut c) = CB.lock() {
        c.input = Some(cb);
    }
}
#[no_mangle]
pub extern "C" fn retro_init() {}
#[no_mangle]
pub extern "C" fn retro_deinit() {
    if let Ok(mut c) = CORE.lock() {
        *c = None;
    }
}
#[no_mangle]
pub extern "C" fn retro_set_controller_port_device(_port: u32, _device: u32) {}

#[no_mangle]
pub unsafe extern "C" fn retro_load_game(game: *const RetroGameInfo) -> bool {
    if game.is_null() || (*game).data.is_null() || (*game).size == 0 {
        return false;
    }
    let mut rom = std::slice::from_raw_parts((*game).data as *const u8, (*game).size).to_vec();
    // Some dumps carry a 512-byte copier header in front.
    if rom.len() % 0x4000 == 512 {
        rom.drain(..512);
    }
    let env = CB.lock().ok().and_then(|c| c.env);
    if let Some(env) = env {
        let mut fmt = PIXEL_FORMAT_XRGB8888;
        if !env(ENV_SET_PIXEL_FORMAT, &mut fmt as *mut u32 as *mut c_void) {
            return false;
        }
    }
    let gg = is_game_gear(&rom, (*game).path);
    let mut m = Box::new(Sms::new(rom));
    m.rate = RATE;
    if gg {
        m.bus.set_game_gear();
    }
    if let Ok(mut c) = CORE.lock() {
        *c = Some(m);
        return true;
    }
    false
}

#[no_mangle]
pub extern "C" fn retro_load_game_special(_t: u32, _info: *const RetroGameInfo, _n: usize) -> bool {
    false
}

#[no_mangle]
pub extern "C" fn retro_unload_game() {
    if let Ok(mut c) = CORE.lock() {
        *c = None;
    }
}

#[no_mangle]
pub extern "C" fn retro_get_region() -> u32 {
    CORE.lock().ok().and_then(|c| c.as_ref().map(|m| u32::from(m.bus.vdp.pal))).unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn retro_reset() {
    if let Ok(mut c) = CORE.lock() {
        if let Some(m) = c.as_mut() {
            let rom = std::mem::take(&mut m.bus.rom);
            let cart_ram = std::mem::take(&mut m.bus.cart_ram);
            let gg = m.bus.gg;
            let mut fresh = Sms::new(rom);
            if gg {
                fresh.bus.set_game_gear();
            }
            fresh.bus.cart_ram = cart_ram;
            fresh.rate = m.rate;
            **m = fresh;
        }
    }
}

/// A Game Gear cartridge: its file is a .gg one, or its header (at $7FF0, $3FF0 or $1FF0, after
/// "TMR SEGA") gives a Game Gear region code (5, 6 or 7 in the top half of its last byte).
unsafe fn is_game_gear(rom: &[u8], path: *const c_char) -> bool {
    if !path.is_null() {
        let p = std::ffi::CStr::from_ptr(path).to_string_lossy().to_lowercase();
        if p.ends_with(".gg") {
            return true;
        }
        if p.ends_with(".sms") {
            return false;
        }
    }
    [0x7FF0usize, 0x3FF0, 0x1FF0].iter().any(|&at| {
        rom.get(at..at + 16).is_some_and(|h| &h[..8] == b"TMR SEGA" && (5..=7).contains(&(h[15] >> 4)))
    })
}

/// The pads as the console's ports read them, from the two players' buttons: $DC holds the
/// first pad and the second's Up and Down, $DD the second's other buttons. A held button reads 0.
/// Start on either pad is the console's Pause button.
unsafe fn read_pads(input: InputFn) -> (u8, u8, bool) {
    let held = |port: u32, id: u32| input(port, DEVICE_JOYPAD, 0, id) != 0;
    // libretro: B 0, Y 1, Select 2, Start 3, Up 4, Down 5, Left 6, Right 7, A 8. Button 1 is B,
    // button 2 is A, as on a Master System pad held the usual way.
    let pad = |p: u32| -> u8 {
        let bits = [held(p, 4), held(p, 5), held(p, 6), held(p, 7), held(p, 0) || held(p, 1), held(p, 8)];
        bits.iter().enumerate().fold(0x3F, |v, (i, on)| if *on { v & !(1 << i) } else { v })
    };
    let (a, b) = (pad(0), pad(1));
    let dc = a | (b & 0x03) << 6;
    let dd = 0xF0 | (b >> 2);
    (dc, dd, held(0, 3) || held(1, 3))
}

#[no_mangle]
pub unsafe extern "C" fn retro_run() {
    let (video, audio, poll, input) = match CB.lock() {
        Ok(c) => (c.video, c.audio_batch, c.poll, c.input),
        Err(_) => return,
    };
    if let Some(poll) = poll {
        poll();
    }
    let Ok(mut slot) = CORE.lock() else { return };
    let Some(m) = slot.as_mut() else { return };
    if let Some(input) = input {
        let (dc, dd, pause) = read_pads(input);
        m.bus.pad_dc = dc;
        m.bus.pad_dd = dd;
        if m.bus.gg {
            // The Game Gear's Start is a button the game reads; it has no Pause.
            m.bus.gg_start = pause;
        } else {
            m.set_pause(pause);
        }
    }
    m.run_frame();
    if let Some(video) = video {
        let mut shown = std::mem::take(&mut m.shown);
        let (w, h) = m.bus.vdp.shown(&mut shown);
        video(shown.as_ptr() as *const c_void, w as u32, h as u32, w * 4);
        m.shown = shown;
    }
    let samples = std::mem::take(&mut m.samples);
    if let Some(audio) = audio {
        let mut off = 0;
        while off < samples.len() / 2 {
            let n = audio(samples[off * 2..].as_ptr(), samples.len() / 2 - off);
            if n == 0 {
                break;
            }
            off += n;
        }
    }
    m.samples = samples;
    m.samples.clear();
}

#[no_mangle]
pub extern "C" fn retro_serialize_size() -> usize {
    CORE.lock().ok().and_then(|c| c.as_ref().map(|m| m.save_state().len() + 1024)).unwrap_or(0)
}

#[no_mangle]
pub unsafe extern "C" fn retro_serialize(data: *mut c_void, size: usize) -> bool {
    let Ok(slot) = CORE.lock() else { return false };
    let Some(m) = slot.as_ref() else { return false };
    let state = m.save_state();
    if data.is_null() || state.len() > size {
        return false;
    }
    let out = std::slice::from_raw_parts_mut(data as *mut u8, size);
    out[..state.len()].copy_from_slice(&state);
    out[state.len()..].fill(0);
    true
}

#[no_mangle]
pub unsafe extern "C" fn retro_unserialize(data: *const c_void, size: usize) -> bool {
    if data.is_null() {
        return false;
    }
    let bytes = std::slice::from_raw_parts(data as *const u8, size);
    let Ok(mut slot) = CORE.lock() else { return false };
    let Some(m) = slot.as_mut() else { return false };
    m.load_state(bytes).is_ok()
}

#[no_mangle]
pub extern "C" fn retro_cheat_reset() {}
#[no_mangle]
pub extern "C" fn retro_cheat_set(_index: u32, _enabled: bool, _code: *const c_char) {}

#[no_mangle]
pub extern "C" fn retro_get_memory_data(id: u32) -> *mut c_void {
    let Ok(mut slot) = CORE.lock() else { return std::ptr::null_mut() };
    let Some(m) = slot.as_mut() else { return std::ptr::null_mut() };
    match id {
        MEMORY_SAVE_RAM => m.bus.cart_ram.as_mut_ptr() as *mut c_void,
        MEMORY_SYSTEM_RAM => m.bus.ram.as_mut_ptr() as *mut c_void,
        MEMORY_VIDEO_RAM => m.bus.vdp.vram.as_mut_ptr() as *mut c_void,
        _ => std::ptr::null_mut(),
    }
}

#[no_mangle]
pub extern "C" fn retro_get_memory_size(id: u32) -> usize {
    let Ok(slot) = CORE.lock() else { return 0 };
    let Some(m) = slot.as_ref() else { return 0 };
    match id {
        MEMORY_SAVE_RAM => m.bus.cart_ram.len(),
        MEMORY_SYSTEM_RAM => m.bus.ram.len(),
        MEMORY_VIDEO_RAM => m.bus.vdp.vram.len(),
        _ => 0,
    }
}
