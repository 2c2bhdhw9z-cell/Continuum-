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
    /// When the game was loaded, and whether it has sent a picture to the screen yet (an EFB
    /// copy to the XFB). Until it has, after `NOTICE_AFTER`, the status screen is shown instead
    /// of a black picture (notice.rs).
    started: std::time::Instant,
    pictured: bool,
    /// Where the CPU was at the end of each field (by 256-byte block), while there is no picture.
    hot: std::collections::HashMap<u32, u32>,
    /// The status screen as last drawn, and when: it is redrawn every `NOTICE_EVERY`, so the
    /// numbers hold still long enough to read and screenshot (owner: they changed too fast).
    notice_frame: Vec<u32>,
    notice_at: Option<std::time::Instant>,
    /// The speed read-out, when it is on (the `cgc_speed_readout` option).
    readout: Option<Readout>,
    /// The always-on telemetry line (`Tel`).
    tel: Option<Tel>,
    /// The read-out panel was drawn onto `frame`: the next field converts the picture again even
    /// if the game's picture did not change, so a switched-off panel does not stay on screen.
    overlaid: bool,
}

/// The speed read-out: every second, how fast the game ran and where the time went, drawn in
/// the corner of the picture (the lines are only for working out what to make faster).
struct Readout {
    since: std::time::Instant,
    fields: u64,
    /// Wall time spent running the machine (in `run_field`).
    run: std::time::Duration,
    start: Counters,
    lines: Vec<String>,
}

/// The machine's running totals at the start of a second.
#[derive(Clone, Copy, Default)]
struct Counters {
    cycles: u64,
    idle: u64,
    jit: u64,
    draw: std::time::Duration,
    wait: std::time::Duration,
    sound: std::time::Duration,
}

impl Counters {
    fn of(m: &Machine) -> Counters {
        let f = m.fast.as_ref();
        Counters {
            cycles: m.cpu.cycles,
            idle: f.map_or(0, |f| f.stats.idle_cycles),
            jit: f.map_or(0, |f| f.stats.jit_instructions),
            draw: m.hw.gx.stats.draw_time,
            wait: m.hw.gx.stats.wait_time,
            sound: m.hw.dsp_time,
        }
    }
}

impl Readout {
    fn new(m: &mut Machine) -> Readout {
        m.hot = Some(Default::default());
        Readout { since: std::time::Instant::now(), fields: 0, run: std::time::Duration::ZERO, start: Counters::of(m), lines: vec!["SPEED ...".into()] }
    }

    /// After a field that took `run` to emulate; once a second the lines are worked out again.
    fn field(&mut self, m: &mut Machine, run: std::time::Duration) {
        self.fields += 1;
        self.run += run;
        let secs = self.since.elapsed().as_secs_f64();
        if secs < 1.0 {
            return;
        }
        let now = Counters::of(m);
        let emulated = now.cycles.saturating_sub(self.start.cycles).max(1);
        let busy = emulated.saturating_sub(now.idle.saturating_sub(self.start.idle));
        let jit = now.jit.saturating_sub(self.start.jit);
        let pct = |d: std::time::Duration| (100.0 * d.as_secs_f64() / secs).round() as i64;
        let (draw, wait, sound) = (now.draw - self.start.draw, now.wait - self.start.wait, now.sound - self.start.sound);
        let cpu = self.run.saturating_sub(draw + wait + sound);
        let app = std::time::Duration::from_secs_f64(secs).saturating_sub(self.run);
        let mut lines = vec![
            format!("SPEED {}%", (100.0 * self.fields as f64 / secs / FPS).round() as i64),
            format!("CPU {}M LOAD {}%", (busy as f64 / secs / 1e6).round() as i64, 100 * busy / emulated),
        ];
        if m.fast.as_ref().is_some_and(|f| f.jit.is_some()) {
            lines.push(format!("JIT {}%", 100 * jit / busy.max(1)));
        }
        lines.push(format!("CPU {} DRAW {}", pct(cpu), pct(draw)));
        lines.push(format!("WAIT {} SND {} APP {}", pct(wait), pct(sound), pct(app)));
        if let Some(hot) = m.hot.as_mut() {
            let total: u32 = hot.values().sum::<u32>().max(1);
            let mut v: Vec<(u32, u32)> = hot.drain().collect();
            v.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
            for (pc, n) in v.into_iter().take(5) {
                lines.push(format!("{pc:08X} {}", 100 * n / total));
            }
        }
        self.lines = lines;
        self.since = std::time::Instant::now();
        self.fields = 0;
        self.run = std::time::Duration::ZERO;
        self.start = now;
    }
}

