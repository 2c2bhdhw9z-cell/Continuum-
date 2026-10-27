//! cAtari as a libretro core: what the app calls. The 7800's picture (320 dots wide, 224 lines on
//! NTSC and 260 on PAL, or every line MARIA draws with "Whole picture" on), its sound at 48 kHz,
//! two controllers (ProLine joysticks, the light gun, paddles), the console's switches, save states,
//! and its memory for cheats and achievements.
//!
//! The pad works as ProSystem's does, so the app's 7800 touch layout fits: B is button 1 (the left
//! fire button), A is button 2, X is the console's Reset, Select is Select, Start is Pause, and L
//! and R are the left and right difficulty switches.

use std::ffi::{c_char, c_void, CStr};
use std::sync::Mutex;

use crate::cart::Cart;
use crate::machine::{Atari7800, PortKind, Region};
use crate::options::{Port, Settings, SlowIo};

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
struct InputDescriptor {
    port: u32,
    device: u32,
    index: u32,
    id: u32,
    description: *const c_char,
}

#[repr(C)]
struct MemoryDescriptor {
    flags: u64,
    ptr: *mut c_void,
    offset: usize,
    start: usize,
    select: usize,
    disconnect: usize,
    len: usize,
    addrspace: *const c_char,
}

#[repr(C)]
struct MemoryMap {
    descriptors: *const MemoryDescriptor,
    num_descriptors: u32,
}

type EnvFn = unsafe extern "C" fn(u32, *mut c_void) -> bool;
type VideoFn = unsafe extern "C" fn(*const c_void, u32, u32, usize);
type AudioFn = unsafe extern "C" fn(i16, i16);
type AudioBatchFn = unsafe extern "C" fn(*const i16, usize) -> usize;
type PollFn = unsafe extern "C" fn();
type InputFn = unsafe extern "C" fn(u32, u32, u32, u32) -> i16;

const ENV_GET_SYSTEM_DIRECTORY: u32 = 9;
const ENV_SET_PIXEL_FORMAT: u32 = 10;
const ENV_SET_INPUT_DESCRIPTORS: u32 = 11;
const ENV_SET_MEMORY_MAPS: u32 = 36;
const PIXEL_FORMAT_XRGB8888: u32 = 1;
const MEMDESC_SYSTEM_RAM: u64 = 1 << 2;
const MEMDESC_SAVE_RAM: u64 = 1 << 3;

const DEVICE_JOYPAD: u32 = 1;
const DEVICE_LIGHTGUN: u32 = 4;
const DEVICE_ANALOG: u32 = 5;
const DEVICE_POINTER: u32 = 6;
const JOY_B: u32 = 0;
const JOY_SELECT: u32 = 2;
const JOY_START: u32 = 3;
const JOY_UP: u32 = 4;
const JOY_DOWN: u32 = 5;
const JOY_LEFT: u32 = 6;
const JOY_RIGHT: u32 = 7;
const JOY_A: u32 = 8;
const JOY_X: u32 = 9;
const JOY_L: u32 = 10;
const JOY_R: u32 = 11;
const MEMORY_SAVE_RAM: u32 = 0;
const MEMORY_SYSTEM_RAM: u32 = 2;

/// Save states start with this, then the version, then the machine.
const STATE_MAGIC: &[u8; 8] = b"CATARI01";

struct Callbacks {
    env: Option<EnvFn>,
    video: Option<VideoFn>,
    audio_batch: Option<AudioBatchFn>,
    poll: Option<PollFn>,
    input: Option<InputFn>,
}

static CB: Mutex<Callbacks> = Mutex::new(Callbacks { env: None, video: None, audio_batch: None, poll: None, input: None });
static CORE: Mutex<Option<Core>> = Mutex::new(None);

