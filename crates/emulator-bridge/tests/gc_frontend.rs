//! Opt-in: loads Continuum GC (cores/continuum-gc, Continuum's own GameCube emulator) as a shared
//! library through the real frontend (NativeLibretroCore), the way the app opens a game, and
//! drives its settings the way the Core settings screen does. CGC_SO=path/to/libcontinuum_gc.so.
//! Does nothing without it, like tests/arcade_frontend.rs.
#![cfg(feature = "native-core")]

use std::path::Path;

use emulator_bridge::cores::native_core::NativeLibretroCore;
use emulator_bridge::cores::{ContentHint, CoreDescriptor, EmulatorCore};
use emulator_bridge::frame::{FrameGeometry, PixelFormat};

/// A program of our own: the display on (640 wide, 240 lines), the framebuffer grey, then wait.
fn program() -> Vec<u8> {
    let words: Vec<u32> = vec![
        0x3C60_CC00, // lis r3, 0xCC00
        0x6063_2000, // ori r3, r3, 0x2000
        0x3880_0F06, // li r4, (240 << 4) | 6
        0xB083_0000, // sth r4, 0(r3)
        0x3880_2850, // li r4, (40 << 8) | 80
        0xB083_0048, // sth r4, 0x48(r3)
        0x3880_0005, // li r4, 5
        0xB083_0002, // sth r4, 2(r3)
        0x4800_0000, // b .
    ];
    let mut dol = vec![0u8; 0x100];
    dol[0..4].copy_from_slice(&0x100u32.to_be_bytes());
    dol[0x48..0x4C].copy_from_slice(&0x8000_3100u32.to_be_bytes());
    dol[0x90..0x94].copy_from_slice(&((words.len() * 4) as u32).to_be_bytes());
    dol[0xE0..0xE4].copy_from_slice(&0x8000_3100u32.to_be_bytes());
    dol.extend(words.iter().flat_map(|w| w.to_be_bytes()));
    dol
}

/// The picture's pixel at (x, y), as 0RGB.
fn pixel(core: &NativeLibretroCore, x: usize, y: usize) -> u32 {
    let v = core.video().expect("a picture");
    let at = y * v.stride_bytes + x * 4;
    u32::from_le_bytes([v.data[at], v.data[at + 1], v.data[at + 2], 0])
}

#[test]
fn continuum_gc_settings_through_the_frontend() {
    let Ok(so) = std::env::var("CGC_SO") else {
        eprintln!("CGC_SO not set: the Continuum GC frontend test was skipped");
        return;
    };
    let dir = std::env::temp_dir().join(format!("cgc-frontend-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let out = dir.to_str().unwrap().to_string();
    let desc = CoreDescriptor {
        id: "continuum_gc".into(),
        display_name: "Continuum GC".into(),
        systems: vec!["gamecube".into()],
        geometry: FrameGeometry {
            base_width: 640,
            base_height: 480,
            max_width: 720,
            max_height: 576,
            aspect_ratio: 4.0 / 3.0,
        },
        target_fps: 59.94,
        audio_sample_rate: 48000,
        pixel_format: PixelFormat::Xrgb8888,
        module_url: so.clone(),
        priority: 0,
    };
    let mut core =
        unsafe { NativeLibretroCore::load(desc, Path::new(&so), Some(&out), Some(&out)) }.unwrap();
    let path = dir.join("test.dol");
    std::fs::write(&path, program()).unwrap();
    let file = path.to_str().unwrap();
    let mut hint = ContentHint::from_filename(file);
    hint.full_path = Some(file.to_string());
    core.load_content(&[], &hint).unwrap();
    let input = emulator_bridge::input::InputState::default().snapshot();
    for _ in 0..3 {
        core.run_frame(&input).unwrap();
    }
    let white = 0x00FF_FFFF;
    assert_ne!(pixel(&core, 6, 4), white, "no read-out by default");
    // The Core settings screen's change, as the app makes it.
    use emulator_bridge::cores::options;
    assert_eq!(
        options::current("continuum_gc", "cgc_speed_readout").as_deref(),
        Some("off")
    );
    options::set("continuum_gc", "cgc_speed_readout", "on", false).expect("the setting takes");
    assert_eq!(
        options::current("continuum_gc", "cgc_speed_readout").as_deref(),
        Some("on")
    );
    for _ in 0..3 {
        core.run_frame(&input).unwrap();
    }
    assert_eq!(pixel(&core, 6, 4), white, "the speed read-out shows");
    options::set("continuum_gc", "cgc_speed_readout", "off", false).unwrap();
    for _ in 0..3 {
        core.run_frame(&input).unwrap();
    }
    assert_ne!(pixel(&core, 6, 4), white, "and goes again");
    let _ = std::fs::remove_dir_all(&dir);
}
