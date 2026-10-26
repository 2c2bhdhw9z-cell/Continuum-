//! A Game Gear's Gear-to-Gear cable to another phone, through the real libretro entry points,
//! with this test playing the app and the other phone: cGear offers the link for a Game Gear
//! game, a byte the game sends leaves the phone as a packet, and the other phone's byte reaches
//! the game.

use std::ffi::{c_char, c_void};
use std::sync::Mutex;

use continuum_sms::libretro::*;

type SendFn = unsafe extern "C" fn(i32, *const c_void, usize, u16);
type StartFn = unsafe extern "C" fn(u16, Option<SendFn>, Option<unsafe extern "C" fn()>);
type ReceiveFn = unsafe extern "C" fn(*const c_void, usize, u16);
type PlainFn = unsafe extern "C" fn();

#[repr(C)]
#[derive(Clone, Copy)]
struct Netpacket {
    start: Option<StartFn>,
    receive: Option<ReceiveFn>,
    stop: Option<PlainFn>,
    poll: Option<PlainFn>,
    connected: Option<unsafe extern "C" fn(u16) -> bool>,
    disconnected: Option<unsafe extern "C" fn(u16)>,
    protocol_version: *const c_char,
}
unsafe impl Send for Netpacket {}

struct App {
    iface: Option<Netpacket>,
    sent: Vec<Vec<u8>>,
    protocol: String,
}
static APP: Mutex<App> = Mutex::new(App { iface: None, sent: Vec::new(), protocol: String::new() });

#[repr(C)]
struct Var {
    key: *const c_char,
    value: *const c_char,
}

fn app() -> std::sync::MutexGuard<'static, App> {
    APP.lock().unwrap_or_else(|e| e.into_inner())
}

unsafe extern "C" fn send(_flags: i32, buf: *const c_void, len: usize, client: u16) {
    assert_eq!(client, 0xFFFF);
    app().sent.push(std::slice::from_raw_parts(buf as *const u8, len).to_vec());
}

unsafe extern "C" fn env(cmd: u32, d: *mut c_void) -> bool {
    match cmd {
        10 => true,
        15 => {
            let v = &mut *(d as *mut Var);
            if std::ffi::CStr::from_ptr(v.key).to_bytes() == b"cgear_console" {
                v.value = c"gg".as_ptr();
                return true;
            }
            false
        }
        78 => {
            let iface = *(d as *const Netpacket);
            let mut a = app();
            a.protocol = std::ffi::CStr::from_ptr(iface.protocol_version).to_string_lossy().into_owned();
            a.iface = Some(iface);
            true
        }
        _ => false,
    }
}
unsafe extern "C" fn video(_d: *const c_void, _w: u32, _h: u32, _p: usize) {}
unsafe extern "C" fn audio(_d: *const i16, n: usize) -> usize {
    n
}
unsafe extern "C" fn poll_input() {}
unsafe extern "C" fn input(_p: u32, _d: u32, _i: u32, _id: u32) -> i16 {
    0
}

/// Serial on at 4800 bps with an NMI on receipt; sends $5A, waits for a byte and keeps it at
/// $C000. The NMI just returns.
fn cartridge() -> Vec<u8> {
    let code: &[u8] = &[
        0xF3, 0x31, 0xF0, 0xDF, 0x3E, 0x38, 0xD3, 0x05, 0x3E, 0x5A, 0xD3, 0x03,
        0xDB, 0x05, 0xCB, 0x4F, 0x28, 0xFA, // wait: IN A,($05) / BIT 1,A / JR Z,wait
        0xDB, 0x04, 0x32, 0x00, 0xC0, 0x18, 0xFE,
    ];
    let mut rom = vec![0u8; 0x8000];
    rom[..code.len()].copy_from_slice(code);
    rom[0x66] = 0xED;
    rom[0x67] = 0x45;
    rom
}

#[test]
fn a_byte_goes_to_the_other_phone_and_its_byte_comes_back() {
    let rom = cartridge();
    unsafe {
        retro_set_environment(env);
        retro_set_video_refresh(video);
        retro_set_audio_sample_batch(audio);
        retro_set_input_poll(poll_input);
        retro_set_input_state(input);
        retro_init();
        let info = RetroGameInfo { path: std::ptr::null(), data: rom.as_ptr() as *const c_void, size: rom.len(), meta: std::ptr::null() };
        assert!(retro_load_game(&info));
        let iface = app().iface.expect("cGear did not offer the link for a Game Gear game");
        assert_eq!(app().protocol, "cgear-gg-link-1");
        (iface.start.unwrap())(0, Some(send), None);
        for _ in 0..3 {
            retro_run();
            (iface.poll.unwrap())();
        }
        assert!(app().sent.contains(&vec![b'S', 0x5A]), "the byte left: {:?}", app().sent);
        let ram = retro_get_memory_data(2) as *const u8;
        assert_ne!(*ram, 0x33);
        let answer = [b'S', 0x33u8];
        (iface.receive.unwrap())(answer.as_ptr() as *const c_void, 2, 0);
        for _ in 0..2 {
            retro_run();
            (iface.poll.unwrap())();
        }
        assert_eq!(*ram, 0x33, "the other phone's byte reached the game");
        (iface.stop.unwrap())();
        retro_unload_game();
        retro_deinit();
    }
}
