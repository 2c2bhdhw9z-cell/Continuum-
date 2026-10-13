//! The two-phone link cable driven through the real libretro entry points, with this test
//! playing the app and the other phone.
//!
//! The core is one global, so only one machine can run here; the other end of the cable is this
//! file, which is what the app itself will be. What is checked: that cBoy offers the link, that a
//! byte the game sends really leaves the phone with the right block around it, that the answer
//! finishes the game's transfer, that a phone with no link at all plays on exactly as before, and
//! that a partner who stops answering cannot leave the game stuck.

use std::ffi::{c_char, c_void};
use std::sync::Mutex;

use continuum_gb::libretro::*;
use continuum_gb::link::{Packet, ANSWER_FRAMES, PACKET_LEN};

type SendFn = unsafe extern "C" fn(i32, *const c_void, usize, u16);
type StartFn = unsafe extern "C" fn(u16, Option<SendFn>, Option<unsafe extern "C" fn()>);
type ReceiveFn = unsafe extern "C" fn(*const c_void, usize, u16);
type PlainFn = unsafe extern "C" fn();

/// libretro.h `struct retro_netpacket_callback`, as a frontend sees it.
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
    /// False to play an app that cannot join two phones up at all.
    offers_link: bool,
    asked: u32,
    iface: Option<Netpacket>,
    /// Everything the core has sent to the other phone, and the flags it used.
    sent: Vec<(i32, u16, Vec<u8>)>,
    protocol: String,
}
static APP: Mutex<App> = Mutex::new(App {
    offers_link: true,
    asked: 0,
    iface: None,
    sent: Vec::new(),
    protocol: String::new(),
});

/// The core is one global, so the tests take turns.
static TURN: Mutex<()> = Mutex::new(());

/// The app's own state. Poison-tolerant: one test failing must not turn the rest into aborts
/// inside a callback the core cannot unwind out of.
fn app() -> std::sync::MutexGuard<'static, App> {
    APP.lock().unwrap_or_else(|e| e.into_inner())
}

unsafe extern "C" fn send(flags: i32, buf: *const c_void, len: usize, client: u16) {
    let bytes = std::slice::from_raw_parts(buf as *const u8, len).to_vec();
    app().sent.push((flags, client, bytes));
}

