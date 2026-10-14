//! Opt-in: a host build of BlastEm with Continuum's patches inside a real engine session, for
//! what only the engine's own calls can show: changing Sega CD discs through the same calls the
//! "Next disc" button makes, a save state that remembers its disc, and the save cartridge going to
//! its own file and coming back. A child of `bridge.rs` (like the netplay glue) so it can install
//! a session without a renderer. `BLASTEM_SO=path/to/blastem_libretro.so`, built as
//! `tests/blastem_frontend.rs` says; without it the tests say they were skipped and pass.
//!
//! A Sega CD needs its start-up ROM to run at all, and nobody here has Sega's. These tests give it
//! one of their own instead: a few 68K instructions that write two bytes to the save cartridge and
//! then wait, which is enough for BlastEm to build the machine, run it, and keep the drive and the
//! cartridge going. No game code runs and no real disc is read, so this shows the plumbing, not a
//! game noticing its next disc: that needs a phone with a real start-up ROM and a real game.

use super::{EmulatorBridge, Session};
use crate::cores::{ContentHint, CoreDescriptor, EmulatorCore};
use crate::frame::{FrameGeometry, PixelFormat};
use crate::memory::{MEMORY_SAVE_RAM, MEMORY_SEGACD_RAM_CART, MEMORY_SYSTEM_RAM};
use std::path::{Path, PathBuf};

fn library() -> Option<String> {
    let so = std::env::var("BLASTEM_SO").ok();
    if so.is_none() {
        eprintln!("BLASTEM_SO not set: skipped");
    }
    so
}

fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("continuum-blastem-cd-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The stand-in start-up ROM, 128 KB like the real one: the stack and the start at $200, which
/// enables writing to the save cartridge ($7FFFFF), writes $5A and $C3 to its first two bytes
/// ($600001 and $600003: one byte per odd address), copies the cartridge's size code ($400001) to
/// work RAM, and waits.
fn stand_in_start_up_rom() -> Vec<u8> {
    let mut rom = vec![0u8; 0x2_0000];
    rom[0..4].copy_from_slice(&0x00FF_FE00u32.to_be_bytes());
    rom[4..8].copy_from_slice(&0x0000_0200u32.to_be_bytes());
    let code: [u16; 19] = [
        0x13FC, 0x0001, 0x007F, 0xFFFF, // move.b #1,($7FFFFF).l
        0x13FC, 0x005A, 0x0060, 0x0001, // move.b #$5A,($600001).l
        0x13FC, 0x00C3, 0x0060, 0x0003, // move.b #$C3,($600003).l
        0x1039, 0x0040, 0x0001, //         move.b ($400001).l,d0
        0x13C0, 0x00FF, 0x0000, //         move.b d0,($FF0000).l
        0x60FE, //                         bra.s *
    ];
    for (i, word) in code.iter().enumerate() {
        rom[0x200 + 2 * i..0x202 + 2 * i].copy_from_slice(&word.to_be_bytes());
    }
    rom
}

/// A disc of our own: a data track of `data_sectors` 2048-byte sectors whose first one is a Sega
/// CD header (US region, the byte BlastEm reads it from), and `audio_tracks` short audio tracks,
/// as `<name>.cue` and its `.bin` files. Returns the cue sheet's path.
fn disc(dir: &Path, name: &str, data_sectors: usize, audio_tracks: usize) -> PathBuf {
    let mut data = vec![0u8; data_sectors * 2048];
    data[..16].copy_from_slice(b"SEGADISCSYSTEM  ");
    data[0x20B] = 0x7A;
    std::fs::write(dir.join(format!("{name}.bin")), data).unwrap();
    let mut cue = format!("FILE \"{name}.bin\" BINARY\n  TRACK 01 MODE1/2048\n    INDEX 01 00:00:00\n");
    for t in 0..audio_tracks {
        let file = format!("{name}-audio{t}.bin");
        std::fs::write(dir.join(&file), vec![0u8; 2352 * 75]).unwrap();
        cue += &format!("FILE \"{file}\" BINARY\n  TRACK {:02} AUDIO\n    INDEX 01 00:00:00\n", t + 2);
    }
    let path = dir.join(format!("{name}.cue"));
    std::fs::write(&path, cue).unwrap();
    path
}

/// An engine with BlastEm (its own copy of the library) playing `game`, the folder being the
/// system and save folder as on the phone.
fn engine(so: &str, dir: &Path, game: &Path) -> Result<EmulatorBridge, String> {
    static LOADS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let copy = dir.join(format!("blastem-{}.so", LOADS.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
    std::fs::copy(so, &copy).unwrap();
    let descriptor = CoreDescriptor {
        id: "blastem".into(),
        display_name: "BlastEm".into(),
        systems: vec!["genesis".into(), "segacd".into(), "sega32x".into()],
        geometry: FrameGeometry::new(320, 224, 4.0 / 3.0),
        target_fps: 59.9227,
        audio_sample_rate: 53_267,
        pixel_format: PixelFormat::Xrgb8888,
        module_url: String::new(),
        priority: 0,
    };
    let mut core =
        unsafe { crate::cores::native_core::NativeLibretroCore::load(descriptor, &copy, dir.to_str(), dir.to_str()) }
            .map_err(|e| e.to_string())?;
    let name = game.file_name().unwrap().to_string_lossy().into_owned();
    let mut hint = ContentHint::from_filename(&name);
    hint.full_path = game.to_str().map(str::to_string);
    core.load_content(&[], &hint)
        .map_err(|e| format!("{e} ({})", crate::cores::native_core::last_core_log_line()))?;
    let mut bridge = EmulatorBridge::new();
    bridge.session = Some(Session {
        core_id: "blastem".into(),
        content_id: name.clone(),
        core: Box::new(core),
        paused: false,
        cheats: Vec::new(),
        enabled_codes: 0,
        pokes: Vec::new(),
        search: None,
        search_region: Default::default(),
        content_ext: hint.extension.to_ascii_lowercase(),
        content_name: name,
        battery_layout: Default::default(),
    });
    Ok(bridge)
}

fn log() -> String {
    crate::cores::native_core::last_core_log_line()
}

#[test]
fn a_playlist_opens_its_first_disc_and_next_disc_changes_it() {
    let Some(so) = library() else { return };
    let _options = crate::cores::options::test_guard();
    let dir = folder("discs");
    std::fs::write(dir.join("bios_CD_U.bin"), stand_in_start_up_rom()).unwrap();
    disc(&dir, "First", 150, 0);
    disc(&dir, "Second", 150, 2);
    let playlist = dir.join("Two Discs.m3u");
    // One with a label of its own after a bar, one without (it is named after its file), and the
    // comment and blank lines a playlist can have.
    std::fs::write(&playlist, "#EXTM3U\n\nFirst.cue|Disc One\r\nSecond.cue\n").unwrap();

    let mut bridge = engine(&so, &dir, &playlist).unwrap();
    let status = bridge.disk_status().expect("a disc game offers the disc controls");
    assert_eq!((status.count, status.index, status.ejected), (2, 0, false));
    assert_eq!(status.labels, vec!["Disc One".to_string(), "Second".to_string()]);
    bridge.step_headless_for_test(60).unwrap();

    // "Next disc": tray open, disc 2, tray shut, all in one go, the way the button does it.
    assert_eq!(bridge.swap_disc(), "inserted Second (2 of 2)");
    assert!(log().contains("Second is going in (disc 2 of 2, 3 tracks)"), "{}", log());
    let status = bridge.disk_status().unwrap();
    assert_eq!((status.index, status.ejected), (1, false));
    // The drive reports the tray moving for half a second, then reads the new disc.
    bridge.step_headless_for_test(29).unwrap();
    assert!(log().contains("is going in"), "{}", log());
    bridge.step_headless_for_test(2).unwrap();
    assert!(log().contains("Second is in the drive (disc 2 of 2, 3 tracks)"), "{}", log());

    // A state made on disc 2 says so, first, in a section of its own.
    let on_disc_two = bridge.save_state().unwrap();
    let section = &on_disc_two[std::mem::size_of::<usize>()..];
    assert_eq!(&section[..2], &[0xC0, 0xDE]);
    assert_eq!(&section[6..14], &[0, 0, 0, 1, 0, 0, 0, 2]);
    // Back to disc 1 (it wraps), then the state puts disc 2 back in.
    assert_eq!(bridge.swap_disc(), "inserted Disc One (1 of 2)");
    bridge.step_headless_for_test(40).unwrap();
    assert!(log().contains("Disc One is in the drive (disc 1 of 2, 1 track)"), "{}", log());
    bridge.load_state(&on_disc_two).unwrap();
    assert!(log().contains("Second is back in for the state"), "{}", log());
    assert_eq!(bridge.disk_status().unwrap().index, 1);
    bridge.step_headless_for_test(30).unwrap();
    // And the "Insert disc..." list reaches the same call with a chosen disc.
    assert_eq!(bridge.insert_disc(0), "inserted Disc One (1 of 2)");
    assert_eq!(bridge.insert_disc(2), "there is no disc 3 (the game has 2)");

    // A disc that will not open leaves the one in the drive where it is.
    std::fs::remove_file(dir.join("Second-audio1.bin")).unwrap();
    bridge.step_headless_for_test(30).unwrap();
    let refused = bridge.swap_disc();
    assert!(refused.contains("would not close"), "{refused}");
    assert!(log().contains("Second would not open, so the disc before it stays in"), "{}", log());
    let status = bridge.disk_status().unwrap();
    assert_eq!((status.index, status.ejected), (0, false));
    bridge.step_headless_for_test(40).unwrap();
    assert!(log().contains("Disc One is in the drive"), "{}", log());
    drop(bridge);

    // With BLASTEM_OLD_SO, the build before this patch: it cannot open a playlist, but a state made
    // on disc 2 of one still loads in it on that disc, because BlastEm skips a section it has no
    // reader for, so a phone on an older build is not stuck with a state it cannot read.
    if let Ok(old) = std::env::var("BLASTEM_OLD_SO") {
        std::fs::write(dir.join("Second-audio1.bin"), vec![0u8; 2352 * 75]).unwrap();
        let mut before = engine(&old, &dir, &dir.join("Second.cue")).unwrap();
        before.step_headless_for_test(5).unwrap();
        before.load_state(&on_disc_two).unwrap();
        assert!(log().contains("No handler for section ID 49374"), "{}", log());
        before.step_headless_for_test(30).unwrap();
        eprintln!("the older build loaded the playlist state and ran on");
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn one_disc_offers_the_controls_and_a_cartridge_does_not() {
    let Some(so) = library() else { return };
    let _options = crate::cores::options::test_guard();
    let dir = folder("one");
    std::fs::write(dir.join("bios_CD_U.bin"), stand_in_start_up_rom()).unwrap();
    let cue = disc(&dir, "Only", 150, 1);
    let mut bridge = engine(&so, &dir, &cue).unwrap();
    let status = bridge.disk_status().expect("a single disc offers the controls too, like the PlayStation's");
    assert_eq!((status.count, status.index), (1, 0));
    assert_eq!(bridge.swap_disc(), "this game has 1 disc, nothing to swap to");
    // The state of a one-disc game is BlastEm's own, with no section in front.
    let state = bridge.save_state().unwrap();
    assert_ne!(&state[std::mem::size_of::<usize>()..][..2], &[0xC0, 0xDE]);
    bridge.load_state(&state).unwrap();
    drop(bridge);

    let mut rom = vec![0u8; 0x2_0000];
    rom[0..8].copy_from_slice(&[0x00, 0xFF, 0xFE, 0x00, 0x00, 0x00, 0x02, 0x00]);
    rom[0x100..0x110].copy_from_slice(b"SEGA MEGA DRIVE ");
    rom[0x200..0x202].copy_from_slice(&[0x60, 0xFE]);
    let cart = dir.join("Cartridge.md");
    std::fs::write(&cart, rom).unwrap();
    let bridge = engine(&so, &dir, &cart).unwrap();
    assert!(bridge.disk_status().is_none());

    // A playlist with no disc in it is refused in a sentence, not opened.
    let empty = dir.join("Empty.m3u");
    std::fs::write(&empty, "#EXTM3U\n# nothing here\n").unwrap();
    let err = engine(&so, &dir, &empty).err().expect("an empty playlist does not load");
    assert!(err.contains("doesn't name any discs"), "{err}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_save_cartridge_is_formatted_saved_and_loaded_back() {
    let Some(so) = library() else { return };
    let _options = crate::cores::options::test_guard();
    let dir = folder("cart");
    std::fs::write(dir.join("bios_CD_U.bin"), stand_in_start_up_rom()).unwrap();
    let cue = disc(&dir, "Saver", 150, 0);
    let srm = dir.join("BatterySaves").join("Saver.cue.srm");
    let srm = srm.to_str().unwrap().to_string();
    let bcr = dir.join("BatterySaves").join("Saver.cue.bcr");

    let mut bridge = engine(&so, &dir, &cue).unwrap();
    // The cartridge BlastEm offers: 128 KB, formatted the way the console formats one, so a game
    // finds an empty cartridge rather than one it has to format.
    let size = bridge.memory_size(MEMORY_SEGACD_RAM_CART);
    assert_eq!(size, 128 * 1024);
    let cart = bridge.read_memory(MEMORY_SEGACD_RAM_CART, 0, size as u64).unwrap();
    assert_eq!(&cart[size - 16..], b"RAM_CARTRIDGE___");
    assert!(cart[..size - 0x40].iter().all(|b| *b == 0));
    // The console's own backup RAM is still the 8 KB save, formatted the same way.
    assert_eq!(bridge.memory_size(MEMORY_SAVE_RAM), 8 * 1024);
    assert_eq!(bridge.load_battery_save_file(&srm).unwrap(), None);

    // The stand-in start-up ROM writes to the cartridge through the console's own bus.
    bridge.step_headless_for_test(10).unwrap();
    let cart = bridge.read_memory(MEMORY_SEGACD_RAM_CART, 0, size as u64).unwrap();
    assert_eq!(&cart[..2], &[0x5A, 0xC3]);
    // And reads its size code, 4 for 128 KB.
    assert_eq!(bridge.read_memory(MEMORY_SYSTEM_RAM, 0, 2).unwrap()[1], 4);
    // One more byte from the app's side, as a game save would be.
    bridge.write_memory(MEMORY_SEGACD_RAM_CART, 2, &[0x77]).unwrap();
    assert_eq!(bridge.persist_battery_save(&srm).unwrap(), 8 * 1024 + 128 * 1024);
    let on_disk = std::fs::read(&bcr).unwrap();
    assert_eq!(on_disk.len(), 128 * 1024);
    assert_eq!(&on_disk[..3], &[0x5A, 0xC3, 0x77]);
    assert_eq!(std::fs::read(&srm).unwrap().len(), 8 * 1024);
    // Unchanged: nothing written again.
    assert_eq!(bridge.persist_battery_save(&srm).unwrap(), 0);
    drop(bridge);

    // The next launch gets it back before the first frame.
    let mut next = engine(&so, &dir, &cue).unwrap();
    assert_eq!(next.read_memory(MEMORY_SEGACD_RAM_CART, 0, 3).unwrap(), vec![0, 0, 0]);
    assert_eq!(next.load_battery_save_file(&srm).unwrap(), Some(8 * 1024 + 128 * 1024));
    assert_eq!(next.read_memory(MEMORY_SEGACD_RAM_CART, 0, 3).unwrap(), vec![0x5A, 0xC3, 0x77]);
    let _ = std::fs::remove_dir_all(dir);
}
