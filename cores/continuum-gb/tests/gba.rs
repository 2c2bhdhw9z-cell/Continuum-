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

/// Which machine a file is run in. The byte that marks an Advance ROM ($96 at $B2) is ordinary
/// code on a Game Boy cartridge, and a Game Boy game started as a 240x160 Advance cannot be
/// played at all, so a file carrying a Game Boy header has to prove the Advance header too.
#[test]
fn a_game_boy_cartridge_is_not_run_as_an_advance_game() {
    // A Game Boy cartridge, with the Nintendo logo, a header checksum that adds up, and $96 at
    // $B2 where its own code happens to put it.
    let mut gb = vec![0u8; 0x8000];
    gb[0x104..0x134].copy_from_slice(&continuum_gb::cart::LOGO);
    gb[0x134..0x13A].copy_from_slice(b"TETRIS");
    gb[0x147] = 0x1B;
    gb[0x149] = 3;
    gb[0x14D] = gb[0x134..0x14D].iter().fold(0u8, |a, b| a.wrapping_sub(*b).wrapping_sub(1));
    gb[0xB2] = 0x96;
    assert!(!continuum_gb::gba::is_gba_rom(&gb), "a Game Boy cartridge must stay a Game Boy one");
    // The same cartridge with its logo replaced, as some fan-made ones have: the checksum still
    // adds up, so it is still a Game Boy cartridge.
    let mut hack = gb.clone();
    hack[0x104..0x134].fill(0x00);
    assert!(!continuum_gb::gba::is_gba_rom(&hack));
    // An Advance cartridge: $96 at $B2 and its own check byte at $BD.
    let mut gba = vec![0u8; 0x200];
    gba[0xB2] = 0x96;
    let sum = gba[0xA0..=0xBC].iter().fold(0u8, |a, b| a.wrapping_add(*b));
    gba[0xBD] = 0u8.wrapping_sub(sum).wrapping_sub(0x19);
    assert!(continuum_gb::gba::is_gba_rom(&gba));
    // Homebrew whose check byte was never put right is still an Advance game: nothing about it
    // looks like a Game Boy cartridge.
    let mut rough = gba.clone();
    rough[0xBD] = rough[0xBD].wrapping_add(1);
    assert!(continuum_gb::gba::is_gba_rom(&rough));
    // And a real one.
    if let Some(real) = rom("arm/arm.gba") {
        assert!(continuum_gb::gba::is_gba_rom(&real));
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

/// "Old fan-made game fixes": compressed data that asks the BIOS to unpack past the size it
/// declared. On, the unpacking stops at that size, the way VisualBoy Advance did and the way
/// those games were built for; off, the extra bytes are written, the way a real console does.
#[test]
fn unpacking_either_stops_at_the_declared_size_or_spills_over() {
    let Some(data) = rom("arm/arm.gba") else { return };
    let (src, dst) = (0x0200_0000u32, 0x0200_1000u32);
    let unpack = |keep_inside: bool| -> Vec<u8> {
        let mut gba = Gba::new(data.clone());
        gba.bus.vba_compat = keep_inside;
        gba.bus.write32(src, 0x10 | (4 << 8)); // LZ77, four bytes of output declared
        gba.bus.write8(src + 4, 0x40); // first piece plain, second a copy
        gba.bus.write8(src + 5, b'A');
        gba.bus.write8(src + 6, 0x70); // copy the byte before it, ten times over
        gba.bus.write8(src + 7, 0x00);
        for i in 0..16 {
            gba.bus.write8(dst + i, 0);
        }
        gba.cpu.r[0] = src;
        gba.cpu.r[1] = dst;
        continuum_gb::gba::bios::swi(&mut gba.cpu, &mut gba.bus, 0x11);
        (0..16).map(|i| gba.bus.peek8(dst + i) as u8).collect()
    };
    let inside = unpack(true);
    assert_eq!(&inside[..4], b"AAAA");
    assert!(inside[4..].iter().all(|b| *b == 0), "nothing past the declared size: {inside:?}");
    let spilled = unpack(false);
    assert_eq!(&spilled[..11], b"AAAAAAAAAAA");
    assert!(spilled[11..].iter().all(|b| *b == 0), "only the run itself spills over: {spilled:?}");
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
