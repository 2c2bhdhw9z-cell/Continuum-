//! Public test ROMs, run headless. Fetch them first with scripts/fetch-test-roms.sh; a missing
//! ROM is reported and the test is skipped (so a fresh checkout without them still builds).

use std::path::PathBuf;

use continuum_gb::gb::{GameBoy, Model};
use continuum_gb::ppu::CgbRev;

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

/// Blargg's OAM corruption (OAM bug) tests: the DMG Mode-2 OAM corruption model from P2.
/// The combined ROM runs all eight subtests; each single pins one of them.
/// These ROMs report through external RAM at $A000 (signature $DE,$B0,$61 at $A001, result
/// code at $A000: $80 while running, 0 when passed), not the serial port — and the OAM bug
/// is DMG-only, so they are forced to a DMG even though their headers are CGB-compatible.
fn oam_bug(name: &str, max_frames: u32) {
    let Some(data) = rom(name) else { return };
    let mut gb = GameBoy::new_mode(data, false).unwrap();
    for _ in 0..max_frames {
        gb.run_frame();
        let ram = &gb.bus.cart.ram;
        if ram.len() >= 4 && ram[1] == 0xDE && ram[2] == 0xB0 && ram[3] == 0x61 && ram[0] != 0x80 {
            let text: Vec<u8> = ram[4..].iter().take_while(|&&b| b != 0).cloned().collect();
            let out = String::from_utf8_lossy(&text).to_string();
            assert_eq!(ram[0], 0, "{name} failed:\n{out}");
            return;
        }
    }
    panic!("{name} timed out (no $A000 result)");
}

#[test]
fn blargg_oam_bug_singles() {
    // 1, 2, 3 pass. The other five fail and are NOT pinned (see below); the combined ROM
    // runs all eight, so it is not pinned either.
    for n in ["1-lcd_sync", "2-causes", "3-non_causes"] {
        oam_bug(&format!("oam_bug/{n}.gb"), 1200);
    }
}

// The remaining oam_bug singles fail (P3, 2026-11-24; re-checked in Item 3, 2026-11-25)
// — a real gap in the P2 OAM corruption model, not a harness problem (all run as DMG, via
// the $A000 protocol above). Item 3 fixed `Cpu::pop()` to note the SP glitch before the
// low-byte read (same M-cycle, Pan Docs: pop corrupts 3 times), which clears test 8's #3
// (POP pattern) in isolation, but #2 still fails, so 8 is still not pinned.
//   4-scanline_timing #3 "INC DE at first corruption": the subtest INC DE lands in VBlank
//     (ly=153, dot 452) instead of line 0 mode 2 — the LCD-enable + 70224-dot delay
//     alignment is ~8 dots off, so the window is missed entirely. The CPU delay chain is
//     hand-verified exact (70223 M-cycles LCD-on to the note: $C1DD(1)=65545, $C1D0(18)=
//     4617, $C1C1(27)=47 incl. CALL), and test 1 pins the first line to 449-456 dots and
//     the LCD-on anchor to <4 dots early, so the shift must come from after the first line.
//     Tried and reverted: line 153 = 454 dots (frame 70222) fixes 4/5/8 but adds +2 dots
//     of drift per frame, which walks test 6's phase-locked ladder into mode 2; and
//     starting line 0 at dot 8 breaks test 1 #2 ("starts too late in scanline").
//   5-timing_bug #2 "Should corrupt at beginning of first scanline": same alignment miss.
//   6-timing_no_bug #1: the INC DE ladder is phase-locked at dot 452 (mode 0, safe, zero
//     drift across the 10 outer iterations), but the DEC DE that undoes it lands at dot 72
//     (row 18, mode 2) and corrupts row 18, where hardware's narrower window leaves OAM
//     alone. Disabling corrupt_oam makes 6 pass (and breaks 2, as expected).
//   7-timing_effect: no $A000 result in 1200 frames (still running / hung) with corruption
//     on; with corruption disabled it fails #1 at frame 374, so the hang is
//     corruption-dependent (the ROM waits on a corruption effect our model produces at
//     the wrong row/time).
//   8-instr_effect #2 "INC/DEC rp pattern is wrong": the INC/DEC notes land ~8 dots early
//     (same alignment miss as 4/5); #3 (POP pattern) is fixed by the pop() M-cycle fix.
// Pan Docs (mode 2, one row per M-cycle, row = dot/4) agrees with our window, so the fix
// is the LCD-enable/frame alignment and the exact window edges — a deeper timing change
// than P3 could land safely without risking the 87/87 Mooneye gate. Do not pin these
// until they pass for the right reason.

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
    mooneye_run(gb, name)
}

/// `mooneye` on a chosen console, as the console model option starts it. The Color and Advance
/// tests are Game Boy cartridges: what they measure is the console, not the cartridge.
pub fn mooneye_model(name: &str, model: continuum_gb::gb::Model) -> Result<(), String> {
    mooneye_model_on(name, model, false)
}

/// `mooneye_model` on a chosen drawer.
pub fn mooneye_model_on(name: &str, model: continuum_gb::gb::Model, exact: bool) -> Result<(), String> {
    let Some(data) = rom(name) else { return Ok(()) };
    let mut gb = GameBoy::with_model(data, model, None)?;
    gb.bus.ppu.exact = exact;
    mooneye_run(gb, name)
}

