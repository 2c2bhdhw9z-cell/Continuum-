//! Public test ROMs, run headless. Fetch them first with scripts/fetch-test-roms.sh; a missing
//! ROM is reported and the test is skipped (so a fresh checkout without them still builds).

use std::path::PathBuf;

use continuum_gb::gb::GameBoy;

fn rom(name: &str) -> Option<Vec<u8>> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test-roms").join(name);
    match std::fs::read(&p) {
        Ok(b) => Some(b),
        Err(_) => {
            eprintln!("SKIPPED: {} not found (run scripts/fetch-test-roms.sh)", p.display());
            None
        }
    }
}

/// Runs a Blargg ROM until it prints Passed or Failed on the serial port.
fn blargg(name: &str, max_frames: u32) {
    let Some(data) = rom(name) else { return };
    let mut gb = GameBoy::new(data).unwrap();
    for _ in 0..max_frames {
        gb.run_frame();
        let out = String::from_utf8_lossy(&gb.bus.serial_out).to_string();
        if out.contains("Passed") {
            return;
        }
        if out.contains("Failed") {
            panic!("{name} failed:\n{out}");
        }
    }
    panic!("{name} timed out:\n{}", String::from_utf8_lossy(&gb.bus.serial_out));
}

#[test]
fn blargg_cpu_instrs() {
    blargg("cpu_instrs.gb", 4000);
}

#[test]
fn blargg_instr_timing() {
    blargg("instr_timing.gb", 600);
}

#[test]
fn blargg_mem_timing() {
    blargg("mem_timing.gb", 600);
}

/// Runs a Mooneye ROM until `LD B,B`; a pass leaves the Fibonacci numbers in the registers.
pub fn mooneye(name: &str) -> Result<(), String> {
    let Some(data) = rom(name) else { return Ok(()) };
    let mut gb = GameBoy::new(data).map_err(|e| e)?;
    for _ in 0..(60 * 30) {
        gb.run_frame();
        if gb.cpu.ld_b_b {
            let c = &gb.cpu;
            let regs = [c.b, c.c, c.d, c.e, c.h, c.l];
            return if regs == [3, 5, 8, 13, 21, 34] {
                Ok(())
            } else {
                Err(format!("{name}: registers {regs:?}"))
            };
        }
        if gb.cpu.locked {
            return Err(format!("{name}: CPU locked at an illegal opcode"));
        }
    }
    Err(format!("{name}: timed out"))
}

#[test]
fn dmg_acid2() {
    let Some(data) = rom("dmg-acid2.gb") else { return };
    let Some(png_bytes) = rom("dmg-acid2-reference.png") else { return };
    let mut gb = GameBoy::new(data).unwrap();
    for _ in 0..120 {
        gb.run_frame();
    }
    let mut decoder = png::Decoder::new(std::io::Cursor::new(png_bytes));
    decoder.set_transformations(png::Transformations::EXPAND);
    let mut reader = decoder.read_info().unwrap();
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).unwrap();
    let bpp = info.line_size / info.width as usize;
    let mut wrong = 0;
    let mut first = None;
    for y in 0..144usize {
        for x in 0..160usize {
            let p = &buf[y * info.line_size + x * bpp..];
            let shade = match p[0] {
                0xFF => 0u8,
                0xAA => 1,
                0x55 => 2,
                _ => 3,
            };
            if gb.bus.ppu.frame[y * 160 + x] != shade {
                wrong += 1;
                first.get_or_insert((x, y));
            }
        }
    }
    assert_eq!(wrong, 0, "dmg-acid2: {wrong} pixels differ, first at {first:?}");
}

/// Writes the screen after N frames to $DUMP_PNG (a debugging aid, not a check).
#[test]
fn dump_screen() {
    let (Ok(path), Ok(name)) = (std::env::var("DUMP_PNG"), std::env::var("DUMP_ROM")) else { return };
    let Some(data) = rom(&name) else { return };
    let mut gb = GameBoy::new(data).unwrap();
    for _ in 0..120 {
        gb.run_frame();
    }
    let f = std::fs::File::create(path).unwrap();
    let mut enc = png::Encoder::new(f, 160, 144);
    enc.set_color(png::ColorType::Grayscale);
    let mut w = enc.write_header().unwrap();
    let px: Vec<u8> = gb.bus.ppu.frame.iter().map(|s| [0xFF, 0xAA, 0x55, 0][*s as usize]).collect();
    w.write_image_data(&px).unwrap();
}
