//! The libretro entry points: what the app calls. The name shown is cGear (the crate's working
//! name is continuum-sms; the owner asked not to be asked about names, so this one was picked and
//! noted in .kiro/notes/tasks/cboy-finish.json, and can be changed there).
//!
//! For now: Master System cartridges, two pads (the console's Pause button on Start), the
//! picture at the mode's own size, sound at 48 kHz, battery saves through the frontend, and save
//! states.

use std::ffi::{c_char, c_void};
use std::sync::Mutex;

use crate::options::{Console, Settings};
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
const DEVICE_ANALOG: u32 = 5;
const DEVICE_POINTER: u32 = 6;
const MEMORY_SAVE_RAM: u32 = 0;
const MEMORY_SYSTEM_RAM: u32 = 2;
const MEMORY_VIDEO_RAM: u32 = 3;
const RATE: u32 = 48_000;
const ENV_GET_SAVE_DIRECTORY: u32 = 31;
/// How often the second Game Gear's save is written when its game has changed it: every two
/// seconds, so at most that much is lost if the phone stops the app.
const SECOND_SAVE_FRAMES: u32 = 120;
const DUAL_MAGIC: &[u8; 8] = b"CGEARDU1";

struct Callbacks {
    env: Option<EnvFn>,
    video: Option<VideoFn>,
    audio_batch: Option<AudioBatchFn>,
    poll: Option<PollFn>,
    input: Option<InputFn>,
}

static CB: Mutex<Callbacks> = Mutex::new(Callbacks { env: None, video: None, audio_batch: None, poll: None, input: None });
static CORE: Mutex<Option<Core>> = Mutex::new(None);

/// The machine and its settings: `s` as the player has them now, `at_start` as they were when
/// the game opened (the ones that say "next start" keep those until it is opened again).
struct Core {
    m: Box<Sms>,
    s: Settings,
    at_start: Settings,
    /// The last picture shown, for the Game Gear's ghosting.
    prev: Vec<u32>,
    /// The sound filter's last output, left and right.
    lp: [f32; 2],
    /// The equaliser's two filters (880 Hz and 5 kHz), left and right.
    eq: [[f32; 2]; 2],
    /// The size of the last picture handed over (`m.shown`).
    shown_size: (usize, usize),
    /// The second of two Game Gears on one phone (link.rs). The app knows nothing about it, so it
    /// keeps its own picture, sound and save here.
    second: Option<Box<Second>>,
}

struct Second {
    m: Box<Sms>,
    picture: Vec<u32>,
    /// Where its cartridge save lives (beside the app's saves, named after the game), and what
    /// is in that file, so it is only written when the game changed it.
    save: Option<std::path::PathBuf>,
    written: Vec<u8>,
    frames: u32,
}

impl Second {
    fn write_save(&mut self) {
        let Some(path) = self.save.as_ref() else { return };
        if !self.m.bus.cart_ram_used || self.m.bus.cart_ram == self.written {
            return;
        }
        if std::fs::write(path, &self.m.bus.cart_ram).is_ok() {
            self.written.clone_from(&self.m.bus.cart_ram);
        }
    }
}

impl Drop for Second {
    fn drop(&mut self) {
        self.write_save();
    }
}

/// The second Game Gear's save file: the game's own name, in the app's save folder.
unsafe fn second_save_path(env: EnvFn, game_path: *const c_char) -> Option<std::path::PathBuf> {
    let mut dir: *const c_char = std::ptr::null();
    if !env(ENV_GET_SAVE_DIRECTORY, &mut dir as *mut *const c_char as *mut c_void) || dir.is_null() {
        return None;
    }
    let dir = std::path::PathBuf::from(std::ffi::CStr::from_ptr(dir).to_string_lossy().into_owned());
    let stem = (!game_path.is_null())
        .then(|| std::path::PathBuf::from(std::ffi::CStr::from_ptr(game_path).to_string_lossy().into_owned()))
        .and_then(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned()));
    Some(dir.join(format!("{} (second Game Gear).sav", stem.unwrap_or_else(|| "game".into()))))
}

impl std::ops::Deref for Core {
    type Target = Sms;
    fn deref(&self) -> &Sms {
        &self.m
    }
}