struct Core {
    m: Box<Atari7800>,
    /// The settings as the player has them now, and as they were when the game opened.
    s: Settings,
    at_start: Settings,
    palette: [u32; 256],
    first_line: u32,
    lines_shown: u32,
    frame: Vec<u32>,
    prev: Vec<u32>,
    /// The difficulty switches as the buttons left them, and the buttons last frame.
    switches: [bool; 2],
    held: [bool; 2],
    /// The sound filters' state: the low-pass, and the DC-removing high-pass.
    lp: f32,
    hp_in: f32,
    hp_out: f32,
    audio: Vec<i16>,
    /// Where the light gun points, in the picture's dots and lines.
    aim: Option<(i32, i32)>,
}

impl Core {
    fn apply(&mut self) {
        self.palette = crate::palette::build(self.s.palette, self.s.saturation, self.s.brightness);
        self.m.sys.input.difficulty_a = self.switches;
    }
}

unsafe fn system_dir(env: EnvFn) -> Option<std::path::PathBuf> {
    let mut dir: *const c_char = std::ptr::null();
    if !env(ENV_GET_SYSTEM_DIRECTORY, &mut dir as *mut *const c_char as *mut c_void) || dir.is_null() {
        return None;
    }
    Some(std::path::PathBuf::from(CStr::from_ptr(dir).to_string_lossy().into_owned()))
}

/// The console's own BIOS from the app's system folder, under the names ProSystem uses: 4 KB for
/// the NTSC console, 16 KB for the PAL one.
unsafe fn read_bios(env: EnvFn, pal: bool) -> Option<Vec<u8>> {
    let dir = system_dir(env)?;
    let names: &[&str] = if pal { &["7800 BIOS (E).rom", "7800 BIOS (EU).rom"] } else { &["7800 BIOS (U).rom", "7800 BIOS (US).rom"] };
    for n in names {
        if let Ok(b) = std::fs::read(dir.join(n)) {
            if b.len() == 0x1000 || b.len() == 0x4000 {
                return Some(b);
            }
        }
    }
    None
}

#[no_mangle]
pub extern "C" fn retro_api_version() -> u32 {
    1
}

/// # Safety
/// `info` must point to writable memory for one `RetroSystemInfo`.
#[no_mangle]
pub unsafe extern "C" fn retro_get_system_info(info: *mut RetroSystemInfo) {
    if info.is_null() {
        return;
    }
    *info = RetroSystemInfo {
        library_name: c"cAtari".as_ptr(),
        library_version: c"0.1.0 (state format 1)".as_ptr(),
        valid_extensions: c"a78|bin".as_ptr(),
        need_fullpath: false,
        block_extract: false,
    };
}

fn geometry(region: Region, overscan: bool) -> (u32, u32, f32) {
    let (vis_first, vis_end) = region.visible();
    let std_lines = vis_end - vis_first;
    let (first, lines) = if overscan { (16, region.lines() - 5 - 16) } else { (vis_first, std_lines) };
    let aspect = 4.0 / 3.0 * std_lines as f32 / lines as f32;
    (first, lines, aspect)
}

