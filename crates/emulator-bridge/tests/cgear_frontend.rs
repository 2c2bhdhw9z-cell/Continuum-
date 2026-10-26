//! Opt-in: loads cGear (cores/continuum-sms, Continuum's own Sega 8-bit emulator) as a shared
//! library through the real frontend (NativeLibretroCore), the way the app opens a game.
//! CGEAR_SO=path/to/libcontinuum_sms.so. Does nothing without it, like tests/cboy_frontend.rs.
//!
//! The game is a small cartridge built here: it turns the screen on with a blue background and
//! loops. Checked: the picture's size and colour, a save state that
//! round-trips, the settings the engine reads off the core, and a Game Gear file opening at the
//! Game Gear's own size.
#![cfg(feature = "native-core")]

use std::path::Path;

use emulator_bridge::cores::native_core::NativeLibretroCore;
use emulator_bridge::cores::{ContentHint, CoreDescriptor, EmulatorCore};
use emulator_bridge::frame::{FrameGeometry, PixelFormat};

/// Colour 0 blue, the screen on, then a loop.
fn cartridge() -> Vec<u8> {
    let code: &[u8] = &[
        0xF3, 0x3E, 0x00, 0xD3, 0xBF, 0x3E, 0xC0, 0xD3, 0xBF, // colour memory address 0
        0x3E, 0x30, 0xD3, 0xBE, // blue
        0x3E, 0x40, 0xD3, 0xBF, 0x3E, 0x81, 0xD3, 0xBF, // register 1 = $40: screen on
        0x18, 0xFE,
    ];
    let mut rom = vec![0u8; 0x8000];
    rom[..code.len()].copy_from_slice(code);
    rom
}

#[test]
fn cgear_through_the_frontend() {
    let Ok(so) = std::env::var("CGEAR_SO") else {
        eprintln!("CGEAR_SO not set: the cGear frontend test was skipped");
        return;
    };
    let dir = std::env::temp_dir().join(format!("cgear-frontend-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let out = dir.to_str().unwrap().to_string();
    let desc = CoreDescriptor {
        id: "continuum_sms".into(),
        display_name: "cGear".into(),
        systems: vec!["sms".into(), "gg".into(), "sg1000".into()],
        geometry: FrameGeometry { base_width: 256, base_height: 192, max_width: 256, max_height: 240, aspect_ratio: 4.0 / 3.0 },
        target_fps: 59.92,
        audio_sample_rate: 48000,
        pixel_format: PixelFormat::Xrgb8888,
        module_url: so.clone(),
        priority: 0,
    };
    let mut core = unsafe { NativeLibretroCore::load(desc.clone(), Path::new(&so), Some(&out), Some(&out)) }.unwrap();
    let rom = cartridge();
    core.load_content(&rom, &ContentHint::from_filename("blue.sms")).unwrap();
    let input = emulator_bridge::input::InputState::default().snapshot();
    for _ in 0..30 {
        core.run_frame(&input).unwrap();
    }
    let v = core.video().expect("a picture");
    assert_eq!((v.width, v.height), (256, 192));
    let px = |x: usize, y: usize| {
        let at = y * v.stride_bytes + x * 4;
        u32::from_le_bytes([v.data[at], v.data[at + 1], v.data[at + 2], 0])
    };
    assert_eq!(px(100, 100), 0x0000FF, "blue");
    let mut buf = vec![0u8; core.state_size()];
    let n = core.save_state(&mut buf).unwrap();
    core.load_state(&buf[..n]).unwrap();
    core.run_frame(&input).unwrap();
    assert_eq!(emulator_bridge::cores::options::current("continuum_sms", "cgear_console").as_deref(), Some("auto"));
    assert_eq!(emulator_bridge::cores::options::current("continuum_sms", "cgear_sprite_limit").as_deref(), Some("on"));
    drop(core);

    // The same cartridge as a Game Gear file: the Game Gear's own screen.
    let mut core = unsafe { NativeLibretroCore::load(desc, Path::new(&so), Some(&out), Some(&out)) }.unwrap();
    // As the app opens a game: by its path.
    let gg = dir.join("blue.gg");
    std::fs::write(&gg, &rom).unwrap();
    let mut hint = ContentHint::from_filename(gg.to_str().unwrap());
    hint.full_path = Some(gg.to_str().unwrap().to_string());
    core.load_content(&[], &hint).unwrap();
    core.run_frame(&input).unwrap();
    let v = core.video().expect("a picture");
    assert_eq!((v.width, v.height), (160, 144));
    let _ = std::fs::remove_dir_all(&dir);
}