impl std::ops::DerefMut for Core {
    fn deref_mut(&mut self) -> &mut Sms {
        &mut self.m
    }
}

impl Core {
    /// The settings that apply to a running game.
    fn apply(&mut self) {
        let s = self.s.clone();
        self.m.bus.vdp.sprite_limit = s.sprite_limit;
        self.m.bus.psg.chan_vol = s.chan_vol;
        self.m.bus.psg.volume = s.volume;
        self.m.bus.fm.volume = s.fm_volume;
        self.m.bus.fm.chan_vol = s.fm_chan_vol;
        self.m.cpu_percent = s.cpu_percent;
        for (port, device) in self.m.bus.ports.iter_mut().zip(s.ports) {
            port.device = device;
            port.speed = s.device_speed;
        }
        self.m.bus.vdp.sms_tms_colours = s.tms_colours_sms.unwrap_or(!self.m.bus.sg);
        self.m.bus.vdp.tms_from_cram = s.tms_colours_sms.is_none() && self.m.bus.gg;
        self.m.bus.vdp.gg_full = self.at_start.gg_full;
        self.m.bus.vdp.borders = self.at_start.borders;
        if let Some(two) = self.second.as_mut() {
            let b = &mut two.m.bus;
            b.vdp.sprite_limit = s.sprite_limit;
            b.psg.chan_vol = s.chan_vol;
            b.psg.volume = s.volume;
            b.vdp.tms_from_cram = s.tms_colours_sms.is_none();
            two.m.cpu_percent = s.cpu_percent;
        }
    }

    /// The picture size the app was told: two Game Gears side by side or stacked when both show.
    fn dual_size(&self) -> Option<(u32, u32)> {
        let _ = self.second.as_ref()?;
        Some(match (self.at_start.gg_link_show, self.at_start.gg_link_side) {
            (0, true) => (320, 144),
            (0, false) => (160, 288),
            _ => (160, 144),
        })
    }
}

/// A new machine for `rom` as the settings ask.
unsafe fn machine(rom: Vec<u8>, s: &Settings, path: *const c_char) -> Box<Sms> {
    let console = match s.console {
        Console::Auto if is_game_gear(&rom, path) => Console::GameGear,
        Console::Auto if has_extension(path, &[".sg", ".sc", ".mv"]) => Console::Sg1000,
        Console::Auto => Console::Sms,
        c => c,
    };
    let mut m = Box::new(Sms::new(rom));
    m.rate = RATE;
    match console {
        Console::GameGear => m.bus.set_game_gear(),
        Console::Sg1000 => m.bus.set_sg1000(),
        _ => {}
    }
    let japan = if s.region_auto { is_japanese(&m.bus.rom, path) } else { s.japan };
    m.bus.export = !japan;
    // Only a Master System ever had the FM chip.
    m.bus.has_fm = matches!(console, Console::Sms | Console::Sms1) && s.fm.unwrap_or(japan);
    m.bus.vdp.sms1 = console == Console::Sms1;
    let pal = if s.refresh_auto { is_european(path) } else { s.pal };
    m.bus.vdp.pal = pal && console != Console::GameGear;
    if let Some(mapper) = s.mapper {
        m.bus.mapper = mapper;
        m.bus.banks = if mapper == crate::bus::Mapper::Codemasters { [0, 1, 0] } else { [0, 1, 2] };
    }
    m
}

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
        valid_extensions: c"sms|gg|sg|sc|mv|bin".as_ptr(),
        need_fullpath: false,
        block_extract: false,
    };
}

