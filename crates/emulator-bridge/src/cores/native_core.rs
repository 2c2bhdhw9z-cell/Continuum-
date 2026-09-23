//! `NativeLibretroCore` — a libretro core loaded from a shared library.
//!
//! The only real core loader: it `dlopen`s a `.dylib` from the app bundle and calls its
//! `retro_*` exports directly. There was a sibling that drove a wasm module through a JS
//! runtime, and it went with the browser build.
//!
//! Step 10 of the Phase 5 sequence loads `libcontinuum_switch.dylib` — the C++ wrapper
//! around a stub engine that renders a rotating colour — which is why this exists before
//! any real core does: it is the piece that proves the loader, the callback plumbing and
//! the hardware-frame handover, with nothing emulator-shaped in the way.
//!
//! ## The callback problem, and why these are statics
//!
//! libretro's callbacks are bare C function pointers with no user-data parameter. There is
//! nowhere to put a `&mut self`. The bridge is already mutably borrowed while `retro_run`
//! is executing, so a callback that reached back into it would be a second mutable borrow,
//! and under the `Mutex` this build holds the engine behind that is a deadlock rather than
//! a panic.
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

/// `retro_system_info`, laid out to match the C struct.
///
/// Read for one field, `library_version`, and that field is load-bearing rather than
/// cosmetic. A libretro save state is an opaque dump of the core's internal structs, and
/// `retro_unserialize` is not versioned: handing a core a state written by a DIFFERENT BUILD
/// of itself does not reliably fail. It can succeed into a subtly corrupted machine that
/// crashes minutes later somewhere unrelated, which is the worst failure mode available
/// because the cause and the symptom are nowhere near each other. Recording the version
/// alongside every saved state is what lets the app refuse the load instead of hoping.
///
/// The strings are owned by the core and must be copied, not retained. Every field is
/// declared even though only one is read, because the layout has to match for the offset of
/// that one to be right.
#[repr(C)]
struct RetroSystemInfo {
    library_name: *const c_char,
    library_version: *const c_char,
    valid_extensions: *const c_char,
    need_fullpath: bool,
    block_extract: bool,
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

// Environment command numbers. Every value below was machine-checked against
// `.work/hdr/libretro/libretro.h` (SESSION_HANDOFF §1 records that a dropped experimental
// bit produces a case that can never match, so the experimental commands keep theirs).
const ENV_GET_CAN_DUPE: c_uint = 3; // libretro.h:767 RETRO_ENVIRONMENT_GET_CAN_DUPE
const ENV_SET_MESSAGE: c_uint = 6; // libretro.h:807 RETRO_ENVIRONMENT_SET_MESSAGE
const ENV_SET_PERFORMANCE_LEVEL: c_uint = 8; // libretro.h:836 RETRO_ENVIRONMENT_SET_PERFORMANCE_LEVEL
const ENV_GET_SYSTEM_DIRECTORY: c_uint = 9; // libretro.h:854 RETRO_ENVIRONMENT_GET_SYSTEM_DIRECTORY
const ENV_SET_PIXEL_FORMAT: c_uint = 10; // libretro.h:869 RETRO_ENVIRONMENT_SET_PIXEL_FORMAT
const ENV_SET_INPUT_DESCRIPTORS: c_uint = 11; // libretro.h:886 RETRO_ENVIRONMENT_SET_INPUT_DESCRIPTORS
const ENV_GET_VARIABLE: c_uint = 15; // libretro.h:970 RETRO_ENVIRONMENT_GET_VARIABLE
const ENV_SET_VARIABLES: c_uint = 16; // libretro.h:1020 RETRO_ENVIRONMENT_SET_VARIABLES
const ENV_GET_VARIABLE_UPDATE: c_uint = 17; // libretro.h:1038 RETRO_ENVIRONMENT_GET_VARIABLE_UPDATE
const ENV_SET_SUPPORT_NO_GAME: c_uint = 18; // libretro.h:1055 RETRO_ENVIRONMENT_SET_SUPPORT_NO_GAME
const ENV_GET_SAVE_DIRECTORY: c_uint = 31; // libretro.h:1330 RETRO_ENVIRONMENT_GET_SAVE_DIRECTORY
const ENV_SET_SYSTEM_AV_INFO: c_uint = 32; // libretro.h:1369 RETRO_ENVIRONMENT_SET_SYSTEM_AV_INFO
const ENV_SET_CONTROLLER_INFO: c_uint = 35; // libretro.h:1510 RETRO_ENVIRONMENT_SET_CONTROLLER_INFO
const ENV_SET_GEOMETRY: c_uint = 37; // libretro.h:1558 RETRO_ENVIRONMENT_SET_GEOMETRY
const ENV_GET_CORE_OPTIONS_VERSION: c_uint = 52; // libretro.h:1854 RETRO_ENVIRONMENT_GET_CORE_OPTIONS_VERSION
const ENV_SET_CORE_OPTIONS: c_uint = 53; // libretro.h:1928 RETRO_ENVIRONMENT_SET_CORE_OPTIONS
const ENV_SET_CORE_OPTIONS_INTL: c_uint = 54; // libretro.h:1951 RETRO_ENVIRONMENT_SET_CORE_OPTIONS_INTL
const ENV_SET_CORE_OPTIONS_DISPLAY: c_uint = 55; // libretro.h:1980 RETRO_ENVIRONMENT_SET_CORE_OPTIONS_DISPLAY
const ENV_SET_MESSAGE_EXT: c_uint = 60; // libretro.h:2079 RETRO_ENVIRONMENT_SET_MESSAGE_EXT
const ENV_SET_MINIMUM_AUDIO_LATENCY: c_uint = 63; // libretro.h:2154 RETRO_ENVIRONMENT_SET_MINIMUM_AUDIO_LATENCY
const ENV_SET_CONTENT_INFO_OVERRIDE: c_uint = 65; // libretro.h:2183 RETRO_ENVIRONMENT_SET_CONTENT_INFO_OVERRIDE
const ENV_SET_CORE_OPTIONS_V2: c_uint = 67; // libretro.h:2345 RETRO_ENVIRONMENT_SET_CORE_OPTIONS_V2
const ENV_SET_CORE_OPTIONS_V2_INTL: c_uint = 68; // libretro.h:2362 RETRO_ENVIRONMENT_SET_CORE_OPTIONS_V2_INTL
const ENV_SET_CORE_OPTIONS_UPDATE_DISPLAY_CALLBACK: c_uint = 69; // libretro.h:2383

/// Stores the pixel format a core negotiated via `SET_PIXEL_FORMAT`.
///
/// A process global for the same lifetime reason as [`DIRECTORIES`]: the core calls
/// `SET_PIXEL_FORMAT` from inside `retro_load_game`, which runs under a `Mutex`-held bridge
/// and possibly off the callback thread, so a thread-local would lose the value. Reset
/// before each load ([`reset_negotiated_format`]) so a stale format from a prior core
/// cannot leak into the next one.
static NEGOTIATED_FORMAT: std::sync::Mutex<Option<PixelFormat>> = std::sync::Mutex::new(None);

fn reset_negotiated_format() {
    let mut guard = match NEGOTIATED_FORMAT.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    *guard = None;
}

fn store_negotiated_format(format: PixelFormat) {
    let mut guard = match NEGOTIATED_FORMAT.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    *guard = Some(format);
}

fn take_negotiated_format() -> Option<PixelFormat> {
    let guard = match NEGOTIATED_FORMAT.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    *guard
}

/// The environment protocol, answering what a real PS1 core (PCSX ReARMed) needs to boot
/// through the software frame path, and refusing everything else.
///
/// Returning `false` means "unsupported", which cores are required to handle, so the safe
/// default for anything unlisted is `false` — and the default arm stays silent to avoid
/// per-frame log spam, matching the existing style. The commands here are exactly those a
/// first PCSX ReARMed boot exercises: `SET_PIXEL_FORMAT` (negotiated, not hardcoded),
/// `GET_VARIABLE` (refused so the core uses its own defaults), the directory queries, and
/// the option/descriptor/geometry/message families that a core announces but a minimal
/// host need only tolerate.
unsafe extern "C" fn on_environment(cmd: c_uint, data: *mut c_void) -> bool {
    match cmd {
        ENV_GET_SYSTEM_DIRECTORY | ENV_GET_SAVE_DIRECTORY => {
            let guard = match DIRECTORIES.lock() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
            let Some(directories) = guard.as_ref() else {
                return false;
            };
            let path = if cmd == ENV_GET_SYSTEM_DIRECTORY {
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
        ENV_SET_PIXEL_FORMAT => {
            // data is `const enum retro_pixel_format *`. libretro numbering is
            // 0=0RGB1555, 1=XRGB8888, 2=RGB565; `from_libretro` maps 1/2 and rejects 0.
            if data.is_null() {
                return false;
            }
            let raw = unsafe { *(data as *const c_uint) };
            match PixelFormat::from_libretro(raw) {
                Some(format) => {
                    store_negotiated_format(format);
                    true
                }
                // 0RGB1555 (and anything else) is refused. PCSX ReARMed logs an error and
                // keeps its current format; it does not abort the load.
                None => false,
            }
        }
        ENV_GET_VARIABLE => {
            // data is `struct retro_variable { const char *key; const char *value; }`.
            // The default answer is still "unset, use your default" — null value, false —
            // which is what PCSX ReARMed's 73 reads expect. The exception is the small
            // override table the loaded core installed; see `option_overrides`.
            if data.is_null() {
                return false;
            }
            let key_slot = data as *mut *const c_char;
            // Offset of `value` is one pointer past `key`.
            let value_slot = unsafe { key_slot.add(1) };
            let key_ptr = unsafe { *key_slot };
            if !key_ptr.is_null() {
                if let Some(value) = lookup_option(unsafe { CStr::from_ptr(key_ptr) }) {
                    unsafe { *value_slot = value };
                    return true;
                }
            }
            unsafe { *value_slot = std::ptr::null() };
            false
        }
        ENV_GET_CAN_DUPE => {
            unsafe { *(data as *mut bool) = true };
            true
        }
        ENV_GET_VARIABLE_UPDATE => {
            unsafe { *(data as *mut bool) = false };
            true
        }
        // Accept-and-noop: the core announces these, and a minimal host need only tolerate
        // them. Returning true means "recognized" without claiming any behaviour a callback
        // would later have to honour. Deliberately absent: SET_AUDIO_BUFFER_STATUS_CALLBACK
        // (62) and GET_INPUT_BITMASKS (51|EXPERIMENTAL) are *not* here — accepting the first
        // would promise a callback we never make (SESSION_HANDOFF §1), and refusing the
        // second makes the core fall back to per-id input_state, which InputSnapshot serves.
        ENV_SET_SUPPORT_NO_GAME
        | ENV_SET_CONTENT_INFO_OVERRIDE
        | ENV_SET_VARIABLES
        | ENV_SET_CORE_OPTIONS
        | ENV_SET_CORE_OPTIONS_INTL
        | ENV_SET_CORE_OPTIONS_DISPLAY
        | ENV_SET_CORE_OPTIONS_V2
        | ENV_SET_CORE_OPTIONS_V2_INTL
        | ENV_SET_CORE_OPTIONS_UPDATE_DISPLAY_CALLBACK
        | ENV_SET_INPUT_DESCRIPTORS
        | ENV_SET_CONTROLLER_INFO
        | ENV_SET_PERFORMANCE_LEVEL
        | ENV_SET_SYSTEM_AV_INFO
        | ENV_SET_GEOMETRY
        | ENV_SET_MESSAGE
        | ENV_SET_MESSAGE_EXT
        | ENV_SET_MINIMUM_AUDIO_LATENCY => true,
        ENV_GET_CORE_OPTIONS_VERSION => {
            // Report core-options API version 0: we accept the SET_CORE_OPTIONS* families
            // as no-ops but implement none of their query surface, so 0 is the honest
            // answer and keeps the core on the SET_VARIABLES-era path.
            unsafe { *(data as *mut c_uint) = 0 };
            true
        }
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

// ------------------------------------------------------------------ core options

/// Core options this host answers, by core id.
///
/// Everything not listed here stays refused, which is the right default: a core's own
/// defaults are chosen by people who know the core, and a frontend inventing values is how
/// you end up debugging someone else's emulator. An entry earns its place only when the
/// core cannot do something we ship without it.
///
/// THE DS TOUCH SCREEN IS THE FIRST SUCH CASE, and the reason is not the one the option's
/// declared default suggests. `melonds_touch_mode` advertises `"Mouse"`, so refusing the
/// read looks harmless. But the core's code is:
///
/// ```c
/// TouchMode new_touch_mode = TouchMode::Disabled;
/// var.key = "melonds_touch_mode";
/// if (environ_cb(RETRO_ENVIRONMENT_GET_VARIABLE, &var) && var.value) { ...parse... }
/// ```
///
/// The advertised default is only ever applied by a frontend that implements the options
/// UI and hands the value back. Refuse the read and the variable keeps its C initialiser,
/// `Disabled`, and `input.cpp` then forces `touching = false` every frame. So without this
/// entry the DS touch screen is not merely mismapped, it is switched off inside the core,
/// and no amount of correct pointer data from our side could ever reach `NDS::TouchScreen`.
fn option_overrides(core_id: &str) -> &'static [(&'static str, &'static str)] {
    match core_id {
        "melonds" => &[
            // `Touch` is the mode that reads RETRO_DEVICE_POINTER. The alternatives move a
            // cursor with a mouse or the right stick, neither of which is what a finger on a
            // phone screen is, and `Disabled` is what refusing the read actually selects.
            ("melonds_touch_mode", "Touch"),
            // The SECOND option whose C initialiser is not its advertised default, found the
            // same way and just as load-bearing: `int DirectBoot = 0` in the core's
            // config.cpp, against an advertised `"enabled"`. Refuse the read and the core
            // boots the DS firmware menu instead of the cartridge.
            //
            // That path cannot work in this app, and the core's own option description says
            // why: booting to the menu needs real BIOS and firmware dumps. We ship none, so
            // the core falls back to its built-in FreeBIOS and a generated firmware, and a
            // generated firmware has no boot menu to reach the cartridge from. The symptom
            // would be a DS that loads a game and then sits there.
            ("melonds_boot_directly", "enabled"),
        ],
        // THE THIRD INSTANCE OF THE SAME TRAP, and this one froze the app rather than disabling a
        // feature. The N64 core picks its renderer like this:
        //
        //     if (gfx_var.value)                       // NULL when the host refuses the read
        //     {
        //         if (!strcmp(gfx_var.value, "auto"))
        //             core_settings_autoselect_gfx_plugin();
        //         ...
        //     }
        //
        // Refuse the read and `gfx_var.value` is NULL, so that whole block is skipped and the
        // autoselect never runs at all. `gfx_plugin` then keeps its zero initialiser, and the enum
        // in Graphics/plugin.h begins:
        //
        //     enum gfx_plugin_type { GFX_GLIDE64 = 0, GFX_RICE, GFX_GLN64, GFX_ANGRYLION, ... };
        //
        // So the default is GLIDE64, an OPENGL renderer, in a build compiled with HAVE_OPENGL=0
        // where that plugin does not exist. The core then tries to render through nothing and the
        // app hangs on the first frame, which presents as the emulator freezing when a game starts.
        //
        // `angrylion` is the software rasteriser and the only value the option even offers when GL
        // is absent. Naming it explicitly takes the `!strcmp(gfx_var.value, "angrylion")` branch,
        // which sets the plugin directly and does not depend on the autoselect running or on
        // HAVE_THR_AL being defined.
        //
        // The RSP is named for the same reason rather than because its initialiser is wrong: zero
        // happens to be RSP_HLE, which is what we want, but it is reached only by the same skipped
        // block, so relying on it would be relying on a coincidence. HLE over the software
        // rasteriser is also the faster pairing, and speed is the entire question for this core.
        "parallel_n64" => &[
            ("parallel-n64-gfxplugin", "angrylion"),
            ("parallel-n64-rspplugin", "hle"),
        ],
        _ => &[],
    }
}

/// The installed overrides, as C strings the core can hold a pointer to.
///
/// A process global for the same lifetime reason as [`DIRECTORIES`], and with one extra
/// requirement: `GET_VARIABLE` hands the core a `*const c_char` INTO this table, so the
/// string has to outlive the call that returned it. Keeping the owned `CString`s here means
/// the pointer stays valid until the next core load replaces the table, which is strictly
/// longer than the core that received it lives.
static OPTIONS: std::sync::Mutex<Vec<(CString, CString)>> = std::sync::Mutex::new(Vec::new());

/// Replaces the option table for a core about to be loaded.
fn install_options(core_id: &str) {
    let mut guard = match OPTIONS.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    guard.clear();
    for (key, value) in option_overrides(core_id) {
        // A key or value that cannot be made into a CString would be a typo in the table
        // above, since both are literals. Skipping is still better than unwrapping: the
        // cost is one option quietly reverting to the core's default, not a crash on load.
        if let (Ok(key), Ok(value)) = (CString::new(*key), CString::new(*value)) {
            guard.push((key, value));
        }
    }
    if !guard.is_empty() {
        log::info!(
            "core '{}' gets {} option override(s): {}",
            core_id,
            guard.len(),
            guard
                .iter()
                .map(|(key, value)| format!(
                    "{}={}",
                    key.to_string_lossy(),
                    value.to_string_lossy()
                ))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
}

/// The value for a key the core asked about, or `None` to leave it at the core's default.
fn lookup_option(key: &CStr) -> Option<*const c_char> {
    let guard = match OPTIONS.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    guard
        .iter()
        .find(|(candidate, _)| candidate.as_c_str() == key)
        .map(|(_, value)| value.as_ptr())
}

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
    get_system_info: unsafe extern "C" fn(*mut RetroSystemInfo),
    serialize_size: unsafe extern "C" fn() -> usize,
    // THESE TWO WERE MISSING UNTIL SAVE STATES WERE TESTED ON A DEVICE, and their absence is
    // worth recording because of how quietly it failed. `serialize_size` was resolved and
    // `state_size()` therefore returned a real, plausible number, so everything upstream
    // believed the core supported save states. But `save_state` and `load_state` were never
    // implemented on this type, so they fell through to the trait's defaults, which return
    // `NotImplemented`. The result was a save button that always failed and a rewind tape that
    // silently recorded nothing at all, because the tick logs a refused snapshot at debug level
    // and carries on. Nothing anywhere said the feature did not exist.
    serialize: unsafe extern "C" fn(*mut c_void, usize) -> bool,
    unserialize: unsafe extern "C" fn(*const c_void, usize) -> bool,
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
    /// The pixel format `video()` reports. Seeded from the descriptor's declared format and
    /// overwritten by whatever the core chose through `SET_PIXEL_FORMAT` during load.
    negotiated_format: PixelFormat,
    /// The core's `library_version`, copied once at load. `None` if the core left it null or
    /// reported something that is not UTF-8.
    ///
    /// Copied rather than borrowed because the pointer belongs to the core, and this outlives
    /// no particular call but the core's own lifetime is not something to bet a `&str` on.
    library_version: Option<String>,
    /// What the core said about how it wants its content, read from the same
    /// `retro_get_system_info` call as the version above.
    ///
    /// `true` means "hand me a path and I will open the file myself", which is what a disc-based
    /// core wants: PCSX ReARMed must not be given a 600 MB `.bin` in memory. `false` means the
    /// libretro contract obliges the FRONTEND to provide the bytes, and `load_content` reads the
    /// file to satisfy that.
    ///
    /// THIS WAS NEVER READ UNTIL A CORE NEEDED IT. Every core was handed a path and no bytes, which
    /// worked for the first six only because each of them either declares `need_fullpath` or
    /// happens to fall back to opening the path anyway. Stella does neither: it `memcpy`s from
    /// `info->data` unconditionally, so it would have received a zero-byte ROM and failed in a way
    /// that looks exactly like a broken core.
    need_fullpath: bool,
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
            get_system_info: symbol!(
                "retro_get_system_info",
                unsafe extern "C" fn(*mut RetroSystemInfo)
            ),
            serialize_size: symbol!("retro_serialize_size", unsafe extern "C" fn() -> usize),
            serialize: symbol!(
                "retro_serialize",
                unsafe extern "C" fn(*mut c_void, usize) -> bool
            ),
            unserialize: symbol!(
                "retro_unserialize",
                unsafe extern "C" fn(*const c_void, usize) -> bool
            ),
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

        // Before `set_environment`, because cores read their options during the environment
        // call and again on init. Installing after would mean the first read saw an empty table.
        //
        // Installed AGAIN at `load_content`, and that is the one that actually matters for the DS:
        // melonDS reads `melonds_touch_mode` from `check_variables` during `retro_load_game`, not
        // here. See the note there.
        install_options(&descriptor.id);

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

        let negotiated_format = descriptor.pixel_format;
        // Read here, before any content is loaded, because `retro_get_system_info` is one of
        // the few libretro entry points a core must answer at any time. Doing it once and
        // keeping the copy means nothing later has to call back into the core for a string that
        // cannot change.
        let (library_version, need_fullpath) = unsafe {
            let mut info = RetroSystemInfo {
                library_name: std::ptr::null(),
                library_version: std::ptr::null(),
                valid_extensions: std::ptr::null(),
                need_fullpath: false,
                block_extract: false,
            };
            (symbols.get_system_info)(&mut info);
            let version = if info.library_version.is_null() {
                None
            } else {
                // A core reporting a version that is not UTF-8 is treated as reporting none,
                // rather than as an error: the version is only used to refuse a mismatched save
                // state, and a core that cannot name itself simply loses that one check.
                CStr::from_ptr(info.library_version)
                    .to_str()
                    .ok()
                    .map(str::to_owned)
            };
            (version, info.need_fullpath)
        };
        log::info!(
            "core '{}' reports version {}, need_fullpath {}",
            descriptor.id,
            library_version.as_deref().unwrap_or("(none)"),
            need_fullpath
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
            negotiated_format,
            library_version,
            need_fullpath,
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
        // `need_fullpath` content — which Switch containers and PS1 discs are — arrives as a
        // path with no bytes, and the core opens the file itself. A core that hard-requires
        // `info->path` (PCSX ReARMed rejects a null path outright) needs a real, openable
        // filesystem path here, not the bare file stem in `hint.name`. So when there are no
        // bytes and the caller supplied `full_path`, hand over that verbatim; otherwise fall
        // back to `name`, which is what the in-memory and switch-stub paths already use.
        let path_string = match (content.is_empty(), hint.full_path.as_ref()) {
            (true, Some(full_path)) => full_path.clone(),
            _ => hint.name.clone(),
        };
        let path = CString::new(path_string).map_err(|_| BridgeError::InvalidContent {
            core_id: self.descriptor.id.clone(),
            reason: "content path contains a NUL byte".into(),
        })?;

        // When the core did NOT ask for a path, libretro obliges the frontend to supply the bytes,
        // and this is where that obligation is met. The host hands every game over as a path with
        // no bytes, which is right for a disc — PCSX ReARMed must not be given a 600 MB track in
        // memory — and silently wrong for a core that reads `info->data` directly. Stella is the
        // first such core here: it `memcpy`s from `data` with no fallback, so it would have been
        // handed a zero-byte ROM and failed in a way indistinguishable from a broken build.
        //
        // Read HERE rather than in Swift, and driven by what the core declared rather than by a list
        // of core ids, so the next core to want bytes needs no change at all. The read is skipped
        // entirely for a `need_fullpath` core, which is what keeps disc images off the heap.
        let mut read_from_path = Vec::new();
        if content.is_empty() && !self.need_fullpath {
            if let Some(full_path) = hint.full_path.as_ref() {
                read_from_path =
                    std::fs::read(full_path).map_err(|err| BridgeError::InvalidContent {
                        core_id: self.descriptor.id.clone(),
                        reason: format!(
                            "core wants the content in memory and {full_path} could not be read: \
                             {err}"
                        ),
                    })?;
                log::info!(
                    "core '{}' declared need_fullpath false, so {} byte(s) were read from {}",
                    self.descriptor.id,
                    read_from_path.len(),
                    full_path
                );
            }
        }
        let content: &[u8] = if read_from_path.is_empty() {
            content
        } else {
            &read_from_path
        };

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

        // Re-installed here, not only at load, because THIS is where the options that matter are
        // read. `OPTIONS` is a process global, so it holds whatever the last core to be opened
        // installed, and the core running a session is not necessarily that core. Today they
        // coincide, but only because of three facts in other files: retention defaults to `Drop`,
        // `launch` stops and unloads before loading, and the host re-opens the dylib each time. A
        // change to any one of them would have silently reverted the DS to a disabled touch screen
        // and a firmware boot, with nothing anywhere saying so. Installing it next to the call
        // that reads it makes the guarantee local to the code that depends on it.
        install_options(&self.descriptor.id);

        // A core negotiates its pixel format from inside `retro_load_game`. Clear any stale
        // value first, then read back whatever it chose so `video()` reports the truth.
        reset_negotiated_format();

        let ok = unsafe { (self.symbols.load_game)(&info) };
        if !ok {
            return Err(BridgeError::InvalidContent {
                core_id: self.descriptor.id.clone(),
                reason: "retro_load_game rejected the content".into(),
            });
        }

        if let Some(format) = take_negotiated_format() {
            self.negotiated_format = format;
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
            // The negotiated format, not a hardcoded value: RGB565 and XRGB8888 are both
            // normalised on the CPU side by gfx/convert.rs, so reporting the true format is
            // what makes PS1 colours correct. Defaults to the descriptor's declared format
            // when no SET_PIXEL_FORMAT arrived.
            format: self.negotiated_format,
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

    /// Writes a save state into `dst`, returning how many bytes the core wrote.
    ///
    /// The size is re-read here rather than taken from the caller's buffer length, because
    /// libretro permits `retro_serialize_size` to CHANGE during a session: a disc swap is the
    /// usual reason, and some cores report a different figure once they have run a few frames.
    /// A caller that sized its buffer a moment ago can therefore be holding a buffer that is
    /// now too small, and handing `retro_serialize` a length longer than the buffer would be a
    /// heap overflow rather than an error. So the buffer is checked against what the core wants
    /// right now, and the core is handed the smaller figure's worth of nothing at all if it does
    /// not fit.
    fn save_state(&self, dst: &mut [u8]) -> Result<usize, BridgeError> {
        if !self.content_loaded {
            return Err(BridgeError::NoSession);
        }
        let size = unsafe { (self.symbols.serialize_size)() };
        if size == 0 {
            return Err(BridgeError::SaveState(format!(
                "{} does not support save states",
                self.descriptor.display_name
            )));
        }
        if dst.len() < size {
            return Err(BridgeError::SaveState(format!(
                "save buffer is {} bytes but {} now needs {size}",
                dst.len(),
                self.descriptor.display_name
            )));
        }
        // Cast through the slice's pointer rather than taking a reference to the whole buffer,
        // so the core is given exactly the length it asked for even when `dst` is longer.
        let ok = unsafe { (self.symbols.serialize)(dst.as_mut_ptr().cast::<c_void>(), size) };
        if !ok {
            return Err(BridgeError::SaveState(format!(
                "{} refused to write a save state",
                self.descriptor.display_name
            )));
        }
        Ok(size)
    }

    /// Restores a save state.
    ///
    /// **A state from the wrong core, or from a different build of the right one, is a real
    /// hazard rather than a rejected input.** `retro_unserialize` reads an opaque dump straight
    /// back into the core's own structs and is not versioned, so a foreign blob of plausible
    /// length can be accepted and leave the emulated machine quietly corrupt, to crash later
    /// somewhere with no visible connection to this call. Nothing at this layer can tell the
    /// difference, which is why the checks live where the metadata does: the host records the
    /// core id, the core's reported version and the exact byte length beside every state and
    /// refuses a mismatch before calling this. The length check below is the only defence
    /// available here, and it is the weakest of the four.
    fn load_state(&mut self, src: &[u8]) -> Result<(), BridgeError> {
        if !self.content_loaded {
            return Err(BridgeError::NoSession);
        }
        if src.is_empty() {
            return Err(BridgeError::SaveState("that save state is empty".into()));
        }
        // DIRECTIONAL, and it did not used to be. This refused any length that was not exactly
        // what the core reports right now, which was wrong and showed up as save states loading on
        // some games and not others. `retro_serialize_size` is permitted to CHANGE during a
        // session and between sessions: several cores report a larger figure once they have run a
        // few frames, and a PlayStation core's figure moves with the disc state. So a state that is
        // a perfectly good state, from this core and this build, can legitimately be a different
        // length from the one the core would write this instant, and refusing it made the feature
        // look broken for exactly the cores that do this.
        //
        // Too SHORT is still refused, because that is the one direction with a real hazard: the
        // core reads its own structures out of the buffer, and a buffer smaller than it expects is
        // how it reads past the end. Too long is harmless, since the core stops when it has what it
        // needs, so the surplus is ignored.
        //
        // This is not the check that stops a state from the WRONG core being loaded. That is the
        // host's job, where the core id and the core's version are recorded beside every state, and
        // it is far stronger than comparing a length.
        let expected = unsafe { (self.symbols.serialize_size)() };
        if expected != 0 && src.len() < expected {
            return Err(BridgeError::SaveState(format!(
                "that save state is {} bytes but {} needs at least {expected}, so it is truncated \
                 or was written by a different core",
                src.len(),
                self.descriptor.display_name
            )));
        }
        if expected != 0 && src.len() != expected {
            log::info!(
                "loading a {} byte state into '{}', which currently reports {expected}; \
                 permitted, the figure is allowed to move during a session",
                src.len(),
                self.descriptor.id
            );
        }
        let ok = unsafe { (self.symbols.unserialize)(src.as_ptr().cast::<c_void>(), src.len()) };
        if !ok {
            return Err(BridgeError::SaveState(format!(
                "{} rejected that save state",
                self.descriptor.display_name
            )));
        }
        Ok(())
    }

    fn version(&self) -> Option<&str> {
        self.library_version.as_deref()
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

#[cfg(test)]
mod tests {
    use super::*;

    // The negotiated-format global is process-wide, so the format tests must not race each
    // other. A dedicated lock serialises them without depending on the test harness's
    // threading, and is poison-tolerant so one failing test does not cascade.
    static FORMAT_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn format_guard() -> std::sync::MutexGuard<'static, ()> {
        match FORMAT_TEST_LOCK.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    /// libretro pixel-format numbering: 0=0RGB1555, 1=XRGB8888, 2=RGB565.
    const LIBRETRO_0RGB1555: c_uint = 0;
    const LIBRETRO_XRGB8888: c_uint = 1;
    const LIBRETRO_RGB565: c_uint = 2;

    #[test]
    fn set_pixel_format_rgb565_is_stored() {
        let _guard = format_guard();
        reset_negotiated_format();
        let mut value: c_uint = LIBRETRO_RGB565;
        let ok = unsafe {
            on_environment(
                ENV_SET_PIXEL_FORMAT,
                &mut value as *mut c_uint as *mut c_void,
            )
        };
        assert!(ok, "RGB565 must be accepted");
        assert_eq!(take_negotiated_format(), Some(PixelFormat::Rgb565));
    }

    #[test]
    fn set_pixel_format_xrgb8888_is_stored() {
        let _guard = format_guard();
        reset_negotiated_format();
        let mut value: c_uint = LIBRETRO_XRGB8888;
        let ok = unsafe {
            on_environment(
                ENV_SET_PIXEL_FORMAT,
                &mut value as *mut c_uint as *mut c_void,
            )
        };
        assert!(ok, "XRGB8888 must be accepted");
        assert_eq!(take_negotiated_format(), Some(PixelFormat::Xrgb8888));
    }

    #[test]
    fn set_pixel_format_0rgb1555_is_refused() {
        let _guard = format_guard();
        reset_negotiated_format();
        let mut value: c_uint = LIBRETRO_0RGB1555;
        let ok = unsafe {
            on_environment(
                ENV_SET_PIXEL_FORMAT,
                &mut value as *mut c_uint as *mut c_void,
            )
        };
        assert!(!ok, "0RGB1555 must be refused");
        // Nothing stored: the core keeps its current format on refusal.
        assert_eq!(take_negotiated_format(), None);
    }

    #[test]
    fn get_variable_returns_false_and_nulls_value() {
        // `retro_variable { key, value }`. The handler must not fabricate an option
        // string: it nulls `value` and returns false, meaning "use your default".
        #[repr(C)]
        struct RetroVariable {
            key: *const c_char,
            value: *const c_char,
        }
        let key = CString::new("pcsx_rearmed_rgb32_output").unwrap();
        // Seed `value` with a non-null sentinel so a passing test proves it was cleared.
        let mut var = RetroVariable {
            key: key.as_ptr(),
            value: key.as_ptr(),
        };
        let ok = unsafe {
            on_environment(
                ENV_GET_VARIABLE,
                &mut var as *mut RetroVariable as *mut c_void,
            )
        };
        assert!(!ok, "GET_VARIABLE must return false so the core uses its default");
        assert!(var.value.is_null(), "value must be nulled, not fabricated");
    }

    /// `retro_variable`, redeclared for the option tests below.
    #[repr(C)]
    struct RetroVariableProbe {
        key: *const c_char,
        value: *const c_char,
    }

    /// Asks the environment callback for one option the way a core does.
    ///
    /// Returns the answer and the string the host handed back, if any.
    fn ask_option(key: &str) -> (bool, Option<String>) {
        let key = CString::new(key).unwrap();
        let mut var = RetroVariableProbe {
            key: key.as_ptr(),
            // A non-null sentinel, so "value was cleared" is provable rather than assumed.
            value: key.as_ptr(),
        };
        let ok = unsafe {
            on_environment(
                ENV_GET_VARIABLE,
                &mut var as *mut RetroVariableProbe as *mut c_void,
            )
        };
        let value = if var.value.is_null() {
            None
        } else {
            Some(unsafe { CStr::from_ptr(var.value) }.to_string_lossy().into_owned())
        };
        (ok, value)
    }

    /// `OPTIONS` is a process global, so the tests that install into it have to take turns.
    /// Without this they race: one clears the table while another is asserting on it.
    static OPTION_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn option_test_guard() -> std::sync::MutexGuard<'static, ()> {
        match OPTION_TEST_LOCK.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    #[test]
    fn ds_touch_mode_is_served_to_the_core() {
        let _guard = option_test_guard();
        install_options("melonds");
        let (ok, value) = ask_option("melonds_touch_mode");
        // The whole DS touch screen rests on this pair of assertions. If the host refuses
        // this read, the core leaves `new_touch_mode` at its C initialiser `Disabled` and
        // forces `touching = false` every frame, so the screen is dead inside the core no
        // matter how good the pointer data is. See `option_overrides`.
        assert!(ok, "the DS touch mode must be answered, not refused");
        assert_eq!(value.as_deref(), Some("Touch"));
    }

    #[test]
    fn ds_boots_the_cartridge_rather_than_the_firmware_menu() {
        let _guard = option_test_guard();
        install_options("melonds");
        let (ok, value) = ask_option("melonds_boot_directly");
        // The other half of "a DS game actually starts". Refusing this read leaves the core's
        // `int DirectBoot = 0` in place, which sends it to a firmware menu that a generated
        // firmware cannot launch a cartridge from. See `option_overrides`.
        assert!(ok, "direct boot must be answered, not refused");
        assert_eq!(value.as_deref(), Some("enabled"));
    }

    #[test]
    fn ds_gets_no_other_options_invented_for_it() {
        let _guard = option_test_guard();
        install_options("melonds");
        // Loading the DS must not turn the host into one that answers every option.
        //
        // The screen layout is the one to justify rather than assume, because the whole
        // coordinate contract rests on it: 256x384 stacked is what `CoreSpec::melonDS`, the
        // letterbox and the pointer's split at y = 0.5 all assume. Refusing it is safe here for
        // a reason that was READ rather than inferred from the advertised default, which the
        // comment on `option_overrides` explains is worth nothing: the core's own line is
        //
        //     ScreenLayout layout = ScreenLayout::TopBottom;
        //
        // so the initialiser and the advertised default agree, and refusing selects the layout
        // we want. Were that initialiser anything else, this option would need answering too.
        let (ok, value) = ask_option("melonds_screen_layout");
        assert!(!ok, "unlisted options stay refused");
        assert!(value.is_none(), "value must be nulled, not fabricated");
    }

    #[test]
    fn the_n64_gets_the_software_rasteriser_rather_than_a_missing_gl_one() {
        let _guard = option_test_guard();
        install_options("parallel_n64");

        // Without this the app FREEZES when an N64 game starts. Refusing the read skips the block
        // that would have auto-selected a renderer, leaving `gfx_plugin` at its zero initialiser,
        // which is GFX_GLIDE64 — an OpenGL plugin absent from a HAVE_OPENGL=0 build. See
        // `option_overrides`.
        let (ok, value) = ask_option("parallel-n64-gfxplugin");
        assert!(ok, "the N64 renderer must be named, not left to a C initialiser");
        assert_eq!(value.as_deref(), Some("angrylion"));

        let (ok, value) = ask_option("parallel-n64-rspplugin");
        assert!(ok, "the N64 RSP must be named for the same reason");
        assert_eq!(value.as_deref(), Some("hle"));
    }

    #[test]
    fn other_cores_keep_every_default() {
        let _guard = option_test_guard();
        for core_id in ["fceumm", "mgba", "genesis_plus_gx", "snes9x", "pcsx_rearmed"] {
            install_options(core_id);
            let (ok, value) = ask_option("melonds_touch_mode");
            assert!(!ok, "{core_id} must not be handed the DS option");
            assert!(value.is_none());
            assert!(
                option_overrides(core_id).is_empty(),
                "{core_id} is expected to run on its own defaults"
            );
        }
    }

    #[test]
    fn installing_options_replaces_rather_than_accumulates() {
        let _guard = option_test_guard();
        install_options("melonds");
        // Loading a second core must not leave the first core's answers behind. This is the
        // in-session core switch: play a DS game, go back to the library, start an NES game.
        install_options("fceumm");
        let (ok, value) = ask_option("melonds_touch_mode");
        assert!(!ok, "a previous core's overrides must not survive the next load");
        assert!(value.is_none());
    }

    #[test]
    fn get_variable_update_reports_no_change() {
        let mut changed = true;
        let ok = unsafe {
            on_environment(
                ENV_GET_VARIABLE_UPDATE,
                &mut changed as *mut bool as *mut c_void,
            )
        };
        assert!(ok);
        assert!(!changed, "no options change between polls in this host");
    }

    #[test]
    fn option_and_message_families_are_accepted_as_noops() {
        for cmd in [
            ENV_SET_VARIABLES,
            ENV_SET_CORE_OPTIONS,
            ENV_SET_CORE_OPTIONS_INTL,
            ENV_SET_CORE_OPTIONS_DISPLAY,
            ENV_SET_CORE_OPTIONS_V2,
            ENV_SET_CORE_OPTIONS_V2_INTL,
            ENV_SET_CORE_OPTIONS_UPDATE_DISPLAY_CALLBACK,
            ENV_SET_INPUT_DESCRIPTORS,
            ENV_SET_CONTROLLER_INFO,
            ENV_SET_PERFORMANCE_LEVEL,
            ENV_SET_SYSTEM_AV_INFO,
            ENV_SET_GEOMETRY,
            ENV_SET_MESSAGE,
            ENV_SET_MESSAGE_EXT,
            ENV_SET_MINIMUM_AUDIO_LATENCY,
            ENV_SET_SUPPORT_NO_GAME,
            ENV_SET_CONTENT_INFO_OVERRIDE,
        ] {
            let ok = unsafe { on_environment(cmd, std::ptr::null_mut()) };
            assert!(ok, "command {cmd} should be accepted as a no-op");
        }
    }

    #[test]
    fn audio_buffer_status_callback_is_refused() {
        // Number 62 is SET_AUDIO_BUFFER_STATUS_CALLBACK. Accepting it would promise a
        // callback we never make (SESSION_HANDOFF §1); refusing cleanly is correct.
        const SET_AUDIO_BUFFER_STATUS_CALLBACK: c_uint = 62;
        let ok = unsafe { on_environment(SET_AUDIO_BUFFER_STATUS_CALLBACK, std::ptr::null_mut()) };
        assert!(!ok, "must refuse a callback we cannot honour");
    }

    #[test]
    fn input_bitmasks_is_refused_so_core_uses_per_id_state() {
        // 51 | EXPERIMENTAL. Refusing routes the core to per-id input_state, which
        // InputSnapshot::libretro_state already serves.
        const GET_INPUT_BITMASKS: c_uint = 51 | 0x10000;
        let ok = unsafe { on_environment(GET_INPUT_BITMASKS, std::ptr::null_mut()) };
        assert!(!ok);
    }

    #[test]
    fn get_core_options_version_reports_zero() {
        let mut version: c_uint = 999;
        let ok = unsafe {
            on_environment(
                ENV_GET_CORE_OPTIONS_VERSION,
                &mut version as *mut c_uint as *mut c_void,
            )
        };
        assert!(ok);
        assert_eq!(version, 0);
    }

    #[test]
    fn from_filename_records_full_path_only_for_paths() {
        // A bare filename has no full_path; a real path retains it verbatim for
        // need_fullpath cores. This is what keeps the web build's behaviour identical.
        let bare = ContentHint::from_filename("Crash.bin");
        assert_eq!(bare.extension, "bin");
        assert_eq!(bare.name, "Crash");
        assert_eq!(bare.full_path, None);

        let path = ContentHint::from_filename("/var/mobile/roms/Crash.bin");
        assert_eq!(path.extension, "bin");
        assert_eq!(path.name, "Crash");
        assert_eq!(path.full_path.as_deref(), Some("/var/mobile/roms/Crash.bin"));
    }
}
