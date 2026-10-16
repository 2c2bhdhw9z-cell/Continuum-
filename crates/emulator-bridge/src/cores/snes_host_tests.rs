//! Opt-in: host builds of Mesen 2 and bsnes playing SNES games inside a real engine session, for
//! what Mesen 2 needs before it can be the second SNES choice: the in-game save kept by the app
//! (`<game>.srm`, the same file bsnes keeps, and no file of Mesen's own), save states, typed
//! Game Genie and Pro Action Replay codes changing what the game sees, work RAM for RAM search
//! and achievements, and its speed next to bsnes. A child of `bridge.rs` (like the BlastEm tests)
//! so it can run a session without a renderer.
//!
//! `MESEN2_SO=path/to/mesen2_libretro.so` (MesenCE at its pin with both Continuum patches, built
//! with `make -f Makefile.libretro HAVE_LTO=0`, the flags the iPhone build uses), and for the
//! comparisons `BSNES_SO=path/to/bsnes_libretro.so` (bsnes-libretro at its pin with its patch).
//! `SNES_HOMEBREW_DIR` holds the open-licensed games the repository does not ship: Artemio's 240p
//! Test Suite (GPL-2.0, `240pSuite.sfc`), MicroPython for SNES's workstation (MIT,
//! FabianKuebler/micropython-snes, `mpyos.sfc`, which keeps its files in battery RAM) and
//! Gothicvania (MIT, donth77/snes-homebrew, `gothicvania.sfc`). Without them the tests that need
//! them say they were skipped and pass. Run with `--release` for the speed numbers.

use super::EmulatorBridge;
use crate::cheats::search::{SearchFilter, SearchWidth};
use crate::cores::{CoreDescriptor, EmulatorCore};
use crate::frame::{FrameGeometry, PixelFormat};
use crate::input::{Button, PadSource};
use crate::memory::{MEMORY_SAVE_RAM, MEMORY_SYSTEM_RAM};
use std::path::{Path, PathBuf};

fn library(var: &str) -> Option<String> {
    let so = std::env::var(var).ok();
    if so.is_none() {
        eprintln!("{var} not set: skipped");
    }
    so
}

fn homebrew(name: &str) -> Option<PathBuf> {
    let path = PathBuf::from(std::env::var("SNES_HOMEBREW_DIR").ok()?).join(name);
    if !path.exists() {
        eprintln!("{} not there: skipped", path.display());
        return None;
    }
    Some(path)
}

fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("continuum-snes-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("BatterySaves")).unwrap();
    dir
}

/// The app's declaration of the core (`CoreCatalog` in ContinuumApp.swift).
fn descriptor(core_id: &str) -> CoreDescriptor {
    let (name, systems, geometry) = match core_id {
        "mesen2" => ("Mesen", vec!["nes".into(), "fds".into(), "snes".into()], FrameGeometry::new(256, 240, 4.0 / 3.0)),
        _ => ("bsnes", vec!["snes".into()], FrameGeometry::new(256, 224, 4.0 / 3.0)),
    };
    CoreDescriptor {
        id: core_id.into(),
        display_name: name.into(),
        systems,
        geometry,
        target_fps: 60.0988,
        audio_sample_rate: 48_000,
        pixel_format: PixelFormat::Xrgb8888,
        module_url: String::new(),
        priority: 0,
    }
}

/// An engine playing `game` on its own copy of the core (cores keep globals), launched the way
/// the app launches one: declared, loaded with `dir` as its system and save folder, attached,
/// then the game opened by its path.
fn engine(core_id: &str, so: &str, dir: &Path, game: &Path) -> EmulatorBridge {
    static LOADS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = LOADS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let copy = std::env::temp_dir().join(format!("continuum-snes-core-{}-{core_id}-{n}.so", std::process::id()));
    std::fs::copy(so, &copy).unwrap();
    let core = unsafe {
        crate::cores::native_core::NativeLibretroCore::load(descriptor(core_id), &copy, dir.to_str(), dir.to_str())
    }
    .unwrap_or_else(|e| panic!("{core_id} would not load: {e}"));
    let mut bridge = EmulatorBridge::new();
    bridge.declare_core(descriptor(core_id));
    bridge.attach_core(core_id, Box::new(core)).unwrap();
    bridge
        .launch_headless_named_for_test(core_id, &[], game.to_str().unwrap())
        .unwrap_or_else(|e| panic!("{core_id} would not open {}: {e} ({})", game.display(), log()));
    bridge
}

fn log() -> String {
    crate::cores::native_core::last_core_log_line()
}

