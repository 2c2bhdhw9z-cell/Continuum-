//! The libretro entry points. One machine at a time, behind a mutex.

#![allow(clippy::missing_safety_doc)]

use std::ffi::{c_char, c_void};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::gb::GameBoy;
use crate::gba::{self, Gba};
use crate::ppu::{H, W};

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
const MEMORY_SAVE_RAM: u32 = 0;
const MEMORY_RTC: u32 = 1;
const MEMORY_SYSTEM_RAM: u32 = 2;
const MEMORY_VIDEO_RAM: u32 = 3;
const DEVICE_JOYPAD: u32 = 1;

/// The RTC block the frontend saves: live registers (5), latched (5), 6 spare, then the Unix
/// time (u64, little-endian) the registers were true at.
const RTC_LEN: usize = 24;

struct Core {
    gb: GameBoy,
    /// Set when the loaded ROM is a Game Boy Advance one (then `gb` is an idle placeholder).
    gba: Option<Box<Gba>>,
    frame: Vec<u32>,
    rtc_block: [u8; RTC_LEN],
    rtc_synced: bool,
    stuck_frames: u32,
    stuck_reports: u32,
}

struct Callbacks {
    env: Option<EnvFn>,
    video: Option<VideoFn>,
    audio_batch: Option<AudioBatchFn>,
    poll: Option<PollFn>,
    input: Option<InputFn>,
}

static CORE: Mutex<Option<Core>> = Mutex::new(None);

/// libretro's log callback (printf-style, variadic).
type LogFn = unsafe extern "C" fn(level: u32, fmt: *const std::ffi::c_char, ...);
#[repr(C)]
struct RetroLogCallback {
    log: Option<LogFn>,
}
const ENV_GET_LOG_INTERFACE: u32 = 27;
static LOG: Mutex<Option<LogFn>> = Mutex::new(None);

/// Sends one line to the frontend's log (level INFO), prefixed "cBoy: ".
fn log_line(line: &str) {
    let Some(log) = LOG.lock().ok().and_then(|l| *l) else { return };
    let Ok(text) = std::ffi::CString::new(format!("cBoy: {line}\n").replace('\0', " ")) else { return };
    unsafe { log(1, c"%s".as_ptr(), text.as_ptr()) };
}
static CB: Mutex<Callbacks> = Mutex::new(Callbacks {
    env: None,
    video: None,
    audio_batch: None,
    poll: None,
    input: None,
});

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

impl Core {
    /// Logs (sparsely) when the game seems stuck: no VBlank interrupt taken for two seconds
    /// while it has them enabled and the LCD on, or the CPU locked. A phone report then says
    /// where the CPU was.
    fn watch_lockup(&mut self, vblanks_before: u32) {
        let b = &self.gb.bus;
        let c = &self.gb.cpu;
        let waiting = b.ie & 1 != 0 && b.ppu.lcdc & 0x80 != 0 && b.irq_taken[0] == vblanks_before;
        self.stuck_frames = if waiting || c.locked { self.stuck_frames + 1 } else { 0 };
        if self.stuck_frames == 120 && self.stuck_reports < 5 {
            self.stuck_reports += 1;
            let line = format!(
                "stuck: no VBlank interrupt for 2 s. PC {:04X} SP {:04X} IME {} halted {} locked {} \
                 IE {:02X} IF {:02X} LCDC {:02X} STAT {:02X} LY {} ROM bank {} VBK {} SVBK {} \
                 speed {} interrupts taken {:?}",
                c.pc, c.sp, c.ime, c.halted, c.locked, b.ie, b.iflag, b.ppu.lcdc, b.ppu.read_stat(),
                b.ppu.ly, b.cart.rom_bank, b.vbk, b.svbk,
                if b.double_speed { "double" } else { "normal" }, b.irq_taken);
            log_line(&line);
        }
    }

