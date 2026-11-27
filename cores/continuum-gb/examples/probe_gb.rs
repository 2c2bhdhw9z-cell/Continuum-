//! Temporary probe for P5 Item 3: run a GB ROM, dump its frame as PNG, print the frame hash.
//! Usage: probe_gb <rom-under-test-roms> <dmg|cgb|ncm> <frames> <out.png>
//!   PROBE_LY="ly:bits,ly:bits..." presses each button when the PPU reaches that LY.
//!   PROBE_KEYS="frame:bits,frame:bits..." presses each button at that frame (5-frame hold).
//! Deleted after the probe work; not part of the crate.
use continuum_gb::gb::{GameBoy, Model};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (rom, mode, frames, out) = (&args[1], args[2].as_str(), args[3].parse::<u32>().unwrap(), &args[4]);
    let data = std::fs::read(format!("test-roms/{rom}")).unwrap();
    let mut gb = match mode {
        "dmg" => GameBoy::new_mode(data, false).unwrap(),
        "cgb" => GameBoy::new_mode(data, true).unwrap(),
        "ncm" => GameBoy::with_model(data, Model::Cgb, None).unwrap(),
        _ => panic!("mode"),
    };
    // LY-timed presses (tellinglys): step until the PPU is on the wanted line.
    if let Ok(spec) = std::env::var("PROBE_LY") {
        for _ in 0..30 {
            gb.run_frame();
        }
        for part in spec.split(',') {
            let (ly, bit) = part.split_once(':').unwrap();
            let (ly, bit): (u8, u8) = (ly.parse().unwrap(), bit.parse().unwrap());
            for _ in 0..200_000 {
                if gb.bus.ppu.ly == ly {
                    break;
                }
                gb.cpu.step(&mut gb.bus);
            }
            eprintln!("press bit {bit} at ly={} (want {ly}) ie={:#04x} if={:#04x}", gb.bus.ppu.ly, gb.bus.ie, gb.bus.iflag);
            gb.bus.set_buttons(bit);
            eprintln!("  after: if={:#04x} ime={} halted={}", gb.bus.iflag, gb.cpu.ime, gb.cpu.halted);
            for _ in 0..20000 {
                gb.cpu.step(&mut gb.bus);
            }
            gb.bus.set_buttons(0);
            for _ in 0..20000 {
                gb.cpu.step(&mut gb.bus);
            }
        }
    }
    // Frame-timed presses (rtc3test menu): (frame, bits) pairs, 5-frame hold each.
    let keys: Vec<(u32, u8)> = std::env::var("PROBE_KEYS").map(|s| {
        s.split(',').map(|p| {
            let (f, b) = p.split_once(':').unwrap();
            (f.parse().unwrap(), b.parse().unwrap())
        }).collect()
    }).unwrap_or_default();
    for f in 0..frames {
        for (kf, bits) in &keys {
            if *kf == f {
                gb.bus.set_buttons(*bits);
            }
            if *kf + 5 == f {
                gb.bus.set_buttons(0);
            }
        }
        gb.run_frame();
    }
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for p in &gb.bus.ppu.frame {
        h = (h ^ u64::from(*p)).wrapping_mul(0x100_0000_01b3);
    }
    println!("hash {h:#018x} ld_b_b={} pc={:04X}", gb.cpu.ld_b_b, gb.cpu.pc);
    if std::env::var("PROBE_WRAM").is_ok() {
        std::fs::write("/tmp/wram.bin", &gb.bus.wram).unwrap();
        std::fs::write("/tmp/hram.bin", &gb.bus.hram).unwrap();
    }
    println!("regs b={} c={} d={} e={} h={} l={}", gb.cpu.b, gb.cpu.c, gb.cpu.d, gb.cpu.e, gb.cpu.h, gb.cpu.l);
    let f = std::fs::File::create(out).unwrap();
    let mut enc = png::Encoder::new(f, 160, 144);
    enc.set_color(png::ColorType::Rgb);
    let mut w = enc.write_header().unwrap();
    let px: Vec<u8> = gb.bus.ppu.frame.iter().flat_map(|p| [(p >> 16) as u8, (p >> 8) as u8, *p as u8]).collect();
    w.write_image_data(&px).unwrap();
}
