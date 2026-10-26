//! The libretro entry points. One game at a time, behind a mutex.
//!
//! A game is a zip (.kiro/notes/docs/ARCADE_CORE.md, "How a game is recognised"). The frontend hands over its
//! bytes and its path (`need_fullpath` is false); the path is how a split clone's parent zip is
//! found next to it. A zip cArcade can't play is refused with one plain sentence, logged as the
//! last line before `retro_load_game` returns false, which is the line the app shows.
//!
//! No panic ever leaves an entry point: one unwinding out of an `extern "C"` function stops the
//! whole process, which is the app. Each entry point's body runs in `guarded`, which catches a
//! panic, logs it once and switches the core off for good (until the app is opened again), the
//! way a poisoned lock leaves it.

#![allow(clippy::missing_safety_doc)]

use std::any::Any;
use std::ffi::{c_char, c_void, CStr};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};

use crate::board::{Controls, FourWay, Image, Machine, Switches};
use crate::options;
use crate::sets::{self, Archive, Refusal, SetSpec};
use crate::zip::Zip;

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
type LogFn = unsafe extern "C" fn(level: u32, fmt: *const c_char, ...);

#[repr(C)]
struct RetroLogCallback {
    log: Option<LogFn>,
}

const ENV_GET_CAN_DUPE: u32 = 3;
const ENV_SET_PIXEL_FORMAT: u32 = 10;
const ENV_GET_LOG_INTERFACE: u32 = 27;
const PIXEL_FORMAT_XRGB8888: u32 = 1;
const DEVICE_JOYPAD: u32 = 1;
// libretro's joypad buttons.
const JOY_B: u32 = 0;
const JOY_Y: u32 = 1;
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

/// Every board's picture fits in this, upright.
const MAX_W: u32 = 224;
const MAX_H: u32 = 288;
/// The monitors are 4:3 turned on their side.
const ASPECT: f32 = 3.0 / 4.0;
const SAMPLE_RATE: f64 = 48_000.0;

/// What starts a save state, so a stranger's bytes (or another set's state) are refused.
const STATE_MAGIC: &[u8; 4] = b"CARC";
/// The layout of the boards' states. It's in `library_version`, so the app refuses a state saved
/// by a build with another layout; bump both together.
const STATE_FORMAT: u8 = 2;

struct Core {
    set: &'static SetSpec,
    image: Image,
    machine: Box<dyn Machine>,
    switches: Switches,
    sticks: [FourWay; 2],
    /// The last frame's sound, both channels.
    stereo: Vec<i16>,
}

struct Callbacks {
    env: Option<EnvFn>,
    video: Option<VideoFn>,
    audio_batch: Option<AudioBatchFn>,
    poll: Option<PollFn>,
    input: Option<InputFn>,
}

static CORE: Mutex<Option<Core>> = Mutex::new(None);
static CB: Mutex<Callbacks> =
    Mutex::new(Callbacks { env: None, video: None, audio_batch: None, poll: None, input: None });
static LOG: Mutex<Option<LogFn>> = Mutex::new(None);
/// The last line logged, and how many there have been, kept for the tests (a frontend's log
/// callback is a C variadic function, which a Rust test can't be). Not in the player's build.
#[cfg(any(test, feature = "test-hooks"))]
static LAST_LINE: Mutex<(String, usize)> = Mutex::new((String::new(), 0));

/// The last line logged (the tests' stand-in for the frontend's log).
#[cfg(any(test, feature = "test-hooks"))]
pub fn last_log_line() -> String {
    lock(&LAST_LINE).0.clone()
}

/// How many lines have been logged since the process started (for the tests).
#[cfg(any(test, feature = "test-hooks"))]
pub fn lines_logged() -> usize {
    lock(&LAST_LINE).1
}