fn core(bridge: &EmulatorBridge) -> &dyn EmulatorCore {
    bridge.session.as_ref().expect("a game is running").core.as_ref()
}

fn ram(bridge: &EmulatorBridge, region: u32, offset: u64, len: u64) -> Vec<u8> {
    bridge.read_memory(region, offset, len).unwrap()
}

/// How many different colours the last frame has: a game that is not running shows one or two.
fn colours(bridge: &EmulatorBridge) -> usize {
    let video = core(bridge).video().expect("a frame");
    let bpp = video.format.bytes_per_pixel();
    let mut seen = std::collections::HashSet::new();
    for y in 0..video.height as usize {
        let row = &video.data[y * video.stride_bytes..][..video.width as usize * bpp];
        for px in row.chunks_exact(bpp) {
            seen.insert(px.to_vec());
        }
    }
    seen.len()
}

/// The last frame as a PPM in SNES_OUT, when it is set, for a person to look at.
fn keep_frame(bridge: &EmulatorBridge, name: &str) {
    let Ok(out) = std::env::var("SNES_OUT") else { return };
    let video = core(bridge).video().expect("a frame");
    if video.format != PixelFormat::Xrgb8888 {
        return;
    }
    let mut ppm = format!("P6 {} {} 255\n", video.width, video.height).into_bytes();
    for y in 0..video.height as usize {
        for x in 0..video.width as usize {
            let o = y * video.stride_bytes + x * 4;
            let px = u32::from_ne_bytes(video.data[o..o + 4].try_into().unwrap());
            ppm.extend_from_slice(&[(px >> 16) as u8, (px >> 8) as u8, px as u8]);
        }
    }
    std::fs::write(Path::new(&out).join(format!("{name}.ppm")), ppm).unwrap();
}

/// Presses and lets go of `button`, `hold` frames each way.
fn tap(bridge: &mut EmulatorBridge, button: Button, hold: u32) {
    bridge.set_button(0, PadSource::Gamepad, button, true);
    bridge.step_headless_for_test(hold).unwrap();
    bridge.set_button(0, PadSource::Gamepad, button, false);
    bridge.step_headless_for_test(hold).unwrap();
}

/// A LoROM cartridge of our own, 128 KB with 8 KB of battery RAM, in 65816 machine code (the
/// console starts in its 6502 mode, with 8-bit registers, and this never leaves it):
///
/// - at power-on it adds one to the first byte of battery RAM ($700000), a boot counter, so a
///   save that came back shows up as the next number (Mesen, like a real console, powers on with
///   battery RAM full of random values, so the first number is whatever that was, plus one);
/// - then, over and over, it copies a ROM byte ($008100, which holds $42) to work RAM $7E0010,
///   where a Game Genie or ROM code on it shows, and work RAM $7E0020 (which nothing writes) to
///   $7E0021, where a work RAM code shows, whichever way the core applies it;
/// - its vertical-blank interrupt counts frames in $7E0012.
fn test_cartridge() -> Vec<u8> {
    let mut rom = vec![0u8; 0x2_0000];
    let code: &[u8] = &[
        0x78, //                   $8000 SEI
        0xD8, //                   $8001 CLD
        0xAF, 0x00, 0x00, 0x70, // $8002 LDA $700000
        0x1A, //                   $8006 INC A
        0x8F, 0x00, 0x00, 0x70, // $8007 STA $700000
        0xA9, 0x80, //             $800B LDA #$80
        0x8D, 0x00, 0x42, //       $800D STA $4200      (vertical-blank interrupt on)
        0xAD, 0x00, 0x81, //       $8010 LDA $8100
        0x8F, 0x10, 0x00, 0x7E, // $8013 STA $7E0010
        0xAF, 0x20, 0x00, 0x7E, // $8017 LDA $7E0020
        0x8F, 0x21, 0x00, 0x7E, // $801B STA $7E0021
        0x80, 0xEF, //             $801F BRA $8010
        0x48, //                   $8021 PHA            (the interrupt)
        0xAF, 0x12, 0x00, 0x7E, // $8022 LDA $7E0012
        0x1A, //                   $8026 INC A
        0x8F, 0x12, 0x00, 0x7E, // $8027 STA $7E0012
        0x68, //                   $802B PLA
        0x40, //                   $802C RTI
    ];
    rom[..code.len()].copy_from_slice(code);
    rom[0x100] = 0x42;
    let header = 0x7FC0;
    rom[header..header + 21].copy_from_slice(b"CONTINUUM SNES TEST  ");
    rom[header + 0x15] = 0x20; // LoROM
    rom[header + 0x16] = 0x02; // ROM, RAM and a battery
    rom[header + 0x17] = 0x07; // 128 KB of ROM
    rom[header + 0x18] = 0x03; // 8 KB of RAM
    rom[header + 0x19] = 0x01; // North America
    // The vectors: every interrupt returns at once except the vertical blank and the reset.
    for vector in (0x7FE4..0x7FF0).chain([0x7FF4, 0x7FF8, 0x7FFE]) {
        rom[vector..vector + 2].copy_from_slice(&0x802Cu16.to_le_bytes());
    }
    rom[0x7FEA..0x7FEC].copy_from_slice(&0x8021u16.to_le_bytes());
    rom[0x7FFA..0x7FFC].copy_from_slice(&0x8021u16.to_le_bytes());
    rom[0x7FFC..0x7FFE].copy_from_slice(&0x8000u16.to_le_bytes());
    // The checksum, and the complement beside it.
    rom[header + 0x1C..header + 0x20].copy_from_slice(&[0xFF, 0xFF, 0x00, 0x00]);
    let sum = rom.iter().fold(0u16, |sum, b| sum.wrapping_add(*b as u16));
    rom[header + 0x1C..header + 0x1E].copy_from_slice(&(!sum).to_le_bytes());
    rom[header + 0x1E..header + 0x20].copy_from_slice(&sum.to_le_bytes());
    rom
}

