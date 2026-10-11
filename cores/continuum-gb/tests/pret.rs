//! Opt-in: runs Pokemon games built locally from pret's disassemblies (never committed). Set
//! PRET_DIR to a folder holding pokeyellow.gbc, pokered.gbc, pokeblue.gbc, pokecrystal.gbc,
//! pokegold.gbc (any subset). Without it the tests do nothing.

use continuum_gb::gb::{GameBoy, Model};

pub fn save_png(gb: &GameBoy, path: &str) {
    let f = std::fs::File::create(path).unwrap();
    let mut enc = png::Encoder::new(f, 160, 144);
    enc.set_color(png::ColorType::Rgb);
    let mut w = enc.write_header().unwrap();
    let px: Vec<u8> = gb.bus.ppu.frame.iter().flat_map(|p| [(p >> 16) as u8, (p >> 8) as u8, *p as u8]).collect();
    w.write_image_data(&px).unwrap();
}

fn load(name: &str) -> Option<GameBoy> {
    let dir = std::env::var("PRET_DIR").ok()?;
    let data = std::fs::read(format!("{dir}/{name}")).ok()?;
    if let Ok(boot) = std::env::var("PRET_BOOT") {
        let b = std::fs::read(boot).unwrap();
        return Some(GameBoy::with_model(data, Model::Cgb, Some(b)).unwrap());
    }
    Some(GameBoy::new(data).unwrap())
}

/// PRET_INPUT="from-to:mask,from-to:mask" (mask bits: A B Select Start Right Left Up Down), or
/// "tapA:start-end:period" to tap A every `period` frames.
fn buttons_at(f: u32) -> u8 {
    let Ok(spec) = std::env::var("PRET_INPUT") else { return 0 };
    let mut b = 0;
    for part in spec.split(',') {
        let (range, mask) = part.split_once(':').unwrap();
        let (period, mask) = match mask.split_once('/') {
            Some((m, p)) => (p.parse::<u32>().unwrap(), m),
            None => (1, mask),
        };
        let (from, to) = range.split_once('-').unwrap();
        let (from, to): (u32, u32) = (from.parse().unwrap(), to.parse().unwrap());
        if f >= from && f <= to && (period == 1 || (f - from) % period < 3) {
            b |= u8::from_str_radix(mask, 16).unwrap();
        }
    }
    b
}