fn mooneye_run(mut gb: GameBoy, name: &str) -> Result<(), String> {
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

/// dmg-acid2 on a Game Boy Color: the console runs it in Game Boy mode, and draws it in the
/// colours its start-up code picks for it, which for this cartridge (not Nintendo's) is the
/// default set. The test's own reference for a Color in Game Boy mode must match pixel for
/// pixel, on both drawers. Behind it, every pixel is also checked to be its Game Boy shade looked
/// up in the Color's palette memory, so a wrong colour says which palette it came out of.
#[test]
fn dmg_acid2_on_a_colour() {
    let Some(data) = rom("dmg-acid2.gb") else { return };
    let Some(png_bytes) = rom("dmg-acid2-reference-cgb.png") else { return };
    let want = png_rgb(&png_bytes);
    for (exact, label) in DRAWERS {
        let run = |model: Model| {
            let mut gb = GameBoy::with_model(data.clone(), model, None).unwrap();
            gb.bus.ppu.exact = exact;
            for _ in 0..120 {
                gb.run_frame();
            }
            gb
        };
        let (colour, mono) = (run(Model::Cgb), run(Model::Dmg));
        assert!(colour.bus.cgb_hardware && !colour.bus.cgb, "Game Boy mode on a Color");
        assert_eq!(colour.bus.ppu.shade, mono.bus.ppu.shade, "{label} drawer: not the Game Boy's shades");
        assert_eq!(colour.bus.ppu.source, mono.bus.ppu.source, "{label} drawer: not the Game Boy's palettes");
        let p = continuum_gb::compat::picked(&data);
        let five = |c: u32| {
            let v = u32::from(continuum_gb::compat::rgb15(c));
            let ch = |x: u32| (x << 3) | (x >> 2);
            ch(v & 31) << 16 | ch(v >> 5 & 31) << 8 | ch(v >> 10 & 31)
        };
        let ppu = &colour.bus.ppu;
        for i in 0..160 * 144 {
            let set = [p.bg, p.obj0, p.obj1][usize::from(ppu.source[i])];
            assert_eq!(ppu.frame[i], five(set[usize::from(ppu.shade[i])]), "{label} drawer, pixel {i}");
        }
        let wrong = (0..160 * 144).filter(|i| ppu.frame[*i] != want[*i]).count();
        assert_eq!(wrong, 0, "dmg-acid2 on a Color ({label} drawer): {wrong} pixels differ from its reference");
        eprintln!("DRAWER {label}: dmg-acid2 on a Color passed");
    }
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
                "acceptance/oam_dma", "acceptance/ppu", "acceptance/serial", "acceptance/timer",
                "emulator-only/mbc1", "emulator-only/mbc5"] {
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
    // 86 until the link port got a real clock of its own, which brought the serial folder's
    // boot_sclk_align in with it.
    assert_eq!(names.len(), 87, "expected the 87 DMG acceptance tests");
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

/// The Mooneye tests written for Color hardware, a gate like the DMG ones. All five are Game Boy
/// cartridges on purpose: what they measure is a Color console playing a Game Boy game, which is
/// how the console model option starts them, not a Color playing a colour game.
#[test]
fn mooneye_cgb() {
    use continuum_gb::gb::Model;
    // All five are a gate now. The three start-up ones are Color only; the two register ones hold
    // on an Advance too, so they are run as both consoles.
    let gated: [(&str, Model); 7] = [
        ("mts/misc/boot_regs-cgb.gb", Model::Cgb),
        ("mts/misc/boot_div-cgbABCDE.gb", Model::Cgb),
        ("mts/misc/boot_hwio-C.gb", Model::Cgb),
        ("mts/misc/boot_hwio-C.gb", Model::Agb),
        ("mts/misc/bits/unused_hwio-C.gb", Model::Cgb),
        ("mts/misc/bits/unused_hwio-C.gb", Model::Agb),
        // On a Color the mode 2 interrupt at the start of line 144 arrives one M-cycle before
        // VBlank, where on a Game Boy the two land together. The rule is in ppu.rs::oam_irq_early
        // and hangs off Ppu::cgb_hardware, which gb.rs now sets, so this one is a gate too.
        ("mts/misc/ppu/vblank_stat_intr-C.gb", Model::Cgb),
    ];
    // On both drawers: what the CPU can see never depends on which one draws.
    for (exact, label) in DRAWERS {
        let failed: Vec<String> = gated.iter().filter_map(|(n, m)| mooneye_model_on(n, *m, exact).err()).collect();
        eprintln!("MOONEYE CGB ({label} drawer): {} of {} runs passed", gated.len() - failed.len(), gated.len());
        for f in &failed {
            eprintln!("  FAIL {f}");
        }
        assert!(failed.is_empty(), "the Mooneye Color tests regressed on the {label} drawer: {failed:?}");
    }
}

/// A black-and-white cartridge that only loops, with this sixteen-byte title, from Nintendo or
/// not, and a header a real start-up file accepts: `logo` is the 48 logo bytes (taken from a
/// test ROM, so the bitmap is not copied into the source), and the header sum is right.
fn mono_cart(title: &[u8; 16], nintendo: bool, logo: &[u8]) -> Vec<u8> {
    let mut rom = vec![0u8; 0x8000];
    rom[0x100..0x104].copy_from_slice(&[0x00, 0xC3, 0x50, 0x01]); // NOP / JP $0150
    rom[0x104..0x134].copy_from_slice(logo);
    rom[0x134..0x144].copy_from_slice(title);
    rom[0x14B] = if nintendo { 0x01 } else { 0x00 };
    rom[0x150..0x152].copy_from_slice(&[0x18, 0xFE]); // JR -2
    rom[0x14D] = rom[0x134..0x14D].iter().fold(0u8, |x, b| x.wrapping_sub(*b).wrapping_sub(1));
    rom
}

/// A title with this sum and this fourth letter. Byte 15 is the cartridge's colour flag, so it
/// stays 0 (a black-and-white game) and byte 14 takes up the difference.
fn title_with(sum: u8, fourth: u8) -> [u8; 16] {
    let mut t = [b'Q'; 16];
    t[3] = fourth;
    t[15] = 0;
    t[14] = 0;
    t[14] = sum.wrapping_sub(t.iter().fold(0u8, |s, b| s.wrapping_add(*b)));
    t
}

/// What a Color has left when the game itself starts, for comparing two ways of starting it.
fn colour_compat_state(gb: &GameBoy) -> (bool, Vec<u8>, Vec<u8>, u8, u8, u8, [u8; 6]) {
    let (b, c) = (&gb.bus, &gb.cpu);
    (b.cgb || b.ppu.cgb, b.ppu.bg_pal[..8].to_vec(), b.ppu.obj_pal[..16].to_vec(), b.ppu.opri,
     b.ppu.bcps, b.ppu.ocps, [c.a, c.f, c.b, c.c, c.d, c.e])
}

/// When the player HAS a Color's own start-up file, it runs for real - in colour mode, reading
/// the header and the pad, then putting the console into Game Boy mode itself - and the game
/// starts with the same colours, registers and object order cBoy gives it without one: for every
/// title the start-up code knows, every shared title sum with a fourth letter it does not know,
/// sums it knows nothing about, a game that is not Nintendo's, and all twelve button schemes, on
/// a Color and on an Advance.
///
/// Needs a Color start-up file, which is never in the repository: `CBOY_CGB_BOOT` and
/// `CBOY_AGB_BOOT` name 2304-byte ones. SameBoy's free reimplementations (MIT, built from its
/// BootROMs folder with rgbds) are what this was run against; they pick from the same tables.
#[test]
fn a_colour_start_up_file_gives_the_same_colours() {
    let Some(logo) = rom("dmg-acid2.gb").map(|r| r[0x104..0x134].to_vec()) else { return };
    for (var, model) in [("CBOY_CGB_BOOT", Model::Cgb), ("CBOY_AGB_BOOT", Model::Agb)] {
        let Ok(path) = std::env::var(var) else {
            eprintln!("SKIPPED: set {var} to a Color start-up file to run this");
            continue;
        };
        let boot = std::fs::read(&path).unwrap();
        let mut carts: Vec<(Vec<u8>, Option<usize>)> = Vec::new();
        for (sum, letter) in continuum_gb::compat::known_titles() {
            carts.push((mono_cart(&title_with(sum, letter.unwrap_or(b'Q')), true, &logo), None));
            if letter.is_some() {
                carts.push((mono_cart(&title_with(sum, b'Z'), true, &logo), None));
            }
        }
        for sum in [0x02u8, 0x03, 0x7F, 0x80, 0xC0, 0xFE] {
            carts.push((mono_cart(&title_with(sum, b'Q'), true, &logo), None));
        }
        carts.push((mono_cart(&title_with(0xDB, b'Q'), false, &logo), None));
        for pick in 0..12 {
            carts.push((mono_cart(&title_with(0xDB, b'Q'), true, &logo), Some(pick)));
        }
        let mut differ = Vec::new();
        for (cart, pick) in &carts {
            let mut real = GameBoy::with_model(cart.clone(), model, Some(boot.clone())).unwrap();
            let mut ours = GameBoy::with_model(cart.clone(), model, None).unwrap();
            if let Some(i) = pick {
                real.pick_colours(&continuum_gb::compat::PICKS[*i]);
                ours.pick_colours(&continuum_gb::compat::PICKS[*i]);
            }
            assert!(real.bus.cgb, "a Color's start-up file runs in colour mode");
            let start = real.bus.cycles;
            while real.bus.boot_on && real.bus.cycles - start < 70_224 * 60 * 20 {
                real.cpu.step(&mut real.bus);
            }
            assert!(!real.bus.boot_on, "{path}: the start-up file never handed over");
            let (sum, fourth) = (cart[0x134..0x144].iter().fold(0u8, |s, b| s.wrapping_add(*b)), cart[0x137]);
            let (a, b) = (colour_compat_state(&real), colour_compat_state(&ours));
            if a != b {
                differ.push(format!("sum {sum:02X} fourth {:?} pick {pick:?}: start-up file {a:02X?}, cBoy {b:02X?}", fourth as char));
            }
            // The two titles the start-up code draws the logo map for: the map itself. HL is
            // left past that map by a route SameBoy's start-up code does not take, so HL is
            // compared for every other start only.
            if continuum_gb::compat::draws_the_logo(Some(sum)) && cart[0x14B] == 1 {
                assert_eq!(real.bus.ppu.vram[0x1900..0x1940], ours.bus.ppu.vram[0x1900..0x1940], "the logo map, sum {sum:02X}");
            } else if (real.cpu.h, real.cpu.l) != (ours.cpu.h, ours.cpu.l) {
                differ.push(format!("sum {sum:02X}: HL {:02X}{:02X} against {:02X}{:02X}", real.cpu.h, real.cpu.l, ours.cpu.h, ours.cpu.l));
            }
        }
        eprintln!("{var}: {} starts compared, {} differ", carts.len(), differ.len());
        assert!(differ.is_empty(), "{}", differ.join("\n"));
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

/// The consoles Mealybug photographed its pictures on, and how cBoy runs each.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Unit {
    Dmg,
    CgbC,
    /// A CPU CGB D: the Color with `CgbRev::D`.
    CgbD,
}

impl Unit {
    fn folder(self) -> &'static str {
        match self {
            Unit::Dmg => "DMG-blob",
            Unit::CgbC => "CPU CGB C",
            Unit::CgbD => "CPU CGB D",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Unit::Dmg => "dmg",
            Unit::CgbC => "cgb",
            Unit::CgbD => "cgbd",
        }
    }
}

/// One Mealybug Tearoom test on the exact drawer (the one these pictures are for): run to
/// `LD B,B`, compare the screen. Returns where it differs.
fn mealybug(name: &str, unit: Unit) -> Option<MealyDiff> {
    let cgb = unit != Unit::Dmg;
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test-roms/mealybug");
    let data = std::fs::read(dir.join(format!("{name}.gb"))).ok()?;
    let exp = std::fs::read(dir.join("expected").join(unit.folder()).join(format!("{name}.png"))).ok()?;
    let want = png_rgb(&exp);
    // Every one of these ROMs is a Game Boy cartridge (header $143 = 0, and no test sets the
    // colour flag or asks which console it is), and the folder a reference sits in names the
    // console it was photographed on. So the Color references are a Game Boy cartridge in a
    // Color console: that console's Game Boy-compatibility mode, which keeps the cartridge's own
    // BGP/OBP palettes, and NOT its colour mode, which would leave every palette entry white.
    let mut gb = if cgb {
        GameBoy::with_model(data, Model::Cgb, None).ok()?
    } else {
        GameBoy::new_mode(data, false).ok()?
    };
    if unit == Unit::CgbD {
        gb.set_cgb_revision(CgbRev::D);
    }
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
    let src = &gb.bus.ppu.source;
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
        std::fs::write(format!("{d}/{}_{name}.ppm", unit.label()), out).unwrap();
    }
    // DMG references are grey levels; ours are the same four greys.
    let map = |p: u32| if cgb { p } else {
        match p & 0xFF { 0xFF => continuum_gb::ppu::SHADES[0], 0xAA => continuum_gb::ppu::SHADES[1], 0x55 => continuum_gb::ppu::SHADES[2], _ => continuum_gb::ppu::SHADES[3] }
    };
    let mut diff = MealyDiff { wrong: 0, lines: 0, first: None };
    // MEALY_DETAIL=<substring of the name> prints every line of that picture as three rows of
    // shade digits - drawn, expected, and where they differ - so a one-dot sampling error can be
    // read off directly instead of guessed at. Every line, not only the differing ones, because
    // the lines that agree are what say where a window of suppressed pixels really begins.
    let detail = std::env::var("MEALY_DETAIL").ok().filter(|d| name.contains(d.as_str())).is_some();
    let digit = |p: u32| continuum_gb::ppu::SHADES.iter().position(|s| *s == p).map_or('?', |i| char::from(b'0' + i as u8));
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
        if detail {
            let row = |f: &dyn Fn(usize) -> u32| (0..160).map(|x| digit(f(x))).collect::<String>();
            eprintln!("    {y:>3} got  {}", row(&|x| got[y * 160 + x]));
            eprintln!("    {y:>3} want {}", row(&|x| map(want[y * 160 + x])));
            eprintln!("    {y:>3} diff {}", (0..160).map(|x| if map(want[y * 160 + x]) == got[y * 160 + x] { '.' } else { 'X' }).collect::<String>());
            // Which stage each pixel came out of: 0 background, 1 or 2 an object through OBP0 or
            // OBP1. A differing pixel means something quite different depending on this, and the
            // picture alone cannot say: the four pixels left in m3_lcdc_bg_en_change sit on lines
            // where the test puts a sprite at X=2, which made an object's left-edge pixel the
            // obvious suspect, and this row is what ruled it out - those lines draw no object
            // pixel anywhere, so all four are background the drawer failed to suppress.
            eprintln!("    {y:>3} from {}", (0..160).map(|x| char::from(b'0' + src[y * 160 + x])).collect::<String>());
        }
    }
    Some(diff)
}

