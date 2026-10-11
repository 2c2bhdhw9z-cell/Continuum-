//! Opt-in: loads the cBoy core as a shared library through the real frontend
//! (NativeLibretroCore) and runs a game. CBOY_SO=path/to/libcontinuum_gb.so CBOY_ROM=game.gbc
//! CBOY_OUT=/tmp/dir. Does nothing without them.
#![cfg(feature = "native-core")]

use emulator_bridge::cores::native_core::NativeLibretroCore;
use emulator_bridge::cores::{ContentHint, CoreDescriptor, EmulatorCore};
use emulator_bridge::frame::{FrameGeometry, PixelFormat};

#[test]
fn cboy_through_the_frontend() {
    let (Ok(so), Ok(rom), Ok(out)) =
        (std::env::var("CBOY_SO"), std::env::var("CBOY_ROM"), std::env::var("CBOY_OUT")) else { return };
    std::fs::create_dir_all(&out).unwrap();
    let desc = CoreDescriptor {
        id: "continuum_gb".into(),
        display_name: "cBoy".into(),
        systems: vec!["gb".into(), "gbc".into(), "gba".into()],
        geometry: FrameGeometry { base_width: 240, base_height: 160, max_width: 240, max_height: 160, aspect_ratio: 1.5 },
        target_fps: 59.7275,
        audio_sample_rate: 48000,
        pixel_format: PixelFormat::Xrgb8888,
        module_url: so.clone(),
        priority: 0,
    };
    let mut core = unsafe { NativeLibretroCore::load(desc, std::path::Path::new(&so), Some(&out), Some(&out)) }.unwrap();
    let mut hint = ContentHint::from_filename(&rom);
    hint.full_path = Some(rom.clone());
    core.load_content(&[], &hint).unwrap();
    let input = emulator_bridge::input::InputState::default().snapshot();
    let frames: u32 = std::env::var("CBOY_FRAMES").ok().and_then(|v| v.parse().ok()).unwrap_or(1500);
    for f in 0..frames {
        core.run_frame(&input).unwrap();
        if f % 50 == 0 {
            if let Some(v) = core.video() {
                let mut px = Vec::new();
                for y in 0..v.height as usize {
                    for x in 0..v.width as usize {
                        let o = y * v.stride_bytes + x * 4;
                        px.extend_from_slice(&[v.data[o + 2], v.data[o + 1], v.data[o]]);
                    }
                }
                let mut ppm = format!("P6 {} {} 255\n", v.width, v.height).into_bytes();
                ppm.extend(px);
                std::fs::write(format!("{out}/f{f:05}.ppm"), ppm).unwrap();
            }
        }
    }
    eprintln!("last core log: {}", emulator_bridge::cores::native_core::last_core_log_line());
}
