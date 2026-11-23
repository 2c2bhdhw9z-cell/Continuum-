//! The few SDL2 functions the app uses, loaded at run time from the app's own lib/ folder
//! (webosbrew's SDL2 webOS build, the same one RetroArch ships on this TV).

#![allow(non_snake_case, dead_code)]

use std::ffi::{c_char, c_int, c_void, CStr, CString};

pub const INIT_TIMER: u32 = 0x1;
pub const INIT_AUDIO: u32 = 0x10;
pub const INIT_VIDEO: u32 = 0x20;
pub const INIT_JOYSTICK: u32 = 0x200;
pub const INIT_HAPTIC: u32 = 0x1000;
pub const INIT_GAMECONTROLLER: u32 = 0x2000;
pub const INIT_EVENTS: u32 = 0x4000;

pub const WINDOW_FULLSCREEN: u32 = 0x1;
pub const WINDOW_OPENGL: u32 = 0x2;
pub const WINDOW_SHOWN: u32 = 0x4;
pub const WINDOW_FULLSCREEN_DESKTOP: u32 = 0x1001;

pub const GL_RED_SIZE: c_int = 0;
pub const GL_GREEN_SIZE: c_int = 1;
pub const GL_BLUE_SIZE: c_int = 2;
pub const GL_ALPHA_SIZE: c_int = 3;
pub const GL_DOUBLEBUFFER: c_int = 5;
pub const GL_DEPTH_SIZE: c_int = 6;
pub const GL_CONTEXT_MAJOR_VERSION: c_int = 17;
pub const GL_CONTEXT_MINOR_VERSION: c_int = 18;
pub const GL_CONTEXT_PROFILE_MASK: c_int = 21;
pub const GL_CONTEXT_PROFILE_ES: c_int = 4;

pub const QUIT: u32 = 0x100;
pub const APP_TERMINATING: u32 = 0x101;
pub const APP_WILLENTERBACKGROUND: u32 = 0x103;
pub const APP_DIDENTERBACKGROUND: u32 = 0x104;
pub const APP_WILLENTERFOREGROUND: u32 = 0x105;
pub const APP_DIDENTERFOREGROUND: u32 = 0x106;
pub const WINDOWEVENT: u32 = 0x200;
pub const KEYDOWN: u32 = 0x300;
pub const KEYUP: u32 = 0x301;
pub const CONTROLLERAXISMOTION: u32 = 0x650;
pub const CONTROLLERBUTTONDOWN: u32 = 0x651;
pub const CONTROLLERBUTTONUP: u32 = 0x652;
pub const CONTROLLERDEVICEADDED: u32 = 0x653;
pub const CONTROLLERDEVICEREMOVED: u32 = 0x654;

pub const WINDOWEVENT_SHOWN: u8 = 1;
pub const WINDOWEVENT_HIDDEN: u8 = 2;
pub const WINDOWEVENT_FOCUS_GAINED: u8 = 12;
pub const WINDOWEVENT_FOCUS_LOST: u8 = 13;

pub const AUDIO_S16LSB: u16 = 0x8010;

/// SDL_Event is a 56-byte union; the app reads the few fields it needs by offset.
#[repr(C, align(8))]
pub struct Event {
    pub raw: [u8; 56],
}

impl Event {
    pub fn new() -> Event {
        Event { raw: [0; 56] }
    }
    fn u32_at(&self, o: usize) -> u32 {
        u32::from_ne_bytes(self.raw[o..o + 4].try_into().unwrap())
    }
    fn i32_at(&self, o: usize) -> i32 {
        i32::from_ne_bytes(self.raw[o..o + 4].try_into().unwrap())
    }
    pub fn kind(&self) -> u32 {
        self.u32_at(0)
    }
    /// Keyboard: (scancode, keycode, repeat). webosbrew's SDL is built with
    /// SDL_WEBOS_BROKEN_ABI: its key events carry an extra `inputSource` after `windowID`, so
    /// everything after it sits four bytes later than in standard SDL (measured on the TV).
    pub fn key(&self) -> (i32, i32, bool) {
        if cfg!(target_arch = "arm") {
            (self.i32_at(20), self.i32_at(24), self.raw[17] != 0)
        } else {
            (self.i32_at(16), self.i32_at(20), self.raw[13] != 0)
        }
    }
    /// Controller button: (joystick instance id, button)
    pub fn cbutton(&self) -> (i32, u8) {
        (self.i32_at(8), self.raw[12])
    }
    /// Controller axis: (instance id, axis, value)
    pub fn caxis(&self) -> (i32, u8, i16) {
        (self.i32_at(8), self.raw[12], i16::from_ne_bytes([self.raw[16], self.raw[17]]))
    }
    /// Device added: device index; removed: instance id.
    pub fn cdevice(&self) -> i32 {
        self.i32_at(8)
    }
    pub fn window_event(&self) -> u8 {
        self.raw[12]
    }
}

