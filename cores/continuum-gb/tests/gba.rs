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

/// Waiting for the next VBlank (BIOS call 5) is "throw away any VBlank that already came, then
/// wait for a new one". The wait halts the machine and runs itself again after each interrupt, so
/// the throwing away has to happen on the call itself and never on those re-runs. It used to
/// happen every time, which threw away the very VBlank the interrupt had just brought: every game
/// that waits for VBlank this way - which is most of them, and all of the ones built with libgba
/// or butano - sat frozen on its first frame with nothing but a blank screen.
#[test]
fn the_vblank_wait_ends_on_the_vblank_it_waited_for() {
    let mut gba = Gba::new(vec![0u8; 0x8000]);
    // The BIOS's own note of which interrupts have been dealt with, at the end of its RAM.
    let flag = |g: &Gba| g.bus.peek16(0x0300_7FF8) & 1;
    let wait = |g: &mut Gba| {
        g.bus.halted = false;
        g.cpu.pc = 0x0800_0100;
        continuum_gb::gba::bios::swi(&mut g.cpu, &mut g.bus, 5);
    };
    // A VBlank that came and went before the call is thrown away: the game wants the next one.
    gba.bus.write16(0x0300_7FF8, 1);
    wait(&mut gba);
    assert!(gba.bus.halted, "the wait halts the machine");
    assert_eq!(gba.bus.io[0x208], 1, "and turns interrupts on, or it could never end");
    assert_eq!(flag(&gba), 0, "the older VBlank is thrown away");
    assert_eq!(gba.cpu.pc, 0x0800_0100 - 4, "and the call is left to run itself again");
    // Nothing has come yet: running again halts again instead of letting the game on.
    wait(&mut gba);
    assert!(gba.bus.halted, "nothing has come, so it keeps waiting");
    assert_eq!(flag(&gba), 0);
    // The interrupt's own handler notes the VBlank; the next run of the call ends the wait.
    gba.bus.write16(0x0300_7FF8, 1);
    wait(&mut gba);
    assert!(!gba.bus.halted, "the wait is over");
    assert_eq!(flag(&gba), 0, "and the VBlank it waited for is taken");
    // The next call is a new call again, so an older VBlank is thrown away once more.
    gba.bus.write16(0x0300_7FF8, 1);
    wait(&mut gba);
    assert!(gba.bus.halted, "a new call waits for a new VBlank");
    assert_eq!(flag(&gba), 0);
    // Waiting for any other interrupt (call 4) works the same way, and a game that asks not to
    // throw the old flag away is let straight through when that interrupt has already come.
    let mut gba = Gba::new(vec![0u8; 0x8000]);
    gba.bus.write16(0x0300_7FF8, 0x40); // a keypad interrupt, already dealt with
    gba.cpu.r[0] = 0; // "do not throw it away"
    gba.cpu.r[1] = 0x40;
    gba.cpu.pc = 0x0800_0100;
    continuum_gb::gba::bios::swi(&mut gba.cpu, &mut gba.bus, 4);
    assert!(!gba.bus.halted, "it had already come, so there is nothing to wait for");
    assert_eq!(gba.bus.peek16(0x0300_7FF8) & 0x40, 0);
    // The same call asking to throw it away waits for a new one.
    gba.bus.write16(0x0300_7FF8, 0x40);
    gba.cpu.r[0] = 1;
    gba.cpu.r[1] = 0x40;
    gba.cpu.pc = 0x0800_0100;
    continuum_gb::gba::bios::swi(&mut gba.cpu, &mut gba.bus, 4);
    assert!(gba.bus.halted);
    assert_eq!(gba.bus.peek16(0x0300_7FF8) & 0x40, 0);
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

/// EEPROM through the bus itself, the way a game does it: Super Mario Advance 2 reads its
/// save back this way and called it corrupt when the bus always answered 1.
#[test]
fn eeprom_bus_read16_read32_and_dma() {
    use continuum_gb::gba::save::SaveKind;
    // A cartridge with an EEPROM save chip: the marker string is what detection looks for.
    let mut rom = vec![0u8; 0x8000];
    rom[0x200..0x208].copy_from_slice(b"EEPROM_V");
    let mut gba = Gba::new(rom);
    assert_eq!(gba.bus.save.kind, SaveKind::Eeprom);
    gba.bus.save.eeprom_dma(81); // 14-bit addresses, as a DMA to the chip would say
    let addr = 5u32;
    let pattern = 0x0123_4567_89AB_CDEFu64;
    // Clock bits into the chip one at a time, through Bus::write16 to $0D000000.
    let write_bits = |gba: &mut Gba, v: u64, n: u32| {
        for i in (0..n).rev() {
            gba.bus.write16(0x0D00_0000, ((v >> i) & 1) as u16);
        }
    };
    write_bits(&mut gba, 0b10, 2); // write command
    write_bits(&mut gba, u64::from(addr), 14);
    write_bits(&mut gba, pattern, 64);
    write_bits(&mut gba, 0, 1); // stop bit
    assert_eq!(&gba.bus.save.data[40..48], &[0x01, 0x23, 0x45, 0x67, 0x89, 0xAB, 0xCD, 0xEF]);
    // Idle reads answer 1: the chip is ready, with no read running.
    assert_eq!(gba.bus.read16(0x0D00_0000), 1);
    assert_eq!(gba.bus.read32(0x0D00_0000), 0x0001_0001);
    // Read it back through Bus::read16: four dummy bits, then the 64 data bits.
    write_bits(&mut gba, 0b11, 2); // read command
    write_bits(&mut gba, u64::from(addr), 14);
    write_bits(&mut gba, 0, 1);
    let mut v = 0u64;
    for i in 0..68 {
        let b = gba.bus.read16(0x0D00_0000);
        assert!(b <= 1, "an EEPROM read is one bit, got {b}");
        if i < 4 {
            assert_eq!(b, 0, "the first four bits are dummy zeros");
        } else {
            v = v << 1 | u64::from(b);
        }
    }
    assert_eq!(v, pattern, "read16 must return the bits written, not all ones");
    // And through Bus::read32_seq: a word is two reads, two bits, low bit first.
    write_bits(&mut gba, 0b11, 2);
    write_bits(&mut gba, u64::from(addr), 14);
    write_bits(&mut gba, 0, 1);
    let mut v = 0u64;
    for i in 0..34 {
        let w = gba.bus.read32_seq(0x0D00_0000, false);
        let (lo, hi) = (w & 1, (w >> 16) & 1);
        if i < 2 {
            assert_eq!((lo, hi), (0, 0), "the first four bits are dummy zeros");
        } else {
            v = v << 1 | u64::from(lo);
            v = v << 1 | u64::from(hi);
        }
    }
    assert_eq!(v, pattern, "read32_seq must return the same bits");
    // A game usually reads the chip by DMA, not one bit at a time: 68 halfword units from
    // $0D000000 into work RAM, after the same read command.
    write_bits(&mut gba, 0b11, 2);
    write_bits(&mut gba, u64::from(addr), 14);
    write_bits(&mut gba, 0, 1);
    let dst = 0x0300_0000u32;
    let set32 = |gba: &mut Gba, a: u32, v: u32| {
        gba.bus.write16(0x0400_0000 + a, v as u16);
        gba.bus.write16(0x0400_0000 + a + 2, (v >> 16) as u16);
    };
    set32(&mut gba, 0xD4, 0x0D00_0000); // DMA 3 source
    set32(&mut gba, 0xD8, dst); // destination
    gba.bus.write16(0x0400_00DC, 68); // count: 68 bits
    gba.bus.write16(0x0400_00DE, 0x8000); // on, immediate, 16-bit units
    gba.bus.idle(4); // let the transfer's start-up pass
    let _ = gba.bus.read16(0x0200_0000); // and give the bus a reason to run it
    let got: Vec<u32> = (0..68).map(|i| gba.bus.peek16(dst + i * 2)).collect();
    assert_eq!(&got[..4], &[0, 0, 0, 0], "a DMA read starts with the dummy bits too");
    let mut v = 0u64;
    for b in &got[4..] {
        v = v << 1 | u64::from(*b);
    }
    assert_eq!(v, pattern, "a DMA from the EEPROM must carry the same bits");
}

/// The BIOS's sound calls: SoundBias (0x19) and MidiKey2Freq (0x1F).
#[test]
fn bios_sound_bias_and_midi_key_2_freq() {
    let mut gba = Gba::new(vec![0u8; 0x8000]);
    // SOUNDBIAS $C1FF: amplitude bits set, level $1FF.
    gba.bus.write16(0x0400_0088, 0xC1FF);
    gba.cpu.r[0] = 0;
    continuum_gb::gba::bios::swi(&mut gba.cpu, &mut gba.bus, 0x19);
    assert_eq!(gba.bus.peek16(0x0400_0088), 0xC000, "level 0, upper bits kept");
    gba.cpu.r[0] = 1;
    continuum_gb::gba::bios::swi(&mut gba.cpu, &mut gba.bus, 0x19);
    assert_eq!(gba.bus.peek16(0x0400_0088), 0xC200, "any nonzero level is $200");
    gba.cpu.r[0] = 0xFFFF;
    continuum_gb::gba::bios::swi(&mut gba.cpu, &mut gba.bus, 0x19);
    assert_eq!(gba.bus.peek16(0x0400_0088), 0xC200);

    // A waveform sampled at 8000 Hz, tuned to key 60: its `freq` is 8000 x 2^((180-60)/12).
    let wa = 0x0200_1000u32;
    gba.bus.write32(wa + 4, 8000 * 1024);
    let freq = |gba: &mut Gba, mk: u32, fp: u32| -> u32 {
        gba.cpu.r[0] = wa;
        gba.cpu.r[1] = mk;
        gba.cpu.r[2] = fp;
        continuum_gb::gba::bios::swi(&mut gba.cpu, &mut gba.bus, 0x1F);
        gba.cpu.r[0]
    };
    assert_eq!(freq(&mut gba, 60, 0), 8000, "its own key plays at its sampling rate");
    assert_eq!(freq(&mut gba, 72, 0), 16000, "an octave up doubles it");
    assert_eq!(freq(&mut gba, 48, 0), 4000, "an octave down halves it");
    assert_eq!(freq(&mut gba, 168, 0), 8000 * 1024 / 2);
    let half_semitone = freq(&mut gba, 60, 128);
    assert!((8230..=8240).contains(&half_semitone), "half a semitone up: {half_semitone}");
    // GBATEK's BIOS-read identity: at key 168 the answer is the word at wa+4, halved.
    gba.bus.write32(wa + 4, 0x1234_5678);
    assert_eq!(freq(&mut gba, 168, 0), 0x091A_2B3C);
}

/// The BIOS sound driver (0x1A-0x1E, 0x28-0x2A): start it, play a note through it, stop it.
#[test]
fn bios_sound_driver() {
    let mut gba = Gba::new(vec![0u8; 0x8000]);
    let area = 0x0200_0000u32;
    let call = |gba: &mut Gba, n: u32| continuum_gb::gba::bios::swi(&mut gba.cpu, &mut gba.bus, n);
    // Nothing happens before the driver is started.
    call(&mut gba, 0x1C);
    assert_eq!(gba.bus.peek32(area), 0);
    // A waveform in work RAM: a looped constant tone, 1000 samples of 0x40, tuned to key 60.
    let wave = 0x0200_1000u32;
    gba.bus.write16(wave + 2, 0x4000); // looped
    gba.bus.write32(wave + 4, 8000 * 1024); // freq: 8000 Hz at key 60
    gba.bus.write32(wave + 8, 0); // loop from the top
    gba.bus.write32(wave + 12, 1000); // 1000 samples
    for i in 0..1000 {
        gba.bus.write8(wave + 16 + i, 0x40);
    }
    // Start the driver on its work area.
    gba.cpu.r[0] = area;
    call(&mut gba, 0x1A);
    assert_eq!(gba.bus.peek32(area), 1, "the area is marked initialized");
    assert_eq!(gba.bus.sound_area, area);
    // Its frequency: MidiKey2Freq of the waveform at key 60 is 8000 Hz.
    gba.cpu.r[0] = wave;
    gba.cpu.r[1] = 60;
    gba.cpu.r[2] = 0;
    call(&mut gba, 0x1F);
    let fr = gba.cpu.r[0];
    assert_eq!(fr, 8000);
    // One channel, full volume both sides, no attack time, holding at full volume.
    let ch = area + 0x14;
    gba.bus.write8(ch, 0x80); // sf: on
    gba.bus.write8(ch + 2, 255); // right volume
    gba.bus.write8(ch + 3, 255); // left volume
    gba.bus.write8(ch + 4, 0); // attack: none, straight to full
    gba.bus.write8(ch + 5, 255); // decay
    gba.bus.write8(ch + 6, 255); // sustain
    gba.bus.write8(ch + 7, 255); // release
    gba.bus.write32(ch + 0x0C, fr);
    gba.bus.write32(ch + 0x10, wave);
    // Mix a frame: both halves of the mix buffer fill with the tone.
    call(&mut gba, 0x1C);
    let pcm = area + 0x194;
    let (right, left) = (gba.bus.peek8(pcm), gba.bus.peek8(pcm + 224));
    assert_eq!((right, left), (63, 63), "64 x 255 x 255 / 65536, both sides");
    assert!(gba.bus.peek16(0x0400_00C6) & 0x8000 != 0, "DMA 1 is feeding FIFO A");
    assert!(gba.bus.peek16(0x0400_00D2) & 0x8000 != 0, "DMA 2 is feeding FIFO B");
    // VSync rewinds both DMAs to the start of their half and counts the transfer.
    call(&mut gba, 0x1D);
    assert_eq!(gba.bus.peek8(area + 0x04), 1);
    let sad = |gba: &Gba, a: usize| u32::from_le_bytes(gba.bus.io[a..a + 4].try_into().unwrap());
    assert_eq!(sad(&gba, 0xBC), pcm, "DMA 1 is rewound to the right half");
    assert_eq!(sad(&gba, 0xC8), pcm + 224, "DMA 2 is rewound to the left half");
    // Key the note off: the release runs it down and the channel stops.
    gba.bus.write8(ch, 0x80 | 0x40);
    gba.bus.write8(ch + 7, 0); // release 0: straight to silent
    call(&mut gba, 0x1C);
    assert_eq!(gba.bus.peek8(ch), 0, "a released channel with no release time stops");
    // VSync off stops the DMA; VSync on starts it again.
    call(&mut gba, 0x28);
    assert_eq!(gba.bus.peek16(0x0400_00C6) & 0x8000, 0);
    call(&mut gba, 0x29);
    assert!(gba.bus.peek16(0x0400_00C6) & 0x8000 != 0);
    // Clear: every channel off, DMA off, mix buffer cleared.
    gba.bus.write8(ch, 0x80);
    call(&mut gba, 0x1E);
    assert_eq!(gba.bus.peek8(ch), 0);
    assert_eq!(gba.bus.peek16(0x0400_00C6) & 0x8000, 0);
    assert_eq!(gba.bus.peek8(pcm), 0, "the mix buffer is cleared too");
    // The jump list is a zeroed buffer: no BIOS-internal entry points exist to hand out.
    let list = 0x0200_2000u32;
    gba.bus.write32(list, 0xDEAD_BEEF);
    gba.cpu.r[0] = list;
    call(&mut gba, 0x2A);
    assert!((0..0x120).step_by(4).all(|i| gba.bus.peek32(list + i as u32) == 0));
}

/// Alyosha's timing tests (alyosha-tas/gba-tests, MIT): jsmolka's collection plus cycle-exact
/// tests of the prefetch buffer, wait states, DMA, timers, interrupts and serial, each written
/// against a real console. Like jsmolka's, a test ends in `B .` with r12 = 0 when it passed
/// (or a failed test's number, or the time it measured). These pass and must keep passing.
/// All 18 prefetch-buffer tests are among them; most of the rest that fail need a DMA that
/// can be interrupted by another, interrupts taken a few cycles late, the serial port's own
/// timing, or the real BIOS's start-up timing.
const ALYOSHA_PASS: &[&str] = &[
    "Bus/DMA_Outside_Bios_Bus.gba", "DMA/DMA_Mode_Change.gba",
    "DMA/DMA_pause_timing_end_1.gba", "DMA/DMA_pause_timing_end_3.gba",
    "DMA/DMA_pause_timing_mid_1.gba", "Interactions/Halt_DMA_IRQ.gba",
    "Interactions/Halt_DMA_IRQ_Read_OAM.gba", "Interactions/Halt_IRQ.gba",
    "Interactions/Internal_Cycle_DMA_IRQ_7.gba",
    "Interactions/Internal_Cycle_DMA_IRQ_Br_pre.gba",
    "Interactions/Internal_Cycle_DMA_IRQ_ST_p3.gba",
    "Interactions/Internal_Cycle_DMA_IRQ_br.gba",
    "Interactions/Internal_Cycle_DMA_IRQ_ldr_IWRAM.gba", "LDM/LDM_ALU.gba",
    "LDM/LDM_ALU_IMM.gba", "LDM/LDM_ALU_Store.gba", "LDM/LDM_Bx.gba", "LDM/LDM_LD.gba",
    "LDM/LDM_MUL_UL_32.gba", "LDM/LDM_MUL_UL_SL.gba", "LDM/LDM_Swap.gba", "STM_ALU_IMM.gba",
    "Serial/serial_read_data.gba", "Serial/serial_time.gba", "Serial/serial_time_2.gba",
    "Serial/serial_time_2M.gba", "Serial/serial_time_2M_2.gba", "Serial/serial_time_2M_3.gba",
    "Serial/serial_time_3.gba", "Serial/serial_time_start_bit.gba", "arm/Coprocessor_14.gba",
    "arm/arm.gba", "bios/bios.gba", "fifo_dma/fifo_3.gba", "fifo_dma/fifo_4.gba",
    "fifo_dma/fifo_6.gba", "fifo_dma/fifo_dma_disable_3.gba",
    "fifo_dma/fifo_dma_disable_4.gba", "fifo_dma/fifo_dma_disable_5.gba", "irq/BL_1.gba",
    "irq/BL_IRQ.gba", "irq/BL_IRQ_2.gba", "irq/BL_IRQ_3.gba", "irq/BL_IRQ_R14.gba",
    "irq/IE.gba", "irq/IF_Timer.gba", "irq/IRQ_sub.gba", "irq/IRQ_sub_2_slow.gba", "irq/IRQ_sub_slow.gba",
    "memory/PAL_8_bit_writes.gba", "memory/VRAM_8_bit_writes.gba", "memory/memory.gba",
    "nes/nes.gba", "ppu/Sprite_Last_VRAM_Access.gba", "ppu/Sprite_Last_VRAM_Access_Free.gba",
    "ppu/hello.gba", "ppu/shades.gba", "ppu/start_up.gba", "ppu/start_up_vbl_irq.gba",
    "ppu/start_up_vbl_irq_halt.gba", "ppu/stripes.gba", "prefetcher/bounday_test_1.gba",
    "prefetcher/prefetcher_boundary_1.gba", "prefetcher/prefetcher_boundary_2.gba",
    "prefetcher/prefetcher_boundary_3.gba", "prefetcher/prefetcher_boundary_4.gba",
    "prefetcher/prefetcher_branch_thumb.gba", "prefetcher/prefetcher_branch_thumb_2.gba",
    "prefetcher/prefetcher_branch_thumb_3.gba", "prefetcher/prefetcher_branch_thumb_4.gba",
    "prefetcher/prefetcher_branch_thumb_5.gba", "prefetcher/prefetcher_branch_thumb_6.gba",
    "prefetcher/prefetcher_branch_thumb_arm.gba",
    "prefetcher/prefetcher_branch_thumb_arm_3.gba",
    "prefetcher/prefetcher_branch_thumb_arm_4.gba", "prefetcher/prefetcher_dma.gba",
    "prefetcher/prefetcher_full_arm.gba", "prefetcher/prefetcher_full_arm_2.gba",
    "prefetcher/prefetcher_full_thumb.gba", "psr/psr.gba", "save/flash128.gba", "save/flash64.gba",
    "save/none.gba", "save/sram.gba", "thumb/Pop_no_regs.gba", "thumb/Push_no_regs.gba",
    "thumb/blx.gba", "thumb/cpy.gba", "thumb/thumb.gba", "timer/timer.gba",
    "timer/timer_disable.gba", "timing/dma_long.gba", "timing/prefetch_enable.gba",
    "timing/prefetch_enable_2.gba", "unsafe/fifo_dma_disable_jam.gba",
    "unsafe/fifo_dma_disable_jam_2.gba", "unsafe/unsafe.gba"
];

#[test]
fn alyosha_timing_tests() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test-roms/alyosha");
    if !root.is_dir() {
        eprintln!("SKIPPED: {} not found (run scripts/fetch-test-roms.sh)", root.display());
        return;
    }
    let mut all = vec![];
    let mut dirs = vec![root.clone()];
    while let Some(d) = dirs.pop() {
        for e in std::fs::read_dir(&d).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                dirs.push(p);
            } else if p.extension().is_some_and(|x| x == "gba") {
                all.push(p);
            }
        }
    }
    all.sort();
    let (mut passed, mut broken, mut newly) = (0, vec![], vec![]);
    for p in &all {
        let name = p.strip_prefix(&root).unwrap().display().to_string();
        let mut gba = Gba::new(std::fs::read(p).unwrap());
        let r = run_to_idle(&mut gba, 600);
        let pinned = ALYOSHA_PASS.contains(&name.as_str());
        match r {
            Some(0) => {
                passed += 1;
                if !pinned {
                    newly.push(name);
                }
            }
            other => {
                if pinned {
                    broken.push(format!("{name}: {other:?}"));
                }
            }
        }
    }
    eprintln!("alyosha: {passed} of {} pass", all.len());
    if !newly.is_empty() {
        eprintln!("also passing now (add them to ALYOSHA_PASS): {newly:?}");
    }
    assert!(broken.is_empty(), "timing tests that passed no longer do: {broken:?}");
    assert_eq!(all.len(), 133, "the pinned collection has 133 tests");
}

