//! `NativeLibretroCore` — a libretro core loaded from a shared library.
//!
//! The native counterpart to [`super::wasm_core::WasmCore`]. Where that one drives a wasm
//! module through a JS runtime, this one `dlopen`s a `.dylib` from the app bundle and calls
//! its `retro_*` exports directly.
//!
//! Step 10 of the Phase 5 sequence loads `libcontinuum_switch.dylib` — the C++ wrapper
//! around a stub engine that renders a rotating colour — which is why this exists before
//! any real core does: it is the piece that proves the loader, the callback plumbing and
//! the hardware-frame handover, with nothing emulator-shaped in the way.
//!
//! ## The callback problem, and why these are statics
//!
//! libretro's callbacks are bare C function pointers with no user-data parameter. There is
//! nowhere to put a `&mut self`. `cores/host.rs` solved this for the wasm build with a
//! thread-local exchange, and the same reasoning applies here for the same reason: the
//! bridge is already mutably borrowed while `retro_run` is executing, so a callback that
//! reached back into it would be a second mutable borrow. Under a `Mutex` — which the
//! native build uses instead of a `RefCell` — that is a deadlock rather than a panic.
//!
//! So the callbacks write into a thread-local `EXCHANGE`, and `run_frame` collects from it
//! afterwards. Exactly the shape `host.rs` already established.

use std::ffi::{c_char, c_uint, c_void, CStr, CString};
use std::path::Path;

use super::{ContentHint, CoreDescriptor, EmulatorCore};
use crate::audio::AudioSink;
use crate::error::BridgeError;
use crate::frame::{FrameView, PixelFormat};
use crate::gfx::hw::{classify_video_refresh, VideoRefreshKind};
use crate::input::InputSnapshot;

// ---------------------------------------------------------------- libretro ABI

type RetroEnvironment = unsafe extern "C" fn(c_uint, *mut c_void) -> bool;
type RetroVideoRefresh = unsafe extern "C" fn(*const c_void, c_uint, c_uint, usize);
type RetroAudioSampleBatch = unsafe extern "C" fn(*const i16, usize) -> usize;
type RetroAudioSample = unsafe extern "C" fn(i16, i16);
type RetroInputPoll = unsafe extern "C" fn();
type RetroInputState = unsafe extern "C" fn(c_uint, c_uint, c_uint, c_uint) -> i16;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct RetroGameInfo {
    path: *const c_char,
    data: *const c_void,
    size: usize,
    meta: *const c_char,
}

