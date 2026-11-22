//! A small libretro host: loads one of the owner's own cores (a Rust cdylib) and runs it.
//!
//! Only one core runs at a time, so the callbacks the core calls (video, audio, input,
//! environment) reach the host through one global.

use std::cell::UnsafeCell;
use std::collections::HashMap;
use std::ffi::{c_char, c_uint, c_void, CStr, CString};
use std::ptr;

// ---------------------------------------------------------------------------------------------
// libretro.h types (32-bit and 64-bit safe: size_t is usize, pointers are pointers)

#[repr(C)]
struct RetroSystemInfo {
    library_name: *const c_char,
    library_version: *const c_char,
    valid_extensions: *const c_char,
    need_fullpath: bool,
    block_extract: bool,
}

#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct Geometry {
    pub base_width: c_uint,
    pub base_height: c_uint,
    pub max_width: c_uint,
    pub max_height: c_uint,
    pub aspect_ratio: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct Timing {
    pub fps: f64,
    pub sample_rate: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct AvInfo {
    pub geometry: Geometry,
    pub timing: Timing,
}

#[repr(C)]
struct RetroGameInfo {
    path: *const c_char,
    data: *const c_void,
    size: usize,
    meta: *const c_char,
}

#[repr(C)]
struct RetroVariable {
    key: *const c_char,
    value: *const c_char,
}

#[repr(C)]
struct OptValue {
    value: *const c_char,
    label: *const c_char,
}

#[repr(C)]
struct OptCategory {
    key: *const c_char,
    desc: *const c_char,
    info: *const c_char,
}

#[repr(C)]
struct OptDefinition {
    key: *const c_char,
    desc: *const c_char,
    desc_categorized: *const c_char,
    info: *const c_char,
    info_categorized: *const c_char,
    category_key: *const c_char,
    values: [OptValue; 128],
    default_value: *const c_char,
}

#[repr(C)]
struct OptionsV2 {
    categories: *const OptCategory,
    definitions: *const OptDefinition,
}

#[repr(C)]
struct OptDisplay {
    key: *const c_char,
    visible: bool,
}

#[repr(C)]
struct LogCallback {
    log: unsafe extern "C" fn(c_uint, *const c_char, ...),
}

#[repr(C)]
struct RumbleInterface {
    set_rumble_state: extern "C" fn(c_uint, c_uint, u16) -> bool,
}

#[repr(C)]
struct UpdateDisplayCallback {
    callback: Option<unsafe extern "C" fn() -> bool>,
}

extern "C" {
    fn continuum_log_cb(level: c_uint, fmt: *const c_char, ...);
}

#[no_mangle]
pub extern "C" fn continuum_log_line(level: c_uint, line: *const c_char) {
    if line.is_null() {
        return;
    }
    let s = unsafe { CStr::from_ptr(line) }.to_string_lossy();
    let s = s.trim_end();
    if level >= 1 || std::env::var_os("CONTINUUM_DEBUG").is_some() {
        crate::log::line(&format!("[core] {s}"));
    }
}

const ENV_EXPERIMENTAL: c_uint = 0x10000;
const ENV_GET_CAN_DUPE: c_uint = 3;
const ENV_SET_MESSAGE: c_uint = 6;
const ENV_SHUTDOWN: c_uint = 7;
const ENV_SET_PERFORMANCE_LEVEL: c_uint = 8;
const ENV_GET_SYSTEM_DIRECTORY: c_uint = 9;
const ENV_SET_PIXEL_FORMAT: c_uint = 10;
const ENV_SET_INPUT_DESCRIPTORS: c_uint = 11;
const ENV_GET_VARIABLE: c_uint = 15;
const ENV_SET_VARIABLES: c_uint = 16;
const ENV_GET_VARIABLE_UPDATE: c_uint = 17;
const ENV_GET_RUMBLE_INTERFACE: c_uint = 23;
const ENV_GET_LOG_INTERFACE: c_uint = 27;
const ENV_GET_CORE_ASSETS_DIRECTORY: c_uint = 30;
const ENV_GET_SAVE_DIRECTORY: c_uint = 31;
const ENV_SET_SYSTEM_AV_INFO: c_uint = 32;
const ENV_SET_CONTROLLER_INFO: c_uint = 35;
const ENV_SET_GEOMETRY: c_uint = 37;
const ENV_GET_LANGUAGE: c_uint = 39;
const ENV_GET_FASTFORWARDING: c_uint = 49 | ENV_EXPERIMENTAL;
const ENV_GET_INPUT_BITMASKS: c_uint = 51 | ENV_EXPERIMENTAL;
const ENV_GET_CORE_OPTIONS_VERSION: c_uint = 52;
const ENV_SET_CORE_OPTIONS_DISPLAY: c_uint = 55;
const ENV_GET_INPUT_MAX_USERS: c_uint = 61;
const ENV_SET_CORE_OPTIONS_V2: c_uint = 67;
const ENV_SET_CORE_OPTIONS_UPDATE_DISPLAY_CALLBACK: c_uint = 69;

const PIXEL_0RGB1555: c_uint = 0;
const PIXEL_XRGB8888: c_uint = 1;
const PIXEL_RGB565: c_uint = 2;

pub const DEVICE_JOYPAD: c_uint = 1;
pub const DEVICE_ANALOG: c_uint = 5;
pub const JOYPAD_MASK: c_uint = 256;

pub const MEMORY_SAVE_RAM: c_uint = 0;
pub const MEMORY_RTC: c_uint = 1;

// Joypad ids
pub const B: u32 = 0;
pub const Y: u32 = 1;
pub const SELECT: u32 = 2;
pub const START: u32 = 3;
pub const UP: u32 = 4;
pub const DOWN: u32 = 5;
pub const LEFT: u32 = 6;
pub const RIGHT: u32 = 7;
pub const A: u32 = 8;
pub const X: u32 = 9;
pub const L: u32 = 10;
pub const R: u32 = 11;

// ---------------------------------------------------------------------------------------------
// Core options, as the core declared them

#[derive(Clone, Debug)]
pub struct OptionDef {
    pub key: String,
    pub label: String,
    pub info: String,
    pub category: String,
    pub values: Vec<(String, String)>,
    pub default: String,
}

#[derive(Clone, Debug)]
pub struct OptionCategory {
    pub key: String,
    pub label: String,
    pub info: String,
}

#[derive(Default)]
pub struct Options {
    pub defs: Vec<OptionDef>,
    pub categories: Vec<OptionCategory>,
    values: HashMap<String, CString>,
    pub hidden: HashMap<String, bool>,
    dirty: bool,
    update_display: Option<unsafe extern "C" fn() -> bool>,
}

impl Options {
    pub fn get(&self, key: &str) -> Option<String> {
        self.values.get(key).map(|v| v.to_string_lossy().into_owned())
    }

    /// Change one option. The core picks it up on its next frame.
    pub fn set(&mut self, key: &str, value: &str) {
        if let Ok(c) = CString::new(value) {
            self.values.insert(key.to_string(), c);
            self.dirty = true;
        }
    }

    pub fn is_visible(&self, key: &str) -> bool {
        !self.hidden.get(key).copied().unwrap_or(false)
    }

    /// The values the player picked, to save.
    pub fn snapshot(&self) -> Vec<(String, String)> {
        let mut v: Vec<_> = self
            .values
            .iter()
            .map(|(k, c)| (k.clone(), c.to_string_lossy().into_owned()))
            .collect();
        v.sort();
        v
    }
}

// ---------------------------------------------------------------------------------------------
// The host side the callbacks write into

pub struct Host {
    pub frame: Vec<u32>,
    pub frame_w: u32,
    pub frame_h: u32,
    pub frame_new: bool,
    pub audio: Vec<i16>,
    pub buttons: [u16; 2],
    pub analog: [[i16; 4]; 2],
    pub options: Options,
    pub rumble: [u16; 2],
    pub rumble_changed: bool,
    pub geometry_changed: bool,
    pub av_override: Option<AvInfo>,
    pub fast_forwarding: bool,
    pub shutdown: bool,
    pub message: Option<String>,
    pixel_format: c_uint,
    system_dir: CString,
    save_dir: CString,
    /// Option values to apply as soon as the core declares its options (saved settings).
    pub preset: Vec<(String, String)>,
}

struct HostCell(UnsafeCell<Option<Host>>);
unsafe impl Sync for HostCell {}
static HOST: HostCell = HostCell(UnsafeCell::new(None));

/// The host state. The app is single-threaded where the core is concerned: the core only calls
/// back from inside retro_* calls made on the main thread.
#[allow(clippy::mut_from_ref)]
pub fn host() -> &'static mut Host {
    unsafe { (*HOST.0.get()).as_mut().expect("libretro host not set up") }
}