/// A lock whose data a panic can't have left half-changed (callbacks, the log, the tests' last
/// line): a poisoned one is taken anyway.
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Sends one line to the frontend's log, prefixed "cArcade: ".
fn log_line(line: &str) {
    #[cfg(any(test, feature = "test-hooks"))]
    {
        let mut last = lock(&LAST_LINE);
        last.0 = line.to_string();
        last.1 += 1;
    }
    let Some(log) = *lock(&LOG) else { return };
    let Ok(text) = std::ffi::CString::new(format!("cArcade: {line}\n").replace('\0', " ")) else { return };
    unsafe { log(1, c"%s".as_ptr(), text.as_ptr()) };
}

/// Set once a panic has been caught. A panic means something went wrong that cArcade can't vouch
/// for, so from then on, until the app is opened again, the core stays switched off, as a
/// poisoned lock leaves it: a game won't load, Restart and save states do nothing, and every
/// frame is the last picture again with silence.
static BROKEN: AtomicBool = AtomicBool::new(false);

/// What a player reads when a game won't open because the core has switched itself off (the app
/// shows the last line logged before `retro_load_game` says no).
pub const BROKEN_LINE: &str =
    "Our arcade emulator ran into a problem. Closing the app and opening it again fixes it.";

/// Runs the body of the entry point `entry`. A panic inside it is caught (it must never unwind
/// into the frontend), logged once and switches the core off; the entry point then answers
/// `failed()`, its own "that didn't work". While the core is off, an entry point that works on
/// the game (`game`) answers `failed()` straight away; the others (the system info, the
/// callbacks, unloading) still do their job.
pub(crate) fn guarded<T>(
    entry: &'static str,
    game: bool,
    failed: impl FnOnce() -> T,
    body: impl FnOnce() -> T,
) -> T {
    if game && BROKEN.load(Ordering::Acquire) {
        return failed();
    }
    let ran = catch_unwind(AssertUnwindSafe(|| {
        planted(entry);
        body()
    }));
    match ran {
        Ok(v) => v,
        Err(why) => {
            switch_off(entry, why.as_ref());
            failed()
        }
    }
}

/// A caught panic: logged, the first time only (what it said and where), and the core switched
/// off.
fn switch_off(entry: &str, why: &(dyn Any + Send)) {
    if BROKEN.swap(true, Ordering::AcqRel) {
        return;
    }
    let what = why
        .downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| why.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "no message".to_string());
    log_line(&format!("switched off after an internal error in {entry}: {what}"));
}

/// Whether this `retro_run` has handed over its picture and its sound yet, so that a frame cut
/// short hands over only what's missing; the frontend's answer to GET_CAN_DUPE; and the size of
/// the last picture.
static SENT_PICTURE: AtomicBool = AtomicBool::new(false);
static SENT_SOUND: AtomicBool = AtomicBool::new(false);
static CAN_DUPE: AtomicBool = AtomicBool::new(false);
static LAST_W: AtomicU32 = AtomicU32::new(MAX_W);
static LAST_H: AtomicU32 = AtomicU32::new(MAX_H);
/// A frame's sound on every board: 792 samples at 48 kHz.
const FRAME_SAMPLES: usize = 792;
static SILENCE: [i16; 2 * FRAME_SAMPLES] = [0; 2 * FRAME_SAMPLES];

/// The frame of a core that has switched off, or of one cut short: the last picture again (or,
/// for a frontend that can't repeat one, a black one the same size) and a frame of silence, as
/// much of it as this frame hasn't handed over.
unsafe fn still_frame() {
    let (video, audio) = {
        let c = lock(&CB);
        (c.video, c.audio_batch)
    };
    if let Some(video) = video {
        if !SENT_PICTURE.swap(true, Ordering::AcqRel) {
            let (w, h) = (LAST_W.load(Ordering::Relaxed), LAST_H.load(Ordering::Relaxed));
            if CAN_DUPE.load(Ordering::Relaxed) {
                video(std::ptr::null(), w, h, w as usize * 4);
            } else {
                let black = vec![0u32; w as usize * h as usize];
                video(black.as_ptr() as *const c_void, w, h, w as usize * 4);
            }
        }
    }
    if let Some(audio) = audio {
        if !SENT_SOUND.swap(true, Ordering::AcqRel) {
            send_sound(audio, &SILENCE);
        }
    }
}