/// Measurement aid for the exact drawer: runs a Mealybug ROM with its mid-line write neutralised
/// by a patch of the same length and cycle count, and prints the screen as shade digits. That
/// gives the picture the line WOULD have drawn with its timing untouched, which is what turns a
/// reference picture into evidence: a pixel the reference draws as colour 0 where this picture
/// has ink is one the hardware suppressed, and a pixel where the two agree is one it did not.
/// Comparing those windows against `MEALY_DETAIL`'s is how the register-write dots in fifo.rs
/// were established, rather than by trying pixels until the count went up.
///
/// `MEALY_BASE=m3_lcdc_bg_en_change MEALY_FIND=cb,81 MEALY_WITH=cb,41` turns that test's
/// `res 0,c` into `bit 0,c`: two bytes, eight cycles, no register touched, so every LCDC write
/// in the line writes the value the register already holds.
#[test]
fn mealy_baseline() {
    let Ok(name) = std::env::var("MEALY_BASE") else { return };
    let find: Vec<u8> = std::env::var("MEALY_FIND").unwrap().split(',').map(|v| u8::from_str_radix(v, 16).unwrap()).collect();
    let with: Vec<u8> = std::env::var("MEALY_WITH").unwrap().split(',').map(|v| u8::from_str_radix(v, 16).unwrap()).collect();
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test-roms/mealybug");
    let mut data = std::fs::read(dir.join(format!("{name}.gb"))).unwrap();
    let at = data.windows(find.len()).position(|w| w == find.as_slice()).expect("pattern");
    assert!(data[at + find.len()..].windows(find.len()).all(|w| w != find.as_slice()), "pattern is not unique");
    data[at..at + with.len()].copy_from_slice(&with);
    let mut gb = GameBoy::new_mode(data, false).unwrap();
    gb.bus.ppu.exact = true;
    for _ in 0..600 {
        gb.run_frame();
        if gb.cpu.ld_b_b {
            break;
        }
    }
    for _ in 0..3 {
        gb.run_frame();
    }
    for y in 0..144usize {
        let row: String = (0..160)
            .map(|x| continuum_gb::ppu::SHADES.iter().position(|s| *s == gb.bus.ppu.frame[y * 160 + x]).map_or('?', |i| char::from(b'0' + i as u8)))
            .collect();
        eprintln!("    {y:>3} base {row}");
    }
}

/// With the `trace` feature: runs one Mealybug ROM (CBOY_ROM=<name>, CBOY_CGB=1 for the Color)
/// and prints the exact drawer's state on every picture-register write of one frame, the same
/// frame scripts/sameboy's harness logs from SameBoy, so scripts/sameboy/compare-landings.py can
/// put the two side by side.
#[cfg(feature = "trace")]
#[test]
fn trace_landings() {
    let Ok(name) = std::env::var("CBOY_ROM") else { return };
    let cgb = std::env::var("CBOY_CGB").is_ok();
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test-roms/mealybug");
    let data = std::fs::read(dir.join(format!("{name}.gb"))).unwrap();
    let mut gb = if cgb { GameBoy::with_model(data, Model::Cgb, None).unwrap() } else { GameBoy::new_mode(data, false).unwrap() };
    gb.bus.ppu.exact = true;
    for _ in 0..600 {
        gb.run_frame();
        if gb.cpu.ld_b_b {
            break;
        }
    }
    gb.run_frame();
    continuum_gb::ppu::TRACE.store(true, std::sync::atomic::Ordering::Relaxed);
    gb.run_frame();
    continuum_gb::ppu::TRACE.store(false, std::sync::atomic::Ordering::Relaxed);
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
    for unit in [Unit::Dmg, Unit::CgbC, Unit::CgbD] {
        let exp = dir.join("expected").join(unit.folder());
        let mut names: Vec<String> = std::fs::read_dir(&exp).unwrap().flatten()
            .map(|e| e.file_name().to_string_lossy().trim_end_matches(".png").to_string()).collect();
        names.sort();
        for n in names {
            let label = format!("{}:{n}", unit.label());
            match mealybug(&n, unit) {
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

/// The Mealybug pictures the exact drawer gets pixel for pixel. This list only ever grows: a
/// change that drops one of these is wrong, whatever else it fixes. All 24 DMG pictures are here.
///
/// So are 27 of the 31 Color pictures: all but the four whose reference is Mealybug's own
/// "Expected result not currently available" placeholder, which nothing can match
/// (.kiro/notes/docs/CBOY_EXACT_DRAWER.md). These ROMs are Game Boy cartridges, so a Color runs
/// them in Game Boy mode: in the colours its start-up code picks for a cartridge that is not
/// Nintendo's (compat.rs), with the Color's own dots for when a write to a picture register lands
/// (`Bus::landing`), with the drawer doing what the Color's hardware does in either mode
/// (`colour_console` in fifo.rs), and with the Color's tile-select glitch (`bitplane_read`).
///
/// And 20 of the 24 CPU CGB D pictures, again all but the four placeholders: a CGB D takes a
/// palette write a dot earlier and reads SCY only at the tile-number step (`ppu::CgbRev`). Run as
/// a C, 7 of those 20 fail.
const MEALYBUG_PASSING: &[&str] = &[
    "cgb:m2_win_en_toggle",
    "cgb:m3_bgp_change",
    "cgb:m3_bgp_change_sprites",
    "cgb:m3_lcdc_bg_en_change",
    "cgb:m3_lcdc_bg_en_change2",
    "cgb:m3_lcdc_bg_map_change",
    "cgb:m3_lcdc_bg_map_change2",
    "cgb:m3_lcdc_obj_en_change",
    "cgb:m3_lcdc_obj_en_change_variant",
    "cgb:m3_lcdc_obj_size_change",
    "cgb:m3_lcdc_obj_size_change_scx",
    "cgb:m3_lcdc_tile_sel_change",
    "cgb:m3_lcdc_tile_sel_change2",
    "cgb:m3_lcdc_tile_sel_win_change",
    "cgb:m3_lcdc_tile_sel_win_change2",
    "cgb:m3_lcdc_win_en_change_multiple",
    "cgb:m3_lcdc_win_map_change",
    "cgb:m3_lcdc_win_map_change2",
    "cgb:m3_obp0_change",
    "cgb:m3_scx_high_5_bits",
    "cgb:m3_scx_high_5_bits_change2",
    "cgb:m3_scx_low_3_bits",
    "cgb:m3_scy_change",
    "cgb:m3_scy_change2",
    "cgb:m3_window_timing",
    "cgb:m3_window_timing_wx_0",
    "cgb:m3_wx_4_change_sprites",
    "cgbd:m2_win_en_toggle",
    "cgbd:m3_bgp_change",
    "cgbd:m3_bgp_change_sprites",
    "cgbd:m3_lcdc_bg_en_change",
    "cgbd:m3_lcdc_bg_map_change",
    "cgbd:m3_lcdc_obj_en_change",
    "cgbd:m3_lcdc_obj_en_change_variant",
    "cgbd:m3_lcdc_obj_size_change",
    "cgbd:m3_lcdc_obj_size_change_scx",
    "cgbd:m3_lcdc_tile_sel_change",
    "cgbd:m3_lcdc_tile_sel_win_change",
    "cgbd:m3_lcdc_win_en_change_multiple",
    "cgbd:m3_lcdc_win_map_change",
    "cgbd:m3_obp0_change",
    "cgbd:m3_scx_high_5_bits",
    "cgbd:m3_scx_low_3_bits",
    "cgbd:m3_scy_change",
    "cgbd:m3_window_timing",
    "cgbd:m3_window_timing_wx_0",
    "cgbd:m3_wx_4_change_sprites",
    "dmg:m2_win_en_toggle",
    "dmg:m3_bgp_change",
    "dmg:m3_bgp_change_sprites",
    "dmg:m3_lcdc_bg_en_change",
    "dmg:m3_lcdc_bg_map_change",
    "dmg:m3_lcdc_obj_en_change",
    "dmg:m3_lcdc_obj_en_change_variant",
    "dmg:m3_lcdc_obj_size_change",
    "dmg:m3_lcdc_obj_size_change_scx",
    "dmg:m3_lcdc_tile_sel_change",
    "dmg:m3_lcdc_tile_sel_win_change",
    "dmg:m3_lcdc_win_en_change_multiple",
    "dmg:m3_lcdc_win_en_change_multiple_wx",
    "dmg:m3_lcdc_win_map_change",
    "dmg:m3_obp0_change",
    "dmg:m3_scx_high_5_bits",
    "dmg:m3_scx_low_3_bits",
    "dmg:m3_scy_change",
    "dmg:m3_window_timing",
    "dmg:m3_window_timing_wx_0",
    "dmg:m3_wx_4_change",
    "dmg:m3_wx_4_change_sprites",
    "dmg:m3_wx_5_change",
    "dmg:m3_wx_6_change",
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

/// Blargg's sound tests, one at a time: the Game Boy ones on a Game Boy, the Color ones on a
/// Color. Each reports through cartridge RAM ($A000 status, $DE $B0 $61 after it, text from $A004).
#[test]
fn blargg_sound() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test-roms/sound");
    let Ok(rd) = std::fs::read_dir(&dir) else {
        eprintln!("SKIPPED: Blargg's sound tests not fetched");
        return;
    };
    let mut names: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "gb")).collect();
    names.sort();
    let mut failed = vec![];
    for p in &names {
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        let mut gb = GameBoy::new_mode(std::fs::read(p).unwrap(), name.starts_with("cgb")).unwrap();
        let mut res = Err("never finished".to_string());
        for _ in 0..3600 {
            gb.run_frame();
            let c = &gb.bus.cart;
            if [c.read_ram(0xA001), c.read_ram(0xA002), c.read_ram(0xA003)] == [0xDE, 0xB0, 0x61] && c.read_ram(0xA000) != 0x80 {
                let text: String = (0xA004..0xA200u16).map(|a| c.read_ram(a)).take_while(|b| *b != 0).map(char::from).collect();
                res = if c.read_ram(0xA000) == 0 { Ok(()) } else { Err(text.replace('\n', " ")) };
                break;
            }
        }
        if let Err(e) = res {
            failed.push(format!("{name}: {e}"));
        }
    }
    assert_eq!(names.len(), 24, "all 24 sound tests fetched");
    assert!(failed.is_empty(), "{failed:?}");
}

/// SameSuite's sound tests, which read the channels back a sample at a time on a Color (revision
/// E, the one cBoy's sound chip models). These 57 pass; SameBoy itself, built from its source and
/// run on the same files, passes exactly these 57 and fails the other 13 with the same results,
/// byte for byte. Five of the 13 are written for a revision B or earlier and pass under that
/// revision instead (`samesuite_sound_rev_b`); the other eight are two revision-specific
/// frequency-change tests (the -A and -cgb0BC ones, which need a CGB-C-and-older PCM read glitch
/// and a different frequency-change phase) and six wave-phase ones (both stop_div, both
/// volume_div, sweep and sweep_restart), whose first duty step lands one tick later on hardware
/// than cBoy's trigger delay gives — a delay the delay/restart tests pin exactly as it is, so
/// the difference is in the trigger-to-first-tick alignment, not the delay constant.
const SAMESUITE_SOUND_PASS: &[&str] = &[
    "channel_1_align", "channel_1_align_cpu", "channel_1_delay", "channel_1_duty", "channel_1_duty_delay",
    "channel_1_freq_change", "channel_1_freq_change_timing-cgbDE", "channel_1_nrx2_glitch",
    "channel_1_nrx2_speed_change", "channel_1_restart", "channel_1_restart_nrx2_glitch", "channel_1_stop_div",
    "channel_1_stop_restart", "channel_1_sweep", "channel_1_sweep_restart", "channel_1_sweep_restart_2", "channel_1_volume",
    "channel_1_volume_div", "channel_2_align", "channel_2_align_cpu", "channel_2_delay",
    "channel_2_duty", "channel_2_duty_delay", "channel_2_freq_change", "channel_2_nrx2_glitch",
    "channel_2_nrx2_speed_change", "channel_2_restart", "channel_2_restart_nrx2_glitch", "channel_2_stop_div",
    "channel_2_stop_restart", "channel_2_volume", "channel_2_volume_div", "channel_3_and_glitch", "channel_3_delay", "channel_3_first_sample",
    "channel_3_freq_change_delay", "channel_3_restart_delay", "channel_3_restart_during_delay",
    "channel_3_restart_stop_delay", "channel_3_shift_delay", "channel_3_shift_skip_delay", "channel_3_stop_delay",
    "channel_3_stop_div", "channel_3_wave_ram_dac_on_rw", "channel_3_wave_ram_locked_write",
    "channel_3_wave_ram_sync", "channel_4_align", "channel_4_delay", "channel_4_equivalent_frequencies",
    "channel_4_freq_change", "channel_4_frequency_alignment", "channel_4_lfsr", "channel_4_lfsr15",
    "channel_4_lfsr_15_7", "channel_4_lfsr_7_15", "channel_4_lfsr_restart", "channel_4_lfsr_restart_fast",
    "channel_4_volume_div", "div_trigger_volume_10", "div_write_trigger", "div_write_trigger_10",
    "div_write_trigger_volume", "div_write_trigger_volume_10",
];

/// SameSuite's sound tests written for a Color revision B or earlier, which cBoy runs with
/// that revision's sound chip (`GameBoy::set_cgb_revision_b`): on those, writing NRx4 clocks
/// the length counter whenever it was disabled before, whether or not the write enables it
/// (fixed on revision C). Four of the five extra-length tests pass under revision B now (the
/// wave channel's revision-B test needs revision B's wave channel loading one more length
/// than revision 0 does, and still fails). Under the default revision E all five fail, as
/// they do on SameBoy.
const SAMESUITE_SOUND_REV_B_PASS: &[&str] = &[
    "channel_1_extra_length_clocking-cgb0B",
    "channel_2_extra_length_clocking-cgb0B",
    "channel_3_extra_length_clocking-cgb0",
    "channel_4_extra_length_clocking-cgb0B",
];

#[test]
fn samesuite_sound_rev_b() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test-roms/samesuite-apu");
    if !dir.is_dir() {
        eprintln!("SKIPPED: SameSuite not built (run scripts/fetch-test-roms.sh)");
        return;
    }
    let mut failed = vec![];
    for name in SAMESUITE_SOUND_REV_B_PASS {
        let p = dir.join(format!("{name}.gb"));
        let mut gb = GameBoy::new_mode(std::fs::read(&p).unwrap(), true).unwrap();
        gb.set_cgb_revision_b();
        let mut passed = false;
        for _ in 0..600 {
            gb.run_frame();
            if gb.cpu.ld_b_b {
                let c = &gb.cpu;
                passed = [c.b, c.c, c.d, c.e, c.h, c.l] == [3, 5, 8, 13, 21, 34];
                break;
            }
        }
        if !passed {
            failed.push(name.to_string());
        }
    }
    assert!(failed.is_empty(), "revision-B sound tests that no longer pass: {failed:?}");
}

#[test]
fn samesuite_sound() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test-roms/samesuite-apu");
    if !dir.is_dir() {
        eprintln!("SKIPPED: SameSuite not built (run scripts/fetch-test-roms.sh)");
        return;
    }
    let (mut failed, mut also) = (vec![], vec![]);
    let mut total = 0;
    for e in std::fs::read_dir(&dir).unwrap().flatten() {
        let p = e.path();
        let Some(name) = p.file_stem().map(|s| s.to_string_lossy().to_string()) else { continue };
        total += 1;
        let mut gb = GameBoy::new_mode(std::fs::read(&p).unwrap(), true).unwrap();
        let mut passed = false;
        for _ in 0..600 {
            gb.run_frame();
            if gb.cpu.ld_b_b {
                let c = &gb.cpu;
                passed = [c.b, c.c, c.d, c.e, c.h, c.l] == [3, 5, 8, 13, 21, 34];
                break;
            }
        }
        match (passed, SAMESUITE_SOUND_PASS.contains(&name.as_str())) {
            (false, true) => failed.push(name),
            (true, false) => also.push(name),
            _ => {}
        }
    }
    if !also.is_empty() {
        eprintln!("also passing now (add them to SAMESUITE_SOUND_PASS): {also:?}");
    }
    assert_eq!(total, 70, "SameSuite's 70 sound tests");
    assert!(failed.is_empty(), "sound tests that passed no longer do: {failed:?}");
}