fn cstr(p: *const c_char) -> String {
    if p.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
    }
}

unsafe extern "C" fn environment(cmd: c_uint, data: *mut c_void) -> bool {
    let h = host();
    match cmd {
        ENV_GET_CAN_DUPE => {
            if !data.is_null() {
                *(data as *mut bool) = true;
            }
            true
        }
        ENV_SET_MESSAGE => {
            if !data.is_null() {
                let msg = *(data as *const *const c_char);
                h.message = Some(cstr(msg));
            }
            true
        }
        ENV_SHUTDOWN => {
            h.shutdown = true;
            true
        }
        ENV_SET_PERFORMANCE_LEVEL | ENV_SET_INPUT_DESCRIPTORS | ENV_SET_CONTROLLER_INFO => true,
        ENV_GET_SYSTEM_DIRECTORY | ENV_GET_CORE_ASSETS_DIRECTORY => {
            *(data as *mut *const c_char) = h.system_dir.as_ptr();
            true
        }
        ENV_GET_SAVE_DIRECTORY => {
            *(data as *mut *const c_char) = h.save_dir.as_ptr();
            true
        }
        ENV_SET_PIXEL_FORMAT => {
            let f = *(data as *const c_uint);
            if f == PIXEL_XRGB8888 || f == PIXEL_RGB565 || f == PIXEL_0RGB1555 {
                h.pixel_format = f;
                true
            } else {
                false
            }
        }
        ENV_GET_VARIABLE => {
            let var = &mut *(data as *mut RetroVariable);
            let key = cstr(var.key);
            match h.options.values.get(&key) {
                Some(v) => {
                    var.value = v.as_ptr();
                    true
                }
                None => {
                    var.value = ptr::null();
                    false
                }
            }
        }
        ENV_SET_VARIABLES => {
            let mut p = data as *const RetroVariable;
            let mut defs = Vec::new();
            while !p.is_null() && !(*p).key.is_null() {
                let key = cstr((*p).key);
                let text = cstr((*p).value);
                let (label, rest) = text.split_once("; ").unwrap_or((text.as_str(), ""));
                let values: Vec<(String, String)> =
                    rest.split('|').filter(|s| !s.is_empty()).map(|s| (s.to_string(), s.to_string())).collect();
                let default = values.first().map(|v| v.0.clone()).unwrap_or_default();
                defs.push(OptionDef {
                    key,
                    label: label.to_string(),
                    info: String::new(),
                    category: String::new(),
                    values,
                    default,
                });
                p = p.add(1);
            }
            install_options(h, defs, Vec::new());
            true
        }
        ENV_GET_VARIABLE_UPDATE => {
            *(data as *mut bool) = h.options.dirty;
            h.options.dirty = false;
            true
        }
        ENV_GET_RUMBLE_INTERFACE => {
            (*(data as *mut RumbleInterface)).set_rumble_state = set_rumble;
            true
        }
        ENV_GET_LOG_INTERFACE => {
            (*(data as *mut LogCallback)).log = continuum_log_cb;
            true
        }
        ENV_SET_SYSTEM_AV_INFO => {
            h.av_override = Some(*(data as *const AvInfo));
            h.geometry_changed = true;
            true
        }
        ENV_SET_GEOMETRY => {
            let g = *(data as *const Geometry);
            let mut av = h.av_override.unwrap_or_default();
            av.geometry = g;
            h.av_override = Some(av);
            h.geometry_changed = true;
            true
        }
        ENV_GET_LANGUAGE => {
            *(data as *mut c_uint) = 0;
            true
        }
        ENV_GET_FASTFORWARDING => {
            if !data.is_null() {
                *(data as *mut bool) = h.fast_forwarding;
            }
            true
        }
        ENV_GET_INPUT_BITMASKS => true,
        ENV_GET_CORE_OPTIONS_VERSION => {
            *(data as *mut c_uint) = 2;
            true
        }
        ENV_GET_INPUT_MAX_USERS => {
            *(data as *mut c_uint) = 2;
            true
        }
        ENV_SET_CORE_OPTIONS_V2 => {
            let o = &*(data as *const OptionsV2);
            let mut cats = Vec::new();
            let mut c = o.categories;
            while !c.is_null() && !(*c).key.is_null() {
                cats.push(OptionCategory { key: cstr((*c).key), label: cstr((*c).desc), info: cstr((*c).info) });
                c = c.add(1);
            }
            let mut defs = Vec::new();
            let mut d = o.definitions;
            while !d.is_null() && !(*d).key.is_null() {
                let def = &*d;
                let mut values = Vec::new();
                for v in def.values.iter() {
                    if v.value.is_null() {
                        break;
                    }
                    let value = cstr(v.value);
                    let label = if v.label.is_null() { value.clone() } else { cstr(v.label) };
                    values.push((value, label));
                }
                let label = if def.desc_categorized.is_null() { cstr(def.desc) } else { cstr(def.desc_categorized) };
                let info = if def.info_categorized.is_null() { cstr(def.info) } else { cstr(def.info_categorized) };
                let default = if def.default_value.is_null() {
                    values.first().map(|v| v.0.clone()).unwrap_or_default()
                } else {
                    cstr(def.default_value)
                };
                defs.push(OptionDef { key: cstr(def.key), label, info, category: cstr(def.category_key), values, default });
                d = d.add(1);
            }
            install_options(h, defs, cats);
            true
        }
        ENV_SET_CORE_OPTIONS_DISPLAY => {
            let d = &*(data as *const OptDisplay);
            if !d.key.is_null() {
                h.options.hidden.insert(cstr(d.key), !d.visible);
            }
            true
        }
        ENV_SET_CORE_OPTIONS_UPDATE_DISPLAY_CALLBACK => {
            if !data.is_null() {
                h.options.update_display = (*(data as *const UpdateDisplayCallback)).callback;
            }
            true
        }
        _ => false,
    }
}