/// Hands stereo samples to the frontend, as many calls as it takes.
unsafe fn send_sound(audio: AudioBatchFn, stereo: &[i16]) {
    let frames = stereo.len() / 2;
    let mut sent = 0;
    while sent < frames {
        let n = audio(stereo[sent * 2..].as_ptr(), frames - sent);
        if n == 0 {
            break;
        }
        sent += n;
    }
}

/// Passes through a point where a test can plant a panic (none in the player's build).
#[inline]
fn planted(_point: &str) {
    #[cfg(any(test, feature = "test-hooks"))]
    test_hooks::fire(_point);
}

/// For the tests: a panic planted where cArcade could panic for real, to show it never gets out,
/// and switching a core that one switched off back on.
#[cfg(any(test, feature = "test-hooks"))]
pub mod test_hooks {
    use std::sync::atomic::Ordering;
    use std::sync::Mutex;

    static PLANTED: Mutex<Option<&'static str>> = Mutex::new(None);

    /// The next time `point` is reached, it panics: an entry point's name panics as it starts,
    /// "retro_run: board" while the frame runs (with the game's lock held, as a real one would).
    pub fn panic_in(point: &'static str) {
        *super::lock(&PLANTED) = Some(point);
    }

    pub(crate) fn fire(point: &str) {
        let mut p = super::lock(&PLANTED);
        if *p == Some(point) {
            *p = None;
            drop(p);
            panic!("a panic planted by a test in {point}");
        }
    }

    /// Whether a caught panic has switched the core off.
    pub fn switched_off() -> bool {
        super::BROKEN.load(Ordering::Acquire)
    }

    /// Switches it back on, with no game loaded, for the next test.
    pub fn revive() {
        *super::lock(&PLANTED) = None;
        super::CORE.clear_poison();
        *super::lock(&super::CORE) = None;
        super::BROKEN.store(false, Ordering::Release);
    }
}

// Every entry point's body is `guarded` (see the module's notes): a test reads this file to check
// that none is left out.

#[no_mangle]
pub extern "C" fn retro_api_version() -> u32 {
    guarded("retro_api_version", false, || 1, || 1)
}

unsafe fn write_system_info(info: *mut RetroSystemInfo) {
    if info.is_null() {
        return;
    }
    *info = RetroSystemInfo {
        library_name: c"cArcade".as_ptr(),
        // Carries the save-state format (STATE_FORMAT).
        library_version: c"0.2.0 (state format 2)".as_ptr(),
        valid_extensions: c"zip|7z".as_ptr(),
        need_fullpath: false,
        block_extract: true,
    };
}

#[no_mangle]
pub unsafe extern "C" fn retro_get_system_info(info: *mut RetroSystemInfo) {
    guarded(
        "retro_get_system_info",
        false,
        || unsafe { write_system_info(info) },
        || unsafe { write_system_info(info) },
    )
}

/// The picture's size and the board's pace: the loaded game's, or with none (or the core switched
/// off) the largest picture at the Pac-Man board's pace.
unsafe fn write_av_info(info: *mut RetroSystemAvInfo, loaded: bool) {
    if info.is_null() {
        return;
    }
    let game = if loaded && !BROKEN.load(Ordering::Acquire) {
        CORE.lock().ok().and_then(|c| {
            c.as_ref().map(|c| {
                let (w, h) = c.image.size();
                (w as u32, h as u32, c.machine.frames_per_second())
            })
        })
    } else {
        None
    };
    let (w, h, fps) = game.unwrap_or((MAX_W, MAX_H, crate::pacman::FPS));
    *info = RetroSystemAvInfo {
        geometry: RetroGameGeometry {
            base_width: w,
            base_height: h,
            max_width: MAX_W,
            max_height: MAX_H,
            aspect_ratio: ASPECT,
        },
        timing: RetroSystemTiming { fps, sample_rate: SAMPLE_RATE },
    };
}