/// The subset of `retro_system_av_info` this needs, laid out to match the C struct.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
struct RetroGameGeometry {
    base_width: c_uint,
    base_height: c_uint,
    max_width: c_uint,
    max_height: c_uint,
    aspect_ratio: f32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
struct RetroSystemTiming {
    fps: f64,
    sample_rate: f64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
struct RetroSystemAvInfo {
    geometry: RetroGameGeometry,
    timing: RetroSystemTiming,
}

// ------------------------------------------------------------- the exchange

/// What the current frame's callbacks reported.
///
/// Thread-local for the same reason as `cores/host.rs`: a callback fired from inside
/// `retro_run` cannot reach the bridge, which is already borrowed.
#[derive(Default)]
struct Exchange {
    /// Set when `video_refresh` was given `RETRO_HW_FRAME_BUFFER_VALID`.
    hardware_frame: bool,
    /// Set when it was given `NULL` — a dupe, which is legal and means "repeat".
    duped: bool,
    width: u32,
    height: u32,
    pitch: usize,
    /// Software pixels, copied out because the pointer is only valid during the call.
    pixels: Vec<u8>,
    audio: Vec<i16>,
    /// `Option` because `InputSnapshot` has no `Default` — and it should not have one,
    /// since "no input" and "all buttons released" are different claims.
    input: Option<InputSnapshot>,
}

thread_local! {
    static EXCHANGE: std::cell::RefCell<Exchange> = std::cell::RefCell::new(Exchange::default());
}

unsafe extern "C" fn on_video_refresh(
    data: *const c_void,
    width: c_uint,
    height: c_uint,
    pitch: usize,
) {
    EXCHANGE.with(|cell| {
        let mut exchange = cell.borrow_mut();
        exchange.width = width;
        exchange.height = height;
        exchange.pitch = pitch;

        // The three-way distinction, in one place. Testing the sentinel *before* treating
        // the pointer as data is the whole point: a hardware frame arrives as
        // `(void*)-1`, and the software path would read from `usize::MAX`.
        match classify_video_refresh(data as usize) {
            VideoRefreshKind::Hardware => {
                exchange.hardware_frame = true;
            }
            VideoRefreshKind::Duped => {
                exchange.duped = true;
            }
            VideoRefreshKind::Pixels => {
                let length = pitch.saturating_mul(height as usize);
                exchange.pixels.clear();
                exchange.pixels.extend_from_slice(unsafe {
                    std::slice::from_raw_parts(data as *const u8, length)
                });
            }
        }
    });
}

unsafe extern "C" fn on_audio_batch(data: *const i16, frames: usize) -> usize {
    if !data.is_null() && frames > 0 {
        EXCHANGE.with(|cell| {
            let mut exchange = cell.borrow_mut();
            exchange
                .audio
                .extend_from_slice(unsafe { std::slice::from_raw_parts(data, frames * 2) });
        });
    }
    frames
}

unsafe extern "C" fn on_audio_sample(left: i16, right: i16) {
    EXCHANGE.with(|cell| {
        let mut exchange = cell.borrow_mut();
        exchange.audio.push(left);
        exchange.audio.push(right);
    });
}

unsafe extern "C" fn on_input_poll() {}

unsafe extern "C" fn on_input_state(
    port: c_uint,
    device: c_uint,
    index: c_uint,
    id: c_uint,
) -> i16 {
    // Delegated to `InputSnapshot::libretro_state`, which already owns the whole mapping —
    // joypad, analog and pointer — and is the same code the wasm build's `CoreHost` calls.
    // Re-deriving it here would be a second mapping to keep in step by hand, which is
    // exactly what putting it in Rust in Phase 1b was meant to avoid.
    EXCHANGE.with(|cell| {
        let exchange = cell.borrow();
        match exchange.input.as_ref() {
            Some(snapshot) => snapshot.libretro_state(port, device, index, id),
            None => 0,
        }
    })
}

/// Environment commands answered for step 10.
///
/// Deliberately small: this is the *native* environment handler and it starts by
/// implementing the two commands the web build refuses and Switch content requires —
/// `GET_SYSTEM_DIRECTORY` for keys and firmware, `GET_SAVE_DIRECTORY` for savedata.
unsafe extern "C" fn on_environment(cmd: c_uint, data: *mut c_void) -> bool {
    const GET_SYSTEM_DIRECTORY: c_uint = 9;
    const GET_SAVE_DIRECTORY: c_uint = 31;
    const GET_VARIABLE_UPDATE: c_uint = 17;
    const SET_SUPPORT_NO_GAME: c_uint = 18;
    const SET_CONTENT_INFO_OVERRIDE: c_uint = 65;
    const GET_CAN_DUPE: c_uint = 3;

    match cmd {
        GET_SYSTEM_DIRECTORY | GET_SAVE_DIRECTORY => {
            let guard = match DIRECTORIES.lock() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
            let Some(directories) = guard.as_ref() else {
                return false;
            };
            let path = if cmd == GET_SYSTEM_DIRECTORY {
                directories.system.as_ref()
            } else {
                directories.save.as_ref()
            };
            match path {
                // Returning false for an absent directory is correct: it means "the
                // frontend has none", which a core must handle. Handing over an empty
                // string would instead have it search the filesystem root.
                Some(value) => {
                    unsafe { *(data as *mut *const c_char) = value.as_ptr() };
                    true
                }
                None => false,
            }
        }
        GET_CAN_DUPE => {
            unsafe { *(data as *mut bool) = true };
            true
        }
        GET_VARIABLE_UPDATE => {
            unsafe { *(data as *mut bool) = false };
            true
        }
        SET_SUPPORT_NO_GAME | SET_CONTENT_INFO_OVERRIDE => true,
        _ => false,
    }
}

#[derive(Default)]
struct Directories {
    system: Option<CString>,
    save: Option<CString>,
}

/// A process global rather than a thread-local, unlike `EXCHANGE`.
///
/// The difference is lifetime, not style. `EXCHANGE` is populated and consumed entirely
/// within one `run_frame` call, so a thread-local is correct however the core is scheduled.
/// These paths are written once at load and read later, during `retro_load_game` — which
/// under a `Mutex`-held bridge may well be a different thread — so a thread-local would
/// hand the core a null system directory and it would fail looking for its keys.
static DIRECTORIES: std::sync::Mutex<Option<Directories>> = std::sync::Mutex::new(None);

// ------------------------------------------------------------------ the core

/// Resolved `retro_*` entry points.
///
/// A struct of function pointers rather than repeated `get` calls, so a missing symbol is a
/// load-time failure naming the symbol instead of a crash on the frame that first needed it.
struct Symbols {
    init: unsafe extern "C" fn(),
    deinit: unsafe extern "C" fn(),
    api_version: unsafe extern "C" fn() -> c_uint,
    set_environment: unsafe extern "C" fn(RetroEnvironment),
    set_video_refresh: unsafe extern "C" fn(RetroVideoRefresh),
    set_audio_sample: unsafe extern "C" fn(RetroAudioSample),
    set_audio_sample_batch: unsafe extern "C" fn(RetroAudioSampleBatch),
    set_input_poll: unsafe extern "C" fn(RetroInputPoll),
    set_input_state: unsafe extern "C" fn(RetroInputState),
    get_system_av_info: unsafe extern "C" fn(*mut RetroSystemAvInfo),
    load_game: unsafe extern "C" fn(*const RetroGameInfo) -> bool,
    unload_game: unsafe extern "C" fn(),
    run: unsafe extern "C" fn(),
    reset: unsafe extern "C" fn(),
    serialize_size: unsafe extern "C" fn() -> usize,
    cheat_reset: unsafe extern "C" fn(),
    cheat_set: unsafe extern "C" fn(c_uint, bool, *const c_char),
}

pub struct NativeLibretroCore {
    descriptor: CoreDescriptor,
    #[allow(dead_code)]
    library: libloading::Library,
    symbols: Symbols,
    frame_count: u64,
    content_loaded: bool,
    last_pixels: Vec<u8>,
    last_width: u32,
    last_height: u32,
    last_pitch: usize,
    last_was_hardware: bool,
    audio: Vec<i16>,
    /// Frames the core reported as dupes. Worth counting rather than discarding: a core
    /// duping steadily is the signature of a stalled hardware path.
    duped_frames: u64,
}

impl NativeLibretroCore {
    /// Loads a core from a path inside the app bundle.
    ///
    /// # Safety
    ///
    /// Loading arbitrary native code is inherently unsafe; the caller must have obtained
    /// this path from the bundle rather than from user input. On iOS this is enforced by
    /// the platform anyway — a downloaded library cannot be `dlopen`ed — but the boundary
    /// is worth naming here rather than assuming.
    pub unsafe fn load(
        descriptor: CoreDescriptor,
        path: &Path,
        system_dir: Option<&str>,
        save_dir: Option<&str>,
    ) -> Result<Self, BridgeError> {
        let library = unsafe { libloading::Library::new(path) }.map_err(|err| {
            BridgeError::InvalidCoreModule {
                core_id: descriptor.id.clone(),
                reason: format!("dlopen failed: {err}"),
            }
        })?;

        macro_rules! symbol {
            ($name:literal, $type:ty) => {{
                let raw: libloading::Symbol<$type> = unsafe { library.get($name.as_bytes()) }
                    .map_err(|_| BridgeError::InvalidCoreModule {
                        core_id: descriptor.id.clone(),
                        reason: format!("missing symbol '{}'", $name),
                    })?;
                *raw
            }};
        }

        let symbols = Symbols {
            init: symbol!("retro_init", unsafe extern "C" fn()),
            deinit: symbol!("retro_deinit", unsafe extern "C" fn()),
            api_version: symbol!("retro_api_version", unsafe extern "C" fn() -> c_uint),
            set_environment: symbol!(
                "retro_set_environment",
                unsafe extern "C" fn(RetroEnvironment)
            ),
            set_video_refresh: symbol!(
                "retro_set_video_refresh",
                unsafe extern "C" fn(RetroVideoRefresh)
            ),
            set_audio_sample: symbol!(
                "retro_set_audio_sample",
                unsafe extern "C" fn(RetroAudioSample)
            ),
            set_audio_sample_batch: symbol!(
                "retro_set_audio_sample_batch",
                unsafe extern "C" fn(RetroAudioSampleBatch)
            ),
            set_input_poll: symbol!("retro_set_input_poll", unsafe extern "C" fn(RetroInputPoll)),
            set_input_state: symbol!(
                "retro_set_input_state",
                unsafe extern "C" fn(RetroInputState)
            ),
            get_system_av_info: symbol!(
                "retro_get_system_av_info",
                unsafe extern "C" fn(*mut RetroSystemAvInfo)
            ),
            load_game: symbol!(
                "retro_load_game",
                unsafe extern "C" fn(*const RetroGameInfo) -> bool
            ),
            unload_game: symbol!("retro_unload_game", unsafe extern "C" fn()),
            run: symbol!("retro_run", unsafe extern "C" fn()),
            reset: symbol!("retro_reset", unsafe extern "C" fn()),
            serialize_size: symbol!("retro_serialize_size", unsafe extern "C" fn() -> usize),
            cheat_reset: symbol!("retro_cheat_reset", unsafe extern "C" fn()),
            cheat_set: symbol!(
                "retro_cheat_set",
                unsafe extern "C" fn(c_uint, bool, *const c_char)
            ),
        };

        // Checked before anything else runs: a core built against a different libretro
        // major version will otherwise misread every struct it is handed.
        let version = unsafe { (symbols.api_version)() };
        if version != 1 {
            return Err(BridgeError::InvalidCoreModule {
                core_id: descriptor.id.clone(),
                reason: format!("libretro API version {version}, expected 1"),
            });
        }

        {
            let mut guard = match DIRECTORIES.lock() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
            *guard = Some(Directories {
                system: system_dir.and_then(|value| CString::new(value).ok()),
                save: save_dir.and_then(|value| CString::new(value).ok()),
            });
        }

        // Order matters and is specified by libretro: the environment callback must be
        // installed before `retro_init`, because cores query it during
        // `retro_set_environment` — which is also where a hardware core declares
        // `SET_HW_RENDER`.
        unsafe {
            (symbols.set_environment)(on_environment);
            (symbols.init)();
            (symbols.set_video_refresh)(on_video_refresh);
            (symbols.set_audio_sample)(on_audio_sample);
            (symbols.set_audio_sample_batch)(on_audio_batch);
            (symbols.set_input_poll)(on_input_poll);
            (symbols.set_input_state)(on_input_state);
        }

        log::info!(
            "native core '{}' loaded from {}",
            descriptor.id,
            path.display()
        );

        Ok(Self {
            descriptor,
            library,
            symbols,
            frame_count: 0,
            content_loaded: false,
            last_pixels: Vec::new(),
            last_width: 0,
            last_height: 0,
            last_pitch: 0,
            last_was_hardware: false,
            audio: Vec::new(),
            duped_frames: 0,
        })
    }