fn install_options(h: &mut Host, defs: Vec<OptionDef>, cats: Vec<OptionCategory>) {
    for d in &defs {
        let saved = h.preset.iter().find(|(k, _)| k == &d.key).map(|(_, v)| v.clone());
        let value = match saved {
            Some(v) if d.values.iter().any(|(x, _)| *x == v) || d.values.is_empty() => v,
            _ => d.default.clone(),
        };
        if let Ok(c) = CString::new(value) {
            h.options.values.insert(d.key.clone(), c);
        }
    }
    h.options.defs = defs;
    h.options.categories = cats;
    h.options.dirty = true;
}

extern "C" fn set_rumble(port: c_uint, _effect: c_uint, strength: u16) -> bool {
    let h = host();
    if (port as usize) < 2 {
        h.rumble[port as usize] = strength;
        h.rumble_changed = true;
    }
    true
}

unsafe extern "C" fn video_refresh(data: *const c_void, width: c_uint, height: c_uint, pitch: usize) {
    let h = host();
    if data.is_null() || width == 0 || height == 0 {
        return; // a repeated frame: keep the last one
    }
    let (w, hh) = (width as usize, height as usize);
    if h.frame.len() != w * hh {
        h.frame.resize(w * hh, 0);
    }
    match h.pixel_format {
        PIXEL_XRGB8888 => {
            for y in 0..hh {
                let src = std::slice::from_raw_parts((data as *const u8).add(y * pitch) as *const u32, w);
                h.frame[y * w..(y + 1) * w].copy_from_slice(src);
            }
        }
        PIXEL_RGB565 => {
            for y in 0..hh {
                let src = std::slice::from_raw_parts((data as *const u8).add(y * pitch) as *const u16, w);
                for (x, &p) in src.iter().enumerate() {
                    let r = ((p >> 11) & 0x1f) as u32;
                    let g = ((p >> 5) & 0x3f) as u32;
                    let b = (p & 0x1f) as u32;
                    h.frame[y * w + x] = ((r << 3 | r >> 2) << 16) | ((g << 2 | g >> 4) << 8) | (b << 3 | b >> 2);
                }
            }
        }
        _ => {
            for y in 0..hh {
                let src = std::slice::from_raw_parts((data as *const u8).add(y * pitch) as *const u16, w);
                for (x, &p) in src.iter().enumerate() {
                    let r = ((p >> 10) & 0x1f) as u32;
                    let g = ((p >> 5) & 0x1f) as u32;
                    let b = (p & 0x1f) as u32;
                    h.frame[y * w + x] = ((r << 3 | r >> 2) << 16) | ((g << 3 | g >> 2) << 8) | (b << 3 | b >> 2);
                }
            }
        }
    }
    h.frame_w = width;
    h.frame_h = height;
    h.frame_new = true;
}