#[no_mangle]
pub unsafe extern "C" fn retro_get_system_av_info(info: *mut RetroSystemAvInfo) {
    guarded(
        "retro_get_system_av_info",
        false,
        || unsafe { write_av_info(info, false) },
        || unsafe { write_av_info(info, true) },
    )
}

#[no_mangle]
pub extern "C" fn retro_set_environment(cb: EnvFn) {
    guarded("retro_set_environment", false, || (), || {
        lock(&CB).env = Some(cb);
        unsafe { options::declare(cb) };
    })
}

#[no_mangle]
pub extern "C" fn retro_set_video_refresh(cb: VideoFn) {
    guarded("retro_set_video_refresh", false, || (), || lock(&CB).video = Some(cb))
}

#[no_mangle]
pub extern "C" fn retro_set_audio_sample(_cb: AudioFn) {
    guarded("retro_set_audio_sample", false, || (), || ())
}

#[no_mangle]
pub extern "C" fn retro_set_audio_sample_batch(cb: AudioBatchFn) {
    guarded("retro_set_audio_sample_batch", false, || (), || lock(&CB).audio_batch = Some(cb))
}

#[no_mangle]
pub extern "C" fn retro_set_input_poll(cb: PollFn) {
    guarded("retro_set_input_poll", false, || (), || lock(&CB).poll = Some(cb))
}

#[no_mangle]
pub extern "C" fn retro_set_input_state(cb: InputFn) {
    guarded("retro_set_input_state", false, || (), || lock(&CB).input = Some(cb))
}

#[no_mangle]
pub extern "C" fn retro_init() {
    guarded("retro_init", false, || (), || ())
}

/// Lets go of the game, even on a core that has switched off (its memory goes either way).
fn drop_game() {
    *lock(&CORE) = None;
}

#[no_mangle]
pub extern "C" fn retro_deinit() {
    guarded("retro_deinit", false, || (), drop_game)
}

#[no_mangle]
pub extern "C" fn retro_set_controller_port_device(_port: u32, _device: u32) {
    guarded("retro_set_controller_port_device", false, || (), || ())
}

/// Reads the zip of a set next to the game, if it's there.
fn sibling_archive(game: &Path, name: &str) -> Option<Archive> {
    let path = sets::sibling(game, name)?;
    let zip = Zip::parse(std::fs::read(&path).ok()?).ok()?;
    log_line(&format!("found {} next to the game", path.display()));
    Some(Archive { zip, stem: name.to_string() })
}

/// Recognises the game and switches its board on, or says in one sentence why it can't.
fn open(bytes: Vec<u8>, path: &str) -> Result<(&'static SetSpec, Image, Box<dyn Machine>), String> {
    let game = Path::new(path);
    let stem = game
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    // A 7z set: the lines the player reads name its kind of file.
    let kind = |line: String| {
        if game.extension().is_some_and(|e| e.eq_ignore_ascii_case("7z")) {
            line.replace(".zip", ".7z").replace("arcade zip", "arcade 7z")
        } else {
            line
        }
    };
    let zip = Zip::parse(bytes).map_err(|why| {
        log_line(&format!("{path}: {why}"));
        kind(Refusal::NotAZip.line())
    })?;
    let names: Vec<String> = zip.entries().iter().map(|e| e.name.clone()).collect();
    let found = sets::identify(Archive { zip, stem: stem.clone() }, &|name| sibling_archive(game, name))
        .map_err(|refusal| {
            log_line(&format!("{stem} ({} files: {}) isn't playable: {refusal:?}", names.len(), names.join(", ")));
            kind(refusal.line())
        })?;
    let spec = found.spec;
    if !found.differing.is_empty() {
        log_line(&format!(
            "{stem}.zip has {} chip(s) that aren't {}'s ({}): playing it anyway, as a hack or homebrew on its board",
            found.differing.len(),
            spec.name,
            found.differing.join(", ")
        ));
    }
    let piece = || kind(Refusal::MissingPiece { title: spec.short_title().into(), set: spec.name.into() }.line());
    let image = Image::from_found(&found).map_err(|why| {
        log_line(&format!("{}: {why}", spec.name));
        piece()
    })?;
    let machine = image.power_on().map_err(|why| {
        log_line(&format!("{}: {why}", spec.name));
        piece()
    })?;
    log_line(&format!(
        "loaded {} ({}, {} {}) on the {:?} board{}",
        spec.name,
        spec.title,
        spec.year,
        spec.maker,
        spec.board,
        if found.absent().is_empty() { String::new() } else { format!(", without {}", found.absent().join(", ")) }
    ));
    Ok((spec, image, machine))
}

