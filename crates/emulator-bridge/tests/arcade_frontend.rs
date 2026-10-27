//! Opt-in: loads cArcade (cores/continuum-arcade, Continuum's own arcade emulator) as a shared
//! library through the real frontend (NativeLibretroCore), the way the app opens an arcade game:
//! a zip handed over by its path. CARCADE_SO=path/to/libcontinuum_arcade.so. Does nothing without
//! it, like tests/cboy_frontend.rs.
//!
//! The game is one of cArcade's own test programs (cores/continuum-arcade/test-programs/pacman)
//! zipped under the file names of MAME's pacman set, and every frame's picture must hash to what
//! MAME 0.289 drew for it (cores/continuum-arcade/tests/references/pacman/video.txt). Then: a game
//! it can't play is refused with the sentence the player reads, a save state round-trips, and its
//! DIP switches are settings the engine knows.
#![cfg(feature = "native-core")]

use std::path::{Path, PathBuf};

use emulator_bridge::cores::native_core::NativeLibretroCore;
use emulator_bridge::cores::{ContentHint, CoreDescriptor, EmulatorCore};
use emulator_bridge::error::BridgeError;
use emulator_bridge::frame::{FrameGeometry, PixelFormat};

fn arcade_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../cores/continuum-arcade")
}

fn program(name: &str) -> Vec<u8> {
    let p = arcade_dir().join("test-programs/pacman").join(name);
    std::fs::read(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

fn crc32(data: &[u8]) -> u32 {
    let mut c = !0u32;
    for &b in data {
        c ^= u32::from(b);
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
        }
    }
    !c
}

/// A plain zip of stored files.
fn zip(files: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let (mut out, mut dir) = (Vec::new(), Vec::new());
    for (name, data) in files {
        let at = out.len() as u32;
        let head = |central: bool| {
            let mut h = Vec::new();
            h.extend(if central { 0x0201_4B50u32 } else { 0x0403_4B50 }.to_le_bytes());
            if central {
                h.extend(20u16.to_le_bytes());
            }
            h.extend([20, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
            h.extend(crc32(data).to_le_bytes());
            h.extend((data.len() as u32).to_le_bytes());
            h.extend((data.len() as u32).to_le_bytes());
            h.extend((name.len() as u16).to_le_bytes());
            h.extend([0, 0]);
            if central {
                h.extend([0u8; 10]);
                h.extend(at.to_le_bytes());
            }
            h.extend(name.as_bytes());
            h
        };
        out.extend(head(false));
        out.extend(data);
        dir.extend(head(true));
    }
    let dir_at = out.len() as u32;
    out.extend(&dir);
    out.extend(0x0605_4B50u32.to_le_bytes());
    out.extend([0, 0, 0, 0]);
    out.extend((files.len() as u16).to_le_bytes());
    out.extend((files.len() as u16).to_le_bytes());
    out.extend((dir.len() as u32).to_le_bytes());
    out.extend(dir_at.to_le_bytes());
    out.extend([0, 0]);
    out
}

fn fnv64(bytes: impl Iterator<Item = u8>) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// MAME's per-frame picture hashes for a test program.
fn reference(name: &str) -> Vec<u64> {
    let p = arcade_dir().join("tests/references/pacman").join(format!("{name}.txt"));
    std::fs::read_to_string(&p)
        .unwrap_or_else(|e| panic!("{}: {e}", p.display()))
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .map(|l| u64::from_str_radix(l.split_whitespace().nth(1).unwrap(), 16).unwrap())
        .collect()
}

fn open(core: &mut NativeLibretroCore, path: &Path) -> Result<(), BridgeError> {
    let file = path.to_str().unwrap();
    let mut hint = ContentHint::from_filename(file);
    hint.full_path = Some(file.to_string());
    // As the app does: no bytes, the path; the engine reads the zip (need_fullpath is false).
    core.load_content(&[], &hint)
}

#[test]
fn carcade_through_the_frontend() {
    let Ok(so) = std::env::var("CARCADE_SO") else {
        eprintln!("CARCADE_SO not set: the cArcade frontend test was skipped");
        return;
    };
    let dir = std::env::temp_dir().join(format!("carcade-frontend-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let out = dir.to_str().unwrap().to_string();
    let desc = CoreDescriptor {
        id: "continuum_arcade".into(),
        display_name: "cArcade".into(),
        systems: vec!["arcade".into()],
        geometry: FrameGeometry { base_width: 224, base_height: 288, max_width: 224, max_height: 288, aspect_ratio: 0.75 },
        target_fps: 60.606,
        audio_sample_rate: 48000,
        pixel_format: PixelFormat::Xrgb8888,
        module_url: so.clone(),
        priority: 0,
    };
    let mut core = unsafe { NativeLibretroCore::load(desc, Path::new(&so), Some(&out), Some(&out)) }.unwrap();

    // A game it can't play: the core's own sentence comes back as the reason, word for word.
    std::fs::write(dir.join("sf2.zip"), zip(&[("sf2_30a.bin", vec![1; 4096])])).unwrap();
    let plays = "Our arcade emulator can\u{2019}t play this one yet. Right now it plays Pac-Man, Ms. Pac-Man, Galaxian, Scramble, Frogger, Amidar, Jump Bug, Donkey Kong, Donkey Kong Jr. and games on the same boards.";
    match open(&mut core, &dir.join("sf2.zip")) {
        Err(BridgeError::InvalidContent { reason, .. }) => {
            assert_eq!(reason, plays);
            assert_eq!(emulator_bridge::launch_failure::own_line("continuum_arcade", &reason).as_deref(), Some(plays));
        }
        other => panic!("sf2.zip: {other:?}"),
    }

    // Pac-Man's board running cArcade's own video test, zipped as MAME's pacman set.
    let p = program("video.bin");
    let g = program("gfx.bin");
    let files = [
        ("pacman.6e", p[..0x1000].to_vec()),
        ("pacman.6f", p[0x1000..0x2000].to_vec()),
        ("pacman.6h", p[0x2000..0x3000].to_vec()),
        ("pacman.6j", p[0x3000..0x4000].to_vec()),
        ("pacman.5e", g[..0x1000].to_vec()),
        ("pacman.5f", g[0x1000..].to_vec()),
        ("82s123.7f", program("palette.bin")),
        ("82s126.4a", program("lookup.bin")),
        ("82s126.1m", program("wave.bin")),
        ("82s126.3m", program("timing.bin")),
    ];
    std::fs::write(dir.join("pacman.zip"), zip(&files)).unwrap();
    open(&mut core, &dir.join("pacman.zip")).unwrap();
    let want = reference("video");
    let input = emulator_bridge::input::InputState::default().snapshot();
    let mut matched = 0;
    for (f, &hash) in want.iter().enumerate() {
        core.run_frame(&input).unwrap();
        let v = core.video().expect("a picture every frame");
        assert_eq!((v.width, v.height), (224, 288), "upright");
        let pixels = (0..v.height as usize).flat_map(|y| {
            let row = &v.data[y * v.stride_bytes..y * v.stride_bytes + v.width as usize * 4];
            row.chunks_exact(4)
                .flat_map(|px| u32::from_le_bytes([px[0], px[1], px[2], 0]).to_le_bytes())
                .collect::<Vec<u8>>()
        });
        assert_eq!(fnv64(pixels), hash, "frame {} isn't the picture MAME drew", f + 1);
        matched += 1;
    }
    eprintln!("cArcade through the frontend: all {matched} frames match MAME's");

    // A save state round-trips through the engine.
    let mut buf = vec![0u8; core.state_size()];
    let n = core.save_state(&mut buf).unwrap();
    core.load_state(&buf[..n]).unwrap();
    core.run_frame(&input).unwrap();

    // Its DIP switches are settings the engine knows, at the factory settings.
    assert_eq!(emulator_bridge::cores::options::current("continuum_arcade", "carcade_pacman_lives").as_deref(), Some("3"));
    assert_eq!(emulator_bridge::cores::options::current("continuum_arcade", "carcade_pacman_cabinet").as_deref(), Some("upright"));
    let _ = std::fs::remove_dir_all(&dir);
}
