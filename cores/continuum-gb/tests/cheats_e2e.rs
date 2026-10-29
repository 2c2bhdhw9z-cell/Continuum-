//! Cheat codes through cBoy's libretro entry points, the way the app sends them: a small Game Boy
//! game and a small Advance game each copy a byte of their own cartridge and a byte of their RAM
//! into RAM every turn of their loop, so what a code changes shows in the RAM the app can read.

use std::ffi::{c_void, CString};
use std::sync::Mutex;

use continuum_gb::gba::cheats::tea_encrypt;
use continuum_gb::libretro::*;

/// The core is one global: tests take turns.
static TURN: Mutex<()> = Mutex::new(());

unsafe extern "C" fn env(cmd: u32, _d: *mut c_void) -> bool {
    cmd == 10
}
unsafe extern "C" fn video(_d: *const c_void, _w: u32, _h: u32, _p: usize) {}
unsafe extern "C" fn audio(_d: *const i16, n: usize) -> usize {
    n
}
unsafe extern "C" fn poll() {}
unsafe extern "C" fn input(_p: u32, _d: u32, _i: u32, _id: u32) -> i16 {
    0
}

/// A Game Boy game: $11 into $C001 once, then for ever: the ROM's byte at $0200 into $C000 and
/// $C001 into $C003.
fn game_boy_game() -> Vec<u8> {
    let mut rom = vec![0u8; 0x8000];
    rom[0x100..0x104].copy_from_slice(&[0x00, 0xC3, 0x50, 0x01]); // NOP / JP $0150
    rom[0x134..0x13A].copy_from_slice(b"CHEATS");
    let code: &[u8] = &[
        0x3E, 0x11, 0xEA, 0x01, 0xC0, // LD A,$11 / LD ($C001),A
        0xFA, 0x00, 0x02, 0xEA, 0x00, 0xC0, // loop: LD A,($0200) / LD ($C000),A
        0xFA, 0x01, 0xC0, 0xEA, 0x03, 0xC0, // LD A,($C001) / LD ($C003),A
        0x18, 0xF2, // JR loop
    ];
    rom[0x150..0x150 + code.len()].copy_from_slice(code);
    rom[0x200] = 0x11;
    rom[0x14D] = rom[0x134..=0x14C].iter().fold(0u8, |a, b| a.wrapping_sub(*b).wrapping_sub(1));
    rom
}

/// An Advance game: for ever, the ROM's halfword at $08000200 ($1111) into $02000000 and the
/// halfword at $02000004 into $02000008.
fn advance_game() -> Vec<u8> {
    let mut rom = vec![0u8; 0x400];
    let words: [u32; 10] = [
        0xE59F_0018, // LDR r0,=$08000200
        0xE59F_1018, // LDR r1,=$02000000
        0xE1D0_20B0, // loop: LDRH r2,[r0]
        0xE1C1_20B0, // STRH r2,[r1]
        0xE1D1_30B4, // LDRH r3,[r1,#4]
        0xE1C1_30B8, // STRH r3,[r1,#8]
        0xEAFF_FFFA, // B loop
        0,
        0x0800_0200,
        0x0200_0000,
    ];
    for (i, w) in words.iter().enumerate() {
        rom[i * 4..i * 4 + 4].copy_from_slice(&w.to_le_bytes());
    }
    rom[0xA0..0xA6].copy_from_slice(b"CHEATS");
    rom[0xB2] = 0x96;
    rom[0xBD] = 0u8.wrapping_sub(rom[0xA0..=0xBC].iter().fold(0u8, |a, b| a.wrapping_add(*b))).wrapping_sub(0x19);
    rom[0x200..0x202].copy_from_slice(&0x1111u16.to_le_bytes());
    rom
}

unsafe fn open(rom: &[u8]) {
    retro_set_environment(env);
    retro_set_video_refresh(video);
    retro_set_audio_sample_batch(audio);
    retro_set_input_poll(poll);
    retro_set_input_state(input);
    retro_init();
    let info = RetroGameInfo { path: std::ptr::null(), data: rom.as_ptr() as *const c_void, size: rom.len(), meta: std::ptr::null() };
    assert!(retro_load_game(&info));
}

unsafe fn frames(n: usize) {
    for _ in 0..n {
        retro_run();
    }
}

unsafe fn set(index: u32, code: &str) {
    let c = CString::new(code).unwrap();
    retro_cheat_set(index, true, c.as_ptr());
}

unsafe fn ram() -> *const u8 {
    retro_get_memory_data(2) as *const u8
}

