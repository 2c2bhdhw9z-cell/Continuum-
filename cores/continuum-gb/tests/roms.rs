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
    let Some(data) = rom(name) else { return Ok(()) };
    let mut gb = GameBoy::new_mode(data, cgb)?;
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

#[test]
fn dmg_acid2() {
    let Some(data) = rom("dmg-acid2.gb") else { return };
    let Some(png_bytes) = rom("dmg-acid2-reference.png") else { return };
    let mut gb = GameBoy::new(data).unwrap();
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
    assert_eq!(wrong, 0, "dmg-acid2: {wrong} pixels differ, first at {first:?}");
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
    let mut failed = Vec::new();
    for n in &names {
        if let Err(e) = mooneye(n) {
            failed.push(e);
        }
    }
    eprintln!("MOONEYE: {} of {} passed", names.len() - failed.len(), names.len());
    for f in &failed {
        eprintln!("  FAIL {f}");
    }
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

static mut FRAMES: u32 = 0;
static mut EXPECT: (u32, u32) = (0, 0);

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
    unsafe { EXPECT = (ew, eh); FRAMES = 0; }
    unsafe extern "C" fn env(_cmd: u32, _d: *mut c_void) -> bool { true }

    unsafe extern "C" fn video(_d: *const c_void, w: u32, h: u32, pitch: usize) {
        assert_eq!((w, h, pitch), (EXPECT.0, EXPECT.1, EXPECT.0 as usize * 4));
        FRAMES += 1;
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
        assert_eq!(FRAMES, 10);
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

#[test]
fn cgb_acid2() {
    let Some(data) = rom("cgb-acid2.gbc") else { return };
    let Some(png_bytes) = rom("cgb-acid2-reference.png") else { return };
    let mut gb = GameBoy::new(data).unwrap();
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
    assert_eq!(wrong, 0, "cgb-acid2: {wrong} pixels differ, first (x, y, got, want) {first:x?}");
}

/// The Mooneye tests made for CGB hardware, run as a CGB. A report, like the DMG one.
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
