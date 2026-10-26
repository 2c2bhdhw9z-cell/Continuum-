//! Free homebrew games run headless, as a check that real programs boot and draw (the games are
//! never committed; HOMEBREW_DIR=<folder of .sms/.gg/.sg files> runs them, for example the
//! retrobrews/sms-games collection). Each runs 30 seconds with Start and button 1 pressed now and
//! then; a game that panics fails the test. HOMEBREW_OUT=<file.png> writes every game's last
//! picture side by side, to look at.

use std::path::PathBuf;

use continuum_sms::sms::Sms;

const FRAMES: u64 = 1800;
const COLS: usize = 8;

#[test]
fn homebrew_games_boot_and_draw() {
    let Ok(dir) = std::env::var("HOMEBREW_DIR") else {
        eprintln!("SKIPPED: set HOMEBREW_DIR to a folder of homebrew games");
        return;
    };
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir).unwrap().filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "sms" || x == "gg" || x == "sg")).collect();
    files.sort();
    // HOMEBREW_ONLY=<part of a name>: just those, with a picture every two seconds on the sheet.
    let only = std::env::var("HOMEBREW_ONLY").ok();
    if let Some(o) = &only {
        files.retain(|p| p.to_string_lossy().contains(o.as_str()));
    }
    let mut shots: Vec<(String, Vec<u32>)> = Vec::new();
    let mut failed = Vec::new();
    for path in &files {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let rom = std::fs::read(path).unwrap();
        let ext = path.extension().unwrap().to_string_lossy().into_owned();
        let only = only.clone();
        let run = std::panic::catch_unwind(move || {
            let mut frames: Vec<Vec<u32>> = Vec::new();
            let mut m = Sms::new(rom);
            match ext.as_str() {
                "gg" => m.bus.set_game_gear(),
                "sg" => m.bus.set_sg1000(),
                _ => {}
            }
            for f in 0..FRAMES {
                // Button 1 for a moment every two seconds, Pause (Start) now and then.
                let press = f % 120 >= 100 && f % 120 < 106;
                m.bus.pad_dc = if press { !0x10 } else { 0xFF };
                m.bus.gg_start = f % 600 >= 300 && f % 600 < 306;
                if !m.bus.gg {
                    m.set_pause(f % 900 == 450);
                }
                m.run_frame();
                m.samples.clear();
                if only.is_some() && f % 120 == 119 && f + 1 < FRAMES {
                    let mut out = Vec::new();
                    m.bus.vdp.shown(&mut out);
                    let w = if m.bus.gg { 160 } else { 256 };
                    let mut cell = vec![0u32; 256 * 192];
                    for y in 0..192.min(out.len() / w) {
                        for x in 0..w {
                            cell[y * 256 + x] = out[y * w + x];
                        }
                    }
                    frames.push(cell);
                }
            }
            let mut out = Vec::new();
            m.bus.vdp.shown(&mut out);
            let (w, h) = if m.bus.gg { (160, 144) } else { (256, out.len() / 256) };
            // Every picture as 256 x 192 for the sheet.
            let mut cell = vec![0u32; 256 * 192];
            for y in 0..192.min(h) {
                for x in 0..256.min(w) {
                    cell[y * 256 + x] = out[y * w + x];
                }
            }
            let colours = cell.iter().collect::<std::collections::HashSet<_>>().len();
            frames.push(cell);
            (frames, colours)
        });
        match run {
            Ok((frames, colours)) => {
                eprintln!("{name}: {colours} colours on screen");
                for cell in frames {
                    shots.push((name.clone(), cell));
                }
            }
            Err(_) => failed.push(name),
        }
    }
    if let Ok(out) = std::env::var("HOMEBREW_OUT") {
        let rows = shots.len().div_ceil(COLS);
        let (w, h) = (COLS * 256, rows * 192);
        let mut raw = vec![0u8; w * h * 3];
        for (i, (_, cell)) in shots.iter().enumerate() {
            let (ox, oy) = ((i % COLS) * 256, (i / COLS) * 192);
            for y in 0..192 {
                for x in 0..256 {
                    let p = cell[y * 256 + x];
                    let at = ((oy + y) * w + ox + x) * 3;
                    raw[at..at + 3].copy_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, p as u8]);
                }
            }
        }
        let file = std::fs::File::create(&out).unwrap();
        let mut enc = png::Encoder::new(file, w as u32, h as u32);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header().unwrap().write_image_data(&raw).unwrap();
        for (i, (n, _)) in shots.iter().enumerate() {
            eprintln!("sheet {}: {n}", i);
        }
    }
    assert!(failed.is_empty(), "these panicked: {failed:?}");
}