#[no_mangle]
pub unsafe extern "C" fn retro_get_system_av_info(info: *mut RetroSystemAvInfo) {
    if info.is_null() {
        return;
    }
    let dual = CORE.lock().ok().and_then(|c| c.as_ref().and_then(Core::dual_size));
    if let Some((w, h)) = dual {
        *info = RetroSystemAvInfo {
            geometry: RetroGameGeometry { base_width: w, base_height: h, max_width: 320, max_height: 294, aspect_ratio: w as f32 / h as f32 },
            timing: RetroSystemTiming { fps: 3_579_545.0 / (228.0 * 262.0), sample_rate: f64::from(RATE) },
        };
        return;
    }
    let (pal, gg, cut, borders) = CORE
        .lock()
        .ok()
        .and_then(|c| c.as_ref().map(|m| {
            let (t, b) = m.bus.vdp.border_lines();
            let sides = if m.bus.vdp.borders == 2 { 13 + 15 } else { 0 };
            let tb = if m.bus.vdp.borders > 0 && !m.bus.gg { Some((sides, t + b)) } else { None };
            (m.bus.vdp.pal, m.bus.gg && !m.at_start.gg_full, m.at_start.hide_left && tb.is_none(), tb)
        }))
        .unwrap_or((false, false, false, None));
    // A Master System on a TV: 256 x 192 shown 4:3 (248 wide without its left column), and with
    // the TV's border round it at the same shape of dot. A Game Gear's own screen: 160 x 144, 10:9.
    let (w, h, aspect) = if gg {
        (160, 144, 10.0 / 9.0)
    } else if let Some((sides, tb)) = borders {
        let (w, h) = (256 + sides as u32, 192 + tb as u32);
        (w, h, w as f32 / h as f32)
    } else if cut {
        (248, 192, 4.0 / 3.0 * 248.0 / 256.0)
    } else {
        (256, 192, 4.0 / 3.0)
    };
    *info = RetroSystemAvInfo {
        geometry: RetroGameGeometry { base_width: w, base_height: h, max_width: 320, max_height: 294, aspect_ratio: aspect },
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
    unsafe { crate::options::declare(cb) };
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
    let s = env.map(|e| crate::options::read(e)).unwrap_or_default();
    let copy = s.gg_link.then(|| rom.clone());
    let m = machine(rom, &s, (*game).path);
    // Two Game Gears: the same cartridge again, its own save in its own file. Game Gear games
    // only, and not with the whole picture shown.
    let second = match copy {
        Some(rom) if m.bus.gg && !s.gg_full => {
            let mut two = machine(rom, &s, (*game).path);
            let save = env.and_then(|e| second_save_path(e, (*game).path));
            let mut written = Vec::new();
            if let Some(bytes) = save.as_ref().and_then(|p| std::fs::read(p).ok()) {
                let n = bytes.len().min(two.bus.cart_ram.len());
                two.bus.cart_ram[..n].copy_from_slice(&bytes[..n]);
                two.bus.cart_ram_used = true;
                written.clone_from(&two.bus.cart_ram);
            }
            Some(Box::new(Second { m: two, picture: Vec::new(), save, written, frames: 0 }))
        }
        _ => None,
    };
    let mut core = Core { m, s: s.clone(), at_start: s, prev: Vec::new(), lp: [0.0; 2], shown_size: (0, 0), eq: [[0.0; 2]; 2], second };
    core.apply();
    if let Ok(mut c) = CORE.lock() {
        *c = Some(core);
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
            power_cycle(&mut m.m);
            if let Some(two) = m.second.as_mut() {
                power_cycle(&mut two.m);
            }
            m.apply();
        }
    }
}

/// The console switched off and on again: everything fresh but the cartridge, its save and how
/// the console was set up.
fn power_cycle(m: &mut Box<Sms>) {
    let rom = std::mem::take(&mut m.bus.rom);
    let cart_ram = std::mem::take(&mut m.bus.cart_ram);
    let (gg, sg, export, pal, mapper) = (m.bus.gg, m.bus.sg, m.bus.export, m.bus.vdp.pal, m.bus.mapper);
    let mut fresh = Sms::new(rom);
    if gg {
        fresh.bus.set_game_gear();
    } else if sg {
        fresh.bus.set_sg1000();
    }
    fresh.bus.export = export;
    fresh.bus.has_fm = m.bus.has_fm;
    fresh.bus.vdp.pal = pal;
    fresh.bus.vdp.sms1 = m.bus.vdp.sms1;
    fresh.bus.mapper = mapper;
    fresh.bus.cart_ram = cart_ram;
    fresh.rate = m.rate;
    **m = fresh;
}

/// The game's file name ends in one of `ends` (lower case).
unsafe fn has_extension(path: *const c_char, ends: &[&str]) -> bool {
    if path.is_null() {
        return false;
    }
    let p = std::ffi::CStr::from_ptr(path).to_string_lossy().to_lowercase();
    ends.iter().any(|e| p.ends_with(e))
}