unsafe extern "C" fn audio_sample(left: i16, right: i16) {
    let h = host();
    h.audio.push(left);
    h.audio.push(right);
}

unsafe extern "C" fn audio_batch(data: *const i16, frames: usize) -> usize {
    let h = host();
    if !data.is_null() {
        h.audio.extend_from_slice(std::slice::from_raw_parts(data, frames * 2));
    }
    frames
}

unsafe extern "C" fn input_poll() {}

unsafe extern "C" fn input_state(port: c_uint, device: c_uint, index: c_uint, id: c_uint) -> i16 {
    let h = host();
    let p = port as usize;
    if p >= 2 {
        return 0;
    }
    match device & 0xff {
        DEVICE_JOYPAD => {
            if id == JOYPAD_MASK {
                h.buttons[p] as i16
            } else if id < 16 {
                ((h.buttons[p] >> id) & 1) as i16
            } else {
                0
            }
        }
        DEVICE_ANALOG => {
            let i = (index as usize).min(1) * 2 + (id as usize).min(1);
            h.analog[p][i]
        }
        _ => 0,
    }
}

// ---------------------------------------------------------------------------------------------
// The core

type FnVoid = unsafe extern "C" fn();
type FnApi = unsafe extern "C" fn() -> c_uint;
type FnSysInfo = unsafe extern "C" fn(*mut RetroSystemInfo);
type FnAvInfo = unsafe extern "C" fn(*mut AvInfo);
type FnSetEnv = unsafe extern "C" fn(unsafe extern "C" fn(c_uint, *mut c_void) -> bool);
type FnSetVideo = unsafe extern "C" fn(unsafe extern "C" fn(*const c_void, c_uint, c_uint, usize));
type FnSetSample = unsafe extern "C" fn(unsafe extern "C" fn(i16, i16));
type FnSetBatch = unsafe extern "C" fn(unsafe extern "C" fn(*const i16, usize) -> usize);
type FnSetPoll = unsafe extern "C" fn(unsafe extern "C" fn());
type FnSetState = unsafe extern "C" fn(unsafe extern "C" fn(c_uint, c_uint, c_uint, c_uint) -> i16);
type FnSetPort = unsafe extern "C" fn(c_uint, c_uint);
type FnSerSize = unsafe extern "C" fn() -> usize;
type FnSer = unsafe extern "C" fn(*mut c_void, usize) -> bool;
type FnUnser = unsafe extern "C" fn(*const c_void, usize) -> bool;
type FnLoad = unsafe extern "C" fn(*const RetroGameInfo) -> bool;
type FnMemData = unsafe extern "C" fn(c_uint) -> *mut c_void;
type FnMemSize = unsafe extern "C" fn(c_uint) -> usize;