fn write_test_cartridge(dir: &Path) -> PathBuf {
    let path = dir.join("Continuum Test.sfc");
    std::fs::write(&path, test_cartridge()).unwrap();
    path
}

/// A SNES Game Genie code that puts `value` at `address`: the console's own scramble, the inverse
/// of the decoding both Mesen (`CheatManager::ConvertFromSnesGameGenie`) and bsnes (its libretro
/// `retro_cheat_set`) do, written in the Game Genie's letters.
fn game_genie(address: u32, value: u8) -> String {
    let a = address;
    let raw = (value as u32) << 24
        | ((a >> 20) & 0xF) << 10
        | ((a >> 16) & 0xF) << 2
        | ((a >> 12) & 0xF) << 20
        | ((a >> 10) & 0x3)
        | ((a >> 8) & 0x3) << 14
        | ((a >> 4) & 0xF) << 16
        | (a & 0xF) << 6;
    let letters = b"DF4709156BC8A23E";
    let text: String = (0..8).rev().map(|i| letters[(raw >> (i * 4)) as usize & 0xF] as char).collect();
    format!("{}-{}", &text[..4], &text[4..])
}

fn files_named(dir: &Path, extension: &str) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_file())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(extension))
        .collect();
    names.sort();
    names
}

/// An NES cartridge of our own (NROM, 16 KB of program, a battery), 6502 machine code: at power-on
/// it adds one to the first byte of battery RAM ($6000), then copies the ROM byte at $8100 ($42)
/// to RAM $0010 over and over. For checking the NES side is as it was.
fn test_nes_cartridge() -> Vec<u8> {
    let mut prg = vec![0u8; 0x4000];
    let code: &[u8] = &[
        0x78, //             $8000 SEI
        0xD8, //             $8001 CLD
        0xAD, 0x00, 0x60, // $8002 LDA $6000
        0x18, //             $8005 CLC
        0x69, 0x01, //       $8006 ADC #$01
        0x8D, 0x00, 0x60, // $8008 STA $6000
        0xAD, 0x00, 0x81, // $800B LDA $8100
        0x85, 0x10, //       $800E STA $10
        0x4C, 0x0B, 0x80, // $8010 JMP $800B
        0x40, //             $8013 RTI
    ];
    prg[..code.len()].copy_from_slice(code);
    prg[0x100] = 0x42;
    prg[0x3FFA..0x3FFC].copy_from_slice(&0x8013u16.to_le_bytes());
    prg[0x3FFC..0x3FFE].copy_from_slice(&0x8000u16.to_le_bytes());
    prg[0x3FFE..0x4000].copy_from_slice(&0x8013u16.to_le_bytes());
    let mut rom = b"NES\x1A\x01\x01\x02\x00\0\0\0\0\0\0\0\0".to_vec();
    rom.extend_from_slice(&prg);
    rom.extend_from_slice(&[0u8; 0x2000]);
    rom
}

