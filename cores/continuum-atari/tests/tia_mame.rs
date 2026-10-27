//! The TIA's sound against MAME 0.289's, sample for sample: test-programs/sound.asm plays every
//! AUDC mode on both channels with many frequencies and volumes (one volume changed part-way
//! through a line), and every level the TIA puts out, at MAME's 31,399 Hz, must be MAME's
//! (tests/references/sound.txt keeps FNV-1a hashes of each 4,096 of MAME's samples, made by
//! tests/mame/make_refs.py). MAME_SOUND=<dir with levels.bin> names the first sample that differs.

use continuum_atari::cart::Cart;
use continuum_atari::machine::{Atari7800, Region};

fn fnv(data: &[u8]) -> u64 {
    let mut h = 0xCBF2_9CE4_8422_2325u64;
    for &b in data {
        h = (h ^ b as u64).wrapping_mul(0x0100_0000_01B3);
    }
    h
}

#[test]
fn the_tia_sounds_as_on_mame_sample_for_sample() {
    let dir = env!("CARGO_MANIFEST_DIR");
    let text = std::fs::read_to_string(format!("{dir}/tests/references/sound.txt")).unwrap();
    let field = |k: &str| text.lines().find_map(|l| l.strip_prefix(k)).unwrap().trim().to_string();
    let frames: u32 = field("frames ").parse().unwrap();
    let count: usize = field("samples ").parse().unwrap();
    let blocks: Vec<u64> = text
        .lines()
        .filter_map(|l| l.strip_prefix("block "))
        .map(|l| u64::from_str_radix(l.split_whitespace().nth(1).unwrap(), 16).unwrap())
        .collect();
    let rom = std::fs::read(format!("{dir}/test-programs/{}.bin", field("program "))).unwrap();
    let mut h = vec![0u8; 128];
    h[1..10].copy_from_slice(b"ATARI7800");
    h[49..53].copy_from_slice(&(rom.len() as u32).to_be_bytes());
    h.extend(rom);
    let bios = std::fs::read(format!("{dir}/test-programs/bios.bin")).unwrap();
    let mut m = Atari7800::new(Cart::load(&h).unwrap(), Region::Ntsc, Some(bios));
    // MAME clocks the TIA at a whole number of hertz, 31,399 (the board's own is 31,399.5), and
    // it takes a write about 30 of MARIA's clocks after the processor makes it, its processor
    // also drifting against MARIA by up to 13 over the test (MAME rounds both clocks to whole
    // hertz). Every sample matches with our first sound step 190 to 205 clocks in, against
    // MAME's second sample: 197.
    m.sys.set_tia_period(7_159_090, 31_399, 197);
    m.sys.tia_log = Some(Vec::new());
    for _ in 0..frames {
        m.run_frame();
    }
    let ours = m.sys.tia_log.take().unwrap();
    assert!(ours.len() >= count, "{} samples, MAME's reference has {count}", ours.len());
    for (k, &want) in blocks.iter().enumerate() {
        let got = &ours[k * 4096..(k + 1) * 4096];
        if fnv(got) != want {
            let mut why = format!("block {k} (samples {} to {}) differs from MAME's", k * 4096, (k + 1) * 4096);
            if let Ok(d) = std::env::var("MAME_SOUND") {
                let mame = std::fs::read(format!("{d}/levels.bin")).unwrap();
                if let Some(i) = (k * 4096..(k + 1) * 4096).find(|&i| ours[i] != mame[i]) {
                    why += &format!(": first at sample {i}, ours {}, MAME {}", ours[i], mame[i]);
                }
            }
            panic!("{why}");
        }
    }
    eprintln!("all {count} samples match MAME's");
}
