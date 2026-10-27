//! The RIOT's timer against Stella: test-programs/timer.asm runs the RIOT checks of
//! timer_body.inc every frame and shows its 14 readings as bands of colour; the same instructions
//! on a 2600 (test-programs/timer2600.asm, the same 6532 chip at the same addresses) read
//! tests/references/timer-stella.txt in Stella, whose 6532 is measured against real consoles
//! (made by tests/stella/stella_riot.py). Every reading must be Stella's, on every frame.

use continuum_atari::cart::Cart;
use continuum_atari::machine::{Atari7800, Region};

#[test]
fn the_riot_timer_reads_what_stella_reads() {
    let dir = env!("CARGO_MANIFEST_DIR");
    let want: Vec<u8> = std::fs::read_to_string(format!("{dir}/tests/references/timer-stella.txt"))
        .unwrap()
        .lines()
        .find_map(|l| l.strip_prefix("values "))
        .unwrap()
        .split_whitespace()
        .map(|h| u8::from_str_radix(h, 16).unwrap())
        .collect();
    assert_eq!(want.len(), 14);
    let rom = std::fs::read(format!("{dir}/test-programs/timer.bin")).unwrap();
    let mut h = vec![0u8; 128];
    h[1..10].copy_from_slice(b"ATARI7800");
    h[49..53].copy_from_slice(&(rom.len() as u32).to_be_bytes());
    h.extend(rom);
    let bios = std::fs::read(format!("{dir}/test-programs/bios.bin")).unwrap();
    let mut m = Atari7800::new(Cart::load(&h).unwrap(), Region::Ntsc, Some(bios));
    for frame in 0..40 {
        m.run_frame();
        if frame < 2 {
            continue;
        }
        // Band 0 is lines 17-31 (picture rows 0-4 here), band k the 15 lines from line 32 + 15(k-1).
        let pic = m.picture(27, 224);
        let got: Vec<u8> = (0..14).map(|k| pic[if k == 0 { 2 } else { 5 + 15 * (k - 1) + 7 } * 320]).collect();
        assert_eq!(got, want, "frame {frame}: the RIOT's readings");
    }
}