#[test]
fn game_boy_codes_reach_the_game() {
    let _t = TURN.lock().unwrap_or_else(|e| e.into_inner());
    let rom = game_boy_game();
    unsafe {
        open(&rom);
        frames(3);
        let wram = ram();
        assert_eq!((*wram, *wram.add(3)), (0x11, 0x11), "no codes: the game's own bytes");

        // Game Genie for $0200: byte 77, address digits 2, 0, 0, and F (F xor F = 0 on top).
        set(0, "772-00F");
        frames(1);
        assert_eq!(*wram, 0x77, "the game reads the code's byte from its cartridge");
        // With a compare byte: $11 (digits A and E) matches, $12 (A and 2) doesn't.
        set(0, "772-00F-A0E");
        frames(1);
        assert_eq!(*wram, 0x77);
        set(0, "772-00F-A02");
        frames(1);
        assert_eq!(*wram, 0x11, "the ROM's byte isn't the compare byte, so nothing changes");
        retro_cheat_set(0, false, std::ptr::null());

        // GameShark 01 5A 01 C0: $5A at $C001 in every blank, and the game copies it on.
        set(1, "015A01C0");
        frames(2);
        assert_eq!((*wram.add(1), *wram.add(3)), (0x5A, 0x5A));
        // A raw code and a CodeBreaker one joined in one entry.
        set(1, "C001:5B+00C002-5C");
        frames(2);
        assert_eq!((*wram.add(1), *wram.add(2), *wram.add(3)), (0x5B, 0x5C, 0x5B));

        // Still on after a state and a restart.
        let size = retro_serialize_size();
        let mut state = vec![0u8; size];
        assert!(retro_serialize(state.as_mut_ptr() as *mut c_void, size));
        assert!(retro_unserialize(state.as_ptr() as *const c_void, size));
        retro_reset();
        frames(3);
        let wram = ram();
        assert_eq!(*wram.add(3), 0x5B, "still on after a state and a restart");

        retro_cheat_reset();
        set(0, "772-00F");
        frames(1);
        retro_cheat_reset();
        frames(1);
        assert_eq!(*wram, 0x11, "all off: the game's own byte again");
        retro_unload_game();
        retro_deinit();
    }
}

#[test]
fn advance_codes_reach_the_game() {
    let _t = TURN.lock().unwrap_or_else(|e| e.into_inner());
    let rom = advance_game();
    let gs = [0x09F4_FBBD, 0x9681_884A, 0x3520_27E9, 0xF3DE_E5A7];
    let ar = [0x7AA9_648F, 0x7FAE_6994, 0xC0EF_AAD5, 0x4271_2C57];
    let pair = |(a, b): (u32, u32)| format!("{a:08X} {b:08X}");
    unsafe {
        open(&rom);
        frames(2);
        let ewram = ram();
        let half = |at: usize| u16::from_le_bytes([*ewram.add(at), *ewram.add(at + 1)]);
        assert_eq!((half(0), half(8)), (0x1111, 0), "no codes: the game's own values");

        // A raw ROM patch: the game reads $2222 from its cartridge instead.
        set(0, "08000200:2222");
        frames(1);
        assert_eq!(half(0), 0x2222);
        retro_cheat_reset();

        // A plain GameShark list (the game's ID line first, as lists have it): $3333 written at
        // $02000004 at every blank, which the game copies to $02000008.
        set(0, "00000000 001DC0DE+12000004 00003333");
        frames(2);
        assert_eq!((half(4), half(8)), (0x3333, 0x3333));
        retro_cheat_reset();

        // The same code encrypted, as GameShark lists are printed.
        let id = tea_encrypt(0, 0x001D_C0DE, &gs);
        let write = tea_encrypt(0x1200_0004, 0x4444, &gs);
        set(0, &format!("{}+{}", pair(id), pair(write)));
        frames(2);
        assert_eq!(half(8), 0x4444, "an encrypted GameShark code");
        retro_cheat_reset();

        // Action Replay, encrypted: a halfword write at $02000004 (its address spelt $0200004).
        let id = tea_encrypt(0, 0x001D_C0DE, &ar);
        let write = tea_encrypt(0x0220_0004, 0x5555, &ar);
        set(0, &pair(id));
        set(1, &pair(write));
        frames(2);
        assert_eq!(half(8), 0x5555, "an encrypted Action Replay code, its lines as two cheats");
        retro_cheat_reset();

        // CodeBreaker: 8 is a halfword write.
        set(0, "82000004 6666");
        frames(2);
        assert_eq!(half(8), 0x6666, "a CodeBreaker code");

        // Still on after a state and a restart.
        let size = retro_serialize_size();
        let mut state = vec![0u8; size];
        assert!(retro_serialize(state.as_mut_ptr() as *mut c_void, size));
        assert!(retro_unserialize(state.as_ptr() as *const c_void, size));
        retro_reset();
        frames(2);
        let ewram = ram();
        assert_eq!(u16::from_le_bytes([*ewram.add(8), *ewram.add(9)]), 0x6666, "still on after a state and a restart");
        retro_unload_game();
        retro_deinit();
    }
}
