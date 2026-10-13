//! Public test ROMs, run headless. Fetch them first with scripts/fetch-test-roms.sh; a missing
//! ROM is reported and the test is skipped (so a fresh checkout without them still builds).

use std::path::PathBuf;

use continuum_gb::gb::GameBoy;

fn rom(name: &str) -> Option<Vec<u8>> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test-roms").join(name);
    match std::fs::read(&p) {
        Ok(b) => Some(b),
        Err(_) => {
            eprintln!("SKIPPED: {} not found (run scripts/fetch-test-roms.sh)", p.display());
            None
        }
    }
}

/// Runs a Blargg ROM until it prints Passed or Failed on the serial port.
fn blargg(name: &str, max_frames: u32) {
    let Some(data) = rom(name) else { return };
    let mut gb = GameBoy::new(data).unwrap();
    for _ in 0..max_frames {
        gb.run_frame();
        let out = String::from_utf8_lossy(&gb.bus.serial_out).to_string();
        if out.contains("Passed") {
            return;
        }
        if out.contains("Failed") {
            panic!("{name} failed:\n{out}");
        }
    }
    panic!("{name} timed out:\n{}", String::from_utf8_lossy(&gb.bus.serial_out));
}

#[test]
fn blargg_cpu_instrs() {
    blargg("cpu_instrs.gb", 4000);
}

#[test]
fn blargg_instr_timing() {
    blargg("instr_timing.gb", 600);
}

#[test]
fn blargg_mem_timing() {
    blargg("mem_timing.gb", 600);
}

/// Runs a Mooneye ROM until `LD B,B`; a pass leaves the Fibonacci numbers in the registers.
pub fn mooneye(name: &str) -> Result<(), String> {
    mooneye_mode(name, false)
}

pub fn mooneye_mode(name: &str, cgb: bool) -> Result<(), String> {
    mooneye_on(name, cgb, false)
}

/// The two picture drawers (core option "Picture drawing"): false Standard, true Exact.
const DRAWERS: [(bool, &str); 2] = [(false, "standard"), (true, "exact")];

/// `mooneye_mode` on a chosen drawer.
pub fn mooneye_on(name: &str, cgb: bool, exact: bool) -> Result<(), String> {
    let Some(data) = rom(name) else { return Ok(()) };
    let mut gb = GameBoy::new_mode(data, cgb)?;
    gb.bus.ppu.exact = exact;
    for _ in 0..(60 * 30) {
        gb.run_frame();
        if gb.cpu.ld_b_b {
            let c = &gb.cpu;
            let regs = [c.b, c.c, c.d, c.e, c.h, c.l];
            return if regs == [3, 5, 8, 13, 21, 34] {
                Ok(())
            } else {
                Err(format!("{name}: registers {regs:?}"))
            };
        }
        if gb.cpu.locked {
            return Err(format!("{name}: CPU locked at an illegal opcode"));
        }
    }
    Err(format!("{name}: timed out"))
}

/// dmg-acid2 must pass on both drawers.
#[test]
fn dmg_acid2() {
    for (exact, label) in DRAWERS {
        dmg_acid2_on(exact, label);
    }
}

fn dmg_acid2_on(exact: bool, label: &str) {
    let Some(data) = rom("dmg-acid2.gb") else { return };
    let Some(png_bytes) = rom("dmg-acid2-reference.png") else { return };
    let mut gb = GameBoy::new(data).unwrap();
    gb.bus.ppu.exact = exact;
    for _ in 0..120 {
        gb.run_frame();
    }
    let mut decoder = png::Decoder::new(std::io::Cursor::new(png_bytes));
    decoder.set_transformations(png::Transformations::EXPAND);
    let mut reader = decoder.read_info().unwrap();
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).unwrap();
    let bpp = info.line_size / info.width as usize;
    let mut wrong = 0;
    let mut first = None;
    for y in 0..144usize {
        for x in 0..160usize {
            let p = &buf[y * info.line_size + x * bpp..];
            let shade = match p[0] {
                0xFF => 0u8,
                0xAA => 1,
                0x55 => 2,
                _ => 3,
            };
            if gb.bus.ppu.frame[y * 160 + x] != continuum_gb::ppu::SHADES[shade as usize] {
                wrong += 1;
                first.get_or_insert((x, y));
            }
        }
    }
    assert_eq!(wrong, 0, "dmg-acid2 ({label} drawer): {wrong} pixels differ, first at {first:?}");
    eprintln!("DRAWER {label}: dmg-acid2 passed");
}