/// Skipping a game's waiting loops changes nothing it can tell. A loop that waits for a flag in
/// work RAM (set by the VBlank interrupt handler, cleared again by the loop) runs for 600 frames
/// with the skip and without: the same machine at the end, byte for byte, frame after frame,
/// and nearly all of it skipped. A loop that reads the screen's line counter instead (an I/O
/// register, which changes by itself) is never skipped.
#[test]
fn skipping_waiting_loops_changes_nothing() {
    let mk = |code: &[u8], handler: &[u8]| {
        let mut r = vec![0u8; 0x8000];
        r[0x100..0x100 + code.len()].copy_from_slice(code);
        r[0x40..0x40 + handler.len()].copy_from_slice(handler);
        r
    };
    let flag = mk(&[0x3E, 0x01, 0xE0, 0xFF, 0xFB,          // ld a,1; ldh (IE),a; ei
                    0xFA, 0x00, 0xC0, 0xA7, 0x28, 0xFA,    // wait: ld a,(C000); and a; jr z,wait
                    0xAF, 0xEA, 0x00, 0xC0, 0x18, 0xF3],   // xor a; ld (C000),a; jr wait
                  &[0x3E, 0x01, 0xEA, 0x00, 0xC0, 0xD9]);  // ld a,1; ld (C000),a; reti
    let ly = mk(&[0xF0, 0x44, 0xFE, 0x90, 0x20, 0xFA, 0x18, 0xF8], &[]); // wait for line 144
    for (rom, should_skip) in [(flag, true), (ly, false)] {
        for cgb in [false, true] {
            let mut a = GameBoy::new_mode(rom.clone(), cgb).unwrap();
            let mut b = GameBoy::new_mode(rom.clone(), cgb).unwrap();
            b.wait.on = true;
            for f in 0..600 {
                a.run_frame();
                b.run_frame();
                assert!(a.cpu.same_state(&b.cpu) && a.bus.cycles == b.bus.cycles, "frame {f}: the machines parted");
                assert_eq!(a.bus.ppu.frame, b.bus.ppu.frame, "frame {f}");
            }
            assert!(a.save_state() == b.save_state(), "the whole machine is the same");
            let frames = b.bus.cycles / 4;
            if should_skip {
                assert!(b.wait.skipped > frames * 9 / 10, "skipped {} of {frames}", b.wait.skipped);
            } else {
                assert_eq!(b.wait.skipped, 0);
            }
        }
    }
}

/// gbmicrotest, run headless: two frames each (30 for is_if_set_during_ime0, ~380ms), as a DMG.
/// Reports the pass/fail counts and asserts every pinned name still passes. The failing and
/// no-result ROMs are listed, not asserted, so a fix that turns one green shows up as a count
/// change here and the name moves into GBMICROTEST_PASSING.
#[test]
fn gbmicrotest_report() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test-roms/gbmicrotest");
    if !dir.exists() {
        eprintln!("SKIPPED: gbmicrotest not fetched (run scripts/fetch-test-roms.sh)");
        return;
    }
    let mut names: Vec<String> = std::fs::read_dir(&dir).unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.ends_with(".gb"))
        .collect();
    names.sort();
    let mut pass = Vec::new();
    let mut fail = Vec::new();
    let mut noresult = Vec::new();
    for n in &names {
        let data = std::fs::read(dir.join(n)).unwrap();
        let mut gb = GameBoy::new_mode(data, false).unwrap();
        let frames = if n.starts_with("is_if_set_during_ime0") { 30 } else { 2 };
        for _ in 0..frames {
            gb.run_frame();
        }
        let h = &gb.bus.hram;
        let stem = n.trim_end_matches(".gb");
        match h[2] {
            0x01 => pass.push(stem.to_string()),
            0xFF => fail.push(format!("{stem} actual={:#04x} expected={:#04x}", h[0], h[1])),
            _ => noresult.push(stem.to_string()),
        }
    }
    eprintln!("GBMICROTEST: {} passed, {} failed, {} no-result, of {}",
        pass.len(), fail.len(), noresult.len(), names.len());
    for f in &fail {
        eprintln!("  FAIL {f}");
    }
    for f in &noresult {
        eprintln!("  NORESULT {f}");
    }
    // 513 ROMs in c-sp/game-boy-test-roms v7.0 (P3, 2026-11-24).
    assert_eq!(names.len(), 513, "gbmicrotest set changed; re-pin GBMICROTEST_PASSING");
    for must in GBMICROTEST_PASSING {
        assert!(pass.iter().any(|p| p == must), "gbmicrotest regressed: {must}");
    }
}