/// Asks the frontend for its log, so the lines that follow reach it.
unsafe fn ask_for_log(env: EnvFn) {
    let mut cb = RetroLogCallback { log: None };
    if env(ENV_GET_LOG_INTERFACE, &mut cb as *mut RetroLogCallback as *mut c_void) {
        *lock(&LOG) = cb.log;
    }
}

#[no_mangle]
pub unsafe extern "C" fn retro_load_game(game: *const RetroGameInfo) -> bool {
    guarded("retro_load_game", true, || unsafe { refuse_switched_off() }, || unsafe { load_game(game) })
}

/// No game loads on a core that has switched off; the player reads why.
unsafe fn refuse_switched_off() -> bool {
    let env = lock(&CB).env;
    if let Some(env) = env {
        ask_for_log(env);
    }
    log_line(BROKEN_LINE);
    false
}

unsafe fn load_game(game: *const RetroGameInfo) -> bool {
    // Whatever was loaded before goes, so a game that won't play leaves nothing behind it.
    if let Ok(mut slot) = CORE.lock() {
        *slot = None;
    }
    if game.is_null() {
        return false;
    }
    let path = if (*game).path.is_null() {
        String::new()
    } else {
        CStr::from_ptr((*game).path).to_string_lossy().into_owned()
    };
    let env = lock(&CB).env;
    if let Some(env) = env {
        ask_for_log(env);
        let mut fmt = PIXEL_FORMAT_XRGB8888;
        if !env(ENV_SET_PIXEL_FORMAT, &mut fmt as *mut u32 as *mut c_void) {
            return false;
        }
        let mut dupe = false;
        let can = env(ENV_GET_CAN_DUPE, &mut dupe as *mut bool as *mut c_void) && dupe;
        CAN_DUPE.store(can, Ordering::Relaxed);
    }
    let bytes = if (*game).data.is_null() {
        // The frontend should have handed the bytes over; read the file if it didn't.
        std::fs::read(&path).unwrap_or_default()
    } else {
        std::slice::from_raw_parts((*game).data as *const u8, (*game).size).to_vec()
    };
    let (set, image, machine) = match open(bytes, &path) {
        Ok(opened) => opened,
        Err(line) => {
            // The last line logged: the one the app shows.
            log_line(&line);
            return false;
        }
    };
    let switches = match env {
        Some(env) => {
            options::game_loaded(env, Some(set.dips));
            options::read(env, set)
        }
        None => options::switches(set, |_| None),
    };
    let (w, h) = image.size();
    LAST_W.store(w as u32, Ordering::Relaxed);
    LAST_H.store(h as u32, Ordering::Relaxed);
    let core = Core {
        set,
        image,
        machine,
        switches,
        sticks: [FourWay::default(); 2],
        stereo: Vec::new(),
    };
    match CORE.lock() {
        Ok(mut slot) => {
            *slot = Some(core);
            true
        }
        Err(_) => false,
    }
}

#[no_mangle]
pub extern "C" fn retro_load_game_special(_t: u32, _info: *const RetroGameInfo, _n: usize) -> bool {
    guarded("retro_load_game_special", true, || false, || false)
}