#[test]
fn mesen_still_gives_the_nes_its_memory_cheats_and_saves() {
    let Some(so) = library("MESEN2_SO") else { return };
    let _options = crate::cores::options::test_guard();
    let dir = folder("nes");
    let game = dir.join("Continuum Test.nes");
    std::fs::write(&game, test_nes_cartridge()).unwrap();
    let srm = dir.join("BatterySaves/Continuum Test.srm");
    let srm = srm.to_str().unwrap();
    let mut first = None;
    for boot in 0..2u8 {
        let mut bridge = engine("mesen2", &so, &dir, &game);
        assert_eq!(bridge.load_battery_save_file(srm).unwrap().is_some(), boot > 0);
        bridge.step_headless_for_test(10).unwrap();
        assert_eq!(bridge.memory_size(MEMORY_SYSTEM_RAM), 0x800);
        assert_eq!(bridge.memory_size(MEMORY_SAVE_RAM), 0x2000);
        assert_eq!(ram(&bridge, MEMORY_SYSTEM_RAM, 0x10, 1), [0x42]);
        let count = ram(&bridge, MEMORY_SAVE_RAM, 0, 1)[0];
        if let Some(first) = first {
            assert_eq!(count, u8::wrapping_add(first, 1), "the NES save came back");
        }
        first = Some(count);
        // A raw NES code still works, and an SNES-shaped one does nothing on the NES.
        bridge.apply_cheats(vec!["8100:77".into()], &[1]).unwrap();
        bridge.step_headless_for_test(3).unwrap();
        assert_eq!(ram(&bridge, MEMORY_SYSTEM_RAM, 0x10, 1), [0x77]);
        bridge.apply_cheats(vec!["008100:66".into()], &[1]).unwrap();
        bridge.step_headless_for_test(3).unwrap();
        assert_eq!(ram(&bridge, MEMORY_SYSTEM_RAM, 0x10, 1), [0x42]);
        bridge.persist_battery_save(srm).unwrap();
    }
    assert_eq!(files_named(&dir, ".sav"), Vec::<String>::new());
    assert_eq!(files_named(&dir, ".srm"), Vec::<String>::new());
}

#[test]
fn mesen_starts_snes_homebrew_and_hands_over_work_ram() {
    let Some(so) = library("MESEN2_SO") else { return };
    let _options = crate::cores::options::test_guard();
    // The workstation is white text on blue: two colours. The other two draw pictures.
    for (name, least) in [("240pSuite.sfc", 8), ("gothicvania.sfc", 8), ("mpyos.sfc", 2)] {
        let Some(game) = homebrew(name) else { continue };
        let dir = folder("boot");
        let mut bridge = engine("mesen2", &so, &dir, &game);
        bridge.step_headless_for_test(300).unwrap();
        let seen = colours(&bridge);
        keep_frame(&bridge, &format!("mesen2-{name}"));
        let video = core(&bridge).video().unwrap();
        let declared = core(&bridge).descriptor().geometry;
        eprintln!(
            "{name} on Mesen: {}x{} ({}x{} declared), {seen} colours at 5 s",
            video.width, video.height, declared.base_width, declared.base_height
        );
        // The 224 lines the game draws, as bsnes shows them, with no blank bands in the picture.
        assert_eq!((video.width, video.height), (256, 224), "{name}");
        assert_eq!((declared.base_width, declared.base_height), (256, 224), "{name}");
        assert!((declared.aspect_ratio - 4.0 / 3.0).abs() < 1e-4, "{name}: 4:3 like bsnes, not {}", declared.aspect_ratio);
        assert!(seen >= least, "{name} is not drawing on Mesen: {seen} colours");
        // The SNES's work RAM, all 128 KB of it, and the game has been using it.
        assert_eq!(bridge.memory_size(MEMORY_SYSTEM_RAM), 0x2_0000);
        assert!(ram(&bridge, MEMORY_SYSTEM_RAM, 0, 0x2_0000).iter().any(|b| *b != 0));
        // No memory map, like bsnes: RetroAchievements then reads work RAM, then cartridge RAM.
        assert!(core(&bridge).memory_map().is_empty());
        // RAM search covers the work RAM.
        let regions = bridge.search_regions();
        assert_eq!(regions.len(), 1);
        assert_eq!((regions[0].key.as_str(), regions[0].size), ("system", 0x2_0000));
    }
}

