//! Two Game Boys on one phone, joined by a link cable.
//!
//! Both machines run the same cartridge, each with its own save, and are kept in step by real
//! time (`Bus::dots`, which double speed does not change) in short slices: the first runs a
//! slice, then the second catches up to it. The cable between them is the one two phones use
//! (`bus.rs`), with the two ends right here instead of a network apart: when the console driving
//! the clock has shifted its eight bits out and is holding the transfer open, the other console
//! is clocked with that byte and its own goes straight back, so both finish together, at most a
//! slice late. With nobody listening on the other end the driving console reads all 1s, as on
//! two phones.

use crate::bus::Bus;
use crate::gb::{GameBoy, FRAME_CYCLES};

/// How far apart the two consoles may get, in picture dots: a seventh of a line. A byte on the
/// cable takes 4096 dots at the normal speed (128 in a Color's fast mode), so the slice is what a
/// transfer can come late by.
pub const SLICE_DOTS: u64 = 64;

/// One frame of the first console, with the second kept in step. `second_frame` is called each
/// time the second console finishes a picture, before it starts drawing the next, so a picture
/// can be taken from it whole.
pub fn run_frame(first: &mut GameBoy, second: &mut GameBoy, mut second_frame: impl FnMut(&mut GameBoy)) {
    let start = first.bus.cycles;
    first.bus.ppu.frame_ready = false;
    second.bus.ppu.frame_ready = false;
    loop {
        let until = first.bus.dots + SLICE_DOTS;
        while first.bus.dots < until && !first.bus.ppu.frame_ready {
            first.cpu.step(&mut first.bus);
        }
        while second.bus.dots < first.bus.dots {
            second.cpu.step(&mut second.bus);
            if second.bus.ppu.frame_ready {
                second.bus.ppu.frame_ready = false;
                second_frame(second);
            }
        }
        exchange(&mut first.bus, &mut second.bus);
        let spent = first.bus.cycles - start;
        if first.bus.ppu.frame_ready || spent >= FRAME_CYCLES * 2 || (!first.bus.ppu.on() && spent >= FRAME_CYCLES) {
            break;
        }
    }
    // The cheat codes are the app's game's, the first console's (the second may play another).
    first.bus.apply_cheat_pokes();
}

/// The cable between the two, both ways.
pub fn exchange(a: &mut Bus, b: &mut Bus) {
    a.link_connected = true;
    b.link_connected = true;
    one_way(a, b);
    one_way(b, a);
}

fn one_way(from: &mut Bus, to: &mut Bus) {
    if !from.link_waiting() {
        return;
    }
    match from.link_out.pop_back() {
        Some(byte) => {
            // Only the newest byte is ever still open; anything older was restarted over.
            from.link_out.clear();
            let answer = to.link_clock(byte).unwrap_or(0xFF);
            from.link_answer(answer);
        }
        None => from.link_give_up(),
    }
}

/// A save state of both consoles: a tag, the first one's length, then the two states.
pub const STATE_TAG: &[u8; 8] = b"CBOYDUO1";

pub fn save_state(first: &GameBoy, second: &GameBoy) -> Vec<u8> {
    let (a, b) = (first.save_state(), second.save_state());
    let mut out = Vec::with_capacity(16 + a.len() + b.len());
    out.extend_from_slice(STATE_TAG);
    out.extend_from_slice(&(a.len() as u64).to_le_bytes());
    out.extend_from_slice(&a);
    out.extend_from_slice(&b);
    out
}