#[no_mangle]
pub extern "C" fn retro_unload_game() {
    guarded("retro_unload_game", false, || (), || {
        drop_game();
        let env = lock(&CB).env;
        if let Some(env) = env {
            unsafe { options::game_loaded(env, None) };
        }
    })
}

#[no_mangle]
pub extern "C" fn retro_get_region() -> u32 {
    guarded("retro_get_region", false, || 0, || 0)
}

/// The app's Restart: the cabinet switched off and on again.
#[no_mangle]
pub extern "C" fn retro_reset() {
    guarded("retro_reset", true, || (), || {
        if let Ok(mut slot) = CORE.lock() {
            if let Some(core) = slot.as_mut() {
                if let Ok(m) = core.image.power_on() {
                    core.machine = m;
                    core.sticks = [FourWay::default(); 2];
                }
            }
        }
    })
}

/// The controls, from the frontend's two pads: on each, the stick or D-pad, Select for the coin
/// and Start for that player's start; B, A, Y, X, L and R are the panel's buttons one to six. X on
/// the first pad is also player 2's start, so a two-player game can be started from one phone.
unsafe fn read_controls(input: InputFn, sticks: &mut [FourWay; 2], four_way: bool) -> Controls {
    let mut c = Controls::default();
    for (port, p) in c.players.iter_mut().enumerate() {
        let held = |id: u32| input(port as u32, DEVICE_JOYPAD, 0, id) != 0;
        let mut dirs = [held(JOY_UP), held(JOY_DOWN), held(JOY_LEFT), held(JOY_RIGHT)];
        if four_way {
            dirs = sticks[port].update(dirs);
        }
        [p.up, p.down, p.left, p.right] = dirs;
        p.buttons = [held(JOY_B), held(JOY_A), held(JOY_Y), held(JOY_X), held(JOY_L), held(JOY_R)];
        p.coin = held(JOY_SELECT);
        p.start = held(JOY_START);
    }
    if c.players[0].buttons[3] {
        c.players[1].start = true;
    }
    c
}

/// One frame; a core that has switched off (or one whose frame is cut short by a panic) hands over
/// the last picture again and silence instead.
#[no_mangle]
pub unsafe extern "C" fn retro_run() {
    SENT_PICTURE.store(false, Ordering::Release);
    SENT_SOUND.store(false, Ordering::Release);
    guarded("retro_run", true, || unsafe { still_frame() }, || unsafe { run_frame() })
}

unsafe fn run_frame() {
    let (video, audio, poll, input, env) = {
        let c = lock(&CB);
        (c.video, c.audio_batch, c.poll, c.input, c.env)
    };
    if let Some(poll) = poll {
        poll();
    }
    let Ok(mut slot) = CORE.lock() else {
        // Poisoned by a panic elsewhere: switched off, as after a panic caught here.
        BROKEN.store(true, Ordering::Release);
        return still_frame();
    };
    let Some(core) = slot.as_mut() else { return };
    if let Some(env) = env {
        if options::changed(env) {
            core.switches = options::read(env, core.set);
        }
    }
    let four_way = core.machine.four_way();
    let controls = match input {
        Some(input) => read_controls(input, &mut core.sticks, four_way),
        None => Controls::default(),
    };
    planted("retro_run: board");
    core.machine.run_frame(&controls, &core.switches);
    if let Some(video) = video {
        let (pixels, w, h) = core.machine.picture();
        LAST_W.store(w as u32, Ordering::Relaxed);
        LAST_H.store(h as u32, Ordering::Relaxed);
        SENT_PICTURE.store(true, Ordering::Release);
        video(pixels.as_ptr() as *const c_void, w as u32, h as u32, w * 4);
    }
    core.stereo.clear();
    for &s in core.machine.sound() {
        core.stereo.extend([s, s]);
    }
    if let Some(audio) = audio {
        SENT_SOUND.store(true, Ordering::Release);
        send_sound(audio, &core.stereo);
    }
}