#[test]
fn mesen_keeps_no_save_of_its_own_and_the_apps_save_comes_back() {
    let Some(so) = library("MESEN2_SO") else { return };
    let _options = crate::cores::options::test_guard();
    let dir = folder("battery");
    let game = write_test_cartridge(&dir);
    let srm = dir.join("BatterySaves/Continuum Test.srm");
    let srm = srm.to_str().unwrap();

    let mut bridge = engine("mesen2", &so, &dir, &game);
    // Opened, and nothing there yet: the game has never saved.
    assert_eq!(bridge.load_battery_save_file(srm).unwrap(), None);
    bridge.step_headless_for_test(10).unwrap();
    assert_eq!(bridge.memory_size(MEMORY_SAVE_RAM), 0x2000, "the cartridge's 8 KB");
    assert_eq!(ram(&bridge, MEMORY_SYSTEM_RAM, 0x10, 1), [0x42], "the program is running");
    let first = ram(&bridge, MEMORY_SAVE_RAM, 0, 1)[0];
    assert_eq!(bridge.persist_battery_save(srm).unwrap(), 0x2000);
    assert_eq!(bridge.persist_battery_save(srm).unwrap(), 0, "unchanged, so not written again");
    drop(bridge);
    // Mesen wrote nothing of its own, even when the game closed (when it saves its own file).
    assert_eq!(files_named(&dir, ".srm"), Vec::<String>::new());

    // The next time: the app's save goes in before the first frame, and the game counts on from it.
    for boot in 1..=3u8 {
        let mut bridge = engine("mesen2", &so, &dir, &game);
        assert_eq!(bridge.load_battery_save_file(srm).unwrap(), Some(0x2000));
        bridge.step_headless_for_test(10).unwrap();
        assert_eq!(ram(&bridge, MEMORY_SAVE_RAM, 0, 1), [first.wrapping_add(boot)]);
        assert_eq!(bridge.persist_battery_save(srm).unwrap(), 0x2000);
    }
    assert_eq!(std::fs::read(srm).unwrap()[0], first.wrapping_add(3));
    assert_eq!(files_named(&dir, ".srm"), Vec::<String>::new());
}

#[test]
fn a_save_moves_between_bsnes_and_mesen_both_ways() {
    let (Some(mesen), Some(bsnes)) = (library("MESEN2_SO"), library("BSNES_SO")) else { return };
    let _options = crate::cores::options::test_guard();
    let dir = folder("both");
    let game = write_test_cartridge(&dir);
    let srm = dir.join("BatterySaves/Continuum Test.srm");
    let srm = srm.to_str().unwrap();
    // Start on Mesen, then bsnes, Mesen, bsnes: each one counts on from the other's save.
    let mut expected = None;
    for (turn, (id, so)) in [("mesen2", &mesen), ("bsnes", &bsnes), ("mesen2", &mesen), ("bsnes", &bsnes)]
        .into_iter()
        .enumerate()
    {
        let mut bridge = engine(id, so, &dir, &game);
        let loaded = bridge.load_battery_save_file(srm).unwrap();
        assert_eq!(loaded.is_some(), turn > 0, "{id}, turn {turn}");
        assert_eq!(bridge.memory_size(MEMORY_SAVE_RAM), 0x2000, "{id}: the same 8 KB on both");
        bridge.step_headless_for_test(10).unwrap();
        let count = ram(&bridge, MEMORY_SAVE_RAM, 0, 1)[0];
        if let Some(previous) = expected {
            assert_eq!(count, previous, "{id}, turn {turn}: counted on from the other core's save");
        }
        eprintln!("turn {turn} on {id}: boot counter {count}");
        bridge.persist_battery_save(srm).unwrap();
        expected = Some(count.wrapping_add(1));
    }
    assert_eq!(files_named(&dir, ".srm"), Vec::<String>::new());
}

