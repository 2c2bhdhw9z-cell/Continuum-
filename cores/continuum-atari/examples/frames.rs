//! Runs a cartridge for a number of frames and writes the visible picture of the frames asked
//! for as PNG files, through a palette file of "index rrggbb" lines (MAME's, as the comparison
//! scripts save it), or as raw MARIA colour values.
//!
//!     cargo run --release --example frames -- cart.a78 bios.bin palette.txt out_dir 30 31 32

use continuum_atari::cart::Cart;
use continuum_atari::machine::{Atari7800, Region};
use std::io::Write;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cart = Cart::load(&std::fs::read(&args[1]).unwrap()).unwrap();
    let bios = if args[2] == "-" { None } else { Some(std::fs::read(&args[2]).unwrap()) };
    let palette: Vec<u32> = std::fs::read_to_string(&args[3])
        .unwrap()
        .lines()
        .map(|l| u32::from_str_radix(l.split_whitespace().nth(1).unwrap(), 16).unwrap() & 0xFFFFFF)
        .collect();
    let out = std::path::PathBuf::from(&args[4]);
    std::fs::create_dir_all(&out).unwrap();
    let wanted: Vec<u64> = args[5..].iter().map(|s| s.parse().unwrap()).collect();
    let region = if cart.info.pal == Some(true) { Region::Pal } else { Region::Ntsc };
    let mut m = Atari7800::new(cart, region, bios);
    let (first, end) = region.visible();
    let last = *wanted.iter().max().unwrap();
    for f in 0..=last {
        m.run_frame();
        if wanted.contains(&f) {
            let pic = m.picture(first, end - first);
            let mut rgb = Vec::with_capacity(pic.len() * 3);
            for &c in pic {
                let p = palette[c as usize];
                rgb.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, p as u8]);
            }
            let file = std::fs::File::create(out.join(format!("f{f:05}.png"))).unwrap();
            let mut enc = png::Encoder::new(std::io::BufWriter::new(file), 320, end - first);
            enc.set_color(png::ColorType::Rgb);
            enc.set_depth(png::BitDepth::Eight);
            enc.write_header().unwrap().write_image_data(&rgb).unwrap();
            let mut raw = std::fs::File::create(out.join(format!("f{f:05}.raw"))).unwrap();
            raw.write_all(pic).unwrap();
        }
    }
}
