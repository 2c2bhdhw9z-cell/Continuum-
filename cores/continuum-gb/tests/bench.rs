//! P8 performance guardrail: repeatable headless ms/frame for the four workloads the
//! programme watches. Each test warms up, times 120 frames, prints the number and asserts
//! only a loose 2x-target guardrail — the shared VM (1.5 CPUs) jitters, so the exact
//! numbers live in p8-report.md and the test just catches a large regression.
//!
//! Workloads:
//! - GB dmg-acid2, Exact drawer (target <= 2.5 ms/frame).
//! - CGB cgb-acid2 with double speed forced in the harness (`bus.double_speed = true`:
//!   no fetched test ROM both switches to double speed itself and runs a continuous
//!   busy workload — the AGE speed-switch ROMs are short tests that finish and idle —
//!   so the flag measures the emulator's 2x CPU-clock cost on a real CGB picture).
//! - GBA jsmolka hello.gba (target <= 2.0 ms/frame).
//! - GBA heavy.gba (Mode 2 affine BG2+BG3, alpha blend, 128 sprites; target <= 2.0).

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Instant;

use continuum_gb::gba::Gba;
use continuum_gb::gb::GameBoy;

/// Serialises the benchmarks: cargo test runs tests in parallel by default, and
/// four CPU-bound benches on a 1.5-CPU VM would each measure 2-3x slow. The lock
/// is held for the whole warm-up + measure of one test, so each number is taken
/// on an otherwise idle core (modulo other tenants of the shared VM).
static BENCH_LOCK: Mutex<()> = Mutex::new(());

const WARMUP: u32 = 30;
const FRAMES: u32 = 120;

fn rom(name: &str) -> Option<Vec<u8>> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(name);
    match std::fs::read(&p) {
        Ok(b) => Some(b),
        Err(_) => {
            eprintln!("SKIPPED: {} not found", p.display());
            None
        }
    }
}

fn report(name: &str, ms_per_frame: f64, target: f64) {
    let verdict = if ms_per_frame <= target { "within target" } else { "OVER TARGET" };
    println!("BENCH {}: {:.3} ms/frame (target <= {:.1}, {})", name, ms_per_frame, target, verdict);
    assert!(
        ms_per_frame <= target * 2.0,
        "{}: {:.3} ms/frame is over 2x the {:.1} target",
        name,
        ms_per_frame,
        target
    );
}

#[test]
fn bench_gb_dmg_acid2_exact() {
    let _guard = BENCH_LOCK.lock().unwrap();
    let Some(data) = rom("test-roms/dmg-acid2.gb") else { return };
    let mut gb = GameBoy::new(data).unwrap();
    gb.bus.ppu.exact = true;
    for _ in 0..WARMUP {
        gb.run_frame();
    }
    let t = Instant::now();
    for _ in 0..FRAMES {
        gb.run_frame();
    }
    let ms = t.elapsed().as_secs_f64() * 1000.0 / FRAMES as f64;
    assert!(gb.bus.ppu.frame.iter().any(|&p| p != 0), "dmg-acid2 rendered a blank frame");
    report("gb-dmg-acid2-exact", ms, 2.5);
}

#[test]
fn bench_gb_cgb_acid2_double_speed() {
    let _guard = BENCH_LOCK.lock().unwrap();
    let Some(data) = rom("test-roms/cgb-acid2.gbc") else { return };
    let mut gb = GameBoy::new(data).unwrap();
    gb.bus.double_speed = true;
    for _ in 0..WARMUP {
        gb.run_frame();
    }
    assert!(gb.bus.double_speed, "double-speed flag did not stick");
    let t = Instant::now();
    for _ in 0..FRAMES {
        gb.run_frame();
    }
    let ms = t.elapsed().as_secs_f64() * 1000.0 / FRAMES as f64;
    assert!(gb.bus.ppu.frame.iter().any(|&p| p != 0), "cgb-acid2 rendered a blank frame");
    report("gb-cgb-acid2-double-speed", ms, 2.5);
}

#[test]
fn bench_gba_hello() {
    let _guard = BENCH_LOCK.lock().unwrap();
    let Some(data) = rom("test-roms/jsmolka/ppu/hello.gba") else { return };
    let mut gba = Gba::new(data);
    for _ in 0..WARMUP {
        gba.run_frame();
    }
    let t = Instant::now();
    for _ in 0..FRAMES {
        gba.run_frame();
    }
    let ms = t.elapsed().as_secs_f64() * 1000.0 / FRAMES as f64;
    report("gba-hello", ms, 2.0);
}

#[test]
fn bench_gba_heavy() {
    let _guard = BENCH_LOCK.lock().unwrap();
    let Some(data) = rom("scripts/p8-bench-build/build/heavy.gba") else { return };
    let mut gba = Gba::new(data);
    for _ in 0..WARMUP {
        gba.run_frame();
    }
    // The heavy ROM must really be drawing Mode 2, not idling on a blank screen.
    assert_eq!(gba.bus.peek16(0x04000000) & 7, 2, "heavy.gba is not in Mode 2");
    let nz = gba.bus.ppu.frame.iter().filter(|&&p| p != 0x00FF_FFFF && p != 0).count();
    assert!(nz > 1000, "heavy.gba frame is blank ({} non-backdrop px)", nz);
    let t = Instant::now();
    for _ in 0..FRAMES {
        gba.run_frame();
    }
    let ms = t.elapsed().as_secs_f64() * 1000.0 / FRAMES as f64;
    report("gba-heavy-mode2-affine-blend", ms, 2.0);
}
