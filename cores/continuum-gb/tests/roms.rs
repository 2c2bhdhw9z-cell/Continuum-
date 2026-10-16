//! Public test ROMs, run headless. Fetch them first with scripts/fetch-test-roms.sh; a missing
//! ROM is reported and the test is skipped (so a fresh checkout without them still builds).

use std::path::PathBuf;

use continuum_gb::gb::{GameBoy, Model};

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

/// One Mealybug Tearoom test on the exact drawer (the one these pictures are for): run to
/// `LD B,B`, compare the screen. Returns where it differs.
fn mealybug(name: &str, cgb: bool) -> Option<MealyDiff> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test-roms/mealybug");
    let data = std::fs::read(dir.join(format!("{name}.gb"))).ok()?;
    let exp = std::fs::read(dir.join("expected").join(if cgb { "CPU CGB C" } else { "DMG-blob" }).join(format!("{name}.png"))).ok()?;
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
        std::fs::write(format!("{d}/{}{name}.ppm", if cgb { "cgb_" } else { "dmg_" }), out).unwrap();
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