/// Writes the screen after N frames to $DUMP_PNG (a debugging aid, not a check).
#[test]
fn dump_screen() {
    let (Ok(path), Ok(name)) = (std::env::var("DUMP_PNG"), std::env::var("DUMP_ROM")) else { return };
    let Some(data) = rom(&name) else { return };
    let mut gb = GameBoy::new(data).unwrap();
    for _ in 0..120 {
        gb.run_frame();
    }
    let f = std::fs::File::create(path).unwrap();
    let mut enc = png::Encoder::new(f, 160, 144);
    enc.set_color(png::ColorType::Grayscale);
    let mut w = enc.write_header().unwrap();
    let px: Vec<u8> = gb.bus.ppu.frame.iter().map(|p| (*p >> 8) as u8).collect();
    w.write_image_data(&px).unwrap();
}

/// Every Mooneye acceptance test that applies to a DMG, as a pass count. Not a gate yet: the
/// list of the ones that fail is printed so progress is visible. Run with --nocapture.
#[test]
fn mooneye_acceptance_report() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test-roms/mts");
    if !dir.exists() {
        eprintln!("SKIPPED: Mooneye suite not fetched");
        return;
    }
    let mut names = Vec::new();
    for sub in ["acceptance", "acceptance/bits", "acceptance/instr", "acceptance/interrupts",
                "acceptance/oam_dma", "acceptance/ppu", "acceptance/timer", "emulator-only/mbc1",
                "emulator-only/mbc5"] {
        let Ok(rd) = std::fs::read_dir(dir.join(sub)) else { continue };
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().to_string();
            if !n.ends_with(".gb") {
                continue;
            }
            // Only the DMG ones: skip tests for other models (-S, -C, -A, -mgb, -sgb, -dmg0).
            let stem = n.trim_end_matches(".gb");
            if let Some(tag) = stem.rsplit_once('-').map(|(_, t)| t) {
                if !tag.contains("dmgABC") && !tag.contains('G') && tag.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == 'm' || c == 'g' || c == 'b' || c == 's' || c == 'd') {
                    continue;
                }
            }
            names.push(format!("mts/{sub}/{n}"));
        }
    }
    names.sort();
    // On both drawers: timing never depends on which one draws.
    for (exact, label) in DRAWERS {
        let mut failed = Vec::new();
        for n in &names {
            if let Err(e) = mooneye_on(n, false, exact) {
                failed.push(e);
            }
        }
        eprintln!("MOONEYE ({label} drawer): {} of {} passed", names.len() - failed.len(), names.len());
        for f in &failed {
            eprintln!("  FAIL {f}");
        }
        assert!(failed.is_empty(), "Mooneye DMG acceptance regressed on the {label} drawer: {failed:?}");
    }
    assert_eq!(names.len(), 86, "expected the 86 DMG acceptance tests");
}

/// A save state taken mid-run and loaded into a fresh machine continues identically.
#[test]
fn save_state_round_trip() {
    let Some(data) = rom("cpu_instrs.gb") else { return };
    let mut a = GameBoy::new(data.clone()).unwrap();
    for _ in 0..200 {
        a.run_frame();
    }
    let state = a.save_state();
    let mut b = GameBoy::new(data).unwrap();
    b.load_state(&state).unwrap();
    for _ in 0..300 {
        a.run_frame();
        b.run_frame();
    }
    assert_eq!(a.cpu.pc, b.cpu.pc);
    assert_eq!(a.bus.cycles, b.bus.cycles);
    assert_eq!(a.bus.ppu.frame, b.bus.ppu.frame);
    assert!(b.load_state(b"junk").is_err());
}