/// The always-on telemetry: one `cGC:` line a second into the app's core log (which the
/// diagnostics upload carries), saying what the machine did and where the time went. Owner,
/// 25 October 2026: "add all the telemetry".
struct Tel {
    since: std::time::Instant,
    seconds: u64,
    fields: u64,
    run: std::time::Duration,
    run_max: std::time::Duration,
    start: Counters,
    hw: crate::hw::HwTel,
    lines: u64,
    gx: (u64, u64, u64, u64, u64, u64),
    fast: (u64, u64, u64, u64, [u64; 6]),
    jit: (u64, u64, u64, u64),
}

impl Tel {
    fn snapshot(m: &Machine) -> (crate::hw::HwTel, u64, (u64, u64, u64, u64, u64, u64), (u64, u64, u64, u64, [u64; 6]), (u64, u64, u64, u64)) {
        let g = &m.hw.gx.stats;
        let f = m.fast.as_ref();
        let fast = f.map_or((0, 0, 0, 0, [0; 6]), |f| {
            (f.stats.blocks_built, f.stats.invalidations, f.stats.interp_instructions, f.stats.idle_skips, f.stats.not_compiled)
        });
        let jit = f.and_then(|f| f.jit.as_ref()).map_or((0, 0, 0, 0), |j| {
            (j.stats.compiled, j.stats.resets, j.stats.native_instructions, j.stats.handler_instructions)
        });
        (m.hw.tel, m.hw.vi_line_count(), (g.triangles, g.pixels, g.efb_copies, g.xfb_copies, g.display_lists, g.commands), fast, jit)
    }

    fn new(m: &Machine) -> Tel {
        let (hw, lines, gx, fast, jit) = Self::snapshot(m);
        Tel {
            since: std::time::Instant::now(),
            seconds: 0,
            fields: 0,
            run: std::time::Duration::ZERO,
            run_max: std::time::Duration::ZERO,
            start: Counters::of(m),
            hw,
            lines,
            gx,
            fast,
            jit,
        }
    }

    fn field(&mut self, m: &mut Machine, run: std::time::Duration, drain_hot: bool) {
        self.fields += 1;
        self.run += run;
        self.run_max = self.run_max.max(run);
        let secs = self.since.elapsed().as_secs_f64();
        if secs < 1.0 {
            return;
        }
        self.seconds += 1;
        let now = Counters::of(m);
        let (hw, lines, gx, fast, jit) = Self::snapshot(m);
        let ms = |d: std::time::Duration| (d.as_secs_f64() * 1000.0 / secs).round() as i64;
        let (draw, wait, sound) = (now.draw - self.start.draw, now.wait - self.start.wait, now.sound - self.start.sound);
        let picture = hw.picture_time.saturating_sub(self.hw.picture_time);
        let cpu = self.run.saturating_sub(draw + wait + sound);
        let emulated = now.cycles.saturating_sub(self.start.cycles).max(1);
        let idle = now.idle.saturating_sub(self.start.idle);
        let busy = emulated.saturating_sub(idle);
        let jit_n = now.jit.saturating_sub(self.start.jit);
        let interp = fast.2.saturating_sub(self.fast.2);
        let new_pics = hw.pictures_new - self.hw.pictures_new;
        let same_pics = hw.pictures_same - self.hw.pictures_same;
        let dma_on = hw.dma_frames - self.hw.dma_frames;
        let dma_off = hw.dma_silent - self.hw.dma_silent;
        let dma_32 = hw.dma_32k - self.hw.dma_32k;
        let stream = hw.stream_frames - self.hw.stream_frames;
        let total_dma = (dma_on + dma_off).max(1);
        let handler = jit.3 - self.jit.3;
        let native = jit.2 - self.jit.2;
        let nc: Vec<u64> = (0..6).map(|k| fast.4[k] - self.fast.4[k]).collect();
        let hot = m.hot.as_mut().map(|h| {
            let total: u32 = h.values().sum::<u32>().max(1);
            let mut v: Vec<(u32, u32)> = h.iter().map(|(a, n)| (*a, *n)).collect();
            v.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
            let text: Vec<String> = v.iter().take(6).map(|(pc, n)| format!("{pc:08X} {}%", 100 * n / total)).collect();
            if drain_hot {
                h.clear();
            }
            text.join(", ")
        });
        log_line(&format!(
            "second {}: SCREEN {} new pictures/s ({} repeats); {} fields ({}% speed), {} lines; \
             run {} ms/s (worst field {:.1} ms) = cpu {} + draw {} + wait {} + sound {} + picture {}; \
             gekko {:.0}M cycles/s, {}% idle-skipped, jit {}% of run instructions (compiled {} blocks, \
             {} resets, {}% of compiled ops via handlers), interpreted {:.1}M; blocks built {}, \
             invalidated {}, not compiled {:?}; gx {} tris, {:.1}M px, {} efb copies, {} xfb copies, \
             {} display lists, {} commands; sound dma on {}%, 32 kHz {}%, stream {}; disc {} reads \
             {} KB ({} errors); aram {} dmas {} KB; hot {}",
            self.seconds,
            new_pics,
            same_pics,
            self.fields,
            (100.0 * self.fields as f64 / secs / FPS).round() as i64,
            lines - self.lines,
            ms(self.run),
            self.run_max.as_secs_f64() * 1000.0,
            ms(cpu),
            ms(draw),
            ms(wait),
            ms(sound),
            ms(picture),
            emulated as f64 / secs / 1e6,
            100 * idle / emulated,
            100 * jit_n / busy.max(1),
            jit.0 - self.jit.0,
            jit.1 - self.jit.1,
            100 * handler / (handler + native).max(1),
            interp as f64 / secs / 1e6,
            fast.0 - self.fast.0,
            fast.1 - self.fast.1,
            nc,
            gx.0 - self.gx.0,
            (gx.1 - self.gx.1) as f64 / 1e6,
            gx.2 - self.gx.2,
            gx.3 - self.gx.3,
            gx.4 - self.gx.4,
            gx.5 - self.gx.5,
            100 * dma_on / total_dma,
            100 * dma_32 / total_dma,
            stream,
            hw.disc_reads - self.hw.disc_reads,
            (hw.disc_bytes - self.hw.disc_bytes) / 1024,
            hw.disc_errors - self.hw.disc_errors,
            hw.aram_dmas - self.hw.aram_dmas,
            (hw.aram_bytes - self.hw.aram_bytes) / 1024,
            hot.unwrap_or_default(),
        ));
        let seconds = self.seconds;
        *self = Tel::new(m);
        self.seconds = seconds;
    }
}