#[repr(C)]
pub struct AudioSpec {
    pub freq: c_int,
    pub format: u16,
    pub channels: u8,
    pub silence: u8,
    pub samples: u16,
    pub padding: u16,
    pub size: u32,
    pub callback: *const c_void,
    pub userdata: *mut c_void,
}

macro_rules! sdl_fns {
    ($($name:ident : fn($($arg:ty),*) $(-> $ret:ty)?;)*) => {
        pub struct Sdl {
            $(pub $name: unsafe extern "C" fn($($arg),*) $(-> $ret)?,)*
        }
        impl Sdl {
            unsafe fn load(h: *mut c_void) -> Result<Sdl, String> {
                Ok(Sdl {
                    $($name: {
                        let n = CString::new(concat!("SDL_", stringify!($name))).unwrap();
                        let p = libc::dlsym(h, n.as_ptr());
                        if p.is_null() {
                            return Err(format!("SDL has no {}", stringify!($name)));
                        }
                        std::mem::transmute::<*mut c_void, unsafe extern "C" fn($($arg),*) $(-> $ret)?>(p)
                    },)*
                })
            }
        }
    };
}

sdl_fns! {
    Init: fn(u32) -> c_int;
    Quit: fn();
    GetError: fn() -> *const c_char;
    SetHint: fn(*const c_char, *const c_char) -> c_int;
    CreateWindow: fn(*const c_char, c_int, c_int, c_int, c_int, u32) -> *mut c_void;
    GL_SetAttribute: fn(c_int, c_int) -> c_int;
    GL_CreateContext: fn(*mut c_void) -> *mut c_void;
    GL_MakeCurrent: fn(*mut c_void, *mut c_void) -> c_int;
    GL_SetSwapInterval: fn(c_int) -> c_int;
    GL_SwapWindow: fn(*mut c_void);
    GL_GetProcAddress: fn(*const c_char) -> *mut c_void;
    GL_GetDrawableSize: fn(*mut c_void, *mut c_int, *mut c_int);
    PollEvent: fn(*mut Event) -> c_int;
    OpenAudioDevice: fn(*const c_char, c_int, *const AudioSpec, *mut AudioSpec, c_int) -> u32;
    PauseAudioDevice: fn(u32, c_int);
    QueueAudio: fn(u32, *const c_void, u32) -> c_int;
    GetQueuedAudioSize: fn(u32) -> u32;
    ClearQueuedAudio: fn(u32);
    CloseAudioDevice: fn(u32);
    NumJoysticks: fn() -> c_int;
    IsGameController: fn(c_int) -> c_int;
    GameControllerOpen: fn(c_int) -> *mut c_void;
    GameControllerClose: fn(*mut c_void);
    GameControllerGetJoystick: fn(*mut c_void) -> *mut c_void;
    GameControllerName: fn(*mut c_void) -> *const c_char;
    GameControllerRumble: fn(*mut c_void, u16, u16, u32) -> c_int;
    JoystickInstanceID: fn(*mut c_void) -> i32;
    ShowCursor: fn(c_int) -> c_int;
    GetTicks: fn() -> u32;
    Delay: fn(u32);
    GetScancodeName: fn(c_int) -> *const c_char;
    RaiseWindow: fn(*mut c_void);
    ShowWindow: fn(*mut c_void);
    RestoreWindow: fn(*mut c_void);
}

impl Sdl {
    pub fn open(path: &str) -> Result<Sdl, String> {
        unsafe {
            let c = CString::new(path).unwrap();
            let h = libc::dlopen(c.as_ptr(), libc::RTLD_NOW | libc::RTLD_GLOBAL);
            if h.is_null() {
                let e = libc::dlerror();
                let msg = if e.is_null() { String::new() } else { CStr::from_ptr(e).to_string_lossy().into_owned() };
                return Err(format!("can't open SDL: {msg}"));
            }
            Sdl::load(h)
        }
    }

    pub fn error(&self) -> String {
        unsafe {
            let p = (self.GetError)();
            if p.is_null() {
                String::new()
            } else {
                CStr::from_ptr(p).to_string_lossy().into_owned()
            }
        }
    }

    pub fn hint(&self, name: &str, value: &str) {
        let (n, v) = (CString::new(name).unwrap(), CString::new(value).unwrap());
        unsafe { (self.SetHint)(n.as_ptr(), v.as_ptr()) };
    }

    pub fn scancode_name(&self, sc: i32) -> String {
        unsafe {
            let p = (self.GetScancodeName)(sc);
            if p.is_null() {
                String::new()
            } else {
                CStr::from_ptr(p).to_string_lossy().into_owned()
            }
        }
    }
}