unsafe extern "C" fn env(cmd: u32, d: *mut c_void) -> bool {
    match cmd {
        // SET_PIXEL_FORMAT.
        10 => true,
        c if c == ENV_SET_NETPACKET_INTERFACE => {
            let iface = *(d as *const Netpacket);
            let mut a = app();
            a.asked += 1;
            if !a.offers_link {
                return false;
            }
            assert!(iface.start.is_some() && iface.receive.is_some(), "a core must offer both");
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

/// A cartridge that sends one byte down the cable and writes what came back to work RAM.
fn sender_rom(byte: u8) -> Vec<u8> {
    let code: [u8; 21] = [
        0x3E, byte, // LD A, byte
        0xE0, 0x01, // LDH (SB), A
        0x3E, 0x81, // LD A, $81   start, this console drives the clock
        0xE0, 0x02, // LDH (SC), A
        0xF0, 0x02, // LDH A, (SC)      <- $0108
        0xCB, 0x7F, // BIT 7, A
        0x20, 0xFA, // JR NZ, -6        still going
        0xF0, 0x01, // LDH A, (SB)      what came back
        0xEA, 0x00, 0xC0, // LD ($C000), A
        0x18, 0xFE, // JR -2            and stop here
    ];
    let mut rom = vec![0u8; 0x8000];
    rom[0x100..0x100 + code.len()].copy_from_slice(&code);
    rom[0x147] = 0x00; // no mapper
    rom[0x149] = 0x00; // no save RAM
    rom
}

/// What the game wrote to $C000, which is nothing until its transfer has finished.
unsafe fn wram_result() -> u8 {
    let p = retro_get_memory_data(2) as *const u8;
    assert!(!p.is_null());
    *p
}

unsafe fn load(rom: &[u8]) {
    retro_set_environment(env);
    retro_set_video_refresh(video);
    retro_set_audio_sample_batch(audio);
    retro_set_input_poll(poll_input);
    retro_set_input_state(input);
    retro_init();
    let info = RetroGameInfo {
        path: std::ptr::null(),
        data: rom.as_ptr() as *const c_void,
        size: rom.len(),
        meta: std::ptr::null(),
    };
    assert!(retro_load_game(&info));
}

/// One frame, then the app carries whatever the core put on the cable.
unsafe fn frame(iface: &Netpacket) {
    retro_run();
    if let Some(poll) = iface.poll {
        poll();
    }
}

fn reset_app(offers_link: bool) {
    let mut a = app();
    a.offers_link = offers_link;
    a.asked = 0;
    a.iface = None;
    a.sent.clear();
    a.protocol.clear();
}

/// Everything the core has sent, as packets.
fn sent_packets() -> Vec<Packet> {
    app()
        .sent
        .iter()
        .map(|(flags, client, bytes)| {
            assert_eq!(bytes.len(), PACKET_LEN, "a cable block is {PACKET_LEN} bytes");
            assert_eq!(*flags & 1, 1, "a cable's bytes have to arrive, and in order");
            assert_eq!(*client, 0xFFFF, "to the other end of the cable");
            Packet::decode(bytes).expect("the core sent something that is not one of its own")
        })
        .collect()
}

/// The whole job end to end: the game sends $11, that byte really leaves the phone, the other
/// phone's $22 comes back, and the game reads it.
#[test]
fn a_byte_crosses_to_the_other_phone_and_its_answer_comes_back() {
    let _turn = TURN.lock().unwrap_or_else(|e| e.into_inner());
    reset_app(true);
    unsafe {
        load(&sender_rom(0x11));
        let iface = app().iface.expect("cBoy did not offer the link");
        assert_eq!(app().asked, 1, "asked for once, as libretro says");
        assert_eq!(app().protocol, "cboy-link-1", "the two phones must agree on it");
        // The app joins the two phones up.
        (iface.start.unwrap())(0, Some(send), None);
        // The game starts its byte and the cable carries it over.
        for _ in 0..3 {
            frame(&iface);
        }
        let out = sent_packets();
        assert_eq!(out.first(), Some(&Packet::Clock { seq: 0, byte: 0x11 }), "{out:?}");
        assert_eq!(wram_result(), 0x00, "the game is still waiting for the answer");
        // The other phone answers.
        let answer = Packet::Answer { seq: 0, byte: 0x22 }.encode();
        (iface.receive.unwrap())(answer.as_ptr() as *const c_void, answer.len(), 0);
        for _ in 0..3 {
            frame(&iface);
        }
        assert_eq!(wram_result(), 0x22, "the game read the other phone's byte");
        (iface.stop.unwrap())();
        retro_unload_game();
        retro_deinit();
    }
}

/// An app with no two-phone play at all, which is every app today: the core asks once, is told
/// no, and the game plays exactly as it always did, reading a loose cable's all-1s.
#[test]
fn a_phone_with_no_link_plays_on_as_before() {
    let _turn = TURN.lock().unwrap_or_else(|e| e.into_inner());
    reset_app(false);
    unsafe {
        load(&sender_rom(0x11));
        assert_eq!(app().asked, 1);
        assert!(app().iface.is_none());
        for _ in 0..3 {
            retro_run();
        }
        assert_eq!(wram_result(), 0xFF, "nothing attached, so all 1s");
        assert!(app().sent.is_empty(), "and nothing was sent anywhere");
        retro_unload_game();
        retro_deinit();
    }
}

/// The other phone drops off mid-byte. The game must not be left waiting for ever.
#[test]
fn a_partner_that_goes_quiet_does_not_freeze_the_game() {
    let _turn = TURN.lock().unwrap_or_else(|e| e.into_inner());
    reset_app(true);
    unsafe {
        load(&sender_rom(0x33));
        let iface = app().iface.unwrap();
        (iface.start.unwrap())(0, Some(send), None);
        for _ in 0..2 {
            frame(&iface);
        }
        assert_eq!(sent_packets().first(), Some(&Packet::Clock { seq: 0, byte: 0x33 }));
        assert_eq!(wram_result(), 0x00, "waiting");
        for _ in 0..ANSWER_FRAMES + 2 {
            frame(&iface);
        }
        assert_eq!(wram_result(), 0xFF, "given up on, the way a pulled-out cable reads");
        (iface.stop.unwrap())();
        retro_unload_game();
        retro_deinit();
    }
}

/// The app hands over rubbish, or a packet from a newer version of this core. It must not reach
/// the game or upset the transfer that is open.
#[test]
fn rubbish_off_the_network_cannot_reach_the_game() {
    let _turn = TURN.lock().unwrap_or_else(|e| e.into_inner());
    reset_app(true);
    unsafe {
        load(&sender_rom(0x44));
        let iface = app().iface.unwrap();
        (iface.start.unwrap())(0, Some(send), None);
        for _ in 0..2 {
            frame(&iface);
        }
        let receive = iface.receive.unwrap();
        let junk: [&[u8]; 5] = [&[], b"A", &[b'A', 0], &[b'Z', 0, 0x55], &[b'A', 0, 0x55, 9]];
        for bytes in junk {
            receive(bytes.as_ptr() as *const c_void, bytes.len(), 0);
        }
        receive(std::ptr::null(), 3, 0);
        for _ in 0..2 {
            frame(&iface);
        }
        assert_eq!(wram_result(), 0x00, "none of that finished the game's transfer");
        // And the real answer still works afterwards.
        let answer = Packet::Answer { seq: 0, byte: 0x66 }.encode();
        receive(answer.as_ptr() as *const c_void, answer.len(), 0);
        for _ in 0..2 {
            frame(&iface);
        }
        assert_eq!(wram_result(), 0x66);
        (iface.stop.unwrap())();
        retro_unload_game();
        retro_deinit();
    }
}