/// gbmicrotest (aappleby/GBMicrotest, via c-sp/game-boy-test-roms v7.0): cycle-exact
/// register/PPU/CPU microtests, run as DMG, reporting through HRAM $FF80-$FF82
/// ($FF82 = 0x01 pass, 0xFF fail; $FF80 actual, $FF81 expected). The list only ever
/// grows: a change that drops one of these is wrong, whatever else it fixes.
/// 390 of 513 pass (P6, 2026-11-25); the rest are interrupt-timing and PPU-latch
/// edge cases and 31 non-HRAM tests.
const GBMICROTEST_PASSING: &[&str] = &[
    "div_inc_timing_a", "div_inc_timing_b", "dma_0x1000", "dma_0x9000",
    "dma_0xA000", "dma_0xC000", "dma_0xE000", "dma_timing_a",
    "halt_op_dupe", "hblank_int_di_timing_a", "hblank_int_di_timing_b", "hblank_int_if_b",
    "hblank_int_l0", "hblank_int_l1", "hblank_int_l2", "hblank_int_scx0",
    "hblank_int_scx0_if_a", "hblank_int_scx0_if_c", "hblank_int_scx0_if_d", "hblank_int_scx1_if_a",
    "hblank_int_scx1_if_c", "hblank_int_scx2_if_a", "hblank_int_scx2_if_c", "hblank_int_scx3",
    "hblank_int_scx3_if_a", "hblank_int_scx3_if_c", "hblank_int_scx3_if_d", "hblank_int_scx3_nops_a",
    "hblank_int_scx3_nops_b", "hblank_int_scx4", "hblank_int_scx4_if_a", "hblank_int_scx4_if_c",
    "hblank_int_scx4_if_d", "hblank_int_scx4_nops_a", "hblank_int_scx4_nops_b", "hblank_int_scx5_if_a",
    "hblank_int_scx5_if_c", "hblank_int_scx6_if_a", "hblank_int_scx6_if_c", "hblank_int_scx7",
    "hblank_int_scx7_if_a", "hblank_int_scx7_if_c", "hblank_int_scx7_if_d", "hblank_int_scx7_nops_a",
    "hblank_int_scx7_nops_b", "hblank_scx2_if_a", "hblank_scx3_if_a", "hblank_scx3_if_c",
    "hblank_scx3_if_d", "hblank_scx3_int_a", "hblank_scx3_int_b", "int_hblank_halt_bug_a",
    "int_hblank_halt_bug_b", "int_hblank_halt_scx1", "int_hblank_halt_scx2", "int_hblank_halt_scx5",
    "int_hblank_halt_scx6", "int_hblank_incs_scx0", "int_hblank_incs_scx1", "int_hblank_incs_scx2",
    "int_hblank_incs_scx3", "int_hblank_incs_scx4", "int_hblank_incs_scx5", "int_hblank_incs_scx6",
    "int_hblank_incs_scx7", "int_hblank_nops_scx0", "int_hblank_nops_scx1", "int_hblank_nops_scx2",
    "int_hblank_nops_scx3", "int_hblank_nops_scx4", "int_hblank_nops_scx5", "int_hblank_nops_scx6",
    "int_hblank_nops_scx7", "int_oam_halt", "int_timer_halt", "int_timer_halt_div_a",
    "int_timer_halt_div_b", "int_timer_nops_div_b", "is_if_set_during_ime0", "lcdon_halt_to_vblank_int_b",
    "lcdon_nops_to_vblank_int_b", "lcdon_to_if_oam_a", "lcdon_to_if_oam_b", "lcdon_to_ly1_a",
    "lcdon_to_ly1_b", "lcdon_to_ly2_a", "lcdon_to_ly2_b", "lcdon_to_ly3_a",
    "lcdon_to_ly3_b", "lcdon_to_oam_unlock_a", "lcdon_to_oam_unlock_b", "lcdon_to_oam_unlock_c",
    "lcdon_to_oam_unlock_d", "lcdon_to_stat0_a", "lcdon_to_stat0_b", "lcdon_to_stat0_c",
    "lcdon_to_stat0_d", "lcdon_to_stat1_a", "lcdon_to_stat1_b", "lcdon_to_stat1_d",
    "lcdon_to_stat1_e", "lcdon_to_stat2_a", "lcdon_to_stat2_b", "lcdon_to_stat2_c",
    "lcdon_to_stat2_d", "lcdon_to_stat3_a", "lcdon_to_stat3_b", "lcdon_to_stat3_c",
    "lcdon_to_stat3_d", "line_144_oam_int_a", "line_153_ly_a", "line_153_ly_b",
    "line_153_ly_e", "line_153_ly_f", "line_153_lyc0_stat_timing_a", "line_153_lyc0_stat_timing_b",
    "line_153_lyc0_stat_timing_c", "line_153_lyc0_stat_timing_f", "line_153_lyc0_stat_timing_g", "line_153_lyc0_stat_timing_h",
    "line_153_lyc0_stat_timing_i", "line_153_lyc0_stat_timing_j", "line_153_lyc0_stat_timing_k", "line_153_lyc0_stat_timing_l",
    "line_153_lyc0_stat_timing_m", "line_153_lyc0_stat_timing_n", "line_153_lyc153_stat_timing_a", "line_153_lyc153_stat_timing_b",
    "line_153_lyc153_stat_timing_e", "line_153_lyc153_stat_timing_f", "line_153_lyc_a", "line_153_lyc_b",
    "line_153_lyc_int_b", "line_65_ly", "lyc1_int_halt_b", "lyc1_int_if_edge_a",
    "lyc1_int_if_edge_b", "lyc1_int_if_edge_c", "lyc1_int_if_edge_d", "lyc1_int_nops_b",
    "lyc1_write_timing_a", "lyc1_write_timing_c", "lyc2_int_halt_b", "lyc_int_halt_b",
    "mbc1_ram_banks", "mbc1_rom_banks", "oam_int_if_edge_a", "oam_int_if_edge_b",
    "oam_int_if_edge_c", "oam_int_if_edge_d", "oam_int_if_level_c", "oam_read_l0_a",
    "oam_read_l0_b", "oam_read_l0_c", "oam_read_l0_d", "oam_read_l1_a",
    "oam_read_l1_b", "oam_read_l1_c", "oam_read_l1_d", "oam_read_l1_e",
    "oam_read_l1_f", "oam_write_l0_a", "oam_write_l0_b", "oam_write_l0_c",
    "oam_write_l0_d", "oam_write_l0_e", "oam_write_l1_a", "oam_write_l1_b",
    "oam_write_l1_c", "oam_write_l1_d", "oam_write_l1_e", "oam_write_l1_f",
    "poweron_bgp_000", "poweron_div_000", "poweron_div_004", "poweron_div_005",
    "poweron_dma_000", "poweron_if_000", "poweron_joy_000", "poweron_lcdc_000",
    "poweron_ly_119", "poweron_ly_120", "poweron_ly_233", "poweron_ly_234",
    "poweron_lyc_000", "poweron_oam_000", "poweron_oam_005", "poweron_oam_006",
    "poweron_oam_069", "poweron_oam_070", "poweron_oam_119", "poweron_oam_120",
    "poweron_oam_121", "poweron_oam_183", "poweron_oam_184", "poweron_oam_233",
    "poweron_oam_234", "poweron_oam_235", "poweron_obp0_000", "poweron_obp1_000",
    "poweron_sb_000", "poweron_sc_000", "poweron_scx_000", "poweron_scy_000",
    "poweron_stat_006", "poweron_stat_007", "poweron_stat_026", "poweron_stat_027",
    "poweron_stat_069", "poweron_stat_070", "poweron_stat_119", "poweron_stat_120",
    "poweron_stat_121", "poweron_stat_140", "poweron_stat_141", "poweron_stat_183",
    "poweron_stat_184", "poweron_stat_234", "poweron_stat_235", "poweron_tac_000",
    "poweron_tima_000", "poweron_tma_000", "poweron_vram_000", "poweron_vram_025",
    "poweron_vram_026", "poweron_vram_069", "poweron_vram_070", "poweron_vram_139",
    "poweron_vram_140", "poweron_vram_183", "poweron_vram_184", "poweron_wx_000",
    "poweron_wy_000", "ppu_sprite0_scx0_a", "ppu_sprite0_scx0_b", "ppu_sprite0_scx1_a",
    "ppu_sprite0_scx1_b", "ppu_sprite0_scx2_a", "ppu_sprite0_scx2_b", "ppu_sprite0_scx3_a",
    "ppu_sprite0_scx3_b", "ppu_sprite0_scx4_a", "ppu_sprite0_scx4_b", "ppu_sprite0_scx5_a",
    "ppu_sprite0_scx5_b", "ppu_sprite0_scx6_a", "ppu_sprite0_scx6_b", "ppu_sprite0_scx7_a",
    "ppu_sprite0_scx7_b", "sprite4_0_a", "sprite4_0_b", "sprite4_1_a",
    "sprite4_1_b", "sprite4_2_a", "sprite4_2_b", "sprite4_3_a",
    "sprite4_3_b", "sprite4_4_a", "sprite4_4_b", "sprite4_5_a",
    "sprite4_5_b", "sprite4_6_a", "sprite4_6_b", "sprite4_7_a",
    "sprite4_7_b", "sprite_0_a", "sprite_0_b", "sprite_1_a",
    "sprite_1_b", "stat_write_glitch_l0_a", "stat_write_glitch_l0_b", "stat_write_glitch_l143_a",
    "stat_write_glitch_l143_b", "stat_write_glitch_l143_c", "stat_write_glitch_l143_d", "stat_write_glitch_l154_a",
    "stat_write_glitch_l154_b", "stat_write_glitch_l1_a", "stat_write_glitch_l1_b", "stat_write_glitch_l1_c",
    "timer_div_phase_c", "timer_div_phase_d", "timer_tima_inc_256k_a", "timer_tima_inc_256k_b",
    "timer_tima_inc_256k_c", "timer_tima_inc_256k_d", "timer_tima_inc_256k_e", "timer_tima_inc_256k_f",
    "timer_tima_inc_256k_g", "timer_tima_inc_256k_h", "timer_tima_inc_256k_i", "timer_tima_inc_256k_j",
    "timer_tima_inc_256k_k", "timer_tima_inc_64k_a", "timer_tima_inc_64k_b", "timer_tima_inc_64k_c",
    "timer_tima_inc_64k_d", "timer_tima_phase_a", "timer_tima_phase_b", "timer_tima_phase_c",
    "timer_tima_phase_d", "timer_tima_phase_e", "timer_tima_phase_f", "timer_tima_phase_g",
    "timer_tima_phase_h", "timer_tima_phase_i", "timer_tima_phase_j", "timer_tima_reload_256k_a",
    "timer_tima_reload_256k_b", "timer_tima_reload_256k_c", "timer_tima_reload_256k_d", "timer_tima_reload_256k_e",
    "timer_tima_reload_256k_f", "timer_tima_reload_256k_g", "timer_tima_reload_256k_h", "timer_tima_reload_256k_i",
    "timer_tima_reload_256k_j", "timer_tima_reload_256k_k", "timer_tima_write_a", "timer_tima_write_b",
    "timer_tima_write_c", "timer_tima_write_d", "timer_tima_write_e", "timer_tima_write_f",
    "timer_tma_write_a", "timer_tma_write_b", "vblank2_int_halt_b", "vblank2_int_if_a",
    "vblank2_int_if_b", "vblank2_int_if_c", "vblank2_int_if_d", "vblank2_int_nops_b",
    "vblank_int_halt_b", "vblank_int_if_a", "vblank_int_if_b", "vblank_int_if_c",
    "vblank_int_if_d", "vblank_int_nops_b", "vram_read_l0_a", "vram_read_l0_b",
    "vram_read_l0_c", "vram_read_l0_d", "vram_read_l1_a", "vram_read_l1_b",
    "vram_read_l1_c", "vram_read_l1_d", "vram_write_l0_a", "vram_write_l0_b",
    "vram_write_l0_c", "vram_write_l0_d", "vram_write_l1_a", "vram_write_l1_b",
    "vram_write_l1_c", "vram_write_l1_d", "win0_a", "win0_b",
    "win0_scx3_a", "win0_scx3_b", "win10_a", "win10_b",
    "win10_scx3_a", "win10_scx3_b", "win11_a", "win11_b",
    "win12_a", "win12_b", "win13_a", "win13_b",
    "win14_a", "win14_b", "win15_a", "win15_b",
    "win1_a", "win1_b", "win2_a", "win2_b",
    "win3_a", "win3_b", "win4_a", "win4_b",
    "win5_a", "win5_b", "win6_a", "win6_b",
    "win7_a", "win7_b", "win8_a", "win8_b",
    "win9_a", "win9_b",
];

