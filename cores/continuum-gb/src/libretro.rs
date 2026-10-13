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
const DEVICE_ANALOG: u32 = 5;
const ENV_GET_SYSTEM_DIRECTORY: u32 = 9;
const ENV_GET_RUMBLE_INTERFACE: u32 = 23;
/// libretro.h: `25 | RETRO_ENVIRONMENT_EXPERIMENTAL` (0x10000). The bit is part of the number; a
/// plain 25 is answered by nobody, which left every motion cartridge with a still console.
pub const ENV_GET_SENSOR_INTERFACE: u32 = 25 | 0x10000;

/// The RTC block the frontend saves: live registers (5), latched (5), 6 spare, then the Unix
/// time (u64, little-endian) the registers were true at. HuC3 uses it differently (see
/// `write_rtc_block`).
const RTC_LEN: usize = 24;
/// T-cycles in one second of the cartridge clock.
const GB_HZ: u32 = 4_194_304;

type RumbleFn = unsafe extern "C" fn(u32, u32, u16) -> bool;
#[repr(C)]
struct RumbleInterface {
    set_rumble_state: Option<RumbleFn>,
}
type SensorStateFn = unsafe extern "C" fn(u32, u32, u32) -> bool;
type SensorInputFn = unsafe extern "C" fn(u32, u32) -> f32;
#[repr(C)]
struct SensorInterface {
    set_sensor_state: Option<SensorStateFn>,
    get_sensor_input: Option<SensorInputFn>,
}

struct Core {
    gb: GameBoy,
    /// Set when the loaded ROM is a Game Boy Advance one (then `gb` is an idle placeholder).
    gba: Option<Box<Gba>>,
    frame: Vec<u32>,
    out: Vec<u32>,
    rtc_block: [u8; RTC_LEN],
    rtc_synced: bool,
    stuck_frames: u32,
    stuck_reports: u32,
    settings: crate::options::Settings,
    post: crate::options::Post,
    system_dir: Option<String>,
    frame_no: u64,
    rumble: Option<RumbleFn>,
    sensors: Option<SensorInterface>,
    sensors_started: bool,
    last_motor: Option<bool>,
    /// Auto frame skip: the last retro_run took longer than a frame.
    behind: bool,
    /// Frames skipped in a row by auto frame skip.
    skipped_run: u32,
    buzz_phase: u32,
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

/// SNES 15-bit BGR (Super Game Boy palettes) to XRGB.
fn snes_rgb(c: u16) -> u32 {
    let ch = |v: u16| -> u32 {
        let v = u32::from(v & 0x1F);
        (v << 3) | (v >> 2)
    };
    ch(c) << 16 | ch(c >> 5) << 8 | ch(c >> 10)
}

/// Reads a file from <system>/cboy/ or <system>/.
fn system_file(dir: Option<&str>, name: &str) -> Option<Vec<u8>> {
    let dir = dir?;
    std::fs::read(format!("{dir}/cboy/{name}")).or_else(|_| std::fs::read(format!("{dir}/{name}"))).ok()
}

/// Per-frame inputs read from the frontend.
#[derive(Default, Clone, Copy)]
struct Inputs {
    gb_buttons: u8,
    gba_keys: u16,
    tilt: (f32, f32),
    turn: f32,
    light: Option<u8>,
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

    /// Fills the block the frontend saves (as `<game>.rtc`, next to the battery save).
    fn write_rtc_block(&mut self) {
        let cart = &self.gb.bus.cart;
        if cart.kind == crate::cart::MbcKind::HuC3 {
            // HuC3 counts minutes of the day and days, not MBC3's registers: bytes 0-3 hold those
            // two, byte 10 the seconds into the current minute.
            let (m, d) = (cart.huc3.minutes, cart.huc3.days);
            self.rtc_block[..4].copy_from_slice(&[m as u8, (m >> 8) as u8, d as u8, (d >> 8) as u8]);
            self.rtc_block[4..10].fill(0);
            self.rtc_block[10] = (cart.rtc.sub / GB_HZ).min(59) as u8;
        } else {
            self.rtc_block[..5].copy_from_slice(&cart.rtc.regs);
            self.rtc_block[5..10].copy_from_slice(&cart.rtc.latched);
        }
        self.rtc_block[16..24].copy_from_slice(&now_secs().to_le_bytes());
    }