/// Debug aid: PRET_GAME=pokeyellow.gbc PRET_FRAMES=1500 PRET_OUT=/tmp/y dumps a PNG every 50
/// frames and prints the core's event log and a PC trace when it stops taking VBlanks.
#[test]
fn pret_debug_run() {
    let (Ok(game), Ok(out)) = (std::env::var("PRET_GAME"), std::env::var("PRET_OUT")) else { return };
    let frames: u32 = std::env::var("PRET_FRAMES").ok().and_then(|v| v.parse().ok()).unwrap_or(1500);
    let Some(mut gb) = load(&game) else { return };
    if std::env::var("PRET_SCANLINE").is_ok() {
        gb.bus.ppu.no_render = true;
    }
    if std::env::var("PRET_AGB").is_ok() {
        gb.cpu.b = 0x01;
        gb.cpu.f = 0x00;
    }
    if std::env::var("PRET_WRAMFF").is_ok() {
        for b in gb.bus.wram.iter_mut().skip(0x1000) {
            *b = 0xFF;
        }
    }
    if let Ok(sav) = std::env::var("PRET_SAV") {
        let d = std::fs::read(sav).unwrap();
        let n = d.len().min(gb.bus.cart.ram.len());
        gb.bus.cart.ram[..n].copy_from_slice(&d[..n]);
    }
    std::fs::create_dir_all(&out).unwrap();
    let mut last_vb = 0;
    let mut stuck = 0;
    let mut min_sp = u16::MAX;
    let rewind = std::env::var("PRET_REWIND").is_ok();
    // PRET_RESUME=N: at frame N, save a state, start a NEW machine from the ROM and load it there
    // (what the app's auto-resume does on the next launch).
    let resume: Option<u32> = std::env::var("PRET_RESUME").ok().and_then(|v| v.parse().ok());
    for f in 0..frames {
        if rewind {
            // What a frontend's rewind or run-ahead does: a state every frame, loaded back.
            let st = gb.save_state();
            gb.load_state(&st).unwrap();
        }
        if resume == Some(f) {
            let st = gb.save_state();
            let rom = gb.bus.cart.rom.clone();
            let ram = gb.bus.cart.ram.clone();
            gb = GameBoy::new(rom).unwrap();
            gb.bus.cart.ram = ram;
            gb.load_state(&st).unwrap();
        }
        if std::env::var("PRET_INJECT").is_ok() && f == 100 {
            // The phone's third copy: length $39 written while the Game Freak copy's
            // addresses are still sitting in the registers.
            gb.bus.raw_write(0xFF55, 0x39);
        }
        gb.bus.set_buttons(buttons_at(f));
        gb.run_frame();
        min_sp = min_sp.min(gb.cpu.sp);
        let ptr = gb.bus.raw_read(0xFFF4);
        if ptr != 0 && ptr != 0x42 && ptr != 0x43 {
            eprintln!("[{f}] hLCDCPointer {ptr:02X} ly {}", gb.bus.ppu.ly);
        }
        for l in std::mem::take(&mut gb.bus.events) {
            eprintln!("[{f}] {l}");
        }
        if std::env::var("PRET_TRACE_AT").ok().and_then(|v| v.parse::<u32>().ok()) == Some(f) {
            let mut pcs = Vec::new();
            for _ in 0..std::env::var("PRET_TRACE_N").ok().and_then(|v| v.parse().ok()).unwrap_or(400) {
                pcs.push(format!("{:02X}:{:04X}", gb.bus.cart.rom_bank, gb.cpu.pc));
                gb.cpu.step(&mut gb.bus);
            }
            pcs.dedup();
            if let Ok(bp) = std::env::var("PRET_BREAK") {
                let bps: Vec<u16> = bp.split(',').map(|a| u16::from_str_radix(a, 16).unwrap()).collect();
                let mut hits = 0;
                for _ in 0..3_000_000 {
                    if bps.contains(&gb.cpu.pc) {
                        let c = &gb.cpu;
                        eprintln!("  break {:04X} bank {:02X} a {:02X} f {:02X} bc {:02X}{:02X} de {:02X}{:02X} hl {:02X}{:02X} sp {:04X}",
                            c.pc, gb.bus.cart.rom_bank, c.a, c.f, c.b, c.c, c.d, c.e, c.h, c.l, c.sp);
                        let sp = gb.cpu.sp;
                        let st: Vec<String> = (0..12u16).map(|i| format!("{:04X}", u16::from(gb.bus.raw_read(sp + i * 2)) | u16::from(gb.bus.raw_read(sp + i * 2 + 1)) << 8)).collect();
                        eprintln!("    stack {}", st.join(" "));
                        hits += 1;
                        if hits > 30 { break; }
                    }
                    gb.cpu.step(&mut gb.bus);
                }
            }
            if std::env::var("PRET_VRAM").is_ok() {
                let v = &gb.bus.ppu.vram;
                eprintln!("  vram len {}", v.len());
                eprintln!("  tile 7A b0 {:02X?}", &v[0x17A0..0x17B0]);
                eprintln!("  tile 7A b1 {:02X?}", &v[0x37A0..0x37B0]);
                eprintln!("  map 9800 b0 {:02X?}", &v[0x1800..0x1814]);
                eprintln!("  attr 9800 b1 {:02X?}", &v[0x3800..0x3814]);
                eprintln!("  attr 9900 b1 {:02X?}", &v[0x3900..0x3914]);
                let nz0 = v[..0x1800].iter().filter(|&&b| b != 0).count();
                let nz1 = v[0x2000..0x3800].iter().filter(|&&b| b != 0).count();
                eprintln!("  nonzero tile bytes bank0 {nz0} bank1 {nz1}");
            }
            if std::env::var("PRET_PALS").is_ok() {
                eprintln!("  bgpal {:02X?}", gb.bus.ppu.bg_pal);
                eprintln!("  objpal {:02X?}", gb.bus.ppu.obj_pal);
                eprintln!("  bgp {:02X} obp {:02X} {:02X}", gb.bus.ppu.bgp, gb.bus.ppu.obp0, gb.bus.ppu.obp1);
            }
            if let Ok(peek) = std::env::var("PRET_PEEK") {
                for a in peek.split(',') {
                    let a = u16::from_str_radix(a, 16).unwrap();
                    eprintln!("  [{a:04X}] = {:02X}", gb.bus.raw_read(a));
                }
            }
            eprintln!("[{f}] trace {}", pcs.join(" "));
        }
        if f % 50 == 0 {
            save_png(&gb, &format!("{out}/f{f:05}.png"));
        }
        let vb = gb.bus.irq_taken[0];
        stuck = if vb == last_vb { stuck + 1 } else { 0 };
        last_vb = vb;
        if stuck == 120 {
            let c = &gb.cpu;
            eprintln!("[{f}] STUCK pc {:04X} sp {:04X} ime {} halted {} ie {:02X} if {:02X} lcdc {:02X} stat {:02X} ly {} bank {} irqs {:?}",
                c.pc, c.sp, c.ime, c.halted, gb.bus.ie, gb.bus.iflag, gb.bus.ppu.lcdc, gb.bus.ppu.read_stat(), gb.bus.ppu.ly, gb.bus.cart.rom_bank, gb.bus.irq_taken);
            let mut pcs = Vec::new();
            for _ in 0..60 {
                pcs.push(format!("{:04X}", gb.cpu.pc));
                gb.cpu.step(&mut gb.bus);
            }
            eprintln!("  trace {}", pcs.join(" "));
            save_png(&gb, &format!("{out}/stuck{f}.png"));
        }
    }
    eprintln!("ran; vblanks {:?} min_sp {min_sp:04X} hlcd {:02X}", gb.bus.irq_taken, gb.bus.raw_read(0xFFF4));
}