struct Fns {
    init: FnVoid,
    deinit: FnVoid,
    api_version: FnApi,
    get_system_info: FnSysInfo,
    get_system_av_info: FnAvInfo,
    set_environment: FnSetEnv,
    set_video_refresh: FnSetVideo,
    set_audio_sample: FnSetSample,
    set_audio_sample_batch: FnSetBatch,
    set_input_poll: FnSetPoll,
    set_input_state: FnSetState,
    set_controller_port_device: FnSetPort,
    reset: FnVoid,
    run: FnVoid,
    serialize_size: FnSerSize,
    serialize: FnSer,
    unserialize: FnUnser,
    load_game: FnLoad,
    unload_game: FnVoid,
    get_memory_data: FnMemData,
    get_memory_size: FnMemSize,
}

pub struct SystemInfo {
    pub name: String,
    pub version: String,
    pub extensions: Vec<String>,
}

pub struct Core {
    handle: *mut c_void,
    f: Fns,
    pub info: SystemInfo,
    pub av: AvInfo,
    loaded: bool,
    _rom: Vec<u8>,
    _path: CString,
}

unsafe fn sym<T: Copy>(handle: *mut c_void, name: &str) -> Result<T, String> {
    let c = CString::new(name).unwrap();
    let p = libc::dlsym(handle, c.as_ptr());
    if p.is_null() {
        return Err(format!("the core has no {name}"));
    }
    Ok(std::mem::transmute_copy::<*mut c_void, T>(&p))
}