/// Loads a state of both consoles. A state of one console (taken with the second one off) goes
/// into the first and leaves the second as it is.
pub fn load_state(first: &mut GameBoy, second: &mut GameBoy, data: &[u8]) -> Result<(), String> {
    let Some(rest) = data.strip_prefix(STATE_TAG.as_slice()) else {
        return first.load_state(data);
    };
    let len = rest.get(..8).map(|b| u64::from_le_bytes(b.try_into().unwrap_or([0; 8])) as usize).ok_or("state too short")?;
    let body = &rest[8..];
    if len > body.len() {
        return Err("state too short".into());
    }
    // Checked before either is touched, so a broken state changes neither.
    let (a, b) = body.split_at(len);
    let (mut na, mut nb) = (first.clone(), second.clone());
    na.load_state(a)?;
    nb.load_state(b)?;
    *first = na;
    *second = nb;
    // Both start counting time from the same point again.
    first.bus.dots = 0;
    second.bus.dots = 0;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn machine(code: &[u8]) -> GameBoy {
        let mut rom = vec![0u8; 0x8000];
        rom[0x100..0x100 + code.len()].copy_from_slice(code);
        GameBoy::new(rom).unwrap()
    }

    /// LD A,$91 / LDH ($40),A / JR -2: the screen on, and a loop.
    const IDLE: &[u8] = &[0x3E, 0x91, 0xE0, 0x40, 0x18, 0xFE];

    #[test]
    fn the_two_stay_in_step_and_both_draw() {
        let (mut a, mut b) = (machine(IDLE), machine(IDLE));
        let mut pictures = 0;
        for _ in 0..10 {
            run_frame(&mut a, &mut b, |_| pictures += 1);
            assert!(b.bus.dots >= a.bus.dots && b.bus.dots - a.bus.dots < SLICE_DOTS, "{} {}", a.bus.dots, b.bus.dots);
        }
        assert!((9..=10).contains(&pictures), "{pictures}");
    }

    /// The point of it: a byte crosses each way, both ends get their interrupt, and the driving
    /// end waits for the other instead of reading 1s.
    #[test]
    fn a_byte_crosses_the_cable_both_ways() {
        let (mut a, mut b) = (machine(IDLE), machine(IDLE));
        run_frame(&mut a, &mut b, |_| {});
        b.bus.write(0xFF01, 0x22);
        b.bus.write(0xFF02, 0x80);
        a.bus.write(0xFF01, 0x11);
        a.bus.write(0xFF02, 0x81);
        a.bus.iflag = 0;
        b.bus.iflag = 0;
        run_frame(&mut a, &mut b, |_| {});
        assert_eq!((a.bus.sb, b.bus.sb), (0x22, 0x11));
        for g in [&a, &b] {
            assert_ne!(g.bus.iflag & crate::bus::INT_SERIAL, 0);
            assert!(!g.bus.link_busy());
        }
    }

    #[test]
    fn nobody_listening_reads_all_ones() {
        let (mut a, mut b) = (machine(IDLE), machine(IDLE));
        b.bus.write(0xFF01, 0x22);
        a.bus.write(0xFF01, 0x11);
        a.bus.write(0xFF02, 0x81);
        run_frame(&mut a, &mut b, |_| {});
        assert_eq!((a.bus.sb, b.bus.sb), (0xFF, 0x22), "the other console's own byte is left alone");
        assert!(!a.bus.link_busy());
    }

    #[test]
    fn a_state_of_both_comes_back_and_a_single_one_still_loads() {
        let (mut a, mut b) = (machine(IDLE), machine(IDLE));
        run_frame(&mut a, &mut b, |_| {});
        b.bus.wram[0] = 0x5A;
        let both = save_state(&a, &b);
        b.bus.wram[0] = 0;
        load_state(&mut a, &mut b, &both).unwrap();
        assert_eq!(b.bus.wram[0], 0x5A);
        a.bus.wram[1] = 0x77;
        let single = a.save_state();
        a.bus.wram[1] = 0;
        load_state(&mut a, &mut b, &single).unwrap();
        assert_eq!((a.bus.wram[1], b.bus.wram[0]), (0x77, 0x5A));
        assert!(load_state(&mut a, &mut b, &both[..20]).is_err());
        assert_eq!(b.bus.wram[0], 0x5A, "a broken state changes nothing");
    }
}