/// FuzzARM (DenSinH/FuzzARM, MIT): random ARM/Thumb instruction tests, ~9992 per ROM, run to the
/// idle loop (about 831 frames) and reporting through EWRAM: a failed test writes 'AAAA' or
/// 'TTTT' at 0x02000000, so a clean EWRAM means every test passed.
#[test]
fn fuzzarm_tests() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test-roms/fuzzarm");
    if !root.is_dir() {
        eprintln!("SKIPPED: {} not found (run scripts/fetch-test-roms.sh)", root.display());
        return;
    }
    let mut names: Vec<String> = std::fs::read_dir(&root).unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.ends_with(".gba"))
        .collect();
    names.sort();
    let mut passed = Vec::new();
    let mut failed = Vec::new();
    for n in &names {
        let mut gba = Gba::new(std::fs::read(root.join(n)).unwrap());
        match run_to_idle(&mut gba, 2000) {
            Some(0) => {
                let marker = u32::from_le_bytes(gba.bus.ewram[0..4].try_into().unwrap());
                if marker == 0 {
                    passed.push(n.clone());
                } else {
                    failed.push(format!("{n}: failed test dumped to EWRAM (marker {marker:#010x})"));
                }
            }
            other => failed.push(format!("{n}: never finished ({other:?}, pc {:08X})", gba.cpu.pc)),
        }
    }
    eprintln!("fuzzarm: {} of {} pass", passed.len(), names.len());
    for f in &failed {
        eprintln!("  FAIL {f}");
    }
    assert_eq!(names.len(), 5, "the pinned FuzzARM set has 5 ROMs");
    for must in FUZZARM_PASS {
        assert!(passed.iter().any(|p| p == must), "fuzzarm regressed: {must}");
    }
}