pub struct CoreDirs {
    pub system: String,
    pub saves: String,
}

impl Core {
    /// Load the core library, set up the host, and start the core (retro_init).
    pub fn open(path: &str, dirs: &CoreDirs, preset: Vec<(String, String)>) -> Result<Core, String> {
        unsafe {
            let c = CString::new(path).map_err(|e| e.to_string())?;
            let handle = libc::dlopen(c.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL);
            if handle.is_null() {
                let e = libc::dlerror();
                return Err(format!("can't open the core: {}", cstr(e)));
            }
            let f = Fns {
                init: sym(handle, "retro_init")?,
                deinit: sym(handle, "retro_deinit")?,
                api_version: sym(handle, "retro_api_version")?,
                get_system_info: sym(handle, "retro_get_system_info")?,
                get_system_av_info: sym(handle, "retro_get_system_av_info")?,
                set_environment: sym(handle, "retro_set_environment")?,
                set_video_refresh: sym(handle, "retro_set_video_refresh")?,
                set_audio_sample: sym(handle, "retro_set_audio_sample")?,
                set_audio_sample_batch: sym(handle, "retro_set_audio_sample_batch")?,
                set_input_poll: sym(handle, "retro_set_input_poll")?,
                set_input_state: sym(handle, "retro_set_input_state")?,
                set_controller_port_device: sym(handle, "retro_set_controller_port_device")?,
                reset: sym(handle, "retro_reset")?,
                run: sym(handle, "retro_run")?,
                serialize_size: sym(handle, "retro_serialize_size")?,
                serialize: sym(handle, "retro_serialize")?,
                unserialize: sym(handle, "retro_unserialize")?,
                load_game: sym(handle, "retro_load_game")?,
                unload_game: sym(handle, "retro_unload_game")?,
                get_memory_data: sym(handle, "retro_get_memory_data")?,
                get_memory_size: sym(handle, "retro_get_memory_size")?,
            };
            if (f.api_version)() != 1 {
                return Err("the core speaks a different libretro version".into());
            }
            *HOST.0.get() = Some(Host {
                frame: Vec::new(),
                frame_w: 0,
                frame_h: 0,
                frame_new: false,
                audio: Vec::with_capacity(4096),
                buttons: [0; 2],
                analog: [[0; 4]; 2],
                options: Options::default(),
                rumble: [0; 2],
                rumble_changed: false,
                geometry_changed: false,
                av_override: None,
                fast_forwarding: false,
                shutdown: false,
                message: None,
                pixel_format: PIXEL_0RGB1555,
                system_dir: CString::new(dirs.system.as_str()).unwrap_or_default(),
                save_dir: CString::new(dirs.saves.as_str()).unwrap_or_default(),
                preset,
            });
            (f.set_environment)(environment);
            (f.set_video_refresh)(video_refresh);
            (f.set_audio_sample)(audio_sample);
            (f.set_audio_sample_batch)(audio_batch);
            (f.set_input_poll)(input_poll);
            (f.set_input_state)(input_state);
            (f.init)();
            let mut si = RetroSystemInfo {
                library_name: ptr::null(),
                library_version: ptr::null(),
                valid_extensions: ptr::null(),
                need_fullpath: false,
                block_extract: false,
            };
            (f.get_system_info)(&mut si);
            let info = SystemInfo {
                name: cstr(si.library_name),
                version: cstr(si.library_version),
                extensions: cstr(si.valid_extensions).split('|').map(|s| s.to_ascii_lowercase()).collect(),
            };
            Ok(Core {
                handle,
                f,
                info,
                av: AvInfo::default(),
                loaded: false,
                _rom: Vec::new(),
                _path: CString::default(),
            })
        }
    }