    /// Whether the most recent frame came from a hardware target rather than pixels.
    pub fn last_frame_was_hardware(&self) -> bool {
        self.last_was_hardware
    }

    pub fn duped_frames(&self) -> u64 {
        self.duped_frames
    }

    fn sync_descriptor_from_core(&mut self) {
        let mut info = RetroSystemAvInfo::default();
        unsafe { (self.symbols.get_system_av_info)(&mut info) };
        if info.geometry.base_width > 0 {
            self.descriptor.geometry.base_width = info.geometry.base_width;
        }
        if info.geometry.base_height > 0 {
            self.descriptor.geometry.base_height = info.geometry.base_height;
        }
        if info.geometry.max_width > 0 {
            self.descriptor.geometry.max_width = info.geometry.max_width;
        }
        if info.geometry.max_height > 0 {
            self.descriptor.geometry.max_height = info.geometry.max_height;
        }
        if info.geometry.aspect_ratio > 0.0 {
            self.descriptor.geometry.aspect_ratio = info.geometry.aspect_ratio;
        }
        if info.timing.fps > 0.0 {
            self.descriptor.target_fps = info.timing.fps;
        }
        if info.timing.sample_rate > 0.0 {
            self.descriptor.audio_sample_rate = info.timing.sample_rate as u32;
        }
    }
}

impl EmulatorCore for NativeLibretroCore {
    fn descriptor(&self) -> &CoreDescriptor {
        &self.descriptor
    }