/// Blargg's interrupt-time and halt-bug tests (c-sp/game-boy-test-roms v7.0). These two do
/// NOT report through the serial port (both timed out empty there): they draw their result
/// on the screen, so they are screenshot cases in `screenshot_suites_report` below, not
/// `blargg()` runs. `halt_bug.gb` was fetched by scripts/fetch-test-roms.sh all along but
/// never wired until P5 Item 3.

/// How a screenshot test is run: which console, and which drawer. The reference PNGs are
/// 160x144 in the colours the suite's howto names (DMG greys, CGB 5-bit channels expanded
/// with (X << 3) | (X >> 2), CGB-compatibility palettes for non-colour mode), so the frame
/// is compared pixel for pixel against the decoded PNG.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Shot {
    Dmg,
    Cgb,
    /// A Color in Game Boy mode (what the AGE "ncm" ROMs and the -cgb screenshots of the
    /// DMG suites were photographed on).
    Ncm,
}

/// Runs a ROM for `frames` frames and compares the framebuffer to a reference PNG.
/// Returns the number of differing pixels and an FNV hash of the frame (the hash says
/// whether the picture was still changing: run twice and compare). None when the ROM or
/// the PNG is missing.
fn shot(name: &str, png: &str, frames: u32, mode: Shot, exact: bool) -> Option<(usize, u64)> {
    let data = rom(name)?;
    let want = png_rgb(&rom(png)?);
    if want.len() != 160 * 144 {
        eprintln!("shot {name}: reference {png} is not 160x144");
        return None;
    }
    let mut gb = match mode {
        Shot::Dmg => GameBoy::new_mode(data, false).ok()?,
        Shot::Cgb => GameBoy::new_mode(data, true).ok()?,
        Shot::Ncm => GameBoy::with_model(data, Model::Cgb, None).ok()?,
    };
    gb.bus.ppu.exact = exact;
    for _ in 0..frames {
        gb.run_frame();
    }
    let wrong = gb.bus.ppu.frame.iter().zip(&want).filter(|(g, w)| g != w).count();
    if wrong > 0 {
        let first: Vec<(usize, usize, u32, u32)> = gb.bus.ppu.frame.iter().zip(&want).enumerate()
            .filter(|(_, (g, w))| g != w).take(5)
            .map(|(i, (g, w))| (i % 160, i / 160, *g, *w)).collect();
        eprintln!("shot {name}: first differing (x, y, got, want): {first:06X?}");
    }
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for p in &gb.bus.ppu.frame {
        h = (h ^ u64::from(*p)).wrapping_mul(0x100_0000_01b3);
    }
    Some((wrong, h))
}

/// One screenshot case: label, ROM, reference PNG, frames, mode, drawer.
struct ShotCase {
    label: &'static str,
    rom: &'static str,
    png: &'static str,
    frames: u32,
    mode: Shot,
    exact: bool,
}