/// The FuzzARM ROMs that pass. This list only ever grows.
const FUZZARM_PASS: &[&str] = &[
    "ARM_Any.gba",
    "ARM_DataProcessing.gba",
    "FuzzARM.gba",
    "THUMB_Any.gba",
    "THUMB_DataProcessing.gba",
];

/// AGBEEG Aging Cartridge (zaydlang/AGBEEG-Aging-Cartridge v0.0.2, MIT): the clean-room
/// reimplementation of Nintendo's AGS Aging Cartridge. It runs all its tests at boot and reports
/// through the CI register ($04999990, captured in `Bus::ci_log`): null-terminated test name,
/// null-terminated "PASS"/"FAIL", repeated, then null-terminated "DONE".
#[test]
fn ags_aging_tests() {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("test-roms/ags-aging/AGBEEG_AGING_CARTRIDGE.gba");
    let Some(data) = std::fs::read(&p).ok() else {
        eprintln!("SKIPPED: {} not found (run scripts/fetch-test-roms.sh)", p.display());
        return;
    };
    let mut gba = Gba::new(data);
    let mut done = false;
    for _ in 0..600 {
        gba.run_frame();
        if gba.bus.ci_log.windows(4).any(|w| w == b"DONE") {
            done = true;
            break;
        }
    }
    assert!(done, "AGBEEG never reported DONE (ci_log={:?})", String::from_utf8_lossy(&gba.bus.ci_log));
    // Split the log on NULs into (name, result) pairs, ending at "DONE".
    let log = String::from_utf8_lossy(&gba.bus.ci_log);
    let parts: Vec<&str> = log.split('\0').filter(|s| !s.is_empty()).collect();
    let mut results = Vec::new();
    let mut i = 0;
    while i + 1 < parts.len() && parts[i] != "DONE" {
        results.push((parts[i], parts[i + 1]));
        i += 2;
    }
    let (mut passed, mut failed) = (Vec::new(), Vec::new());
    for (name, result) in &results {
        if *result == "PASS" {
            passed.push(*name);
        } else {
            failed.push(format!("{name}: {result}"));
        }
    }
    eprintln!("ags-aging: {} passed, {} failed, of {}", passed.len(), failed.len(), results.len());
    for f in &failed {
        eprintln!("  FAIL {f}");
    }
    assert_eq!(results.len(), 6, "the pinned AGBEEG set has 6 tests");
    for must in AGS_AGING_PASS {
        assert!(passed.iter().any(|p| p == must), "ags-aging regressed: {must}");
    }
}

