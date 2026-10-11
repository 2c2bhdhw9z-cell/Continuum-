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

#[test]
fn jsmolka_save_sram() {
    jsmolka("save/sram.gba");
}

#[test]
fn jsmolka_save_flash64() {
    jsmolka("save/flash64.gba");
}

#[test]
fn jsmolka_save_flash128() {
    jsmolka("save/flash128.gba");
}

#[test]
fn jsmolka_save_none() {
    jsmolka("save/none.gba");
}

#[test]
fn jsmolka_unsafe() {
    jsmolka("unsafe/unsafe.gba");
}

fn frame_hash(name: &str) -> Option<u64> {
    let data = rom(name)?;
    let mut gba = Gba::new(data);
    for _ in 0..30 {
        gba.run_frame();
    }
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for p in &gba.bus.ppu.frame {
        h = (h ^ u64::from(*p)).wrapping_mul(0x100_0000_01b3);
    }
    Some(h)
}

/// The three jsmolka PPU ROMs draw a picture with no pass flag. Their frames were checked by eye
/// (11 Oct 2026: "Hello world!", a 16-step blue ramp, vertical stripes) and are pinned here.
#[test]
fn jsmolka_ppu_frames() {
    for (name, want) in [("ppu/hello.gba", 0x1ceaf0034f053882u64), ("ppu/shades.gba", 0x4b59a3af7897d125), ("ppu/stripes.gba", 0x25bd2db3043c2a25)] {
        if let Some(h) = frame_hash(name) {
            eprintln!("FRAME {name} {h:#018x}");
            if want != 0 {
                assert_eq!(h, want, "{name} frame changed");
            }
        }
    }
}

#[test]
fn gba_save_state_round_trip() {
    let Some(data) = rom("arm/arm.gba") else { return };
    let mut a = Gba::new(data.clone());
    for _ in 0..3 {
        a.run_frame();
    }
    let state = a.save_state();
    let mut b = Gba::new(data);
    b.load_state(&state).unwrap();
    for _ in 0..20 {
        a.run_frame();
        b.run_frame();
    }
    assert_eq!(a.cpu.pc, b.cpu.pc);
    assert_eq!(a.bus.cycles, b.bus.cycles);
    assert_eq!(a.bus.ppu.frame, b.bus.ppu.frame);
}

#[test]
fn eeprom_write_then_read() {
    use continuum_gb::gba::save::{Save, SaveKind};
    let mut s = Save::new(SaveKind::Eeprom);
    s.eeprom_dma(81); // 14-bit addresses
    let addr = 5u32;
    let bits = |s: &mut Save, v: u64, n: u32| for i in (0..n).rev() { s.eeprom_write(((v >> i) & 1) as u16) };
    bits(&mut s, 0b10, 2);
    bits(&mut s, u64::from(addr), 14);
    bits(&mut s, 0x0123_4567_89AB_CDEF, 64);
    bits(&mut s, 0, 1);
    assert_eq!(&s.data[40..48], &[0x01, 0x23, 0x45, 0x67, 0x89, 0xAB, 0xCD, 0xEF]);
    bits(&mut s, 0b11, 2);
    bits(&mut s, u64::from(addr), 14);
    bits(&mut s, 0, 1);
    let mut v = 0u64;
    for i in 0..68 {
        let b = s.eeprom_read();
        if i >= 4 {
            v = v << 1 | u64::from(b);
        }
    }
    assert_eq!(v, 0x0123_4567_89AB_CDEF);
}