/// How long a game may show nothing before the status screen replaces the black picture.
const NOTICE_AFTER: std::time::Duration = std::time::Duration::from_secs(15);
/// How often the status screen's numbers are brought up to date.
const NOTICE_EVERY: std::time::Duration = std::time::Duration::from_secs(5);

impl Core {
    fn new(machine: Machine) -> Core {
        Core {
            machine,
            frame: vec![0; 640 * 480],
            size: (640, 480),
            started: std::time::Instant::now(),
            pictured: false,
            hot: Default::default(),
            notice_frame: Vec::new(),
            notice_at: None,
            readout: None,
            tel: None,
            overlaid: false,
        }
    }

    /// Turns the speed read-out on or off.
    fn set_readout(&mut self, on: bool) {
        if on && self.readout.is_none() {
            self.readout = Some(Readout::new(&mut self.machine));
        } else if !on {
            // The hot-block sampling stays on: the always-on telemetry line uses it too.
            self.readout = None;
        }
    }

    /// After a field: the status screen while the game has shown nothing for a while.
    fn notice(&mut self) {
        if self.pictured {
            return;
        }
        if self.machine.hw.gx.stats.xfb_copies > 0 {
            self.pictured = true;
            self.hot = Default::default();
            return;
        }
        if self.hot.len() < 4096 {
            *self.hot.entry(self.machine.cpu.pc & !0xFF).or_default() += 1;
        }
        let waited = self.started.elapsed();
        if waited < NOTICE_AFTER {
            return;
        }
        if self.notice_at.is_none_or(|t| t.elapsed() >= NOTICE_EVERY) {
            let mut hot: Vec<(u32, u32)> = self.hot.iter().map(|(a, n)| (*a, *n)).collect();
            hot.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
            let lines = self.machine.status_lines(waited.as_secs(), &hot);
            self.notice_frame.resize(640 * 480, 0);
            crate::notice::draw(&mut self.notice_frame, 640, 480, &lines);
            self.notice_at = Some(std::time::Instant::now());
        }
        // The game's own (black) picture is replaced every field; the screen stays as drawn.
        self.frame.clear();
        self.frame.extend_from_slice(&self.notice_frame);
        self.size = (640, 480);
    }
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
    // SAFETY: the frontend's environment callback.
    unsafe { crate::options::declare(f) };
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

/// libretro's log callback (printf-style, variadic).
type LogFn = unsafe extern "C" fn(level: u32, fmt: *const c_char, ...);
#[repr(C)]
struct RetroLogCallback {
    log: Option<LogFn>,
}
const ENV_GET_LOG_INTERFACE: u32 = 27;
static LOG: Mutex<Option<LogFn>> = Mutex::new(None);

/// One line to the frontend's log, prefixed "cGC: ". The app shows the last such line as the
/// reason when a game does not start, so a refusal is always a sentence.
fn log_line(line: &str) {
    let Some(log) = LOG.lock().ok().and_then(|l| *l) else { return };
    let Ok(text) = std::ffi::CString::new(format!("cGC: {line}\n").replace('\0', " ")) else { return };
    // SAFETY: the frontend's log callback, with a "%s" format and one C string.
    unsafe { log(1, c"%s".as_ptr(), text.as_ptr()) };
}

/// Why a game did not start, for the player: what failed in plain words, then the technical
/// detail for whoever fixes it.
pub fn refusal(detail: &str) -> String {
    let lower = detail.to_ascii_lowercase();
    let what = if lower.contains("wii disc") {
        "Wii games don't play yet."
    } else if lower.contains("not a gamecube") || lower.contains("no apploader") {
        "this file isn't a GameCube game, or the copy is damaged."
    } else if lower.contains("apploader") {
        "this game's start-up program stopped."
    } else {
        "this file could not be read."
    };
    format!("Continuum GC could not start the game: {what} Details for Kiro: {detail}")
}

/// An option read through the frontend's environment callback (false when there is none).
fn read_option(f: unsafe fn(EnvFn) -> bool) -> bool {
    let env = cb().env;
    // SAFETY: the frontend's environment callback.
    env.is_some_and(|e| unsafe { f(e) })
}

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
            let mut c = Core::new(machine);
            c.set_readout(read_option(crate::options::speed_readout));
            *core() = Some(c);
            true
        }
        Err(e) => {
            log_line(&refusal(&e));
            false
        }
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
    let env = cb().env;
    if let Some(env) = env {
        // SAFETY: the frontend's environment callback.
        if unsafe { crate::options::changed(env) } {
            core.set_readout(unsafe { crate::options::speed_readout(env) });
        }
    }
    let t = std::time::Instant::now();
    core.machine.run_one_field();
    let ran = t.elapsed();
    // Converted only when the TV picture changed (`vi_picture_if_new`); otherwise the app is told
    // it is the same picture (a dupe) and keeps showing it, unless something is drawn over it.
    let same = match core.machine.hw.vi_picture_if_new() {
        crate::hw::Picture::New(w, h, px) => {
            core.frame = px;
            core.size = (w, h);
            false
        }
        crate::hw::Picture::Same if core.overlaid && core.readout.is_none() => {
            // The panel was drawn over this picture and is off now: the clean picture again.
            if let Some((w, h, px)) = core.machine.hw.vi_picture() {
                core.frame = px;
                core.size = (w, h);
            }
            core.overlaid = false;
            false
        }
        crate::hw::Picture::Same => true,
        crate::hw::Picture::Off => false,
    };
    core.notice();
    if core.tel.is_none() {
        if core.machine.hot.is_none() {
            core.machine.hot = Some(Default::default());
        }
        core.tel = Some(Tel::new(&core.machine));
    }
    let drain_hot = core.readout.is_none();
    if let Some(t) = core.tel.as_mut() {
        t.field(&mut core.machine, ran, drain_hot);
    }
    if let Some(r) = core.readout.as_mut() {
        r.field(&mut core.machine, ran);
        let (w, h) = (core.size.0 as usize, core.size.1 as usize);
        crate::notice::draw_panel(&mut core.frame, w, h, &r.lines);
        core.overlaid = true;
    }
    let dupe = same && core.pictured && core.readout.is_none();
    if let Some(video) = video {
        let (w, h) = core.size;
        if dupe {
            // SAFETY: a null frame is libretro's "same as last time" (the app said it can dupe).
            unsafe { video(std::ptr::null(), w, h, w as usize * 4) };
        } else {
            // SAFETY: `frame` holds w*h pixels of 4 bytes, and lives until the call returns.
            unsafe { video(core.frame.as_ptr() as *const c_void, w, h, w as usize * 4) };
        }
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
    // (time_t is marked as changing on musl, the ARM64 Linux test builds; not on the phone.)
    #[allow(deprecated)]
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