use std::sync::atomic::{AtomicU32, Ordering::SeqCst};
static FRAMES: AtomicU32 = AtomicU32::new(0);
static EXPECT_W: AtomicU32 = AtomicU32::new(0);
static EXPECT_H: AtomicU32 = AtomicU32::new(0);

/// The libretro entry points, called the way a frontend does, for a GB and a GBA ROM.
#[test]
fn libretro_api_smoke() {
    run_smoke("dmg-acid2.gb", 160, 144);
    run_smoke("jsmolka/arm/arm.gba", 240, 160);
}

fn run_smoke(name: &str, ew: u32, eh: u32) {
    use continuum_gb::libretro::*;
    use std::ffi::c_void;
    let Some(data) = rom(name) else { return };
    EXPECT_W.store(ew, SeqCst);
    EXPECT_H.store(eh, SeqCst);
    FRAMES.store(0, SeqCst);
    unsafe extern "C" fn env(_cmd: u32, _d: *mut c_void) -> bool { true }

    unsafe extern "C" fn video(_d: *const c_void, w: u32, h: u32, pitch: usize) {
        let ew = EXPECT_W.load(SeqCst);
        assert_eq!((w, h, pitch), (ew, EXPECT_H.load(SeqCst), ew as usize * 4));
        FRAMES.fetch_add(1, SeqCst);
    }
    unsafe extern "C" fn audio(_d: *const i16, n: usize) -> usize { n }
    unsafe extern "C" fn poll() {}
    unsafe extern "C" fn input(_p: u32, _d: u32, _i: u32, _id: u32) -> i16 { 0 }
    unsafe {
        retro_set_environment(env);
        retro_set_video_refresh(video);
        retro_set_audio_sample_batch(audio);
        retro_set_input_poll(poll);
        retro_set_input_state(input);
        retro_init();
        let info = RetroGameInfo {
            path: std::ptr::null(),
            data: data.as_ptr() as *const c_void,
            size: data.len(),
            meta: std::ptr::null(),
        };
        assert!(retro_load_game(&info));
        for _ in 0..10 {
            retro_run();
        }
        let size = retro_serialize_size();
        let mut buf = vec![0u8; size];
        assert!(retro_serialize(buf.as_mut_ptr() as *mut c_void, size));
        assert!(retro_unserialize(buf.as_ptr() as *const c_void, size));
        retro_unload_game();
        retro_deinit();
        assert_eq!(FRAMES.load(SeqCst), 10);
    }
}

/// Debug aid: DBG_ROM=<path under test-roms> prints the registers at LD B,B and the serial text.
#[test]
fn debug_rom() {
    let Ok(name) = std::env::var("DBG_ROM") else { return };
    let mut gb = GameBoy::new(rom(&name).unwrap()).unwrap();
    let stop: Option<u16> = std::env::var("DBG_PC").ok().and_then(|v| u16::from_str_radix(&v, 16).ok());
    for _ in 0..20_000_000u64 {
        gb.cpu.step(&mut gb.bus);
        if gb.cpu.ld_b_b || Some(gb.cpu.pc) == stop { break; }
    }
    eprintln!("cpu {:?}", gb.cpu);
    eprintln!("serial {}", String::from_utf8_lossy(&gb.bus.serial_out));
}

#[test]
fn debug_io() {
    if std::env::var("DBG_IO").is_err() { return }
    let gb = GameBoy::new(rom("cpu_instrs.gb").unwrap()).unwrap();
    let v: Vec<String> = (0xFF00u16..0xFF80).map(|a| format!("{:02X}", gb.bus.raw_read(a))).collect();
    for c in v.chunks(8) { eprintln!("IO {}", c.join(" ")); }
}

/// cgb-acid2 must pass on both drawers.
#[test]
fn cgb_acid2() {
    for (exact, label) in DRAWERS {
        cgb_acid2_on(exact, label);
    }
}