    /// Once, on the first frame: the frontend has written the saved block by then. How the
    /// clock moves on depends on the clock option.
    fn read_rtc_block(&mut self) {
        let stamp = u64::from_le_bytes(self.rtc_block[16..24].try_into().unwrap_or([0; 8]));
        let mode = self.settings.rtc_mode;
        let offset = self.settings.rtc_offset_hours * 3600;
        // Real time: the clock runs on while the game is closed.
        let closed = if stamp != 0 && mode == crate::options::RtcMode::Real {
            now_secs().saturating_sub(stamp)
        } else {
            0
        };
        let cart = &mut self.gb.bus.cart;
        if cart.kind == crate::cart::MbcKind::HuC3 {
            let b = &self.rtc_block;
            if stamp != 0 {
                cart.huc3.minutes = (u16::from(b[0]) | u16::from(b[1]) << 8) % 1440;
                cart.huc3.days = u16::from(b[2]) | u16::from(b[3]) << 8;
                let secs = u64::from(b[10].min(59)) + closed;
                cart.huc3.advance_minutes(secs / 60);
                cart.rtc.sub = (secs % 60) as u32 * GB_HZ;
            }
            if mode == crate::options::RtcMode::Phone {
                let t = gba::gpio::local_time(offset);
                cart.huc3.minutes = ((t[4] * 60 + t[5]) % 1440) as u16;
            }
            return;
        }
        let rtc = &mut cart.rtc;
        if stamp != 0 {
            rtc.regs.copy_from_slice(&self.rtc_block[..5]);
            rtc.latched.copy_from_slice(&self.rtc_block[5..10]);
            rtc.advance_seconds(closed);
        }
        if mode == crate::options::RtcMode::Phone {
            let t = gba::gpio::local_time(offset);
            rtc.regs[0] = t[6].min(59) as u8;
            rtc.regs[1] = t[5] as u8;
            rtc.regs[2] = t[4] as u8;
        }
    }

    /// Settings that apply to a running machine. Called on load, on every option change and
    /// after a state is loaded (the state does not carry them).
    fn apply_live(&mut self) {
        let s = &self.settings;
        if let Some(g) = self.gba.as_mut() {
            g.bus.ppu.layers = s.gba_layers;
            g.bus.apu.chan_vol = s.chan_vol;
            g.bus.fifo_vol = s.fifo_vol;
            g.bus.fifo_smooth = s.gba_smooth;
            g.bus.gbp = s.player_handshake();
            g.bus.vba_compat = s.vba_compat;
            g.bus.gpio.clock_offset = s.rtc_offset_hours * 3600;
            g.bus.gpio.game_clock = s.gba_game_clock;
            g.idle_skip = s.idle_skip;
            g.bus.apu.out_hz = s.sample_rate;
            g.bus.apu.hq = s.audio_hq;
        } else {
            self.gb.bus.ppu.layers = s.gb_layers;
            self.gb.bus.apu.chan_vol = s.chan_vol;
            self.gb.bus.link_loopback = s.link_loopback;
            self.gb.bus.apu.out_hz = s.sample_rate;
            self.gb.bus.apu.hq = s.audio_hq;
            self.gb.bus.ppu.no_sprite_limit = !s.sprite_limit;
            // Between frames (line 144, never mid-line), so switching drawers live is safe.
            self.gb.bus.ppu.exact = s.exact_renderer;
        }
        if let Some(g) = self.gba.as_mut() {
            g.bus.ppu.obj_limit = s.gba_obj_limit;
        }
    }