/// The AGBEEG tests that pass. This list only ever grows. All 6 pass since Item 4
/// (2026-11-25): "SWP LOCKS BUS" (SWP now locks the bus for its read and write, so a DMA
/// that comes due in between waits until the write) and "CPU RUNS IDLES DURING DMA" (the
/// DMA catch-up's overlap accounting was fixed: it measured the transfer's length as
/// `pending - before`, which is 1 minus the CPU's own pending cycles — 0 for a one-cycle
/// idle and a wrap to ~2^32 for any more, so the machine clock jumped by billions of
/// cycles and the one-cycle error in the 1-idle case showed through).
const AGS_AGING_PASS: &[&str] = &[
    "CPU RUNS IDLES DURING DMA",
    "LDM DOES NOT LOCK BUS",
    "ROM ACCESS DURING PREFETCH",
    "STM DOES NOT LOCK BUS",
    "SWP LOCKS BUS",
    "TOGGLE PREFETCHER",
];

/// Prints every instruction of $GBA_TRACE (a file under test-roms/) whose address is in
/// [$GBA_LO, $GBA_HI), with the cycles it took (a debugging aid; with `--features gba_trace`
/// every bus access too).
#[test]
fn gba_trace() {
    let Ok(name) = std::env::var("GBA_TRACE") else { return };
    let hex = |k: &str, d: u32| std::env::var(k).ok().and_then(|v| u32::from_str_radix(&v, 16).ok()).unwrap_or(d);
    let (lo, hi) = (hex("GBA_LO", 0x0800_0000), hex("GBA_HI", 0x0800_1000));
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test-roms").join(name);
    let mut gba = Gba::new(std::fs::read(path).unwrap());
    let mut shown = 0;
    while shown < 2000 && gba.bus.cycles < 50_000_000 {
        let (pc, t) = (gba.cpu.pc, gba.bus.now());
        let op = if gba.cpu.thumb() { gba.bus.peek16(pc) } else { gba.bus.peek32(pc) };
        gba.step();
        if (lo..hi).contains(&pc) {
            println!("{pc:08X} {} {op:08X} +{:3}", if gba.cpu.thumb() { "T" } else { "A" }, gba.bus.now() - t);
            shown += 1;
        }
    }
}

