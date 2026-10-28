//! Opt-in: loads cAtari (cores/continuum-atari, Continuum's own Atari 7800 emulator) as a shared
//! library through the real frontend (NativeLibretroCore), the way the app opens a game.
//! CATARI_SO=path/to/libcontinuum_atari.so. Does nothing without it, like tests/cgear_frontend.rs.
//!
//! The game is cAtari's own test program (cores/continuum-atari/test-programs/zones.asm) in an
//! .a78 header. Checked: the picture's size and that it draws, a save state that round-trips, the
//! settings the engine reads off the core, and the RAM the core hands over.
#![cfg(feature = "native-core")]

use std::path::Path;

use emulator_bridge::cores::native_core::NativeLibretroCore;
use emulator_bridge::cores::{ContentHint, CoreDescriptor, EmulatorCore};
use emulator_bridge::frame::{FrameGeometry, PixelFormat};

/// A libretro core is one per process, so the tests take turns with it.
static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn cartridge() -> Vec<u8> {
    let rom = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../../cores/continuum-atari/test-programs/zones.bin")).unwrap();
    let mut h = vec![0u8; 128];
    h[0] = 3;
    h[1..10].copy_from_slice(b"ATARI7800");
    h[49..53].copy_from_slice(&(rom.len() as u32).to_be_bytes());
    h[55] = 1;
    h[56] = 1;
    h[100..128].copy_from_slice(b"ACTUAL CART DATA STARTS HERE");
    h.extend(rom);
    h
}

