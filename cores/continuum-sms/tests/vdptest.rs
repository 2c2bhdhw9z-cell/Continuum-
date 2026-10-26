//! FluBBa's SMS VDP test (VDPTest 1.31, from emudev-org/emudev-hwtests, fetched by
//! scripts/fetch-test-roms.sh): the picture chip's ports, counters and interrupt timings, each
//! test reporting on screen. VDPTEST_SHOTS=<folder> writes the screens as PNGs, VDPTEST_KEYS a
//! list of frame:button presses (buttons as the pad's port bits: 1 up 2 down 4 left 8 right 16
//! button 1 32 button 2). Measurement aid; the pinned results come once the screens are read.

use std::path::PathBuf;

use continuum_sms::sms::Sms;

fn shot(m: &Sms, path: &str) {
    let (w, h) = (256usize, 192usize);
    let mut raw = Vec::with_capacity(w * h * 3);
    for p in &m.bus.vdp.frame[..w * h] {
        raw.extend([(p >> 16) as u8, (p >> 8) as u8, *p as u8]);
    }
    let file = std::fs::File::create(path).unwrap();
    let mut enc = png::Encoder::new(file, w as u32, h as u32);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().unwrap().write_image_data(&raw).unwrap();
}

/// The name table as text: each tile's number, less `VDPTEST_FONT` (the tile of a space), as a
/// character, with its palette bit as a capital-or-not hint lost. Enough to read a test's report.
fn screen_text(m: &Sms) -> String {
    let v = &m.bus.vdp;
    let names = usize::from(v.regs[2] & 0x0E) << 10;
    let base: i32 = std::env::var("VDPTEST_FONT").ok().and_then(|x| x.parse().ok()).unwrap_or(0);
    let mut out = String::new();
    for row in 0..24 {
        for col in 0..32 {
            let at = names + (row * 32 + col) * 2;
            let tile = i32::from(v.vram[at]) | i32::from(v.vram[at + 1] & 1) << 8;
            let c = tile - base + 32;
            out.push(if (32..127).contains(&c) { char::from(c as u8) } else { '.' });
        }
        out.push('\n');
    }
    out
}

#[test]
fn vdptest_screens() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test-roms/VDPTest.sms");
    let Ok(rom) = std::fs::read(&path) else {
        eprintln!("SKIPPED: {} not found", path.display());
        return;
    };
    let Ok(dir) = std::env::var("VDPTEST_SHOTS") else { return };
    let keys: Vec<(u64, u8)> = std::env::var("VDPTEST_KEYS").unwrap_or_default().split(',').filter(|s| !s.is_empty())
        .map(|k| { let (f, b) = k.split_once(':').unwrap(); (f.parse().unwrap(), b.parse().unwrap()) }).collect();
    let mut m = Sms::new(rom);
    let total: u64 = std::env::var("VDPTEST_FRAMES").ok().and_then(|v| v.parse().ok()).unwrap_or(600);
    for f in 0..total {
        let held = keys.iter().filter(|(at, _)| f >= *at && f < at + 4).fold(0u8, |a, (_, b)| a | b);
        m.bus.pad_dc = !held;
        m.run_frame();
        if f % 120 == 119 || f + 1 == total {
            shot(&m, &format!("{dir}/f{f:05}.png"));
        }
        if f + 1 == total {
            eprintln!("{}", screen_text(&m));
        }
    }
}

/// The pages of the test as a pad would walk them: button 1 on the menu, then once a page.
fn pages(rom: Vec<u8>) -> Vec<String> {
    let mut m = Sms::new(rom);
    let presses = [300u64, 1500, 2700];
    let mut out = Vec::new();
    for f in 0..3900u64 {
        let held = presses.iter().any(|at| f >= *at && f < at + 4);
        m.bus.pad_dc = if held { !16 } else { 0xFF };
        m.run_frame();
        if presses.contains(&(f + 1)) && f > 1000 || f + 1 == 3900 {
            out.push(screen_text(&m));
        }
    }
    out
}

/// What cGear passes, pinned so it only ever grows. The rest of the misc and sprite pages is the
/// H counter and the timing of events inside a line, which cGear does not model yet
/// (.kiro/notes/tasks/cboy-finish.json S1).
#[test]
fn vdptest_results() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test-roms/VDPTest.sms");
    let Ok(rom) = std::fs::read(&path) else {
        eprintln!("SKIPPED: {} not found (run scripts/fetch-test-roms.sh)", path.display());
        return;
    };
    let pages = pages(rom);
    let all: String = pages.concat();
    eprintln!("{all}");
    let must = [
        "Normal read/write:", "Read after code1 wr:", "Read after code2 wr:", "Read after code3 wr:",
        "Write after code0 wr:", "Write after code2 wr:", "Write after code3 wr:", "Mixed read/write:",
        "VRAM wr set VDPbuffer:", "CRAM wr set VDPbuffer:", "1byte wr sets rd adr:", "1byte wr sets wr adr:",
        "1byte wr keeps wr mode:", "Address wraps at $3FFF:", "VRAM & CRAM share adr:", "Rd VDPCtrl reset latch:",
        "Rd VDPData reset latch:", "Wr VDPData reset latch:", "Unused regs, no effect:",
        "VCounter values: NTSC", "HCounter keeps value:", "VDP Register mirrors:", "VDP data mirrors:",
        "VDP ctrl mirrors:", "VCounter mirrors:", "Frame IRQ VCount:", "Line IRQ VCount:", "VINT flag keept:",
        "No sprite collision:", "No disp, no spr col:", "9th sprite no col:", "Transp pixl, no col:",
        "Offscreen X, no col:", "Sprite collision:", "Colflag keept in vbl:", "Spr col behind tile:",
        "Spr col correct line:", "Spr col many lines:", "No sprite overflow:", "Offscreen Y, no ovr:",
        "Sprite overflow:", "Ovrflag keept in vbl:", "Spr ovr correct line:", "Spr ovr many lines:",
    ];
    for name in must {
        let line = all.lines().find(|l| l.contains(name)).unwrap_or_else(|| panic!("{name} not on any page"));
        assert!(line.contains("Ok."), "{line}");
    }
}