    fn load_content(&mut self, content: &[u8], hint: &ContentHint) -> Result<(), BridgeError> {
        // `need_fullpath` content — which Switch containers are — arrives as a path with no
        // bytes. Both shapes are handled, and which one applies is the core's declaration
        // rather than our choice.
        let path = CString::new(hint.name.clone()).map_err(|_| BridgeError::InvalidContent {
            core_id: self.descriptor.id.clone(),
            reason: "content path contains a NUL byte".into(),
        })?;

        let info = RetroGameInfo {
            path: path.as_ptr(),
            data: if content.is_empty() {
                std::ptr::null()
            } else {
                content.as_ptr() as *const c_void
            },
            size: content.len(),
            meta: std::ptr::null(),
        };

        let ok = unsafe { (self.symbols.load_game)(&info) };
        if !ok {
            return Err(BridgeError::InvalidContent {
                core_id: self.descriptor.id.clone(),
                reason: "retro_load_game rejected the content".into(),
            });
        }

        self.content_loaded = true;
        self.frame_count = 0;
        self.sync_descriptor_from_core();
        Ok(())
    }

    fn run_frame(&mut self, input: &InputSnapshot) -> Result<(), BridgeError> {
        if !self.content_loaded {
            return Err(BridgeError::NoSession);
        }

        EXCHANGE.with(|cell| {
            let mut exchange = cell.borrow_mut();
            exchange.hardware_frame = false;
            exchange.duped = false;
            exchange.audio.clear();
            exchange.input = Some(*input);
        });

        unsafe { (self.symbols.run)() };

        EXCHANGE.with(|cell| {
            let mut exchange = cell.borrow_mut();
            self.last_was_hardware = exchange.hardware_frame;
            if exchange.duped {
                self.duped_frames += 1;
            } else if !exchange.hardware_frame && !exchange.pixels.is_empty() {
                std::mem::swap(&mut self.last_pixels, &mut exchange.pixels);
                self.last_width = exchange.width;
                self.last_height = exchange.height;
                self.last_pitch = exchange.pitch;
            } else if exchange.hardware_frame {
                self.last_width = exchange.width;
                self.last_height = exchange.height;
            }
            std::mem::swap(&mut self.audio, &mut exchange.audio);
        });

        self.frame_count += 1;
        Ok(())
    }