fn cgb_acid2_on(exact: bool, label: &str) {
    let Some(data) = rom("cgb-acid2.gbc") else { return };
    let Some(png_bytes) = rom("cgb-acid2-reference.png") else { return };
    let mut gb = GameBoy::new(data).unwrap();
    gb.bus.ppu.exact = exact;
    assert!(gb.bus.cgb, "cgb-acid2 must run in colour mode");
    for _ in 0..120 {
        gb.run_frame();
    }
    let mut decoder = png::Decoder::new(std::io::Cursor::new(png_bytes));
    decoder.set_transformations(png::Transformations::EXPAND);
    let mut reader = decoder.read_info().unwrap();
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).unwrap();
    let bpp = info.line_size / info.width as usize;
    let mut wrong = 0;
    let mut first = None;
    for y in 0..144usize {
        for x in 0..160usize {
            let p = &buf[y * info.line_size + x * bpp..];
            let want = if bpp >= 3 {
                u32::from(p[0]) << 16 | u32::from(p[1]) << 8 | u32::from(p[2])
            } else {
                u32::from(p[0]) * 0x010101
            };
            let got = gb.bus.ppu.frame[y * 160 + x];
            if got != want {
                wrong += 1;
                first.get_or_insert((x, y, got, want));
            }
        }
    }
    assert_eq!(wrong, 0, "cgb-acid2 ({label} drawer): {wrong} pixels differ, first (x, y, got, want) {first:x?}");
    eprintln!("DRAWER {label}: cgb-acid2 passed");
}

/// The Mooneye tests made for CGB hardware, run as a CGB. A report, like the DMG one.
/// The CGB/AGB boot-state tests on a DMG cartridge, as the console model option runs them.
#[test]
fn mooneye_models() {
    use continuum_gb::gb::Model;
    let cases: [(&str, Model, bool); 10] = [
        ("mts/acceptance/boot_div-dmg0.gb", Model::Dmg0, false), ("mts/acceptance/boot_regs-dmg0.gb", Model::Dmg0, false),
        ("mts/acceptance/boot_div-S.gb", Model::Sgb, false), ("mts/acceptance/boot_regs-sgb.gb", Model::Sgb, false),
        ("mts/misc/boot_regs-cgb.gb", Model::Cgb, false), ("mts/misc/boot_div-cgbABCDE.gb", Model::Cgb, false),
        ("mts/misc/boot_div-cgb0.gb", Model::Cgb, true), ("mts/misc/boot_regs-A.gb", Model::Agb, false),
        ("mts/misc/boot_div-A.gb", Model::Agb, false), ("mts/acceptance/boot_regs-mgb.gb", Model::Mgb, false),
    ];
    let mut failed = Vec::new();
    for (name, model, rev0) in cases {
        let Some(data) = rom(name) else { return };
        let mut gb = GameBoy::with_model(data, model, None).unwrap();
        if rev0 {
            gb.set_cgb_revision_0();
        }
        for _ in 0..600 {
            gb.run_frame();
            if gb.cpu.ld_b_b { break; }
        }
        let c = &gb.cpu;
        if [c.b, c.c, c.d, c.e, c.h, c.l] != [3, 5, 8, 13, 21, 34] {
            failed.push(name);
        }
    }
    assert!(failed.is_empty(), "{failed:?}");
}

#[test]
fn mooneye_cgb_report() {
    let names = ["mts/misc/boot_regs-cgb.gb", "mts/misc/boot_hwio-C.gb", "mts/misc/boot_div-cgbABCDE.gb",
                 "mts/misc/bits/unused_hwio-C.gb", "mts/misc/ppu/vblank_stat_intr-C.gb"];
    let mut failed = Vec::new();
    for n in names {
        if let Err(e) = mooneye_mode(n, true) {
            failed.push(e);
        }
    }
    eprintln!("MOONEYE CGB: {} of {} passed", names.len() - failed.len(), names.len());
    for f in &failed {
        eprintln!("  FAIL {f}");
    }
}

