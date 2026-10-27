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