/// nba-emu/hw-test (MIT, pinned to master 3932a0e5): hardware tests for the Advance that
/// print through the libgba text console — a tilemap at VRAM 0x06002000 (screen base
/// block 4) whose tile index is the character code — and finish with a `pass: N` /
/// `total: M` summary, or `congratulations!` when everything passed. The older archive
/// IRQ tests print one `#N NOPs ... PASS/FAIL` line per case and no summary.
fn hw_console(gba: &Gba) -> Vec<String> {
    let vram = &gba.bus.vram;
    let mut lines = Vec::new();
    for row in 0..32 {
        let mut s = String::new();
        for col in 0..32 {
            let off = 0x2000 + (row * 32 + col) * 2;
            let t = u16::from_le_bytes([vram[off], vram[off + 1]]) & 0x3FF;
            s.push(if (0x20..0x7F).contains(&t) { char::from(t as u8) } else { ' ' });
        }
        lines.push(s.trim_end().to_string());
    }
    while lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    lines
}

/// Runs one hw-test ROM until its summary appears or its console text stops changing
/// (120 frames stable), at most 1200 frames. Returns the console lines.
fn hw_run(name: &str) -> Option<Vec<String>> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test-roms/hw-test").join(name);
    let data = std::fs::read(&p).ok()?;
    let mut gba = Gba::new(data);
    let mut last = Vec::new();
    let mut stable = 0;
    for f in 0..1200 {
        gba.run_frame();
        if f % 10 != 9 {
            continue;
        }
        let lines = hw_console(&gba);
        if lines.iter().any(|l| l.starts_with("pass:") || l.contains("congratulations!")) {
            return Some(lines);
        }
        if lines == last {
            stable += 10;
            if stable >= 120 {
                return Some(lines);
            }
        } else {
            stable = 0;
            last = lines;
        }
    }
    Some(hw_console(&gba))
}