    unsafe fn read_inputs(&mut self, input: Option<InputFn>) -> Inputs {
        let mut i = Inputs::default();
        let Some(input) = input else { return i };
        let s = &self.settings;
        // Index 0 is the left stick, 1 the right; axis 0 is X, 1 is Y.
        let (side, dz) = (u32::from(s.swap_stick), s.stick_deadzone);
        let stick = |id: u32| {
            let v = f32::from(input(0, DEVICE_ANALOG, side, id)) / 32768.0;
            if v.abs() <= dz { 0.0 } else { v.signum() * (v.abs() - dz) / (1.0 - dz).max(0.01) }
        };
        let (sx, sy) = if s.stick_dpad { (stick(0), stick(1)) } else { (0.0, 0.0) };
        let pad = |id: u32| input(0, DEVICE_JOYPAD, 0, id) != 0;
        let mut dirs = [pad(4) || sy < -0.5, pad(5) || sy > 0.5, pad(6) || sx < -0.5, pad(7) || sx > 0.5];
        if !s.opposing {
            if dirs[0] && dirs[1] { dirs[0] = false; dirs[1] = false; }
            if dirs[2] && dirs[3] { dirs[2] = false; dirs[3] = false; }
        }
        let held = |id: u32| match id { 4..=7 => dirs[(id - 4) as usize], _ => pad(id) };
        let turbo_on = s.turbo_period > 0 && self.frame_no % u64::from(s.turbo_period) < u64::from(s.turbo_period / 2);
        let a = held(8) || (s.turbo_period > 0 && held(9) && turbo_on);
        let b = held(0) || (s.turbo_period > 0 && held(1) && turbo_on);
        // Game Boy: A B Select Start Right Left Up Down.
        let gb = [a, b, held(2), held(3), held(7), held(6), held(4), held(5)];
        for (bit, on) in gb.iter().enumerate() {
            i.gb_buttons |= u8::from(*on) << bit;
        }
        // KEYINPUT: A B Select Start Right Left Up Down R L.
        let gba = [a, b, held(2), held(3), held(7), held(6), held(4), held(5), held(11), held(10)];
        for (bit, on) in gba.iter().enumerate() {
            i.gba_keys |= u16::from(*on) << bit;
        }
        let sensors = self.sensors.as_ref();
        let read = |id: u32| sensors.and_then(|x| x.get_sensor_input).map_or(0.0, |f| f(0, id));
        i.tilt = match s.tilt {
            crate::options::Source::Phone => (read(0).clamp(-1.0, 1.0), read(1).clamp(-1.0, 1.0)),
            crate::options::Source::Stick => (stick(0), stick(1)),
            crate::options::Source::Off => (0.0, 0.0),
        };
        i.turn = match s.gyro {
            crate::options::Source::Phone => (read(5) / 4.0).clamp(-1.0, 1.0),
            crate::options::Source::Stick => stick(0),
            crate::options::Source::Off => 0.0,
        };
        let k = s.motion_scale;
        i.tilt = ((i.tilt.0 * k).clamp(-1.0, 1.0), (i.tilt.1 * k).clamp(-1.0, 1.0));
        i.turn = (i.turn * k).clamp(-1.0, 1.0);
        i.light = match s.solar {
            Some(level) => Some(level),
            // Lux to 0-255: full sun saturates well before daylight levels.
            None => Some((read(6) / 4000.0 * 255.0).clamp(0.0, 255.0) as u8),
        };
        i
    }

    /// Turns the phone's sensors on (or off again) for what the cartridge has and the options
    /// ask for. On the first frame, and again after any option change, so switching Tilt input to
    /// Phone motion mid-game starts the motion sensor (`sensors_started` is cleared then).
    unsafe fn start_sensors(&mut self) {
        if self.sensors_started {
            return;
        }
        self.sensors_started = true;
        let Some(set) = self.sensors.as_ref().and_then(|s| s.set_sensor_state) else { return };
        let s = &self.settings;
        let phone = crate::options::Source::Phone;
        let has_tilt = self.gb.bus.cart.kind == crate::cart::MbcKind::Mbc7
            || self.gba.as_ref().is_some_and(|g| g.bus.gpio.hw == gba::gpio::Hardware::Tilt);
        let has_gyro = self.gba.as_ref().is_some_and(|g| g.bus.gpio.hw == gba::gpio::Hardware::Gyro);
        let has_light = self.gba.as_ref().is_some_and(|g| g.bus.gpio.hw == gba::gpio::Hardware::Solar);
        // libretro sensor actions: 0/1 accelerometer on/off, 2/3 gyroscope, 4/5 illuminance.
        if has_tilt {
            if s.tilt == phone { set(0, 0, 60) } else { set(0, 1, 0) };
        }
        if has_gyro {
            if s.gyro == phone { set(0, 2, 60) } else { set(0, 3, 0) };
        }
        if has_light {
            if s.solar.is_none() { set(0, 4, 10) } else { set(0, 5, 0) };
        }
    }

    fn feed(&mut self, i: &Inputs) {
        if let Some(g) = self.gba.as_mut() {
            g.set_keys(i.gba_keys);
            g.bus.gpio.tilt = i.tilt;
            g.bus.gpio.gyro_rate = i.turn;
            if let Some(l) = i.light {
                g.bus.gpio.light = l;
            }
        } else {
            self.gb.bus.set_buttons(i.gb_buttons);
            self.gb.bus.cart.tilt = i.tilt;
        }
    }