#[test]
fn micropython_finds_its_files_in_the_save_the_app_kept() {
    let Some(so) = library("MESEN2_SO") else { return };
    let Some(original) = homebrew("mpyos.sfc") else { return };
    let _options = crate::cores::options::test_guard();
    let dir = folder("mpy");
    let game = dir.join("mpyos.sfc");
    std::fs::copy(&original, &game).unwrap();
    let srm = dir.join("BatterySaves/mpyos.srm");
    let srm = srm.to_str().unwrap();

    // First start, no save: the workstation formats its 32 KB of battery RAM (a "MF" mark, version
    // 1) and writes a starter file, hello.py, into the first of its file slots.
    let mut bridge = engine("mesen2", &so, &dir, &game);
    assert_eq!(bridge.load_battery_save_file(srm).unwrap(), None);
    assert_eq!(bridge.memory_size(MEMORY_SAVE_RAM), 0x8000);
    bridge.step_headless_for_test(600).unwrap();
    let sram = ram(&bridge, MEMORY_SAVE_RAM, 0, 0x8000);
    assert_eq!(&sram[0..4], &[0x46, 0x4D, 0x01, 0x00], "formatted");
    assert_eq!(&sram[8..16], b"hello.py");
    assert_eq!(bridge.persist_battery_save(srm).unwrap(), 0x8000);
    drop(bridge);
    assert_eq!(files_named(&dir, ".srm"), Vec::<String>::new());

    // The player's file, as the saved battery RAM has it: hello.py renamed to keep.py. A game that
    // did not get this save back would find unformatted RAM and write hello.py again.
    let mut saved = std::fs::read(srm).unwrap();
    saved[8..16].copy_from_slice(b"keep.py\0");
    std::fs::write(srm, &saved).unwrap();
    let mut bridge = engine("mesen2", &so, &dir, &game);
    assert_eq!(bridge.load_battery_save_file(srm).unwrap(), Some(0x8000));
    bridge.step_headless_for_test(600).unwrap();
    let sram = ram(&bridge, MEMORY_SAVE_RAM, 0, 0x8000);
    assert_eq!(&sram[0..4], &[0x46, 0x4D, 0x01, 0x00]);
    assert_eq!(&sram[8..16], b"keep.py\0", "the workstation kept the file it found");
    keep_frame(&bridge, "mesen2-mpyos-second-start");
    drop(bridge);

    // And bsnes, given the same file, finds it too.
    if let Some(bsnes) = library("BSNES_SO") {
        let mut bridge = engine("bsnes", &bsnes, &dir, &game);
        assert_eq!(bridge.load_battery_save_file(srm).unwrap(), Some(0x8000));
        bridge.step_headless_for_test(600).unwrap();
        assert_eq!(&ram(&bridge, MEMORY_SAVE_RAM, 8, 8), b"keep.py\0");
    }
    assert_eq!(files_named(&dir, ".srm"), Vec::<String>::new());
}

#[test]
fn a_mesen_save_state_comes_back_exactly() {
    let Some(so) = library("MESEN2_SO") else { return };
    let _options = crate::cores::options::test_guard();
    let mut games = vec![("our cartridge".to_string(), None)];
    for name in ["240pSuite.sfc", "gothicvania.sfc", "mpyos.sfc"] {
        if let Some(path) = homebrew(name) {
            games.push((name.to_string(), Some(path)));
        }
    }
    for (name, path) in games {
        let dir = folder("state");
        let game = path.unwrap_or_else(|| write_test_cartridge(&dir));
        let mut bridge = engine("mesen2", &so, &dir, &game);
        bridge.step_headless_for_test(240).unwrap();
        tap(&mut bridge, Button::Start, 4);
        bridge.step_headless_for_test(60).unwrap();
        let state = bridge.save_state().unwrap();
        let at_save = ram(&bridge, MEMORY_SYSTEM_RAM, 0, 0x2_0000);
        bridge.step_headless_for_test(120).unwrap();
        let later = ram(&bridge, MEMORY_SYSTEM_RAM, 0, 0x2_0000);
        // Something for the load to undo even in a game that sits still (the workstation waits
        // for a key): the last byte of work RAM changed from the app's side.
        bridge.write_memory(MEMORY_SYSTEM_RAM, 0x1_FFFF, &[!at_save[0x1_FFFF]]).unwrap();
        let moved = later != at_save;

        bridge.load_state(&state).unwrap();
        assert_eq!(ram(&bridge, MEMORY_SYSTEM_RAM, 0, 0x2_0000), at_save, "{name}: back where it was");
        bridge.step_headless_for_test(120).unwrap();
        assert_eq!(ram(&bridge, MEMORY_SYSTEM_RAM, 0, 0x2_0000), later, "{name}: and it plays on the same way");

        // The same state in a fresh start of the same game, the way it comes back next time.
        let mut fresh = engine("mesen2", &so, &dir, &game);
        fresh.step_headless_for_test(5).unwrap();
        fresh.load_state(&state).unwrap();
        fresh.step_headless_for_test(120).unwrap();
        assert_eq!(ram(&fresh, MEMORY_SYSTEM_RAM, 0, 0x2_0000), later, "{name}: from a fresh start");
        eprintln!(
            "{name}: a {} KB state came back exactly{}",
            state.len() / 1024,
            if moved { ", after the game had moved on" } else { " (the game was sitting still)" }
        );
    }
}