/// The file's name, lower case ("" when the app gave none).
unsafe fn file_name(path: *const c_char) -> String {
    if path.is_null() {
        return String::new();
    }
    let p = std::ffi::CStr::from_ptr(path).to_string_lossy().to_lowercase();
    p.rsplit('/').next().unwrap_or("").to_owned()
}

/// A Japanese cartridge: its header's region code says so (3 Master System Japan, 5 Game Gear
/// Japan), or its file is named the usual way for one ("(Japan)", "(J)").
unsafe fn is_japanese(rom: &[u8], path: *const c_char) -> bool {
    let name = file_name(path);
    if name.contains("(japan") || name.contains("(j)") || name.contains("(jp)") {
        return true;
    }
    [0x7FF0usize, 0x3FF0, 0x1FF0].iter().any(|&at| {
        rom.get(at..at + 16).is_some_and(|h| &h[..8] == b"TMR SEGA" && matches!(h[15] >> 4, 3 | 5))
    })
}

/// A European release, by its file's name: the 50 Hz consoles' games.
unsafe fn is_european(path: *const c_char) -> bool {
    let name = file_name(path);
    ["(europe", "(e)", "(eu)", "(uk)", "(australia", "(germany", "(france", "(italy", "(spain"]
        .iter()
        .any(|t| name.contains(t))
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

/// Where the console's picture starts in the picture handed to the app (the border, or the left
/// column cut off).
fn picture_offset(m: &Core) -> (i32, i32) {
    if m.bus.gg {
        return (0, 0);
    }
    if m.bus.vdp.borders > 0 {
        let left = if m.bus.vdp.borders == 2 { crate::vdp::Vdp::SIDE_BORDERS.0 as i32 } else { 0 };
        return (left, m.bus.vdp.border_lines().0 as i32);
    }
    if m.at_start.hide_left { (-8, 0) } else { (0, 0) }
}

/// The controls for whatever is plugged into the ports besides a pad: the stick, the D-pad, the
/// buttons, and a finger on the picture for the Light Phaser in port 1.
unsafe fn read_devices(m: &mut Core, input: InputFn) {
    let lines = f32::from(m.bus.vdp.height());
    let (ox, oy) = picture_offset(m);
    let (w, h) = m.shown_size;
    let (adx, ady) = m.s.phaser_offset;
    for p in 0..2u32 {
        if m.bus.ports[p as usize].device == crate::peripherals::Device::Pad {
            continue;
        }
        let pad = |id: u32| input(p, DEVICE_JOYPAD, 0, id) != 0;
        let axis = |id: u32| f32::from(input(p, DEVICE_ANALOG, 0, id)) / 32767.0;
        let touch = if p == 0 && w > 0 && input(0, DEVICE_POINTER, 0, 2) != 0 {
            let fx = (f32::from(input(0, DEVICE_POINTER, 0, 0)) + 32767.0) / 65534.0;
            let fy = (f32::from(input(0, DEVICE_POINTER, 0, 1)) + 32767.0) / 65534.0;
            Some((fx * w as f32 - ox as f32, fy * h as f32 - oy as f32))
        } else {
            None
        };
        let c = crate::peripherals::Controls {
            up: pad(4),
            down: pad(5),
            left: pad(6),
            right: pad(7),
            b1: pad(0) || pad(1),
            b2: pad(8),
            stick: (axis(0), axis(1)),
            touch,
        };
        let port = &mut m.bus.ports[p as usize];
        // The aim adjustment moves where the console sees the gun, not the crosshair.
        port.aim.0 -= adx as f32;
        port.aim.1 -= ady as f32;
        port.frame(&c, lines);
        port.aim.0 += adx as f32;
        port.aim.1 += ady as f32;
    }
}

fn draw_crosshairs(m: &Core, px: &mut [u32], w: usize, h: usize) {
    if !m.s.crosshair || m.bus.gg {
        return;
    }
    let (ox, oy) = picture_offset(m);
    let (adx, ady) = m.s.phaser_offset;
    for port in &m.bus.ports {
        if port.device == crate::peripherals::Device::Phaser && port.aim_on {
            let (x, y) = (port.aim.0 as i32 - adx + ox, port.aim.1 as i32 - ady + oy);
            crate::peripherals::draw_crosshair(px, w, h, x, y, m.s.crosshair_square, m.s.crosshair_colour);
        }
    }
}

/// The pads as the console's ports read them, from the two players' buttons: $DC holds the
/// first pad and the second's Up and Down, $DD the second's other buttons. A held button reads 0.
/// Start on either pad is the console's Pause button.
/// Two Game Gears' pictures as one (side by side or stacked, or only one, the second first when
/// swapped). Returns the size.
fn compose_pair(m: &mut Core, shown: &mut Vec<u32>, w: usize, h: usize) -> (usize, usize) {
    let Some(two) = m.second.as_mut() else { return (w, h) };
    let (show, side, swap) = (m.at_start.gg_link_show, m.at_start.gg_link_side, m.s.gg_link_swap);
    let (w2, h2) = two.m.bus.vdp.shown(&mut two.picture);
    if (w2, h2) != (w, h) {
        return (w, h);
    }
    if show != 0 {
        if show == 2 {
            shown.clone_from(&two.picture);
        }
        return (w, h);
    }
    let first = std::mem::take(shown);
    let (one, other) = if swap { (&two.picture, &first) } else { (&first, &two.picture) };
    if side {
        for y in 0..h {
            shown.extend_from_slice(&one[y * w..(y + 1) * w]);
            shown.extend_from_slice(&other[y * w..(y + 1) * w]);
        }
        (w * 2, h)
    } else {
        shown.extend_from_slice(one);
        shown.extend_from_slice(other);
        (w, h * 2)
    }
}

/// One player's pad: its six buttons as a port reads them (0 held), and Start.
unsafe fn pad_of(input: InputFn, port: u32, opposing: bool) -> (u8, bool) {
    let on = |i: u32| input(port, DEVICE_JOYPAD, 0, i) != 0;
    let held = |id: u32| match id {
        4 | 5 if !opposing && on(4) && on(5) => false,
        6 | 7 if !opposing && on(6) && on(7) => false,
        _ => on(id),
    };
    let bits = [held(4), held(5), held(6), held(7), held(0) || held(1), held(8)];
    (bits.iter().enumerate().fold(0x3F, |v, (i, on)| if *on { v & !(1 << i) } else { v }), held(3))
}

unsafe fn read_pads(input: InputFn, opposing: bool) -> (u8, u8, bool) {
    let held = |port: u32, id: u32| {
        let on = |i: u32| input(port, DEVICE_JOYPAD, 0, i) != 0;
        // A real pad's rocker cannot press both ways at once.
        match id {
            4 | 5 if !opposing && on(4) && on(5) => false,
            6 | 7 if !opposing && on(6) && on(7) => false,
            _ => on(id),
        }
    };
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
    let (video, audio, poll, input, env) = match CB.lock() {
        Ok(c) => (c.video, c.audio_batch, c.poll, c.input, c.env),
        Err(_) => return,
    };
    if let Some(poll) = poll {
        poll();
    }
    let Ok(mut slot) = CORE.lock() else { return };
    let Some(m) = slot.as_mut() else { return };
    if let Some(env) = env {
        if crate::options::changed(env) {
            m.s = crate::options::read(env);
            m.apply();
        }
    }
    if let (Some(input), true) = (input, m.second.is_some()) {
        // Two Game Gears: each player's pad plays one console.
        let (mine, theirs) = (pad_of(input, 0, m.s.opposing), pad_of(input, 1, m.s.opposing));
        let (first, other) = if m.s.gg_link_controls_second { (theirs, mine) } else { (mine, theirs) };
        m.bus.pad_dc = 0xC0 | first.0;
        m.bus.gg_start = first.1;
        if let Some(two) = m.second.as_mut() {
            two.m.bus.pad_dc = 0xC0 | other.0;
            two.m.bus.gg_start = other.1;
        }
    } else if let Some(input) = input {
        let (dc, dd, pause) = read_pads(input, m.s.opposing);
        m.bus.pad_dc = dc;
        m.bus.pad_dd = dd;
        if m.bus.gg {
            // The Game Gear's Start is a button the game reads; it has no Pause.
            m.bus.gg_start = pause;
        } else {
            m.set_pause(pause);
        }
    }
    if let Some(input) = input {
        read_devices(m, input);
    }
    if let Some(two) = m.second.as_mut() {
        m.m.run_frame_linked(&mut two.m);
        two.frames += 1;
        if two.frames % SECOND_SAVE_FRAMES == 0 {
            two.write_save();
        }
    } else {
        m.run_frame();
    }
    // A 3-D glasses game with one eye chosen: the other eye's frames show the last one again.
    let other_eye = m.bus.glasses_used && m.s.glasses_eye.is_some_and(|left| left != m.frame_left_eye) && !m.shown.is_empty();
    if let (Some(video), true) = (video, other_eye) {
        let (w, h) = m.shown_size;
        video(m.shown.as_ptr() as *const c_void, w as u32, h as u32, w * 4);
    } else if let Some(video) = video {
        let mut shown = std::mem::take(&mut m.shown);
        let (mut w, h) = m.bus.vdp.shown(&mut shown);
        if m.at_start.hide_left && !m.bus.gg && m.bus.vdp.borders == 0 {
            shown = shown.chunks(w).flat_map(|row| row[8..].iter().copied()).collect();
            w -= 8;
        }
        draw_crosshairs(m, &mut shown, w, h);
        let (w, h) = compose_pair(m, &mut shown, w, h);
        if m.s.gg_ghosting > 0 && m.bus.gg {
            if m.prev.len() == shown.len() {
                let keep = u32::from(m.s.gg_ghosting);
                let mix = |a: u32, b: u32, sh: u32| (((a >> sh & 0xFF) * (100 - keep) + (b >> sh & 0xFF) * keep) / 100) << sh;
                for (p, q) in shown.iter_mut().zip(&m.prev) {
                    *p = mix(*p, *q, 16) | mix(*p, *q, 8) | mix(*p, *q, 0);
                }
            }
            m.prev.clone_from(&shown);
        }
        video(shown.as_ptr() as *const c_void, w as u32, h as u32, w * 4);
        m.shown = shown;
        m.shown_size = (w, h);
    }
    let mut samples = std::mem::take(&mut m.samples);
    if let Some(two) = m.second.as_mut() {
        // The two run line for line, so they make the same amount of sound.
        let other = std::mem::take(&mut two.m.samples);
        let n = samples.len().min(other.len());
        match m.s.gg_link_audio {
            1 => samples[..n].copy_from_slice(&other[..n]),
            2 => {
                for (a, b) in samples.iter_mut().zip(&other[..n]) {
                    *a = ((i32::from(*a) + i32::from(*b)) / 2) as i16;
                }
            }
            _ => {}
        }
    }
    if m.s.mono {
        for pair in samples.chunks_mut(2) {
            let v = ((i32::from(pair[0]) + i32::from(pair[1])) / 2) as i16;
            pair.fill(v);
        }
    }
    if m.s.eq != [100; 3] {
        let gains = m.s.eq;
        equalise(&mut m.eq, gains, &mut samples);
    }
    if m.s.lowpass > 0 {
        // One pole: each sample moves part of the way to the new value.
        let a = 1.0 - (-std::f32::consts::TAU * f32::from(m.s.lowpass) / RATE as f32).exp();
        for pair in samples.chunks_mut(2) {
            for (x, y) in pair.iter_mut().zip(m.lp.iter_mut()) {
                *y += a * (f32::from(*x) - *y);
                *x = *y as i16;
            }
        }
    }
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
    CORE.lock().ok().and_then(|c| c.as_ref().map(|m| state_of(m).len() + 1024)).unwrap_or(0)
}

#[no_mangle]
pub unsafe extern "C" fn retro_serialize(data: *mut c_void, size: usize) -> bool {
    let Ok(slot) = CORE.lock() else { return false };
    let Some(m) = slot.as_ref() else { return false };
    let state = state_of(m);
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
    // Two Game Gears' state is both, the first one's length in front. A single state loads into
    // the first of two, and the first of a pair's into a single one.
    let ok = match bytes.strip_prefix(DUAL_MAGIC.as_slice()) {
        Some(rest) if rest.len() >= 4 => {
            let n = u32::from_le_bytes([rest[0], rest[1], rest[2], rest[3]]) as usize;
            let (first, other) = rest[4..].split_at(n.min(rest.len() - 4));
            let ok = m.m.load_state(first).is_ok();
            if let Some(two) = m.second.as_mut() {
                let _ = two.m.load_state(other);
            }
            ok
        }
        _ => m.m.load_state(bytes).is_ok(),
    };
    // The volumes are settings, not part of a state: put them back over the loaded machine.
    m.apply();
    ok
}

/// A save state: the machine's, or both Game Gears'.
fn state_of(m: &Core) -> Vec<u8> {
    let first = m.m.save_state();
    let Some(two) = m.second.as_ref() else { return first };
    let mut out = DUAL_MAGIC.to_vec();
    out.extend_from_slice(&(first.len() as u32).to_le_bytes());
    out.extend_from_slice(&first);
    out.extend_from_slice(&two.m.save_state());
    out
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

/// The equaliser: three bands from two one-pole low-passes (880 Hz and 5 kHz) per side, each
/// band at its level (percent).
fn equalise(state: &mut [[f32; 2]; 2], levels: [u8; 3], samples: &mut [i16]) {
    let pole = |hz: f32| 1.0 - (-std::f32::consts::TAU * hz / RATE as f32).exp();
    let (a, b) = (pole(880.0), pole(5000.0));
    let g = levels.map(|v| f32::from(v) / 100.0);
    for pair in samples.chunks_mut(2) {
        for (c, x) in pair.iter_mut().enumerate() {
            let v = f32::from(*x);
            state[0][c] += a * (v - state[0][c]);
            state[1][c] += b * (v - state[1][c]);
            let (low, below_high) = (state[0][c], state[1][c]);
            let out = low * g[0] + (below_high - low) * g[1] + (v - below_high) * g[2];
            *x = out.clamp(-32768.0, 32767.0) as i16;
        }
    }
}

#[cfg(test)]
mod eq_tests {
    use super::*;

    /// A sine of `hz` through the equaliser; its loudness at the end, as a fraction of the input's.
    fn through(hz: f32, levels: [u8; 3]) -> f32 {
        let mut s: Vec<i16> = (0..9600).flat_map(|i| {
            let v = ((i as f32 * std::f32::consts::TAU * hz / RATE as f32).sin() * 10_000.0) as i16;
            [v, v]
        }).collect();
        let mut st = [[0.0; 2]; 2];
        equalise(&mut st, levels, &mut s);
        s[s.len() - 2400..].iter().map(|v| i32::from(*v).abs()).max().unwrap() as f32 / 10_000.0
    }

    #[test]
    fn region_and_refresh_auto_go_by_the_header_and_the_file_name() {
        let mut rom = vec![0u8; 0x8000];
        rom[0x7FF0..0x7FF8].copy_from_slice(b"TMR SEGA");
        rom[0x7FFF] = 0x3C;
        let none = std::ptr::null();
        unsafe {
            assert!(is_japanese(&rom, none), "region code 3: Master System Japan");
            rom[0x7FFF] = 0x4C;
            assert!(!is_japanese(&rom, none));
            assert!(is_japanese(&rom, c"/g/Hokuto no Ken (Japan).sms".as_ptr()));
            assert!(is_european(c"/g/Sonic (Europe).sms".as_ptr()));
            assert!(!is_european(c"/g/Sonic (USA).sms".as_ptr()));
        }
    }

    #[test]
    fn each_band_turns_its_own_notes_up_or_down() {
        assert!((through(200.0, [100; 3]) - 1.0).abs() < 0.01, "flat changes nothing");
        assert!(through(100.0, [0, 100, 100]) < 0.25, "bass off: a low note mostly gone");
        assert!(through(100.0, [100, 100, 0]) > 0.9, "treble off: a low note stays");
        assert!(through(12_000.0, [100, 100, 0]) < 0.45, "treble off: a high note much quieter");
        assert!(through(12_000.0, [150, 100, 150]) > 1.3, "treble up: a high note louder");
    }
}