/// The screenshot suites from c-sp/game-boy-test-roms v7.0: cgb-acid-hell, BullyGB,
/// TurtleTests, Scribbltests, Strikethrough, mbc3-tester and the AGE m3 picture tests.
/// Frame counts come from each suite's howto (Bully/Turtle/Strikethrough half a second,
/// Scribbltests 10 frames except statcount-auto's 270, mbc3-tester 40, acid-style 120).
/// Reports every case; the passing set is pinned in SCREENSHOT_PASSING below.
#[test]
fn screenshot_suites_report() {
    let cases = [
        ShotCase { label: "blargg:interrupt_time:dmg", rom: "blargg/interrupt_time/interrupt_time.gb", png: "blargg/interrupt_time/interrupt_time-dmg.png", frames: 120, mode: Shot::Dmg, exact: false },
        ShotCase { label: "blargg:interrupt_time:cgb", rom: "blargg/interrupt_time/interrupt_time.gb", png: "blargg/interrupt_time/interrupt_time-cgb.png", frames: 120, mode: Shot::Cgb, exact: false },
        ShotCase { label: "blargg:halt_bug:dmg", rom: "blargg/halt_bug.gb", png: "blargg/halt_bug-dmg-cgb.png", frames: 120, mode: Shot::Dmg, exact: false },
        ShotCase { label: "blargg:halt_bug:cgb", rom: "blargg/halt_bug.gb", png: "blargg/halt_bug-dmg-cgb.png", frames: 120, mode: Shot::Cgb, exact: false },
        ShotCase { label: "cgb-acid-hell:cgb", rom: "cgb-acid-hell/cgb-acid-hell.gbc", png: "cgb-acid-hell/cgb-acid-hell.png", frames: 120, mode: Shot::Cgb, exact: false },
        ShotCase { label: "cgb-acid-hell:cgb:exact", rom: "cgb-acid-hell/cgb-acid-hell.gbc", png: "cgb-acid-hell/cgb-acid-hell.png", frames: 120, mode: Shot::Cgb, exact: true },
        ShotCase { label: "bully:cgb", rom: "bully/bully.gb", png: "bully/bully.png", frames: 30, mode: Shot::Cgb, exact: false },
        ShotCase { label: "bully:ncm", rom: "bully/bully.gb", png: "bully/bully.png", frames: 30, mode: Shot::Ncm, exact: false },
        ShotCase { label: "bully:dmg", rom: "bully/bully.gb", png: "bully/bully.png", frames: 30, mode: Shot::Dmg, exact: false },
        ShotCase { label: "turtle:window_y_trigger:dmg", rom: "turtle-tests/window_y_trigger/window_y_trigger.gb", png: "turtle-tests/window_y_trigger/window_y_trigger.png", frames: 30, mode: Shot::Dmg, exact: false },
        ShotCase { label: "turtle:window_y_trigger_wx_offscreen:dmg", rom: "turtle-tests/window_y_trigger_wx_offscreen/window_y_trigger_wx_offscreen.gb", png: "turtle-tests/window_y_trigger_wx_offscreen/window_y_trigger_wx_offscreen.png", frames: 30, mode: Shot::Dmg, exact: false },
        ShotCase { label: "scribbl:lycscy:dmg", rom: "scribbltests/lycscy/lycscy.gb", png: "scribbltests/lycscy/lycscy-cgb-dmg.png", frames: 10, mode: Shot::Dmg, exact: false },
        ShotCase { label: "scribbl:lycscy:dmg:exact", rom: "scribbltests/lycscy/lycscy.gb", png: "scribbltests/lycscy/lycscy-cgb-dmg.png", frames: 10, mode: Shot::Dmg, exact: true },
        ShotCase { label: "scribbl:lycscx:dmg", rom: "scribbltests/lycscx/lycscx.gb", png: "scribbltests/lycscx/lycscx-cgb-dmg.png", frames: 10, mode: Shot::Dmg, exact: false },
        ShotCase { label: "scribbl:lycscx:dmg:exact", rom: "scribbltests/lycscx/lycscx.gb", png: "scribbltests/lycscx/lycscx-cgb-dmg.png", frames: 10, mode: Shot::Dmg, exact: true },
        ShotCase { label: "scribbl:statcount-auto:dmg", rom: "scribbltests/statcount/statcount-auto.gb", png: "scribbltests/statcount/statcount_auto-cgb-dmg.png", frames: 270, mode: Shot::Dmg, exact: false },
        ShotCase { label: "scribbl:statcount-auto:dmg:exact", rom: "scribbltests/statcount/statcount-auto.gb", png: "scribbltests/statcount/statcount_auto-cgb-dmg.png", frames: 270, mode: Shot::Dmg, exact: true },
        ShotCase { label: "scribbl:palettely:dmg", rom: "scribbltests/palettely/palettely.gb", png: "scribbltests/palettely/palettely-dmg.png", frames: 10, mode: Shot::Dmg, exact: false },
        ShotCase { label: "scribbl:palettely:dmg:exact", rom: "scribbltests/palettely/palettely.gb", png: "scribbltests/palettely/palettely-dmg.png", frames: 10, mode: Shot::Dmg, exact: true },
        ShotCase { label: "scribbl:palettely:cgb", rom: "scribbltests/palettely/palettely.gb", png: "scribbltests/palettely/palettely-cgb.png", frames: 10, mode: Shot::Ncm, exact: false },
        ShotCase { label: "scribbl:palettely:cgb:exact", rom: "scribbltests/palettely/palettely.gb", png: "scribbltests/palettely/palettely-cgb.png", frames: 10, mode: Shot::Ncm, exact: true },
        ShotCase { label: "scribbl:scxly:dmg", rom: "scribbltests/scxly/scxly.gb", png: "scribbltests/scxly/scxly-dmg.png", frames: 10, mode: Shot::Dmg, exact: false },
        ShotCase { label: "scribbl:scxly:dmg:exact", rom: "scribbltests/scxly/scxly.gb", png: "scribbltests/scxly/scxly-dmg.png", frames: 10, mode: Shot::Dmg, exact: true },
        ShotCase { label: "scribbl:scxly:cgb", rom: "scribbltests/scxly/scxly.gb", png: "scribbltests/scxly/scxly-cgb.png", frames: 10, mode: Shot::Ncm, exact: false },
        ShotCase { label: "scribbl:scxly:cgb:exact", rom: "scribbltests/scxly/scxly.gb", png: "scribbltests/scxly/scxly-cgb.png", frames: 10, mode: Shot::Ncm, exact: true },
        ShotCase { label: "strikethrough:dmg", rom: "strikethrough/strikethrough.gb", png: "strikethrough/strikethrough-dmg.png", frames: 30, mode: Shot::Dmg, exact: false },
        ShotCase { label: "strikethrough:cgb", rom: "strikethrough/strikethrough.gb", png: "strikethrough/strikethrough-cgb.png", frames: 30, mode: Shot::Cgb, exact: false },
        ShotCase { label: "mbc3-tester:dmg", rom: "mbc3-tester/mbc3-tester.gb", png: "mbc3-tester/mbc3-tester-dmg.png", frames: 40, mode: Shot::Dmg, exact: false },
        ShotCase { label: "mbc3-tester:cgb", rom: "mbc3-tester/mbc3-tester.gb", png: "mbc3-tester/mbc3-tester-cgb.png", frames: 40, mode: Shot::Ncm, exact: false },
        ShotCase { label: "age:m3-bg-bgp:dmg", rom: "age-test-roms/m3-bg-bgp/m3-bg-bgp.gb", png: "age-test-roms/m3-bg-bgp/m3-bg-bgp-dmgC.png", frames: 120, mode: Shot::Dmg, exact: false },
        ShotCase { label: "age:m3-bg-bgp:dmg:exact", rom: "age-test-roms/m3-bg-bgp/m3-bg-bgp.gb", png: "age-test-roms/m3-bg-bgp/m3-bg-bgp-dmgC.png", frames: 120, mode: Shot::Dmg, exact: true },
        ShotCase { label: "age:m3-bg-bgp:ncm", rom: "age-test-roms/m3-bg-bgp/m3-bg-bgp.gb", png: "age-test-roms/m3-bg-bgp/m3-bg-bgp-ncmBC.png", frames: 120, mode: Shot::Ncm, exact: false },
        ShotCase { label: "age:m3-bg-bgp:ncm:exact", rom: "age-test-roms/m3-bg-bgp/m3-bg-bgp.gb", png: "age-test-roms/m3-bg-bgp/m3-bg-bgp-ncmBC.png", frames: 120, mode: Shot::Ncm, exact: true },
        ShotCase { label: "age:m3-bg-lcdc:dmg", rom: "age-test-roms/m3-bg-lcdc/m3-bg-lcdc.gb", png: "age-test-roms/m3-bg-lcdc/m3-bg-lcdc-dmgC.png", frames: 120, mode: Shot::Dmg, exact: false },
        ShotCase { label: "age:m3-bg-lcdc:dmg:exact", rom: "age-test-roms/m3-bg-lcdc/m3-bg-lcdc.gb", png: "age-test-roms/m3-bg-lcdc/m3-bg-lcdc-dmgC.png", frames: 120, mode: Shot::Dmg, exact: true },
        ShotCase { label: "age:m3-bg-lcdc:cgb", rom: "age-test-roms/m3-bg-lcdc/m3-bg-lcdc.gb", png: "age-test-roms/m3-bg-lcdc/m3-bg-lcdc-cgbBCE.png", frames: 120, mode: Shot::Cgb, exact: false },
        ShotCase { label: "age:m3-bg-lcdc:cgb:exact", rom: "age-test-roms/m3-bg-lcdc/m3-bg-lcdc.gb", png: "age-test-roms/m3-bg-lcdc/m3-bg-lcdc-cgbBCE.png", frames: 120, mode: Shot::Cgb, exact: true },
        ShotCase { label: "age:m3-bg-lcdc:ds:cgb", rom: "age-test-roms/m3-bg-lcdc/m3-bg-lcdc-ds.gb", png: "age-test-roms/m3-bg-lcdc/m3-bg-lcdc-ds-cgbBCE.png", frames: 120, mode: Shot::Cgb, exact: false },
        ShotCase { label: "age:m3-bg-lcdc:ds:cgb:exact", rom: "age-test-roms/m3-bg-lcdc/m3-bg-lcdc-ds.gb", png: "age-test-roms/m3-bg-lcdc/m3-bg-lcdc-ds-cgbBCE.png", frames: 120, mode: Shot::Cgb, exact: true },
        ShotCase { label: "age:m3-bg-lcdc:nocgb:ncm", rom: "age-test-roms/m3-bg-lcdc/m3-bg-lcdc-nocgb.gb", png: "age-test-roms/m3-bg-lcdc/m3-bg-lcdc-nocgb-ncmBCE.png", frames: 120, mode: Shot::Ncm, exact: false },
        ShotCase { label: "age:m3-bg-lcdc:nocgb:ncm:exact", rom: "age-test-roms/m3-bg-lcdc/m3-bg-lcdc-nocgb.gb", png: "age-test-roms/m3-bg-lcdc/m3-bg-lcdc-nocgb-ncmBCE.png", frames: 120, mode: Shot::Ncm, exact: true },
        ShotCase { label: "age:m3-bg-scx:dmg", rom: "age-test-roms/m3-bg-scx/m3-bg-scx.gb", png: "age-test-roms/m3-bg-scx/m3-bg-scx-dmgC.png", frames: 120, mode: Shot::Dmg, exact: false },
        ShotCase { label: "age:m3-bg-scx:dmg:exact", rom: "age-test-roms/m3-bg-scx/m3-bg-scx.gb", png: "age-test-roms/m3-bg-scx/m3-bg-scx-dmgC.png", frames: 120, mode: Shot::Dmg, exact: true },
        ShotCase { label: "age:m3-bg-scx:cgb", rom: "age-test-roms/m3-bg-scx/m3-bg-scx.gb", png: "age-test-roms/m3-bg-scx/m3-bg-scx-cgbBCE.png", frames: 120, mode: Shot::Cgb, exact: false },
        ShotCase { label: "age:m3-bg-scx:cgb:exact", rom: "age-test-roms/m3-bg-scx/m3-bg-scx.gb", png: "age-test-roms/m3-bg-scx/m3-bg-scx-cgbBCE.png", frames: 120, mode: Shot::Cgb, exact: true },
        ShotCase { label: "age:m3-bg-scx:ds:cgb", rom: "age-test-roms/m3-bg-scx/m3-bg-scx-ds.gb", png: "age-test-roms/m3-bg-scx/m3-bg-scx-ds-cgbBCE.png", frames: 120, mode: Shot::Cgb, exact: false },
        ShotCase { label: "age:m3-bg-scx:ds:cgb:exact", rom: "age-test-roms/m3-bg-scx/m3-bg-scx-ds.gb", png: "age-test-roms/m3-bg-scx/m3-bg-scx-ds-cgbBCE.png", frames: 120, mode: Shot::Cgb, exact: true },
        ShotCase { label: "age:m3-bg-scx:nocgb:ncm", rom: "age-test-roms/m3-bg-scx/m3-bg-scx-nocgb.gb", png: "age-test-roms/m3-bg-scx/m3-bg-scx-nocgb-ncmBCE.png", frames: 120, mode: Shot::Ncm, exact: false },
        ShotCase { label: "age:m3-bg-scx:nocgb:ncm:exact", rom: "age-test-roms/m3-bg-scx/m3-bg-scx-nocgb.gb", png: "age-test-roms/m3-bg-scx/m3-bg-scx-nocgb-ncmBCE.png", frames: 120, mode: Shot::Ncm, exact: true },
    ];
    let mut pass = Vec::new();
    let mut fail = Vec::new();
    let mut skipped = Vec::new();
    for c in &cases {
        match shot(c.rom, c.png, c.frames, c.mode, c.exact) {
            Some((0, h)) => {
                eprintln!("  PASS {} hash {h:#018x}", c.label);
                pass.push(c.label);
            }
            Some((wrong, h)) => {
                eprintln!("  FAIL {}: {wrong} px differ, hash {h:#018x}", c.label);
                fail.push(c.label);
            }
            None => {
                eprintln!("  SKIP {}", c.label);
                skipped.push(c.label);
            }
        }
    }
    eprintln!(
        "SCREENSHOT: {} passed, {} failed, {} skipped, of {}",
        pass.len(), fail.len(), skipped.len(), cases.len()
    );
    // A case whose ROM or reference picture is not in test-roms/ cannot be judged, so it is not
    // a regression: fetch-test-roms.sh fetches none of these suites (their pictures were
    // photographed on real hardware), and ios.yml's own note says a missing ROM means those tests
    // report SKIPPED and the unit tests still run. A pinned case that DID run and did not match
    // is still an assert, and so is a pinned label that has left the list below.
    for must in SCREENSHOT_PASSING {
        if skipped.iter().any(|s| s == must) {
            eprintln!("  NOT JUDGED {must}: its ROM or reference picture is not in test-roms/");
            continue;
        }
        assert!(pass.iter().any(|p| p == must), "screenshot regressed: {must}");
    }
}

/// The screenshot cases that match their reference pixel for pixel. This list only ever
/// grows. Pinned P5 Item 3 (2026-11-25) from the probe runs: Blargg's two screen tests in
/// both modes; both TurtleTests; Scribbltests lycscy/lycscx/statcount-auto/palettely/
/// scxly on DMG (both drawers) plus palettely in CGB-compat mode; mbc3-tester on DMG;
/// and the AGE m3 pictures on the exact drawer (the standard drawer cannot do their
/// mid-scanline palette/register changes: m3-bg-lcdc is 5120 px off there, m3-bg-bgp
/// all white). Not pinned: cgb-acid-hell (2 px: one yellow pixel one line low at
/// (50,44)/(50,45), both drawers), BullyGB (fails "Invalid initial DIV" in CGB/NCM,
/// "Uninitialized RAM not randomized" on DMG), strikethrough (55 px: one line at y=44),
/// scxly in CGB-compat mode (the game's own green palette is not reproduced there),
/// mbc3-tester in CGB-compat mode (the picked compat palette's blue is 6 where the
/// reference has 9), m3-bg-bgp on DMG exact (2 px on the left edge), m3-bg-lcdc-ds on
/// the exact drawer (2304 px in double speed), and the m3 standard-drawer runs.
const SCREENSHOT_PASSING: &[&str] = &[
    "blargg:interrupt_time:dmg",
    "blargg:interrupt_time:cgb",
    "blargg:halt_bug:dmg",
    "blargg:halt_bug:cgb",
    "turtle:window_y_trigger:dmg",
    "turtle:window_y_trigger_wx_offscreen:dmg",
    "scribbl:lycscy:dmg",
    "scribbl:lycscy:dmg:exact",
    "scribbl:lycscx:dmg",
    "scribbl:lycscx:dmg:exact",
    "scribbl:statcount-auto:dmg",
    "scribbl:statcount-auto:dmg:exact",
    "scribbl:palettely:dmg",
    "scribbl:palettely:dmg:exact",
    "scribbl:palettely:cgb",
    "scribbl:palettely:cgb:exact",
    "scribbl:scxly:dmg",
    "scribbl:scxly:dmg:exact",
    "mbc3-tester:dmg",
    "age:m3-bg-bgp:ncm:exact",
    "age:m3-bg-lcdc:dmg:exact",
    "age:m3-bg-lcdc:cgb:exact",
    "age:m3-bg-lcdc:nocgb:ncm:exact",
    "age:m3-bg-scx:dmg:exact",
    "age:m3-bg-scx:cgb:exact",
    "age:m3-bg-scx:ds:cgb:exact",
    "age:m3-bg-scx:nocgb:ncm:exact",
];