    fn snapshot(&self) -> Vec<u8> {
        match &self.gba { Some(g) => g.save_state(), None => self.gb.save_state() }
    }

    fn restore(&mut self, s: &[u8]) {
        let _ = match self.gba.as_mut() { Some(g) => g.load_state(s), None => self.gb.load_state(s) };
        self.apply_live();
    }

    /// Runs one frame, drawing it or not.
    fn run_one(&mut self, draw: bool) {
        if let Some(g) = self.gba.as_mut() {
            g.bus.ppu.no_render = !draw;
            g.run_frame();
        } else {
            self.gb.bus.ppu.no_render = !draw;
            self.gb.run_frame();
        }
    }

    fn motor(&self) -> bool {
        match &self.gba {
            Some(g) => g.bus.gpio.motor || g.bus.gbp_motor,
            None => self.gb.bus.cart.motor,
        }
    }

    /// Builds the output picture from the machine's frame. Returns (width, height).
    fn compose(&mut self) -> (usize, usize) {
        use crate::options::Machine;
        let title = self.gb.bus.cart.title.clone();
        if let Some(g) = self.gba.as_ref() {
            self.out.clear();
            self.out.extend_from_slice(&g.bus.ppu.frame);
            self.post.video(&mut self.out, &[], &[], &self.settings, Machine::Gba, &title);
            return (gba::ppu::W, gba::ppu::H);
        }
        self.frame.resize(W * H, 0);
        self.gb.frame_xrgb(&mut self.frame);
        let ppu = &self.gb.bus.ppu;
        if let Some(sgb) = self.gb.bus.sgb.as_ref() {
            sgb.colour(&ppu.shade, &mut self.frame, snes_rgb);
            self.post.blend(&mut self.frame, self.settings.blend);
            if self.settings.sgb_border {
                sgb.compose(&self.frame, &mut self.out, snes_rgb);
                return (crate::sgb::BW, crate::sgb::BH);
            }
            self.out.clear();
            self.out.extend_from_slice(&self.frame);
            return (W, H);
        }
        let machine = if self.gb.bus.cgb { Machine::Cgb } else { Machine::Dmg };
        let (shade, source) = (ppu.shade.clone(), ppu.source.clone());
        self.post.video(&mut self.frame, &shade, &source, &self.settings, machine, &title);
        self.out.clear();
        if self.settings.border_always {
            // A plain frame in the darkest colour of the picture's palette.
            self.out.resize(crate::sgb::BW * crate::sgb::BH, 0x0010_1010);
            for y in 0..H {
                let row = (crate::sgb::GY + y) * crate::sgb::BW + crate::sgb::GX;
                self.out[row..row + W].copy_from_slice(&self.frame[y * W..(y + 1) * W]);
            }
            return (crate::sgb::BW, crate::sgb::BH);
        }
        self.out.extend_from_slice(&self.frame);
        (W, H)
    }

    fn take_samples(&mut self) -> Vec<i16> {
        match self.gba.as_mut() {
            Some(g) => std::mem::take(&mut g.bus.apu.samples),
            None => std::mem::take(&mut self.gb.bus.apu.samples),
        }
    }

