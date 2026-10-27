//! Prints the 16 band values a test program shows (one colour value per zone), frame by frame.
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
    let frames: u32 = a[3].parse().unwrap();
    for f in 0..frames {
        m.run_frame();
        let pic = m.picture(27, 224);
        let row = |r: usize| pic[r * 320];
        let vals: Vec<String> = (0..14).map(|z| format!("{:02x}", row(z * 15 + 12))).collect();
        println!("frame {f}: {}", vals.join(" "));
    }
}
