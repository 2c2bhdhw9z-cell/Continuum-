//! Writes the TIA's level at every one of its sound steps (one byte each), at MAME's rate.
use continuum_atari::cart::Cart;
use continuum_atari::machine::{Atari7800, Region};
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let rom = std::fs::read(&a[1]).unwrap();
    let mut h = vec![0u8; 128];
    h[1..10].copy_from_slice(b"ATARI7800");
    h[49..53].copy_from_slice(&(rom.len() as u32).to_be_bytes());
    h.extend(rom);
    let bios = std::fs::read(&a[2]).unwrap();
    let mut m = Atari7800::new(Cart::load(&h).unwrap(), Region::Ntsc, Some(bios));
    m.sys.set_tia_period(7_159_090, 31_399, 197);
    m.sys.tia_log = Some(Vec::new());
    let frames: u32 = a[3].parse().unwrap();
    for _ in 0..frames {
        m.run_frame();
    }
    std::fs::write(&a[4], m.sys.tia_log.take().unwrap()).unwrap();
}