/// Runs every .gb in $GB_ROM_DIR (e.g. SameSuite, built locally with rgbds) in CGB mode, Mooneye
/// style (`LD B,B` with the Fibonacci registers), and prints a report. A debugging aid; no ROMs
/// needed for CI.
#[test]
fn rom_dir_cgb_report() {
    let Ok(dir) = std::env::var("GB_ROM_DIR") else { return };
    let mut names: Vec<_> = std::fs::read_dir(&dir).unwrap().filter_map(|e| e.ok())
        .map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "gb")).collect();
    names.sort();
    let mut pass = 0;
    for p in &names {
        let data = std::fs::read(p).unwrap();
        let mut gb = GameBoy::new_mode(data, true).unwrap();
        let mut res = "timed out".to_string();
        for _ in 0..600 {
            gb.run_frame();
            if gb.cpu.ld_b_b {
                let c = &gb.cpu;
                let regs = [c.b, c.c, c.d, c.e, c.h, c.l];
                res = if regs == [3, 5, 8, 13, 21, 34] { "PASS".into() } else { format!("regs {regs:02X?}") };
                break;
            }
            if gb.cpu.locked { res = "locked".into(); break; }
        }
        if res == "PASS" { pass += 1 }
        eprintln!("  {res:<30} {}", p.file_name().unwrap().to_string_lossy());
    }
    eprintln!("CGB DIR: {pass} of {} passed", names.len());
}

/// Mooneye's MBC2 tests (bank switching, RAM nibbles and mirroring).
#[test]
fn mooneye_mbc2() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test-roms/mts/emulator-only/mbc2");
    let Ok(rd) = std::fs::read_dir(&dir) else {
        eprintln!("SKIPPED: Mooneye suite not fetched");
        return;
    };
    let mut names: Vec<String> = rd.flatten().map(|e| e.file_name().to_string_lossy().to_string()).filter(|n| n.ends_with(".gb")).collect();
    names.sort();
    let failed: Vec<String> = names.iter().filter_map(|n| mooneye(&format!("mts/emulator-only/mbc2/{n}")).err()).collect();
    assert!(failed.is_empty() && !names.is_empty(), "{failed:?}");
}

fn png_rgb(bytes: &[u8]) -> Vec<u32> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND);
    let mut reader = decoder.read_info().unwrap();
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).unwrap();
    let bpp = info.line_size / info.width as usize;
    let mut out = Vec::with_capacity(160 * 144);
    for y in 0..info.height as usize {
        for x in 0..info.width as usize {
            let p = &buf[y * info.line_size + x * bpp..];
            let (r, g, b) = if bpp >= 3 { (p[0], p[1], p[2]) } else { (p[0], p[0], p[0]) };
            out.push(u32::from(r) << 16 | u32::from(g) << 8 | u32::from(b));
        }
    }
    out
}

/// Where a Mealybug picture differs from the expected one. A pass/fail count alone cannot say
/// which piece of the drawer is wrong, so every failure reports its position too.
struct MealyDiff {
    /// Differing pixels in the whole picture.
    wrong: usize,
    /// Screen lines with at least one differing pixel.
    lines: usize,
    /// The first differing pixel in reading order: x, y, the colour drawn, the colour expected.
    first: Option<(usize, usize, u32, u32)>,
}

impl std::fmt::Display for MealyDiff {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self.first {
            None => write!(f, "identical"),
            Some((x, y, got, want)) => write!(
                f,
                "{} px on {} lines, first line {y}, first pixel x={x}, got {got:06X} want {want:06X}",
                self.wrong, self.lines
            ),
        }
    }
}