#[test]
fn typed_snes_codes_change_what_the_game_sees_on_mesen_as_on_bsnes() {
    let Some(mesen) = library("MESEN2_SO") else { return };
    let _options = crate::cores::options::test_guard();
    let bsnes = library("BSNES_SO");
    let dir = folder("cheats");
    let game = write_test_cartridge(&dir);
    let genie = game_genie(0x00_8100, 0x77);
    let mut cores = vec![("mesen2", mesen)];
    cores.extend(bsnes.map(|so| ("bsnes", so)));
    for (id, so) in cores {
        let mut bridge = engine(id, &so, &dir, &game);
        bridge.step_headless_for_test(5).unwrap();
        assert_eq!(ram(&bridge, MEMORY_SYSTEM_RAM, 0x10, 1), [0x42], "{id}: no code yet");

        // A Game Genie code on the ROM byte the program copies.
        assert_eq!(bridge.apply_cheats(vec![genie.clone()], &[1]).unwrap(), 1);
        bridge.step_headless_for_test(3).unwrap();
        assert_eq!(ram(&bridge, MEMORY_SYSTEM_RAM, 0x10, 1), [0x77], "{id}: Game Genie {genie}");

        // Switched off: the ROM's own byte again.
        assert_eq!(bridge.apply_cheats(vec![genie.clone()], &[0]).unwrap(), 0);
        bridge.step_headless_for_test(3).unwrap();
        assert_eq!(ram(&bridge, MEMORY_SYSTEM_RAM, 0x10, 1), [0x42], "{id}: Game Genie off");

        // Pro Action Replay on the same ROM byte and on a work RAM byte, in every spelling the
        // cheat screen takes, two codes joined with a '+'.
        for code in ["0081005A+7E00205B", "008100:5A+7E0020:5B", "008100=5A + 7e0020=5b"] {
            assert_eq!(bridge.apply_cheats(vec![code.into()], &[1]).unwrap(), 1);
            bridge.step_headless_for_test(3).unwrap();
            assert_eq!(ram(&bridge, MEMORY_SYSTEM_RAM, 0x10, 1), [0x5A], "{id}: {code}, ROM");
            assert_eq!(ram(&bridge, MEMORY_SYSTEM_RAM, 0x21, 1), [0x5B], "{id}: {code}, work RAM, as the game reads it");
            assert_eq!(ram(&bridge, MEMORY_SYSTEM_RAM, 0x20, 1), [0x5B], "{id}: {code}, work RAM itself");
            bridge.clear_cheats().unwrap();
            bridge.step_headless_for_test(3).unwrap();
            // Only checked on Mesen, which never changes the ROM itself. bsnes writes a ROM code
            // into the ROM and forgets the byte it replaced for every part but the last when it is
            // given two at once (its Interface::cheats builds the kept codes afresh), so on bsnes
            // the first part stays in until the game is opened again.
            if id == "mesen2" {
                assert_eq!(ram(&bridge, MEMORY_SYSTEM_RAM, 0x10, 1), [0x42], "{id}: {code} cleared");
            }
        }

        // A code with one part that is not a code does nothing on Mesen, as on FCEUmm. (bsnes
        // reads any eight characters as a code, so that is not checked against it.)
        if id == "mesen2" {
            bridge.apply_cheats(vec![format!("{genie}+ZZZZ-ZZZZ")], &[1]).unwrap();
            bridge.step_headless_for_test(3).unwrap();
            assert_eq!(ram(&bridge, MEMORY_SYSTEM_RAM, 0x10, 1), [0x42], "{id}: half a code");
            bridge.clear_cheats().unwrap();
        }
        eprintln!("{id}: {genie} and the Pro Action Replay codes all changed what the game read");
    }
}