    fn write_rtc_block(&mut self) {
        let rtc = &self.gb.bus.cart.rtc;
        self.rtc_block[..5].copy_from_slice(&rtc.regs);
        self.rtc_block[5..10].copy_from_slice(&rtc.latched);
        self.rtc_block[16..24].copy_from_slice(&now_secs().to_le_bytes());
    }

    /// Once, on the first frame: the frontend has written the saved block by then. The clock is
    /// moved on by the real time that passed while the game was closed.
    fn read_rtc_block(&mut self) {
        let stamp = u64::from_le_bytes(self.rtc_block[16..24].try_into().unwrap_or([0; 8]));
        if stamp == 0 {
            return;
        }
        let rtc = &mut self.gb.bus.cart.rtc;
        rtc.regs.copy_from_slice(&self.rtc_block[..5]);
        rtc.latched.copy_from_slice(&self.rtc_block[5..10]);
        rtc.advance_seconds(now_secs().saturating_sub(stamp));
    }
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
        library_name: c"cBoy".as_ptr(),
        library_version: c"0.1.0".as_ptr(),
        valid_extensions: c"gb|gbc|dmg|cgb|gba|agb".as_ptr(),
        need_fullpath: false,
        block_extract: false,
    };
}

#[no_mangle]
pub unsafe extern "C" fn retro_get_system_av_info(info: *mut RetroSystemAvInfo) {
    if info.is_null() {
        return;
    }
    let is_gba = CORE.lock().ok().is_some_and(|c| c.as_ref().is_some_and(|c| c.gba.is_some()));
    let (w, h, aspect, fps) = if is_gba {
        (gba::ppu::W as u32, gba::ppu::H as u32, 1.5f32, 16_777_216.0 / 280_896.0)
    } else {
        (W as u32, H as u32, 10.0 / 9.0, 4_194_304.0 / 70_224.0)
    };
    *info = RetroSystemAvInfo {
        geometry: RetroGameGeometry {
            base_width: w,
            base_height: h,
            max_width: gba::ppu::W as u32,
            max_height: gba::ppu::H as u32,
            aspect_ratio: aspect,
        },
        timing: RetroSystemTiming {
            fps,
            sample_rate: f64::from(crate::apu::OUT_HZ),
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
    if let Ok(mut core) = CORE.lock() {
        *core = None;
    }
}
#[no_mangle]
pub extern "C" fn retro_set_controller_port_device(_port: u32, _device: u32) {}

#[no_mangle]
pub unsafe extern "C" fn retro_load_game(game: *const RetroGameInfo) -> bool {
    if game.is_null() || (*game).data.is_null() {
        return false;
    }
    let rom = std::slice::from_raw_parts((*game).data as *const u8, (*game).size).to_vec();
    let is_gba = gba::is_gba_rom(&rom);
    let (gb, gba_machine) = if is_gba {
        let Ok(idle) = GameBoy::new(vec![0; 0x8000]) else { return false };
        (idle, Some(Box::new(Gba::new(rom))))
    } else {
        let Ok(gb) = GameBoy::new(rom) else { return false };
        (gb, None)
    };
    let env = CB.lock().ok().and_then(|c| c.env);
    if let Some(env) = env {
        let mut cb = RetroLogCallback { log: None };
        if env(ENV_GET_LOG_INTERFACE, &mut cb as *mut RetroLogCallback as *mut c_void) {
            if let Ok(mut l) = LOG.lock() {
                *l = cb.log;
            }
        }
        let mut fmt = PIXEL_FORMAT_XRGB8888;
        if !env(ENV_SET_PIXEL_FORMAT, &mut fmt as *mut u32 as *mut c_void) {
            return false;
        }
    }
    if gba_machine.is_some() {
        log_line("loaded a Game Boy Advance ROM");
    } else {
        let c = &gb.bus.cart;
        log_line(&format!("loaded \"{}\" as {}: {:?}, ROM {} KB, RAM {} KB, battery {}, clock {}, header $143 = {:02X}",
            c.title, if gb.bus.cgb { "Game Boy Color" } else { "Game Boy" }, c.kind, c.rom.len() / 1024,
            c.ram.len() / 1024, c.has_battery, c.has_rtc, c.cgb_flag));
    }
    let mut core = Core { gb, gba: gba_machine, frame: vec![0; W * H], rtc_block: [0; RTC_LEN], rtc_synced: false, stuck_frames: 0, stuck_reports: 0 };
    core.write_rtc_block();
    core.rtc_block[16..24].fill(0); // "nothing saved yet" until the frontend writes its copy
    if let Ok(mut slot) = CORE.lock() {
        *slot = Some(core);
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
    if let Ok(mut core) = CORE.lock() {
        *core = None;
    }
}

#[no_mangle]
pub extern "C" fn retro_get_region() -> u32 {
    0
}

#[no_mangle]
pub extern "C" fn retro_reset() {
    if let Ok(mut slot) = CORE.lock() {
        if let Some(core) = slot.as_mut() {
            if let Some(g) = core.gba.as_mut() {
                let rom = std::mem::take(&mut g.bus.rom);
                let save = g.bus.save.clone();
                let mut fresh = Gba::new(rom);
                fresh.bus.save = save;
                **g = fresh;
                return;
            }
            let rom = std::mem::take(&mut core.gb.bus.cart.rom);
            let ram = std::mem::take(&mut core.gb.bus.cart.ram);
            let rtc = core.gb.bus.cart.rtc.clone();
            if let Ok(mut gb) = GameBoy::new(rom) {
                if gb.bus.cart.ram.len() == ram.len() {
                    gb.bus.cart.ram = ram;
                }
                gb.bus.cart.rtc = rtc;
                core.gb = gb;
            }
        }
    }
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
    let mut buttons = 0u8;
    if let Some(input) = input {
        // libretro ids: B 0, SELECT 2, START 3, UP 4, DOWN 5, LEFT 6, RIGHT 7, A 8.
        let map = [(8u32, 0u8), (0, 1), (2, 2), (3, 3), (7, 4), (6, 5), (4, 6), (5, 7)];
        for (id, bit) in map {
            if input(0, DEVICE_JOYPAD, 0, id) != 0 {
                buttons |= 1 << bit;
            }
        }
    }
    let Ok(mut slot) = CORE.lock() else { return };
    let Some(core) = slot.as_mut() else { return };
    if let Some(g) = core.gba.as_mut() {
        let mut keys = 0u16;
        if let Some(input) = input {
            // KEYINPUT order: A B Select Start Right Left Up Down R L.
            let map = [8u32, 0, 2, 3, 7, 6, 4, 5, 11, 10];
            for (bit, id) in map.iter().enumerate() {
                if input(0, DEVICE_JOYPAD, 0, *id) != 0 {
                    keys |= 1 << bit;
                }
            }
        }
        g.set_keys(keys);
        g.run_frame();
        let samples = std::mem::take(&mut g.bus.apu.samples);
        if let Some(video) = video {
            video(g.bus.ppu.frame.as_ptr() as *const c_void, gba::ppu::W as u32,
                  gba::ppu::H as u32, gba::ppu::W * 4);
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
        let mut samples = samples;
        samples.clear();
        g.bus.apu.samples = samples;
        return;
    }
    let _ = buttons;
    if !core.rtc_synced {
        core.rtc_synced = true;
        if core.gb.bus.cart.has_rtc {
            core.read_rtc_block();
        }
    }
    core.gb.bus.set_buttons(buttons);
    let vblanks = core.gb.bus.irq_taken[0];
    core.gb.run_frame();
    core.watch_lockup(vblanks);
    for line in std::mem::take(&mut core.gb.bus.events) {
        log_line(&line);
    }
    let mut frame = std::mem::take(&mut core.frame);
    core.gb.frame_xrgb(&mut frame);
    core.frame = frame;
    if core.gb.bus.cart.has_rtc {
        core.write_rtc_block();
    }
    let samples = std::mem::take(&mut core.gb.bus.apu.samples);
    if let Some(video) = video {
        video(core.frame.as_ptr() as *const c_void, W as u32, H as u32, W * 4);
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
    let mut samples = samples;
    samples.clear();
    core.gb.bus.apu.samples = samples;
}

#[no_mangle]
pub extern "C" fn retro_serialize_size() -> usize {
    CORE.lock()
        .ok()
        .and_then(|s| s.as_ref().map(|c| match &c.gba { Some(g) => g.save_state().len(), None => c.gb.save_state().len() } + 1024))
        .unwrap_or(0)
}

#[no_mangle]
pub unsafe extern "C" fn retro_serialize(data: *mut c_void, size: usize) -> bool {
    let Ok(slot) = CORE.lock() else { return false };
    let Some(core) = slot.as_ref() else { return false };
    let state = match &core.gba { Some(g) => g.save_state(), None => core.gb.save_state() };
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
    let Some(core) = slot.as_mut() else { return false };
    match core.gba.as_mut() { Some(g) => g.load_state(bytes).is_ok(), None => core.gb.load_state(bytes).is_ok() }
}

#[no_mangle]
pub extern "C" fn retro_cheat_reset() {}
#[no_mangle]
pub extern "C" fn retro_cheat_set(_index: u32, _enabled: bool, _code: *const c_char) {}

#[no_mangle]
pub extern "C" fn retro_get_memory_data(id: u32) -> *mut c_void {
    let Ok(mut slot) = CORE.lock() else { return std::ptr::null_mut() };
    let Some(core) = slot.as_mut() else { return std::ptr::null_mut() };
    if let Some(g) = core.gba.as_mut() {
        return match id {
            MEMORY_SAVE_RAM if !g.bus.save.data.is_empty() => g.bus.save.data.as_mut_ptr() as *mut c_void,
            MEMORY_SYSTEM_RAM => g.bus.ewram.as_mut_ptr() as *mut c_void,
            MEMORY_VIDEO_RAM => g.bus.vram.as_mut_ptr() as *mut c_void,
            _ => std::ptr::null_mut(),
        };
    }
    match id {
        MEMORY_SAVE_RAM if core.gb.bus.cart.has_battery && !core.gb.bus.cart.ram.is_empty() => {
            core.gb.bus.cart.ram.as_mut_ptr() as *mut c_void
        }
        MEMORY_RTC if core.gb.bus.cart.has_rtc => core.rtc_block.as_mut_ptr() as *mut c_void,
        MEMORY_SYSTEM_RAM => core.gb.bus.wram.as_mut_ptr() as *mut c_void,
        MEMORY_VIDEO_RAM => core.gb.bus.ppu.vram.as_mut_ptr() as *mut c_void,
        _ => std::ptr::null_mut(),
    }
}

#[no_mangle]
pub extern "C" fn retro_get_memory_size(id: u32) -> usize {
    let Ok(slot) = CORE.lock() else { return 0 };
    let Some(core) = slot.as_ref() else { return 0 };
    if let Some(g) = core.gba.as_ref() {
        return match id {
            MEMORY_SAVE_RAM => g.bus.save.data.len(),
            MEMORY_SYSTEM_RAM => g.bus.ewram.len(),
            MEMORY_VIDEO_RAM => g.bus.vram.len(),
            _ => 0,
        };
    }
    match id {
        MEMORY_SAVE_RAM if core.gb.bus.cart.has_battery => core.gb.bus.cart.ram.len(),
        MEMORY_RTC if core.gb.bus.cart.has_rtc => RTC_LEN,
        MEMORY_SYSTEM_RAM => core.gb.bus.wram.len(),
        MEMORY_VIDEO_RAM => core.gb.bus.ppu.vram.len(),
        _ => 0,
    }
}