/// AGE test ROMs (c-sp/game-boy-test-roms v7.0, age-test-roms/): Mooneye-style, ending in
/// `LD B,B` with the Fibonacci registers on success. The file name says which consoles it
/// was verified on: `dmgC` a DMG, `cgbBCE`/`cgbBC`/`cgbE` a Color in colour mode, `ncm`
/// a Color in Game Boy mode, `ds` a ROM that switches to double speed itself (run as a
/// Color). A name with two devices (e.g. `dmgC-cgbBCE`) is run in both modes.
fn age_modes(stem: &str) -> Vec<Shot> {
    let mut modes = Vec::new();
    if stem.contains("dmgC") {
        modes.push(Shot::Dmg);
    }
    if stem.contains("ncm") {
        modes.push(Shot::Ncm);
    } else if stem.contains("cgb") {
        modes.push(Shot::Cgb);
    }
    modes
}

/// Runs one AGE ROM in one mode, Mooneye-style, with a 600-frame cap. Ok(true) passed,
/// Ok(false) failed (registers or a lock), Err timed out / never finished.
fn age_run(name: &str, mode: Shot) -> Result<bool, String> {
    let Some(data) = rom(name) else { return Err("missing".into()) };
    let mut gb = match mode {
        Shot::Dmg => GameBoy::new_mode(data, false).map_err(|e| e.to_string())?,
        Shot::Cgb => GameBoy::new_mode(data, true).map_err(|e| e.to_string())?,
        Shot::Ncm => GameBoy::with_model(data, Model::Cgb, None).map_err(|e| e.to_string())?,
    };
    for _ in 0..600 {
        gb.run_frame();
        if gb.cpu.ld_b_b {
            let c = &gb.cpu;
            return Ok([c.b, c.c, c.d, c.e, c.h, c.l] == [3, 5, 8, 13, 21, 34]);
        }
        if gb.cpu.locked {
            return Ok(false);
        }
    }
    Err("timed out".into())
}

/// AGE, all of it: every auto-verifiable ROM in every mode its name claims. The m3
/// picture ROMs are screenshot cases in `screenshot_suites_report`, not here.
#[test]
fn age_report() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test-roms/age-test-roms");
    if !dir.exists() {
        eprintln!("SKIPPED: AGE not fetched (run scripts/fetch-test-roms.sh)");
        return;
    }
    let mut names: Vec<String> = Vec::new();
    let mut stack = vec![dir.clone()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "gb") {
                names.push(p.strip_prefix(&dir).unwrap().to_string_lossy().to_string());
            }
        }
    }
    names.sort();
    let mut pass = Vec::new();
    let mut fail = Vec::new();
    let mut timeout = Vec::new();
    for n in &names {
        let stem = n.trim_end_matches(".gb");
        if stem.contains("m3-bg-") {
            continue;
        }
        for mode in age_modes(stem) {
            let tag = match mode { Shot::Dmg => "dmg", Shot::Cgb => "cgb", Shot::Ncm => "ncm" };
            let label = format!("{tag}:{n}");
            match age_run(&format!("age-test-roms/{n}"), mode) {
                Ok(true) => pass.push(label),
                Ok(false) => fail.push(label),
                Err(e) => timeout.push(format!("{label} ({e})")),
            }
        }
    }
    eprintln!("AGE: {} passed, {} failed, {} timed out/missing", pass.len(), fail.len(), timeout.len());
    for p in &pass {
        eprintln!("  PASS {p}");
    }
    for f in &fail {
        eprintln!("  FAIL {f}");
    }
    for f in &timeout {
        eprintln!("  TIMEOUT {f}");
    }
    assert_eq!(names.len(), 47, "the AGE set changed; re-pin AGE_PASSING");
    for must in AGE_PASSING {
        assert!(pass.iter().any(|p| p == must), "AGE regressed: {must}");
    }
}

/// The AGE (rom, mode) pairs that pass. This list only ever grows. Pinned P5 Item 3
/// (2026-11-25): 5 of 48 runs — the two halt tests in both modes, and the CGB
/// speed-switch DIV test. The other 43 are real failures of timing edge cases (STAT
/// mode timing, LY reads, OAM/VRAM read-write timing, speed-switch interrupts), each
/// ending in `LD B,B` with non-Fibonacci registers or a lock; see the probe log. They
/// are the same class of gap as the Mooneye OAM 3/8: mid-scanline/frame alignment a few
/// dots out, not a harness problem.
const AGE_PASSING: &[&str] = &[
    "cgb:halt/ei-halt-dmgC-cgbBCE.gb",
    "cgb:halt/halt-prefetch-dmgC-cgbBCE.gb",
    "cgb:speed-switch/spsw-div-cgbBCE.gb",
    "dmg:halt/ei-halt-dmgC-cgbBCE.gb",
    "dmg:halt/halt-prefetch-dmgC-cgbBCE.gb",
];

/// Telling LYs (little-things-gb): the joypad interrupt must fire at varied LY values, or
/// the program can tell an emulator was used. All eight buttons are pressed, one after
/// another, each at a different line of the frame, and the result screen must be the
/// reference "Pass!" one. Button bits for `set_buttons`: 0 A, 1 B, 2 Select, 3 Start,
/// 4 Right, 5 Left, 6 Up, 7 Down.
///
/// NOT passing yet (P5 Item 3): with all eight presses landing at LY 10, 30, 50, 70, 90,
/// 110, 130, 150 (verified by probe: each press's interrupt is serviced, IF clears), the
/// program shows its "Johny, Johny yes papa" joke screen instead of the Pass screen, and
/// further presses just add more lines of the rhyme ("running copies?", "telling LYs?",
/// "open the folder", "here you go", "good boy"). Long holds (a full frame each) do not
/// change it either. So the program is not seeing the spread — either it samples LY
/// somewhere cBoy gets wrong (e.g. it reads LY in the main loop after the interrupt
/// handler returns, at a point the emulator's HALT wake-up shifts), or the harness's
/// interaction differs from a human's in a way that matters here. The test stays
/// report-style (nothing pinned) until that is understood; do not claim it passes.
#[test]
fn telling_lys() {
    let Some(data) = rom("little-things-gb/tellinglys.gb") else { return };
    let mut gb = GameBoy::new(data).unwrap();
    // Let the title screen come up, then press each button at its own line.
    for _ in 0..30 {
        gb.run_frame();
    }
    let buttons: [(u8, u8); 8] = [
        (1, 10), (2, 30), (4, 50), (8, 70), (16, 90), (32, 110), (64, 130), (128, 150),
    ];
    for (bit, want_ly) in buttons {
        // Step until the PPU is on the wanted line, then press and hold briefly.
        for _ in 0..200_000 {
            if gb.bus.ppu.ly == want_ly {
                break;
            }
            gb.cpu.step(&mut gb.bus);
        }
        gb.bus.set_buttons(bit);
        for _ in 0..5000 {
            gb.cpu.step(&mut gb.bus);
        }
        gb.bus.set_buttons(0);
        for _ in 0..5000 {
            gb.cpu.step(&mut gb.bus);
        }
    }
    let want = png_rgb(&rom("little-things-gb/tellinglys-dmg.png").unwrap());
    for _ in 0..120 {
        gb.run_frame();
    }
    let wrong = gb.bus.ppu.frame.iter().zip(&want).filter(|(g, w)| g != w).count();
    eprintln!("TELLINGLYS: {wrong} pixels differ from the Pass screen (0 = pass)");
    if TELLINGLYS_PASSING {
        assert_eq!(wrong, 0, "tellinglys regressed");
    }
}

/// Whether tellinglys matches its Pass screen. False until the program shows Pass.
const TELLINGLYS_PASSING: bool = false;

/// rtc3test (MBC3 RTC): three subtests picked from the start-up menu — A for the basic
/// tests (13 emulated seconds), Down then A for the range tests (8s), Down Down then A
/// for the sub-second writes (26s) — each ending on its reference screenshot. The -dmg
/// references are the DMG-mode pictures, so the ROM is run as a DMG for those (it is
/// CGB-compatible, so `GameBoy::new` would run it in colour mode instead); the -cgb
/// references are the colour-mode pictures.
#[test]
fn rtc3test_report() {
    let cases: [(&str, &[u8], u32, &str, Shot); 6] = [
        ("basic:dmg", &[1], 13 * 60 + 60, "rtc3test/rtc3test-basic-tests-dmg.png", Shot::Dmg),
        ("basic:cgb", &[1], 13 * 60 + 60, "rtc3test/rtc3test-basic-tests-cgb.png", Shot::Cgb),
        ("range:dmg", &[128, 1], 8 * 60 + 60, "rtc3test/rtc3test-range-tests-dmg.png", Shot::Dmg),
        ("range:cgb", &[128, 1], 8 * 60 + 60, "rtc3test/rtc3test-range-tests-cgb.png", Shot::Cgb),
        ("sub-second:dmg", &[128, 128, 1], 26 * 60 + 60, "rtc3test/rtc3test-sub-second-writes-dmg.png", Shot::Dmg),
        ("sub-second:cgb", &[128, 128, 1], 26 * 60 + 60, "rtc3test/rtc3test-sub-second-writes-cgb.png", Shot::Cgb),
    ];
    let mut pass = Vec::new();
    let mut fail = Vec::new();
    for (label, keys, frames, png, mode) in cases {
        let Some(data) = rom("rtc3test/rtc3test.gb") else { return };
        let want = png_rgb(&rom(png).unwrap());
        let mut gb = match mode {
            Shot::Dmg => GameBoy::new_mode(data, false).unwrap(),
            Shot::Cgb => GameBoy::new_mode(data, true).unwrap(),
            Shot::Ncm => GameBoy::with_model(data, Model::Cgb, None).unwrap(),
        };
        for _ in 0..30 {
            gb.run_frame();
        }
        // Press each menu key in turn, holding it for a few frames.
        for &bit in keys {
            gb.bus.set_buttons(bit);
            for _ in 0..5 {
                gb.run_frame();
            }
            gb.bus.set_buttons(0);
            for _ in 0..5 {
                gb.run_frame();
            }
        }
        for _ in 0..frames {
            gb.run_frame();
        }
        let wrong = gb.bus.ppu.frame.iter().zip(&want).filter(|(g, w)| g != w).count();
        if wrong == 0 {
            eprintln!("  PASS rtc3test:{label}");
            pass.push(label);
        } else {
            eprintln!("  FAIL rtc3test:{label}: {wrong} px differ");
            fail.push(label);
        }
    }
    eprintln!("RTC3TEST: {} passed, {} failed, of {}", pass.len(), fail.len(), cases.len());
    for must in RTC3TEST_PASSING {
        assert!(pass.iter().any(|p| p == must), "rtc3test regressed: {must}");
    }
}

/// The rtc3test subtests that match their reference screenshot. This list only ever
/// grows. Pinned P5 Item 3 (2026-11-25): basic and range in both modes. The sub-second
/// writes subtest does not match in either mode yet (see the probe log).
const RTC3TEST_PASSING: &[&str] = &["basic:dmg", "basic:cgb", "range:dmg", "range:cgb"];
