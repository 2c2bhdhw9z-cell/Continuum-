// pcs <rom> <frames> <from>: run with idle skip, print time per frame and where the CPU runs
use cboy_tv::gba::Gba;
use std::collections::HashMap;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let rom = std::fs::read(&a[1]).unwrap();
    let frames: u32 = a[2].parse().unwrap();
    let from: u32 = a.get(3).and_then(|s| s.parse().ok()).unwrap_or(frames / 2);
    let mut g = Gba::new(rom);
    g.idle_skip = std::env::var_os("NOSKIP").is_none();
    let mut hist: HashMap<u32, u64> = HashMap::new();
    let mut steps = 0u64;
    let t = std::time::Instant::now();
    let mut t_from = t;
    let skipped0 = 0u64;
    let mut sk_from = 0;
    for f in 0..frames {
        let p = f % 120;
        // KEYINPUT bits: A=0 B=1 Select=2 Start=3 Right=4 ...
        let keys = if p < 6 { 1 << 3 } else if (40..46).contains(&p) { 1 } else { 0 };
        g.set_keys(keys);
        if f == from { t_from = std::time::Instant::now(); sk_from = g.idle_skipped(); }
        if f >= from {
            g.bus.frame_ready = false;
            let start = g.bus.cycles;
            while !g.bus.frame_ready && g.bus.cycles - start < 280896 * 2 {
                *hist.entry(g.cpu.pc & !1).or_default() += 1;
                steps += 1;
                g.step();
            }
        } else {
            g.run_frame();
        }
    }
    let n = (frames - from) as f64;
    eprintln!("{:.2} ms a frame, {:.0} instructions a frame, idle skipped {:.0} cycles a frame (of 280896)",
        t_from.elapsed().as_secs_f64() / n * 1000.0, steps as f64 / n, (g.idle_skipped() - sk_from) as f64 / n);
    let _ = skipped0;
    let mut v: Vec<_> = hist.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1));
    for (pc, c) in v.iter().take(25) { println!("{pc:08X} {:5.1}%", *c as f64 * 100.0 / steps as f64); }
}