/// # Safety
/// `info` must point to writable memory for one `RetroSystemAvInfo`.
#[no_mangle]
pub unsafe extern "C" fn retro_get_system_av_info(info: *mut RetroSystemAvInfo) {
    if info.is_null() {
        return;
    }
    let (region, overscan, rate) = CORE
        .lock()
        .ok()
        .and_then(|c| c.as_ref().map(|c| (c.m.sys.region, c.at_start.overscan, c.at_start.sound_rate)))
        .unwrap_or((Region::Ntsc, false, 48_000));
    let (_, lines, aspect) = geometry(region, overscan);
    *info = RetroSystemAvInfo {
        geometry: RetroGameGeometry { base_width: 320, base_height: lines, max_width: 320, max_height: 292, aspect_ratio: aspect },
        timing: RetroSystemTiming { fps: region.fps(), sample_rate: rate as f64 },
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

unsafe fn describe_inputs(env: EnvFn) {
    let mut d: Vec<InputDescriptor> = Vec::new();
    for port in 0..2 {
        for (id, text) in [
            (JOY_LEFT, c"Left"), (JOY_UP, c"Up"), (JOY_DOWN, c"Down"), (JOY_RIGHT, c"Right"),
            (JOY_B, c"Button 1"), (JOY_A, c"Button 2"),
        ] {
            d.push(InputDescriptor { port, device: DEVICE_JOYPAD, index: 0, id, description: text.as_ptr() });
        }
    }
    for (id, text) in [
        (JOY_X, c"Console Reset"), (JOY_SELECT, c"Console Select"), (JOY_START, c"Console Pause"),
        (JOY_L, c"Left Difficulty"), (JOY_R, c"Right Difficulty"),
    ] {
        d.push(InputDescriptor { port: 0, device: DEVICE_JOYPAD, index: 0, id, description: text.as_ptr() });
    }
    d.push(InputDescriptor { port: 0, device: 0, index: 0, id: 0, description: std::ptr::null() });
    env(ENV_SET_INPUT_DESCRIPTORS, d.as_mut_ptr() as *mut c_void);
}

/// Tells the app where the 7800's memory is, at the addresses the console sees it, so achievements
/// and cheats find it. The pointers stay valid for the whole game: loading a state copies into the
/// same memory.
unsafe fn publish_memory(env: EnvFn, m: &mut Atari7800) {
    let ram = m.sys.ram.as_mut_ptr() as *mut c_void;
    let riot = m.sys.riot.ram.as_mut_ptr() as *mut c_void;
    let mut d = vec![
        MemoryDescriptor { flags: MEMDESC_SYSTEM_RAM, ptr: ram, offset: 0, start: 0x1800, select: 0xF800, disconnect: 0, len: 0x800, addrspace: std::ptr::null() },
        MemoryDescriptor { flags: MEMDESC_SYSTEM_RAM, ptr: ram, offset: 0x800, start: 0x2000, select: 0xF800, disconnect: 0, len: 0x800, addrspace: std::ptr::null() },
        MemoryDescriptor { flags: MEMDESC_SYSTEM_RAM, ptr: ram, offset: 0x840, start: 0x0040, select: 0xFFC0, disconnect: 0, len: 0x40, addrspace: std::ptr::null() },
        MemoryDescriptor { flags: MEMDESC_SYSTEM_RAM, ptr: ram, offset: 0x880, start: 0x0080, select: 0xFF80, disconnect: 0, len: 0x80, addrspace: std::ptr::null() },
        MemoryDescriptor { flags: MEMDESC_SYSTEM_RAM, ptr: ram, offset: 0x940, start: 0x0140, select: 0xFFC0, disconnect: 0, len: 0x40, addrspace: std::ptr::null() },
        MemoryDescriptor { flags: MEMDESC_SYSTEM_RAM, ptr: ram, offset: 0x980, start: 0x0180, select: 0xFF80, disconnect: 0, len: 0x80, addrspace: std::ptr::null() },
        MemoryDescriptor { flags: 0, ptr: riot, offset: 0, start: 0x0480, select: 0xFF80, disconnect: 0, len: 0x80, addrspace: std::ptr::null() },
    ];
    if !m.sys.cart.ram.is_empty() {
        let cart = m.sys.cart.ram.as_mut_ptr() as *mut c_void;
        d.push(MemoryDescriptor { flags: MEMDESC_SAVE_RAM, ptr: cart, offset: 0, start: 0x4000, select: 0xC000, disconnect: 0, len: m.sys.cart.ram.len().min(0x4000), addrspace: std::ptr::null() });
    }
    let d: &'static mut Vec<MemoryDescriptor> = Box::leak(Box::new(d));
    let mut map = MemoryMap { descriptors: d.as_ptr(), num_descriptors: d.len() as u32 };
    env(ENV_SET_MEMORY_MAPS, &mut map as *mut MemoryMap as *mut c_void);
}

fn port_kind(p: Port, cart: &Cart, port: usize) -> PortKind {
    match p {
        Port::Joystick => PortKind::Joystick,
        Port::LightGun if port == 0 => PortKind::LightGun,
        Port::LightGun => PortKind::Joystick,
        Port::Paddles => PortKind::Paddles,
        Port::Auto => match cart.info.controllers[port] {
            2 if port == 0 => PortKind::LightGun,
            3 => PortKind::Paddles,
            _ if port == 0 && cart.info.light_gun => PortKind::LightGun,
            _ => PortKind::Joystick,
        },
    }
}

/// A new machine for `data` as the settings ask.
unsafe fn machine(data: &[u8], s: &Settings, env: Option<EnvFn>) -> Option<Box<Atari7800>> {
    let cart = Cart::load(data).ok()?;
    let pal = s.region.or(cart.info.pal).unwrap_or(false);
    let region = if pal { Region::Pal } else { Region::Ntsc };
    let bios = match (env, s.bios) {
        (Some(e), true) => read_bios(e, pal),
        _ => None,
    };
    let ports = [port_kind(s.ports[0], &cart, 0), port_kind(s.ports[1], &cart, 1)];
    let mut m = Box::new(Atari7800::new(cart, region, bios));
    m.sys.ports = ports;
    m.sys.slow_io = match s.slow_io {
        SlowIo::On => true,
        SlowIo::Off => false,
        SlowIo::Auto => ports[0] == PortKind::LightGun,
    };
    m.sys.set_output_rate(s.sound_rate as u64);
    Some(m)
}

/// # Safety
/// `game` must be null or point to a valid `RetroGameInfo` whose data outlives the call.
#[no_mangle]
pub unsafe extern "C" fn retro_load_game(game: *const RetroGameInfo) -> bool {
    if game.is_null() || (*game).data.is_null() || (*game).size == 0 {
        return false;
    }
    let data = std::slice::from_raw_parts((*game).data as *const u8, (*game).size);
    let env = CB.lock().ok().and_then(|c| c.env);
    if let Some(env) = env {
        let mut fmt = PIXEL_FORMAT_XRGB8888;
        if !env(ENV_SET_PIXEL_FORMAT, &mut fmt as *mut u32 as *mut c_void) {
            return false;
        }
        describe_inputs(env);
    }
    let s = env.map(|e| crate::options::read(e)).unwrap_or_default();
    let Some(mut m) = machine(data, &s, env) else { return false };
    if let Some(env) = env {
        publish_memory(env, &mut m);
    }
    let (first_line, lines_shown, _) = geometry(m.sys.region, s.overscan);
    let mut core = Core {
        m,
        palette: [0; 256],
        first_line,
        lines_shown,
        frame: vec![0; 320 * lines_shown as usize],
        prev: Vec::new(),
        switches: s.difficulty_a,
        held: [false; 2],
        lp: 0.0,
        hp_in: 0.0,
        hp_out: 0.0,
        audio: Vec::with_capacity(2048),
        aim: None,
        s: s.clone(),
        at_start: s,
    };
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
    CORE.lock().ok().and_then(|c| c.as_ref().map(|c| u32::from(c.m.sys.region == Region::Pal))).unwrap_or(0)
}

/// The console switched off and on: everything fresh but the cartridge (and its RAM, the way a
/// real one keeps it while the power is off for a moment).
#[no_mangle]
pub extern "C" fn retro_reset() {
    if let Ok(mut c) = CORE.lock() {
        if let Some(core) = c.as_mut() {
            let m = &mut core.m;
            let mut cart = m.sys.cart.clone();
            cart.restore_rom(&m.sys.cart);
            let bios = m.sys.bios.clone();
            let (ports, slow, region) = (m.sys.ports, m.sys.slow_io, m.sys.region);
            let rate = core.at_start.sound_rate;
            let mut fresh = Atari7800::new(cart, region, bios);
            fresh.sys.ports = ports;
            fresh.sys.slow_io = slow;
            fresh.sys.set_output_rate(rate as u64);
            // Keep the RAM where the app was told it is.
            fresh.sys.ram = std::mem::take(&mut m.sys.ram);
            fresh.sys.ram.iter_mut().for_each(|b| *b = 0);
            fresh.sys.riot.ram = std::mem::take(&mut m.sys.riot.ram);
            fresh.sys.riot.ram.iter_mut().for_each(|b| *b = 0);
            fresh.sys.cart.ram = std::mem::take(&mut m.sys.cart.ram);
            **m = fresh;
            core.apply();
        }
    }
}

unsafe fn pressed(input: InputFn, port: u32, id: u32) -> bool {
    input(port, DEVICE_JOYPAD, 0, id) != 0
}

/// The sticks, buttons and switches, read for one frame.
unsafe fn read_controls(core: &mut Core, input: InputFn) {
    let s = &core.s;
    let i = &mut core.m.sys.input;
    for p in 0..2u32 {
        let mut d = [pressed(input, p, JOY_UP), pressed(input, p, JOY_DOWN), pressed(input, p, JOY_LEFT), pressed(input, p, JOY_RIGHT)];
        if p == 1 && s.dual_stick {
            // Player 2's stick on player 1's right stick.
            let x = input(0, DEVICE_ANALOG, 1, 0);
            let y = input(0, DEVICE_ANALOG, 1, 1);
            d[0] |= y < -0x4000;
            d[1] |= y > 0x4000;
            d[2] |= x < -0x4000;
            d[3] |= x > 0x4000;
        }
        if !s.opposing {
            if d[0] && d[1] {
                d[0] = false;
                d[1] = false;
            }
            if d[2] && d[3] {
                d[2] = false;
                d[3] = false;
            }
        }
        i.dirs[p as usize] = d;
        i.buttons[p as usize] = [pressed(input, p, JOY_B), pressed(input, p, JOY_A)];
    }
    i.reset = pressed(input, 0, JOY_X);
    i.select = pressed(input, 0, JOY_SELECT);
    i.pause = pressed(input, 0, JOY_START);
    let lr = [pressed(input, 0, JOY_L), pressed(input, 0, JOY_R)];
    for k in 0..2 {
        if s.difficulty_hold {
            core.switches[k] = !lr[k] && s.difficulty_a[k];
        } else if lr[k] && !core.held[k] {
            core.switches[k] = !core.switches[k];
        }
    }
    core.held = lr;
    i.difficulty_a = core.switches;
    // Paddles: the left stick (and the D-pad, slowly) turn the knob; the fire button is its button.
    for p in 0..2usize {
        if core.m.sys.ports[p] == PortKind::Paddles {
            for k in 0..2usize {
                let x = input(p as u32, DEVICE_ANALOG, k as u32, 0) as i32;
                let v = (((x + 0x8000) * 255) / 0xFFFF).clamp(0, 255) as u8;
                core.m.sys.input.paddles[2 * p + k] = v;
                core.m.sys.input.paddle_buttons[2 * p + k] =
                    pressed(input, p as u32, if k == 0 { JOY_B } else { JOY_A });
            }
        }
    }
    // The light gun: the app's pointer (a touch on the picture), or a light gun device.
    if core.m.sys.ports[0] == PortKind::LightGun {
        let touch = input(0, DEVICE_POINTER, 0, 2) != 0;
        let (px, py) = (input(0, DEVICE_POINTER, 0, 0) as i32, input(0, DEVICE_POINTER, 0, 1) as i32);
        let gun_x = input(0, DEVICE_LIGHTGUN, 0, 13) as i32; // SCREEN_X
        let gun_y = input(0, DEVICE_LIGHTGUN, 0, 14) as i32; // SCREEN_Y
        let gun_trigger = input(0, DEVICE_LIGHTGUN, 0, 2) != 0; // TRIGGER
        let (x, y) = if touch || (px != 0 || py != 0) { (px, py) } else { (gun_x, gun_y) };
        let w = 320i32;
        let h = core.lines_shown as i32;
        // -0x7FFF..0x7FFF across the picture.
        let dot = ((x + 0x7FFF) * w) / 0xFFFE + core.s.gun_offset.0;
        let row = ((y + 0x7FFF) * h) / 0xFFFE + core.s.gun_offset.1;
        let on = (0..w).contains(&dot) && (0..h).contains(&row) && (x != 0 || y != 0 || touch);
        core.aim = on.then_some((dot, row));
        let i = &mut core.m.sys.input;
        i.trigger = touch || gun_trigger || pressed(input, 0, JOY_B) || pressed(input, 0, JOY_A);
        i.gun = core.aim.map(|(dot, row)| {
            // The beam passes the spot (dot, row) about 95 processor half-cycles' worth of MARIA
            // dots after the line starts, as a7800's calibration has it.
            let line = core.first_line + row as u32;
            let clock = ((dot as u32 + 190) % 454) as u32;
            (line, clock)
        });
    }
}

fn draw_crosshair(px: &mut [u32], w: usize, h: usize, at: (i32, i32), colour: u32) {
    let (cx, cy) = at;
    for d in -4..=4i32 {
        for (x, y) in [(cx + d, cy), (cx, cy + d)] {
            if x >= 0 && y >= 0 && (x as usize) < w && (y as usize) < h {
                px[y as usize * w + x as usize] = colour;
            }
        }
    }
}

/// # Safety
/// Called by the frontend on its thread, after a game was loaded.
#[no_mangle]
pub unsafe extern "C" fn retro_run() {
    let cbs = match CB.lock() {
        Ok(c) => (c.env, c.video, c.audio_batch, c.poll, c.input),
        Err(_) => return,
    };
    let (env, video, audio, poll, input) = cbs;
    let mut guard = match CORE.lock() {
        Ok(g) => g,
        Err(_) => return,
    };
    let Some(core) = guard.as_mut() else { return };
    if let Some(env) = env {
        if crate::options::changed(env) {
            let mut s = crate::options::read(env);
            // The ones that say "next start" keep their values until then.
            s.region = core.at_start.region;
            s.bios = core.at_start.bios;
            s.overscan = core.at_start.overscan;
            s.sound_rate = core.at_start.sound_rate;
            s.ports = core.at_start.ports;
            if s.difficulty_a != core.s.difficulty_a {
                core.switches = s.difficulty_a;
            }
            core.s = s;
            core.apply();
        }
    }
    if let Some(poll) = poll {
        poll();
    }
    if let Some(input) = input {
        read_controls(core, input);
    }
    core.m.run_frame();

    // The picture.
    let w = 320usize;
    let h = core.lines_shown as usize;
    let pic = core.m.picture(core.first_line, core.lines_shown);
    for (o, &c) in core.frame.iter_mut().zip(pic) {
        *o = core.palette[c as usize];
    }
    if core.s.blend > 0 {
        let b = core.s.blend as u32;
        if core.prev.len() == core.frame.len() {
            for (o, p) in core.frame.iter_mut().zip(core.prev.iter()) {
                let mix = |s: u32| -> u32 {
                    let a = (*o >> s) & 0xFF;
                    let q = (*p >> s) & 0xFF;
                    ((a * (100 - b) + q * b) / 100) << s
                };
                let v = mix(16) | mix(8) | mix(0);
                *o = v;
            }
        }
        core.prev.clone_from(&core.frame);
    }
    let mut shown = core.frame.clone();
    if let (true, Some(at)) = (core.s.crosshair, core.aim) {
        draw_crosshair(&mut shown, w, h, at, core.s.crosshair_colour);
    }
    if let Some(video) = video {
        video(shown.as_ptr() as *const c_void, w as u32, h as u32, w * 4);
    }

    // The sound: the TIA's level (0-30) and POKEY's (0-60) averaged over each output sample.
    let s = &core.s;
    let tia = s.tia_volume as f32 / 100.0 / 30.0;
    let pokey = s.pokey_volume as f32 / 100.0 / 60.0;
    let master = s.volume as f32 / 100.0;
    let a = if s.lowpass { s.lowpass_level as f32 / 100.0 } else { 0.0 };
    core.audio.clear();
    for (t, p) in core.m.sys.out.drain(..) {
        let x = (t * tia + p * pokey) * master;
        // Remove the steady offset (the TIA's output only swings one way), as the TV's input did.
        let hp = 0.999 * (core.hp_out + x - core.hp_in);
        core.hp_in = x;
        core.hp_out = hp;
        core.lp = core.lp * a + hp * (1.0 - a);
        let v = (core.lp * 24_000.0).clamp(-32768.0, 32767.0) as i16;
        core.audio.push(v);
        core.audio.push(v);
    }
    if let Some(audio) = audio {
        let mut sent = 0;
        while sent < core.audio.len() / 2 {
            let n = audio(core.audio.as_ptr().add(sent * 2), core.audio.len() / 2 - sent);
            if n == 0 {
                break;
            }
            sent += n;
        }
    }
}

fn state_of(m: &Atari7800) -> Vec<u8> {
    let mut out = STATE_MAGIC.to_vec();
    out.extend(bincode::serialize(m).unwrap_or_default());
    out
}

#[no_mangle]
pub extern "C" fn retro_serialize_size() -> usize {
    CORE.lock().ok().and_then(|c| c.as_ref().map(|c| state_of(&c.m).len() + 1024)).unwrap_or(0)
}

/// # Safety
/// `data` must point to `size` writable bytes.
#[no_mangle]
pub unsafe extern "C" fn retro_serialize(data: *mut c_void, size: usize) -> bool {
    let Ok(c) = CORE.lock() else { return false };
    let Some(core) = c.as_ref() else { return false };
    let st = state_of(&core.m);
    if st.len() + 8 > size {
        return false;
    }
    let out = std::slice::from_raw_parts_mut(data as *mut u8, size);
    out[..8].copy_from_slice(&(st.len() as u64).to_le_bytes());
    out[8..8 + st.len()].copy_from_slice(&st);
    true
}

/// # Safety
/// `data` must point to `size` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn retro_unserialize(data: *const c_void, size: usize) -> bool {
    if data.is_null() || size < 16 {
        return false;
    }
    let bytes = std::slice::from_raw_parts(data as *const u8, size);
    let len = u64::from_le_bytes(bytes[..8].try_into().unwrap()) as usize;
    if len + 8 > size || len < 8 || &bytes[8..16] != STATE_MAGIC {
        return false;
    }
    let Ok(mut new) = bincode::deserialize::<Atari7800>(&bytes[16..8 + len]) else { return false };
    let Ok(mut c) = CORE.lock() else { return false };
    let Some(core) = c.as_mut() else { return false };
    let m = &mut core.m;
    if new.sys.ram.len() != m.sys.ram.len() || new.sys.riot.ram.len() != m.sys.riot.ram.len() || new.sys.cart.ram.len() != m.sys.cart.ram.len() {
        return false;
    }
    // The memory the app was told about stays where it is: copy into it.
    m.sys.ram.copy_from_slice(&new.sys.ram);
    m.sys.riot.ram.copy_from_slice(&new.sys.riot.ram);
    m.sys.cart.ram.copy_from_slice(&new.sys.cart.ram);
    new.sys.ram = std::mem::take(&mut m.sys.ram);
    new.sys.riot.ram = std::mem::take(&mut m.sys.riot.ram);
    new.sys.cart.ram = std::mem::take(&mut m.sys.cart.ram);
    new.sys.cart.restore_rom(&m.sys.cart);
    new.sys.bios = m.sys.bios.take();
    new.sys.out = Vec::new();
    **m = new;
    core.apply();
    true
}

#[no_mangle]
pub extern "C" fn retro_cheat_reset() {}
#[no_mangle]
pub extern "C" fn retro_cheat_set(_index: u32, _enabled: bool, _code: *const c_char) {}

#[no_mangle]
pub extern "C" fn retro_get_memory_data(id: u32) -> *mut c_void {
    let Ok(mut c) = CORE.lock() else { return std::ptr::null_mut() };
    let Some(core) = c.as_mut() else { return std::ptr::null_mut() };
    match id {
        MEMORY_SYSTEM_RAM => core.m.sys.ram.as_mut_ptr() as *mut c_void,
        MEMORY_SAVE_RAM => std::ptr::null_mut(),
        _ => std::ptr::null_mut(),
    }
}

#[no_mangle]
pub extern "C" fn retro_get_memory_size(id: u32) -> usize {
    let Ok(c) = CORE.lock() else { return 0 };
    let Some(core) = c.as_ref() else { return 0 };
    match id {
        MEMORY_SYSTEM_RAM => core.m.sys.ram.len(),
        _ => 0,
    }
}