/// nba-emu/hw-test, all of it: every prebuilt ROM in the repo, run headless, its console
/// text parsed for PASS/FAIL. Reports each ROM's count; the fully-passing set is pinned
/// in HW_TEST_PASSING below. The visual-only ROMs (ppu/mode2, mode3, mode4, bgpd, bgx,
/// dispcnt-latch, greenswap, sprite-hmosaic) draw a picture with no text result, and the
/// two menu ROMs (ppu/ram-access-timing, ppu/status-irq-dma) are timing sweeps that dump
/// a table to SRAM rather than a pass/fail — neither can be checked headlessly, so they
/// are reported as such and not pinned.
#[test]
fn hw_test_report() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test-roms/hw-test");
    if !root.is_dir() {
        eprintln!("SKIPPED: hw-test not fetched (run scripts/fetch-test-roms.sh)");
        return;
    }
    let mut all = vec![];
    let mut dirs = vec![root.clone()];
    while let Some(d) = dirs.pop() {
        for e in std::fs::read_dir(&d).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                dirs.push(p);
            } else if p.extension().is_some_and(|x| x == "gba") {
                all.push(p);
            }
        }
    }
    all.sort();
    let mut pass = Vec::new();
    let mut fail = Vec::new();
    let mut noword = Vec::new();
    for p in &all {
        let name = p.strip_prefix(&root).unwrap().display().to_string();
        let Some(lines) = hw_run(&name) else { continue };
        let (mut ok, mut bad) = (0usize, 0usize);
        let mut fails = Vec::new();
        for (i, l) in lines.iter().enumerate() {
            if l.contains("PASS") {
                ok += 1;
            }
            if l.contains("FAIL") {
                bad += 1;
                // The expected value is on the next line ("  expected: ...").
                let exp = lines.get(i + 1).map(|s| s.trim()).unwrap_or("");
                fails.push(format!("{} ({})", l.trim(), exp));
            }
        }
        // The summary lines, when the test prints them.
        let summary = lines.iter().find(|l| l.starts_with("pass:"));
        let total = lines.iter().find(|l| l.starts_with("total:"));
        let done = lines.iter().any(|l| l.contains("congratulations!"));
        let label = match (summary, total) {
            (Some(p), Some(t)) => format!("{} / {}", p.trim(), t.trim()),
            _ if done => format!("{ok}/{ok} congratulations!"),
            _ => format!("{ok} PASS, {bad} FAIL (no summary)"),
        };
        if bad == 0 && ok > 0 {
            eprintln!("  PASS {name}: {label}");
            pass.push(name.clone());
        } else if ok == 0 && bad == 0 {
            eprintln!("  NOTEXT {name}: no PASS/FAIL lines (visual or menu test)");
            noword.push(name.clone());
        } else {
            eprintln!("  FAIL {name}: {label}");
            for f in &fails {
                eprintln!("    {f}");
            }
            fail.push(name.clone());
        }
    }
    eprintln!("HW-TEST: {} fully pass, {} fail, {} no text result, of {}", pass.len(), fail.len(), noword.len(), all.len());
    assert_eq!(all.len(), 25, "the hw-test set changed; re-pin HW_TEST_PASSING");
    for must in HW_TEST_PASSING {
        assert!(pass.iter().any(|p| p == must), "hw-test regressed: {must}");
    }
}