#[test]
fn catari_through_the_frontend() {
    let Ok(so) = std::env::var("CATARI_SO") else {
        eprintln!("CATARI_SO not set: the cAtari frontend test was skipped");
        return;
    };
    let _turn = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let dir = std::env::temp_dir().join(format!("catari-frontend-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let out = dir.to_str().unwrap().to_string();
    let desc = CoreDescriptor {
        id: "continuum_atari".into(),
        display_name: "cAtari".into(),
        systems: vec!["atari7800".into()],
        geometry: FrameGeometry { base_width: 320, base_height: 224, max_width: 320, max_height: 292, aspect_ratio: 4.0 / 3.0 },
        target_fps: 59.96,
        audio_sample_rate: 48000,
        pixel_format: PixelFormat::Xrgb8888,
        module_url: so.clone(),
        priority: 0,
    };
    let mut core = unsafe { NativeLibretroCore::load(desc, Path::new(&so), Some(&out), Some(&out)) }.unwrap();
    core.load_content(&cartridge(), &ContentHint::from_filename("zones.a78")).unwrap();
    let input = emulator_bridge::input::InputState::default().snapshot();
    for _ in 0..30 {
        core.run_frame(&input).unwrap();
    }
    let v = core.video().expect("a picture");
    assert_eq!((v.width, v.height), (320, 224));
    let mut colours: Vec<u32> = (0..v.height as usize)
        .flat_map(|y| (0..v.width as usize).map(move |x| (x, y)))
        .map(|(x, y)| {
            let at = y * v.stride_bytes + x * 4;
            u32::from_le_bytes([v.data[at], v.data[at + 1], v.data[at + 2], 0])
        })
        .collect();
    colours.sort_unstable();
    colours.dedup();
    assert!(colours.len() >= 5, "the test program's objects are drawn ({} colours)", colours.len());
    let mut buf = vec![0u8; core.state_size()];
    let n = core.save_state(&mut buf).unwrap();
    core.load_state(&buf[..n]).unwrap();
    core.run_frame(&input).unwrap();
    assert_eq!(emulator_bridge::cores::options::current("continuum_atari", "catari_region").as_deref(), Some("auto"));
    assert_eq!(emulator_bridge::cores::options::current("continuum_atari", "catari_palette").as_deref(), Some("atari"));
    let _ = std::fs::remove_dir_all(&dir);
}

/// Rikki & Vikki's way of coming: a ProSystem description (.cdf) naming its ROM and its music
/// files in folders, with Windows paths and capitals that don't match, while the app keeps
/// every imported file side by side in one folder. The game is a stand-in that asks the Souper
/// board's music player for song 0; the music must come out of the engine's sound.
#[test]
fn a_cdf_finds_its_music_beside_it_through_the_frontend() {
    let Ok(so) = std::env::var("CATARI_SO") else {
        eprintln!("CATARI_SO not set: the cAtari frontend test was skipped");
        return;
    };
    let _turn = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let dir = std::env::temp_dir().join(format!("catari-cdf-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // The game: reset the music player, play song 0, wait.
    let mut rom = vec![0u8; 32 * 0x4000];
    let last = 31 * 0x4000;
    let program = [0xA9, 0x00, 0x8D, 0x07, 0x80, 0xA9, 0x80, 0x8D, 0x07, 0x80, 0x4C, 0x0A, 0xC0];
    rom[last..last + program.len()].copy_from_slice(&program);
    rom[last + 0x3FFC..last + 0x3FFE].copy_from_slice(&[0x00, 0xC0]);
    // One square-wave sample, an instrument that loops it, a song that holds one note.
    let mut smp = b"CSMP".to_vec();
    for w in [1u32, 24, 8, 8000 << 16, 1000 << 16] {
        smp.extend(w.to_le_bytes());
    }
    smp.extend([100, 100, 100, 100, 156, 156, 156, 156]);
    let mut ins = b"CINS".to_vec();
    for w in [1u32, 0, 20, 0] {
        ins.extend(w.to_le_bytes());
    }
    ins.extend([3, 127, 0, 0, 2, 0, 0, 8, 0, 0xFF, 0xFF, 0xFF, 0x7F]);
    let mut mus = b"CMUS".to_vec();
    mus.extend(1u32.to_le_bytes());
    mus.push(10);
    mus.extend(13u32.to_le_bytes());
    mus.extend([3, 69, 0xFF, 0xFF, 0xFF, 0x7F]);
    for (name, data) in [("Game.bin", rom), ("rv_samples.smp", smp), ("RV_Instruments.ins", ins), ("Song 0.mus", mus)] {
        std::fs::write(dir.join(name), data).unwrap();
    }
    let cdf = "ProSystem\r\nSOUPER\r\nRikki stand-in\r\nRnV\\Game.bin\r\nCORETONE\r\nMusic\\RV_Samples.smp\r\n\
               Music\\RV_Instruments.ins\r\nMusic\\Song 0.mus\r\n";
    let cdf_path = dir.join("Rikki stand-in.cdf");
    std::fs::write(&cdf_path, cdf).unwrap();

    let out = dir.to_str().unwrap().to_string();
    let desc = CoreDescriptor {
        id: "continuum_atari".into(),
        display_name: "cAtari".into(),
        systems: vec!["atari7800".into()],
        geometry: FrameGeometry { base_width: 320, base_height: 224, max_width: 320, max_height: 292, aspect_ratio: 4.0 / 3.0 },
        target_fps: 59.96,
        audio_sample_rate: 48000,
        pixel_format: PixelFormat::Xrgb8888,
        module_url: so.clone(),
        priority: 0,
    };
    let mut core = unsafe { NativeLibretroCore::load(desc, Path::new(&so), Some(&out), Some(&out)) }.unwrap();
    // As the app opens a game: its path, no bytes.
    core.load_content(&[], &ContentHint::from_filename(cdf_path.to_str().unwrap())).unwrap();
    let input = emulator_bridge::input::InputState::default().snapshot();
    let mut sink = emulator_bridge::audio::RingAudioSink::new(48_000, 48_000, 59.96, 8);
    let mut loudest = 0f32;
    let mut buf = vec![0f32; 48_000];
    for _ in 0..30 {
        core.run_frame(&input).unwrap();
        core.drain_audio(&mut sink);
        let n = emulator_bridge::audio::AudioSink::drain(&mut sink, &mut buf);
        loudest = buf[..n].iter().fold(loudest, |m, v| m.max(v.abs()));
    }
    assert!(loudest > 0.05, "the music plays (loudest {loudest})");
    let _ = std::fs::remove_dir_all(&dir);
}