/// Returns the longest run of frames without a VBlank interrupt, and the most colours seen on any
/// frame sampled in the last 3000.
fn run_script(gb: &mut GameBoy, frames: u32, script: &[(u32, u32, u8, u32)]) -> (u32, usize) {
    let mut best = 0;
    let mut stuck_max = 0;
    let mut stuck = 0;
    let mut last = gb.bus.irq_taken[0];
    for f in 0..frames {
        let mut b = 0;
        for &(from, to, mask, period) in script {
            if f >= from && f <= to && (f - from) % period < 3 {
                b |= mask;
            }
        }
        gb.bus.set_buttons(b);
        gb.run_frame();
        gb.bus.events.clear();
        let vb = gb.bus.irq_taken[0];
        stuck = if vb == last { stuck + 1 } else { 0 };
        stuck_max = stuck_max.max(stuck);
        last = vb;
        if f + 3000 >= frames && f % 100 == 0 {
            let mut c = gb.bus.ppu.frame.clone();
            c.sort_unstable();
            c.dedup();
            best = best.max(c.len());
        }
    }
    (stuck_max, best)
}

/// Each game: boot, sit through the intro, press Start at the title, then tap A (and Start now and
/// then) through Oak's speech and naming until the player stands in their room. Checks the game
/// never stops taking VBlank interrupts for more than 2 s and that the last picture is the
/// overworld, not a blank or frozen screen: some frame in the last 3000 uses at least four
/// colours. The end pictures were checked by eye (player's room in every game). Writes the last frame to PRET_OUT when set.
#[test]
fn pret_games_reach_the_overworld() {
    // (file, frames, script: from, to, buttons, every)
    let games: &[(&str, u32, &[(u32, u32, u8, u32)])] = &[
        ("pokeyellow.gbc", 9000, &[(1100, 1102, 0x08, 1), (1300, 9000, 0x01, 40), (1300, 9000, 0x08, 400)]),
        ("pokered.gbc", 9000, &[(200, 9000, 0x01, 40), (200, 9000, 0x08, 400)]),
        ("pokeblue.gbc", 9000, &[(200, 9000, 0x01, 40), (200, 9000, 0x08, 400)]),
        ("pokecrystal.gbc", 9000, &[(200, 9000, 0x01, 40), (200, 9000, 0x08, 400)]),
        ("pokegold.gbc", 12000, &[(200, 2980, 0x01, 40), (2990, 2992, 0x01, 1), (3100, 12000, 0x01, 60), (3100, 12000, 0x08, 600)]),
    ];
    let mut ran = 0;
    for &(file, frames, script) in games {
        let Some(mut gb) = load(file) else { continue };
        ran += 1;
        let (stuck, colours) = run_script(&mut gb, frames, script);
        if let Ok(out) = std::env::var("PRET_OUT") {
            std::fs::create_dir_all(&out).unwrap();
            save_png(&gb, &format!("{out}/{file}.end.png"));
        }
        eprintln!("{file}: longest VBlank gap {stuck} frames, up to {colours} colours late in the run");
        assert!(stuck < 120, "{file}: no VBlank for {stuck} frames");
        assert!(colours >= 4, "{file}: only {colours} colours late in the run");
    }
    eprintln!("{ran} pret games run");
}
