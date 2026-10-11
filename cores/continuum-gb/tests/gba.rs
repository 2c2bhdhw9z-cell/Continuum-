//! Game Boy Advance test ROMs (jsmolka/gba-tests, MIT), fetched by scripts/fetch-test-roms.sh.
//! A test ROM ends in an endless `B .` with r12 = 0 when every test passed, or the number of the
//! first failed one.

use std::path::PathBuf;

use continuum_gb::gba::Gba;

fn rom(name: &str) -> Option<Vec<u8>> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test-roms/jsmolka").join(name);
    let r = std::fs::read(&p).ok();
    if r.is_none() {
        eprintln!("SKIPPED: {} not found (run scripts/fetch-test-roms.sh)", p.display());
    }
    r
}

/// Runs until the CPU sits on a branch to itself. Returns r12.
fn run_to_idle(gba: &mut Gba, max_frames: u32) -> Option<u32> {
    for _ in 0..max_frames {
        gba.run_frame();
        let pc = gba.cpu.pc;
        let arm_idle = !gba.cpu.thumb() && gba.bus.peek32(pc) == 0xEAFF_FFFE;
        let thumb_idle = gba.cpu.thumb() && gba.bus.peek16(pc) == 0xE7FE;
        if arm_idle || thumb_idle {
            return Some(gba.cpu.r[12]);
        }
    }
    None
}

fn jsmolka(name: &str) {
    let Some(data) = rom(name) else { return };
    let mut gba = Gba::new(data);
    match run_to_idle(&mut gba, 600) {
        Some(0) => {}
        Some(n) => panic!("{name}: test {n} failed"),
        None => panic!("{name}: never finished (pc {:08X})", gba.cpu.pc),
    }
}

#[test]
fn jsmolka_arm() {
    jsmolka("arm/arm.gba");
}

#[test]
fn jsmolka_thumb() {
    jsmolka("thumb/thumb.gba");
}

#[test]
fn jsmolka_memory() {
    jsmolka("memory/memory.gba");
}

#[test]
fn jsmolka_bios() {
    jsmolka("bios/bios.gba");
}

/// Writes the screen of $GBA_ROM after 60 frames to $GBA_PNG (a debugging aid).
#[test]
fn gba_dump_screen() {
    let (Ok(name), Ok(path)) = (std::env::var("GBA_ROM"), std::env::var("GBA_PNG")) else { return };
    let Some(data) = rom(&name) else { return };
    let mut gba = Gba::new(data);
    for _ in 0..60 {
        gba.run_frame();
    }
    let f = std::fs::File::create(path).unwrap();
    let mut enc = png::Encoder::new(f, 240, 160);
    enc.set_color(png::ColorType::Rgb);
    let mut w = enc.write_header().unwrap();
    let px: Vec<u8> = gba.bus.ppu.frame.iter().flat_map(|p| [(p >> 16) as u8, (p >> 8) as u8, *p as u8]).collect();
    w.write_image_data(&px).unwrap();
    eprintln!("pc {:08X} r12 {}", gba.cpu.pc, gba.cpu.r[12]);
}