    /// Start a game from its bytes (the core never reads the file itself).
    pub fn load(&mut self, path: &str, rom: Vec<u8>) -> Result<(), String> {
        unsafe {
            self._path = CString::new(path).unwrap_or_default();
            self._rom = rom;
            let gi = RetroGameInfo {
                path: self._path.as_ptr(),
                data: self._rom.as_ptr() as *const c_void,
                size: self._rom.len(),
                meta: ptr::null(),
            };
            if !(self.f.load_game)(&gi) {
                return Err("the game didn't start".into());
            }
            self.loaded = true;
            (self.f.set_controller_port_device)(0, DEVICE_JOYPAD);
            self.refresh_av();
            Ok(())
        }
    }

    pub fn refresh_av(&mut self) {
        let mut av = AvInfo::default();
        unsafe { (self.f.get_system_av_info)(&mut av) };
        if let Some(o) = host().av_override.take() {
            if o.timing.fps > 0.0 {
                av.timing = o.timing;
            }
            if o.geometry.base_width > 0 {
                av.geometry = o.geometry;
            }
        }
        self.av = av;
    }

    pub fn run(&mut self) {
        unsafe { (self.f.run)() };
        if host().geometry_changed {
            host().geometry_changed = false;
            self.refresh_av();
        }
    }

    pub fn reset(&mut self) {
        unsafe { (self.f.reset)() };
    }

    pub fn save_state(&mut self) -> Option<Vec<u8>> {
        unsafe {
            let n = (self.f.serialize_size)();
            if n == 0 {
                return None;
            }
            let mut buf = vec![0u8; n];
            if (self.f.serialize)(buf.as_mut_ptr() as *mut c_void, n) {
                Some(buf)
            } else {
                None
            }
        }
    }

    pub fn load_state(&mut self, data: &[u8]) -> bool {
        unsafe { (self.f.unserialize)(data.as_ptr() as *const c_void, data.len()) }
    }

    /// A memory region (battery save or clock), if the game has one.
    pub fn memory(&mut self, id: c_uint) -> Option<&mut [u8]> {
        unsafe {
            let p = (self.f.get_memory_data)(id);
            let n = (self.f.get_memory_size)(id);
            if p.is_null() || n == 0 {
                None
            } else {
                Some(std::slice::from_raw_parts_mut(p as *mut u8, n))
            }
        }
    }

    /// Tell the core its options changed the visibility of others (after a change).
    pub fn refresh_option_display(&mut self) {
        if let Some(cb) = host().options.update_display {
            unsafe { cb() };
        }
    }
}

impl Drop for Core {
    fn drop(&mut self) {
        unsafe {
            if self.loaded {
                (self.f.unload_game)();
            }
            (self.f.deinit)();
            // The library stays mapped: Rust cdylibs keep thread-local destructors registered,
            // and unloading one while they are pending crashes on exit.
            let _ = self.handle;
        }
    }
}
