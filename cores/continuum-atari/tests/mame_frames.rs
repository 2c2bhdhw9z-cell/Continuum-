//! cAtari against MAME 0.289's a7800, picture by picture: each of our own test programs
//! (test-programs/*.asm) runs here with the same stand-in BIOS MAME ran, and every frame's MARIA
//! colour values must hash to what MAME drew (tests/references/<case>.txt, made by
//! tests/mame/make_refs.py). MAME_IDX=<dir> (the script's KEEP=1 output) names the first dot that
//! differs.

use continuum_atari::cart::Cart;
use continuum_atari::machine::{Atari7800, Input, Region};
use std::path::PathBuf;

fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn fnv(data: &[u8]) -> u64 {
    let mut h = 0xCBF2_9CE4_8422_2325u64;
    for &b in data {
        h = (h ^ b as u64).wrapping_mul(0x0100_0000_01B3);
    }
    h
}

fn a78(rom: &[u8]) -> Vec<u8> {
    let mut h = vec![0u8; 128];
    h[0] = 3;
    h[1..10].copy_from_slice(b"ATARI7800");
    h[17..28].copy_from_slice(b"cAtari test");
    h[49..53].copy_from_slice(&(rom.len() as u32).to_be_bytes());
    h[55] = 1;
    h[56] = 1;
    h[100..128].copy_from_slice(b"ACTUAL CART DATA STARTS HERE");
    h.extend_from_slice(rom);
    h
}

/// One of MAME's input changes, on our controls.
fn apply(i: &mut Input, tag: &str, mask: u32, value: u32) {
    let on = value != 0;
    match tag {
        ":JOYSTICKS" => {
            let (p, bit) = if mask >= 0x10 { (0, mask >> 4) } else { (1, mask) };
            let d = match bit {
                1 => 0,
                2 => 1,
                4 => 2,
                _ => 3,
            };
            i.dirs[p][d] = on;
        }
        ":BUTTONS" => match mask {
            0x08 => i.buttons[0][0] = on,
            0x02 => i.buttons[0][1] = on,
            0x04 => i.buttons[1][0] = on,
            _ => i.buttons[1][1] = on,
        },
        ":CONSOLE" => match mask {
            0x01 => i.reset = on,
            0x02 => i.select = on,
            0x08 => i.pause = on,
            0x40 => i.difficulty_a[0] = !on,
            _ => i.difficulty_a[1] = !on,
        },
        _ => panic!("{tag}"),
    }
}

fn check(case: &str) {
    let text = std::fs::read_to_string(crate_dir().join(format!("tests/references/{case}.txt"))).unwrap();
    let mut program = String::new();
    let mut want = Vec::new();
    let mut events: Vec<(usize, String, u32, u32)> = Vec::new();
    for line in text.lines() {
        let w: Vec<&str> = line.split_whitespace().collect();
        match w.first() {
            Some(&"program") => program = w[1].to_string(),
            Some(&"frame") => want.push(u64::from_str_radix(w[3], 16).unwrap()),
            Some(&"event") => events.push((
                w[1].parse().unwrap(),
                w[2].to_string(),
                w[3].parse().unwrap(),
                w[4].parse().unwrap(),
            )),
            _ => {}
        }
    }
    let rom = std::fs::read(crate_dir().join(format!("test-programs/{program}.bin"))).unwrap();
    let bios = std::fs::read(crate_dir().join("test-programs/bios.bin")).unwrap();
    let cart = Cart::load(&a78(&rom)).unwrap();
    let mut m = Atari7800::new(cart, Region::Ntsc, Some(bios));
    let (first, end) = Region::Ntsc.visible();
    for (k, &h) in want.iter().enumerate() {
        // MAME shows a change made in its frame callback k two frames later (a difficulty switch
        // three); here it is made before frame k + 1 (k + 2) runs.
        let switch = |tag: &str, mask: u32| tag == ":CONSOLE" && mask >= 0x40;
        for (_, tag, mask, value) in events.iter().filter(|e| e.0 + if switch(&e.1, e.2) { 2 } else { 1 } == k) {
            apply(&mut m.sys.input, tag, *mask, *value);
        }
        m.run_frame();
        let pic = m.picture(first, end - first);
        let got = fnv(pic);
        if got != h {
            let mut why = format!("{case}: frame {k} differs from MAME's");
            if let Ok(dir) = std::env::var("MAME_IDX") {
                let mame = std::fs::read(PathBuf::from(dir).join(format!("mame-{case}/f{k:05}.idx"))).unwrap();
                if let Some(i) = (0..pic.len()).find(|&i| pic[i] != mame[i]) {
                    let n = (0..pic.len()).filter(|&i| pic[i] != mame[i]).count();
                    why += &format!(": {n} dots, the first at ({}, {}): ours {:02x}, MAME {:02x}", i % 320, i / 320, pic[i], mame[i]);
                }
            }
            panic!("{why}");
        }
    }
    eprintln!("{case}: all {} frames match MAME", want.len());
}

#[test]
fn zones_matches_mame() {
    check("zones");
}

#[test]
fn dma_matches_mame() {
    check("dma");
}

#[test]
fn modes_matches_mame() {
    check("modes");
}

#[test]
fn inputs_match_mame() {
    check("inputs");
}