    fn give_samples(&mut self, mut v: Vec<i16>) {
        v.clear();
        match self.gba.as_mut() {
            Some(g) => g.bus.apu.samples = v,
            None => self.gb.bus.apu.samples = v,
        }
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
        // Carries the save-state format: the app refuses a state whose recorded version differs, so a
        // state from a build with another layout is never loaded as garbage.
        library_version: c"0.3.0 (state format 3)".as_ptr(),
        valid_extensions: c"gb|gbc|dmg|cgb|sgb|gba|agb".as_ptr(),
        need_fullpath: false,
        block_extract: false,
    };
}

#[no_mangle]
pub unsafe extern "C" fn retro_get_system_av_info(info: *mut RetroSystemAvInfo) {
    if info.is_null() {
        return;
    }
    let (is_gba, sgb_border, rate, sgb_clock) = CORE.lock().ok().and_then(|c| c.as_ref().map(|c| {
        // A Super Game Boy runs from the SNES clock: about 2.4% fast (NTSC) or 1.5% (PAL).
        let clock = match (c.gb.bus.sgb.is_some(), c.settings.sgb_pal) {
            (true, false) => 21_477_272.0 / 5.0,
            (true, true) => 21_281_370.0 / 5.0,
            _ => 4_194_304.0,
        };
        (c.gba.is_some(), (c.gb.bus.sgb.is_some() && c.settings.sgb_border) || c.settings.border_always, c.settings.sample_rate, clock)
    })).unwrap_or((false, false, crate::apu::OUT_HZ, 4_194_304.0));
    let (w, h, aspect, fps) = if is_gba {
        (gba::ppu::W as u32, gba::ppu::H as u32, 1.5f32, 16_777_216.0 / 280_896.0)
    } else if sgb_border {
        (crate::sgb::BW as u32, crate::sgb::BH as u32, 8.0 / 7.0, sgb_clock / 70_224.0)
    } else {
        (W as u32, H as u32, 10.0 / 9.0, sgb_clock / 70_224.0)
    };
    *info = RetroSystemAvInfo {
        geometry: RetroGameGeometry {
            base_width: w,
            base_height: h,
            max_width: crate::sgb::BW as u32,
            max_height: crate::sgb::BH as u32,
            aspect_ratio: aspect,
        },
        timing: RetroSystemTiming { fps, sample_rate: f64::from(rate) },
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
    if let Ok(mut core) = CORE.lock() {
        *core = None;
    }
}
#[no_mangle]
pub extern "C" fn retro_set_controller_port_device(_port: u32, _device: u32) {}

/// Builds the machine the options ask for.
fn build_machine(rom: Vec<u8>, s: &crate::options::Settings, dir: Option<&str>) -> Option<(GameBoy, Option<Box<Gba>>)> {
    use crate::gb::Model;
    if gba::is_gba_rom(&rom) {
        let idle = GameBoy::new(vec![0; 0x8000]).ok()?;
        let mut g = None;
        if s.gba_bios_file {
            match system_file(dir, "gba_bios.bin") {
                Some(bios) => match Gba::with_bios(rom.clone(), bios) {
                    Ok(mut m) => {
                        if s.gba_bios_skip_logo {
                            m.cpu = gba::cpu::Cpu::default();
                            log_line("using the Game Boy Advance BIOS file, skipping its logo");
                        } else {
                            log_line("starting from the Game Boy Advance BIOS file");
                        }
                        g = Some(m);
                    }
                    Err(e) => log_line(&format!("BIOS file not used: {e}")),
                },
                None => log_line("no gba_bios.bin in the system folder; using the built-in BIOS"),
            }
        }
        let mut g = Box::new(g.unwrap_or_else(|| Gba::new(rom)));
        if let Some((kind, bits)) = s.gba_save {
            g.bus.save = gba::save::Save::new(kind);
            g.bus.save.eeprom_addr_bits = bits;
            log_line(&format!("save chip set by option: {kind:?}"));
        }
        if let Some(hw) = s.gba_hardware {
            g.bus.gpio = gba::gpio::Gpio::new(hw);
        }
        if g.bus.gpio.hw != gba::gpio::Hardware::None {
            log_line(&format!("cartridge extras: {:?}", g.bus.gpio.hw));
        }
        return Some((idle, Some(g)));
    }
    let mut rom = rom;
    if let Some(why) = crate::cart::cutoff_reason(&rom) {
        log_line(&why);
        return None;
    }
    if s.force_color && rom.len() > 0x143 && rom[0x143] & 0x80 == 0 && !matches!(s.model, Model::Dmg | Model::Dmg0 | Model::Mgb | Model::Sgb | Model::Sgb2) {
        rom[0x143] = 0x80;
        log_line("Game Boy cartridge run in Color mode (option)");
    }
    let cart = crate::cart::Cart::new(rom.clone()).ok()?;
    let colour = cart.cgb_flag & 0x80 != 0;
    // Header byte 0x146 = 3 marks Super Game Boy support (needs the old licensee code 0x33).
    let sgb_cart = rom.get(0x146) == Some(&3) && rom.get(0x14B) == Some(&0x33);
    let model = s.console_for(cart.cgb_flag, sgb_cart);
    let boot = if s.boot_rom {
        let name = match model {
            Model::Dmg0 => "dmg0_boot.bin",
            Model::Mgb => "mgb_boot.bin",
            Model::Sgb => "sgb_boot.bin",
            Model::Sgb2 => "sgb2_boot.bin",
            Model::Cgb | Model::Agb if colour => "cgb_boot.bin",
            _ => "dmg_boot.bin",
        };
        let f = system_file(dir, name);
        if f.is_none() {
            log_line(&format!("no {name} in the system folder, so the game simply starts"));
        }
        f
    } else {
        None
    };
    let had_boot = boot.is_some();
    let mut gb = match GameBoy::with_model(rom.clone(), model, boot) {
        Ok(gb) => gb,
        Err(e) => {
            log_line(&format!("boot ROM not used: {e}"));
            GameBoy::with_model(rom, model, None).ok()?
        }
    };
    if s.cgb_rev0 && !had_boot {
        gb.set_cgb_revision_0();
    }
    Some((gb, None))
}

#[no_mangle]
pub unsafe extern "C" fn retro_load_game(game: *const RetroGameInfo) -> bool {
    if game.is_null() || (*game).data.is_null() {
        return false;
    }
    let rom = std::slice::from_raw_parts((*game).data as *const u8, (*game).size).to_vec();
    let env = CB.lock().ok().and_then(|c| c.env);
    let mut system_dir = None;
    let mut rumble = None;
    let mut sensors = None;
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
        let mut dir: *const c_char = std::ptr::null();
        if env(ENV_GET_SYSTEM_DIRECTORY, &mut dir as *mut *const c_char as *mut c_void) && !dir.is_null() {
            system_dir = Some(std::ffi::CStr::from_ptr(dir).to_string_lossy().into_owned());
        }
        let mut r = RumbleInterface { set_rumble_state: None };
        if env(ENV_GET_RUMBLE_INTERFACE, &mut r as *mut RumbleInterface as *mut c_void) {
            rumble = r.set_rumble_state;
        }
        let mut si = SensorInterface { set_sensor_state: None, get_sensor_input: None };
        if env(ENV_GET_SENSOR_INTERFACE, &mut si as *mut SensorInterface as *mut c_void) {
            sensors = Some(si);
        }
    }
    let settings = env.map(|e| crate::options::read(e, system_dir.as_deref())).unwrap_or_default();
    let Some((gb, gba_machine)) = build_machine(rom, &settings, system_dir.as_deref()) else { return false };
    if gba_machine.is_some() {
        log_line("loaded a Game Boy Advance ROM");
    } else {
        let c = &gb.bus.cart;
        log_line(&format!("loaded \"{}\" as {}{}: {:?}, ROM {} KB, RAM {} KB, battery {}, clock {}, header $143 = {:02X}",
            c.title, if gb.bus.cgb { "Game Boy Color" } else { "Game Boy" },
            if gb.bus.sgb.is_some() { " (Super Game Boy)" } else { "" }, c.kind,
            c.file_len.max(1) / 1024,
            c.ram.len() / 1024, c.has_battery, c.has_rtc, c.cgb_flag));
    }
    let mut core = Core {
        gb, gba: gba_machine, frame: vec![0; W * H], out: Vec::new(), rtc_block: [0; RTC_LEN], rtc_synced: false,
        stuck_frames: 0, stuck_reports: 0, settings, post: crate::options::Post::default(), system_dir,
        frame_no: 0, rumble, sensors, sensors_started: false, last_motor: None, behind: false, skipped_run: 0, buzz_phase: 0,
    };
    core.apply_live();
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
            let dir = core.system_dir.clone();
            if let Some(g) = core.gba.as_mut() {
                let rom = std::mem::take(&mut g.bus.rom);
                let save = g.bus.save.clone();
                let gpio_hw = g.bus.gpio.hw;
                if let Some((_, Some(mut fresh))) = build_machine(rom, &core.settings, dir.as_deref()) {
                    fresh.bus.save = save;
                    fresh.bus.gpio = gba::gpio::Gpio::new(gpio_hw);
                    *g = fresh;
                }
            } else {
                let rom = std::mem::take(&mut core.gb.bus.cart.rom);
                let ram = std::mem::take(&mut core.gb.bus.cart.ram);
                let rtc = core.gb.bus.cart.rtc.clone();
                // HuC3's clock and clock memory too: Restart is not a new cartridge.
                let huc3 = core.gb.bus.cart.huc3.clone();
                if let Some((mut gb, None)) = build_machine(rom, &core.settings, dir.as_deref()) {
                    if gb.bus.cart.ram.len() == ram.len() {
                        gb.bus.cart.ram = ram;
                    }
                    gb.bus.cart.rtc = rtc;
                    gb.bus.cart.huc3 = huc3;
                    core.gb = gb;
                }
            }
            core.apply_live();
        }
    }
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
    let Some(core) = slot.as_mut() else { return };
    if let Some(env) = env {
        if crate::options::changed(env) {
            core.settings = crate::options::read(env, core.system_dir.as_deref());
            core.apply_live();
            // The tilt, turn or sunlight source may have changed: ask for the sensors again.
            core.sensors_started = false;
        }
    }
    core.start_sensors();
    let inputs = core.read_inputs(input);
    core.feed(&inputs);
    if core.gba.is_none() && !core.rtc_synced {
        core.rtc_synced = true;
        if core.gb.bus.cart.has_rtc {
            core.read_rtc_block();
        }
    }
    let s = core.settings.clone();
    let started = std::time::Instant::now();
    let skip = (s.frameskip > 0 && core.frame_no % u64::from(s.frameskip + 1) != 0)
        || (s.frameskip_auto && core.behind && core.skipped_run < s.frameskip_interval);
    core.skipped_run = if skip { core.skipped_run + 1 } else { 0 };
    core.frame_no += 1;
    let vblanks = core.gb.bus.irq_taken[0];
    let ahead = s.run_ahead;
    // The real frame. With run-ahead it is never drawn: the picture comes from the frames ahead.
    core.run_one(ahead == 0 && !skip);
    if core.gba.is_none() {
        core.watch_lockup(vblanks);
        for line in std::mem::take(&mut core.gb.bus.events) {
            log_line(&line);
        }
        if core.gb.bus.cart.has_rtc {
            core.write_rtc_block();
        }
    }
    let mut samples = core.take_samples();
    let motor = core.motor();
    if ahead > 0 && !skip {
        let saved = core.snapshot();
        for n in 0..ahead {
            core.feed(&inputs);
            core.run_one(n + 1 == ahead);
            let junk = core.take_samples();
            core.give_samples(junk);
        }
        let _ = core.compose();
        let shown = std::mem::take(&mut core.out);
        core.restore(&saved);
        core.out = shown;
    }
    // Rumble on change only.
    if core.last_motor != Some(motor) {
        core.last_motor = Some(motor);
        if let Some(r) = core.rumble {
            let strength = if motor { (u32::from(s.rumble_strength) * 0xFFFF / 100) as u16 } else { 0 };
            r(0, 0, strength);
        }
    }
    if s.interference > 0 && core.gba.is_none() && core.gb.bus.ppu.lcdc & 0x80 != 0 {
        // A faint buzz at the line rate (about 9.2 kHz) and its frame-rate hum.
        let amp = i32::from(s.interference) * 6;
        for pair in samples.chunks_mut(2) {
            core.buzz_phase = core.buzz_phase.wrapping_add(1);
            let line = (u64::from(core.buzz_phase) * 9198 * 2 / u64::from(s.sample_rate)) % 2 == 0;
            let v = if line { amp } else { -amp };
            for x in pair.iter_mut() {
                *x = (i32::from(*x) + v).clamp(-32768, 32767) as i16;
            }
        }
    }
    core.post.audio(&mut samples, &s, s.sample_rate);
    if let Some(video) = video {
        if skip {
            video(std::ptr::null(), 0, 0, 0);
        } else {
            let (w, h) = if ahead > 0 {
                match core.out.len() {
                    n if n == crate::sgb::BW * crate::sgb::BH => (crate::sgb::BW, crate::sgb::BH),
                    n if n == gba::ppu::W * gba::ppu::H => (gba::ppu::W, gba::ppu::H),
                    _ => (W, H),
                }
            } else {
                core.compose()
            };
            video(core.out.as_ptr() as *const c_void, w as u32, h as u32, w * 4);
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
    core.give_samples(samples);
    // Auto frame skip: behind when emulating took most of a 60 Hz frame.
    core.behind = started.elapsed() > std::time::Duration::from_micros(16_667 * u64::from(s.frameskip_threshold) / 100);
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
    let ok = match core.gba.as_mut() { Some(g) => g.load_state(bytes).is_ok(), None => core.gb.load_state(bytes).is_ok() };
    core.apply_live();
    if ok {
        // No ghost of the picture from before the load.
        core.post.blend(&mut [], 0);
    }
    ok
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