/// One Mealybug Tearoom test on the exact drawer (the one these pictures are for): run to
/// `LD B,B`, compare the screen. Returns where it differs.
fn mealybug(name: &str, cgb: bool) -> Option<MealyDiff> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test-roms/mealybug");
    let data = std::fs::read(dir.join(format!("{name}.gb"))).ok()?;
    let exp = std::fs::read(dir.join("expected").join(if cgb { "CPU CGB C" } else { "DMG-blob" }).join(format!("{name}.png"))).ok()?;
    let want = png_rgb(&exp);
    let mut gb = GameBoy::new_mode(data, cgb).ok()?;
    gb.bus.ppu.exact = true;
    for _ in 0..600 {
        gb.run_frame();
        if gb.cpu.ld_b_b {
            break;
        }
    }
    // The picture is complete a couple of frames later.
    for _ in 0..3 {
        gb.run_frame();
    }
    let got = &gb.bus.ppu.frame;
    if let Ok(d) = std::env::var("MEALY_DUMP") {
        let mut out = format!("P6 320 144 255\n").into_bytes();
        for y in 0..144 {
            for half in [got.as_slice(), want.as_slice()] {
                for x in 0..160 {
                    let p = half[y * 160 + x];
                    out.extend([(p >> 16) as u8, (p >> 8) as u8, p as u8]);
                }
            }
        }
        std::fs::write(format!("{d}/{}{name}.ppm", if cgb { "cgb_" } else { "dmg_" }), out).unwrap();
    }
    // DMG references are grey levels; ours are the same four greys.
    let map = |p: u32| if cgb { p } else {
        match p & 0xFF { 0xFF => continuum_gb::ppu::SHADES[0], 0xAA => continuum_gb::ppu::SHADES[1], 0x55 => continuum_gb::ppu::SHADES[2], _ => continuum_gb::ppu::SHADES[3] }
    };
    let mut diff = MealyDiff { wrong: 0, lines: 0, first: None };
    for y in 0..144usize {
        let mut line_wrong = false;
        for x in 0..160usize {
            let (w, g) = (map(want[y * 160 + x]), got[y * 160 + x]);
            if w != g {
                diff.wrong += 1;
                line_wrong = true;
                diff.first.get_or_insert((x, y, g, w));
            }
        }
        diff.lines += usize::from(line_wrong);
    }
    Some(diff)
}

/// Mid-scanline register changes (Mealybug Tearoom). Reports; the passing set is pinned below.
#[test]
fn mealybug_report() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test-roms/mealybug");
    if !dir.exists() {
        eprintln!("SKIPPED: mealybug not fetched");
        return;
    }
    let mut pass = Vec::new();
    let mut fail = Vec::new();
    for cgb in [false, true] {
        let exp = dir.join("expected").join(if cgb { "CPU CGB C" } else { "DMG-blob" });
        let mut names: Vec<String> = std::fs::read_dir(&exp).unwrap().flatten()
            .map(|e| e.file_name().to_string_lossy().trim_end_matches(".png").to_string()).collect();
        names.sort();
        for n in names {
            let label = format!("{}{n}", if cgb { "cgb:" } else { "dmg:" });
            match mealybug(&n, cgb) {
                Some(d) if d.wrong == 0 => pass.push(label),
                Some(d) => fail.push((label, d)),
                None => {}
            }
        }
    }
    eprintln!("MEALYBUG (exact drawer): {} passed, {} failed", pass.len(), fail.len());
    for p in &pass {
        eprintln!("  PASS {p}");
    }
    // Where each failure differs, not just that it does: the map for work on the exact drawer.
    for (label, d) in &fail {
        eprintln!("  FAIL {label}: {d}");
    }
    for must in MEALYBUG_PASSING {
        assert!(pass.iter().any(|p| p == must), "mealybug regressed: {must}");
    }
}

const MEALYBUG_PASSING: &[&str] = &[
    "dmg:m2_win_en_toggle",
    "dmg:m3_bgp_change",
    "dmg:m3_lcdc_win_en_change_multiple_wx",
    "dmg:m3_scx_low_3_bits",
    "dmg:m3_wx_4_change_sprites",
];

/// On the exact drawer, which is the one that reads SCX mid-line.
#[test]
fn scx_low_bits_take_effect_on_line_72() {
    let Some(data) = rom("mealybug/m3_scx_low_3_bits.gb") else { return };
    let mut gb = GameBoy::new(data).unwrap();
    gb.bus.ppu.exact = true;
    for b in gb.bus.ppu.vram[0x190..0x1A0].iter_mut() {
        *b = 0xFF;
    }
    for _ in 0..30 {
        gb.run_frame();
        if gb.cpu.ld_b_b {
            break;
        }
    }
    let frame = &gb.bus.ppu.frame;
    let x_at = |y: usize| (0..160).find(|x| frame[y * 160 + x] & 0xFF == 0).unwrap_or(999);
    assert_eq!((x_at(0), x_at(71), x_at(72)), (152, 152, 150));
}