/// A save state: the magic, the format, the set's name, then the board's own state.
fn snapshot(core: &Core) -> Vec<u8> {
    let board = core.machine.save_state();
    let mut out = Vec::with_capacity(board.len() + 32);
    out.extend_from_slice(STATE_MAGIC);
    out.push(STATE_FORMAT);
    out.push(core.set.name.len() as u8);
    out.extend_from_slice(core.set.name.as_bytes());
    out.extend_from_slice(&(board.len() as u32).to_le_bytes());
    out.extend_from_slice(&board);
    out
}

fn restore(core: &mut Core, data: &[u8]) -> Result<(), String> {
    let rest = data.strip_prefix(STATE_MAGIC).ok_or("not a cArcade save state")?;
    let (&format, rest) = rest.split_first().ok_or("a cut-off save state")?;
    if format != STATE_FORMAT {
        return Err(format!("a save state in format {format}, not {STATE_FORMAT}"));
    }
    let (&n, rest) = rest.split_first().ok_or("a cut-off save state")?;
    let name = rest.get(..usize::from(n)).ok_or("a cut-off save state")?;
    if name != core.set.name.as_bytes() {
        return Err(format!("a save state of {}, not {}", String::from_utf8_lossy(name), core.set.name));
    }
    let rest = &rest[usize::from(n)..];
    let len = u32::from_le_bytes(rest.get(..4).ok_or("a cut-off save state")?.try_into().unwrap_or([0; 4]));
    let end = usize::try_from(len).ok().and_then(|n| n.checked_add(4)).ok_or("a cut-off save state")?;
    let board = rest.get(4..end).ok_or("a cut-off save state")?;
    core.machine.load_state(board)
}

#[no_mangle]
pub extern "C" fn retro_serialize_size() -> usize {
    guarded("retro_serialize_size", true, || 0, || {
        // Room to spare: a board's state can grow by a few bytes from frame to frame (sound
        // already made for the next frame), and the frontend asks for the size once.
        CORE.lock().ok().and_then(|s| s.as_ref().map(|c| snapshot(c).len() + 1024)).unwrap_or(0)
    })
}

#[no_mangle]
pub unsafe extern "C" fn retro_serialize(data: *mut c_void, size: usize) -> bool {
    guarded("retro_serialize", true, || false, || unsafe {
        let Ok(slot) = CORE.lock() else { return false };
        let Some(core) = slot.as_ref() else { return false };
        let state = snapshot(core);
        if data.is_null() || state.len() > size {
            return false;
        }
        let out = std::slice::from_raw_parts_mut(data as *mut u8, size);
        out[..state.len()].copy_from_slice(&state);
        out[state.len()..].fill(0);
        true
    })
}

#[no_mangle]
pub unsafe extern "C" fn retro_unserialize(data: *const c_void, size: usize) -> bool {
    guarded("retro_unserialize", true, || false, || unsafe {
        if data.is_null() {
            return false;
        }
        let bytes = std::slice::from_raw_parts(data as *const u8, size);
        let Ok(mut slot) = CORE.lock() else { return false };
        let Some(core) = slot.as_mut() else { return false };
        match restore(core, bytes) {
            Ok(()) => true,
            Err(why) => {
                log_line(&format!("save state not loaded: {why}"));
                false
            }
        }
    })
}

#[no_mangle]
pub extern "C" fn retro_cheat_reset() {
    guarded("retro_cheat_reset", false, || (), || ())
}

#[no_mangle]
pub extern "C" fn retro_cheat_set(_index: u32, _enabled: bool, _code: *const c_char) {
    guarded("retro_cheat_set", false, || (), || ())
}

#[no_mangle]
pub extern "C" fn retro_get_memory_data(_id: u32) -> *mut c_void {
    guarded("retro_get_memory_data", false, std::ptr::null_mut, std::ptr::null_mut)
}

#[no_mangle]
pub extern "C" fn retro_get_memory_size(_id: u32) -> usize {
    guarded("retro_get_memory_size", false, || 0, || 0)
}