/// The hw-test ROMs that fully pass on cBoy. This list only ever grows. Pinned P5 Item 3
/// (2026-11-25): 4 of 25 — the two archive IRQ-cancellation tests that pass, the DMA
/// non-sequential-access test, and the timer start/stop test. Pinned P7 (2026-11-25):
/// 6 of 25 — plus the DMA start-delay test (DMA0 startup is +4 cycles, channels 1-3
/// +2) and the IRQ-delay test (IE/IME writes restart the delay for an already-pending
/// flag, and an IRQ within 2 cycles of the write is committed, not cancelled). The 11 failing ROMs are
/// real timing gaps (IRQ service 3-10 cycles early; DMA start-delay 2 cycles early;
/// DMA decrement-boundary ±2; HALTCNT wildly off; PPU STAT/VCOUNT flag timing returning
/// -1/0; VRAM mirror mode 3; timer reload edge cases), each printed in full by the
/// report test. The 10 no-text ROMs are visual-only (mode2/3/4, bgpd, bgx,
/// dispcnt-latch, greenswap, sprite-hmosaic) or menu-driven timing sweeps that dump a
/// table to SRAM (ram-access-timing, status-irq-dma), not pass/fail tests.
const HW_TEST_PASSING: &[&str] = &[
    "archive/irq/cancel-irq-if/cancel-irq-if.gba",
    "archive/irq/cancel-irq-ime/cancel-irq-ime.gba",
    "dma/force-nseq-access/force-nseq-access.gba",
    "dma/start-delay/start-delay.gba",
    "irq/irq-delay/irq-delay.gba",
    "timer/start-stop/start-stop.gba",
];


/// The BIOS's own arithmetic (SWI 6 to 10), which games use in place of their own: the signs a
/// division leaves behind, the value a division by zero answers with, the whole part of a square
/// root, and the approximation ArcTan and ArcTan2 are written against. A real console was measured
/// for the polynomial, and these are what it gives; nothing here may drift.
#[test]
fn bios_math_edge_cases() {
    let mut gba = Gba::new(vec![0u8; 0x8000]);
    let mut call = |n: u32, r0: u32, r1: u32| -> (u32, u32, u32) {
        gba.cpu.r[0] = r0;
        gba.cpu.r[1] = r1;
        gba.cpu.pc = 0x0800_0100;
        continuum_gb::gba::bios::swi(&mut gba.cpu, &mut gba.bus, n);
        (gba.cpu.r[0], gba.cpu.r[1], gba.cpu.r[3])
    };
    // A division is truncated toward zero, so the remainder takes the dividend's sign, and r3 is
    // the size of the quotient without its sign.
    assert_eq!(call(6, 100, 7), (14, 2, 14));
    assert_eq!(call(6, (-100i32) as u32, 7), ((-14i32) as u32, (-2i32) as u32, 14));
    assert_eq!(call(6, 100, (-7i32) as u32), ((-14i32) as u32, 2, 14));
    assert_eq!(call(6, (-100i32) as u32, (-7i32) as u32), (14, (-2i32) as u32, 14));
    // INT_MIN / -1 has no answer in 32 bits: the console leaves INT_MIN and no remainder.
    assert_eq!(call(6, 0x8000_0000, 0xFFFF_FFFF), (0x8000_0000, 0, 0x8000_0000));
    // Dividing by zero divides nothing: the dividend's sign alone, the dividend as the remainder,
    // and 1. A zero dividend counts as positive, as mGBA answers it too.
    assert_eq!(call(6, 100, 0), (1, 100, 1));
    assert_eq!(call(6, (-100i32) as u32, 0), (0xFFFF_FFFF, (-100i32) as u32, 1));
    // DivArm is the same division with the two registers the other way round.
    assert_eq!(call(7, 7, 100), (14, 2, 14));
    // Sqrt is the whole part of the square root of the word, read without a sign.
    for (x, want) in [(0u32, 0u32), (1, 1), (2, 1), (3, 1), (4, 2), (0xFFFF, 0xFF), (0xFFFF_FFFF, 0xFFFF)] {
        let (v, _, _) = call(8, x, 0);
        assert_eq!(v, want, "sqrt({x:#X})");
    }
    // A value just under a square never rounds up to it.
    for k in [1u32, 2, 3, 255, 256, 4096, 65535] {
        let (v, _, _) = call(8, k * k - 1, 0);
        assert_eq!(v, k - 1, "sqrt({} - 1)", k * k);
    }
    // ArcTan of a tangent of 1 is an eighth of a turn (0x2000 of 0x10000), and its working is
    // left in r1 and r3 for anything that reads them back. The registers are words, so the
    // polynomial's own term is compared as the word it is, sign and all.
    let (v, a, b) = call(9, 1 << 14, 0);
    assert_eq!(v, 0x2000, "arctan(1)");
    assert_eq!(a, (-0x4000i32) as u32, "the polynomial's own term");
    assert!(b > 0x2000, "and a coefficient it ended on");
    // On an axis ArcTan2 is exact: 0 along +x, a quarter turn along +y, and a half or three
    // quarters the other way.
    assert_eq!(call(10, 100, 0).0 & 0xFFFF, 0);
    assert_eq!(call(10, 0, 100).0, 0x4000);
    assert_eq!(call(10, (-100i32) as u32, 0).0, 0x8000);
    assert_eq!(call(10, 0, (-100i32) as u32).0, 0xC000);
    // A pair on a diagonal sits on an eighth of a turn, and each quadrant adds its own quarters.
    for ((x, y), want) in [
        ((100i32, 100i32), 0x2000i32), ((-100, 100), 0x6000),
        ((-100, -100), 0xA000), ((100, -100), 0xE000),
    ] {
        let (v, _, r3) = call(10, x as u32, y as u32);
        let v = v as i32;
        assert!((v - want).abs() <= 0x20, "arctan2({x}, {y}) = {v:#X}, want {want:#X}");
        assert_eq!(r3, 0x170);
    }
}