#[test]
fn ram_search_finds_the_frame_counter_on_mesen_and_a_found_value_can_be_pinned() {
    let Some(so) = library("MESEN2_SO") else { return };
    let _options = crate::cores::options::test_guard();
    let dir = folder("search");
    let game = write_test_cartridge(&dir);
    let mut bridge = engine("mesen2", &so, &dir, &game);
    bridge.step_headless_for_test(10).unwrap();
    assert_eq!(bridge.search_start(SearchWidth::Bits8, false).unwrap(), 0x2_0000);
    for _ in 0..5 {
        bridge.step_headless_for_test(1).unwrap();
        bridge.search_filter(SearchFilter::IncreasedBy(1)).unwrap();
    }
    let hits = bridge.search_results(10);
    assert_eq!(hits.iter().map(|h| h.address).collect::<Vec<_>>(), vec![0x12], "the counter, alone");

    // Pinned through the cheat list, the way the search screen does it.
    let pin = bridge.search_poke_code(0x12, 0x99, 1).unwrap();
    assert_eq!(bridge.apply_cheats(vec![pin], &[1]).unwrap(), 1);
    bridge.step_headless_for_test(10).unwrap();
    assert_eq!(ram(&bridge, MEMORY_SYSTEM_RAM, 0x12, 1), [0x99]);

    // Achievements' view of the same memory, with rcheevos' SNES table (rc_consoles.c, console 3):
    // work RAM from $000000, cartridge RAM from $020000.
    use crate::achievements::memory_map::{build, read, ConsoleRegion, RC_MEMORY_TYPE_SAVE_RAM, RC_MEMORY_TYPE_SYSTEM_RAM};
    let console = [
        ConsoleRegion { start: 0x00000, end: 0x1FFFF, real: 0x07E_0000, kind: RC_MEMORY_TYPE_SYSTEM_RAM },
        ConsoleRegion { start: 0x20000, end: 0x9FFFF, real: 0x100_0000, kind: RC_MEMORY_TYPE_SAVE_RAM },
    ];
    let core = core(&bridge);
    let blocks = build(&console, core.memory_map(), |id| core.memory_region(id).map(|m| (m.as_ptr(), m.len())));
    let byte = |address: u32| {
        let mut out = [0u8; 1];
        (unsafe { read(&blocks, address, &mut out) } == 1).then_some(out[0])
    };
    assert_eq!(byte(0x10), Some(0x42), "work RAM");
    assert_eq!(byte(0x20000), Some(ram(&bridge, MEMORY_SAVE_RAM, 0, 1)[0]), "cartridge RAM");
    assert!(byte(0x21FFF).is_some(), "all 8 KB of it");
}

/// Frames per second of each core on each game, after four seconds of warm-up, as the engine runs
/// them (the frame and the engine's per-frame work, no picture drawn). Mesen with its own
/// defaults; bsnes with the options the app gives it (its fast PPU, DSP and coprocessor paths),
/// and then with those three off, its accurate paths, which is what Mesen always runs.
#[test]
fn speed_of_mesen_next_to_bsnes() {
    let (Some(mesen), Some(bsnes)) = (library("MESEN2_SO"), library("BSNES_SO")) else { return };
    let _options = crate::cores::options::test_guard();
    let frames: u32 = std::env::var("SNES_SPEED_FRAMES").ok().and_then(|n| n.parse().ok()).unwrap_or(1800);
    let dir = folder("speed");
    let mut games = vec![("our cartridge".to_string(), write_test_cartridge(&dir))];
    for name in ["240pSuite.sfc", "gothicvania.sfc", "mpyos.sfc"] {
        if let Some(path) = homebrew(name) {
            games.push((name.to_string(), path));
        }
    }
    let accurate = [("bsnes_ppu_fast", "OFF"), ("bsnes_dsp_fast", "OFF"), ("bsnes_coprocessor_delayed_sync", "OFF")];
    for (name, game) in games {
        let mut line = format!("{name}:");
        for (label, id, so) in [("mesen2", "mesen2", &mesen), ("bsnes", "bsnes", &bsnes), ("bsnes accurate", "bsnes", &bsnes)] {
            if label == "bsnes accurate" {
                // Stored the way the Settings screen stores them, so the next load reads them.
                std::fs::create_dir_all(dir.join("options")).unwrap();
                crate::cores::options::set_root(Some(dir.join("options")));
                for (key, value) in accurate {
                    crate::cores::options::set("bsnes", key, value, false).unwrap();
                }
            }
            let mut bridge = engine(id, so, &dir, &game);
            if label == "bsnes accurate" {
                assert_eq!(crate::cores::options::current("bsnes", "bsnes_ppu_fast").as_deref(), Some("OFF"));
            }
            bridge.step_headless_for_test(240).unwrap();
            tap(&mut bridge, Button::Start, 4);
            let start = std::time::Instant::now();
            bridge.step_headless_for_test(frames).unwrap();
            let fps = frames as f64 / start.elapsed().as_secs_f64();
            // The state size too: rewind keeps a fixed number of bytes, not a number of states.
            let state = bridge.save_state().unwrap().len();
            line += &format!(" {label} {fps:.0} fps ({:.1}x full speed, {} KB states);", fps / 60.0988, state / 1024);
            drop(bridge);
            crate::cores::options::set_root(None);
            crate::cores::options::reset_for_tests();
        }
        eprintln!("{line}");
    }
}