    fn video(&self) -> Option<FrameView<'_>> {
        // A hardware frame is not in `last_pixels` — it is in a texture the compositor
        // already has. Returning `None` here is what routes presentation through
        // `FrameSourceKind::Texture` instead of an upload.
        if self.last_was_hardware || self.last_pixels.is_empty() {
            return None;
        }
        Some(FrameView {
            data: &self.last_pixels,
            width: self.last_width,
            height: self.last_height,
            stride_bytes: self.last_pitch,
            format: PixelFormat::Rgba8888,
        })
    }

    fn drain_audio(&mut self, sink: &mut dyn AudioSink) {
        if self.audio.is_empty() {
            return;
        }
        sink.submit_i16(&self.audio);
        self.audio.clear();
    }

    fn reset(&mut self) -> Result<(), BridgeError> {
        if !self.content_loaded {
            return Err(BridgeError::NoSession);
        }
        unsafe { (self.symbols.reset)() };
        self.frame_count = 0;
        Ok(())
    }

    fn state_size(&self) -> usize {
        if self.content_loaded {
            unsafe { (self.symbols.serialize_size)() }
        } else {
            0
        }
    }

    fn reset_cheats(&mut self) -> Result<(), BridgeError> {
        unsafe { (self.symbols.cheat_reset)() };
        Ok(())
    }

    fn set_cheat(&mut self, index: u32, enabled: bool, code: &str) -> Result<(), BridgeError> {
        let code = CString::new(code)
            .map_err(|_| BridgeError::Cheat("cheat code contains a NUL byte".into()))?;
        unsafe { (self.symbols.cheat_set)(index, enabled, code.as_ptr()) };
        Ok(())
    }

    fn supports_cheats(&self) -> bool {
        true
    }

    fn frame_count(&self) -> u64 {
        self.frame_count
    }
}

impl Drop for NativeLibretroCore {
    fn drop(&mut self) {
        if self.content_loaded {
            unsafe { (self.symbols.unload_game)() };
        }
        unsafe { (self.symbols.deinit)() };
        log::info!(
            "native core '{}' released after {} frames ({} duped)",
            self.descriptor.id,
            self.frame_count,
            self.duped_frames
        );
    }
}

/// Reads a C string a core handed us, for logging.
#[allow(dead_code)]
fn c_str(pointer: *const c_char) -> String {
    if pointer.is_null() {
        return String::new();
    }
    unsafe { CStr::from_ptr(pointer) }
        .to_string_lossy()
        .into_owned()
}