/// The halfwords the mGBA suite writes $FFFF to and reads back (its `io-read.c` table): the
/// Advance's registers answer with the switches they have, the halves with no register behind
/// them read zero, and the write-only ones read the shared lines. Nothing here may drift.
#[test]
fn io_read_masks_are_the_consoles() {
    /// Writes `v` to a register and reads it back, as the suite's `_runTest` does.
    fn read_back(gba: &mut Gba, a: u32, v: u16) -> u32 {
        gba.bus.write16(0x0400_0000 + a, v);
        gba.bus.read16(0x0400_0000 + a) & 0xFFFF
    }
    let mut gba = Gba::new(vec![0u8; 0x8000]);
    // Sound: a register answers with the bits the Advance's chip really has. The select and
    // the envelope registers are whole; the frequency ones only the top of them; the trigger
    // and the length registers nothing below it. (SOUNDCNT_L reads zero while the sound
    // circuit is off, so it is switched on first.)
    gba.bus.write16(0x0400_0084, 0x0080);
    for (a, want) in [
        (0x60u32, 0x007Fu32),
        (0x62, 0xFFC0),
        (0x64, 0x4000),
        (0x68, 0xFFC0),
        (0x6C, 0x4000),
        (0x70, 0x00E0),
        (0x72, 0xE000),
        (0x74, 0x4000),
        (0x78, 0xFF00),
        (0x7C, 0x40FF),
        (0x80, 0xFF77),
        (0x82, 0x770F),
        (0x90, 0xFFFF),
        (0x9E, 0xFFFF),
    ] {
        assert_eq!(read_back(&mut gba, a, 0xFFFF), want, "sound register {a:#04X}");
    }
    // SOUNDCNT_X is the master switch in bit 7 and the running channels in the four bits below
    // it; the three between them read zero (the Game Boy's own chip raises them), which is how
    // a game tells the two apart.
    let status = read_back(&mut gba, 0x84, 0xFFFF);
    assert_eq!(status & 0xFF80, 0x0080, "SOUNDCNT_X {status:#06X}");
    assert_eq!(status & 0x0070, 0, "the three bits between read zero");
    let status = read_back(&mut gba, 0x84, 0);
    assert_eq!(status, 0, "SOUNDCNT_X with the switch off");
    gba.bus.write16(0x0400_0084, 0x0080);
    // The halves between the sound registers have nothing behind them.
    for a in [0x66u32, 0x6A, 0x6E, 0x76, 0x7A, 0x7E, 0x86, 0x8A] {
        assert_eq!(read_back(&mut gba, a, 0xFFFF), 0, "unused half {a:#04X}");
    }
    // A channel's count is written, not read back, and its switch leaves out the Game Pak's
    // DRQ bit on the first three channels (the fourth has it). The count is written alone, so
    // no channel starts.
    for a in [0xB8u32, 0xC4, 0xD0, 0xDC] {
        assert_eq!(read_back(&mut gba, a, 0xFFFF), 0, "count {a:#04X}");
    }
    for a in [0xBAu32, 0xC6, 0xD2] {
        assert_eq!(read_back(&mut gba, a, 0x77FF), 0x77E0, "switch {a:#04X}");
    }
    assert_eq!(read_back(&mut gba, 0xDE, 0x77FF), 0x77E0, "the fourth channel's switch");
    // The two halves in the gap after SOUNDBIAS answer as a write-only register below them
    // does: with the shared lines, not with zero.
    let lines = read_back(&mut gba, 0x10, 0xFFFF);
    for a in [0x8Cu32, 0x8E] {
        assert_eq!(read_back(&mut gba, a, 0xFFFF), lines, "the lines at {a:#04X}");
    }
    // Above the mirrors the region is unmapped and the shared lines answer there too.
    assert_eq!(gba.bus.read16(0x0400_100C) & 0xFFFF, lines, "above the mirrors");
}
