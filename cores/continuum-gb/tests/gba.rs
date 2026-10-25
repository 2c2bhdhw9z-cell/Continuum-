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

/// Alyosha's timing tests (alyosha-tas/gba-tests, MIT): jsmolka's collection plus cycle-exact
/// tests of the prefetch buffer, wait states, DMA, timers, interrupts and serial, each written
/// against a real console. Like jsmolka's, a test ends in `B .` with r12 = 0 when it passed
/// (or a failed test's number, or the time it measured). These pass and must keep passing.
/// All 18 prefetch-buffer tests are among them; most of the rest that fail need a DMA that
/// can be interrupted by another, interrupts taken a few cycles late, the serial port's own
/// timing, or the real BIOS's start-up timing.
const ALYOSHA_PASS: &[&str] = &[
    "Bus/DMA_Outside_Bios_Bus.gba", "DMA/DMA_Mode_Change.gba", "LDM/LDM_ALU.gba", "LDM/LDM_ALU_IMM.gba",
    "LDM/LDM_ALU_Store.gba", "LDM/LDM_Bx.gba", "LDM/LDM_LD.gba", "LDM/LDM_MUL_UL_32.gba", "LDM/LDM_MUL_UL_SL.gba",
    "LDM/LDM_Swap.gba", "STM_ALU_IMM.gba", "Serial/serial_time_start_bit.gba", "arm/Coprocessor_14.gba",
    "arm/arm.gba", "bios/bios.gba", "fifo_dma/fifo_dma_disable_3.gba", "fifo_dma/fifo_dma_disable_5.gba",
    "irq/BL_1.gba", "irq/IF_Timer.gba", "irq/IRQ_sub.gba", "irq/IRQ_sub_slow.gba", "memory/PAL_8_bit_writes.gba",
    "memory/VRAM_8_bit_writes.gba", "memory/memory.gba", "nes/nes.gba", "ppu/hello.gba", "ppu/shades.gba",
    "ppu/start_up.gba", "ppu/start_up_vbl_irq.gba", "ppu/start_up_vbl_irq_halt.gba", "ppu/stripes.gba",
    "prefetcher/bounday_test_1.gba", "prefetcher/prefetcher_boundary_1.gba", "prefetcher/prefetcher_boundary_2.gba",
    "prefetcher/prefetcher_boundary_3.gba", "prefetcher/prefetcher_boundary_4.gba",
    "prefetcher/prefetcher_branch_thumb.gba", "prefetcher/prefetcher_branch_thumb_2.gba",
    "prefetcher/prefetcher_branch_thumb_3.gba", "prefetcher/prefetcher_branch_thumb_4.gba",
    "prefetcher/prefetcher_branch_thumb_5.gba", "prefetcher/prefetcher_branch_thumb_6.gba",
    "prefetcher/prefetcher_branch_thumb_arm.gba", "prefetcher/prefetcher_branch_thumb_arm_3.gba",
    "prefetcher/prefetcher_branch_thumb_arm_4.gba", "prefetcher/prefetcher_dma.gba",
    "prefetcher/prefetcher_full_arm.gba", "prefetcher/prefetcher_full_arm_2.gba",
    "prefetcher/prefetcher_full_thumb.gba", "save/flash128.gba", "save/flash64.gba", "save/none.gba",
    "save/sram.gba", "thumb/Push_no_regs.gba", "thumb/blx.gba", "thumb/cpy.gba", "thumb/thumb.gba",
    "timer/timer_disable.gba", "timing/dma_long.gba", "timing/prefetch_enable.gba",
    "timing/prefetch_enable_2.gba", "unsafe/fifo_dma_disable_jam.gba", "unsafe/fifo_dma_disable_jam_2.gba",
    "unsafe/unsafe.gba",
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
