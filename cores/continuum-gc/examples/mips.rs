//! How fast a program runs on each CPU: a DOL or ELF run for a few seconds by the plain
//! interpreter, the fast interpreter and (ARM64, or x86 with --features unicorn) the JIT, in
//! millions of emulated instructions a second, with the share of wall time spent drawing.
//!
//! cargo run --release --example mips -- program.elf [seconds] [plain,fast,jit]
//! (CGC_DRAW_THREADS=n draws on n helper threads, as the app does)
use continuum_gc::libretro::open;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let path = &a[1];
    let seconds: f64 = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(3.0);
    let modes = a.get(3).cloned().unwrap_or_else(|| "plain,fast,jit".into());
    for mode in modes.split(',') {
        let mut m = open(path).expect("boots");
        if let Some(n) = std::env::var("CGC_DRAW_THREADS").ok().and_then(|v| v.parse().ok()) {
            m.hw.gx.set_draw_threads(n);
        }
        match mode {
            "plain" => m.fast = None,
            "jit" if !m.enable_jit() => {
                println!("jit: no code memory here");
                continue;
            }
            _ => {}
        }
        let t = std::time::Instant::now();
        let start = m.cpu.cycles;
        let mut fields = 0u64;
        while t.elapsed().as_secs_f64() < seconds {
            m.run_field(m.cpu_hz / 60);
            fields += 1;
        }
        let s = t.elapsed().as_secs_f64();
        let draw = m.hw.gx.stats.draw_time.as_secs_f64();
        let wait = m.hw.gx.stats.wait_time.as_secs_f64();
        let skipped = m.fast.as_ref().map_or(0, |f| f.stats.idle_skips);
        println!(
            "{mode}: {:.0} million instructions a second, {fields} fields ({:.0}% of full speed), drawing {:.0}% of the time, waiting for pixels {:.0}%, idle rounds skipped {skipped}",
            (m.cpu.cycles - start) as f64 / s / 1e6,
            100.0 * fields as f64 / (s * 59.94),
            100.0 * draw / s,
            100.0 * wait / s
        );
    }
}
