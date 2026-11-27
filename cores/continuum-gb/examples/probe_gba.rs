//! Temporary probe for P5 Item 3: run a GBA ROM, print the libgba console text found in
//! VRAM (the hw-test suite prints PASS/FAIL there), the frame hash, and any SRAM text.
//! Usage: probe_gba <rom-under-test-roms> <frames>
//! Deleted after the probe work; not part of the crate.
use continuum_gb::gba::Gba;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (rom, frames) = (&args[1], args[2].parse::<u32>().unwrap());
    let data = std::fs::read(format!("test-roms/{rom}")).unwrap();
    let mut gba = Gba::new(data);
    for _ in 0..frames {
        gba.run_frame();
    }
    println!("pc={:08X} r12={} thumb={}", gba.cpu.pc, gba.cpu.r[12], gba.cpu.thumb());
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for p in &gba.bus.ppu.frame {
        h = (h ^ u64::from(*p)).wrapping_mul(0x100_0000_01b3);
    }
    println!("frame hash {h:#018x}");
    // Scan every 2 KiB screen block in VRAM for rows of printable-ASCII tile indices
    // (libgba's console writes the character code as the tile number).
    let vram = &gba.bus.vram;
    for base in (0..0x18000).step_by(0x800) {
        let mut lines = Vec::new();
        for row in 0..32 {
            let mut s = String::new();
            for col in 0..32 {
                let off = base + (row * 32 + col) * 2;
                let t = u16::from_le_bytes([vram[off], vram[off + 1]]) & 0x3FF;
                let c = if (0x20..0x7F).contains(&t) { char::from(t as u8) } else { ' ' };
                s.push(c);
            }
            let t = s.trim_end();
            if !t.is_empty() {
                lines.push(format!("{row:2}: {t}"));
            }
        }
        if !lines.is_empty() {
            println!("VRAM map block {:#06x}:", base);
            for l in lines {
                println!("  {l}");
            }
        }
    }
    // SRAM text (the mGBA suite writes results there with savprintf).
    let sram = &gba.bus.save.data;
    if !sram.is_empty() {
        let text: String = sram.iter().take_while(|&&b| b != 0).map(|&b| b as char).collect();
        if text.chars().any(|c| c.is_ascii_graphic()) {
            println!("SRAM text: {:?}", text.chars().take(400).collect::<String>());
        }
    }
}
